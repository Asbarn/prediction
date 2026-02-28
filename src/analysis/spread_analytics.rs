//! Spread analytics computation layer.
//!
//! Provides pure computation functions that accept `&[SpreadResult]` and return
//! serializable result structs, plus table-rendering functions for terminal output.
//! Used by the `spread-analytics` CLI binary.

use std::collections::BTreeMap;

use chrono::Timelike;
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::analysis::output::{new_table, section_header, set_numeric_columns, Table};
use crate::analysis::output::LoadingSummary;
use crate::analysis::stats::{mean_f64, median_f64, percentile_f64, stddev_f64};
use crate::spread::patterns::{SpreadPattern, SpreadResult};

// ---------------------------------------------------------------------------
// Formatting constants
// ---------------------------------------------------------------------------

const SPREAD_DP: usize = 4;
const PERCENT_DP: usize = 1;

fn fmt_f64(v: f64, dp: usize) -> String {
    format!("{v:.dp$}")
}

fn fmt_opt(v: Option<f64>, dp: usize) -> String {
    match v {
        Some(val) => fmt_f64(val, dp),
        None => "-".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Result structs
// ---------------------------------------------------------------------------

/// Summary statistics for a set of spread values.
#[derive(Debug, Clone, Serialize)]
pub struct SpreadStats {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub stddev: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub p5: f64,
    pub p25: f64,
    pub p75: f64,
    pub p95: f64,
}

/// Distribution summary for net and gross spreads.
#[derive(Debug, Clone, Serialize)]
pub struct DistributionSummary {
    pub net_spread: SpreadStats,
    pub gross_spread: SpreadStats,
}

/// A single row in the hourly breakdown table.
#[derive(Debug, Clone, Serialize)]
pub struct HourlyRow {
    pub hour: u8,
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub stddev: Option<f64>,
    pub pct_positive: f64,
}

/// Hourly breakdown with exactly 24 rows (hours 0..23).
#[derive(Debug, Clone, Serialize)]
pub struct HourlyBreakdown {
    pub rows: Vec<HourlyRow>,
}

/// Statistics for a single venue pair, with per-direction breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct VenuePairStats {
    pub pair_label: String,
    pub directions: BTreeMap<String, SpreadStats>,
    pub total: SpreadStats,
}

/// Venue-pair breakdown containing all venue pairs found in the data.
#[derive(Debug, Clone, Serialize)]
pub struct VenuePairBreakdown {
    pub pairs: Vec<VenuePairStats>,
}

/// Complete analysis result for a set of spread records.
#[derive(Debug, Clone, Serialize)]
pub struct SpreadAnalysis {
    pub distribution: Option<DistributionSummary>,
    pub hourly: Option<HourlyBreakdown>,
    pub venue_pairs: Option<VenuePairBreakdown>,
}

/// Full output struct combining loading metadata and analysis results.
#[derive(Debug, Clone, Serialize)]
pub struct FullSpreadOutput {
    pub loading: LoadingSummary,
    pub aggregate: SpreadAnalysis,
    pub by_event: Option<BTreeMap<String, SpreadAnalysis>>,
}

// ---------------------------------------------------------------------------
// Helper: compute SpreadStats from a slice of f64
// ---------------------------------------------------------------------------

/// Compute summary statistics from a slice of f64 values.
/// Returns None if the slice is empty.
pub fn compute_spread_stats(values: &[f64]) -> Option<SpreadStats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Some(SpreadStats {
        count: values.len(),
        mean: mean_f64(values)?,
        median: median_f64(&sorted)?,
        stddev: stddev_f64(values),
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        p5: percentile_f64(&sorted, 5.0)?,
        p25: percentile_f64(&sorted, 25.0)?,
        p75: percentile_f64(&sorted, 75.0)?,
        p95: percentile_f64(&sorted, 95.0)?,
    })
}

// ---------------------------------------------------------------------------
// Computation functions (pure, accept &[SpreadResult])
// ---------------------------------------------------------------------------

