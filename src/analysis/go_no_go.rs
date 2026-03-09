//! Go/no-go statistical validation and decision report.
//!
//! Synthesizes all statistical metrics (autocorrelation-corrected t-test,
//! effective sample size, hit rate, Sharpe, PSR) into a final PROCEED /
//! DO NOT PROCEED / INSUFFICIENT DATA recommendation.

use serde::Serialize;

use crate::analysis::io::DateRange;
use crate::analysis::output::{new_table, section_header, set_numeric_columns, Table};
use crate::analysis::scoring::compute_psr;
use crate::analysis::stats::{
    autocorrelation_lag1, compute_corrected_edge_test, effective_sample_size, mean_f64, stddev_f64,
    wilson_ci,
};
use crate::signal::types::ArbSignal;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Final go/no-go decision.
#[derive(Debug, Clone, Serialize)]
pub enum GoNoGoDecision {
    Proceed,
    DoNotProceed,
    InsufficientData,
}

impl std::fmt::Display for GoNoGoDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GoNoGoDecision::Proceed => write!(f, "PROCEED"),
            GoNoGoDecision::DoNotProceed => write!(f, "DO NOT PROCEED"),
            GoNoGoDecision::InsufficientData => write!(f, "INSUFFICIENT DATA"),
        }
    }
}

