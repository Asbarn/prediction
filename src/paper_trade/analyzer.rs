//! Signal analysis accumulator and enrichment engine.
//!
//! Provides keyed lifetime statistics (AccumulatorKey -> AccumulatorBucket) that
//! track hit rates, edge, convergence, false positive rates, and stale fill counts
//! across settled paper positions. SignalAnalyzer produces enriched
//! AnalysisSettlementRecords for JSONL logging and emits Prometheus gauges.
//!
//! Also provides FilteredSignalTracker for threshold effectiveness analysis:
//! tracks signals that did NOT pass the dynamic threshold (PassedStaticOnly or
//! Filtered) and correlates them with settlement outcomes to determine if
//! profitable signals were being filtered out.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::config::AnalysisConfig;
use crate::settlement::types::OutcomeKind;
use crate::signal::types::ThresholdStatus;
use crate::spread::patterns::SpreadPattern;

use super::position::PaperPosition;

// ---------------------------------------------------------------------------
// AccumulatorKey
// ---------------------------------------------------------------------------

/// Composite key for signal analysis accumulation.
///
/// Groups settlement statistics by venue pair, event, and threshold status
/// so that hit rates and edge can be broken down along these dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccumulatorKey {
    pub venue_pair: String,
    pub event_id: String,
    pub threshold_status: ThresholdStatus,
}

// ---------------------------------------------------------------------------
// AccumulatorBucket
// ---------------------------------------------------------------------------

/// Running counters for a single accumulation key.
///
/// Tracks everything needed to compute hit rate, net edge, convergence time,
/// false positive rate, and stale fill frequency.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccumulatorBucket {
    /// Total number of positions settled in this bucket.
    pub total_settled: u64,
    /// Positions with positive gross (pre-fee) P&L.
    pub gross_hits: u64,
    /// Positions with positive net (post-fee) P&L.
    pub net_hits: u64,
    /// Sum of gross P&L across all settled positions (for average edge).
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_gross_pnl: Decimal,
    /// Sum of net P&L across all settled positions (for average edge).
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_net_pnl: Decimal,
    /// Sum of total fees across all settled positions.
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_fees: Decimal,
    /// Sum of total slippage estimates across all settled positions.
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_slippage: Decimal,
    /// Sum of convergence time in seconds across all settled positions.
    pub sum_convergence_secs: f64,
    /// Number of positions flagged with stale fills.
    pub stale_fill_count: u64,
}

// ---------------------------------------------------------------------------
// AnalysisSettlementRecord
// ---------------------------------------------------------------------------

/// Enriched settlement record for JSONL logging.
///
/// Combines per-position data with running accumulator metrics at the time
/// of settlement, giving a complete snapshot for post-hoc analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSettlementRecord {
    pub event_id: String,
    pub position_id: String,
    pub venue_pair: String,
    pub pattern: String,
    pub threshold_status: Option<ThresholdStatus>,
    pub convergence_secs: f64,
    pub gross_hit: bool,
    pub net_hit: bool,
    pub total_raw_pnl: String,
    pub total_net_pnl: String,
    pub total_fees: String,
    pub total_slippage: String,
    pub inter_leg_gap_ms: Option<i64>,
    pub stale_fill: bool,
    // Running metrics at time of settlement
    pub running_gross_hit_rate: f64,
    pub running_net_hit_rate: f64,
    pub running_avg_net_edge: f64,
    pub running_false_positive_rate: f64,
    pub running_avg_convergence_secs: f64,
    pub settled_at_ms: i64,
}

// ---------------------------------------------------------------------------
// LifetimeSummary
// ---------------------------------------------------------------------------

/// Aggregate summary across all accumulator keys.
///
/// Used for periodic log line emission (e.g., daily summary).
#[derive(Debug, Clone)]
pub struct LifetimeSummary {
    pub total_settled: u64,
    pub gross_hit_rate: f64,
    pub net_hit_rate: f64,
    pub avg_net_edge: f64,
    pub false_positive_rate: f64,
    pub avg_convergence_secs: f64,
    pub stale_fill_count: u64,
}

// ---------------------------------------------------------------------------
// FilteredSignalEvent
// ---------------------------------------------------------------------------

/// Lightweight event sent from SpreadEngine for non-PassedBoth results.
///
/// Carries just enough data to correlate with settlement outcomes later,
/// without the full SpreadResult overhead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredSignalEvent {
    pub event_id: String,
    pub pattern: SpreadPattern,
    pub threshold_status: ThresholdStatus,
    #[serde(with = "rust_decimal::serde::str")]
    pub net_spread: Decimal,
    pub timestamp_ms: i64,
}

// ---------------------------------------------------------------------------
// FilteredSignalEntry
// ---------------------------------------------------------------------------

/// Stored entry for a filtered signal (without event_id, which is the map key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredSignalEntry {
    pub pattern: SpreadPattern,
    pub threshold_status: ThresholdStatus,
    #[serde(with = "rust_decimal::serde::str")]
    pub net_spread: Decimal,
    pub timestamp_ms: i64,
}

