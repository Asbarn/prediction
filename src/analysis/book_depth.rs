//! Book depth analysis for order book quality assessment.
//!
//! Computes effective spread, fill ratios, depth levels, and a composite depth
//! quality score from ArbSignal records. Helps operators determine whether
//! negative edge comes from poor book quality vs cost model issues.

use std::collections::BTreeMap;

use rust_decimal::prelude::*;
use serde::Serialize;

use crate::analysis::output::{
    new_table, section_header, set_numeric_columns, LoadingSummary, Table,
};
use crate::analysis::stats::{mean_f64, median_f64};
use crate::signal::types::ArbSignal;

// ---------------------------------------------------------------------------
// Result structs
// ---------------------------------------------------------------------------

/// Per-instrument depth metrics.
#[derive(Debug, Clone, Serialize)]
pub struct InstrumentDepth {
    pub instrument_id: String,
    pub venue: String,
    pub signal_count: usize,
    pub effective_spread_mean: f64,
    pub effective_spread_median: f64,
    pub fill_ratio_mean: f64,
    pub fill_ratio_min: f64,
    pub depth_levels_mean: f64,
    /// Composite: `fill_ratio_mean * min(depth_levels_mean / 10.0, 1.0)`
    pub depth_quality_score: f64,
    /// `target_notional * fill_ratio_mean`
    pub estimated_max_fill: f64,
}

/// Aggregate book depth result across all signals.
#[derive(Debug, Clone, Serialize)]
pub struct BookDepthResult {
    pub signal_count: usize,
    pub instrument_count: usize,
    pub effective_spread_mean: f64,
    pub effective_spread_median: f64,
    pub fill_ratio_mean: f64,
    pub depth_levels_mean: f64,
    pub depth_quality_score: f64,
    /// Per-instrument breakdown, sorted by depth_quality_score ascending (worst first).
    pub instruments: Vec<InstrumentDepth>,
}

