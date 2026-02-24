//! Core spread computation engine.
//!
//! Consumes `MarketSnapshot` events from the fan-in channel, pairs snapshots
//! by event ID via `EventRegistry`, enforces staleness gates, computes all 4
//! spread patterns with full cost model (walk-the-book, fees, carry), logs
//! every computation to JSONL, maintains rolling statistics per event, evaluates
//! dynamic thresholds, and emits periodic aggregate statistics.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::events::registry::EventRegistry;
use crate::events::risk::BasisRiskCache;
use crate::spread::book_walker::{walk_the_book, WalkResult};
use crate::spread::config::SpreadConfig;
use crate::spread::cost_model::{carry_cost, kalshi_taker_fee, polymarket_fee};
use crate::spread::logger::SpreadLogger;
use crate::spread::patterns::{compute_gross_spread, SpreadPattern, SpreadResult};
use crate::spread::rolling_stats::RollingStats;
use crate::spread::threshold::compute_threshold;
use crate::types::{MarketSnapshot, Venue};

/// Stateful spread computation context.
///
/// Pairs incoming market snapshots by event ID, enforces staleness gates,
/// computes all 4 directional spread patterns with the full cost model,
/// and evaluates dynamic thresholds for signal generation.
pub struct SpreadEngine {
    /// Latest snapshot per (event_id, venue).
    latest: HashMap<(String, Venue), MarketSnapshot>,
    /// Rolling statistics per event_id (for dynamic threshold).
    stats: HashMap<String, RollingStats>,
    /// Spread computation configuration.
    config: SpreadConfig,
    /// JSONL logger for spread computations.
    logger: SpreadLogger,
    /// Count of signals above threshold (for metrics).
    signal_count: u64,
    /// When true, wall-clock staleness gates are bypassed.
    /// Used in replay mode where historical data would otherwise be rejected.
    replay_mode: bool,
    /// Optional shared cache of basis risk data per event.
    /// Populated by ContractLifecycleManager, read here for premium calculation.
    basis_risk_cache: Option<BasisRiskCache>,
}

impl SpreadEngine {
    /// Create a new SpreadEngine with the given configuration.
    pub fn new(config: SpreadConfig) -> Self {
        let logger = SpreadLogger::new(&config.log_dir);
        Self {
            latest: HashMap::new(),
            stats: HashMap::new(),
            config,
            logger,
            signal_count: 0,
            replay_mode: false,
            basis_risk_cache: None,
        }
    }

    /// Enable or disable replay mode.
    ///
    /// When replay mode is active, wall-clock staleness checks are bypassed
    /// so that historical data is not rejected as stale. The processor-level
    /// `is_stale` flag on MarketSnapshot still functions normally.
    pub fn with_replay_mode(mut self, replay: bool) -> Self {
        self.replay_mode = replay;
        self
    }

    /// Attach a shared BasisRiskCache for settlement risk premium lookups.
    pub fn with_basis_risk_cache(mut self, cache: BasisRiskCache) -> Self {
        self.basis_risk_cache = Some(cache);
        self
    }

    /// Look up basis risk premium for an event from the shared cache.
    /// Returns Decimal::ZERO if cache is not configured or event has no entry.
    fn lookup_basis_risk_premium(&self, event_id: &str) -> Decimal {
        use rust_decimal::prelude::FromPrimitive;

        let cache = match &self.basis_risk_cache {
            Some(c) => c,
            None => return Decimal::ZERO,
        };
        // Non-blocking read -- if lock is contended, return zero
        let guard = match cache.try_read() {
            Ok(g) => g,
            Err(_) => return Decimal::ZERO,
        };
        match guard.get(event_id) {
            Some(info) => {
                Decimal::from_f64(info.effective_composite)
                    .unwrap_or(Decimal::ZERO)
                    * self.config.basis_risk_scale
            }
            None => Decimal::ZERO,
        }
    }