// ---------------------------------------------------------------------------
// FilteredCorrelation
// ---------------------------------------------------------------------------

/// Result of correlating a filtered signal with a settlement outcome.
///
/// Answers: "If this filtered signal had been acted upon, would it have been
/// profitable given the actual settlement outcome?"
#[derive(Debug, Clone, Serialize)]
pub struct FilteredCorrelation {
    pub threshold_status: ThresholdStatus,
    #[serde(with = "rust_decimal::serde::str")]
    pub net_spread: Decimal,
    pub hypothetical_hit: bool,
    pub timestamp_ms: i64,
}

// ---------------------------------------------------------------------------
// FilteredSignalTracker
// ---------------------------------------------------------------------------

/// Tracks filtered (non-PassedBoth) signals for threshold effectiveness analysis.
///
/// Stores signals keyed by event_id so that when settlement arrives, we can
/// correlate filtered signals with the actual outcome to determine if the
/// threshold was too aggressive (filtering out winners).
pub struct FilteredSignalTracker {
    /// Filtered signals keyed by event_id, capped per event.
    signals: HashMap<String, Vec<FilteredSignalEntry>>,
    /// Max entries per event_id to prevent unbounded growth.
    max_per_event: usize,
    /// Running counters for filtered signal correlations.
    filtered_total: u64,
    filtered_hits: u64,
}

impl FilteredSignalTracker {
    /// Create a new FilteredSignalTracker with the given per-event cap.
    pub fn new(max_per_event: usize) -> Self {
        Self {
            signals: HashMap::new(),
            max_per_event,
            filtered_total: 0,
            filtered_hits: 0,
        }
    }

    /// Record a filtered signal event.
    ///
    /// If the event already has `max_per_event` entries, the oldest is removed.
    pub fn record(&mut self, event: FilteredSignalEvent) {
        let entries = self.signals.entry(event.event_id).or_default();
        if entries.len() >= self.max_per_event {
            entries.remove(0);
        }
        entries.push(FilteredSignalEntry {
            pattern: event.pattern,
            threshold_status: event.threshold_status,
            net_spread: event.net_spread,
            timestamp_ms: event.timestamp_ms,
        });
    }

    /// Correlate filtered signals for an event with its settlement outcome.
    ///
    /// For each filtered signal, determines whether acting on it would have
    /// been profitable given the actual outcome. Uses pattern direction to
    /// determine hypothetical profitability:
    /// - BuyPolyYesSellKalshiYes: profits if outcome is Yes (buy YES pays off)
    /// - SellPolyYesBuyKalshiYes: profits if outcome is No (sell YES pays off)
    /// - BuyPolyNoSellKalshiNo: profits if outcome is No (buy NO pays off)
    /// - SellPolyNoBuyKalshiNo: profits if outcome is Yes (sell NO pays off)
    pub fn correlate_with_settlement(
        &mut self,
        event_id: &str,
        outcome: &OutcomeKind,
    ) -> Vec<FilteredCorrelation> {
        let entries = match self.signals.get(event_id) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let mut correlations = Vec::with_capacity(entries.len());

        for entry in entries {
            let hypothetical_hit = match outcome {
                OutcomeKind::Yes => matches!(
                    entry.pattern,
                    SpreadPattern::BuyPolyYesSellKalshiYes
                        | SpreadPattern::SellPolyNoBuyKalshiNo
                ),
                OutcomeKind::No => matches!(
                    entry.pattern,
                    SpreadPattern::SellPolyYesBuyKalshiYes
                        | SpreadPattern::BuyPolyNoSellKalshiNo
                ),
                OutcomeKind::Ambiguous { .. } => {
                    // For ambiguous outcomes, use net_spread as proxy: positive = hit
                    entry.net_spread > Decimal::ZERO
                }
                OutcomeKind::Timeout => false,
            };

            // Update running counters
            self.filtered_total += 1;
            if hypothetical_hit {
                self.filtered_hits += 1;
            }

            correlations.push(FilteredCorrelation {
                threshold_status: entry.threshold_status,
                net_spread: entry.net_spread,
                hypothetical_hit,
                timestamp_ms: entry.timestamp_ms,
            });
        }

        correlations
    }

    /// Remove all filtered signals for an event (cleanup after settlement).
    pub fn remove_event(&mut self, event_id: &str) {
        self.signals.remove(event_id);
    }

    /// Export the filtered signal state for checkpoint persistence.
    pub fn export_state(&self) -> HashMap<String, Vec<FilteredSignalEntry>> {
        self.signals.clone()
    }

    /// Import filtered signal state from a checkpoint.
    pub fn import_state(&mut self, state: HashMap<String, Vec<FilteredSignalEntry>>) {
        self.signals = state;
    }