/// Full go/no-go analysis report.
#[derive(Debug, Clone, Serialize)]
pub struct GoNoGoReport {
    pub decision: GoNoGoDecision,
    pub decision_reason: String,
    pub train_range: String,
    pub test_range: String,
    pub train_signals: usize,
    pub test_signals: usize,
    pub raw_n: usize,
    pub effective_n: usize,
    pub autocorrelation: f64,
    pub mean_edge: f64,
    pub ci_95_lower: f64,
    pub ci_95_upper: f64,
    pub p_value: f64,
    pub hit_rate: f64,
    pub hit_rate_ci_95: (f64, f64),
    pub sharpe_per_trade: Option<f64>,
    pub psr: Option<f64>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Run the full go/no-go analysis on a set of signals.
///
/// Splits signals into train/test by comparing each signal's wall-clock
/// timestamp against the provided date ranges. Only the test set drives
/// the recommendation.
pub fn run_go_no_go(
    signals: &[ArbSignal],
    train_range: &DateRange,
    test_range: &DateRange,
    min_effective_n: usize,
) -> GoNoGoReport {
    // Split signals by timestamp into train and test sets
    let mut train_signals: Vec<&ArbSignal> = Vec::new();
    let mut test_signals: Vec<&ArbSignal> = Vec::new();

    for signal in signals {
        let date = signal.timestamp.wall.date_naive();
        if date >= test_range.from && date <= test_range.to {
            test_signals.push(signal);
        } else if date >= train_range.from && date <= train_range.to {
            train_signals.push(signal);
        }
    }

    let train_count = train_signals.len();
    let test_count = test_signals.len();

    // Extract net_edge as f64 from test set
    let net_edges: Vec<f64> = test_signals
        .iter()
        .filter_map(|s| s.net_edge.to_string().parse::<f64>().ok())
        .collect();

    let raw_n = net_edges.len();

    // If no test data, return InsufficientData immediately
    if raw_n < 3 {
        return GoNoGoReport {
            decision: GoNoGoDecision::InsufficientData,
            decision_reason: format!(
                "Only {} test signals (need at least 3 for statistical analysis)",
                raw_n
            ),
            train_range: train_range.to_string(),
            test_range: test_range.to_string(),
            train_signals: train_count,
            test_signals: test_count,
            raw_n,
            effective_n: raw_n,
            autocorrelation: 0.0,
            mean_edge: 0.0,
            ci_95_lower: 0.0,
            ci_95_upper: 0.0,
            p_value: 1.0,
            hit_rate: 0.0,
            hit_rate_ci_95: (0.0, 0.0),
            sharpe_per_trade: None,
            psr: None,
            warnings: vec![format!("Very few test signals ({raw_n}) -- results may be unreliable")],
        };
    }

    // Corrected edge test (autocorrelation-aware t-test)
    let edge_test = compute_corrected_edge_test(&net_edges);

    let (mean_edge, ci_lower, ci_upper, p_value, autocorrelation, n_eff) =
        if let Some(ref result) = edge_test {
            (
                result.mean_edge,
                result.ci_95.0,
                result.ci_95.1,
                result.p_value,
                result.autocorrelation,
                result.effective_n,
            )
        } else {
            let rho = autocorrelation_lag1(&net_edges).unwrap_or(0.0);
            let n_eff = effective_sample_size(raw_n, rho);
            let mean = mean_f64(&net_edges).unwrap_or(0.0);
            (mean, 0.0, 0.0, 1.0, rho, n_eff)
        };

    // Hit rate
    let hits = net_edges.iter().filter(|&&e| e > 0.0).count();
    let hit_rate = hits as f64 / raw_n as f64;

    // Wilson CI on hit rate using n_eff
    let scaled_successes = (hit_rate * n_eff as f64).round() as usize;
    let hit_rate_ci = wilson_ci(scaled_successes, n_eff, 1.96).unwrap_or((0.0, 1.0));

    // Per-trade Sharpe and PSR
    let sharpe_per_trade = if let (Some(mean), Some(sd)) = (mean_f64(&net_edges), stddev_f64(&net_edges)) {
        if sd > 0.0 {
            Some(mean / sd)
        } else {
            None
        }
    } else {
        None
    };

    let psr = sharpe_per_trade.and_then(|s| compute_psr(&net_edges, s));

    // Build warnings
    let mut warnings = Vec::new();

    if autocorrelation > 0.5 {
        let pct = (n_eff as f64 / raw_n as f64) * 100.0;
        warnings.push(format!(
            "HIGH autocorrelation ({autocorrelation:.3}) -- effective sample size is {pct:.0}% of raw count"
        ));
    }

    if n_eff < min_effective_n {
        warnings.push(format!(
            "Effective sample size ({n_eff}) below minimum ({min_effective_n})"
        ));
    }

    if test_count < 10 {
        warnings.push(format!(
            "Very few test signals ({test_count}) -- results may be unreliable"
        ));
    }

    if train_count == 0 {
        warnings.push(
            "No training data found -- cannot confirm out-of-sample validity".to_string(),
        );
    }

    // Decision logic
    let (decision, decision_reason) = if n_eff < min_effective_n {
        (
            GoNoGoDecision::InsufficientData,
            format!(
                "Effective sample size ({n_eff}) is below minimum threshold ({min_effective_n})"
            ),
        )
    } else if ci_lower > 0.0 {
        (
            GoNoGoDecision::Proceed,
            format!(
                "95% CI lower bound ({ci_lower:.6}) > 0 -- statistically significant positive edge"
            ),
        )
    } else {
        (
            GoNoGoDecision::DoNotProceed,
            format!(
                "95% CI lower bound ({ci_lower:.6}) <= 0 -- edge not distinguishable from zero"
            ),
        )
    };

    GoNoGoReport {
        decision,
        decision_reason,
        train_range: train_range.to_string(),
        test_range: test_range.to_string(),
        train_signals: train_count,
        test_signals: test_count,
        raw_n,
        effective_n: n_eff,
        autocorrelation,
        mean_edge,
        ci_95_lower: ci_lower,
        ci_95_upper: ci_upper,
        p_value,
        hit_rate,
        hit_rate_ci_95: hit_rate_ci,
        sharpe_per_trade,
        psr,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Render a GoNoGoReport as a two-column key/value table.
pub fn go_no_go_table(report: &GoNoGoReport) -> Table {
    let mut table = new_table(&["Metric", "Value"]);
    set_numeric_columns(&mut table, &[1]);

    // Data Split section
    section_header(&mut table, "--- Data Split ---", 2);
    table.add_row(vec!["Train Range".to_string(), report.train_range.clone()]);
    table.add_row(vec!["Test Range".to_string(), report.test_range.clone()]);
    table.add_row(vec![
        "Train Signals".to_string(),
        report.train_signals.to_string(),
    ]);
    table.add_row(vec![
        "Test Signals".to_string(),
        report.test_signals.to_string(),
    ]);

    // Autocorrelation section
    section_header(&mut table, "--- Autocorrelation ---", 2);
    table.add_row(vec![
        "Lag-1 ACF".to_string(),
        format!("{:.4}", report.autocorrelation),
    ]);
    table.add_row(vec!["Raw n".to_string(), report.raw_n.to_string()]);
    table.add_row(vec![
        "Effective n".to_string(),
        report.effective_n.to_string(),
    ]);

    // Edge Analysis section
    section_header(&mut table, "--- Edge Analysis ---", 2);
    table.add_row(vec![
        "Mean Edge".to_string(),
        format!("{:.6}", report.mean_edge),
    ]);
    table.add_row(vec![
        "95% CI".to_string(),
        format!("[{:.6}, {:.6}]", report.ci_95_lower, report.ci_95_upper),
    ]);
    table.add_row(vec![
        "p-value".to_string(),
        format!("{:.4}", report.p_value),
    ]);

    // Performance section
    section_header(&mut table, "--- Performance ---", 2);
    table.add_row(vec![
        "Hit Rate".to_string(),
        format!(
            "{:.1}% [{:.1}%, {:.1}%]",
            report.hit_rate * 100.0,
            report.hit_rate_ci_95.0 * 100.0,
            report.hit_rate_ci_95.1 * 100.0,
        ),
    ]);
    table.add_row(vec![
        "Per-Trade Sharpe".to_string(),
        report
            .sharpe_per_trade
            .map(|s| format!("{s:.4}"))
            .unwrap_or_else(|| "N/A".to_string()),
    ]);
    table.add_row(vec![
        "PSR (prob Sharpe > 0)".to_string(),
        report
            .psr
            .map(|p| format!("{:.1}%", p * 100.0))
            .unwrap_or_else(|| "N/A".to_string()),
    ]);

    // Decision section
    section_header(&mut table, "--- Decision ---", 2);
    table.add_row(vec![
        "Recommendation".to_string(),
        report.decision.to_string(),
    ]);
    table.add_row(vec!["Reason".to_string(), report.decision_reason.clone()]);

    table
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{ConfidenceComponents, PricingMethod};
    use crate::signal::types::{ArbDirection, CostBreakdown, LegInfo, ThresholdStatus};
    use crate::types::{DualTimestamp, Venue};
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_signal_with_edge_and_date(net_edge: &str, date: NaiveDate) -> ArbSignal {
        let wall = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
        ArbSignal {
            signal_id: uuid::Uuid::now_v7().to_string(),
            event_id: "test-event".to_string(),
            direction: ArbDirection::BuyPredictionSellOptions,
            raw_spread: dec("0.05"),
            net_edge: dec(net_edge),
            confidence: 0.8,
            prediction_leg: LegInfo {
                venue: Venue::Polymarket,
                instrument_id: "TEST".to_string(),
                probability: dec("0.55"),
                executable_price: dec("0.54"),
                book_depth_levels: 5,
                fill_ratio: dec("0.95"),
            },
            options_leg: LegInfo {
                venue: Venue::Deribit,
                instrument_id: "TEST-OPT".to_string(),
                probability: dec("0.50"),
                executable_price: dec("0.49"),
                book_depth_levels: 3,
                fill_ratio: dec("0.90"),
            },
            timestamp: DualTimestamp {
                mono: tokio::time::Instant::now(),
                wall,
            },
            ttl_secs: 30,
            pricing_method: PricingMethod::CallSpreadReplication,
            confidence_components: ConfidenceComponents {
                iv_spread: 0.9,
                book_depth: 0.85,
                method_agreement: 0.78,
                solver_convergence: 0.95,
            },
            solver_meta: None,
            iv_spread: 0.02,
            skew_adjustment: -0.01,
            cost_breakdown: CostBreakdown {
                prediction_fee: dec("0.005"),
                options_fee_estimate: dec("0.0003"),
                carry_cost: dec("0.002"),
                prediction_slippage: dec("0.001"),
                options_spread_cost: dec("0.003"),
                basis_risk_premium: dec("0"),
                liquidity_factor: dec("0.95"),
                total_cost: dec("0.01"),
            },
            prediction_venue: Venue::Polymarket,
            threshold_status: ThresholdStatus::PassedBoth,
            threshold_value: dec("0.025"),
            threshold_components: None,
        }
    }

    fn make_test_signals(edges: &[&str], base_date: NaiveDate) -> Vec<ArbSignal> {
        edges
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                let date = base_date + chrono::Duration::days(i as i64);
                make_signal_with_edge_and_date(edge, date)
            })
            .collect()
    }

    #[test]
    fn all_positive_edges_returns_proceed() {
        // Train: Jan 1-6, Test: Jan 7-10
        let train_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        };
        let test_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        };