    /// Main event loop: consume snapshots, compute spreads, emit signals.
    ///
    /// Uses `tokio::select!` with biased selection:
    /// 1. Cancellation token (highest priority)
    /// 2. Stats emission interval tick
    /// 3. Snapshot reception from fan-in channel
    ///
    /// If `ptrade_snap_tx` is provided, each received snapshot is forwarded
    /// (best-effort, non-blocking) to the paper trade tracker for next-tick
    /// fill and MTM updates.
    pub async fn run(
        mut self,
        mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
        registry: Arc<RwLock<EventRegistry>>,
        cancel: CancellationToken,
        signal_tx: mpsc::Sender<SpreadResult>,
        ptrade_snap_tx: Option<mpsc::Sender<MarketSnapshot>>,
    ) {
        let mut stats_interval = tokio::time::interval(Duration::from_secs(
            self.config.stats_emission_interval_secs,
        ));
        // Don't fire immediately on start.
        stats_interval.tick().await;

        tracing::info!("SpreadEngine started");

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    tracing::info!(signal_count = self.signal_count, "SpreadEngine shutting down");
                    break;
                }

                _ = stats_interval.tick() => {
                    self.emit_aggregate_stats();
                }

                snapshot = snapshot_rx.recv() => {
                    match snapshot {
                        Some(snap) => {
                            // Forward snapshot to paper trade tracker (best effort)
                            if let Some(ref tx) = ptrade_snap_tx {
                                let _ = tx.try_send(snap.clone());
                            }
                            self.process_snapshot(snap, &registry, &signal_tx).await;
                        }
                        None => {
                            tracing::info!("snapshot channel closed, SpreadEngine stopping");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Process a single snapshot: pair, staleness check, compute spreads.
    async fn process_snapshot(
        &mut self,
        snap: MarketSnapshot,
        registry: &Arc<RwLock<EventRegistry>>,
        signal_tx: &mpsc::Sender<SpreadResult>,
    ) {
        // 1. Look up event mapping
        let reg = registry.read().await;
        let mapping = match reg.lookup_by_instrument(snap.venue, &snap.instrument_id.to_string()) {
            Some(m) => m.clone(),
            None => return, // unmapped instrument
        };
        drop(reg);

        // 2. Only process if mapping has both Polymarket AND Kalshi venue entries
        if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() {
            return; // Deribit-only or single-venue -- skip (Phase 8)
        }

        let event_id = mapping.id.clone();

        // 3. Store latest snapshot keyed by (event_id, venue)
        self.latest
            .insert((event_id.clone(), snap.venue), snap);

        // 4. Try to get both snapshots
        let poly = match self.latest.get(&(event_id.clone(), Venue::Polymarket)) {
            Some(s) => s.clone(),
            None => return, // waiting for other leg
        };
        let kalshi = match self.latest.get(&(event_id.clone(), Venue::Kalshi)) {
            Some(s) => s.clone(),
            None => return, // waiting for other leg
        };

        // 5. Staleness gate
        if !self.passes_staleness_gate(&event_id, &poly, &kalshi) {
            return;
        }

        // 6. Compute all 4 patterns
        let now_ms = chrono::Utc::now().timestamp_millis();

        for pattern in SpreadPattern::all() {
            // Compute gross spread
            let gross = match compute_gross_spread(pattern, &poly, &kalshi) {
                Some(g) => g,
                None => continue, // missing probabilities
            };

            // Walk the book for realistic fill prices
            let (buy_walk, sell_walk) =
                self.walk_both_sides(pattern, &poly, &kalshi);

            // Compute fees
            let (buy_fee, sell_fee) = self.compute_fees(pattern, &buy_walk, &sell_walk, &poly, &kalshi);

            // Compute carry cost
            let carry = carry_cost(self.config.target_notional, &self.config.carry);

            // Settlement basis risk premium
            let basis_risk_premium = self.lookup_basis_risk_premium(&event_id);

            // Total cost
            let total_cost = buy_fee + sell_fee + carry + basis_risk_premium;

            // Net spread: sell_fill_price - buy_fill_price - total_cost
            let net_spread = sell_walk.avg_fill_price - buy_walk.avg_fill_price - total_cost;

            // Rolling stats update
            let rolling = self
                .stats
                .entry(event_id.clone())
                .or_insert_with(|| RollingStats::new(self.config.rolling_window_secs));

            let net_spread_f64 = decimal_to_f64(net_spread);
            rolling.push(net_spread_f64, now_ms);

            // Threshold computation
            let (threshold_value, components) = compute_threshold(
                rolling,
                &self.config.threshold,
                buy_walk.fill_ratio(),
                sell_walk.fill_ratio(),
            );

            // Build SpreadResult
            let result = SpreadResult {
                event_id: event_id.clone(),
                pattern,
                gross_spread: gross.gross_spread,
                net_spread,
                buy_fill_price: buy_walk.avg_fill_price,
                sell_fill_price: sell_walk.avg_fill_price,
                buy_fee,
                sell_fee,
                carry_cost: carry,
                total_cost,
                basis_risk_premium,
                buy_fill_ratio: buy_walk.fill_ratio(),
                sell_fill_ratio: sell_walk.fill_ratio(),
                target_notional: self.config.target_notional,
                timestamp_ms: now_ms,
                poly_exchange_ts: poly.exchange_timestamp,
                kalshi_exchange_ts: kalshi.exchange_timestamp,
                threshold: Some(threshold_value),
                threshold_components: Some(components),
            };

            // JSONL logging -- every computation
            if let Err(e) = self.logger.log(&result).await {
                tracing::warn!(error = %e, "failed to write spread log");
            }

            // Prometheus metrics -- every computation
            metrics::histogram!("spread_net", "event" => event_id.clone(), "pattern" => pattern.label())
                .record(net_spread_f64);
            metrics::counter!("spread_computations_total", "event" => event_id.clone())
                .increment(1);

            // Threshold check -- signal if net_spread > threshold
            if net_spread > threshold_value {
                self.signal_count += 1;
                metrics::counter!("spread_signals_total", "event" => event_id.clone(), "pattern" => pattern.label())
                    .increment(1);

                // Send to paper trade tracker (best effort)
                let _ = signal_tx.try_send(result);
            }
        }
    }

    /// Check staleness gate on both legs.
    ///
    /// Returns true if both snapshots pass; false if either is stale.
    /// In replay mode, wall-clock timestamp age checks are bypassed since
    /// historical data would always appear stale relative to current time.
    fn passes_staleness_gate(
        &self,
        event_id: &str,
        poly: &MarketSnapshot,
        kalshi: &MarketSnapshot,
    ) -> bool {
        // In replay mode, skip all wall-clock staleness gates
        if self.replay_mode {
            return true;
        }

        let now_ms = chrono::Utc::now().timestamp_millis();

        // Check Polymarket staleness
        if poly.is_stale {
            tracing::debug!(
                event_id = event_id,
                venue = "polymarket",
                reason = "is_stale flag set",
                "staleness gate rejection"
            );
            metrics::counter!("spread_staleness_rejections", "event" => event_id.to_string(), "venue" => "polymarket")
                .increment(1);
            return false;
        }

        // Check Polymarket exchange timestamp age
        if let Some(exchange_ts) = poly.exchange_timestamp {
            let age_ms = now_ms - exchange_ts;
            if age_ms > self.config.staleness_threshold_ms as i64 {
                tracing::debug!(
                    event_id = event_id,
                    venue = "polymarket",
                    age_ms = age_ms,
                    threshold_ms = self.config.staleness_threshold_ms,
                    reason = "exchange timestamp too old",
                    "staleness gate rejection"
                );
                metrics::counter!("spread_staleness_rejections", "event" => event_id.to_string(), "venue" => "polymarket")
                    .increment(1);
                return false;
            }
        }

        // Check Kalshi staleness
        if kalshi.is_stale {
            tracing::debug!(
                event_id = event_id,
                venue = "kalshi",
                reason = "is_stale flag set",
                "staleness gate rejection"
            );
            metrics::counter!("spread_staleness_rejections", "event" => event_id.to_string(), "venue" => "kalshi")
                .increment(1);
            return false;
        }

        // Check Kalshi wall clock age (REST-polled, no exchange timestamp)
        let kalshi_wall_ms = kalshi.timestamp.wall().timestamp_millis();
        let kalshi_age_ms = now_ms - kalshi_wall_ms;
        if kalshi_age_ms > self.config.kalshi_staleness_threshold_ms as i64 {
            tracing::debug!(
                event_id = event_id,
                venue = "kalshi",
                age_ms = kalshi_age_ms,
                threshold_ms = self.config.kalshi_staleness_threshold_ms,
                reason = "wall clock timestamp too old",
                "staleness gate rejection"
            );
            metrics::counter!("spread_staleness_rejections", "event" => event_id.to_string(), "venue" => "kalshi")
                .increment(1);
            return false;
        }

        true
    }

    /// Walk the book on both sides for a given pattern.
    ///
    /// Determines which depth sides to walk based on the pattern:
    /// - Pattern 1 (Buy Poly YES, Sell Kalshi YES): buy walks poly asks, sell walks kalshi bids
    /// - Pattern 2 (Sell Poly YES, Buy Kalshi YES): buy walks kalshi asks, sell walks poly bids
    /// - Pattern 3 (Buy Poly NO, Sell Kalshi NO): buy walks poly bids (NO=inverse), sell walks kalshi asks
    /// - Pattern 4 (Sell Poly NO, Buy Kalshi NO): buy walks kalshi bids, sell walks poly asks
    fn walk_both_sides(
        &self,
        pattern: SpreadPattern,
        poly: &MarketSnapshot,
        kalshi: &MarketSnapshot,
    ) -> (WalkResult, WalkResult) {
        let target = self.config.target_notional;

        let (buy_depth, sell_depth) = match pattern {
            SpreadPattern::BuyPolyYesSellKalshiYes => {
                (&poly.depth_asks, &kalshi.depth_bids)
            }
            SpreadPattern::SellPolyYesBuyKalshiYes => {
                (&kalshi.depth_asks, &poly.depth_bids)
            }
            SpreadPattern::BuyPolyNoSellKalshiNo => {
                // NO side: buy walks bids (inverse), sell walks asks
                (&poly.depth_bids, &kalshi.depth_asks)
            }
            SpreadPattern::SellPolyNoBuyKalshiNo => {
                (&kalshi.depth_bids, &poly.depth_asks)
            }
        };

        let buy_walk = walk_the_book(buy_depth, target);
        let sell_walk = walk_the_book(sell_depth, target);

        (buy_walk, sell_walk)
    }

    /// Compute fees for both sides of a spread trade.
    ///
    /// Determines which venue is on the buy vs sell side from the pattern,
    /// then applies the appropriate fee model.
    fn compute_fees(
        &self,
        pattern: SpreadPattern,
        buy_walk: &WalkResult,
        sell_walk: &WalkResult,
        poly: &MarketSnapshot,
        kalshi: &MarketSnapshot,
    ) -> (Decimal, Decimal) {
        let buy_venue = pattern.buy_venue();
        let sell_venue = pattern.sell_venue();

        let buy_fee = self.compute_venue_fee(buy_venue, buy_walk, poly, kalshi);
        let sell_fee = self.compute_venue_fee(sell_venue, sell_walk, poly, kalshi);

        (buy_fee, sell_fee)
    }

    /// Compute fee for a single venue side of the trade.
    fn compute_venue_fee(
        &self,
        venue: Venue,
        walk: &WalkResult,
        poly: &MarketSnapshot,
        kalshi: &MarketSnapshot,
    ) -> Decimal {
        match venue {
            Venue::Polymarket => {
                let price = poly
                    .bid_probability
                    .map(|p| p.into_inner())
                    .unwrap_or(walk.avg_fill_price);
                polymarket_fee(walk.filled_notional, price, &self.config.polymarket_fees)
            }
            Venue::Kalshi => {
                let price = kalshi
                    .bid_probability
                    .map(|p| p.into_inner())
                    .unwrap_or(walk.avg_fill_price);
                // Kalshi fee uses contracts (filled_notional) and probability
                kalshi_taker_fee(walk.filled_notional, price, &self.config.kalshi_fees)
            }
            Venue::Deribit => Decimal::ZERO, // Not used in prediction market spreads
        }
    }

    /// Emit aggregate statistics for all tracked events.
    fn emit_aggregate_stats(&self) {
        for (event_id, stats) in &self.stats {
            let count = stats.count();
            if count == 0 {
                continue;
            }

            let mean = stats.mean();
            let stddev = stats.stddev();
            let p50 = stats.percentile(50.0);
            let p95 = stats.percentile(95.0);

            tracing::info!(
                event_id = event_id.as_str(),
                count = count,
                mean = mean,
                stddev = stddev,
                p50 = p50,
                p95 = p95,
                "spread aggregate stats"
            );

            metrics::gauge!("spread_rolling_mean", "event" => event_id.clone()).set(mean);
            metrics::gauge!("spread_rolling_stddev", "event" => event_id.clone()).set(stddev);
        }
    }
}

/// Convert Decimal to f64 for metrics and rolling stats.
fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DualTimestamp, InstrumentId, Notional, Price, Probability, TraceId};
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn prob(s: &str) -> Probability {
        Probability::new(dec(s)).unwrap()
    }

    fn make_snapshot(
        venue: Venue,
        bid_prob: Option<&str>,
        ask_prob: Option<&str>,
        depth_bids: Vec<(Price, Notional)>,
        depth_asks: Vec<(Price, Notional)>,
        is_stale: bool,
        exchange_ts: Option<i64>,
    ) -> MarketSnapshot {
        MarketSnapshot {
            venue,
            instrument_id: InstrumentId::new("TEST-INST"),
            event_id: None,
            bid: None,
            ask: None,
            bid_size: None,
            ask_size: None,
            depth_bids,
            depth_asks,
            bid_probability: bid_prob.map(prob),
            ask_probability: ask_prob.map(prob),
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
            exchange_timestamp: exchange_ts,
            timestamp: DualTimestamp::now(),
            sequence: 1,
            trace_id: TraceId::new(),
            is_stale,
        }
    }

    fn default_config() -> SpreadConfig {
        SpreadConfig {
            target_notional: dec("500"),
            staleness_threshold_ms: 5000,
            kalshi_staleness_threshold_ms: 15000,
            ..SpreadConfig::default()
        }
    }

    // -- Staleness gate tests --

    #[test]
    fn staleness_gate_rejects_stale_poly() {
        let engine = SpreadEngine::new(default_config());
        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            true, // stale
            Some(chrono::Utc::now().timestamp_millis()),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            None,
        );
        assert!(!engine.passes_staleness_gate("test-event", &poly, &kalshi));
    }