    /// Get the hypothetical hit rate for filtered signals.
    pub fn hypothetical_hit_rate(&self) -> f64 {
        if self.filtered_total == 0 {
            0.0
        } else {
            self.filtered_hits as f64 / self.filtered_total as f64
        }
    }
}

// ---------------------------------------------------------------------------
// SignalAnalyzer
// ---------------------------------------------------------------------------

/// Core signal analysis engine.
///
/// Maintains per-key accumulators that are updated on each settlement.
/// Produces enriched records for JSONL, emits Prometheus gauges, and supports
/// checkpoint export/import for state persistence. Also tracks filtered signals
/// for threshold effectiveness analysis.
pub struct SignalAnalyzer {
    accumulators: HashMap<AccumulatorKey, AccumulatorBucket>,
    filtered_tracker: FilteredSignalTracker,
    config: AnalysisConfig,
}

impl SignalAnalyzer {
    /// Create a new SignalAnalyzer with the given configuration.
    pub fn new(config: AnalysisConfig) -> Self {
        Self {
            accumulators: HashMap::new(),
            filtered_tracker: FilteredSignalTracker::new(100),
            config,
        }
    }

    /// Record a settled position and return an enriched analysis record.
    ///
    /// Updates the accumulator bucket for the position's key, then computes
    /// running rates from the updated bucket.
    pub fn record_settlement(&mut self, pos: &PaperPosition) -> AnalysisSettlementRecord {
        let venue_pair = pos.pattern.venue_pair_label().to_string();
        let threshold_status = pos.threshold_status.unwrap_or(ThresholdStatus::PassedBoth);

        // Compute P&L components from settled legs
        let total_raw_pnl: Decimal = pos.settled_legs.iter().map(|l| l.raw_pnl).sum();
        let total_net_pnl: Decimal = pos.settled_legs.iter().map(|l| l.net_pnl).sum();
        let total_fees: Decimal = pos
            .settled_legs
            .iter()
            .map(|l| l.entry_fee + l.exit_fee)
            .sum();
        let total_slippage: Decimal = pos
            .settled_legs
            .iter()
            .map(|l| l.slippage_estimate)
            .sum();

        // Compute convergence time
        let settled_at_ms = pos.settled_at_ms.unwrap_or(0);
        let convergence_secs = if settled_at_ms > 0 && pos.signal_timestamp_ms > 0 {
            (settled_at_ms - pos.signal_timestamp_ms) as f64 / 1000.0
        } else {
            0.0
        };

        let gross_hit = total_raw_pnl > Decimal::ZERO;
        let net_hit = total_net_pnl > Decimal::ZERO;

        // Build accumulator key and update bucket
        let key = AccumulatorKey {
            venue_pair: venue_pair.clone(),
            event_id: pos.event_id.clone(),
            threshold_status,
        };

        let bucket = self.accumulators.entry(key).or_default();
        bucket.total_settled += 1;
        if gross_hit {
            bucket.gross_hits += 1;
        }
        if net_hit {
            bucket.net_hits += 1;
        }
        bucket.sum_gross_pnl += total_raw_pnl;
        bucket.sum_net_pnl += total_net_pnl;
        bucket.sum_fees += total_fees;
        bucket.sum_slippage += total_slippage;
        bucket.sum_convergence_secs += convergence_secs;
        if pos.stale_fill {
            bucket.stale_fill_count += 1;
        }

        // Compute running rates from UPDATED bucket
        let running_gross_hit_rate = safe_rate(bucket.gross_hits, bucket.total_settled);
        let running_net_hit_rate = safe_rate(bucket.net_hits, bucket.total_settled);
        let running_avg_net_edge = if bucket.total_settled > 0 {
            bucket
                .sum_net_pnl
                .to_f64()
                .unwrap_or(0.0)
                / bucket.total_settled as f64
        } else {
            0.0
        };
        // False positive rate: gross_hit but not net_hit (fees ate the edge)
        let false_positives = bucket.gross_hits.saturating_sub(bucket.net_hits);
        let running_false_positive_rate = safe_rate(false_positives, bucket.total_settled);
        let running_avg_convergence_secs = if bucket.total_settled > 0 {
            bucket.sum_convergence_secs / bucket.total_settled as f64
        } else {
            0.0
        };

        AnalysisSettlementRecord {
            event_id: pos.event_id.clone(),
            position_id: pos.id.clone(),
            venue_pair,
            pattern: format!("{:?}", pos.pattern),
            threshold_status: pos.threshold_status,
            convergence_secs,
            gross_hit,
            net_hit,
            total_raw_pnl: total_raw_pnl.to_string(),
            total_net_pnl: total_net_pnl.to_string(),
            total_fees: total_fees.to_string(),
            total_slippage: total_slippage.to_string(),
            inter_leg_gap_ms: pos.inter_leg_gap_ms,
            stale_fill: pos.stale_fill,
            running_gross_hit_rate,
            running_net_hit_rate,
            running_avg_net_edge,
            running_false_positive_rate,
            running_avg_convergence_secs,
            settled_at_ms,
        }
    }

