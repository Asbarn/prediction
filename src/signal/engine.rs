//! Cross-asset signal generation engine.
//!
//! Consumes `ImpliedProbability` events (from PricingEngine) and prediction
//! market `MarketSnapshot` events (from fan-out), pairs them by event ID via
//! `EventRegistry`, computes directional spreads with the full cost model,
//! evaluates dynamic thresholds, logs every computation to JSONL, and emits
//! `ArbSignal` structs on a channel for downstream consumption.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::alert::PipelineLiveness;
use crate::events::registry::EventRegistry;
use crate::events::risk::BasisRiskCache;
use crate::pricing::types::ImpliedProbability;
use crate::signal::config::SignalGenerationConfig;
use crate::signal::logger::SignalLogger;
use crate::signal::types::{
    ArbDirection, ArbSignal, CostBreakdown, LegInfo, ThresholdStatus,
};
use crate::spread::book_walker::walk_the_book;
use crate::spread::cost_model::{carry_cost, kalshi_taker_fee, polymarket_fee};
use crate::spread::rolling_stats::RollingStats;
use crate::spread::threshold::compute_threshold;
use crate::subscription::CleanupEvent;
use crate::types::{DualTimestamp, MarketSnapshot, Venue};

/// Cross-asset arbitrage signal generation engine.
///
/// Pairs options-implied probabilities with prediction market prices,
/// computes directional spreads with the full cost model, evaluates
/// dynamic thresholds, and emits `ArbSignal` structs for downstream use.
pub struct CrossAssetEngine {
    /// Latest options-implied probability per event_id.
    latest_prob: HashMap<String, ImpliedProbability>,
    /// Latest prediction market snapshot per (event_id, venue).
    latest_pred: HashMap<(String, Venue), MarketSnapshot>,
    /// Rolling statistics per event_id for dynamic threshold.
    stats: HashMap<String, RollingStats>,
    /// Configuration.
    config: SignalGenerationConfig,
    /// JSONL logger for all signal computations.
    logger: SignalLogger,
    /// Count of signals that passed threshold and were emitted.
    signal_count: u64,
    /// Count of signals filtered by threshold.
    filtered_count: u64,
    /// When true, wall-clock staleness gates are bypassed.
    /// Used in replay mode where historical data would otherwise be rejected.
    replay_mode: bool,
    /// Optional shared cache of basis risk data per event.
    /// Populated by ContractLifecycleManager, read here for premium and threshold inflation.
    basis_risk_cache: Option<BasisRiskCache>,
    /// Optional pipeline liveness tracker for AlertMonitor.
    liveness: Option<Arc<PipelineLiveness>>,
}

impl CrossAssetEngine {
    /// Create a new CrossAssetEngine with the given configuration.
    pub fn new(config: SignalGenerationConfig) -> Self {
        let logger = SignalLogger::new(&config.log_dir);
        Self {
            latest_prob: HashMap::new(),
            latest_pred: HashMap::new(),
            stats: HashMap::new(),
            config,
            logger,
            signal_count: 0,
            filtered_count: 0,
            replay_mode: false,
            basis_risk_cache: None,
            liveness: None,
        }
    }

    /// Enable or disable replay mode.
    ///
    /// When replay mode is active, wall-clock staleness checks are bypassed
    /// so that historical data is not rejected as stale.
    pub fn with_replay_mode(mut self, replay: bool) -> Self {
        self.replay_mode = replay;
        self
    }

    /// Attach a shared BasisRiskCache for settlement risk premium lookups
    /// and near-expiry threshold inflation.
    pub fn with_basis_risk_cache(mut self, cache: BasisRiskCache) -> Self {
        self.basis_risk_cache = Some(cache);
        self
    }

    /// Attach a PipelineLiveness tracker for AlertMonitor stage liveness.
    pub fn with_liveness(mut self, liveness: Arc<PipelineLiveness>) -> Self {
        self.liveness = Some(liveness);
        self
    }

