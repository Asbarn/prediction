# Phase 27: Spread Analytics CLI - Research

**Researched:** 2026-02-28
**Domain:** Spread distribution computation, hourly time-bucket analysis, venue-pair breakdown for CLI binary
**Confidence:** HIGH

## Summary

Phase 27 adds the spread analytics computation layer to the existing `spread-analytics` CLI binary. Phase 26 already built all infrastructure: the `analysis::stats` module (mean, stddev, percentile, wilson_ci), the `analysis::io` module (DateRange, load_jsonl, LoadResult), the `analysis::output` module (OutputFormat, render_output, new_table, set_numeric_columns, section_header, LoadingSummary), and the CLI binary skeleton (`src/bin/spread_analytics.rs`) with all flags wired up (--from, --to, --last, --output, --by-event, --log-dir). The binary currently loads SpreadResult records and displays a LoadingSummary placeholder. Phase 27 replaces that placeholder with three analysis sections: spread distribution summary, hourly breakdown, and venue-pair breakdown.

The implementation is straightforward because all building blocks exist. The data loading, date filtering, and output infrastructure are proven and tested (574 tests pass). The SpreadResult type already derives Deserialize with all fields accessible. The stats module already provides mean_decimal, mean_f64, stddev_f64, percentile_f64, median_f64, and wilson_ci. What Phase 27 adds is: (1) a new `analysis::spread_analytics` module with computation functions that accept `&[SpreadResult]` and return serializable result structs, (2) table-rendering functions for each analysis section, and (3) wiring these into the existing CLI binary to replace the placeholder output.

The critical design decision for Phase 27 is grouping strategy. The success criteria require venue-pair breakdown "with directional detail, never mixed into a single aggregate." This means the primary grouping key is `venue_pair_label()` (e.g., "kalshi_polymarket"), and within each venue pair, directional detail shows per-SpreadPattern stats. The `--by-event` flag adds an outer grouping by `event_id`. All three analyses (distribution summary, hourly, venue-pair) must support the `--by-event` modifier.

**Primary recommendation:** Create a single new module `src/analysis/spread_analytics.rs` with three pure computation functions (one per analysis section) that return `Serialize`-deriving result structs. Wire them into the existing binary, replacing the LoadingSummary with the actual analysis output. Keep the LoadingSummary as the first output section for context, followed by the three analysis tables.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SPREAD-01 | User can view spread distribution summary statistics (count, mean, median, stddev, min, max, p5/p25/p75/p95) for net and gross spreads over a date range | All stats functions exist in analysis::stats (mean_decimal, mean_f64, stddev_f64, percentile_f64, median_f64). Compute by collecting net_spread and gross_spread from loaded SpreadResult records into sorted Vec<f64>, then call stats functions. |
| SPREAD-02 | User can view hourly time-bucket analysis showing per-hour spread statistics across 24 UTC hours to identify when opportunities cluster | Extract hour from SpreadResult.timestamp_ms via chrono::DateTime::from_timestamp_millis().hour(). Bucket into BTreeMap<u8, Vec<f64>> for 0..24 hours. Compute per-bucket stats using same stats functions. |
| SPREAD-03 | User can view venue-pair breakdown showing spread statistics grouped by venue pair with directional detail | Group by SpreadResult.pattern.venue_pair_label() for venue-pair sections, then sub-group by SpreadResult.pattern for directional rows within each venue pair. |
</phase_requirements>

## Standard Stack

### Core (Already Available -- No New Dependencies)

| Library | Version | Purpose | Status |
|---------|---------|---------|--------|
| `analysis::stats` | (project module) | mean_decimal, mean_f64, stddev_f64, percentile_f64, median_f64, wilson_ci | Built in Phase 26-01 |
| `analysis::io` | (project module) | DateRange, load_jsonl, LoadResult | Built in Phase 26-01 |
| `analysis::output` | (project module) | OutputFormat, render_output, new_table, set_numeric_columns, section_header, Table re-export | Built in Phase 26-02 |
| `spread-analytics` binary | (project binary) | CLI skeleton with all flags, loads SpreadResult, renders LoadingSummary | Built in Phase 26-02 |
| `rust_decimal` | 1.40 | Decimal arithmetic for spread values | Existing |
| `chrono` | 0.4 | Timestamp parsing, hour extraction | Existing |
| `comfy-table` | 7 | Terminal table rendering (via output module re-export) | Added in Phase 26 |
| `serde` / `serde_json` | 1.0 | Serialization for JSON output mode | Existing |
| `clap` | 4.5 | CLI argument parsing with derive | Existing |