    /// Emit Prometheus gauges for all accumulator buckets.
    ///
    /// Iterates all keys, computes rates from each bucket, and emits gauges
    /// with venue_pair, event_id, and threshold_status labels.
    pub fn emit_prometheus_gauges(&self) {
        for (key, bucket) in &self.accumulators {
            if bucket.total_settled == 0 {
                continue;
            }

            let vp = key.venue_pair.clone();
            let eid = key.event_id.clone();
            let ts = format!("{:?}", key.threshold_status);

            let gross_hit_rate = safe_rate(bucket.gross_hits, bucket.total_settled);
            let net_hit_rate = safe_rate(bucket.net_hits, bucket.total_settled);
            let false_positives = bucket.gross_hits.saturating_sub(bucket.net_hits);
            let fp_rate = safe_rate(false_positives, bucket.total_settled);
            let avg_net_edge = bucket
                .sum_net_pnl
                .to_f64()
                .unwrap_or(0.0)
                / bucket.total_settled as f64;
            let avg_convergence = bucket.sum_convergence_secs / bucket.total_settled as f64;

            metrics::gauge!(
                "signal_analysis_gross_hit_rate",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(gross_hit_rate);

            metrics::gauge!(
                "signal_analysis_net_hit_rate",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(net_hit_rate);

            metrics::gauge!(
                "signal_analysis_false_positive_rate",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(fp_rate);

            metrics::gauge!(
                "signal_analysis_avg_net_edge",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(avg_net_edge);

            metrics::gauge!(
                "signal_analysis_avg_convergence_secs",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(avg_convergence);

            metrics::gauge!(
                "signal_analysis_total_settled",
                "venue_pair" => vp.clone(),
                "event_id" => eid.clone(),
                "threshold_status" => ts.clone()
            )
            .set(bucket.total_settled as f64);

            metrics::gauge!(
                "signal_analysis_stale_fill_count",
                "venue_pair" => vp,
                "event_id" => eid,
                "threshold_status" => ts
            )
            .set(bucket.stale_fill_count as f64);
        }

        // Emit filtered signal hypothetical hit rate gauge
        let filtered_hit_rate = self.filtered_tracker.hypothetical_hit_rate();
        metrics::gauge!("signal_analysis_filtered_hypothetical_hit_rate")
            .set(filtered_hit_rate);
    }

    /// Record a filtered signal event (delegates to FilteredSignalTracker).
    pub fn record_filtered_signal(&mut self, event: FilteredSignalEvent) {
        self.filtered_tracker.record(event);
    }

    /// Correlate filtered signals with a settlement outcome.
    ///
    /// Returns correlation results and updates threshold effectiveness
    /// accumulators for the Filtered/PassedStaticOnly categories.
    pub fn correlate_filtered_with_settlement(
        &mut self,
        event_id: &str,
        outcome: &OutcomeKind,
    ) -> Vec<FilteredCorrelation> {
        let correlations = self.filtered_tracker.correlate_with_settlement(event_id, outcome);

        // Clean up after correlation
        self.filtered_tracker.remove_event(event_id);

        correlations
    }

    /// Export accumulator state for checkpoint persistence.
    pub fn export_state(&self) -> HashMap<AccumulatorKey, AccumulatorBucket> {
        self.accumulators.clone()
    }

    /// Export filtered signal tracker state for checkpoint persistence.
    pub fn export_filtered_state(&self) -> HashMap<String, Vec<FilteredSignalEntry>> {
        self.filtered_tracker.export_state()
    }

    /// Import accumulator state from a checkpoint, replacing current state.
    pub fn import_state(&mut self, state: HashMap<AccumulatorKey, AccumulatorBucket>) {
        self.accumulators = state;
    }

    /// Import filtered signal tracker state from a checkpoint.
    pub fn import_filtered_state(&mut self, state: HashMap<String, Vec<FilteredSignalEntry>>) {
        self.filtered_tracker.import_state(state);
    }

    /// Compute aggregate lifetime summary across all accumulator keys.
    ///
    /// Used for periodic (e.g., daily) log line emission.
    pub fn lifetime_summary(&self) -> LifetimeSummary {
        let mut total_settled: u64 = 0;
        let mut total_gross_hits: u64 = 0;
        let mut total_net_hits: u64 = 0;
        let mut total_net_pnl = Decimal::ZERO;
        let mut total_convergence_secs: f64 = 0.0;
        let mut total_stale_fills: u64 = 0;

        for bucket in self.accumulators.values() {
            total_settled += bucket.total_settled;
            total_gross_hits += bucket.gross_hits;
            total_net_hits += bucket.net_hits;
            total_net_pnl += bucket.sum_net_pnl;
            total_convergence_secs += bucket.sum_convergence_secs;
            total_stale_fills += bucket.stale_fill_count;
        }

        let gross_hit_rate = safe_rate(total_gross_hits, total_settled);
        let net_hit_rate = safe_rate(total_net_hits, total_settled);
        let avg_net_edge = if total_settled > 0 {
            total_net_pnl.to_f64().unwrap_or(0.0) / total_settled as f64
        } else {
            0.0
        };
        let false_positives = total_gross_hits.saturating_sub(total_net_hits);
        let false_positive_rate = safe_rate(false_positives, total_settled);
        let avg_convergence_secs = if total_settled > 0 {
            total_convergence_secs / total_settled as f64
        } else {
            0.0
        };

        LifetimeSummary {
            total_settled,
            gross_hit_rate,
            net_hit_rate,
            avg_net_edge,
            false_positive_rate,
            avg_convergence_secs,
            stale_fill_count: total_stale_fills,
        }
    }

    /// Access to the analysis config (e.g., for stale fill threshold).
    pub fn config(&self) -> &AnalysisConfig {
        &self.config
    }
}

/// Safe division for rate computation: returns 0.0 when denominator is zero.
fn safe_rate(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AnalysisConfig;
    use crate::paper_trade::position::{PaperPosition, PositionStatus};
    use crate::settlement::types::{OutcomeKind, ResolutionSource, SettledLeg};
    use crate::signal::types::ThresholdStatus;
    use crate::spread::patterns::{SpreadPattern, SpreadResult};
    use crate::types::Venue;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn default_config() -> AnalysisConfig {
        AnalysisConfig {
            enabled: true,
            max_leg_fill_gap_ms: 2000,
        }
    }

    fn make_signal(event_id: &str) -> SpreadResult {
        SpreadResult {
            event_id: event_id.to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            gross_spread: dec("0.05"),
            net_spread: dec("0.03"),
            buy_fill_price: dec("0.45"),
            sell_fill_price: dec("0.50"),
            buy_fee: dec("0.005"),
            sell_fee: dec("0.007"),
            carry_cost: dec("0.002"),
            total_cost: dec("0.014"),
            basis_risk_premium: dec("0"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("0.95"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: Some(1700000000100),
            kalshi_exchange_ts: Some(1700000000200),
            threshold: Some(dec("0.025")),
            threshold_components: None,
            threshold_status: Some(ThresholdStatus::PassedBoth),
        }
    }

    fn make_settled_leg(
        venue: Venue,
        raw_pnl: Decimal,
        fees: Decimal,
        slippage: Decimal,
    ) -> SettledLeg {
        SettledLeg {
            venue,
            outcome: OutcomeKind::Yes,
            raw_pnl,
            entry_fee: fees,
            exit_fee: Decimal::ZERO,
            slippage_estimate: slippage,
            net_pnl: raw_pnl - fees - slippage,
            fee_model_version: "v1.0".to_string(),
            resolved_at: Utc::now(),
            detected_at: Utc::now(),
            resolution_source: ResolutionSource::DeribitDelivery,
        }
    }

    fn make_settled_position(
        event_id: &str,
        raw_pnl_leg1: &str,
        raw_pnl_leg2: &str,
        fees: &str,
        settled_at_ms: i64,
    ) -> PaperPosition {
        let signal = make_signal(event_id);
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg1 = make_settled_leg(Venue::Polymarket, dec(raw_pnl_leg1), dec(fees), Decimal::ZERO);
        let leg2 = make_settled_leg(Venue::Kalshi, dec(raw_pnl_leg2), dec(fees), Decimal::ZERO);
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);

        pos.settlement_pnl = Some(
            pos.settled_legs.iter().map(|l| l.net_pnl).sum(),
        );
        pos.settled_at_ms = Some(settled_at_ms);
        pos.status = PositionStatus::Settled;
        pos
    }

    // -- AccumulatorKey tests --

    #[test]
    fn accumulator_key_hash_and_equality() {
        let key1 = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-1".to_string(),
            threshold_status: ThresholdStatus::PassedBoth,
        };
        let key2 = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-1".to_string(),
            threshold_status: ThresholdStatus::PassedBoth,
        };
        let key3 = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-1".to_string(),
            threshold_status: ThresholdStatus::Filtered,
        };

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);

        // Verify they work as HashMap keys
        let mut map = HashMap::new();
        map.insert(key1.clone(), 1);
        map.insert(key2, 2); // should overwrite
        map.insert(key3, 3);

        assert_eq!(map.len(), 2);
        assert_eq!(map[&key1], 2);
    }

    // -- record_settlement tests --

    #[test]
    fn record_settlement_updates_bucket_correctly() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Winning position: raw_pnl = 50 + (-10) = 40, net_pnl = (50-2) + (-10-2) = 36
        let pos = make_settled_position("evt-1", "50", "-10", "2", 1700000010000);
        let record = analyzer.record_settlement(&pos);

        assert!(record.gross_hit); // raw_pnl = 40 > 0
        assert!(record.net_hit); // net_pnl = 36 > 0
        assert_eq!(record.event_id, "evt-1");
        assert_eq!(record.venue_pair, "kalshi_polymarket");
        assert_eq!(record.settled_at_ms, 1700000010000);

        // Check bucket
        let key = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-1".to_string(),
            threshold_status: ThresholdStatus::PassedBoth,
        };
        let bucket = &analyzer.accumulators[&key];
        assert_eq!(bucket.total_settled, 1);
        assert_eq!(bucket.gross_hits, 1);
        assert_eq!(bucket.net_hits, 1);
        assert_eq!(bucket.sum_gross_pnl, dec("40")); // 50 + (-10)
        assert_eq!(bucket.sum_net_pnl, dec("36")); // (50-2) + (-10-2)
        assert_eq!(bucket.sum_fees, dec("4")); // 2 + 2
    }

