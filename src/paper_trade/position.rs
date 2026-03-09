//! Paper trade position lifecycle: Pending -> Open -> PartiallySettled -> Settled.
//!
//! Tracks hypothetical positions from signal through fill to settlement,
//! recording adverse selection (signal vs fill spread difference), mark-to-market
//! history, per-leg settlement P&L, and cross-venue divergence annotations.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::settlement::types::{
    DivergenceType, OutcomeKind, SettledLeg, SettlementDivergence,
};
use crate::signal::types::ThresholdStatus;
use crate::spread::patterns::{SpreadPattern, SpreadResult};

/// Position lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    /// Signal received, awaiting next-tick fill.
    Pending,
    /// Position filled, tracking MTM.
    Open,
    /// Some venue legs settled, waiting for remaining.
    PartiallySettled,
    /// Position closed with final P&L.
    Settled,
}

/// A hypothetical paper trade position.
///
/// Tracks the full lifecycle from signal (Pending) through fill (Open)
/// to settlement (Settled), capturing adverse selection, MTM history,
/// and final P&L.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperPosition {
    /// Unique trade identifier (sequential).
    pub id: String,
    /// The mapped event ID linking both venues.
    pub event_id: String,
    /// Which directional pattern triggered this trade.
    pub pattern: SpreadPattern,
    /// Current lifecycle status.
    pub status: PositionStatus,
    /// Fixed notional per trade (from config).
    #[serde(with = "rust_decimal::serde::str")]
    pub notional: Decimal,
    /// Net spread at signal time.
    #[serde(with = "rust_decimal::serde::str")]
    pub signal_spread: Decimal,
    /// Timestamp when the signal fired (milliseconds since epoch).
    pub signal_timestamp_ms: i64,
    /// Fill price on the buy side (set on next-tick fill).
    pub entry_price_buy: Option<Decimal>,
    /// Fill price on the sell side (set on next-tick fill).
    pub entry_price_sell: Option<Decimal>,
    /// Timestamp when the position was filled.
    pub entry_timestamp_ms: Option<i64>,
    /// Adverse selection: entry spread minus signal spread.
    /// Positive means fill was worse than signal.
    pub adverse_selection: Option<Decimal>,
    /// Mark-to-market snapshots over position lifetime.
    pub mtm_history: Vec<MtmSnapshot>,
    /// Hold-to-settlement P&L (if settled).
    pub settlement_pnl: Option<Decimal>,
    /// Timestamp when the position was settled.
    pub settled_at_ms: Option<i64>,
    /// Settled venue legs with per-leg P&L breakdown.
    #[serde(default)]
    pub settled_legs: Vec<SettledLeg>,
    /// Cross-venue divergence annotation (populated when all legs settle).
    #[serde(default)]
    pub divergence: Option<SettlementDivergence>,
    /// Threshold evaluation status from the originating SpreadResult signal.
    #[serde(default)]
    pub threshold_status: Option<ThresholdStatus>,
    /// Inter-leg fill gap in milliseconds (absolute difference between exchange timestamps).
    #[serde(default)]
    pub inter_leg_gap_ms: Option<i64>,
    /// True if inter_leg_gap_ms exceeds max_leg_fill_gap_ms config threshold.
    #[serde(default)]
    pub stale_fill: bool,
    /// Exchange timestamp from Polymarket snapshot (from SpreadResult).
    #[serde(default)]
    pub poly_exchange_ts: Option<i64>,
    /// Exchange timestamp from Kalshi snapshot (from SpreadResult).
    #[serde(default)]
    pub kalshi_exchange_ts: Option<i64>,
}

/// A single mark-to-market snapshot during position lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtmSnapshot {
    /// Timestamp of this MTM observation.
    pub timestamp_ms: i64,
    /// Current spread value at this point.
    #[serde(with = "rust_decimal::serde::str")]
    pub current_spread: Decimal,
    /// Unrealized P&L based on current spread vs entry spread.
    #[serde(with = "rust_decimal::serde::str")]
    pub unrealized_pnl: Decimal,
}