/// Compute distribution summary statistics for net and gross spreads.
/// Returns None if records is empty.
pub fn compute_distribution(records: &[SpreadResult]) -> Option<DistributionSummary> {
    if records.is_empty() {
        return None;
    }
    let net_values: Vec<f64> = records.iter().filter_map(|r| r.net_spread.to_f64()).collect();
    let gross_values: Vec<f64> = records
        .iter()
        .filter_map(|r| r.gross_spread.to_f64())
        .collect();

    let net_stats = compute_spread_stats(&net_values)?;
    let gross_stats = compute_spread_stats(&gross_values)?;

    Some(DistributionSummary {
        net_spread: net_stats,
        gross_spread: gross_stats,
    })
}

/// Compute hourly breakdown of net spread statistics.
/// Always returns exactly 24 rows (hours 0..23). Hours with no data show zeroed stats.
/// Skips records with timestamp_ms <= 0 or where from_timestamp_millis returns None.
/// Returns None if records is empty.
pub fn compute_hourly(records: &[SpreadResult]) -> Option<HourlyBreakdown> {
    if records.is_empty() {
        return None;
    }

    // Pre-populate all 24 hours
    let mut buckets: BTreeMap<u8, Vec<f64>> = BTreeMap::new();
    for h in 0..24u8 {
        buckets.insert(h, Vec::new());
    }

    // Bucket records by UTC hour
    for record in records {
        if record.timestamp_ms <= 0 {
            continue;
        }
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(record.timestamp_ms) {
            let hour = dt.hour() as u8;
            if let Some(val) = record.net_spread.to_f64() {
                buckets.entry(hour).or_default().push(val);
            }
        }
    }

    let rows: Vec<HourlyRow> = (0..24u8)
        .map(|hour| {
            let values = buckets.get(&hour).cloned().unwrap_or_default();
            if values.is_empty() {
                HourlyRow {
                    hour,
                    count: 0,
                    mean: 0.0,
                    median: 0.0,
                    stddev: None,
                    pct_positive: 0.0,
                }
            } else {
                let positive_count = values.iter().filter(|&&v| v > 0.0).count();
                let pct_positive = (positive_count as f64 / values.len() as f64) * 100.0;

                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                HourlyRow {
                    hour,
                    count: values.len(),
                    mean: mean_f64(&values).unwrap_or(0.0),
                    median: median_f64(&sorted).unwrap_or(0.0),
                    stddev: stddev_f64(&values),
                    pct_positive,
                }
            }
        })
        .collect();

    Some(HourlyBreakdown { rows })
}

/// Compute venue-pair breakdown with per-direction detail.
/// Returns None if records is empty.
pub fn compute_venue_pairs(records: &[SpreadResult]) -> Option<VenuePairBreakdown> {
    if records.is_empty() {
        return None;
    }

    // Group by venue pair label, then by pattern within each pair
    let mut pair_groups: BTreeMap<&str, BTreeMap<SpreadPattern, Vec<f64>>> = BTreeMap::new();
    for record in records {
        let pair_label = record.pattern.venue_pair_label();
        if let Some(val) = record.net_spread.to_f64() {
            pair_groups
                .entry(pair_label)
                .or_default()
                .entry(record.pattern)
                .or_default()
                .push(val);
        }
    }

    let pairs: Vec<VenuePairStats> = pair_groups
        .into_iter()
        .filter_map(|(pair_label, direction_groups)| {
            let mut directions = BTreeMap::new();
            let mut all_values = Vec::new();

            for (pattern, values) in &direction_groups {
                all_values.extend(values.iter().copied());
                if let Some(stats) = compute_spread_stats(values) {
                    directions.insert(pattern.label().to_string(), stats);
                }
            }

            let total = compute_spread_stats(&all_values)?;
            Some(VenuePairStats {
                pair_label: pair_label.to_string(),
                directions,
                total,
            })
        })
        .collect();

    if pairs.is_empty() {
        None
    } else {
        Some(VenuePairBreakdown { pairs })
    }
}

/// Compute all three analysis sections from a set of spread records.
pub fn compute_analysis(records: &[SpreadResult]) -> SpreadAnalysis {
    SpreadAnalysis {
        distribution: compute_distribution(records),
        hourly: compute_hourly(records),
        venue_pairs: compute_venue_pairs(records),
    }
}