### What NOT to Add

| Do Not Add | Reason |
|------------|--------|
| Any new crate dependency | All computation is covered by existing stats module + std library |
| polars / ndarray | Vec<f64> operations with iterators are sufficient for this scale |
| plotters / textplots | Terminal tables are the output format; JSON output for external tooling |
| tokio | Binary uses synchronous fn main() -- no async needed |

**Installation:** No changes to Cargo.toml needed for Phase 27. All dependencies were added in Phase 26.

## Architecture Patterns

### Recommended Module Structure

```
src/analysis/
    mod.rs                   # Add: pub mod spread_analytics;
    stats.rs                 # EXISTS -- pure stats functions
    io.rs                    # EXISTS -- DateRange, load_jsonl
    output.rs                # EXISTS -- OutputFormat, render_output, table helpers
    spread_analytics.rs      # NEW -- spread computation + table rendering
```

### Pattern 1: Computation Function Returns Serializable Struct

**What:** Each analysis section has a pure computation function that accepts `&[SpreadResult]` and returns a struct deriving `Serialize`. The binary calls the computation function, then passes the result to `render_output` with a table-building closure.

**When to use:** Every analysis section (distribution, hourly, venue-pair).

**Why:** Separates computation from presentation. The same result struct serves both table and JSON output modes. Unit-testable without output formatting.

```rust
// In src/analysis/spread_analytics.rs

use serde::Serialize;
use crate::spread::patterns::SpreadResult;

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

pub fn compute_distribution(records: &[SpreadResult]) -> Option<DistributionSummary> {
    if records.is_empty() { return None; }
    // Collect net_spread and gross_spread into sorted Vec<f64>
    // Call stats functions
    // Return DistributionSummary
}
```

### Pattern 2: BTreeMap Bucketing for Ordered Output

**What:** Use `BTreeMap<K, Vec<SpreadResult>>` (or references) for grouping, so iteration order is deterministic (hours 0-23, venue pairs alphabetical).

**When to use:** Hourly bucketing (BTreeMap<u8, ...>), venue-pair grouping (BTreeMap<&str, ...>).

**Why:** HashMap iteration order is random. BTreeMap gives sorted keys, which means hourly rows appear 00..23 and venue pairs appear alphabetically without a separate sort step.

```rust
use std::collections::BTreeMap;

pub fn bucket_by_hour(records: &[SpreadResult]) -> BTreeMap<u8, Vec<&SpreadResult>> {
    let mut buckets: BTreeMap<u8, Vec<&SpreadResult>> = BTreeMap::new();
    for record in records {
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(record.timestamp_ms) {
            let hour = dt.hour() as u8;
            buckets.entry(hour).or_default().push(record);
        }
    }
    buckets
}
```

### Pattern 3: Dual-Layer Grouping for --by-event

**What:** When `--by-event` is set, wrap all analyses in an outer `BTreeMap<String, ...>` keyed by event_id. Run the same computation functions on each event's subset of records.

**When to use:** All three analysis sections when --by-event flag is active.

**Why:** The success criteria require "all three analyses additionally broken down per event_id." The simplest implementation groups records by event_id first, then runs the standard computation on each group.

```rust
pub fn group_by_event(records: &[SpreadResult]) -> BTreeMap<String, Vec<&SpreadResult>> {
    let mut groups: BTreeMap<String, Vec<&SpreadResult>> = BTreeMap::new();
    for record in records {
        groups.entry(record.event_id.clone()).or_default().push(record);
    }
    groups
}
```

### Pattern 4: Table Rendering Alongside Computation

**What:** Each analysis section has a paired table-building function that converts the result struct into a comfy_table::Table.

**When to use:** Every analysis section.

```rust
use crate::analysis::output::{new_table, set_numeric_columns, section_header, Table};

pub fn distribution_table(summary: &DistributionSummary) -> Table {
    let mut table = new_table(&["Metric", "Net Spread", "Gross Spread"]);
    set_numeric_columns(&mut table, &[1, 2]);
    table.add_row(vec![
        "Count".to_string(),
        summary.net_spread.count.to_string(),
        summary.gross_spread.count.to_string(),
    ]);
    // ... mean, median, stddev, min, max, p5, p25, p75, p95 rows
    table
}
```

