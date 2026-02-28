//! Signal scoring computation functions.
//!
//! Provides pure statistical computation functions for signal performance analysis:
//! hit rates with Wilson confidence intervals, edge t-tests, Sharpe ratio with
//! probabilistic Sharpe ratio (PSR), and maximum drawdown tracking.
//!
//! Each function takes slices and returns Option, making them testable in isolation
//! and reusable for both aggregate and per-event analysis.

use crate::analysis::stats::{kurtosis_f64, mean_f64, skewness_f64, stddev_f64, wilson_ci};
use crate::paper_trade::analyzer::AnalysisSettlementRecord;
use serde::Serialize;
use statrs::distribution::{ContinuousCDF, Normal, StudentsT};

// ---------------------------------------------------------------------------
// Result structs
// ---------------------------------------------------------------------------

/// Hit rate computation result with Wilson confidence intervals.
#[derive(Debug, Clone, Serialize)]
pub struct HitRateResult {
    pub gross_hits: usize,
    pub net_hits: usize,
    pub total: usize,
    pub gross_rate: f64,
    pub net_rate: f64,
    pub gross_ci_95: (f64, f64),
    pub gross_ci_99: (f64, f64),
    pub net_ci_95: (f64, f64),
    pub net_ci_99: (f64, f64),
}

/// Edge t-test result (H0: mean edge = 0).
#[derive(Debug, Clone, Serialize)]
pub struct EdgeTestResult {
    pub mean_edge: f64,
    pub std_error: f64,
    pub t_statistic: f64,
    pub p_value: f64,
    pub ci_95: (f64, f64),
    pub n: usize,
}

/// Sharpe ratio result with annualization and probabilistic Sharpe ratio.
#[derive(Debug, Clone, Serialize)]
pub struct SharpeResult {
    pub per_trade_sharpe: f64,
    pub annualized_sharpe: Option<f64>,
    pub trades_per_year: Option<f64>,
    pub psr: Option<f64>,
    pub n: usize,
}

/// Maximum drawdown result with dates.
#[derive(Debug, Clone, Serialize)]
pub struct DrawdownResult {
    pub max_drawdown_abs: f64,
    pub max_drawdown_pct: Option<f64>,
    pub peak_date: String,
    pub trough_date: String,
    pub recovery_date: Option<String>,
    pub current_drawdown_abs: f64,
    pub current_drawdown_pct: Option<f64>,
}

/// Composite scoring result assembling all five computations.
#[derive(Debug, Clone, Serialize)]
pub struct ScoringResult {
    pub hit_rates: Option<HitRateResult>,
    pub edge_test: Option<EdgeTestResult>,
    pub sharpe: Option<SharpeResult>,
    pub drawdown: Option<DrawdownResult>,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract P&L series from settlement records, skipping parse failures.
pub fn extract_pnl_series(records: &[AnalysisSettlementRecord]) -> Vec<f64> {
    records
        .iter()
        .filter_map(|r| r.total_net_pnl.parse::<f64>().ok())
        .collect()
}

fn timestamp_to_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Computation functions
// ---------------------------------------------------------------------------

/// Compute hit rates with Wilson confidence intervals at 95% and 99% levels.
///
/// Uses the boolean `gross_hit` and `net_hit` fields directly (NOT P&L sign).
/// Returns None if records is empty.
pub fn compute_hit_rates(records: &[AnalysisSettlementRecord]) -> Option<HitRateResult> {
    if records.is_empty() {
        return None;
    }

    let total = records.len();
    let gross_hits = records.iter().filter(|r| r.gross_hit).count();
    let net_hits = records.iter().filter(|r| r.net_hit).count();

    let gross_rate = gross_hits as f64 / total as f64;
    let net_rate = net_hits as f64 / total as f64;

    // Wilson CI: z=1.96 for 95%, z=2.576 for 99%
    let gross_ci_95 = wilson_ci(gross_hits, total, 1.96).unwrap_or((0.0, 0.0));
    let gross_ci_99 = wilson_ci(gross_hits, total, 2.576).unwrap_or((0.0, 0.0));
    let net_ci_95 = wilson_ci(net_hits, total, 1.96).unwrap_or((0.0, 0.0));
    let net_ci_99 = wilson_ci(net_hits, total, 2.576).unwrap_or((0.0, 0.0));

    Some(HitRateResult {
        gross_hits,
        net_hits,
        total,
        gross_rate,
        net_rate,
        gross_ci_95,
        gross_ci_99,
        net_ci_95,
        net_ci_99,
    })
}

/// One-sample t-test for edge (H0: mean = 0).
///
/// Returns None if fewer than 2 observations or zero standard deviation.
pub fn compute_edge_test(pnl: &[f64]) -> Option<EdgeTestResult> {
    let n = pnl.len();
    if n < 2 {
        return None;
    }

    let mean = mean_f64(pnl)?;
    let sd = stddev_f64(pnl)?;
    if sd == 0.0 {
        return None;
    }

    let nf = n as f64;
    let se = sd / nf.sqrt();
    let t_stat = mean / se;
    let df = (n - 1) as f64;

    // Two-tailed p-value
    let t_dist = StudentsT::new(0.0, 1.0, df).ok()?;
    let p_value = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));

    // 95% CI: mean +/- t_crit * se
    let t_crit = t_dist.inverse_cdf(0.975);
    let ci_95 = (mean - t_crit * se, mean + t_crit * se);

    Some(EdgeTestResult {
        mean_edge: mean,
        std_error: se,
        t_statistic: t_stat,
        p_value,
        ci_95,
        n,
    })
}

