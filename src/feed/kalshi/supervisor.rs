//! Kalshi reconnection supervisor.
//!
//! Long-lived task that wraps KalshiClient with exponential backoff
//! reconnection. Creates fresh auth signatures on each attempt since
//! the timestamp is part of the signing message.

use std::sync::Arc;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use rsa::RsaPrivateKey;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::KalshiConfig;
use crate::feed::health::VenueHealth;
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
    tickers_rx: watch::Receiver<Vec<String>>,
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
}

impl KalshiSupervisor {
    pub fn new(
        config: KalshiConfig,
        api_key_id: String,
        private_key: RsaPrivateKey,
        tickers_rx: watch::Receiver<Vec<String>>,
        cancel: CancellationToken,
        health: Arc<VenueHealth>,
    ) -> Self {
        Self {
            config,
            api_key_id,
            private_key,
            tickers_rx,
            cancel,
            health,
        }
    }

    /// Run the reconnection loop, forwarding all messages to `tx`.
    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
        // Mark initial value as seen to prevent spurious startup reconnect.
        self.tickers_rx.borrow_and_update();

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

            // Read latest tickers and inject into a cloned config.
            let tickers = self.tickers_rx.borrow().clone();
            let mut config = self.config.clone();
            config.market_tickers = tickers;

            attempt += 1;
            self.health.increment_connections();
            tracing::info!(attempt = attempt, tickers = config.market_tickers.len(), "KalshiSupervisor connecting...");

            // Fresh client per attempt = fresh auth signature
            let client = KalshiClient::new(
                config,
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

                            result = self.tickers_rx.changed() => {
                                match result {
                                    Ok(()) => {
                                        tracing::info!("KalshiSupervisor: ticker list updated, reconnecting");
                                        backoff.reset();
                                        break;
                                    }
                                    Err(_) => {
                                        tracing::warn!("KalshiSupervisor: subscription channel closed, continuing with current tickers");
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
                                        self.health.mark_unavailable("connection lost".to_string());
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
                    self.health.mark_unavailable(format!("connection failed: {e}"));
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
