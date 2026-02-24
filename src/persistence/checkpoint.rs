//! Checkpoint state snapshot for paper trade recovery.
//!
//! Captures the full mutable state of the paper trade engine at a point in time,
//! enabling periodic save-to-disk and startup restore without replaying the full
//! JSONL trade log.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::paper_trade::aggregator::DailyRollup;
use crate::paper_trade::position::PaperPosition;

/// Complete snapshot of the paper trade engine state at a point in time.
///
/// Serialized to JSON and written atomically to disk by the checkpoint manager.
/// On startup, the most recent valid checkpoint is loaded and passed to
/// `PaperTradeTracker::restore_state()`.
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointState {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Timestamp when this checkpoint was written (epoch millis).
    pub checkpoint_timestamp_ms: i64,
    /// Pending positions awaiting next-tick fill, keyed by event_id.
    pub pending: HashMap<String, Vec<PaperPosition>>,
    /// Active open positions.
    pub open: Vec<PaperPosition>,
    /// Daily P&L rollup data, keyed by date string (YYYY-MM-DD).
    pub daily_rollups: HashMap<String, DailyRollup>,
    /// Running total trade count.
    pub total_trades: u64,
}

impl CheckpointState {
    /// Current schema version. Bump when the checkpoint format changes.
    pub fn current_version() -> u32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper_trade::position::PaperPosition;
    use crate::spread::patterns::{SpreadPattern, SpreadResult};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
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
            poly_exchange_ts: None,
            kalshi_exchange_ts: None,
            threshold: None,
            threshold_components: None,
        }
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        // Build sample state
        let signal = make_signal("evt-001");
        let pending_pos = PaperPosition::new_pending(&signal, dec("500"));

        let signal2 = make_signal("evt-002");
        let mut open_pos = PaperPosition::new_pending(&signal2, dec("500"));
        open_pos.fill(dec("0.46"), dec("0.49"), 1700000001000);

        let mut pending = HashMap::new();
        pending.insert("evt-001".to_string(), vec![pending_pos]);

        let mut daily_rollups = HashMap::new();
        daily_rollups.insert(
            "2026-01-15".to_string(),
            DailyRollup {
                date: "2026-01-15".to_string(),
                trade_count: 5,
                signal_count: 10,
                total_pnl: dec("25.50"),
                winning_trades: 3,
                losing_trades: 2,
                avg_pnl: dec("5.10"),
                max_win: dec("15.00"),
                max_loss: dec("-3.50"),
            },
        );

        let state = CheckpointState {
            version: CheckpointState::current_version(),
            checkpoint_timestamp_ms: 1700000005000,
            pending,
            open: vec![open_pos],
            daily_rollups,
            total_trades: 42,
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&state).expect("serialize");

        // Deserialize back
        let restored: CheckpointState =
            serde_json::from_str(&json).expect("deserialize");

        // Verify all fields
        assert_eq!(restored.version, 1);
        assert_eq!(restored.checkpoint_timestamp_ms, 1700000005000);
        assert_eq!(restored.total_trades, 42);
        assert_eq!(restored.pending.len(), 1);
        assert_eq!(restored.pending["evt-001"].len(), 1);
        assert_eq!(restored.open.len(), 1);
        assert_eq!(restored.open[0].event_id, "evt-002");
        assert_eq!(restored.daily_rollups.len(), 1);

        let rollup = &restored.daily_rollups["2026-01-15"];
        assert_eq!(rollup.trade_count, 5);
        assert_eq!(rollup.signal_count, 10);
        assert_eq!(rollup.total_pnl, dec("25.50"));
        assert_eq!(rollup.winning_trades, 3);
        assert_eq!(rollup.losing_trades, 2);
        assert_eq!(rollup.max_win, dec("15.00"));
        assert_eq!(rollup.max_loss, dec("-3.50"));
    }
}
