//! Polymarket reconnection supervisor.
//!
//! Long-lived task that wraps PolymarketClient with exponential backoff
//! reconnection, following the DeribitSupervisor pattern.

use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::PolymarketConfig;
use crate::feed::polymarket::client::PolymarketClient;
use crate::feed::traits::RawMessage;

/// Reconnection supervisor for Polymarket WebSocket feed.
///
/// Creates a fresh PolymarketClient per reconnection attempt.
/// No rate limiter needed for Polymarket (public read-only channel).
pub struct PolymarketSupervisor {
    config: PolymarketConfig,
    cancel: CancellationToken,
}

impl PolymarketSupervisor {
    pub fn new(config: PolymarketConfig, cancel: CancellationToken) -> Self {
        Self { config, cancel }
    }

    /// Run the reconnection loop, forwarding all messages to `tx`.
    pub async fn run(self, tx: mpsc::Sender<RawMessage>) {
        let reconnect = &self.config.reconnect;
        let mut backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(reconnect.initial_backoff_ms))
            .with_max_interval(Duration::from_millis(reconnect.max_backoff_ms))
            .with_randomization_factor(reconnect.randomization_factor)
            .with_multiplier(2.0)
            .with_max_elapsed_time(None)
            .build();

        let mut attempt: u64 = 0;

        loop {
            if self.cancel.is_cancelled() {
                tracing::info!("PolymarketSupervisor cancelled, exiting");
                break;
            }

            attempt += 1;
            tracing::info!(attempt = attempt, "PolymarketSupervisor connecting...");

            let client = PolymarketClient::new(self.config.clone(), self.cancel.clone());

            match client.start().await {
                Ok(mut raw_rx) => {
                    tracing::info!(
                        attempt = attempt,
                        "PolymarketSupervisor connected, forwarding messages"
                    );
                    let mut received_first = false;

                    loop {
                        tokio::select! {
                            biased;

                            _ = self.cancel.cancelled() => {
                                tracing::info!("PolymarketSupervisor cancelled during forwarding");
                                return;
                            }

                            msg = raw_rx.recv() => {
                                match msg {
                                    Some(raw) => {
                                        if !received_first {
                                            received_first = true;
                                            backoff.reset();
                                            tracing::info!(
                                                "PolymarketSupervisor: first message received, backoff reset"
                                            );
                                        }
                                        if tx.send(raw).await.is_err() {
                                            tracing::warn!(
                                                "PolymarketSupervisor: downstream receiver dropped, exiting"
                                            );
                                            return;
                                        }
                                    }
                                    None => {
                                        tracing::warn!(
                                            attempt = attempt,
                                            "PolymarketSupervisor: connection lost, will reconnect"
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        attempt = attempt,
                        error = %e,
                        "PolymarketSupervisor: connection attempt failed"
                    );
                }
            }

            // Apply backoff before retry
            match backoff.next_backoff() {
                Some(delay) => {
                    let delay_ms = delay.as_millis() as u64;
                    tracing::info!(
                        delay_ms = delay_ms,
                        "PolymarketSupervisor: waiting before reconnect"
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.cancel.cancelled() => {
                            tracing::info!("PolymarketSupervisor cancelled during backoff");
                            break;
                        }
                    }
                }
                None => {
                    tracing::error!("PolymarketSupervisor: backoff exhausted (unexpected)");
                    break;
                }
            }
        }
    }
}
