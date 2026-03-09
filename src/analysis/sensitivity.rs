//! Perturbation-based sensitivity analysis for cost components.
//!
//! Ranks cost components by their impact on net edge by perturbing each
//! component at several scaling factors and measuring the resulting change
//! in mean net edge.

use rust_decimal::prelude::*;
use serde::Serialize;

use crate::analysis::output::{new_table, section_header, set_numeric_columns, Table};
use crate::signal::types::{ArbSignal, CostBreakdown};

/// The six cost components we analyse.
const COMPONENT_NAMES: &[&str] = &[
    "prediction_fee",
    "options_fee_estimate",
    "carry_cost",
    "prediction_slippage",
    "options_spread_cost",
    "basis_risk_premium",
];

/// Default perturbation factors: 0.5x through 1.5x in quarter steps.
const DEFAULT_FACTORS: &[f64] = &[0.5, 0.75, 1.0, 1.25, 1.5];

/// Sensitivity result for a single cost component.
#[derive(Debug, Clone, Serialize)]
pub struct SensitivityResult {
    /// Name of the cost component.
    pub component_name: String,
    /// Mean net edge at the baseline (factor = 1.0).
    pub base_mean_net_edge: f64,
    /// Slope: change in mean net edge per unit change in factor.
    /// Negative slope means increasing this component decreases net edge (expected).
    pub slope: f64,
    /// Rank by |slope| (1 = largest impact). Assigned after sorting.
    pub impact_rank: usize,
    /// (factor, mean_adjusted_net_edge) pairs for each perturbation factor.
    pub factor_results: Vec<(f64, f64)>,
}

/// Aggregated sensitivity report.
#[derive(Debug, Clone, Serialize)]
pub struct SensitivityReport {
    /// Number of signals analysed.
    pub signal_count: usize,
    /// True when fewer than 20 signals were used (results may be noisy).
    pub min_sample_warning: bool,
    /// Per-component results, sorted by |slope| descending.
    pub results: Vec<SensitivityResult>,
}