/// Group records by event_id.
pub fn group_by_event<'a>(records: &'a [SpreadResult]) -> BTreeMap<String, Vec<&'a SpreadResult>> {
    let mut groups: BTreeMap<String, Vec<&'a SpreadResult>> = BTreeMap::new();
    for record in records {
        groups.entry(record.event_id.clone()).or_default().push(record);
    }
    groups
}

// ---------------------------------------------------------------------------
// Table-rendering functions
// ---------------------------------------------------------------------------

/// Render distribution summary as a two-column comparison table (Net vs Gross).
pub fn distribution_table(summary: &DistributionSummary) -> Table {
    let mut table = new_table(&["Statistic", "Net Spread", "Gross Spread"]);
    set_numeric_columns(&mut table, &[1, 2]);

    let rows = [
        (
            "Count",
            summary.net_spread.count.to_string(),
            summary.gross_spread.count.to_string(),
        ),
        (
            "Mean",
            fmt_f64(summary.net_spread.mean, SPREAD_DP),
            fmt_f64(summary.gross_spread.mean, SPREAD_DP),
        ),
        (
            "Median",
            fmt_f64(summary.net_spread.median, SPREAD_DP),
            fmt_f64(summary.gross_spread.median, SPREAD_DP),
        ),
        (
            "Std Dev",
            fmt_opt(summary.net_spread.stddev, SPREAD_DP),
            fmt_opt(summary.gross_spread.stddev, SPREAD_DP),
        ),
        (
            "Min",
            fmt_f64(summary.net_spread.min, SPREAD_DP),
            fmt_f64(summary.gross_spread.min, SPREAD_DP),
        ),
        (
            "Max",
            fmt_f64(summary.net_spread.max, SPREAD_DP),
            fmt_f64(summary.gross_spread.max, SPREAD_DP),
        ),
        (
            "P5",
            fmt_f64(summary.net_spread.p5, SPREAD_DP),
            fmt_f64(summary.gross_spread.p5, SPREAD_DP),
        ),
        (
            "P25",
            fmt_f64(summary.net_spread.p25, SPREAD_DP),
            fmt_f64(summary.gross_spread.p25, SPREAD_DP),
        ),
        (
            "P75",
            fmt_f64(summary.net_spread.p75, SPREAD_DP),
            fmt_f64(summary.gross_spread.p75, SPREAD_DP),
        ),
        (
            "P95",
            fmt_f64(summary.net_spread.p95, SPREAD_DP),
            fmt_f64(summary.gross_spread.p95, SPREAD_DP),
        ),
    ];

    for (label, net, gross) in rows {
        table.add_row(vec![label.to_string(), net, gross]);
    }
    table
}

/// Render hourly breakdown as a 24-row table.
pub fn hourly_table(breakdown: &HourlyBreakdown) -> Table {
    let mut table = new_table(&["Hour", "Count", "Mean", "Median", "Std Dev", "% Pos"]);
    set_numeric_columns(&mut table, &[1, 2, 3, 4, 5]);

    for row in &breakdown.rows {
        if row.count == 0 {
            table.add_row(vec![
                format!("{:02}", row.hour),
                "0".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ]);
        } else {
            table.add_row(vec![
                format!("{:02}", row.hour),
                row.count.to_string(),
                fmt_f64(row.mean, SPREAD_DP),
                fmt_f64(row.median, SPREAD_DP),
                fmt_opt(row.stddev, SPREAD_DP),
                fmt_f64(row.pct_positive, PERCENT_DP),
            ]);
        }
    }
    table
}

