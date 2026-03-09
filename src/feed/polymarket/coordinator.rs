//! Polymarket source coordinator: exclusive WS/REST mode switching.
//!
//! Manages a state machine that runs exactly one data source at a time
//! (WebSocket or REST polling). Switches to REST when WS becomes unavailable
//! (data timeout or connection loss), and probes WS recovery periodically
//! to switch back when sustained messages confirm stability.
//!
//! Design invariant: NEVER run WS and REST simultaneously on the same
//! snapshot channel. Cancel the old source FIRST, then start the new one.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::{PolymarketAsset, PolymarketConfig};
use crate::feed::health::VenueHealth;
use crate::feed::polymarket::client::PolymarketClient;
use crate::feed::polymarket::normalize::PolymarketProcessor;
use crate::feed::polymarket::rest_poller::PolymarketRestPoller;
use crate::feed::polymarket::supervisor::PolymarketSupervisor;
use crate::feed::reliability::rate_limiter::VenueRateLimiter;
use crate::feed::traits::{RawMessage, RecordLine};
use crate::subscription::PolymarketSubscription;
use crate::types::MarketSnapshot;

/// Current data source mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceMode {
    /// WebSocket feed via PolymarketSupervisor + PolymarketProcessor.
    WebSocket,
    /// REST polling via PolymarketRestPoller.
    Rest,
}

/// Source coordinator for Polymarket data feed.
///
/// Wraps both the WS supervisor and REST poller, ensuring exactly one is
/// active at any time. Monitors health and switches modes automatically.
pub struct SourceCoordinator {
    config: PolymarketConfig,
    assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
    rate_limiter: VenueRateLimiter,
    http_client: reqwest::Client,
    recording_tx: Option<mpsc::Sender<RecordLine>>,
}

impl SourceCoordinator {
    /// Create a new source coordinator.
    pub fn new(
        config: PolymarketConfig,
        assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,
        cancel: CancellationToken,
        health: Arc<VenueHealth>,
        rate_limiter: VenueRateLimiter,
        http_client: reqwest::Client,
        recording_tx: Option<mpsc::Sender<RecordLine>>,
    ) -> Self {
        Self {
            config,
            assets_rx,
            cancel,
            health,
            rate_limiter,
            http_client,
            recording_tx,
        }
    }

    /// Run the coordinator state machine loop.
    ///
    /// Starts in WebSocket mode. Switches to REST when WS becomes unavailable,
    /// and probes WS recovery periodically to switch back.
    ///
    /// Produces `MarketSnapshot` values on `snapshot_tx` from whichever source
    /// is currently active.
    pub async fn run(mut self, snapshot_tx: mpsc::Sender<MarketSnapshot>) {
        let mut current_mode = SourceMode::WebSocket;

        // Emit initial mode metric
        metrics::gauge!(
            "feed_source_mode",
            "venue" => "polymarket"
        )
        .set(0.0); // 0 = WS

        tracing::info!("SourceCoordinator starting in WebSocket mode");

        loop {
            if self.cancel.is_cancelled() {
                tracing::info!("SourceCoordinator cancelled, exiting");
                break;
            }

            match current_mode {
                SourceMode::WebSocket => {
                    let next = self.run_ws_mode(&snapshot_tx).await;
                    match next {
                        Some(mode) => {
                            current_mode = mode;
                        }
                        None => break, // cancelled or snapshot_tx dropped
                    }
                }
                SourceMode::Rest => {
                    let next = self.run_rest_mode(&snapshot_tx).await;
                    match next {
                        Some(mode) => {
                            current_mode = mode;
                        }
                        None => break, // cancelled or snapshot_tx dropped
                    }
                }
            }
        }

        tracing::info!("SourceCoordinator exiting");
    }

