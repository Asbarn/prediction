//! Polymarket reconnection supervisor.
//!
//! Long-lived task that wraps PolymarketClient with exponential backoff
//! reconnection, following the DeribitSupervisor pattern.

use std::sync::Arc;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::{PolymarketAsset, PolymarketConfig};
use crate::feed::health::VenueHealth;
use crate::feed::polymarket::client::PolymarketClient;
use crate::feed::traits::RawMessage;
use crate::subscription::PolymarketSubscription;

/// Reconnection supervisor for Polymarket WebSocket feed.
///
/// Creates a fresh PolymarketClient per reconnection attempt.
/// No rate limiter needed for Polymarket (public read-only channel).
pub struct PolymarketSupervisor {
    config: PolymarketConfig,
    assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
}

impl PolymarketSupervisor {
    pub fn new(
        config: PolymarketConfig,
        assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,
        cancel: CancellationToken,
        health: Arc<VenueHealth>,
    ) -> Self {
        Self { config, assets_rx, cancel, health }
    }

    /// Run the reconnection loop, forwarding all messages to `tx`.
    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
        // Mark initial value as seen to prevent spurious startup reconnect.
        self.assets_rx.borrow_and_update();

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

            // Read latest asset list and inject into a cloned config.
            let subscriptions = self.assets_rx.borrow().clone();
            let mut config = self.config.clone();
            config.assets = subscriptions.into_iter().map(|s| PolymarketAsset {
                condition_id: s.condition_id,
                token_id: s.token_id,
            }).collect();

            attempt += 1;
            self.health.increment_connections();
            tracing::info!(attempt = attempt, assets = config.assets.len(), "PolymarketSupervisor connecting...");

            let client = PolymarketClient::new(config, self.cancel.clone());

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

                            result = self.assets_rx.changed() => {
                                match result {
                                    Ok(()) => {
                                        tracing::info!("PolymarketSupervisor: asset list updated, reconnecting");
                                        backoff.reset();
                                        break;
                                    }
                                    Err(_) => {
                                        tracing::warn!("PolymarketSupervisor: subscription channel closed, continuing with current assets");
                                    }
                                }
                            }

                            msg = raw_rx.recv() => {
                                match msg {
                                    Some(raw) => {
                                        if !received_first {
                                            received_first = true;
                                            backoff.reset();
                                            self.health.mark_available();
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
                                        self.health.mark_unavailable("connection lost".to_string());
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
                    self.health.mark_unavailable(format!("connection failed: {e}"));
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