impl PaperPosition {
    /// Create a new Pending position from a spread signal.
    pub fn new_pending(signal: &SpreadResult, notional: Decimal) -> Self {
        // Sequential ID based on timestamp + event
        let id = format!("pt-{}-{}", signal.timestamp_ms, &signal.event_id[..signal.event_id.len().min(8)]);

        // Compute inter-leg gap from exchange timestamps when both are present
        let inter_leg_gap_ms = match (signal.poly_exchange_ts, signal.kalshi_exchange_ts) {
            (Some(poly_ts), Some(kalshi_ts)) => Some((poly_ts - kalshi_ts).abs()),
            _ => None,
        };

        Self {
            id,
            event_id: signal.event_id.clone(),
            pattern: signal.pattern,
            status: PositionStatus::Pending,
            notional,
            signal_spread: signal.net_spread,
            signal_timestamp_ms: signal.timestamp_ms,
            entry_price_buy: None,
            entry_price_sell: None,
            entry_timestamp_ms: None,
            adverse_selection: None,
            mtm_history: Vec::new(),
            settlement_pnl: None,
            settled_at_ms: None,
            settled_legs: Vec::new(),
            divergence: None,
            threshold_status: signal.threshold_status,
            inter_leg_gap_ms,
            stale_fill: false,
            poly_exchange_ts: signal.poly_exchange_ts,
            kalshi_exchange_ts: signal.kalshi_exchange_ts,
        }
    }

    /// Mark this position's fill as stale if inter-leg gap exceeds the threshold.
    ///
    /// Sets `stale_fill = true` when `inter_leg_gap_ms` is present and exceeds
    /// `max_gap_ms`. Called after position creation when config is available.
    pub fn mark_stale_fill(&mut self, max_gap_ms: i64) {
        if self.inter_leg_gap_ms.map_or(false, |gap| gap > max_gap_ms) {
            self.stale_fill = true;
        }
    }

    /// Fill the position at next-tick prices: Pending -> Open.
    ///
    /// Computes adverse selection as the difference between the entry spread
    /// (sell - buy at fill) and the signal spread. Positive adverse selection
    /// means the fill was worse than the signal price.
    pub fn fill(&mut self, buy_price: Decimal, sell_price: Decimal, timestamp_ms: i64) {
        debug_assert_eq!(self.status, PositionStatus::Pending);
        self.entry_price_buy = Some(buy_price);
        self.entry_price_sell = Some(sell_price);
        self.entry_timestamp_ms = Some(timestamp_ms);

        let entry_spread = sell_price - buy_price;
        self.adverse_selection = Some(self.signal_spread - entry_spread);
        self.status = PositionStatus::Open;
    }

    /// Record a mark-to-market observation.
    ///
    /// Computes unrealized P&L as: (current_spread - entry_spread) * notional
    /// where entry_spread = entry_sell - entry_buy.
    pub fn update_mtm(&mut self, current_spread: Decimal, timestamp_ms: i64) {
        let entry_spread = match (self.entry_price_sell, self.entry_price_buy) {
            (Some(sell), Some(buy)) => sell - buy,
            _ => return, // Not yet filled
        };

        // Unrealized P&L: how much better/worse the current spread is vs entry
        let unrealized_pnl = (current_spread - entry_spread) * self.notional;

        self.mtm_history.push(MtmSnapshot {
            timestamp_ms,
            current_spread,
            unrealized_pnl,
        });
    }

    /// Settle the position with final P&L: Open -> Settled.
    pub fn settle(&mut self, settlement_pnl: Decimal, timestamp_ms: i64) {
        debug_assert_eq!(self.status, PositionStatus::Open);
        self.settlement_pnl = Some(settlement_pnl);
        self.settled_at_ms = Some(timestamp_ms);
        self.status = PositionStatus::Settled;
    }

    /// Record a settled venue leg. Transitions Open -> PartiallySettled.
    ///
    /// Does NOT set `settlement_pnl` -- that happens in `finalize_settlement()`
    /// once all legs are settled.
    pub fn record_settled_leg(&mut self, leg: SettledLeg) {
        self.settled_legs.push(leg);
        if self.status == PositionStatus::Open {
            self.status = PositionStatus::PartiallySettled;
        }
    }

    /// Check if all expected venue legs have settled.
    pub fn all_legs_settled(&self, expected_venue_count: usize) -> bool {
        self.settled_legs.len() >= expected_venue_count
    }