/// Extract the f64 value of a named cost component from a `CostBreakdown`.
pub fn get_component_value(cb: &CostBreakdown, name: &str) -> f64 {
    match name {
        "prediction_fee" => cb.prediction_fee.to_f64().unwrap_or(0.0),
        "options_fee_estimate" => cb.options_fee_estimate.to_f64().unwrap_or(0.0),
        "carry_cost" => cb.carry_cost.to_f64().unwrap_or(0.0),
        "prediction_slippage" => cb.prediction_slippage.to_f64().unwrap_or(0.0),
        "options_spread_cost" => cb.options_spread_cost.to_f64().unwrap_or(0.0),
        "basis_risk_premium" => cb.basis_risk_premium.to_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Compute sensitivity for a single cost component across the given factors.
///
/// For each factor, the adjusted net edge for each signal is:
///   `adjusted = original_net_edge + component_value * (1.0 - factor)`
///
/// Rationale: scaling a cost component by `factor` changes total_cost by
/// `component * (factor - 1)`, so net_edge decreases by that amount.
pub fn component_sensitivity(
    signals: &[ArbSignal],
    component_name: &str,
    factors: &[f64],
) -> SensitivityResult {
    if signals.is_empty() || factors.is_empty() {
        return SensitivityResult {
            component_name: component_name.to_string(),
            base_mean_net_edge: 0.0,
            slope: 0.0,
            impact_rank: 0,
            factor_results: Vec::new(),
        };
    }

    // Precompute per-signal: (net_edge_f64, component_value)
    let signal_data: Vec<(f64, f64)> = signals
        .iter()
        .map(|s| {
            let ne = s.net_edge.to_f64().unwrap_or(0.0);
            let cv = get_component_value(&s.cost_breakdown, component_name);
            (ne, cv)
        })
        .collect();

    let n = signal_data.len() as f64;

    let mut factor_results: Vec<(f64, f64)> = Vec::with_capacity(factors.len());
    let mut base_mean = 0.0;

    for &factor in factors {
        let sum: f64 = signal_data
            .iter()
            .map(|(ne, cv)| ne + cv * (1.0 - factor))
            .sum();
        let mean = sum / n;
        factor_results.push((factor, mean));
        if (factor - 1.0).abs() < 1e-12 {
            base_mean = mean;
        }
    }

    // Compute slope via (y_last - y_first) / (factor_last - factor_first)
    let slope = if factors.len() >= 2 {
        let first = factor_results.first().unwrap();
        let last = factor_results.last().unwrap();
        let df = last.0 - first.0;
        if df.abs() > 1e-15 {
            (last.1 - first.1) / df
        } else {
            0.0
        }
    } else {
        0.0
    };

    SensitivityResult {
        component_name: component_name.to_string(),
        base_mean_net_edge: base_mean,
        slope,
        impact_rank: 0, // assigned later after sorting
        factor_results,
    }
}

/// Run sensitivity analysis across all cost components.
///
/// Returns results sorted by |slope| descending (largest impact first).
/// Sets `min_sample_warning` if fewer than 20 signals are provided.
pub fn sensitivity_analysis(signals: &[ArbSignal]) -> SensitivityReport {
    let min_sample_warning = signals.len() < 20;

    let mut results: Vec<SensitivityResult> = COMPONENT_NAMES
        .iter()
        .map(|name| component_sensitivity(signals, name, DEFAULT_FACTORS))
        .collect();

    // Sort by |slope| descending
    results.sort_by(|a, b| {
        b.slope
            .abs()
            .partial_cmp(&a.slope.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Assign 1-based ranks
    for (i, r) in results.iter_mut().enumerate() {
        r.impact_rank = i + 1;
    }

    SensitivityReport {
        signal_count: signals.len(),
        min_sample_warning,
        results,
    }
}

/// Build a table rendering of a `SensitivityReport`.
pub fn sensitivity_table(report: &SensitivityReport) -> Table {
    let mut table = new_table(&[
        "Rank",
        "Component",
        "Slope (d_edge/d_factor)",
        "At 0.5x",
        "At 1.0x",
        "At 1.5x",
    ]);
    set_numeric_columns(&mut table, &[0, 2, 3, 4, 5]);

    if report.min_sample_warning {
        section_header(
            &mut table,
            "WARNING: < 20 signals -- results may be noisy",
            6,
        );
    }

    for r in &report.results {
        // Find the mean net edge at specific factors
        let at_05 = r
            .factor_results
            .iter()
            .find(|(f, _)| (*f - 0.5).abs() < 1e-12)
            .map(|(_, v)| format!("{v:.6}"))
            .unwrap_or_default();
        let at_10 = r
            .factor_results
            .iter()
            .find(|(f, _)| (*f - 1.0).abs() < 1e-12)
            .map(|(_, v)| format!("{v:.6}"))
            .unwrap_or_default();
        let at_15 = r
            .factor_results
            .iter()
            .find(|(f, _)| (*f - 1.5).abs() < 1e-12)
            .map(|(_, v)| format!("{v:.6}"))
            .unwrap_or_default();

        table.add_row(vec![
            r.impact_rank.to_string(),
            r.component_name.clone(),
            format!("{:.6}", r.slope),
            at_05,
            at_10,
            at_15,
        ]);
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{ConfidenceComponents, PricingMethod};
    use crate::signal::types::{
        ArbDirection, ArbSignal, CostBreakdown, LegInfo, ThresholdStatus,
    };
    use crate::types::{DualTimestamp, Venue};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_signal_with_costs(
        net_edge: &str,
        prediction_fee: &str,
        options_fee: &str,
        carry: &str,
        slippage: &str,
        spread_cost: &str,
        basis_risk: &str,
    ) -> ArbSignal {
        let cb = CostBreakdown {
            prediction_fee: dec(prediction_fee),
            options_fee_estimate: dec(options_fee),
            carry_cost: dec(carry),
            prediction_slippage: dec(slippage),
            options_spread_cost: dec(spread_cost),
            basis_risk_premium: dec(basis_risk),
            liquidity_factor: dec("1.0"),
            total_cost: dec("0.01"), // not used by sensitivity
        };

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
            timestamp: DualTimestamp::now(),
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
            cost_breakdown: cb,
            prediction_venue: Venue::Polymarket,
            threshold_status: ThresholdStatus::PassedBoth,
            threshold_value: dec("0.025"),
            threshold_components: None,
        }
    }

    #[test]
    fn get_component_value_extracts_correct_fields() {
        let cb = CostBreakdown {
            prediction_fee: dec("0.005"),
            options_fee_estimate: dec("0.0003"),
            carry_cost: dec("0.002"),
            prediction_slippage: dec("0.001"),
            options_spread_cost: dec("0.003"),
            basis_risk_premium: dec("0.0015"),
            liquidity_factor: dec("0.95"),
            total_cost: dec("0.012"),
        };

        assert!((get_component_value(&cb, "prediction_fee") - 0.005).abs() < 1e-10);
        assert!((get_component_value(&cb, "options_fee_estimate") - 0.0003).abs() < 1e-10);
        assert!((get_component_value(&cb, "carry_cost") - 0.002).abs() < 1e-10);
        assert!((get_component_value(&cb, "prediction_slippage") - 0.001).abs() < 1e-10);
        assert!((get_component_value(&cb, "options_spread_cost") - 0.003).abs() < 1e-10);
        assert!((get_component_value(&cb, "basis_risk_premium") - 0.0015).abs() < 1e-10);
        assert!((get_component_value(&cb, "unknown_field") - 0.0).abs() < 1e-10);
    }

    #[test]
    fn dominant_component_ranks_first() {
        // Create signals where prediction_fee is much larger than others
        let signals: Vec<ArbSignal> = (0..5)
            .map(|_| {
                make_signal_with_costs(
                    "0.01",  // net_edge
                    "0.050", // prediction_fee -- dominant
                    "0.001", // options_fee
                    "0.001", // carry
                    "0.001", // slippage
                    "0.001", // spread_cost
                    "0.001", // basis_risk
                )
            })
            .collect();

        let report = sensitivity_analysis(&signals);
        assert_eq!(report.signal_count, 5);
        assert!(report.min_sample_warning); // < 20 signals
        assert_eq!(report.results.len(), 6);

        // The largest component (prediction_fee = 0.050) should rank #1
        let top = &report.results[0];
        assert_eq!(top.impact_rank, 1);
        assert_eq!(top.component_name, "prediction_fee");
        // Slope should be negative (increasing cost decreases edge)
        assert!(
            top.slope < 0.0,
            "Slope should be negative, got {}",
            top.slope
        );
        // |slope| for prediction_fee should be largest
        assert!(top.slope.abs() > report.results[1].slope.abs());
    }

    #[test]
    fn empty_signals_returns_empty_results() {
        let report = sensitivity_analysis(&[]);
        assert_eq!(report.signal_count, 0);
        assert!(report.min_sample_warning);
        assert_eq!(report.results.len(), 6);
        for r in &report.results {
            assert!((r.slope).abs() < 1e-15);
            assert!(r.factor_results.is_empty());
        }
    }

    #[test]
    fn slope_sign_correctness() {
        // All components positive -> increasing any factor should decrease net edge
        let signals: Vec<ArbSignal> = (0..3)
            .map(|_| {
                make_signal_with_costs(
                    "0.03", "0.005", "0.003", "0.002", "0.001", "0.004", "0.001",
                )
            })
            .collect();

        let report = sensitivity_analysis(&signals);
        for r in &report.results {
            // slope = d(mean_net_edge) / d(factor)
            // When factor increases, cost increases, net_edge decreases => slope <= 0
            assert!(
                r.slope <= 0.0,
                "Component {} should have non-positive slope, got {}",
                r.component_name,
                r.slope
            );
        }
    }

    #[test]
    fn sensitivity_table_renders() {
        let signals: Vec<ArbSignal> = (0..3)
            .map(|_| {
                make_signal_with_costs(
                    "0.03", "0.005", "0.003", "0.002", "0.001", "0.004", "0.001",
                )
            })
            .collect();

        let report = sensitivity_analysis(&signals);
        let table = sensitivity_table(&report);
        let rendered = format!("{table}");
        assert!(rendered.contains("Rank"));
        assert!(rendered.contains("Component"));
        assert!(rendered.contains("Slope"));
        assert!(rendered.contains("WARNING"));
        assert!(rendered.contains("prediction_fee"));
    }

    #[test]
    fn report_serializes_to_json() {
        let signals: Vec<ArbSignal> = (0..3)
            .map(|_| {
                make_signal_with_costs(
                    "0.03", "0.005", "0.003", "0.002", "0.001", "0.004", "0.001",
                )
            })
            .collect();

        let report = sensitivity_analysis(&signals);
        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["results"].is_array());
        assert_eq!(parsed["signal_count"], 3);
        assert_eq!(parsed["min_sample_warning"], true);
    }
}