    #[test]
    fn staleness_gate_rejects_stale_kalshi() {
        let engine = SpreadEngine::new(default_config());
        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            Some(chrono::Utc::now().timestamp_millis()),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            true, // stale
            None,
        );
        assert!(!engine.passes_staleness_gate("test-event", &poly, &kalshi));
    }

    #[test]
    fn staleness_gate_rejects_old_poly_exchange_ts() {
        let engine = SpreadEngine::new(default_config());
        let old_ts = chrono::Utc::now().timestamp_millis() - 10_000; // 10s ago
        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            Some(old_ts),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            None,
        );
        // Poly exchange ts is 10s old, threshold is 5s
        assert!(!engine.passes_staleness_gate("test-event", &poly, &kalshi));
    }

    #[test]
    fn staleness_gate_passes_fresh_snapshots() {
        let engine = SpreadEngine::new(default_config());
        let now_ms = chrono::Utc::now().timestamp_millis();
        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            Some(now_ms),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.52"),
            vec![],
            vec![],
            false,
            None,
        );
        assert!(engine.passes_staleness_gate("test-event", &poly, &kalshi));
    }

    // -- Walk both sides tests --

    #[test]
    fn walk_both_sides_pattern1() {
        let engine = SpreadEngine::new(default_config());

        let poly_asks = vec![
            (Price::new(dec("0.45")), Notional::new(dec("600"))),
        ];
        let kalshi_bids = vec![
            (Price::new(dec("0.50")), Notional::new(dec("600"))),
        ];

        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.42"),
            Some("0.45"),
            vec![],
            poly_asks,
            false,
            Some(chrono::Utc::now().timestamp_millis()),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.53"),
            kalshi_bids,
            vec![],
            false,
            None,
        );

        let (buy_walk, sell_walk) = engine.walk_both_sides(
            SpreadPattern::BuyPolyYesSellKalshiYes,
            &poly,
            &kalshi,
        );

        assert_eq!(buy_walk.avg_fill_price, dec("0.45"));
        assert_eq!(sell_walk.avg_fill_price, dec("0.50"));
    }

    // -- 4-pattern computation test --

    #[tokio::test]
    async fn four_patterns_produce_results() {
        let config = default_config();
        let mut engine = SpreadEngine::new(config.clone());

        // Create snapshots with depth
        let poly_bids = vec![
            (Price::new(dec("0.42")), Notional::new(dec("600"))),
        ];
        let poly_asks = vec![
            (Price::new(dec("0.45")), Notional::new(dec("600"))),
        ];
        let kalshi_bids = vec![
            (Price::new(dec("0.50")), Notional::new(dec("600"))),
        ];
        let kalshi_asks = vec![
            (Price::new(dec("0.53")), Notional::new(dec("600"))),
        ];

        let now_ms = chrono::Utc::now().timestamp_millis();
        let poly = make_snapshot(
            Venue::Polymarket,
            Some("0.42"),
            Some("0.45"),
            poly_bids,
            poly_asks,
            false,
            Some(now_ms),
        );
        let kalshi = make_snapshot(
            Venue::Kalshi,
            Some("0.50"),
            Some("0.53"),
            kalshi_bids,
            kalshi_asks,
            false,
            None,
        );

        let event_id = "test-event".to_string();
        engine
            .latest
            .insert((event_id.clone(), Venue::Polymarket), poly.clone());
        engine
            .latest
            .insert((event_id.clone(), Venue::Kalshi), kalshi.clone());

        // Collect results from all 4 patterns
        let mut results = Vec::new();
        for pattern in SpreadPattern::all() {
            let gross = compute_gross_spread(pattern, &poly, &kalshi);
            assert!(gross.is_some(), "gross spread should be Some for {:?}", pattern);

            let (buy_walk, sell_walk) = engine.walk_both_sides(pattern, &poly, &kalshi);
            let (buy_fee, sell_fee) = engine.compute_fees(pattern, &buy_walk, &sell_walk, &poly, &kalshi);
            let carry = carry_cost(config.target_notional, &config.carry);
            let total_cost = buy_fee + sell_fee + carry;
            let net_spread = sell_walk.avg_fill_price - buy_walk.avg_fill_price - total_cost;

            results.push((pattern, net_spread));
        }

        assert_eq!(results.len(), 4, "should produce 4 spread results");

        // Verify distinct patterns
        let patterns: Vec<_> = results.iter().map(|(p, _)| p.label()).collect();
        let mut unique = patterns.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 4, "all 4 patterns should be distinct");
    }
}