    /// Run in WebSocket mode. Returns the next mode to switch to, or None to exit.
    async fn run_ws_mode(
        &mut self,
        snapshot_tx: &mpsc::Sender<MarketSnapshot>,
    ) -> Option<SourceMode> {
        tracing::info!("SourceCoordinator: entering WebSocket mode");

        // Create child cancellation token for this WS session
        let child_cancel = self.cancel.child_token();

        // Create internal channels for supervisor -> processor -> snapshot
        let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);

        // Spawn WS supervisor
        let supervisor = PolymarketSupervisor::new(
            self.config.clone(),
            self.assets_rx.clone(),
            child_cancel.clone(),
            self.health.clone(),
        );
        tokio::spawn(supervisor.run(supervisor_tx));

        // Spawn processor
        let (processor, mut venue_snapshot_rx) = PolymarketProcessor::new(
            supervisor_rx,
            self.recording_tx.clone(),
            child_cancel.clone(),
            self.config.staleness_threshold_ms,
        );
        tokio::spawn(processor.run());

        // Forward snapshots from processor to snapshot_tx, while monitoring health
        let result = loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    child_cancel.cancel();
                    break None;
                }

                snapshot = venue_snapshot_rx.recv() => {
                    match snapshot {
                        Some(snap) => {
                            if snapshot_tx.send(snap).await.is_err() {
                                tracing::warn!(
                                    "SourceCoordinator: downstream receiver dropped"
                                );
                                child_cancel.cancel();
                                break None;
                            }
                        }
                        None => {
                            // Processor channel closed -- check if health says unavailable
                            if !self.health.is_available() {
                                tracing::info!(
                                    "SourceCoordinator: WS processor exited, health unavailable, switching to REST"
                                );
                                break Some(SourceMode::Rest);
                            }
                            // Processor exited but health still OK -- unusual, treat as exit
                            tracing::warn!(
                                "SourceCoordinator: WS processor channel closed unexpectedly"
                            );
                            break Some(SourceMode::Rest);
                        }
                    }
                }

                // Poll health state periodically to detect supervisor marking unavailable
                // (e.g., data timeout fires, connection lost)
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if !self.health.is_available() && self.health.connection_count() > 0 {
                        // Health went unavailable (supervisor detected timeout/loss)
                        // Wait a moment for the supervisor to potentially recover on its own
                        // If health stays unavailable for a few seconds, switch to REST
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        if !self.health.is_available() && !self.cancel.is_cancelled() {
                            tracing::info!(
                                "SourceCoordinator: WS health unavailable, switching to REST"
                            );
                            break Some(SourceMode::Rest);
                        }
                    }
                }
            }
        };

        // Cancel the child token to stop supervisor + processor
        child_cancel.cancel();

        if result == Some(SourceMode::Rest) {
            // Emit switch metrics
            metrics::gauge!(
                "feed_source_mode",
                "venue" => "polymarket"
            )
            .set(1.0); // 1 = REST

            metrics::counter!(
                "feed_source_switches_total",
                "venue" => "polymarket",
                "from" => "ws",
                "to" => "rest"
            )
            .increment(1);

            tracing::info!("Polymarket: switching from WebSocket to REST polling");
        }

        result
    }

    /// Run in REST mode. Returns the next mode to switch to, or None to exit.
    async fn run_rest_mode(
        &mut self,
        snapshot_tx: &mpsc::Sender<MarketSnapshot>,
    ) -> Option<SourceMode> {
        tracing::info!("SourceCoordinator: entering REST mode");

        // Create child cancellation token for REST poller
        let rest_cancel = self.cancel.child_token();

        // Spawn REST poller -- it sends directly to snapshot_tx
        let poller = PolymarketRestPoller::new(
            self.config.clone(),
            self.http_client.clone(),
            self.rate_limiter.clone(),
            rest_cancel.clone(),
            self.health.clone(),
        );
        tokio::spawn(poller.run(self.assets_rx.clone(), snapshot_tx.clone()));

        // Periodically probe WS recovery
        let recovery_interval = Duration::from_secs(self.config.ws_recovery_check_secs);
        let data_timeout = Duration::from_secs(self.config.data_timeout_secs);
        let recovery_threshold = self.config.ws_recovery_threshold;

        loop {
            // Wait for recovery check interval or cancellation
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    rest_cancel.cancel();
                    return None;
                }
                _ = tokio::time::sleep(recovery_interval) => {}
            }

            if self.cancel.is_cancelled() {
                rest_cancel.cancel();
                return None;
            }

            // Attempt WS probe with a SEPARATE temporary channel
            tracing::info!(
                "SourceCoordinator: probing WS recovery (need {} messages within {}s)",
                recovery_threshold,
                self.config.data_timeout_secs
            );

            match self.probe_ws_recovery(data_timeout, recovery_threshold).await {
                true => {
                    tracing::info!(
                        "SourceCoordinator: WS probe successful, switching back to WebSocket"
                    );

                    // Cancel REST poller FIRST (exclusive mode guarantee)
                    rest_cancel.cancel();

                    // Emit switch metrics
                    metrics::gauge!(
                        "feed_source_mode",
                        "venue" => "polymarket"
                    )
                    .set(0.0); // 0 = WS

                    metrics::counter!(
                        "feed_source_switches_total",
                        "venue" => "polymarket",
                        "from" => "rest",
                        "to" => "ws"
                    )
                    .increment(1);

                    tracing::info!(
                        "Polymarket: switching from REST to WebSocket (recovery confirmed)"
                    );

                    return Some(SourceMode::WebSocket);
                }
                false => {
                    tracing::info!(
                        "SourceCoordinator: WS probe failed, staying in REST mode"
                    );
                    // Continue loop -- try again after next recovery interval
                }
            }
        }
    }

    /// Probe WS connectivity by creating a temporary client and checking for messages.
    ///
    /// Uses a SEPARATE temporary channel -- does NOT send to snapshot_tx.
    /// Returns true if at least `threshold` messages are received within `timeout`.
    async fn probe_ws_recovery(&self, timeout: Duration, threshold: u32) -> bool {
        let probe_cancel = CancellationToken::new();

        // Build a config with current subscriptions for the probe
        let subscriptions = self.assets_rx.borrow().clone();
        let mut probe_config = self.config.clone();
        probe_config.assets = subscriptions
            .into_iter()
            .map(|s| PolymarketAsset {
                condition_id: s.condition_id,
                token_id: s.token_id,
            })
            .collect();

        if probe_config.assets.is_empty() {
            tracing::debug!("SourceCoordinator: no assets for WS probe, skipping");
            return false;
        }

        let client = PolymarketClient::new(probe_config, probe_cancel.clone());

        let raw_rx = match client.start().await {
            Ok(rx) => rx,
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "SourceCoordinator: WS probe connection failed"
                );
                return false;
            }
        };

        let result = Self::count_messages(raw_rx, timeout, threshold, probe_cancel.clone()).await;

        // Always clean up the probe connection
        probe_cancel.cancel();

        result
    }

    /// Count messages received on a channel, returning true if threshold is met within timeout.
    async fn count_messages(
        mut rx: mpsc::Receiver<RawMessage>,
        timeout: Duration,
        threshold: u32,
        cancel: CancellationToken,
    ) -> bool {
        let mut count: u32 = 0;
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    return false;
                }

                _ = tokio::time::sleep_until(deadline) => {
                    tracing::debug!(
                        received = count,
                        threshold = threshold,
                        "SourceCoordinator: WS probe timed out"
                    );
                    return count >= threshold;
                }

                msg = rx.recv() => {
                    match msg {
                        Some(_) => {
                            count += 1;
                            if count >= threshold {
                                tracing::debug!(
                                    count = count,
                                    "SourceCoordinator: WS probe reached threshold"
                                );
                                return true;
                            }
                        }
                        None => {
                            tracing::debug!(
                                received = count,
                                "SourceCoordinator: WS probe channel closed"
                            );
                            return false;
                        }
                    }
                }
            }
        }
    }
}