### Anti-Patterns to Avoid

- **Mixing Decimal and f64 in output:** Convert Decimal to f64 once at the stats boundary. Display with fixed decimal places (4dp for spreads, 2dp for percentages). Use `format!("{:.4}", value)` consistently.
- **Inline computation in the binary:** Keep `src/bin/spread_analytics.rs` thin -- it parses args, loads data, calls computation functions, renders output. All logic lives in `src/analysis/spread_analytics.rs`.
- **Forgetting empty-data cases:** Every computation function must return `Option` or handle the zero-records case gracefully. "No data in range" is a valid output, not a panic.
- **Using `pattern.label()` for venue-pair grouping:** Use `pattern.venue_pair_label()` which normalizes direction (Kalshi-Poly and Poly-Kalshi both produce "kalshi_polymarket"). For directional detail within a venue pair, THEN use `pattern.label()`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Statistical functions | Custom mean/stddev/percentile | `analysis::stats::*` | Already tested with 12 unit tests in Phase 26 |
| Date-range file loading | Custom file enumeration | `DateRange::files_in_dir()` + `load_jsonl()` | Already tested with 10 unit tests in Phase 26 |
| Table formatting | Manual println! with format strings | `output::new_table()` + `set_numeric_columns()` | comfy-table handles alignment, wrapping, dynamic width |
| JSON output | Custom JSON building | `render_output()` with `Serialize` structs | render_output already handles Table/Json dispatch |
| CLI arg parsing | Manual arg parsing | Existing `Cli` struct in spread_analytics.rs | clap Parser already wired with all flags |

**Key insight:** Phase 26 built everything except the domain-specific computation. Phase 27 is purely additive: one new module file, modifications to the existing binary, and additions to mod.rs.

## Common Pitfalls

### Pitfall 1: Venue-Pair Mixing in Aggregate Statistics

**What goes wrong:** Computing a single aggregate distribution over all SpreadResult records regardless of venue pair. The success criteria explicitly state: "never mixed into a single aggregate."
**Why it happens:** It is the natural first implementation -- collect all net_spread values, compute stats.
**How to avoid:** Always compute distribution stats per venue pair. Show an aggregate ONLY if it is labeled "All Pairs (Aggregate)" and appears after per-pair sections. The venue-pair breakdown IS the primary output, not an optional drill-down.
**Warning signs:** A single statistics table without venue-pair labels.

### Pitfall 2: Displaying All 4 SpreadPattern Variants Redundantly

**What goes wrong:** The 4 SpreadPattern variants (BuyPolyYesSellKalshiYes, SellPolyYesBuyKalshiYes, BuyPolyNoSellKalshiNo, SellPolyNoBuyKalshiNo) produce symmetric spreads. Patterns 1 and 4 are algebraic complements; patterns 2 and 3 are algebraic complements. Showing all 4 with identical statistics doubles the output for no new information.
**Why it happens:** Pattern.all() returns 4 variants, and the loop processes all of them.
**How to avoid:** Group by `venue_pair_label()` first (gives one group for all Kalshi-Polymarket patterns). Within each venue pair, show directional detail as the buy direction: "Buy Poly / Sell Kalshi" and "Buy Kalshi / Sell Poly". This collapses 4 rows to 2 meaningful directions per venue pair.
**Warning signs:** Output table has 4 rows per venue pair with near-identical absolute values.

### Pitfall 3: Decimal-to-f64 Conversion Losing Precision for Display

**What goes wrong:** Converting `Decimal` to `f64` for stats computation is correct (stats inherently use floating point). But displaying the f64 result without fixed formatting produces inconsistent decimal places (e.g., "0.020000000000000004" vs "0.02").
**Why it happens:** f64 display defaults to full precision.
**How to avoid:** Always use `format!("{:.4}", value)` for spread values (4 decimal places). Define formatting constants: SPREAD_DP = 4, PERCENT_DP = 1, COUNT_DP = 0. Apply consistently across all tables.
**Warning signs:** Numbers in output have varying decimal places or show floating-point artifacts.

### Pitfall 4: Hourly Buckets Missing Hours With No Data