/// Render venue-pair breakdown with per-direction rows and total row per pair.
pub fn venue_pair_table(breakdown: &VenuePairBreakdown) -> Table {
    let mut table = new_table(&["Direction", "Count", "Mean", "Median", "Std Dev"]);
    set_numeric_columns(&mut table, &[1, 2, 3, 4]);

    for pair in &breakdown.pairs {
        section_header(&mut table, &pair.pair_label, 5);

        for (direction_label, stats) in &pair.directions {
            table.add_row(vec![
                direction_label.clone(),
                stats.count.to_string(),
                fmt_f64(stats.mean, SPREAD_DP),
                fmt_f64(stats.median, SPREAD_DP),
                fmt_opt(stats.stddev, SPREAD_DP),
            ]);
        }

        // Total row
        table.add_row(vec![
            "TOTAL".to_string(),
            pair.total.count.to_string(),
            fmt_f64(pair.total.mean, SPREAD_DP),
            fmt_f64(pair.total.median, SPREAD_DP),
            fmt_opt(pair.total.stddev, SPREAD_DP),
        ]);
    }
    table
}

/// Return a vec of (section_title, table) pairs for each non-None analysis section.
pub fn analysis_tables(analysis: &SpreadAnalysis) -> Vec<(&str, Table)> {
    let mut tables = Vec::new();

    if let Some(ref dist) = analysis.distribution {
        tables.push(("Distribution Summary", distribution_table(dist)));
    }
    if let Some(ref hourly) = analysis.hourly {
        tables.push(("Hourly Breakdown (UTC)", hourly_table(hourly)));
    }
    if let Some(ref venue) = analysis.venue_pairs {
        tables.push(("Venue Pair Breakdown", venue_pair_table(venue)));
    }

    tables
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// Create a test SpreadResult with sensible defaults.
    fn test_spread_result(
        event_id: &str,
        pattern: SpreadPattern,
        net: &str,
        gross: &str,
        ts_ms: i64,
    ) -> SpreadResult {
        SpreadResult {
            event_id: event_id.to_string(),
            pattern,
            gross_spread: dec(gross),
            net_spread: dec(net),
            buy_fill_price: dec("0.50"),
            sell_fill_price: dec("0.55"),
            buy_fee: dec("0"),
            sell_fee: dec("0"),
            carry_cost: dec("0"),
            total_cost: dec("0"),
            basis_risk_premium: dec("0"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("1.0"),
            target_notional: dec("500"),
            timestamp_ms: ts_ms,
            poly_exchange_ts: None,
            kalshi_exchange_ts: None,
            threshold: None,
            threshold_components: None,
            threshold_status: None,
        }
    }

    #[test]
    fn test_compute_spread_stats_known_values() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        let stats = compute_spread_stats(&values).unwrap();
        assert_eq!(stats.count, 5);
        assert!((stats.mean - 3.0).abs() < 1e-10);
        assert!((stats.median - 3.0).abs() < 1e-10);
        assert!((stats.min - 1.0).abs() < 1e-10);
        assert!((stats.max - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_compute_spread_stats_empty() {
        assert!(compute_spread_stats(&[]).is_none());
    }

    #[test]
    fn test_compute_spread_stats_single() {
        let stats = compute_spread_stats(&[42.0]).unwrap();
        assert_eq!(stats.count, 1);
        assert!(stats.stddev.is_none(), "stddev should be None for n=1");
    }

    #[test]
    fn test_compute_distribution_empty() {
        assert!(compute_distribution(&[]).is_none());
    }

    #[test]
    fn test_compute_hourly_always_24_rows() {
        // Even with just 1 record at a single hour, we get 24 rows
        let records = vec![test_spread_result(
            "evt1",
            SpreadPattern::BuyPolyYesSellKalshiYes,
            "0.01",
            "0.02",
            1700000000000, // some valid timestamp
        )];
        let breakdown = compute_hourly(&records).unwrap();
        assert_eq!(breakdown.rows.len(), 24, "Must always produce 24 rows");
    }

    #[test]
    fn test_compute_hourly_skips_zero_timestamp() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                0, // should be skipped
            ),
            test_spread_result(
                "evt2",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                -100, // should be skipped
            ),
        ];
        let breakdown = compute_hourly(&records).unwrap();
        // All 24 hours should have count=0 since both records were skipped
        let total_count: usize = breakdown.rows.iter().map(|r| r.count).sum();
        assert_eq!(total_count, 0, "Records with timestamp_ms <= 0 should be skipped");
    }

    #[test]
    fn test_venue_pair_grouping() {
        // Different patterns but same venue_pair_label should be grouped together
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::SellPolyYesBuyKalshiYes,
                "-0.01",
                "-0.02",
                1700000000000,
            ),
        ];
        let breakdown = compute_venue_pairs(&records).unwrap();
        // Both patterns are kalshi_polymarket, so only 1 pair
        assert_eq!(breakdown.pairs.len(), 1);
        assert_eq!(breakdown.pairs[0].pair_label, "kalshi_polymarket");
        assert_eq!(
            breakdown.pairs[0].directions.len(),
            2,
            "Should have 2 direction entries"
        );
    }

    #[test]
    fn test_venue_pair_directions_are_labeled() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::SellPolyNoBuyKalshiNo,
                "0.02",
                "0.03",
                1700000000000,
            ),
        ];
        let breakdown = compute_venue_pairs(&records).unwrap();
        let pair = &breakdown.pairs[0];
        assert!(
            pair.directions
                .contains_key("buy_poly_yes_sell_kalshi_yes"),
            "Direction key should match pattern.label()"
        );
        assert!(
            pair.directions.contains_key("sell_poly_no_buy_kalshi_no"),
            "Direction key should match pattern.label()"
        );
    }

    #[test]
    fn test_group_by_event() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt2",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.03",
                "0.04",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::SellPolyYesBuyKalshiYes,
                "-0.01",
                "-0.02",
                1700000000000,
            ),
        ];
        let groups = group_by_event(&records);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["evt1"].len(), 2);
        assert_eq!(groups["evt2"].len(), 1);
    }

    #[test]
    fn test_compute_analysis_empty_returns_all_none() {
        let analysis = compute_analysis(&[]);
        assert!(analysis.distribution.is_none());
        assert!(analysis.hourly.is_none());
        assert!(analysis.venue_pairs.is_none());
    }

    #[test]
    fn test_distribution_table_renders() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.03",
                "0.04",
                1700000000000,
            ),
        ];
        let dist = compute_distribution(&records).unwrap();
        let table = distribution_table(&dist);
        let rendered = format!("{table}");
        assert!(rendered.contains("Statistic"));
        assert!(rendered.contains("Net Spread"));
        assert!(rendered.contains("Gross Spread"));
        assert!(rendered.contains("Count"));
        assert!(rendered.contains("Mean"));
    }

    #[test]
    fn test_hourly_table_renders_24_rows() {
        let records = vec![test_spread_result(
            "evt1",
            SpreadPattern::BuyPolyYesSellKalshiYes,
            "0.01",
            "0.02",
            1700000000000,
        )];
        let breakdown = compute_hourly(&records).unwrap();
        let table = hourly_table(&breakdown);
        let rendered = format!("{table}");
        // Should contain hours from 00 to 23
        assert!(rendered.contains("00"));
        assert!(rendered.contains("23"));
    }

    #[test]
    fn test_venue_pair_table_renders() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::SellPolyYesBuyKalshiYes,
                "-0.01",
                "-0.02",
                1700000000000,
            ),
        ];
        let vp = compute_venue_pairs(&records).unwrap();
        let table = venue_pair_table(&vp);
        let rendered = format!("{table}");
        assert!(rendered.contains("kalshi_polymarket"));
        assert!(rendered.contains("TOTAL"));
    }

    #[test]
    fn test_analysis_tables_returns_three_sections() {
        let records = vec![
            test_spread_result(
                "evt1",
                SpreadPattern::BuyPolyYesSellKalshiYes,
                "0.01",
                "0.02",
                1700000000000,
            ),
            test_spread_result(
                "evt1",
                SpreadPattern::SellPolyYesBuyKalshiYes,
                "-0.01",
                "-0.02",
                1700000000000,
            ),
        ];
        let analysis = compute_analysis(&records);
        let tables = analysis_tables(&analysis);
        assert_eq!(tables.len(), 3, "Should have distribution, hourly, venue-pair");
        assert_eq!(tables[0].0, "Distribution Summary");
        assert_eq!(tables[1].0, "Hourly Breakdown (UTC)");
        assert_eq!(tables[2].0, "Venue Pair Breakdown");
    }
}