/// Probabilistic Sharpe Ratio: probability that true Sharpe exceeds zero.
///
/// Uses Bailey & Lopez de Prado (2012) formula accounting for skewness and kurtosis.
/// Returns None if n < 2 or denominator is degenerate.
pub fn compute_psr(pnl: &[f64], sharpe: f64) -> Option<f64> {
    let n = pnl.len();
    if n < 2 {
        return None;
    }

    let skew = skewness_f64(pnl).unwrap_or(0.0);
    let excess_kurt = kurtosis_f64(pnl).unwrap_or(0.0);
    let raw_kurt = excess_kurt + 3.0; // Convert to normal=3 convention

    let nf = n as f64;
    let sr2 = sharpe * sharpe;
    let denom_sq = 1.0 - skew * sharpe + (raw_kurt - 1.0) / 4.0 * sr2;

    if denom_sq <= 0.0 {
        return None;
    }

    let z = sharpe * (nf - 1.0).sqrt() / denom_sq.sqrt();
    let normal = Normal::standard();
    Some(normal.cdf(z))
}

/// Sharpe ratio: per-trade and annualized (using 365-day year for prediction markets).
///
/// Returns None if fewer than 2 observations or zero standard deviation.
pub fn compute_sharpe(pnl: &[f64], first_ms: i64, last_ms: i64) -> Option<SharpeResult> {
    let n = pnl.len();
    if n < 2 {
        return None;
    }

    let mean = mean_f64(pnl)?;
    let sd = stddev_f64(pnl)?;
    if sd == 0.0 {
        return None;
    }

    let per_trade_sharpe = mean / sd;

    // Annualize using observation period
    let obs_ms = (last_ms - first_ms) as f64;
    let ms_per_year = 365.25 * 24.0 * 3600.0 * 1000.0;
    let obs_years = obs_ms / ms_per_year;

    let (annualized_sharpe, trades_per_year) = if obs_years > 0.0 {
        let tpy = n as f64 / obs_years;
        (Some(per_trade_sharpe * tpy.sqrt()), Some(tpy))
    } else {
        (None, None)
    };

    let psr = compute_psr(pnl, per_trade_sharpe);

    Some(SharpeResult {
        per_trade_sharpe,
        annualized_sharpe,
        trades_per_year,
        psr,
        n,
    })
}

/// Maximum drawdown from cumulative P&L curve.
///
/// Returns None if pnl is empty. Tracks running peak and finds the largest
/// peak-to-trough decline, with dates converted from millisecond timestamps.
pub fn compute_max_drawdown(pnl: &[f64], timestamps_ms: &[i64]) -> Option<DrawdownResult> {
    if pnl.is_empty() || timestamps_ms.is_empty() {
        return None;
    }

    let n = pnl.len().min(timestamps_ms.len());

    // Build cumulative P&L curve
    let mut cumulative = Vec::with_capacity(n);
    let mut running = 0.0;
    for &p in pnl.iter().take(n) {
        running += p;
        cumulative.push(running);
    }

    // Walk curve tracking running peak, find max drawdown span
    let mut peak = cumulative[0];
    let mut peak_idx = 0;
    let mut max_dd = 0.0_f64;
    let mut max_dd_peak_idx = 0;
    let mut max_dd_trough_idx = 0;

    for (i, &cum) in cumulative.iter().enumerate() {
        if cum > peak {
            peak = cum;
            peak_idx = i;
        }
        let dd = peak - cum;
        if dd > max_dd {
            max_dd = dd;
            max_dd_peak_idx = peak_idx;
            max_dd_trough_idx = i;
        }
    }

    // Find recovery: first index after trough where cumulative >= peak at max drawdown
    let dd_peak_value = cumulative[max_dd_peak_idx];
    let recovery_idx = cumulative
        .iter()
        .enumerate()
        .skip(max_dd_trough_idx + 1)
        .find(|(_, c)| **c >= dd_peak_value)
        .map(|(i, _)| i);

    // Current drawdown
    let overall_peak = cumulative
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let last_cum = cumulative[n - 1];
    let current_dd = (overall_peak - last_cum).max(0.0);

    // Percentage calculations
    let max_dd_pct = if dd_peak_value.abs() > 0.0 {
        Some(max_dd / dd_peak_value.abs() * 100.0)
    } else {
        None
    };
    let current_dd_pct = if overall_peak.abs() > 0.0 {
        Some(current_dd / overall_peak.abs() * 100.0)
    } else {
        None
    };

    Some(DrawdownResult {
        max_drawdown_abs: max_dd,
        max_drawdown_pct: max_dd_pct,
        peak_date: timestamp_to_date(timestamps_ms[max_dd_peak_idx]),
        trough_date: timestamp_to_date(timestamps_ms[max_dd_trough_idx]),
        recovery_date: recovery_idx.map(|i| timestamp_to_date(timestamps_ms[i])),
        current_drawdown_abs: current_dd,
        current_drawdown_pct: current_dd_pct,
    })
}

