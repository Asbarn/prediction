use rust_decimal::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::analysis::output::{
    new_table, section_header, set_numeric_columns, LoadingSummary, Table,
};
use crate::analysis::stats::{mean_f64, median_f64, stddev_f64};
use crate::signal::types::ArbSignal;

/// A single cost component's descriptive statistics.
#[derive(Debug, Clone, Serialize)]
pub struct CostComponent {
    pub name: String,
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub pct_of_total: f64,
    pub sum: f64,
}

/// Aggregate cost audit result for a set of signals.
#[derive(Debug, Clone, Serialize)]
pub struct CostAuditResult {
    pub signal_count: usize,
    pub mean_raw_spread: f64,
    pub mean_net_edge: f64,
    pub mean_total_cost: f64,
    pub components: Vec<CostComponent>,
}

/// Full output wrapper for JSON serialization.
#[derive(Debug, Clone, Serialize)]
pub struct CostAuditOutput {
    pub loading: LoadingSummary,
    pub aggregate: CostAuditResult,
    pub by_event: Option<BTreeMap<String, CostAuditResult>>,
}

/// Compute cost audit statistics from a slice of signals.
///
/// Decomposes each `CostBreakdown` field into f64 vectors and computes
/// descriptive statistics (mean, median, std dev, % of total) for each
/// component. Components are sorted descending by mean magnitude.
pub fn compute_cost_audit(signals: &[ArbSignal]) -> CostAuditResult {
    if signals.is_empty() {
        return CostAuditResult {
            signal_count: 0,
            mean_raw_spread: 0.0,
            mean_net_edge: 0.0,
            mean_total_cost: 0.0,
            components: Vec::new(),
        };
    }

    // Extract component vectors
    let mut prediction_fees = Vec::with_capacity(signals.len());
    let mut options_fees = Vec::with_capacity(signals.len());
    let mut carry_costs = Vec::with_capacity(signals.len());
    let mut prediction_slippages = Vec::with_capacity(signals.len());
    let mut options_spread_costs = Vec::with_capacity(signals.len());
    let mut basis_risk_premiums = Vec::with_capacity(signals.len());
    let mut liquidity_factors = Vec::with_capacity(signals.len());
    let mut total_costs = Vec::with_capacity(signals.len());
    let mut raw_spreads = Vec::with_capacity(signals.len());
    let mut net_edges = Vec::with_capacity(signals.len());

    for s in signals {
        let cb = &s.cost_breakdown;
        prediction_fees.push(cb.prediction_fee.to_f64().unwrap_or(0.0));
        options_fees.push(cb.options_fee_estimate.to_f64().unwrap_or(0.0));
        carry_costs.push(cb.carry_cost.to_f64().unwrap_or(0.0));
        prediction_slippages.push(cb.prediction_slippage.to_f64().unwrap_or(0.0));
        options_spread_costs.push(cb.options_spread_cost.to_f64().unwrap_or(0.0));
        basis_risk_premiums.push(cb.basis_risk_premium.to_f64().unwrap_or(0.0));
        liquidity_factors.push(cb.liquidity_factor.to_f64().unwrap_or(0.0));
        total_costs.push(cb.total_cost.to_f64().unwrap_or(0.0));
        raw_spreads.push(s.raw_spread.to_f64().unwrap_or(0.0));
        net_edges.push(s.net_edge.to_f64().unwrap_or(0.0));
    }

    let mean_total_cost = mean_f64(&total_costs).unwrap_or(0.0);
    let mean_raw_spread = mean_f64(&raw_spreads).unwrap_or(0.0);
    let mean_net_edge = mean_f64(&net_edges).unwrap_or(0.0);

    let component_data: Vec<(&str, Vec<f64>)> = vec![
        ("prediction_fee", prediction_fees),
        ("options_fee_estimate", options_fees),
        ("carry_cost", carry_costs),
        ("prediction_slippage", prediction_slippages),
        ("options_spread_cost", options_spread_costs),
        ("basis_risk_premium", basis_risk_premiums),
        ("liquidity_factor", liquidity_factors),
    ];

    let mut components: Vec<CostComponent> = component_data
        .into_iter()
        .map(|(name, mut values)| {
            let mean = mean_f64(&values).unwrap_or(0.0);
            let sum: f64 = values.iter().sum();
            let std_dev = stddev_f64(&values).unwrap_or(0.0);
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = median_f64(&values).unwrap_or(0.0);
            let pct_of_total = if mean_total_cost.abs() > 1e-15 {
                mean / mean_total_cost * 100.0
            } else {
                0.0
            };

            CostComponent {
                name: name.to_string(),
                mean,
                median,
                std_dev,
                pct_of_total,
                sum,
            }
        })
        .collect();

    // Sort by mean magnitude descending (largest cost contributor first)
    components.sort_by(|a, b| {
        b.mean
            .abs()
            .partial_cmp(&a.mean.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    CostAuditResult {
        signal_count: signals.len(),
        mean_raw_spread,
        mean_net_edge,
        mean_total_cost,
        components,
    }
}

/// Build a comfy-table rendering of a CostAuditResult.
pub fn cost_audit_table(result: &CostAuditResult) -> Table {
    let mut table = new_table(&["Component", "Mean", "Median", "Std Dev", "% of Total"]);
    set_numeric_columns(&mut table, &[1, 2, 3, 4]);

    // Summary section
    section_header(&mut table, "=== SUMMARY ===", 5);
    table.add_row(vec![
        "Signal Count".to_string(),
        result.signal_count.to_string(),
        String::new(),
        String::new(),
        String::new(),
    ]);
    table.add_row(vec![
        "Mean Raw Spread".to_string(),
        format!("{:.6}", result.mean_raw_spread),
        String::new(),
        String::new(),
        String::new(),
    ]);
    table.add_row(vec![
        "Mean Net Edge".to_string(),
        format!("{:.6}", result.mean_net_edge),
        String::new(),
        String::new(),
        String::new(),
    ]);
    table.add_row(vec![
        "Mean Total Cost".to_string(),
        format!("{:.6}", result.mean_total_cost),
        String::new(),
        String::new(),
        String::new(),
    ]);

    // Cost breakdown section
    section_header(&mut table, "=== COST BREAKDOWN (by magnitude) ===", 5);
    for comp in &result.components {
        table.add_row(vec![
            comp.name.clone(),
            format!("{:.6}", comp.mean),
            format!("{:.6}", comp.median),
            format!("{:.6}", comp.std_dev),
            format!("{:.1}%", comp.pct_of_total),
        ]);
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_cost_audit_empty() {
        let result = compute_cost_audit(&[]);
        assert_eq!(result.signal_count, 0);
        assert!(result.components.is_empty());
    }

    #[test]
    fn cost_audit_table_renders() {
        let result = CostAuditResult {
            signal_count: 10,
            mean_raw_spread: 0.05,
            mean_net_edge: 0.02,
            mean_total_cost: 0.03,
            components: vec![CostComponent {
                name: "prediction_fee".to_string(),
                mean: 0.005,
                median: 0.004,
                std_dev: 0.001,
                pct_of_total: 16.7,
                sum: 0.05,
            }],
        };
        let table = cost_audit_table(&result);
        let rendered = format!("{table}");
        assert!(rendered.contains("prediction_fee"));
        assert!(rendered.contains("SUMMARY"));
        assert!(rendered.contains("COST BREAKDOWN"));
    }
}
