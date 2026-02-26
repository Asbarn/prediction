//! Daily P&L rollup aggregation for paper trades.
//!
//! Aggregates individual paper trade outcomes into per-day summaries
//! with trade counts, win/loss rates, and P&L statistics.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::analyzer::LifetimeSummary;
use super::position::PaperPosition;

/// Daily P&L rollup aggregator.
///
/// Accumulates per-day trade statistics as positions settle or
/// close. Keyed by date string (YYYY-MM-DD).
pub struct DailyAggregator {
    /// Per-day rollup data, keyed by date string.
    daily_pnl: HashMap<String, DailyRollup>,
}

/// Aggregated statistics for a single day of paper trading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyRollup {
    /// Date (YYYY-MM-DD).
    pub date: String,
    /// Number of trades that settled or were recorded this day.
    pub trade_count: usize,
    /// Number of signals received this day.
    pub signal_count: usize,
    /// Total P&L across all trades.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_pnl: Decimal,
    /// Number of profitable trades.
    pub winning_trades: usize,
    /// Number of losing trades.
    pub losing_trades: usize,
    /// Average P&L per trade.
    #[serde(with = "rust_decimal::serde::str")]
    pub avg_pnl: Decimal,
    /// Largest single-trade win.
    #[serde(with = "rust_decimal::serde::str")]
    pub max_win: Decimal,
    /// Largest single-trade loss (stored as negative).
    #[serde(with = "rust_decimal::serde::str")]
    pub max_loss: Decimal,
}

impl DailyRollup {
    /// Create a new empty rollup for the given date.
    fn new(date: &str) -> Self {
        Self {
            date: date.to_string(),
            trade_count: 0,
            signal_count: 0,
            total_pnl: Decimal::ZERO,
            winning_trades: 0,
            losing_trades: 0,
            avg_pnl: Decimal::ZERO,
            max_win: Decimal::ZERO,
            max_loss: Decimal::ZERO,
        }
    }
}

impl DailyAggregator {
    /// Create a new empty aggregator.
    pub fn new() -> Self {
        Self {
            daily_pnl: HashMap::new(),
        }
    }

    /// Record a signal for the given date (increments signal_count).
    pub fn record_signal(&mut self, date: &str) {
        let rollup = self
            .daily_pnl
            .entry(date.to_string())
            .or_insert_with(|| DailyRollup::new(date));
        rollup.signal_count += 1;
    }

    /// Record a completed (settled or MTM-valued) trade into the current day's rollup.
    pub fn record_trade(&mut self, position: &PaperPosition) {
        let pnl = match position.current_pnl() {
            Some(p) => p,
            None => return, // No P&L to record
        };

        let date = chrono::Utc::now()
            .format("%Y-%m-%d")
            .to_string();

        let rollup = self
            .daily_pnl
            .entry(date.clone())
            .or_insert_with(|| DailyRollup::new(&date));

        rollup.trade_count += 1;
        rollup.total_pnl += pnl;

        if pnl > Decimal::ZERO {
            rollup.winning_trades += 1;
            if pnl > rollup.max_win {
                rollup.max_win = pnl;
            }
        } else if pnl < Decimal::ZERO {
            rollup.losing_trades += 1;
            if pnl < rollup.max_loss {
                rollup.max_loss = pnl;
            }
        }

        // Recompute average
        if rollup.trade_count > 0 {
            rollup.avg_pnl = rollup.total_pnl
                / Decimal::from(rollup.trade_count as i64);
        }
    }

    /// Get the rollup for a specific date.
    pub fn get_daily(&self, date: &str) -> Option<&DailyRollup> {
        self.daily_pnl.get(date)
    }

    /// Emit daily summary via tracing and Prometheus metrics.
    ///
    /// If an `analysis_summary` is provided and has settled positions, also emits
    /// the signal analysis daily summary with hit rate, edge, convergence, and
    /// false positive rate metrics.
    pub fn emit_daily_summary(&self, date: &str, analysis_summary: Option<&LifetimeSummary>) {
        if let Some(rollup) = self.daily_pnl.get(date) {
            let total_pnl_f64 = rollup.total_pnl.to_f64().unwrap_or(0.0);
            let avg_pnl_f64 = rollup.avg_pnl.to_f64().unwrap_or(0.0);

            tracing::info!(
                date = date,
                trade_count = rollup.trade_count,
                signal_count = rollup.signal_count,
                total_pnl = total_pnl_f64,
                winning = rollup.winning_trades,
                losing = rollup.losing_trades,
                avg_pnl = avg_pnl_f64,
                "daily paper trade summary"
            );

            metrics::gauge!("paper_trade_daily_pnl").set(total_pnl_f64);
            metrics::gauge!("paper_trade_daily_trades").set(rollup.trade_count as f64);
            metrics::gauge!("paper_trade_daily_win_rate").set(
                if rollup.trade_count > 0 {
                    rollup.winning_trades as f64 / rollup.trade_count as f64
                } else {
                    0.0
                },
            );
        }

        // Emit signal analysis daily summary if available
        if let Some(summary) = analysis_summary {
            if summary.total_settled > 0 {
                tracing::info!(
                    date = date,
                    total_settled = summary.total_settled,
                    gross_hit_rate = format!("{:.1}%", summary.gross_hit_rate * 100.0),
                    net_hit_rate = format!("{:.1}%", summary.net_hit_rate * 100.0),
                    avg_net_edge = format!("{:.4}", summary.avg_net_edge),
                    false_positive_rate = format!("{:.1}%", summary.false_positive_rate * 100.0),
                    avg_convergence_secs = format!("{:.0}", summary.avg_convergence_secs),
                    stale_fills = summary.stale_fill_count,
                    "DAILY ANALYSIS SUMMARY"
                );

                metrics::gauge!("signal_analysis_daily_settled").set(summary.total_settled as f64);
                metrics::gauge!("signal_analysis_daily_net_hit_rate").set(summary.net_hit_rate);
            }
        }
    }

