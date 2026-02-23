//! Paper trade position lifecycle: Pending -> Open -> Settled.
//!
//! Tracks hypothetical positions from signal through fill to settlement,
//! recording adverse selection (signal vs fill spread difference), mark-to-market
//! history, and final settlement P&L.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::spread::patterns::{SpreadPattern, SpreadResult};

/// Position lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    /// Signal received, awaiting next-tick fill.
    Pending,
    /// Position filled, tracking MTM.
    Open,
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
    use crate::spread::patterns::SpreadPattern;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
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
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("0.95"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: Some(1700000000100),
            kalshi_exchange_ts: None,
            threshold: Some(dec("0.025")),
            threshold_components: None,
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
}