    /// Look up basis risk premium for an event from the shared cache.
    /// Returns Decimal::ZERO if cache is not configured or event has no entry.
    fn lookup_basis_risk_premium(&self, event_id: &str) -> Decimal {
        let cache = match &self.basis_risk_cache {
            Some(c) => c,
            None => return Decimal::ZERO,
        };
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

    /// Look up near-expiry inflation factor for threshold adjustment.
    /// Returns Decimal::ONE if no expiry warning or cache not configured.
    fn lookup_expiry_threshold_inflation(&self, event_id: &str) -> Decimal {
        let cache = match &self.basis_risk_cache {
            Some(c) => c,
            None => return Decimal::ONE,
        };
        let guard = match cache.try_read() {
            Ok(g) => g,
            Err(_) => return Decimal::ONE,
        };
        match guard.get(event_id) {
            Some(info) => match &info.expiry_warning {
                Some(w) => Decimal::from_f64(w.risk_inflation_factor)
                    .unwrap_or(Decimal::ONE),
                None => Decimal::ONE,
            },
            None => Decimal::ONE,
        }
    }

    /// Main event loop: consume probabilities and prediction market snapshots,
    /// pair by event ID, compute cross-asset spreads, emit signals.
    ///
    /// Uses biased `tokio::select!`:
    /// 1. Cancellation (highest priority)
    /// 2. Stats emission interval
    /// 3. Probability reception (from PricingEngine)
    /// 4. Prediction market snapshot reception (from fan-out)
    pub async fn run(
        mut self,
        mut prob_rx: mpsc::Receiver<ImpliedProbability>,
        mut pred_snap_rx: mpsc::Receiver<MarketSnapshot>,
        registry: Arc<RwLock<EventRegistry>>,
        cancel: CancellationToken,
        signal_tx: mpsc::Sender<ArbSignal>,
        mut cleanup_rx: mpsc::Receiver<CleanupEvent>,
    ) {
        let mut stats_interval = tokio::time::interval(Duration::from_secs(
            self.config.summary_interval_secs,
        ));
        // Don't fire immediately on start.
        stats_interval.tick().await;

        tracing::info!("CrossAssetEngine started");

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    tracing::info!(
                        signal_count = self.signal_count,
                        filtered_count = self.filtered_count,
                        "CrossAssetEngine shutting down"
                    );
                    break;
                }

                _ = stats_interval.tick() => {
                    self.emit_summary();
                }

                Some(_cleanup) = cleanup_rx.recv() => {
                    // Evict stale entries for instruments no longer subscribed.
                    // Use the registry to determine which event_ids are still active,
                    // then retain only entries matching those active event_ids.
                    let reg = registry.read().await;
                    let active_ids: std::collections::HashSet<String> =
                        reg.active_approved().map(|m| m.id.clone()).collect();
                    drop(reg);

                    let before_prob = self.latest_prob.len();
                    self.latest_prob.retain(|eid, _| active_ids.contains(eid));
                    let before_pred = self.latest_pred.len();
                    self.latest_pred.retain(|&(ref eid, _), _| active_ids.contains(eid));
                    let before_stats = self.stats.len();
                    self.stats.retain(|eid, _| active_ids.contains(eid));
                    tracing::info!(
                        prob_removed = before_prob - self.latest_prob.len(),
                        pred_removed = before_pred - self.latest_pred.len(),
                        stats_removed = before_stats - self.stats.len(),
                        "CrossAssetEngine: cleaned up stale entries"
                    );
                }

                prob = prob_rx.recv() => {
                    match prob {
                        Some(prob) => {
                            self.handle_probability(prob, &registry, &signal_tx).await;
                        }
                        None => {
                            tracing::info!("probability channel closed, CrossAssetEngine stopping");
                            break;
                        }
                    }
                }

                snap = pred_snap_rx.recv() => {
                    match snap {
                        Some(snap) => {
                            self.handle_prediction_snapshot(snap, &registry, &signal_tx).await;
                        }
                        None => {
                            tracing::info!("prediction snapshot channel closed, CrossAssetEngine stopping");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Handle an incoming ImpliedProbability event.
    ///
    /// Looks up the event mapping via EventRegistry, caches the probability,
    /// and attempts spread computation against any existing prediction market data.
    async fn handle_probability(
        &mut self,
        prob: ImpliedProbability,
        registry: &Arc<RwLock<EventRegistry>>,
        signal_tx: &mpsc::Sender<ArbSignal>,
    ) {
        // 1. Look up event mapping for this Deribit instrument
        let reg = registry.read().await;
        let mapping = match reg.lookup_by_instrument(
            Venue::Deribit,
            &prob.instrument_id.to_string(),
        ) {
            Some(m) => m.clone(),
            None => {
                tracing::debug!(
                    instrument = %prob.instrument_id,
                    "unmapped Deribit instrument, skipping"
                );
                metrics::counter!("arb_unmapped_instruments_total").increment(1);
                return;
            }
        };
        drop(reg);

        // 2. Extract event_id
        let event_id = mapping.id.clone();

        // 3. Cache latest probability
        self.latest_prob.insert(event_id.clone(), prob);

        // 4. Try spread computation against each prediction market venue
        for venue in [Venue::Polymarket, Venue::Kalshi] {
            if self.latest_pred.contains_key(&(event_id.clone(), venue)) {
                self.compute_and_emit(&event_id, venue, signal_tx).await;
            }
        }
    }

    /// Handle an incoming prediction market snapshot.
    ///
    /// Only processes Polymarket or Kalshi snapshots. Looks up the event mapping,
    /// caches the snapshot, and attempts spread computation against any existing
    /// options-implied probability.
    async fn handle_prediction_snapshot(
        &mut self,
        snap: MarketSnapshot,
        registry: &Arc<RwLock<EventRegistry>>,
        signal_tx: &mpsc::Sender<ArbSignal>,
    ) {
        // Only process Polymarket or Kalshi snapshots
        if snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi {
            return;
        }

        // Look up event mapping
        let reg = registry.read().await;
        let mapping = match reg.lookup_by_instrument(snap.venue, &snap.instrument_id.to_string()) {
            Some(m) => m.clone(),
            None => return, // unmapped prediction market instrument
        };
        drop(reg);

        let event_id = mapping.id.clone();
        let venue = snap.venue;

        // Cache latest snapshot
        self.latest_pred.insert((event_id.clone(), venue), snap);

        // Try spread computation if we have options probability
        if self.latest_prob.contains_key(&event_id) {
            self.compute_and_emit(&event_id, venue, signal_tx).await;
        }
    }

    /// Core spread computation and signal emission.
    ///
    /// Computes both directions (buy prediction / sell options and vice versa),
    /// applies the full cost model, evaluates dynamic thresholds, logs to JSONL,
    /// and emits signals that pass threshold.
    async fn compute_and_emit(
        &mut self,
        event_id: &str,
        pred_venue: Venue,
        signal_tx: &mpsc::Sender<ArbSignal>,
    ) {
        let prob = match self.latest_prob.get(event_id) {
            Some(p) => p.clone(),
            None => return,
        };
        let snap = match self.latest_pred.get(&(event_id.to_string(), pred_venue)) {
            Some(s) => s.clone(),
            None => return,
        };

        // Current wall-clock time for staleness checks and rolling stats.
        let now_ms = chrono::Utc::now().timestamp_millis();

        // --- Staleness gate ---
        // In replay mode, skip all wall-clock staleness gates since
        // historical data would always appear stale relative to current time.
        if !self.replay_mode {

            // Check options-implied probability staleness
            let prob_age_ms = now_ms - prob.timestamp.wall().timestamp_millis();
            if prob_age_ms > self.config.options_staleness_ms as i64 {
                tracing::debug!(
                    event_id = event_id,
                    age_ms = prob_age_ms,
                    threshold_ms = self.config.options_staleness_ms,
                    "options probability stale, skipping"
                );
                metrics::counter!("arb_staleness_rejections").increment(1);
                return;
            }

            // Check prediction market snapshot staleness
            let pred_staleness_ms = match pred_venue {
                Venue::Polymarket => self.config.polymarket_staleness_ms,
                Venue::Kalshi => self.config.kalshi_staleness_ms,
                _ => self.config.polymarket_staleness_ms,
            };
            let pred_ts_ms = snap
                .exchange_timestamp
                .unwrap_or_else(|| snap.timestamp.wall().timestamp_millis());
            let pred_age_ms = now_ms - pred_ts_ms;
            if pred_age_ms > pred_staleness_ms as i64 {
                tracing::debug!(
                    event_id = event_id,
                    venue = ?pred_venue,
                    age_ms = pred_age_ms,
                    threshold_ms = pred_staleness_ms,
                    "prediction market snapshot stale, skipping"
                );
                metrics::counter!("arb_staleness_rejections").increment(1);
                return;
            }
        }

        // --- Extract probabilities ---
        let options_prob = prob.probability.into_inner();
        let pred_bid = match snap.bid_probability {
            Some(p) => p.into_inner(),
            None => return,
        };
        let pred_ask = match snap.ask_probability {
            Some(p) => p.into_inner(),
            None => return,
        };

        // --- Compute both directions ---
        let directions: [(ArbDirection, Decimal, Decimal, &[(crate::types::Price, crate::types::Notional)]); 2] = [
            // BuyPredictionSellOptions: buy prediction at ask, options prob is the "sell" side
            (
                ArbDirection::BuyPredictionSellOptions,
                options_prob - pred_ask, // raw spread
                pred_ask,               // prediction executable price (top of book)
                &snap.depth_asks,       // walk asks for buying
            ),
            // SellPredictionBuyOptions: sell prediction at bid, options prob is the "buy" side
            (
                ArbDirection::SellPredictionBuyOptions,
                pred_bid - options_prob, // raw spread
                pred_bid,               // prediction executable price (top of book)
                &snap.depth_bids,       // walk bids for selling
            ),
        ];

        for (direction, raw_spread, top_of_book_price, pred_depth) in directions {
            // --- Walk prediction market book ---
            let walk = walk_the_book(pred_depth, self.config.target_notional);

            // --- Compute costs ---
            // Prediction market fee
            let prediction_fee = match pred_venue {
                Venue::Polymarket => polymarket_fee(
                    walk.filled_notional,
                    top_of_book_price,
                    &self.config.polymarket_fees,
                ),
                Venue::Kalshi => kalshi_taker_fee(
                    walk.filled_notional,
                    top_of_book_price,
                    &self.config.kalshi_fees,
                ),
                _ => Decimal::ZERO,
            };

            // Options fee estimate: taker_fee_rate * underlying_price * |delta|
            let options_fee_estimate = self.config.deribit_taker_fee_rate
                * Decimal::from_f64(prob.underlying_price).unwrap_or(Decimal::ZERO)
                * Decimal::from_f64(prob.greeks.delta.abs()).unwrap_or(Decimal::ZERO);

            // Carry cost
            let carry = carry_cost(self.config.target_notional, &self.config.carry);

            // Prediction slippage: |avg_fill - top_of_book|
            let prediction_slippage = (walk.avg_fill_price - top_of_book_price).abs();

            // Options spread cost: half of bid-ask spread in probability space
            let options_spread_cost = match (prob.prob_bid, prob.prob_ask) {
                (Some(bid), Some(ask)) => {
                    (ask.into_inner() - bid.into_inner()).abs() / Decimal::new(2, 0)
                }
                _ => Decimal::ZERO,
            };

            // Liquidity factor: combined from prediction market and options side
            let pred_liquidity_factor = walk.fill_ratio();
            let options_liquidity_factor = match (prob.prob_bid, prob.prob_ask) {
                (Some(bid), Some(ask)) => {
                    let ba_spread = (ask.into_inner() - bid.into_inner()).abs();
                    let factor = Decimal::ONE - ba_spread * Decimal::new(5, 0);
                    factor.max(Decimal::new(1, 1)) // floor at 0.1
                }
                _ => Decimal::new(5, 1), // 0.5 conservative default
            };
            let liquidity_factor = pred_liquidity_factor.min(options_liquidity_factor);

            // Basis risk premium from settlement risk cache
            let basis_risk_premium = self.lookup_basis_risk_premium(event_id);

            // Total cost (excluding liquidity factor, which multiplies edge)
            let total_cost =
                prediction_fee + options_fee_estimate + carry + prediction_slippage + options_spread_cost + basis_risk_premium;

            // Net edge = (raw_spread - total_cost) * liquidity_factor
            let net_edge = (raw_spread - total_cost) * liquidity_factor;

            // --- Rolling stats ---
            let net_edge_f64 = decimal_to_f64(net_edge);
            let rolling = self
                .stats
                .entry(event_id.to_string())
                .or_insert_with(|| RollingStats::new(self.config.rolling_window_secs));
            rolling.push(net_edge_f64, now_ms);

            // --- Threshold evaluation ---
            let (threshold_value, components) = compute_threshold(
                rolling,
                &self.config.threshold,
                walk.fill_ratio(),
                Decimal::ONE, // options side has no walk equivalent
            );

            // Tighten threshold for near-expiry events (EVNT-05)
            let expiry_inflation = self.lookup_expiry_threshold_inflation(event_id);
            let threshold_value = threshold_value * expiry_inflation;

            let threshold_status = if net_edge > threshold_value {
                ThresholdStatus::PassedBoth
            } else if net_edge > self.config.threshold.static_floor {
                ThresholdStatus::PassedStaticOnly
            } else {
                ThresholdStatus::Filtered
            };

            // --- Build ArbSignal ---
            let iv_spread = prob.iv_spread;

            // Prediction leg info
            let pred_book_depth = match direction {
                ArbDirection::BuyPredictionSellOptions => snap.depth_asks.len(),
                ArbDirection::SellPredictionBuyOptions => snap.depth_bids.len(),
            };

            // Options leg: use prob_bid or prob_ask for executable price depending on direction
            let options_executable = match direction {
                ArbDirection::BuyPredictionSellOptions => {
                    // We are "selling" options-implied prob -> use prob_bid if available
                    prob.prob_bid
                        .map(|p| p.into_inner())
                        .unwrap_or(options_prob)
                }
                ArbDirection::SellPredictionBuyOptions => {
                    // We are "buying" options-implied prob -> use prob_ask if available
                    prob.prob_ask
                        .map(|p| p.into_inner())
                        .unwrap_or(options_prob)
                }
            };

            let signal = ArbSignal {
                signal_id: uuid::Uuid::now_v7().to_string(),
                event_id: event_id.to_string(),
                direction,
                raw_spread,
                net_edge,
                confidence: prob.confidence,
                prediction_leg: LegInfo {
                    venue: pred_venue,
                    instrument_id: snap.instrument_id.to_string(),
                    probability: (pred_bid + pred_ask) / Decimal::new(2, 0),
                    executable_price: if walk.filled_notional > Decimal::ZERO {
                        walk.avg_fill_price
                    } else {
                        top_of_book_price
                    },
                    book_depth_levels: pred_book_depth,
                    fill_ratio: walk.fill_ratio(),
                },
                options_leg: LegInfo {
                    venue: Venue::Deribit,
                    instrument_id: prob.instrument_id.to_string(),
                    probability: options_prob,
                    executable_price: options_executable,
                    book_depth_levels: 0, // options don't have depth in our model
                    fill_ratio: Decimal::ONE,
                },
                timestamp: DualTimestamp::now(),
                ttl_secs: self.config.signal_ttl_secs,
                pricing_method: prob.method,
                confidence_components: prob.confidence_components.clone(),
                solver_meta: prob.solver_meta.clone(),
                iv_spread,
                skew_adjustment: prob.skew_adjustment,
                cost_breakdown: CostBreakdown {
                    prediction_fee,
                    options_fee_estimate,
                    carry_cost: carry,
                    prediction_slippage,
                    options_spread_cost,
                    basis_risk_premium,
                    liquidity_factor,
                    total_cost,
                },
                prediction_venue: pred_venue,
                threshold_status,
                threshold_value,
                threshold_components: Some(components),
            };

            // --- Log to JSONL (ALL signals, regardless of status) ---
            if let Err(e) = self.logger.log(&signal).await {
                tracing::warn!(error = %e, "failed to write signal log");
            }

            // --- Emit on channel only if PassedBoth ---
            if threshold_status == ThresholdStatus::PassedBoth {
                self.signal_count += 1;
                let _ = signal_tx.try_send(signal);
                metrics::counter!("arb_signals_emitted_total").increment(1);
            } else {
                self.filtered_count += 1;
                metrics::counter!("arb_signals_filtered_total").increment(1);
            }

            // --- Metrics for every computation ---
            metrics::histogram!("arb_signal_net_edge_bps").record(net_edge_f64 * 10000.0);
            metrics::histogram!("arb_signal_confidence").record(prob.confidence);
            metrics::counter!("arb_computations_total").increment(1);
        }

        // Record liveness timestamp for AlertMonitor (Phase 14).
        if let Some(ref liveness) = self.liveness {
            liveness.record_signal_eval();
        }
    }

    /// Emit periodic summary statistics at info level.
    fn emit_summary(&self) {
        let events_tracked = self.latest_prob.len();

        tracing::info!(
            events_tracked = events_tracked,
            signal_count = self.signal_count,
            filtered_count = self.filtered_count,
            "CrossAssetEngine summary"
        );

        // Per-event rolling stats
        for (event_id, stats) in &self.stats {
            let count = stats.count();
            if count == 0 {
                continue;
            }
            tracing::info!(
                event_id = event_id.as_str(),
                count = count,
                mean_edge = format!("{:.6}", stats.mean()),
                stddev = format!("{:.6}", stats.stddev()),
                "per-event rolling stats"
            );
        }

        // Prometheus gauges
        metrics::gauge!("arb_events_tracked").set(events_tracked as f64);
        metrics::gauge!("arb_signals_total").set(self.signal_count as f64);
        metrics::gauge!("arb_filtered_total").set(self.filtered_count as f64);
    }
}

/// Convert Decimal to f64 for metrics and rolling stats.
fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{
        ConfidenceComponents, ImpliedProbability, InstrumentGreeks, PricingMethod,
    };
    use crate::types::{InstrumentId, Notional, Price, Probability};
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn prob(s: &str) -> Probability {
        Probability::new(dec(s)).unwrap()
    }

    fn make_implied_probability(
        instrument: &str,
        probability: &str,
        prob_bid: Option<&str>,
        prob_ask: Option<&str>,
        confidence: f64,
        underlying_price: f64,
        delta: f64,
        stale: bool,
    ) -> ImpliedProbability {
        let ts = if stale {
            // Create a timestamp 60 seconds ago
            DualTimestamp {
                mono: tokio::time::Instant::now(),
                wall: chrono::Utc::now() - chrono::Duration::seconds(60),
            }
        } else {
            DualTimestamp::now()
        };

        ImpliedProbability {
            instrument_id: InstrumentId::new(instrument),
            probability: prob(probability),
            prob_bid: prob_bid.map(prob),
            prob_ask: prob_ask.map(prob),
            confidence,
            confidence_components: ConfidenceComponents {
                iv_spread: 0.9,
                book_depth: 0.85,
                method_agreement: 0.78,
                solver_convergence: 0.95,
            },
            method: PricingMethod::CallSpreadReplication,
            skew_adjustment: -0.01,
            greeks: InstrumentGreeks {
                delta,
                vega: 0.15,
                theta: -0.02,
            },
            solver_meta: None,
            epsilon_used: Some(5000.0),
            underlying_price,
            timestamp: ts,
            near_expiry: false,
            iv_spread: 0.0,
        }
    }

    fn make_prediction_snapshot(
        venue: Venue,
        bid_prob: Option<&str>,
        ask_prob: Option<&str>,
        depth_bids: Vec<(Price, Notional)>,
        depth_asks: Vec<(Price, Notional)>,
        stale: bool,
    ) -> MarketSnapshot {
        let ts = if stale {
            DualTimestamp {
                mono: tokio::time::Instant::now(),
                wall: chrono::Utc::now() - chrono::Duration::seconds(60),
            }
        } else {
            DualTimestamp::now()
        };

        MarketSnapshot {
            venue,
            instrument_id: InstrumentId::new("PRED-INST"),
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
            exchange_timestamp: if stale {
                Some(chrono::Utc::now().timestamp_millis() - 60_000)
            } else {
                Some(chrono::Utc::now().timestamp_millis())
            },
            timestamp: ts,
            sequence: 1,
            trace_id: crate::types::TraceId::new(),
            is_stale: false,
        }
    }

    fn default_config() -> SignalGenerationConfig {
        SignalGenerationConfig {
            options_staleness_ms: 30_000,
            polymarket_staleness_ms: 5_000,
            kalshi_staleness_ms: 15_000,
            ..SignalGenerationConfig::default()
        }
    }

    // --- Test 1: Staleness gate rejects stale options ---
    #[tokio::test]
    async fn staleness_gate_rejects_stale_options() {
        let config = default_config();
        let mut engine = CrossAssetEngine::new(config);
        let (signal_tx, mut signal_rx) = mpsc::channel::<ArbSignal>(16);

        let event_id = "test-stale-options".to_string();

        // Insert a STALE options probability (60s old, threshold is 30s)
        let stale_prob = make_implied_probability(
            "BTC-27JUN25-100000-C",
            "0.55",
            Some("0.53"),
            Some("0.57"),
            0.8,
            100000.0,
            0.55,
            true, // stale
        );
        engine.latest_prob.insert(event_id.clone(), stale_prob);

        // Insert a fresh prediction market snapshot
        let fresh_snap = make_prediction_snapshot(
            Venue::Polymarket,
            Some("0.45"),
            Some("0.47"),
            vec![(Price::new(dec("0.44")), Notional::new(dec("600")))],
            vec![(Price::new(dec("0.47")), Notional::new(dec("600")))],
            false,
        );
        engine
            .latest_pred
            .insert((event_id.clone(), Venue::Polymarket), fresh_snap);

        // Attempt computation -- should be rejected due to stale options
        engine
            .compute_and_emit(&event_id, Venue::Polymarket, &signal_tx)
            .await;

        // Should not produce any signal
        let result = signal_rx.try_recv();
        assert!(result.is_err(), "stale options should not produce a signal");
    }

    // --- Test 2: Both directions computed ---
    #[tokio::test]
    async fn both_directions_computed() {
        let config = SignalGenerationConfig {
            // Set low threshold so signals pass
            threshold: crate::spread::config::ThresholdConfig {
                static_floor: dec("0.0001"),
                k: dec("0"),
                cold_start_multiplier: dec("1"),
                liquidity_penalty_scale: dec("0"),
                min_samples: 1000, // force cold start
            },
            ..default_config()
        };

        let mut engine = CrossAssetEngine::new(config);
        let (signal_tx, _signal_rx) = mpsc::channel::<ArbSignal>(16);

        let event_id = "test-both-directions".to_string();

        // Options probability at 0.55
        let prob = make_implied_probability(
            "BTC-27JUN25-100000-C",
            "0.55",
            Some("0.53"),
            Some("0.57"),
            0.8,
            100000.0,
            0.55,
            false,
        );
        engine.latest_prob.insert(event_id.clone(), prob);

        // Prediction market at 0.45 bid / 0.47 ask (options_prob > pred_ask)
        let snap = make_prediction_snapshot(
            Venue::Polymarket,
            Some("0.45"),
            Some("0.47"),
            vec![(Price::new(dec("0.45")), Notional::new(dec("600")))],
            vec![(Price::new(dec("0.47")), Notional::new(dec("600")))],
            false,
        );
        engine
            .latest_pred
            .insert((event_id.clone(), Venue::Polymarket), snap);

        // Compute -- should process both directions
        engine
            .compute_and_emit(&event_id, Venue::Polymarket, &signal_tx)
            .await;

        // The BuyPredictionSellOptions direction has raw_spread = 0.55 - 0.47 = 0.08
        // The SellPredictionBuyOptions direction has raw_spread = 0.45 - 0.55 = -0.10
        // After costs, only the positive direction might pass threshold.
        // Both are computed and logged regardless.

        // We check that at least one signal was emitted or stats were updated
        // (both directions should have been processed in rolling stats)
        let stats = engine.stats.get("test-both-directions");
        assert!(stats.is_some(), "rolling stats should exist after computation");
        assert!(
            stats.unwrap().count() >= 2,
            "both directions should push to rolling stats"
        );
    }

    // --- Test 3: Threshold status classification ---
    #[tokio::test]
    async fn threshold_status_correct() {
        // Use zero fees and carry so the large raw spread survives costs
        let low_threshold_config = SignalGenerationConfig {
            threshold: crate::spread::config::ThresholdConfig {
                static_floor: dec("0.0001"),
                k: dec("0"),
                cold_start_multiplier: dec("1"),
                liquidity_penalty_scale: dec("0"),
                min_samples: 1000, // force cold start so threshold = floor * 1 = 0.0001
            },
            deribit_taker_fee_rate: Decimal::ZERO,
            carry: crate::spread::config::CarryConfig {
                annualized_rate: Decimal::ZERO,
                reference_holding_days: 0,
            },
            polymarket_fees: crate::spread::config::PolymarketFeeConfig {
                fee_rate: Decimal::ZERO,
                exponent: 2,
                flat_rate_override: None,
            },
            kalshi_fees: crate::spread::config::KalshiFeeConfig {
                taker_coefficient: Decimal::ZERO,
                use_ceiling: false,
            },
            ..default_config()
        };

        let mut engine = CrossAssetEngine::new(low_threshold_config);
        let (signal_tx, mut signal_rx) = mpsc::channel::<ArbSignal>(64);

        let event_id = "test-threshold-status".to_string();

        // Large spread: options=0.80, pred ask=0.30 -> raw spread = 0.50 (huge edge)
        // Use small underlying_price to keep options fee small
        let prob = make_implied_probability(
            "BTC-27JUN25-100000-C",
            "0.80",
            Some("0.79"),
            Some("0.81"),
            0.9,
            1.0, // small underlying so options_fee_estimate stays tiny
            0.8,
            false,
        );
        engine.latest_prob.insert(event_id.clone(), prob);

        let snap = make_prediction_snapshot(
            Venue::Polymarket,
            Some("0.28"),
            Some("0.30"),
            vec![(Price::new(dec("0.28")), Notional::new(dec("600")))],
            vec![(Price::new(dec("0.30")), Notional::new(dec("600")))],
            false,
        );
        engine
            .latest_pred
            .insert((event_id.clone(), Venue::Polymarket), snap);

        engine
            .compute_and_emit(&event_id, Venue::Polymarket, &signal_tx)
            .await;

        // Collect all emitted signals (PassedBoth only are emitted on channel)
        let mut signals = vec![];
        while let Ok(sig) = signal_rx.try_recv() {
            signals.push(sig);
        }

        // The BuyPrediction direction: raw_spread = 0.80 - 0.30 = 0.50
        // With zero fees, net_edge ~ 0.50 * liquidity_factor > 0.0001 -> PassedBoth
        let passed_both = signals
            .iter()
            .filter(|s| s.threshold_status == ThresholdStatus::PassedBoth)
            .count();
        assert!(
            passed_both > 0,
            "BuyPrediction direction with 0.50 raw spread should pass threshold"
        );

        // The SellPrediction direction: raw_spread = 0.28 - 0.80 = -0.52
        // This should be Filtered (negative net edge below static floor)
        // filtered_count tracks all non-PassedBoth computations
        assert!(
            engine.filtered_count > 0,
            "SellPrediction direction with -0.52 raw spread should be filtered"
        );

        // Verify the emitted signal has correct direction
        let buy_pred_signal = signals
            .iter()
            .find(|s| s.direction == ArbDirection::BuyPredictionSellOptions);
        assert!(
            buy_pred_signal.is_some(),
            "should emit BuyPredictionSellOptions signal"
        );
        let sig = buy_pred_signal.unwrap();
        assert!(sig.net_edge > Decimal::ZERO, "net edge should be positive");
        assert_eq!(sig.threshold_status, ThresholdStatus::PassedBoth);
    }
}