    /// Finalize settlement by computing position-level P&L rollup from settled legs.
    ///
    /// Sets `settlement_pnl` to net P&L (fee-adjusted headline number per CONTEXT.md).
    pub fn finalize_settlement(&mut self) {
        let total_raw_pnl: Decimal = self.settled_legs.iter().map(|l| l.raw_pnl).sum();
        let total_net_pnl: Decimal = self.settled_legs.iter().map(|l| l.net_pnl).sum();
        let _total_fees: Decimal = self
            .settled_legs
            .iter()
            .map(|l| l.entry_fee + l.exit_fee)
            .sum();
        let _total_slippage: Decimal = self
            .settled_legs
            .iter()
            .map(|l| l.slippage_estimate)
            .sum();

        // Net P&L is the headline number per CONTEXT.md decision
        self.settlement_pnl = Some(total_net_pnl);
        self.settled_at_ms = Some(chrono::Utc::now().timestamp_millis());
        self.status = PositionStatus::Settled;

        let _ = total_raw_pnl; // used by caller via settled_legs
    }

    /// Compute cross-venue divergence annotation from settled legs.
    ///
    /// Returns None if fewer than 2 legs (no cross-venue comparison possible).
    pub fn compute_divergence(&self) -> Option<SettlementDivergence> {
        if self.settled_legs.len() < 2 {
            return None;
        }

        // Check for outcome disagreements
        let outcomes: Vec<&OutcomeKind> = self.settled_legs.iter().map(|l| &l.outcome).collect();

        // Check for ambiguous resolutions
        let has_ambiguous = outcomes.iter().any(|o| matches!(o, OutcomeKind::Ambiguous { .. }));
        if has_ambiguous {
            let impact = self.compute_divergence_impact_bps();
            return Some(SettlementDivergence {
                divergence_type: DivergenceType::AmbiguousResolution,
                basis_risk_score_at_entry: Decimal::ZERO,
                actual_impact_bps: impact,
            });
        }

        // Check for binary disagreement (Yes vs No)
        let has_yes = outcomes.iter().any(|o| matches!(o, OutcomeKind::Yes));
        let has_no = outcomes.iter().any(|o| matches!(o, OutcomeKind::No));
        if has_yes && has_no {
            let impact = self.compute_divergence_impact_bps();
            return Some(SettlementDivergence {
                divergence_type: DivergenceType::BinaryDisagree,
                basis_risk_score_at_entry: Decimal::ZERO,
                actual_impact_bps: impact,
            });
        }

        // Check for timing gap (> 4 hours between venue resolutions)
        let resolved_times: Vec<i64> = self
            .settled_legs
            .iter()
            .map(|l| l.resolved_at.timestamp())
            .collect();
        if resolved_times.len() >= 2 {
            let min_t = resolved_times.iter().min().copied().unwrap_or(0);
            let max_t = resolved_times.iter().max().copied().unwrap_or(0);
            let gap_hours = (max_t - min_t) / 3600;
            if gap_hours > 4 {
                let impact = self.compute_divergence_impact_bps();
                return Some(SettlementDivergence {
                    divergence_type: DivergenceType::TimingGap,
                    basis_risk_score_at_entry: Decimal::ZERO,
                    actual_impact_bps: impact,
                });
            }
        }

        None
    }

    /// Compute the actual P&L impact of divergence in basis points.
    ///
    /// Impact = absolute difference in raw_pnl across legs, converted to bps of notional.
    fn compute_divergence_impact_bps(&self) -> Decimal {
        if self.settled_legs.len() < 2 || self.notional.is_zero() {
            return Decimal::ZERO;
        }
        let raw_pnls: Vec<Decimal> = self.settled_legs.iter().map(|l| l.raw_pnl).collect();
        let min_pnl = raw_pnls.iter().min().copied().unwrap_or(Decimal::ZERO);
        let max_pnl = raw_pnls.iter().max().copied().unwrap_or(Decimal::ZERO);
        let diff = (max_pnl - min_pnl).abs();
        // Convert to basis points of notional: (diff / notional) * 10000
        (diff / self.notional) * Decimal::new(10000, 0)
    }

