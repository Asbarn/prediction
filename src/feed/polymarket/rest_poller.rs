//! Polymarket REST polling client for /midpoint endpoint.
//!
//! Fetches midpoint prices via REST as a fallback when WebSocket is unavailable.
//! Produces `MarketSnapshot` values on an mpsc channel, identical in shape to
//! WS-sourced snapshots but with empty depth (REST has no order book depth).
//!
//! IMPORTANT: Uses /midpoint only -- NOT /book (returns stale ghost data per GitHub #180).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::config::PolymarketConfig;
use crate::feed::health::VenueHealth;
use crate::feed::reliability::rate_limiter::VenueRateLimiter;
use crate::subscription::PolymarketSubscription;
use crate::types::{
    DualTimestamp, InstrumentId, MarketSnapshot, Price, Probability, TraceId, Venue,
};

/// Response shape from Polymarket `/midpoint` endpoint.
#[derive(Debug, serde::Deserialize)]
struct MidpointResponse {
    mid: String,
}

/// Polymarket REST polling client.
///
/// Periodically fetches midpoint prices for all subscribed assets and produces
/// `MarketSnapshot` values. Designed to run as a standalone tokio task alongside
/// (but not replacing) the WS supervisor -- the source coordinator (Plan 02)
/// decides which feed is active.
pub struct PolymarketRestPoller {
    config: PolymarketConfig,
    client: reqwest::Client,
    rate_limiter: VenueRateLimiter,
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
    sequence: AtomicU64,
}

impl PolymarketRestPoller {
    /// Create a new REST poller.
    pub fn new(
        config: PolymarketConfig,
        client: reqwest::Client,
        rate_limiter: VenueRateLimiter,
        cancel: CancellationToken,
        health: Arc<VenueHealth>,
    ) -> Self {
        Self {
            config,
            client,
            rate_limiter,
            cancel,
            health,
            sequence: AtomicU64::new(1),
        }
    }

    /// Fetch the midpoint price for a single token.
    ///
    /// Rate-limits before issuing the HTTP request. Returns the parsed
    /// midpoint as a `Decimal`.
    async fn fetch_midpoint(&self, token_id: &str) -> Result<Decimal, Box<dyn std::error::Error + Send + Sync>> {
        self.rate_limiter.wait().await;

        let url = format!(
            "{}/midpoint?token_id={}",
            self.config.rest_url, token_id
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?;

        let body: MidpointResponse = resp.json().await?;
        let mid: Decimal = body.mid.parse()?;
        Ok(mid)
    }

    /// Run the polling loop, producing `MarketSnapshot` values on `tx`.
    ///
    /// Reads the current asset list from the `assets` watch channel each tick.
    /// Polls every `rest_poll_interval_secs` seconds. Exits when the
    /// cancellation token is triggered or the downstream receiver drops.
    pub async fn run(
        self,
        assets: watch::Receiver<Vec<PolymarketSubscription>>,
        tx: mpsc::Sender<MarketSnapshot>,
    ) {
        tracing::info!(
            poll_interval_secs = self.config.rest_poll_interval_secs,
            "PolymarketRestPoller starting"
        );

        let poll_interval = Duration::from_secs(self.config.rest_poll_interval_secs);
        let mut first_success = false;

        loop {
            // Wait for next tick or cancellation
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    tracing::info!("PolymarketRestPoller cancelled, exiting");
                    return;
                }
                _ = tokio::time::sleep(poll_interval) => {}
            }

            // Read current subscriptions
            let subscriptions = assets.borrow().clone();
            if subscriptions.is_empty() {
                tracing::debug!("PolymarketRestPoller: no assets subscribed, skipping poll");
                continue;
            }

            for sub in &subscriptions {
                if self.cancel.is_cancelled() {
                    return;
                }

                let poll_start = Instant::now();

                match self.fetch_midpoint(&sub.token_id).await {
                    Ok(midpoint) => {
                        let poll_duration_ms = poll_start.elapsed().as_millis() as f64;
                        metrics::histogram!(
                            "feed_rest_poll_duration_ms",
                            "venue" => "polymarket"
                        )
                        .record(poll_duration_ms);

                        metrics::counter!(
                            "feed_rest_polls_total",
                            "venue" => "polymarket",
                            "status" => "success"
                        )
                        .increment(1);

                        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
                        let snapshot = MarketSnapshot {
                            venue: Venue::Polymarket,
                            instrument_id: InstrumentId::new(&sub.token_id),
                            event_id: None,
                            bid: Some(Price::new(midpoint)),
                            ask: Some(Price::new(midpoint)),
                            bid_size: None,
                            ask_size: None,
                            depth_bids: vec![],
                            depth_asks: vec![],
                            bid_probability: Probability::new(midpoint).ok(),
                            ask_probability: Probability::new(midpoint).ok(),
                            last_price: None,
                            mark_price: None,
                            index_price: None,
                            mark_iv: None,
                            open_interest: None,
                            volume_24h: None,
                            greeks: None,
                            bid_iv: None,
                            ask_iv: None,
                            underlying_price: None,
                            underlying_index: None,
                            exchange_timestamp: None,
                            timestamp: DualTimestamp::now(),
                            sequence: seq,
                            trace_id: TraceId::new(),
                            is_stale: false,
                        };

                        if tx.send(snapshot).await.is_err() {
                            tracing::warn!(
                                "PolymarketRestPoller: downstream receiver dropped, exiting"
                            );
                            return;
                        }

                        if !first_success {
                            first_success = true;
                            self.health.mark_available();
                            tracing::info!(
                                "PolymarketRestPoller: first successful poll, marked available"
                            );
                        }
                    }
                    Err(e) => {
                        let poll_duration_ms = poll_start.elapsed().as_millis() as f64;
                        metrics::histogram!(
                            "feed_rest_poll_duration_ms",
                            "venue" => "polymarket"
                        )
                        .record(poll_duration_ms);

                        metrics::counter!(
                            "feed_rest_polls_total",
                            "venue" => "polymarket",
                            "status" => "error"
                        )
                        .increment(1);

                        tracing::warn!(
                            token_id = %sub.token_id,
                            error = %e,
                            "PolymarketRestPoller: failed to fetch midpoint"
                        );
                    }
                }
            }
        }
    }
}
