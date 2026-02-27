//! Deribit reconnection supervisor.
//!
//! Long-lived task that wraps DeribitClient with exponential backoff
//! reconnection. On each connection attempt, creates a fresh DeribitClient,
//! forwards messages to the pipeline, and re-enters the backoff loop on
//! connection drop.

use std::sync::Arc;
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoffBuilder;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::DeribitConfig;
use crate::feed::deribit::client::DeribitClient;
use crate::feed::health::VenueHealth;
use crate::feed::reliability::VenueRateLimiter;
use crate::feed::traits::RawMessage;

/// Reconnection supervisor for Deribit WebSocket feed.
///
/// Does NOT replace DeribitClient -- wraps it. The client stays as a
/// simple "connect once, read until done" component. The supervisor
/// handles the retry loop. Passes the rate limiter to each client
/// instance so outbound messages (subscribe, set_heartbeat) are
/// rate-limited.
pub struct DeribitSupervisor {
    config: DeribitConfig,
    instruments_rx: watch::Receiver<Vec<String>>,
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,
    health: Arc<VenueHealth>,
}

impl DeribitSupervisor {
    pub fn new(
        config: DeribitConfig,
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
    /// 1. Creates a fresh DeribitClient
    /// 2. Calls start() to connect and subscribe
    /// 3. Forwards all messages from the client's receiver to `tx`
    /// 4. On disconnect, waits with exponential backoff, then retries
    ///
    /// Backoff resets only after successful connection AND first message
    /// received (per research: prevents burn-through when server accepts
    /// TCP but immediately closes WebSocket).
    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
        // Mark initial value as seen to prevent spurious startup reconnect.
        // Without this, changed() would fire immediately since the receiver
        // hasn't observed the initial value yet (Pitfall 1 from research).
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
                tracing::info!("DeribitSupervisor cancelled, exiting");
                break;
            }

            // Read latest instrument list at the top of each reconnect iteration.
            // borrow().clone() drops the Ref immediately before any .await point.
            let instruments = self.instruments_rx.borrow().clone();

            attempt += 1;
            self.health.increment_connections();
            tracing::info!(attempt = attempt, instruments = instruments.len(), "DeribitSupervisor connecting...");

            // Create a fresh client for each attempt, passing the rate limiter
            let client = DeribitClient::new(
                self.config.clone(),
                instruments,
                self.cancel.clone(),
            )
            .with_rate_limiter(self.rate_limiter.clone());

            match client.start().await {
                Ok(mut raw_rx) => {
                    tracing::info!(
                        attempt = attempt,
                        "DeribitSupervisor connected, forwarding messages"
                    );
                    let mut received_first = false;

                    // Forward messages until the client's channel closes
                    loop {
                        tokio::select! {
                            biased;

                            _ = self.cancel.cancelled() => {
                                tracing::info!("DeribitSupervisor cancelled during forwarding");
                                return;
                            }

                            result = self.instruments_rx.changed() => {
                                match result {
                                    Ok(()) => {
                                        tracing::info!("DeribitSupervisor: instrument list updated, reconnecting");
                                        backoff.reset(); // Intentional reconnect, not a failure
                                        break; // -> outer loop re-enters, reads updated list
                                    }
                                    Err(_) => {
                                        tracing::warn!("DeribitSupervisor: subscription channel closed, continuing with current instruments");
                                        // Channel closed (SubscriptionManager dropped). Continue operating
                                        // with current instruments. changed() will keep returning Err,
                                        // but biased select checks cancel first, so no busy loop.
                                    }
                                }
                            }

                            msg = raw_rx.recv() => {
                                match msg {
                                    Some(raw) => {
                                        if !received_first {
                                            received_first = true;
                                            // Reset backoff on first message
                                            // (confirms connection is actually working)
                                            backoff.reset();
                                            self.health.mark_available();
                                            tracing::info!(
                                                "DeribitSupervisor: first message received, backoff reset"
                                            );
                                        }
                                        if tx.send(raw).await.is_err() {
                                            tracing::warn!(
                                                "DeribitSupervisor: downstream receiver dropped, exiting"
                                            );
                                            return;
                                        }
                                    }
                                    None => {
                                        // Client channel closed = connection lost
                                        self.health.mark_unavailable("connection lost".to_string());
                                        tracing::warn!(
                                            attempt = attempt,
                                            "DeribitSupervisor: connection lost, will reconnect"
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
                        "DeribitSupervisor: connection attempt failed"
                    );
                }
            }

            // Apply backoff before retry
            match backoff.next_backoff() {
                Some(delay) => {
                    let delay_ms = delay.as_millis() as u64;
                    tracing::info!(
                        delay_ms = delay_ms,
                        "DeribitSupervisor: waiting before reconnect"
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = self.cancel.cancelled() => {
                            tracing::info!("DeribitSupervisor cancelled during backoff");
                            break;
                        }
                    }
                }
                None => {
                    // max_elapsed_time reached (shouldn't happen with None)
                    tracing::error!("DeribitSupervisor: backoff exhausted (unexpected)");
                    break;
                }
            }
        }
    }
}