    /// Get the current P&L: settlement P&L if settled, latest MTM P&L if open.
    pub fn current_pnl(&self) -> Option<Decimal> {
        if let Some(pnl) = self.settlement_pnl {
            return Some(pnl);
        }
        self.mtm_history.last().map(|m| m.unrealized_pnl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settlement::types::{OutcomeKind, ResolutionSource};
    use crate::spread::patterns::SpreadPattern;
    use crate::types::Venue;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_settled_leg(venue: Venue, outcome: OutcomeKind, raw_pnl: Decimal, fees: Decimal) -> SettledLeg {
        let net_pnl = raw_pnl - fees;
        SettledLeg {
            venue,
            outcome,
            raw_pnl,
            entry_fee: fees,
            exit_fee: Decimal::ZERO,
            slippage_estimate: Decimal::ZERO,
            net_pnl,
            fee_model_version: "v1.0".to_string(),
            resolved_at: Utc::now(),
            detected_at: Utc::now(),
            resolution_source: ResolutionSource::DeribitDelivery,
        }
    }

    fn make_signal() -> SpreadResult {
        SpreadResult {
            event_id: "test-event-123".to_string(),
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
            kalshi_exchange_ts: None,
            options_exchange_ts: None,
            threshold: Some(dec("0.025")),
            threshold_components: None,
            threshold_status: None,
        }
    }

    #[test]
    fn new_pending_creates_correct_position() {
        let signal = make_signal();
        let pos = PaperPosition::new_pending(&signal, dec("500"));

        assert_eq!(pos.status, PositionStatus::Pending);
        assert_eq!(pos.event_id, "test-event-123");
        assert_eq!(pos.pattern, SpreadPattern::BuyPolyYesSellKalshiYes);
        assert_eq!(pos.notional, dec("500"));
        assert_eq!(pos.signal_spread, dec("0.03"));
        assert_eq!(pos.signal_timestamp_ms, 1700000000000);
        assert!(pos.entry_price_buy.is_none());
        assert!(pos.entry_price_sell.is_none());
        assert!(pos.adverse_selection.is_none());
        assert!(pos.mtm_history.is_empty());
        assert!(pos.settlement_pnl.is_none());
    }

    #[test]
    fn fill_transitions_pending_to_open() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));

        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        assert_eq!(pos.status, PositionStatus::Open);
        assert_eq!(pos.entry_price_buy, Some(dec("0.46")));
        assert_eq!(pos.entry_price_sell, Some(dec("0.49")));
        assert_eq!(pos.entry_timestamp_ms, Some(1700000001000));
    }

    #[test]
    fn adverse_selection_computed_correctly() {
        let signal = make_signal(); // signal_spread = 0.03
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));

        // Fill at worse prices: entry_spread = 0.49 - 0.46 = 0.03
        // adverse_selection = signal_spread - entry_spread = 0.03 - 0.03 = 0.00
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        assert_eq!(pos.adverse_selection, Some(dec("0.00")));

        // Now test with worse fill: signal_spread=0.03, entry_spread = 0.48-0.47=0.01
        let mut pos2 = PaperPosition::new_pending(&signal, dec("500"));
        pos2.fill(dec("0.47"), dec("0.48"), 1700000001000);
        // adverse_selection = 0.03 - 0.01 = 0.02 (positive = fill was worse)
        assert_eq!(pos2.adverse_selection, Some(dec("0.02")));
    }

    #[test]
    fn mtm_history_accumulates_snapshots() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        // entry_spread = 0.49 - 0.46 = 0.03

        pos.update_mtm(dec("0.04"), 1700000002000);
        pos.update_mtm(dec("0.05"), 1700000003000);
        pos.update_mtm(dec("0.02"), 1700000004000);

        assert_eq!(pos.mtm_history.len(), 3);

        // Check first MTM: unrealized_pnl = (0.04 - 0.03) * 500 = 5.0
        assert_eq!(pos.mtm_history[0].current_spread, dec("0.04"));
        assert_eq!(pos.mtm_history[0].unrealized_pnl, dec("5.0"));

        // Second: (0.05 - 0.03) * 500 = 10.0
        assert_eq!(pos.mtm_history[1].unrealized_pnl, dec("10.0"));

        // Third: (0.02 - 0.03) * 500 = -5.0
        assert_eq!(pos.mtm_history[2].unrealized_pnl, dec("-5.0"));
    }

    #[test]
    fn settle_transitions_open_to_settled() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        pos.settle(dec("15.0"), 1700000010000);

        assert_eq!(pos.status, PositionStatus::Settled);
        assert_eq!(pos.settlement_pnl, Some(dec("15.0")));
        assert_eq!(pos.settled_at_ms, Some(1700000010000));
    }

    #[test]
    fn current_pnl_returns_settlement_if_settled() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos.update_mtm(dec("0.04"), 1700000002000);
        pos.settle(dec("15.0"), 1700000010000);

        // Settlement P&L should take precedence over MTM
        assert_eq!(pos.current_pnl(), Some(dec("15.0")));
    }

    #[test]
    fn current_pnl_returns_latest_mtm_if_open() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos.update_mtm(dec("0.04"), 1700000002000);
        pos.update_mtm(dec("0.05"), 1700000003000);

        // Latest MTM: (0.05 - 0.03) * 500 = 10.0
        assert_eq!(pos.current_pnl(), Some(dec("10.0")));
    }

    #[test]
    fn current_pnl_returns_none_if_pending() {
        let signal = make_signal();
        let pos = PaperPosition::new_pending(&signal, dec("500"));
        assert!(pos.current_pnl().is_none());
    }

    #[test]
    fn record_settled_leg_transitions_open_to_partially_settled() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        assert_eq!(pos.status, PositionStatus::Open);

        let leg = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("50.0"), dec("2.5"));
        pos.record_settled_leg(leg);

        assert_eq!(pos.status, PositionStatus::PartiallySettled);
        assert_eq!(pos.settled_legs.len(), 1);
    }

    #[test]
    fn all_legs_settled_checks_count() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        assert!(!pos.all_legs_settled(2));

        let leg1 = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("50.0"), dec("2.5"));
        pos.record_settled_leg(leg1);
        assert!(!pos.all_legs_settled(2));

        let leg2 = make_settled_leg(Venue::Kalshi, OutcomeKind::Yes, dec("-30.0"), dec("1.0"));
        pos.record_settled_leg(leg2);
        assert!(pos.all_legs_settled(2));

        // Also works for single-leg
        let signal2 = make_signal();
        let mut pos2 = PaperPosition::new_pending(&signal2, dec("500"));
        pos2.fill(dec("0.46"), dec("0.49"), 1700000001000);
        let leg = make_settled_leg(Venue::Deribit, OutcomeKind::Yes, dec("100.0"), dec("0.0"));
        pos2.record_settled_leg(leg);
        assert!(pos2.all_legs_settled(1));
    }

    #[test]
    fn finalize_settlement_computes_correct_rollup() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg1 = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("150.0"), dec("2.5"));
        let leg2 = make_settled_leg(Venue::Kalshi, OutcomeKind::Yes, dec("-50.0"), dec("1.0"));
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);

        pos.finalize_settlement();

        assert_eq!(pos.status, PositionStatus::Settled);
        // net_pnl = (150.0 - 2.5) + (-50.0 - 1.0) = 147.5 + (-51.0) = 96.5
        assert_eq!(pos.settlement_pnl, Some(dec("96.5")));
        assert!(pos.settled_at_ms.is_some());
    }

    #[test]
    fn compute_divergence_detects_binary_disagree() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg1 = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("250.0"), dec("0.0"));
        let leg2 = make_settled_leg(Venue::Kalshi, OutcomeKind::No, dec("-250.0"), dec("0.0"));
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);

        let div = pos.compute_divergence();
        assert!(div.is_some());
        let div = div.unwrap();
        assert_eq!(div.divergence_type, DivergenceType::BinaryDisagree);
        // Impact = |250 - (-250)| / 500 * 10000 = 500/500 * 10000 = 10000 bps
        assert_eq!(div.actual_impact_bps, dec("10000"));
    }

    #[test]
    fn compute_divergence_returns_none_for_single_leg() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("50.0"), dec("0.0"));
        pos.record_settled_leg(leg);

        assert!(pos.compute_divergence().is_none());
    }

    #[test]
    fn compute_divergence_detects_ambiguous_resolution() {
        let signal = make_signal();
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let leg1 = make_settled_leg(Venue::Polymarket, OutcomeKind::Yes, dec("50.0"), dec("0.0"));
        let leg2 = make_settled_leg(
            Venue::Kalshi,
            OutcomeKind::Ambiguous { settlement_price: dec("0.42") },
            dec("-30.0"),
            dec("0.0"),
        );
        pos.record_settled_leg(leg1);
        pos.record_settled_leg(leg2);

        let div = pos.compute_divergence();
        assert!(div.is_some());
        assert_eq!(div.unwrap().divergence_type, DivergenceType::AmbiguousResolution);
    }

    #[test]
    fn new_pending_has_empty_settled_legs_and_no_divergence() {
        let signal = make_signal();
        let pos = PaperPosition::new_pending(&signal, dec("500"));
        assert!(pos.settled_legs.is_empty());
        assert!(pos.divergence.is_none());
    }
}