**What goes wrong:** BTreeMap only contains hours that have data. Hours with zero spread records (e.g., 3 AM UTC if the system was down) are simply absent from the output, making the "24-row table" fewer than 24 rows.
**Why it happens:** BTreeMap::entry only inserts when records exist for that hour.
**How to avoid:** Pre-populate all 24 hours (0..24) in the BTreeMap before bucketing. Hours with no data show "0" count and "-" for statistics. The output MUST always be exactly 24 rows for easy visual scanning.
**Warning signs:** Hourly table has fewer than 24 rows.

### Pitfall 5: --by-event Producing Unreadable Output for Many Events

**What goes wrong:** If there are 20+ events, repeating all three analysis tables for each event produces hundreds of lines of output.
**Why it happens:** The naive implementation repeats the full analysis for each event_id group.
**How to avoid:** For table output, use section_header() to clearly separate events. For JSON output, nest the per-event results under an "events" key. Consider: the --by-event flag adds per-event sections AFTER the aggregate analysis, not instead of it. Keep aggregate as the primary view.
**Warning signs:** Output exceeds terminal height with no visual separation between events.

### Pitfall 6: timestamp_ms = 0 or Negative Values

**What goes wrong:** Malformed or old-format records might have timestamp_ms = 0, which `DateTime::from_timestamp_millis(0)` maps to 1970-01-01 00:00 UTC. This pollutes hour 0's bucket.
**Why it happens:** Edge cases in JSONL data from crashes or schema evolution.
**How to avoid:** Validate timestamp_ms > 0 and within a reasonable range (e.g., after 2025-01-01). Skip records with invalid timestamps with a warning count.
**Warning signs:** Hour 0 has an anomalously high count compared to neighboring hours.

## Code Examples

### Example 1: Computing SpreadStats from a Slice of f64 Values

```rust
use crate::analysis::stats::{mean_f64, stddev_f64, percentile_f64, median_f64};

fn compute_spread_stats(values: &[f64]) -> Option<SpreadStats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    Some(SpreadStats {
        count: values.len(),
        mean: mean_f64(values)?,
        median: median_f64(&sorted)?,
        stddev: stddev_f64(values), // None if n < 2
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        p5: percentile_f64(&sorted, 5.0)?,
        p25: percentile_f64(&sorted, 25.0)?,
        p75: percentile_f64(&sorted, 75.0)?,
        p95: percentile_f64(&sorted, 95.0)?,
    })
}
```

### Example 2: Extracting UTC Hour from timestamp_ms

```rust
fn extract_hour(timestamp_ms: i64) -> Option<u8> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.hour() as u8)
}
```

### Example 3: Converting Decimal Fields to f64 for Stats

```rust
use rust_decimal::prelude::ToPrimitive;

fn extract_net_spreads(records: &[SpreadResult]) -> Vec<f64> {
    records
        .iter()
        .filter_map(|r| r.net_spread.to_f64())
        .collect()
}

fn extract_gross_spreads(records: &[SpreadResult]) -> Vec<f64> {
    records
        .iter()
        .filter_map(|r| r.gross_spread.to_f64())
        .collect()
}
```

### Example 4: Venue-Pair Grouping with Directional Sub-Groups

```rust
use std::collections::BTreeMap;
use crate::spread::patterns::{SpreadPattern, SpreadResult};

/// Group records by venue pair label, then by pattern within each pair.
fn group_by_venue_pair(
    records: &[SpreadResult],
) -> BTreeMap<&'static str, BTreeMap<SpreadPattern, Vec<&SpreadResult>>> {
    let mut pairs: BTreeMap<&'static str, BTreeMap<SpreadPattern, Vec<&SpreadResult>>> =
        BTreeMap::new();
    for record in records {
        let pair_label = record.pattern.venue_pair_label();
        pairs
            .entry(pair_label)
            .or_default()
            .entry(record.pattern)
            .or_default()
            .push(record);
    }
    pairs
}
```

### Example 5: Rendering Distribution Summary Table