        // Train signals
        let mut signals = make_test_signals(
            &["0.02", "0.03", "0.01", "0.025", "0.015", "0.02"],
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        // Test signals: all clearly positive
        signals.extend(make_test_signals(
            &[
                "0.05", "0.04", "0.06", "0.03", "0.07", "0.05", "0.04", "0.06", "0.05", "0.04",
                "0.03", "0.05", "0.06", "0.04", "0.05", "0.03", "0.07", "0.05", "0.04", "0.06",
                "0.05", "0.04", "0.03", "0.05", "0.06",
            ],
            NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
        ));

        let report = run_go_no_go(&signals, &train_range, &test_range, 10);
        assert!(
            matches!(report.decision, GoNoGoDecision::Proceed),
            "Expected Proceed, got {}: {}",
            report.decision,
            report.decision_reason
        );
        assert!(!report.decision_reason.is_empty());
        assert!(report.mean_edge > 0.0);
        assert!(report.ci_95_lower > 0.0);
    }

    #[test]
    fn all_negative_edges_returns_do_not_proceed() {
        let train_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        };
        let test_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        };

        let mut signals = make_test_signals(
            &["-0.02", "-0.03", "-0.01", "-0.025", "-0.015", "-0.02"],
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        signals.extend(make_test_signals(
            &[
                "-0.05", "-0.04", "-0.06", "-0.03", "-0.07", "-0.05", "-0.04", "-0.06", "-0.05",
                "-0.04", "-0.03", "-0.05", "-0.06", "-0.04", "-0.05", "-0.03", "-0.07", "-0.05",
                "-0.04", "-0.06", "-0.05", "-0.04", "-0.03", "-0.05", "-0.06",
            ],
            NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
        ));

        let report = run_go_no_go(&signals, &train_range, &test_range, 10);
        assert!(
            matches!(report.decision, GoNoGoDecision::DoNotProceed),
            "Expected DoNotProceed, got {}: {}",
            report.decision,
            report.decision_reason
        );
        assert!(!report.decision_reason.is_empty());
    }

    #[test]
    fn few_signals_returns_insufficient_data() {
        let train_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        };
        let test_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        };

        // Only 5 test signals, min_effective_n = 30
        let mut signals = make_test_signals(
            &["0.02", "0.03"],
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        signals.extend(make_test_signals(
            &["0.05", "0.04", "0.06", "0.03", "0.07"],
            NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
        ));

        let report = run_go_no_go(&signals, &train_range, &test_range, 30);
        assert!(
            matches!(report.decision, GoNoGoDecision::InsufficientData),
            "Expected InsufficientData, got {}: {}",
            report.decision,
            report.decision_reason
        );
        assert!(!report.decision_reason.is_empty());
    }

    #[test]
    fn high_autocorrelation_includes_warning() {
        let train_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        };
        let test_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
        };

        // Create a strongly trending series (high positive autocorrelation)
        let mut signals = make_test_signals(
            &["0.01", "0.01", "0.01", "0.01", "0.01", "0.01"],
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );

        // Strongly trending test data: linearly increasing
        let test_edges: Vec<String> = (0..40).map(|i| format!("0.{:03}", 10 + i * 5)).collect();
        let test_edge_refs: Vec<&str> = test_edges.iter().map(|s| s.as_str()).collect();
        signals.extend(make_test_signals(
            &test_edge_refs,
            NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
        ));

        let report = run_go_no_go(&signals, &train_range, &test_range, 5);

        // If autocorrelation is high, there should be a warning
        if report.autocorrelation > 0.5 {
            assert!(
                report.warnings.iter().any(|w| w.contains("HIGH autocorrelation")),
                "Expected HIGH autocorrelation warning, warnings: {:?}",
                report.warnings
            );
        }
        // Verify raw_n > effective_n when autocorrelation is positive
        if report.autocorrelation > 0.0 {
            assert!(
                report.effective_n <= report.raw_n,
                "effective_n ({}) should be <= raw_n ({}) with positive autocorrelation",
                report.effective_n,
                report.raw_n
            );
        }
    }

    #[test]
    fn decision_reason_always_nonempty() {
        let train_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 6).unwrap(),
        };
        let test_range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        };

        // Test with sufficient positive data (Proceed)
        let mut signals = make_test_signals(
            &["0.02", "0.03", "0.01"],
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        signals.extend(make_test_signals(
            &[
                "0.05", "0.04", "0.06", "0.03", "0.07", "0.05", "0.04", "0.06", "0.05", "0.04",
                "0.03", "0.05", "0.06", "0.04", "0.05", "0.03", "0.07", "0.05", "0.04", "0.06",
                "0.05", "0.04", "0.03", "0.05", "0.06",
            ],
            NaiveDate::from_ymd_opt(2026, 1, 7).unwrap(),
        ));
        let report = run_go_no_go(&signals, &train_range, &test_range, 10);
        assert!(!report.decision_reason.is_empty(), "Proceed reason should be non-empty");

        // Test with no test data (InsufficientData)
        let empty_report = run_go_no_go(&[], &train_range, &test_range, 30);
        assert!(!empty_report.decision_reason.is_empty(), "InsufficientData reason should be non-empty");
    }
}
