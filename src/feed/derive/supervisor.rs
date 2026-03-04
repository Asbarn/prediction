//! Derive reconnection supervisor.
//!
//! Long-lived task that wraps DeriveClient with exponential backoff
//! reconnection. On each connection attempt, creates a fresh DeriveClient,
//! forwards messages to the pipeline, and re-enters the backoff loop on
//! connection drop. Accepts dynamic instrument list updates via watch channel.

use std::sync::Arc;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::DeriveConfig;
use crate::feed::derive::client::DeriveClient;
use crate::feed::health::VenueHealth;
use crate::feed::reliability::VenueRateLimiter;
use crate::feed::traits::RawMessage;

/// Reconnection supervisor for Derive WebSocket feed.
///
/// Does NOT replace DeriveClient -- wraps it. The client stays as a
/// simple "connect once, read until done" component. The supervisor
/// handles the retry loop. Passes the rate limiter to each client
/// instance so outbound messages (subscribe) are rate-limited.
///
/// Key difference from DeribitSupervisor: no heartbeat-related logic.
/// Structurally identical otherwise.
pub struct DeriveSupervisor {
    config: DeriveConfig,
    instruments_rx: watch::Receiver<Vec<String>>,
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,
    health: Arc<VenueHealth>,
}

impl DeriveSupervisor {
    pub fn new(
        config: DeriveConfig,
        instruments_rx: watch::Receiver<Vec<String>>,
        cancel: CancellationToken,
        rate_limiter: VenueRateLimiter,
        health: Arc<VenueHealth>,
    ) -> Self {
        Self {
            config,
            instruments_rx,
            cancel,
            rate_limiter,
            health,
        }
    }

    /// Run the reconnection loop, forwarding all messages to `tx`.
    ///
    /// This method runs indefinitely until cancelled. It:
    /// 1. Creates a fresh DeriveClient
    /// 2. Calls start() to connect and subscribe
    /// 3. Forwards all messages from the client's receiver to `tx`
    /// 4. On disconnect, waits with exponential backoff, then retries
    /// 5. On instrument list change, drops current client and reconnects immediately
    ///
    /// Backoff resets only after successful connection AND first message
    /// received (per research: prevents burn-through when server accepts
    /// TCP but immediately closes WebSocket).
    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
        // Mark initial value as seen to prevent spurious startup reconnect.
        // Without this, changed() would fire immediately since the receiver
        // hasn't observed the initial value yet.
        self.instruments_rx.borrow_and_update();

        let reconnect = &self.config.reconnect;
        let mut backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(reconnect.initial_backoff_ms))
            .with_max_interval(Duration::from_millis(reconnect.max_backoff_ms))
            .with_randomization_factor(reconnect.randomization_factor)
            .with_multiplier(2.0)
            .with_max_elapsed_time(None) // Never give up
            .build();

        let mut attempt: u64 = 0;

        loop {
            if self.cancel.is_cancelled() {
                tracing::info!(venue = "derive", "DeriveSupervisor cancelled, exiting");
                break;
            }

            // Read latest instrument list at the top of each reconnect iteration.
            let instruments = self.instruments_rx.borrow().clone();

            // If instruments list is empty, nothing to subscribe to -- wait and retry.
            if instruments.is_empty() {
                tracing::info!(venue = "derive", "DeriveSupervisor: no instruments to subscribe to, waiting...");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                    _ = self.cancel.cancelled() => break,
                    result = self.instruments_rx.changed() => {
                        if result.is_ok() {
                            tracing::info!(venue = "derive", "DeriveSupervisor: instrument list updated while waiting");
                        }
                        continue;
                    }
                }
            }

            attempt += 1;
            self.health.increment_connections();
            tracing::info!(
                venue = "derive",
                attempt = attempt,
                instruments = instruments.len(),
                "DeriveSupervisor connecting..."
            );

            // Create a fresh client for each attempt, passing the rate limiter.
            let client = DeriveClient::new(
                self.config.clone(),
                instruments,
                self.cancel.child_token(),
                Some(self.rate_limiter.clone()),
            );

            match client.start().await {
                Ok(mut raw_rx) => {
                    tracing::info!(
                        venue = "derive",
                        attempt = attempt,
                        "DeriveSupervisor connected, forwarding messages"
                    );
                    let mut received_first = false;

                    // Forward messages until the client's channel closes
                    loop {
                        tokio::select! {
                            biased;

                            _ = self.cancel.cancelled() => {
                                tracing::info!(venue = "derive", "DeriveSupervisor cancelled during forwarding");
                                return;
                            }

                            result = self.instruments_rx.changed() => {
                                match result {
                                    Ok(()) => {
                                        tracing::info!(venue = "derive", "DeriveSupervisor: instrument list updated, reconnecting");
                                        backoff.reset(); // Intentional reconnect, not a failure
                                        break; // -> outer loop re-enters, reads updated list
                                    }
                                    Err(_) => {
                                        tracing::warn!(venue = "derive", "DeriveSupervisor: subscription channel closed, continuing with current instruments");
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
                                                venue = "derive",
                                                "DeriveSupervisor: first message received, backoff reset"
                                            );
                                        }
                                        if tx.send(raw).await.is_err() {
                                            tracing::warn!(
                                                venue = "derive",
                                                "DeriveSupervisor: downstream receiver dropped, exiting"
                                            );
                                            return;
                                        }
                                    }
                                    None => {
                                        // Client channel closed = connection lost
                                        self.health.mark_unavailable("connection lost".to_string());
                                        tracing::warn!(
                                            venue = "derive",
                                            attempt = attempt,
                                            "DeriveSupervisor: connection lost, will reconnect"
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
                        venue = "derive",
                        attempt = attempt,
                        error = %e,
                        "DeriveSupervisor: connection attempt failed"
                    );
                }
            }

            // Apply backoff before retry
            match backoff.next_backoff() {
                Some(delay) => {
                    let delay_ms = delay.as_millis() as u64;
                    tracing::info!(
                        venue = "derive",
                        delay_ms = delay_ms,
                        "DeriveSupervisor: waiting before reconnect"
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.cancel.cancelled() => {
                            tracing::info!(venue = "derive", "DeriveSupervisor cancelled during backoff");
                            break;
                        }
                    }
                }
                None => {
                    // max_elapsed_time reached (shouldn't happen with None)
                    tracing::error!(venue = "derive", "DeriveSupervisor: backoff exhausted (unexpected)");
                    break;
                }
            }
        }
    }
}
