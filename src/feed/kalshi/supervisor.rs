//! Kalshi reconnection supervisor.
//!
//! Long-lived task that wraps KalshiClient with exponential backoff
//! reconnection. Creates fresh auth signatures on each attempt since
//! the timestamp is part of the signing message.

use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use rsa::RsaPrivateKey;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::KalshiConfig;
use crate::feed::kalshi::client::KalshiClient;
use crate::feed::traits::RawMessage;

/// Reconnection supervisor for Kalshi WebSocket feed.
///
/// Creates a fresh KalshiClient (with fresh auth signature) per
/// reconnection attempt. Uses exponential backoff with jitter.
pub struct KalshiSupervisor {
    config: KalshiConfig,
    api_key_id: String,
    private_key: RsaPrivateKey,
    cancel: CancellationToken,
}

impl KalshiSupervisor {
    pub fn new(
        config: KalshiConfig,
        api_key_id: String,
        private_key: RsaPrivateKey,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            config,
            api_key_id,
            private_key,
            cancel,
        }
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
                tracing::info!("KalshiSupervisor cancelled, exiting");
                break;
            }

            attempt += 1;
            tracing::info!(attempt = attempt, "KalshiSupervisor connecting...");

            // Fresh client per attempt = fresh auth signature
            let client = KalshiClient::new(
                self.config.clone(),
                self.api_key_id.clone(),
                self.private_key.clone(),
                self.cancel.clone(),
            );

            match client.start().await {
                Ok(mut raw_rx) => {
                    tracing::info!(
                        attempt = attempt,
                        "KalshiSupervisor connected, forwarding messages"
                    );
                    let mut received_first = false;

                    loop {
                        tokio::select! {
                            biased;

                            _ = self.cancel.cancelled() => {
                                tracing::info!("KalshiSupervisor cancelled during forwarding");
                                return;
                            }

                            msg = raw_rx.recv() => {
                                match msg {
                                    Some(raw) => {
                                        if !received_first {
                                            received_first = true;
                                            backoff.reset();
                                            tracing::info!(
                                                "KalshiSupervisor: first message received, backoff reset"
                                            );
                                        }
                                        if tx.send(raw).await.is_err() {
                                            tracing::warn!(
                                                "KalshiSupervisor: downstream receiver dropped, exiting"
                                            );
                                            return;
                                        }
                                    }
                                    None => {
                                        tracing::warn!(
                                            attempt = attempt,
                                            "KalshiSupervisor: connection lost, will reconnect"
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
                        "KalshiSupervisor: connection attempt failed"
                    );
                }
            }

            // Apply backoff before retry
            match backoff.next_backoff() {
                Some(delay) => {
                    let delay_ms = delay.as_millis() as u64;
                    tracing::info!(
                        delay_ms = delay_ms,
                        "KalshiSupervisor: waiting before reconnect"
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.cancel.cancelled() => {
                            tracing::info!("KalshiSupervisor cancelled during backoff");
                            break;
                        }
                    }
                }
                None => {
                    tracing::error!("KalshiSupervisor: backoff exhausted (unexpected)");
                    break;
                }
            }
        }
    }
}