```rust
use crate::analysis::output::{new_table, set_numeric_columns, Table};

fn distribution_table(summary: &DistributionSummary) -> Table {
    let mut table = new_table(&["Statistic", "Net Spread", "Gross Spread"]);
    set_numeric_columns(&mut table, &[1, 2]);

    let rows = [
        ("Count", format!("{}", summary.net_spread.count), format!("{}", summary.gross_spread.count)),
        ("Mean", format!("{:.4}", summary.net_spread.mean), format!("{:.4}", summary.gross_spread.mean)),
        ("Median", format!("{:.4}", summary.net_spread.median), format!("{:.4}", summary.gross_spread.median)),
        ("Std Dev", fmt_opt(summary.net_spread.stddev), fmt_opt(summary.gross_spread.stddev)),
        ("Min", format!("{:.4}", summary.net_spread.min), format!("{:.4}", summary.gross_spread.min)),
        ("Max", format!("{:.4}", summary.net_spread.max), format!("{:.4}", summary.gross_spread.max)),
        ("P5", format!("{:.4}", summary.net_spread.p5), format!("{:.4}", summary.gross_spread.p5)),
        ("P25", format!("{:.4}", summary.net_spread.p25), format!("{:.4}", summary.gross_spread.p25)),
        ("P75", format!("{:.4}", summary.net_spread.p75), format!("{:.4}", summary.gross_spread.p75)),
        ("P95", format!("{:.4}", summary.net_spread.p95), format!("{:.4}", summary.gross_spread.p95)),
    ];

    for (label, net, gross) in rows {
        table.add_row(vec![label.to_string(), net, gross]);
    }
    table
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{:.4}", x)).unwrap_or_else(|| "-".to_string())
}
```

## Existing Infrastructure Inventory (Phase 26 Deliverables)

This section documents exactly what Phase 26 built, verified against source code, so the planner knows what exists and what needs to be created.

### analysis::stats (src/analysis/stats.rs -- 187 lines, 12 tests)

| Function | Signature | Notes |
|----------|-----------|-------|
| `mean_decimal` | `(&[Decimal]) -> Option<Decimal>` | Full Decimal precision |
| `mean_f64` | `(&[f64]) -> Option<f64>` | For statistical computations |
| `stddev_f64` | `(&[f64]) -> Option<f64>` | Sample stddev (n-1 denominator), None if n < 2 |
| `percentile_f64` | `(&[f64], f64) -> Option<f64>` | Caller must pre-sort. p is 0-100. |
| `median_f64` | `(&[f64]) -> Option<f64>` | Convenience wrapper for percentile(50.0) |
| `wilson_ci` | `(usize, usize, f64) -> Option<(f64, f64)>` | Wilson score CI for proportions |

### analysis::io (src/analysis/io.rs -- 281 lines, 10 tests)

| Item | Type | Notes |
|------|------|-------|
| `DateRange` | struct | `from: NaiveDate`, `to: NaiveDate` |
| `DateRange::from_args` | method | Resolves --from/--to, --last N, or error |
| `DateRange::files_in_dir` | method | Enumerates `{YYYY-MM-DD}.jsonl` files |
| `DateRange::files_in_dir_prefixed` | method | Enumerates `{prefix}{YYYY-MM-DD}.jsonl` files |
| `LoadResult<T>` | struct | records, errors, files_loaded, files_missing |
| `load_jsonl<T>` | function | Tolerant line-by-line parsing, skips errors |

### analysis::output (src/analysis/output.rs -- 156 lines, 4 tests)

| Item | Type | Notes |
|------|------|-------|
| `OutputFormat` | enum | Table, Json -- derives clap::ValueEnum |
| `render_output<T: Serialize>` | function | Dispatches table_fn or JSON serialization |
| `new_table` | function | Creates table with dynamic arrangement and headers |
| `set_numeric_columns` | function | Right-justifies specified columns |
| `section_header` | function | Inserts section header row spanning columns |
| `LoadingSummary` | struct | date_range, files_loaded, files_missing, records_loaded, parse_errors, events_found |
| `render_loading_summary` | function | Renders LoadingSummary as table or JSON |
| `Table` | re-export | comfy_table::Table re-exported |

### spread_analytics binary (src/bin/spread_analytics.rs -- 81 lines)

| Item | Current State | Phase 27 Change |
|------|--------------|-----------------|
| `Cli` struct | All flags wired (--from, --to, --last, --output, --by-event, --log-dir) | No changes to CLI flags |
| `main()` | Loads SpreadResult via load_jsonl, counts unique events, renders LoadingSummary | Replace LoadingSummary-only output with full analysis sections |
| Data loading | `load_jsonl::<SpreadResult>(&files)` works | Keep as-is |
| Output | `render_loading_summary(&summary, &cli.output)` | Add distribution, hourly, venue-pair sections below the loading summary |

## SpreadResult Schema (Fields Used by Phase 27)