/// Convenience function that runs all five scoring computations.
///
/// Sorts records by `settled_at_ms` for chronological order (needed for drawdown
/// and Sharpe annualization). Extracts P&L series once and passes to all functions.
pub fn compute_scoring(records: &[AnalysisSettlementRecord]) -> ScoringResult {
    let mut sorted_records: Vec<AnalysisSettlementRecord> = records.to_vec();
    sorted_records.sort_by_key(|r| r.settled_at_ms);

    let hit_rates = compute_hit_rates(&sorted_records);
    let pnl = extract_pnl_series(&sorted_records);

    let edge_test = compute_edge_test(&pnl);

    let sharpe = if !sorted_records.is_empty() {
        let first_ms = sorted_records.first().unwrap().settled_at_ms;
        let last_ms = sorted_records.last().unwrap().settled_at_ms;
        compute_sharpe(&pnl, first_ms, last_ms)
    } else {
        None
    };

    let timestamps_ms: Vec<i64> = sorted_records.iter().map(|r| r.settled_at_ms).collect();
    let drawdown = compute_max_drawdown(&pnl, &timestamps_ms);

    ScoringResult {
        hit_rates,
        edge_test,
        sharpe,
        drawdown,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::types::ThresholdStatus;

    /// Helper to create a test AnalysisSettlementRecord with minimal fields.
    fn make_record(
        gross_hit: bool,
        net_hit: bool,
        total_net_pnl: &str,
        settled_at_ms: i64,
    ) -> AnalysisSettlementRecord {
        AnalysisSettlementRecord {
            event_id: "test-event".to_string(),
            position_id: "test-pos".to_string(),
            venue_pair: "polymarket-kalshi".to_string(),
            pattern: "YES_NO".to_string(),
            threshold_status: Some(ThresholdStatus::PassedBoth),
            convergence_secs: 300.0,
            gross_hit,
            net_hit,
            total_raw_pnl: total_net_pnl.to_string(),
            total_net_pnl: total_net_pnl.to_string(),
            total_fees: "0.00".to_string(),
            total_slippage: "0.00".to_string(),
            inter_leg_gap_ms: Some(100),
            stale_fill: false,
            running_gross_hit_rate: 0.0,
            running_net_hit_rate: 0.0,
            running_avg_net_edge: 0.0,
            running_false_positive_rate: 0.0,
            running_avg_convergence_secs: 0.0,
            settled_at_ms,
        }
    }

    #[test]
    fn hit_rates_known_values() {
        // 7/10 gross hits, 5/10 net hits
        let records: Vec<_> = (0..10)
            .map(|i| make_record(i < 7, i < 5, "1.00", 1000 + i * 100))
            .collect();

        let result = compute_hit_rates(&records).unwrap();
        assert_eq!(result.gross_hits, 7);
        assert_eq!(result.net_hits, 5);
        assert_eq!(result.total, 10);
        assert!((result.gross_rate - 0.7).abs() < 1e-10);
        assert!((result.net_rate - 0.5).abs() < 1e-10);

        // 95% CI should bracket the rate
        assert!(result.gross_ci_95.0 < result.gross_rate);
        assert!(result.gross_ci_95.1 > result.gross_rate);
        assert!(result.net_ci_95.0 < result.net_rate);
        assert!(result.net_ci_95.1 > result.net_rate);

        // 99% CI should be wider than 95%
        assert!(result.gross_ci_99.0 <= result.gross_ci_95.0);
        assert!(result.gross_ci_99.1 >= result.gross_ci_95.1);
    }

    #[test]
    fn hit_rates_empty_returns_none() {
        assert!(compute_hit_rates(&[]).is_none());
    }

    #[test]
    fn edge_test_positive_edge() {
        // Known positive mean values
        let pnl = vec![5.0, 3.0, 7.0, 4.0, 6.0, 8.0, 2.0, 9.0, 5.5, 6.5];
        let result = compute_edge_test(&pnl).unwrap();
        assert!(
            result.mean_edge > 0.0,
            "Mean edge should be positive, got {}",
            result.mean_edge
        );
        assert!(
            result.t_statistic > 0.0,
            "t-statistic should be positive, got {}",
            result.t_statistic
        );
        assert!(
            result.p_value < 0.05,
            "p-value should be < 0.05 for strong positive edge, got {}",
            result.p_value
        );
    }

    #[test]
    fn edge_test_too_few_returns_none() {
        assert!(compute_edge_test(&[]).is_none());
        assert!(compute_edge_test(&[1.0]).is_none());
    }

    #[test]
    fn sharpe_known_values() {
        // Known: mean=2.0, sd=1.0 -> per_trade_sharpe=2.0
        let pnl = vec![1.0, 2.0, 3.0]; // mean=2.0, sd=1.0
        let first_ms = 1_000_000;
        let last_ms = 1_000_000 + 86_400_000 * 30; // 30 days apart

        let result = compute_sharpe(&pnl, first_ms, last_ms).unwrap();
        assert!(
            (result.per_trade_sharpe - 2.0).abs() < 1e-10,
            "per_trade_sharpe should be 2.0, got {}",
            result.per_trade_sharpe
        );
        assert!(result.annualized_sharpe.is_some());
        assert!(result.trades_per_year.is_some());
    }

    #[test]
    fn sharpe_zero_period_no_annualized() {
        let pnl = vec![1.0, 2.0, 3.0];
        let same_ts = 1_000_000;

        let result = compute_sharpe(&pnl, same_ts, same_ts).unwrap();
        assert!(result.annualized_sharpe.is_none());
        assert!(result.trades_per_year.is_none());
    }

    #[test]
    fn psr_positive_sharpe() {
        // Positive Sharpe with roughly normal data should give PSR > 0.5
        let pnl = vec![1.0, 2.0, 1.5, 3.0, 2.5, 1.8, 2.2, 2.8, 1.3, 2.7];
        let mean = pnl.iter().sum::<f64>() / pnl.len() as f64;
        let sd = stddev_f64(&pnl).unwrap();
        let sharpe = mean / sd;

        let psr = compute_psr(&pnl, sharpe).unwrap();
        assert!(
            psr > 0.5,
            "PSR should be > 0.5 for positive Sharpe, got {psr}"
        );
    }

    #[test]
    fn psr_too_few_returns_none() {
        assert!(compute_psr(&[], 1.0).is_none());
        assert!(compute_psr(&[1.0], 1.0).is_none());
    }

    #[test]
    fn drawdown_known_series() {
        // P&L: [10, -5, -3, 8, -1]
        // Cumulative: [10, 5, 2, 10, 9]
        // Peak at idx 0: 10, trough at idx 2: 2, drawdown = 8
        // Recovery at idx 3: cumulative reaches 10 again
        let pnl = vec![10.0, -5.0, -3.0, 8.0, -1.0];
        let timestamps = vec![
            1_700_000_000_000i64,
            1_700_086_400_000,
            1_700_172_800_000,
            1_700_259_200_000,
            1_700_345_600_000,
        ];

        let result = compute_max_drawdown(&pnl, &timestamps).unwrap();
        assert!(
            (result.max_drawdown_abs - 8.0).abs() < 1e-10,
            "Max drawdown should be 8.0, got {}",
            result.max_drawdown_abs
        );
        assert!(result.recovery_date.is_some(), "Should have recovery date");
        // Current drawdown: peak=10, last=9, dd=1
        assert!(
            (result.current_drawdown_abs - 1.0).abs() < 1e-10,
            "Current drawdown should be 1.0, got {}",
            result.current_drawdown_abs
        );
    }

    #[test]
    fn drawdown_empty_returns_none() {
        assert!(compute_max_drawdown(&[], &[]).is_none());
    }

    #[test]
    fn extract_pnl_handles_parse_failures() {
        let records = vec![
            make_record(true, true, "1.50", 1000),
            make_record(true, true, "not_a_number", 2000),
            make_record(true, true, "-0.75", 3000),
        ];

        let pnl = extract_pnl_series(&records);
        assert_eq!(pnl.len(), 2, "Should skip unparseable P&L strings");
        assert!((pnl[0] - 1.50).abs() < 1e-10);
        assert!((pnl[1] - (-0.75)).abs() < 1e-10);
    }
}