    #[test]
    fn running_rates_computed_correctly() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Two winners
        let pos1 = make_settled_position("evt-1", "50", "10", "2", 1700000010000);
        let pos2 = make_settled_position("evt-1", "30", "5", "1", 1700000020000);
        analyzer.record_settlement(&pos1);
        let record2 = analyzer.record_settlement(&pos2);

        // 2 gross hits out of 2, 2 net hits out of 2
        assert_eq!(record2.running_gross_hit_rate, 1.0);
        assert_eq!(record2.running_net_hit_rate, 1.0);
        assert_eq!(record2.running_false_positive_rate, 0.0);
    }

    #[test]
    fn division_by_zero_guard_when_no_settlements() {
        let analyzer = SignalAnalyzer::new(default_config());
        let summary = analyzer.lifetime_summary();

        assert_eq!(summary.total_settled, 0);
        assert_eq!(summary.gross_hit_rate, 0.0);
        assert_eq!(summary.net_hit_rate, 0.0);
        assert_eq!(summary.avg_net_edge, 0.0);
        assert_eq!(summary.false_positive_rate, 0.0);
        assert_eq!(summary.avg_convergence_secs, 0.0);
    }

    #[test]
    fn stale_fill_count_increments() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        let signal = make_signal("evt-stale");
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos.stale_fill = true; // manually flag

        let leg1 = make_settled_leg(Venue::Polymarket, dec("50"), dec("2"), Decimal::ZERO);
        let leg2 = make_settled_leg(Venue::Kalshi, dec("10"), dec("1"), Decimal::ZERO);
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);
        pos.settlement_pnl = Some(dec("57")); // (50-2)+(10-1)
        pos.settled_at_ms = Some(1700000010000);
        pos.status = PositionStatus::Settled;

        analyzer.record_settlement(&pos);

        let key = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-stale".to_string(),
            threshold_status: ThresholdStatus::PassedBoth,
        };
        assert_eq!(analyzer.accumulators[&key].stale_fill_count, 1);
    }

    #[test]
    fn convergence_secs_computed_from_timestamps() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // signal_timestamp_ms = 1700000000000, settled_at_ms = 1700000010000
        // convergence = (10000) / 1000.0 = 10.0 seconds
        let pos = make_settled_position("evt-conv", "50", "10", "2", 1700000010000);
        let record = analyzer.record_settlement(&pos);

        assert!((record.convergence_secs - 10.0).abs() < 0.001);
    }

    #[test]
    fn export_import_state_roundtrip() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        let pos1 = make_settled_position("evt-1", "50", "10", "2", 1700000010000);
        let pos2 = make_settled_position("evt-2", "30", "-20", "1", 1700000020000);
        analyzer.record_settlement(&pos1);
        analyzer.record_settlement(&pos2);

        // Export
        let state = analyzer.export_state();
        assert_eq!(state.len(), 2); // two different event_ids

        // Import into fresh analyzer
        let mut analyzer2 = SignalAnalyzer::new(default_config());
        analyzer2.import_state(state.clone());

        let state2 = analyzer2.export_state();
        assert_eq!(state.len(), state2.len());

        // Verify bucket data survived the roundtrip
        for (key, bucket) in &state {
            let bucket2 = &state2[key];
            assert_eq!(bucket.total_settled, bucket2.total_settled);
            assert_eq!(bucket.gross_hits, bucket2.gross_hits);
            assert_eq!(bucket.net_hits, bucket2.net_hits);
            assert_eq!(bucket.sum_gross_pnl, bucket2.sum_gross_pnl);
            assert_eq!(bucket.sum_net_pnl, bucket2.sum_net_pnl);
        }
    }

    #[test]
    fn lifetime_summary_aggregates_across_multiple_keys() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Two events, each with one settlement
        let pos1 = make_settled_position("evt-1", "50", "10", "2", 1700000010000);
        let pos2 = make_settled_position("evt-2", "-30", "-20", "1", 1700000020000);
        analyzer.record_settlement(&pos1);
        analyzer.record_settlement(&pos2);

        let summary = analyzer.lifetime_summary();
        assert_eq!(summary.total_settled, 2);
        // pos1: gross_pnl = 60, net_pnl = 56 -> gross_hit, net_hit
        // pos2: gross_pnl = -50, net_pnl = -52 -> no hit
        assert_eq!(summary.gross_hit_rate, 0.5); // 1/2
        assert_eq!(summary.net_hit_rate, 0.5); // 1/2
        assert_eq!(summary.stale_fill_count, 0);
    }

    #[test]
    fn false_positive_tracking() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Position with gross profit but net loss (fees ate the edge)
        // raw_pnl: 5 + (-2) = 3 > 0 (gross hit)
        // net_pnl: (5-4) + (-2-4) = 1 + (-6) = -5 < 0 (net miss)
        let signal = make_signal("evt-fp");
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg1 = make_settled_leg(Venue::Polymarket, dec("5"), dec("4"), Decimal::ZERO);
        let leg2 = make_settled_leg(Venue::Kalshi, dec("-2"), dec("4"), Decimal::ZERO);
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);
        pos.settlement_pnl = Some(dec("-5"));
        pos.settled_at_ms = Some(1700000010000);
        pos.status = PositionStatus::Settled;

        let record = analyzer.record_settlement(&pos);
        assert!(record.gross_hit); // gross pnl = 3 > 0
        assert!(!record.net_hit); // net pnl = -5 < 0

        // False positive rate should be 1/1 = 100%
        assert_eq!(record.running_false_positive_rate, 1.0);
    }

    #[test]
    fn accumulator_key_serde_roundtrip() {
        let key = AccumulatorKey {
            venue_pair: "kalshi_polymarket".to_string(),
            event_id: "evt-1".to_string(),
            threshold_status: ThresholdStatus::PassedStaticOnly,
        };

        let json = serde_json::to_string(&key).unwrap();
        let parsed: AccumulatorKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, parsed);
    }

    #[test]
    fn accumulator_bucket_serde_roundtrip() {
        let bucket = AccumulatorBucket {
            total_settled: 10,
            gross_hits: 7,
            net_hits: 5,
            sum_gross_pnl: dec("150.5"),
            sum_net_pnl: dec("120.3"),
            sum_fees: dec("30.2"),
            sum_slippage: dec("5.1"),
            sum_convergence_secs: 85.5,
            stale_fill_count: 2,
        };

        let json = serde_json::to_string(&bucket).unwrap();
        let parsed: AccumulatorBucket = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_settled, 10);
        assert_eq!(parsed.gross_hits, 7);
        assert_eq!(parsed.sum_gross_pnl, dec("150.5"));
    }

    #[test]
    fn emit_prometheus_gauges_skips_empty_buckets() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Insert an empty bucket manually
        analyzer.accumulators.insert(
            AccumulatorKey {
                venue_pair: "test".to_string(),
                event_id: "evt-empty".to_string(),
                threshold_status: ThresholdStatus::Filtered,
            },
            AccumulatorBucket::default(),
        );

        // Should not panic when iterating
        analyzer.emit_prometheus_gauges();
    }

    #[test]
    fn record_settlement_with_no_settled_legs() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Position with no legs (edge case)
        let signal = make_signal("evt-empty");
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos.settled_at_ms = Some(1700000010000);
        pos.status = PositionStatus::Settled;

        let record = analyzer.record_settlement(&pos);
        assert!(!record.gross_hit);
        assert!(!record.net_hit);
        assert_eq!(record.total_raw_pnl, "0");
        assert_eq!(record.total_net_pnl, "0");
    }

    // -- FilteredSignalTracker tests --

    #[test]
    fn filtered_tracker_record_and_cap() {
        let mut tracker = FilteredSignalTracker::new(3);

        // Record 4 entries for the same event -- oldest should be evicted
        for i in 0..4 {
            tracker.record(FilteredSignalEvent {
                event_id: "evt-1".to_string(),
                pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
                threshold_status: ThresholdStatus::Filtered,
                net_spread: dec("0.01"),
                timestamp_ms: 1700000000000 + i * 1000,
            });
        }

        let entries = &tracker.signals["evt-1"];
        assert_eq!(entries.len(), 3, "should cap at max_per_event=3");
        // Oldest (timestamp 0) should have been evicted, earliest remaining is 1000
        assert_eq!(entries[0].timestamp_ms, 1700000001000);
    }

    #[test]
    fn filtered_tracker_correlate_yes_outcome() {
        let mut tracker = FilteredSignalTracker::new(100);

        // BuyPolyYesSellKalshiYes profits from Yes outcome
        tracker.record(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::PassedStaticOnly,
            net_spread: dec("0.02"),
            timestamp_ms: 1700000000000,
        });

        // SellPolyYesBuyKalshiYes does NOT profit from Yes
        tracker.record(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::SellPolyYesBuyKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.01"),
            timestamp_ms: 1700000001000,
        });

        let correlations = tracker.correlate_with_settlement("evt-1", &OutcomeKind::Yes);
        assert_eq!(correlations.len(), 2);
        assert!(correlations[0].hypothetical_hit); // BuyPolyYes profits from Yes
        assert!(!correlations[1].hypothetical_hit); // SellPolyYes does not profit from Yes
    }

    #[test]
    fn filtered_tracker_correlate_no_outcome() {
        let mut tracker = FilteredSignalTracker::new(100);

        // SellPolyYesBuyKalshiYes profits from No outcome
        tracker.record(FilteredSignalEvent {
            event_id: "evt-2".to_string(),
            pattern: SpreadPattern::SellPolyYesBuyKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.015"),
            timestamp_ms: 1700000000000,
        });

        // BuyPolyNoSellKalshiNo profits from No outcome
        tracker.record(FilteredSignalEvent {
            event_id: "evt-2".to_string(),
            pattern: SpreadPattern::BuyPolyNoSellKalshiNo,
            threshold_status: ThresholdStatus::PassedStaticOnly,
            net_spread: dec("0.02"),
            timestamp_ms: 1700000001000,
        });

        let correlations = tracker.correlate_with_settlement("evt-2", &OutcomeKind::No);
        assert_eq!(correlations.len(), 2);
        assert!(correlations[0].hypothetical_hit); // SellPolyYes profits from No
        assert!(correlations[1].hypothetical_hit); // BuyPolyNo profits from No
    }

    #[test]
    fn filtered_tracker_correlate_timeout_all_miss() {
        let mut tracker = FilteredSignalTracker::new(100);

        tracker.record(FilteredSignalEvent {
            event_id: "evt-3".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.02"),
            timestamp_ms: 1700000000000,
        });

        let correlations = tracker.correlate_with_settlement("evt-3", &OutcomeKind::Timeout);
        assert_eq!(correlations.len(), 1);
        assert!(!correlations[0].hypothetical_hit); // Timeout = always miss
    }

    #[test]
    fn filtered_tracker_remove_event_cleans_up() {
        let mut tracker = FilteredSignalTracker::new(100);

        tracker.record(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.01"),
            timestamp_ms: 1700000000000,
        });

        assert!(tracker.signals.contains_key("evt-1"));
        tracker.remove_event("evt-1");
        assert!(!tracker.signals.contains_key("evt-1"));
    }

    #[test]
    fn filtered_tracker_export_import_roundtrip() {
        let mut tracker = FilteredSignalTracker::new(100);

        tracker.record(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.01"),
            timestamp_ms: 1700000000000,
        });
        tracker.record(FilteredSignalEvent {
            event_id: "evt-2".to_string(),
            pattern: SpreadPattern::SellPolyYesBuyKalshiYes,
            threshold_status: ThresholdStatus::PassedStaticOnly,
            net_spread: dec("0.02"),
            timestamp_ms: 1700000001000,
        });

        let state = tracker.export_state();
        assert_eq!(state.len(), 2);

        let mut tracker2 = FilteredSignalTracker::new(100);
        tracker2.import_state(state.clone());

        let state2 = tracker2.export_state();
        assert_eq!(state.len(), state2.len());
        assert_eq!(
            state["evt-1"][0].net_spread,
            state2["evt-1"][0].net_spread
        );
        assert_eq!(
            state["evt-2"][0].threshold_status,
            state2["evt-2"][0].threshold_status
        );
    }

    #[test]
    fn filtered_tracker_correlate_nonexistent_event() {
        let mut tracker = FilteredSignalTracker::new(100);

        // Correlating with a non-existent event should return empty
        let correlations =
            tracker.correlate_with_settlement("does-not-exist", &OutcomeKind::Yes);
        assert!(correlations.is_empty());
    }

    #[test]
    fn analyzer_record_and_correlate_filtered_signal() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        // Record a filtered signal through the analyzer
        analyzer.record_filtered_signal(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::PassedStaticOnly,
            net_spread: dec("0.02"),
            timestamp_ms: 1700000000000,
        });

        // Correlate through the analyzer
        let correlations =
            analyzer.correlate_filtered_with_settlement("evt-1", &OutcomeKind::Yes);
        assert_eq!(correlations.len(), 1);
        assert!(correlations[0].hypothetical_hit);

        // After correlation, the event should be cleaned up
        let correlations2 =
            analyzer.correlate_filtered_with_settlement("evt-1", &OutcomeKind::Yes);
        assert!(correlations2.is_empty());
    }

    #[test]
    fn analyzer_export_import_filtered_state() {
        let mut analyzer = SignalAnalyzer::new(default_config());

        analyzer.record_filtered_signal(FilteredSignalEvent {
            event_id: "evt-1".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            threshold_status: ThresholdStatus::Filtered,
            net_spread: dec("0.015"),
            timestamp_ms: 1700000000000,
        });

        let state = analyzer.export_filtered_state();
        assert_eq!(state.len(), 1);

        let mut analyzer2 = SignalAnalyzer::new(default_config());
        analyzer2.import_filtered_state(state);

        let state2 = analyzer2.export_filtered_state();
        assert_eq!(state2.len(), 1);
        assert_eq!(state2["evt-1"][0].net_spread, dec("0.015"));
    }
}