    /// Export all rollup data for checkpointing.
    pub fn export_rollups(&self) -> HashMap<String, DailyRollup> {
        self.daily_pnl.clone()
    }

    /// Import rollup data from a checkpoint, replacing current state.
    pub fn import_rollups(&mut self, rollups: HashMap<String, DailyRollup>) {
        self.daily_pnl = rollups;
    }

    /// Get all dates with rollups (for final summary).
    pub fn all_dates(&self) -> Vec<&str> {
        let mut dates: Vec<&str> = self.daily_pnl.keys().map(|s| s.as_str()).collect();
        dates.sort();
        dates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper_trade::position::PaperPosition;
    use crate::spread::patterns::{SpreadPattern, SpreadResult};
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_signal(event_id: &str, net_spread: &str) -> SpreadResult {
        SpreadResult {
            event_id: event_id.to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            gross_spread: dec("0.05"),
            net_spread: dec(net_spread),
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
            threshold_status: None,
        }
    }

    #[test]
    fn aggregator_counts_and_sums_correctly() {
        let mut agg = DailyAggregator::new();

        // Create 3 trades: 2 winners, 1 loser
        let signal1 = make_signal("evt1", "0.03");
        let mut pos1 = PaperPosition::new_pending(&signal1, dec("500"));
        pos1.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos1.settle(dec("10.0"), 1700000010000);

        let signal2 = make_signal("evt2", "0.04");
        let mut pos2 = PaperPosition::new_pending(&signal2, dec("500"));
        pos2.fill(dec("0.44"), dec("0.50"), 1700000001000);
        pos2.settle(dec("20.0"), 1700000010000);

        let signal3 = make_signal("evt3", "0.02");
        let mut pos3 = PaperPosition::new_pending(&signal3, dec("500"));
        pos3.fill(dec("0.47"), dec("0.48"), 1700000001000);
        pos3.settle(dec("-5.0"), 1700000010000);

        agg.record_trade(&pos1);
        agg.record_trade(&pos2);
        agg.record_trade(&pos3);

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let rollup = agg.get_daily(&today).unwrap();

        assert_eq!(rollup.trade_count, 3);
        assert_eq!(rollup.winning_trades, 2);
        assert_eq!(rollup.losing_trades, 1);
        assert_eq!(rollup.total_pnl, dec("25.0")); // 10 + 20 - 5
        assert_eq!(rollup.max_win, dec("20.0"));
        assert_eq!(rollup.max_loss, dec("-5.0"));
    }

    #[test]
    fn aggregator_ignores_positions_without_pnl() {
        let mut agg = DailyAggregator::new();

        let signal = make_signal("evt1", "0.03");
        let pos = PaperPosition::new_pending(&signal, dec("500"));

        // Pending position has no P&L
        agg.record_trade(&pos);

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert!(agg.get_daily(&today).is_none());
    }

    #[test]
    fn signal_count_tracked_separately() {
        let mut agg = DailyAggregator::new();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        agg.record_signal(&today);
        agg.record_signal(&today);
        agg.record_signal(&today);

        let rollup = agg.get_daily(&today).unwrap();
        assert_eq!(rollup.signal_count, 3);
        assert_eq!(rollup.trade_count, 0);
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut agg = DailyAggregator::new();

        // Record some signals and trades
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        agg.record_signal(&today);
        agg.record_signal(&today);

        let signal = make_signal("evt1", "0.03");
        let mut pos = PaperPosition::new_pending(&signal, dec("500"));
        pos.fill(dec("0.46"), dec("0.49"), 1700000001000);
        pos.settle(dec("10.0"), 1700000010000);
        agg.record_trade(&pos);

        // Export rollups
        let exported = agg.export_rollups();
        assert_eq!(exported.len(), 1);
        let rollup = &exported[&today];
        assert_eq!(rollup.signal_count, 2);
        assert_eq!(rollup.trade_count, 1);
        assert_eq!(rollup.total_pnl, dec("10.0"));

        // Import into a fresh aggregator
        let mut agg2 = DailyAggregator::new();
        agg2.import_rollups(exported);

        let restored = agg2.get_daily(&today).unwrap();
        assert_eq!(restored.signal_count, 2);
        assert_eq!(restored.trade_count, 1);
        assert_eq!(restored.total_pnl, dec("10.0"));
        assert_eq!(restored.winning_trades, 1);
        assert_eq!(restored.losing_trades, 0);
    }
}