/// Full output for JSON serialization.
#[derive(Debug, Clone, Serialize)]
pub struct BookDepthOutput {
    pub loading: LoadingSummary,
    pub aggregate: BookDepthResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_event: Option<BTreeMap<String, BookDepthResult>>,
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Compute book depth metrics from a set of ArbSignals.
pub fn compute_book_depth(signals: &[ArbSignal], target_notional: f64) -> BookDepthResult {
    if signals.is_empty() {
        return BookDepthResult {
            signal_count: 0,
            instrument_count: 0,
            effective_spread_mean: 0.0,
            effective_spread_median: 0.0,
            fill_ratio_mean: 0.0,
            depth_levels_mean: 0.0,
            depth_quality_score: 0.0,
            instruments: Vec::new(),
        };
    }

    // Extract per-signal metrics from prediction leg
    let mut all_spreads = Vec::with_capacity(signals.len());
    let mut all_fill_ratios = Vec::with_capacity(signals.len());
    let mut all_depth_levels = Vec::with_capacity(signals.len());

    // Group by instrument_id
    let mut by_instrument: BTreeMap<String, Vec<&ArbSignal>> = BTreeMap::new();

    for signal in signals {
        let leg = &signal.prediction_leg;
        let spread = (leg.executable_price - leg.probability)
            .to_f64()
            .unwrap_or(0.0)
            .abs();
        let fill = leg.fill_ratio.to_f64().unwrap_or(0.0);
        let depth = leg.book_depth_levels as f64;

        all_spreads.push(spread);
        all_fill_ratios.push(fill);
        all_depth_levels.push(depth);

        by_instrument
            .entry(leg.instrument_id.clone())
            .or_default()
            .push(signal);
    }

    // Aggregate stats
    let eff_spread_mean = mean_f64(&all_spreads).unwrap_or(0.0);
    let mut sorted_spreads = all_spreads.clone();
    sorted_spreads.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let eff_spread_median = median_f64(&sorted_spreads).unwrap_or(0.0);
    let fill_mean = mean_f64(&all_fill_ratios).unwrap_or(0.0);
    let depth_mean = mean_f64(&all_depth_levels).unwrap_or(0.0);
    let quality = fill_mean * (depth_mean / 10.0).min(1.0);

    // Per-instrument breakdown
    let mut instruments: Vec<InstrumentDepth> = by_instrument
        .into_iter()
        .map(|(inst_id, sigs)| {
            let mut spreads = Vec::with_capacity(sigs.len());
            let mut fills = Vec::with_capacity(sigs.len());
            let mut depths = Vec::with_capacity(sigs.len());
            let venue_name = sigs[0].prediction_leg.venue.to_string();

            for s in &sigs {
                let leg = &s.prediction_leg;
                spreads.push(
                    (leg.executable_price - leg.probability)
                        .to_f64()
                        .unwrap_or(0.0)
                        .abs(),
                );
                fills.push(leg.fill_ratio.to_f64().unwrap_or(0.0));
                depths.push(leg.book_depth_levels as f64);
            }

            let inst_spread_mean = mean_f64(&spreads).unwrap_or(0.0);
            let mut sorted = spreads.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let inst_spread_median = median_f64(&sorted).unwrap_or(0.0);
            let inst_fill_mean = mean_f64(&fills).unwrap_or(0.0);
            let inst_fill_min = fills
                .iter()
                .copied()
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(0.0);
            let inst_depth_mean = mean_f64(&depths).unwrap_or(0.0);
            let inst_quality = inst_fill_mean * (inst_depth_mean / 10.0).min(1.0);

            InstrumentDepth {
                instrument_id: inst_id,
                venue: venue_name,
                signal_count: sigs.len(),
                effective_spread_mean: inst_spread_mean,
                effective_spread_median: inst_spread_median,
                fill_ratio_mean: inst_fill_mean,
                fill_ratio_min: inst_fill_min,
                depth_levels_mean: inst_depth_mean,
                depth_quality_score: inst_quality,
                estimated_max_fill: target_notional * inst_fill_mean,
            }
        })
        .collect();

    // Sort by depth_quality_score ascending (worst first)
    instruments.sort_by(|a, b| {
        a.depth_quality_score
            .partial_cmp(&b.depth_quality_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    BookDepthResult {
        signal_count: signals.len(),
        instrument_count: instruments.len(),
        effective_spread_mean: eff_spread_mean,
        effective_spread_median: eff_spread_median,
        fill_ratio_mean: fill_mean,
        depth_levels_mean: depth_mean,
        depth_quality_score: quality,
        instruments,
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Build display tables for a BookDepthResult.
///
/// Returns `(title, table)` pairs following the spread_analytics pattern.
pub fn book_depth_tables(result: &BookDepthResult) -> Vec<(String, Table)> {
    let mut tables = Vec::new();

    // Aggregate metrics table
    let mut agg = new_table(&["Metric", "Value"]);
    set_numeric_columns(&mut agg, &[1]);

    section_header(&mut agg, "=== AGGREGATE ===", 2);
    agg.add_row(vec![
        "Signal Count".to_string(),
        result.signal_count.to_string(),
    ]);
    agg.add_row(vec![
        "Instrument Count".to_string(),
        result.instrument_count.to_string(),
    ]);
    agg.add_row(vec![
        "Effective Spread (Mean)".to_string(),
        format!("{:.6}", result.effective_spread_mean),
    ]);
    agg.add_row(vec![
        "Effective Spread (Median)".to_string(),
        format!("{:.6}", result.effective_spread_median),
    ]);
    agg.add_row(vec![
        "Fill Ratio (Mean)".to_string(),
        format!("{:.6}", result.fill_ratio_mean),
    ]);
    agg.add_row(vec![
        "Depth Levels (Mean)".to_string(),
        format!("{:.6}", result.depth_levels_mean),
    ]);
    agg.add_row(vec![
        "Depth Quality Score".to_string(),
        format!("{:.6}", result.depth_quality_score),
    ]);

    tables.push(("Aggregate Depth Metrics".to_string(), agg));

    // Per-instrument table (worst first)
    if !result.instruments.is_empty() {
        let mut inst_table = new_table(&[
            "Instrument",
            "Signals",
            "Eff Spread",
            "Fill Ratio",
            "Depth Lvls",
            "Quality",
            "Est Max Fill",
        ]);
        set_numeric_columns(&mut inst_table, &[1, 2, 3, 4, 5, 6]);

        for inst in &result.instruments {
            inst_table.add_row(vec![
                inst.instrument_id.clone(),
                inst.signal_count.to_string(),
                format!("{:.4}", inst.effective_spread_mean),
                format!("{:.4}", inst.fill_ratio_mean),
                format!("{:.1}", inst.depth_levels_mean),
                format!("{:.4}", inst.depth_quality_score),
                format!("{:.2}", inst.estimated_max_fill),
            ]);
        }

        tables.push((
            "Per-Instrument Depth (worst first)".to_string(),
            inst_table,
        ));
    }

    tables
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{ConfidenceComponents, PricingMethod};
    use crate::signal::types::{
        ArbDirection, CostBreakdown, LegInfo, ThresholdStatus,
    };
    use crate::types::{DualTimestamp, Venue};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_signal(inst_id: &str, prob: &str, exec_price: &str, fill: &str, depth: usize) -> ArbSignal {
        ArbSignal {
            signal_id: "test-signal".to_string(),
            event_id: "evt-1".to_string(),
            direction: ArbDirection::BuyPredictionSellOptions,
            raw_spread: dec("0.05"),
            net_edge: dec("0.03"),
            confidence: 0.8,
            prediction_leg: LegInfo {
                venue: Venue::Polymarket,
                instrument_id: inst_id.to_string(),
                probability: dec(prob),
                executable_price: dec(exec_price),
                book_depth_levels: depth,
                fill_ratio: dec(fill),
            },
            options_leg: LegInfo {
                venue: Venue::Deribit,
                instrument_id: "OPT-1".to_string(),
                probability: dec("0.60"),
                executable_price: dec("0.59"),
                book_depth_levels: 10,
                fill_ratio: dec("1.0"),
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
            cost_breakdown: CostBreakdown {
                prediction_fee: dec("0.005"),
                options_fee_estimate: dec("0.0003"),
                carry_cost: dec("0.002"),
                prediction_slippage: dec("0.001"),
                options_spread_cost: dec("0.003"),
                basis_risk_premium: dec("0"),
                liquidity_factor: dec("0.95"),
                total_cost: dec("0.0113"),
            },
            prediction_venue: Venue::Polymarket,
            threshold_status: ThresholdStatus::PassedBoth,
            threshold_value: dec("0.025"),
            threshold_components: None,
        }
    }

    #[test]
    fn empty_signals_returns_zero() {
        let result = compute_book_depth(&[], 500.0);
        assert_eq!(result.signal_count, 0);
        assert_eq!(result.instrument_count, 0);
        assert_eq!(result.depth_quality_score, 0.0);
    }

    #[test]
    fn single_signal_computes_correctly() {
        let signals = vec![make_signal("INST-A", "0.50", "0.52", "0.90", 8)];
        let result = compute_book_depth(&signals, 500.0);

        assert_eq!(result.signal_count, 1);
        assert_eq!(result.instrument_count, 1);

        // effective_spread = |0.52 - 0.50| = 0.02
        assert!((result.effective_spread_mean - 0.02).abs() < 1e-6);
        // fill_ratio = 0.90
        assert!((result.fill_ratio_mean - 0.90).abs() < 1e-6);
        // depth_levels = 8.0
        assert!((result.depth_levels_mean - 8.0).abs() < 1e-6);
        // quality = 0.90 * min(8.0/10.0, 1.0) = 0.90 * 0.8 = 0.72
        assert!((result.depth_quality_score - 0.72).abs() < 1e-6);

        // Per-instrument
        assert_eq!(result.instruments.len(), 1);
        assert_eq!(result.instruments[0].instrument_id, "INST-A");
        assert!((result.instruments[0].estimated_max_fill - 450.0).abs() < 1e-6);
    }

    #[test]
    fn multiple_instruments_sorted_worst_first() {
        let signals = vec![
            // INST-A: good quality (high fill, deep book)
            make_signal("INST-A", "0.50", "0.51", "0.95", 10),
            // INST-B: poor quality (low fill, shallow book)
            make_signal("INST-B", "0.50", "0.55", "0.40", 3),
        ];
        let result = compute_book_depth(&signals, 500.0);

        assert_eq!(result.instrument_count, 2);
        // INST-B should be first (worst quality)
        assert_eq!(result.instruments[0].instrument_id, "INST-B");
        assert_eq!(result.instruments[1].instrument_id, "INST-A");

        // INST-B quality = 0.40 * min(3.0/10.0, 1.0) = 0.40 * 0.3 = 0.12
        assert!((result.instruments[0].depth_quality_score - 0.12).abs() < 1e-6);
        // INST-A quality = 0.95 * min(10.0/10.0, 1.0) = 0.95 * 1.0 = 0.95
        assert!((result.instruments[1].depth_quality_score - 0.95).abs() < 1e-6);
    }

    #[test]
    fn tables_render_without_panic() {
        let signals = vec![
            make_signal("INST-A", "0.50", "0.51", "0.95", 10),
            make_signal("INST-B", "0.50", "0.55", "0.40", 3),
        ];
        let result = compute_book_depth(&signals, 500.0);
        let tables = book_depth_tables(&result);
        assert_eq!(tables.len(), 2); // aggregate + per-instrument
        for (title, table) in &tables {
            assert!(!title.is_empty());
            let rendered = format!("{table}");
            assert!(!rendered.is_empty());
        }
    }
}