| Field | Type | Used For | Access |
|-------|------|----------|--------|
| `net_spread` | `Decimal` (serde str) | Distribution stats, all analyses | `record.net_spread.to_f64()` |
| `gross_spread` | `Decimal` (serde str) | Distribution stats (gross column) | `record.gross_spread.to_f64()` |
| `pattern` | `SpreadPattern` enum | Venue-pair grouping, directional detail | `record.pattern.venue_pair_label()`, `record.pattern.label()` |
| `event_id` | `String` | --by-event grouping | `record.event_id.clone()` |
| `timestamp_ms` | `i64` | Hourly bucketing | `DateTime::from_timestamp_millis(record.timestamp_ms)` |
| `total_cost` | `Decimal` (serde str) | Cost breakdown in distribution (optional enrichment) | `record.total_cost.to_f64()` |
| `buy_fill_ratio` | `Decimal` (serde str) | Fill quality summary (optional enrichment) | Not needed for SPREAD-01/02/03 |
| `sell_fill_ratio` | `Decimal` (serde str) | Fill quality summary (optional enrichment) | Not needed for SPREAD-01/02/03 |

### SpreadPattern.venue_pair_label() Mapping

| Pattern Variant | venue_pair_label() | Direction |
|----------------|-------------------|-----------|
| BuyPolyYesSellKalshiYes | "kalshi_polymarket" | Buy Poly, Sell Kalshi |
| SellPolyYesBuyKalshiYes | "kalshi_polymarket" | Buy Kalshi, Sell Poly |
| BuyPolyNoSellKalshiNo | "kalshi_polymarket" | Buy Poly NO, Sell Kalshi NO |
| SellPolyNoBuyKalshiNo | "kalshi_polymarket" | Buy Kalshi NO, Sell Poly NO |

Note: Currently all 4 patterns map to "kalshi_polymarket" because the system only has Polymarket-Kalshi spread computation (Deribit spreads go through the signal/arb pipeline, not the spread logger). The code must still support "deribit_polymarket" and "deribit_kalshi" labels for forward compatibility, but current soak test data will only contain "kalshi_polymarket" records. The venue-pair breakdown section should display all pairs found in the data, which will correctly show only the pairs that exist.

## Output Design

### Expected CLI Output Structure (Table Mode)

```
=== Spread Analytics ===

+----------------+-------+
| Metric         | Value |
+----------------+-------+
| Date Range     | 2026-02-25 to 2026-02-28 |
| Files Loaded   | 4     |
| Records Loaded | 12345 |
| Parse Errors   | 0     |
+----------------+-------+

--- Distribution Summary ---
+----------+-------------+--------------+
| Stat     | Net Spread  | Gross Spread |
+----------+-------------+--------------+
| Count    |       12345 |        12345 |
| Mean     |      0.0123 |       0.0456 |
| Median   |      0.0080 |       0.0350 |
| Std Dev  |      0.0234 |       0.0198 |
| Min      |     -0.0500 |      -0.0100 |
| Max      |      0.1200 |       0.1500 |
| P5       |     -0.0300 |       0.0050 |
| P25      |      0.0010 |       0.0200 |
| P75      |      0.0200 |       0.0650 |
| P95      |      0.0600 |       0.0900 |
+----------+-------------+--------------+

--- Hourly Breakdown (UTC) ---
+------+-------+---------+---------+---------+
| Hour | Count | Mean    | Median  | Std Dev |
+------+-------+---------+---------+---------+
|   00 |   520 |  0.0105 |  0.0080 |  0.0210 |
|   01 |   510 |  0.0098 |  0.0075 |  0.0205 |
| ...  |       |         |         |         |
|   23 |   515 |  0.0110 |  0.0085 |  0.0215 |
+------+-------+---------+---------+---------+

--- Venue Pair: kalshi_polymarket ---
+-----------------------------------+-------+---------+---------+---------+
| Direction                         | Count | Mean    | Median  | Std Dev |
+-----------------------------------+-------+---------+---------+---------+
| buy_poly_yes_sell_kalshi_yes      |  3100 |  0.0130 |  0.0090 |  0.0220 |
| sell_poly_yes_buy_kalshi_yes      |  3080 | -0.0130 | -0.0090 |  0.0220 |
| buy_poly_no_sell_kalshi_no        |  3090 | -0.0125 | -0.0088 |  0.0218 |
| sell_poly_no_buy_kalshi_no        |  3075 |  0.0125 |  0.0088 |  0.0218 |
| TOTAL                             | 12345 |  0.0000 |  0.0000 |  0.0220 |
+-----------------------------------+-------+---------+---------+---------+
```

### JSON Output Structure

```json
{
  "loading": { ... },
  "distribution": {
    "net_spread": { "count": 12345, "mean": 0.0123, ... },
    "gross_spread": { "count": 12345, "mean": 0.0456, ... }
  },
  "hourly": [
    { "hour": 0, "count": 520, "mean_net": 0.0105, ... },
    ...
  ],
  "venue_pairs": {
    "kalshi_polymarket": {
      "total": { "count": 12345, ... },
      "directions": {
        "buy_poly_yes_sell_kalshi_yes": { "count": 3100, ... },
        ...
      }
    }
  },
  "by_event": null
}
```

When `--by-event` is passed, the `"by_event"` key contains a map from event_id to the same structure (distribution, hourly, venue_pairs).

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| LoadingSummary placeholder output | Full analysis output (distribution + hourly + venue-pair) | Phase 27 (this phase) | Replaces placeholder with actual analysis |
| No analysis module | analysis::spread_analytics module | Phase 27 (this phase) | First domain-specific computation in analysis crate |

## Open Questions

1. **Whether to show positive-spread percentage per hour**
   - What we know: The success criteria mention "reveals when arbitrage opportunities cluster." Count + mean + median + stddev cover this. A "% positive" column would explicitly show which hours have more opportunities.
   - What's unclear: Whether this is clutter or valuable signal.
   - Recommendation: Include it. One extra column, directly answers "when do opportunities cluster." The column header should be "% Pos" with 1 decimal place.

2. **Whether to include aggregate distribution before venue-pair breakdown**
   - What we know: Success criteria say "never mixed into a single aggregate" for venue-pair breakdown. But the distribution summary (SPREAD-01) does not have this restriction.
   - What's unclear: Should SPREAD-01 show aggregate distribution across all venue pairs, or per-venue-pair distributions?
   - Recommendation: Show aggregate distribution first (SPREAD-01), then venue-pair breakdown (SPREAD-03) which repeats stats per pair. The aggregate gives a quick overview; the breakdown gives per-pair detail. This matches the success criteria: SPREAD-01 is "for net and gross spreads over a date range" (aggregate), SPREAD-03 is "grouped by venue pair."

3. **Hourly breakdown: net spread only, or both net and gross?**
   - What we know: The hourly table needs to show "per-hour spread statistics." Having both net and gross doubles the column count.
   - Recommendation: Show net spread in the hourly table (primary metric for actionable opportunities). Gross spread hourly is available via --by-event or JSON output but not needed in the default 24-row table.

## Sources

### Primary (HIGH confidence)
- Direct source analysis: `src/analysis/stats.rs` (187 lines, verified functions and signatures)
- Direct source analysis: `src/analysis/io.rs` (281 lines, verified DateRange and load_jsonl)
- Direct source analysis: `src/analysis/output.rs` (156 lines, verified OutputFormat and table helpers)
- Direct source analysis: `src/bin/spread_analytics.rs` (81 lines, verified CLI skeleton and flags)
- Direct source analysis: `src/spread/patterns.rs` (SpreadResult schema, SpreadPattern enum, venue_pair_label method)
- Phase 26-01 SUMMARY: confirmed stats and io module deliverables
- Phase 26-02 SUMMARY: confirmed output module and CLI binary deliverables
- REQUIREMENTS.md: SPREAD-01, SPREAD-02, SPREAD-03 requirement definitions
- ROADMAP.md: Phase 27 success criteria

### Secondary (MEDIUM confidence)
- FEATURES.md: TS-1, TS-2, TS-3 feature specifications with implementation details
- PITFALLS.md: Pitfall 10 (venue-pair mixing), Pitfall 12 (Decimal display), Pitfall 9 (output verbosity)
- ARCHITECTURE.md: Component boundaries and data flow patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already exist, verified against Cargo.toml and source
- Architecture: HIGH -- Phase 26 infrastructure directly inspected, all APIs verified
- Pitfalls: HIGH -- domain pitfalls documented from prior research, code patterns verified

**Research date:** 2026-02-28
**Valid until:** 2026-03-30 (stable -- no external API changes, computation logic is mathematical)
