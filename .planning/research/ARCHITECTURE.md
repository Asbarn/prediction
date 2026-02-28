# Architecture Research: CLI Analysis Tooling Integration

**Domain:** Spread analytics and signal scoring CLI tools for prediction market arbitrage system
**Researched:** 2026-02-28
**Confidence:** HIGH (based on direct analysis of 35,580 LOC codebase, existing serde types, JSONL schema inspection, Cargo.toml dependency audit)

## Executive Summary

The v1.4 milestone adds two CLI-based analysis tools -- a spread analytics CLI and a signal scoring CLI -- that read existing JSONL log data offline and produce statistical summaries. These tools are pure consumers of data already produced by the running service. They share no runtime state with the main binary; they only share serde types for deserialization and computation logic for statistical analysis.

This research resolves five architectural questions: binary structure, JSONL parsing strategy, data access patterns, shared computation, and output format flexibility. The core recommendation is: **add two new `[[bin]]` targets in Cargo.toml that import the `prediction` library crate for serde types, place new analysis computation in a shared `src/analysis/` module, and use streaming line-by-line JSONL parsing with date-range filtering to handle arbitrarily large log files.**

The existing codebase is perfectly structured for this. The `lib.rs` already exposes all modules publicly. The `SpreadResult` and `ArbSignal` types already derive `Serialize + Deserialize`. The `clap` crate (4.5, derive feature) is already a dependency. Zero new crate dependencies are required, continuing the v1.1/v1.2/v1.3 pattern.

## Current Architecture (Verified Against Source)

### Data Flow Producing JSONL Logs

```
MarketSnapshots (3 venues)
       |
       v
  SpreadEngine -----> SpreadLogger -----> spread_logs/{YYYY-MM-DD}.jsonl
       |                                  (SpreadResult serde)
       |
  forward to signal engine
       |
       v
  CrossAssetEngine -> SignalLogger -----> signal_logs/{YYYY-MM-DD}.jsonl
       |                                  (ArbSignal serde)
       |
  forward to paper trade tracker
       |
       v
  PaperTradeTracker -> TradeLogger -----> paper_trades/trades-{YYYY-MM-DD}.jsonl
       |
       +-----------> SettlementLogger --> settlement_logs/settlements-{YYYY-MM-DD}.jsonl
                                          (AnalysisSettlementRecord serde)
```

### Existing Log Locations and Schemas

| Log Directory | Filename Pattern | Serde Type | Size (observed) |
|---------------|-----------------|------------|-----------------|
| `spread_logs/` | `{YYYY-MM-DD}.jsonl` | `SpreadResult` | Not yet in production (default config) |
| `signal_logs/` | `{YYYY-MM-DD}.jsonl` | `ArbSignal` | ~30-160 KB/day (6 days of soak data) |
| `paper_trades/` | `trades-{YYYY-MM-DD}.jsonl` | `TradeEvent` (internal) | Not yet produced |
| `settlement_logs/` | `settlements-{YYYY-MM-DD}.jsonl` | `AnalysisSettlementRecord` | Not yet produced |

### Key Serde Types Already Available

All types in `prediction::spread::patterns` and `prediction::signal::types` derive both `Serialize` and `Deserialize`, meaning CLI tools can deserialize directly into the exact same structs. Verified types:

- `SpreadResult` -- 18 fields, all with `serde(with = "rust_decimal::serde::str")` for decimal fields
- `ThresholdComponents` -- 7 fields for threshold breakdown analysis
- `ArbSignal` -- 24 fields including nested `LegInfo`, `CostBreakdown`, `ConfidenceComponents`
- `ThresholdStatus` -- enum with `PassedBoth`, `PassedStaticOnly`, `Filtered` variants
- `SpreadPattern` -- enum with 4 directional patterns
- `ArbDirection` -- enum with `BuyPredictionSellOptions`, `SellPredictionBuyOptions`

### Existing Statistical Building Blocks

In `paper_trade::analyzer`:
- `AccumulatorBucket` -- running counters for hit rate, edge, convergence, false positive rate
- `LifetimeSummary` -- aggregate across all accumulator keys
- `FilteredSignalTracker` -- threshold effectiveness analysis

In `paper_trade::aggregator`:
- `DailyRollup` -- per-day P&L statistics (trade count, win rate, max win/loss)

These are runtime accumulators (receive data via channels). The CLI tools need batch equivalents that operate on JSONL files, but the statistical logic can be extracted and reused.

## Architectural Decisions

### Decision 1: Binary Structure -- Separate `[[bin]]` Targets

**Decision:** Add `src/bin/spread_analytics.rs` and `src/bin/signal_scoring.rs` as separate `[[bin]]` targets in Cargo.toml, not subcommands of the main binary.

**Rationale:**

1. **Separation of concerns.** The main binary is a long-running service with tokio runtime, WebSocket connections, and channel plumbing. CLI tools are short-lived batch processors. Mixing them as subcommands would pull the entire service dependency graph into the CLI path.

2. **Build time isolation.** A `prediction-spread-analytics` binary compiles only what it imports from the library crate. The main binary's `main.rs` (40,839 lines of orchestration) stays untouched.

3. **Deployment simplicity.** `cargo build --release` produces three binaries. The analysis tools can be copied to any machine with the JSONL files -- no config.toml, no API credentials, no WebSocket setup required.

4. **Existing pattern.** The codebase already uses `clap` (4.5, derive feature) with `#[derive(Parser)]` and `#[command(subcommand)]` in `main.rs`. The same pattern applies to each CLI binary with its own `Cli` struct.

**Why not subcommands of main:** The main binary requires `AppConfig` loading, logging init, Prometheus setup, and credential loading. A `prediction check-config` subcommand exists but it still loads config. Analysis CLIs should work with zero configuration -- just point at JSONL files.

**Cargo.toml additions:**

```toml
[[bin]]
name = "prediction"
path = "src/main.rs"

[[bin]]
name = "spread-analytics"
path = "src/bin/spread_analytics.rs"

[[bin]]
name = "signal-scoring"
path = "src/bin/signal_scoring.rs"
```

### Decision 2: JSONL Parsing -- Reuse Existing Serde Types Directly

**Decision:** Deserialize JSONL lines directly into `prediction::spread::patterns::SpreadResult` and `prediction::signal::types::ArbSignal`. No CLI-specific read types.

**Rationale:**

1. **Types already roundtrip.** Both types have tests proving `serde_json::to_string` then `serde_json::from_str` roundtrips. The JSONL files are written by `serde_json::to_string`, so deserialization into the same types is guaranteed correct.

2. **Schema stability.** The JSONL schema has been stable since v1.0 (spread) and v1.0 (signals). Both types use `#[serde(default)]` on newer fields (`basis_risk_premium`, `threshold_status`) ensuring forward compatibility with older log files.

3. **Zero duplication.** Defining separate "read" types would duplicate 40+ field definitions and diverge over time. The existing types are the canonical schema definition.

4. **Field-level access.** The CLI needs to slice by `event_id`, `pattern`/`direction`, `timestamp_ms`/`timestamp`, `threshold_status`, and compute on `net_spread`/`net_edge`, `gross_spread`/`raw_spread`, `confidence`, and `cost_breakdown` fields. All are directly accessible on the existing types.

**One concern addressed: `rust_decimal::serde::str`.** Both read and write use the same serde annotation. The JSONL stores decimals as JSON strings (e.g., `"0.05"`). The `serde(with = "rust_decimal::serde::str")` attribute handles deserialization from strings correctly. Verified in existing tests.

### Decision 3: Data Access Pattern -- Streaming Line-by-Line with Date-Range Filtering

**Decision:** Use `BufReader::lines()` with line-by-line `serde_json::from_str` and filter by date range at the file level. Do not load all data into memory.

**Rationale:**

1. **File sizes are manageable but growing.** Current signal logs are 30-160 KB/day. A month-long soak test would produce ~5 MB. But spread logs (every computation, not just signals) will be significantly larger -- the Deribit recording alone hit 39 MB in one day. Streaming handles any size.

2. **Date-range filtering is free.** JSONL files are named `{YYYY-MM-DD}.jsonl`. To analyze a date range, just iterate matching filenames. No need to parse every line to check timestamps.

3. **Two-pass pattern for statistics requiring full data.** Some metrics (e.g., percentiles, standard deviation) require all values. For these, a first pass collects values into a `Vec<Decimal>`, then a second pass computes statistics. This is bounded by the date range, not total history.

4. **No async needed.** File I/O in a CLI tool does not benefit from async. Use `std::io::BufReader` with `std::io::BufRead::lines()`. No tokio runtime required in CLI binaries. This simplifies the binary significantly -- no `#[tokio::main]`, just plain `fn main()`.

**Pattern:**

```rust
fn load_spread_results(dir: &Path, from: NaiveDate, to: NaiveDate) -> Vec<SpreadResult> {
    let mut results = Vec::new();
    let mut date = from;
    while date <= to {
        let path = dir.join(format!("{}.jsonl", date.format("%Y-%m-%d")));
        if path.exists() {
            let file = File::open(&path).expect("open JSONL file");
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line.expect("read line");
                match serde_json::from_str::<SpreadResult>(&line) {
                    Ok(result) => results.push(result),
                    Err(e) => eprintln!("WARN: skip malformed line: {e}"),
                }
            }
        }
        date += chrono::Duration::days(1);
    }
    results
}
```

**Error handling:** Malformed lines are warned and skipped. A corrupted line (e.g., incomplete write from crash) should not abort analysis of the entire dataset.

### Decision 4: Shared Computation -- New `src/analysis/` Module in Library Crate

**Decision:** Create `src/analysis/` module in the library crate with pure computation functions. Both CLI binaries import from `prediction::analysis`. Do not inline computation in each binary.

**Rationale:**

1. **Reuse between CLIs.** Both CLIs need time bucketing, statistical aggregation, confidence interval calculation, and output formatting. These are shared concerns.

2. **Testability.** Pure functions that take `&[SpreadResult]` or `&[ArbSignal]` and return computed metrics are trivially unit-testable without file I/O.

3. **Future reuse.** When (if) a dashboard or API endpoint wants the same analysis, the computation is already factored out.

4. **Existing precedent.** The `paper_trade::analyzer` module already contains accumulator logic. The new `analysis` module can reference (or extract from) that logic, but tuned for batch operation rather than streaming accumulation.

**Module structure:**

```
src/analysis/
    mod.rs          -- pub mod spread_analytics; pub mod signal_scoring; pub mod stats;
    spread_analytics.rs  -- time bucketing, venue-pair slicing, spread distribution
    signal_scoring.rs    -- hit rate, Sharpe, drawdown, confidence intervals
    stats.rs             -- shared statistical functions (mean, stddev, percentile, CI)
```

**Key distinction from existing `paper_trade::analyzer`:**

| Aspect | `paper_trade::analyzer` | `analysis::*` |
|--------|------------------------|---------------|
| Input | Live positions via channel | Batch from JSONL |
| State | Running accumulators (HashMap) | Computed from full dataset |
| Output | Prometheus gauges + JSONL | Formatted terminal/JSON/CSV |
| Runtime | Requires tokio, channels | Pure `fn`, no runtime |

### Decision 5: Output Format Flexibility -- `--format` Flag with Table Default

**Decision:** Support `--format table` (default), `--format json`, and `--format csv` via a clap enum argument. Use no additional crate dependencies.

**Rationale:**

1. **Table for human consumption.** The primary use case is a solo trader reviewing soak test data in a terminal. Aligned columns with headers are the most readable format.

2. **JSON for programmatic use.** Enables piping output to `jq` for ad-hoc queries or feeding into scripts. Use `serde_json::to_string_pretty` -- already a dependency.

3. **CSV for spreadsheet analysis.** For deeper statistical work, CSV import into a spreadsheet is the path of least resistance. Implement with manual comma-separated formatting -- no `csv` crate needed for simple tabular output.

4. **Zero new dependencies.** Table formatting can be done with Rust's `format!` width specifiers (`{:<15}`, `{:>10}`). This continues the zero-new-deps pattern from v1.1/v1.2/v1.3.

## Component Boundaries

### New Components

| Component | Location | Responsibility | Depends On |
|-----------|----------|---------------|------------|
| `spread-analytics` binary | `src/bin/spread_analytics.rs` | CLI entry point: arg parsing, file loading, output routing | `prediction::analysis::spread_analytics`, `prediction::spread::patterns` |
| `signal-scoring` binary | `src/bin/signal_scoring.rs` | CLI entry point: arg parsing, file loading, output routing | `prediction::analysis::signal_scoring`, `prediction::signal::types` |
| `analysis::spread_analytics` | `src/analysis/spread_analytics.rs` | Time bucketing, venue-pair slicing, spread distribution stats | `prediction::spread::patterns::SpreadResult`, `analysis::stats` |
| `analysis::signal_scoring` | `src/analysis/signal_scoring.rs` | Hit rate, Sharpe ratio, drawdown, edge CI, threshold effectiveness | `prediction::signal::types::ArbSignal`, `analysis::stats` |
| `analysis::stats` | `src/analysis/stats.rs` | Mean, stddev, percentile, confidence intervals, Sharpe formula | `rust_decimal`, `statrs` (already dep) |

### Modified Components

| Component | Change | Risk |
|-----------|--------|------|
| `Cargo.toml` | Add two `[[bin]]` entries | None -- additive only |
| `src/lib.rs` | Add `pub mod analysis;` | None -- additive only |

### Unchanged Components

Everything else. The main binary, all feed modules, spread engine, signal engine, paper trade tracker, settlement system, config system -- **zero modifications** to any existing component.

## Data Flow

### Spread Analytics CLI

```
User invokes:
  spread-analytics --dir spread_logs/ --from 2026-02-20 --to 2026-02-28 --bucket hourly

  1. Parse CLI args (clap)
  2. Enumerate JSONL files in date range
  3. For each file:
     a. BufReader::lines()
     b. serde_json::from_str::<SpreadResult>(line)
     c. Skip malformed lines with warning
  4. Group by time bucket (hourly/daily)
  5. Within each bucket, compute:
     - Count of spread computations
     - Mean/median/p95 gross spread
     - Mean/median/p95 net spread
     - Spread distribution (positive/negative/zero)
     - Venue pair breakdown
     - Threshold pass rates (PassedBoth/PassedStaticOnly/Filtered)
  6. Format output (table/json/csv)
  7. Print to stdout
```

### Signal Scoring CLI

```
User invokes:
  signal-scoring --dir signal_logs/ --from 2026-02-20 --to 2026-02-28

  1. Parse CLI args (clap)
  2. Enumerate JSONL files in date range
  3. For each file:
     a. BufReader::lines()
     b. serde_json::from_str::<ArbSignal>(line)
     c. Skip malformed lines with warning
  4. Compute overall metrics:
     - Total signals by threshold status
     - Hit rate (signals with positive net_edge vs total)
     - Cost-adjusted edge distribution
     - Sharpe ratio (mean_edge / stddev_edge, annualized)
     - Max drawdown (sequential cumulative P&L)
     - Confidence intervals (bootstrap or normal approx)
  5. Compute breakdowns:
     - By event_id
     - By direction (BuyPrediction vs SellPrediction)
     - By prediction_venue (Polymarket vs Kalshi)
     - By time of day (hourly buckets)
     - Threshold effectiveness (PassedBoth hit rate vs PassedStaticOnly)
  6. Format output (table/json/csv)
  7. Print to stdout
```

### Optional: Combined Settlement Correlation

If settlement logs exist, the signal scoring CLI can optionally cross-reference:

```
  signal-scoring --dir signal_logs/ --settlements settlement_logs/ --from 2026-02-20

  Additional step: Load settlement outcomes, match by event_id,
  compute actual hit rate (signal direction matched settlement outcome)
  vs estimated hit rate (positive net_edge at signal time).
```

## CLI Interface Design

### Spread Analytics

```
spread-analytics [OPTIONS]

Options:
  --dir <PATH>           Spread logs directory [default: spread_logs]
  --from <YYYY-MM-DD>    Start date (inclusive)
  --to <YYYY-MM-DD>      End date (inclusive) [default: today]
  --bucket <BUCKET>      Time bucket size: hourly, daily [default: hourly]
  --event <EVENT_ID>     Filter to specific event ID
  --pattern <PATTERN>    Filter to spread pattern
  --format <FORMAT>      Output format: table, json, csv [default: table]
  --venue-pair           Group results by venue pair
  --threshold-breakdown  Show threshold status distribution per bucket
  -v, --verbose          Show per-line parse warnings
```

### Signal Scoring

```
signal-scoring [OPTIONS]

Options:
  --dir <PATH>              Signal logs directory [default: signal_logs]
  --from <YYYY-MM-DD>       Start date (inclusive)
  --to <YYYY-MM-DD>         End date (inclusive) [default: today]
  --settlements <PATH>      Settlement logs directory (optional)
  --event <EVENT_ID>        Filter to specific event ID
  --direction <DIR>         Filter to direction
  --venue <VENUE>           Filter to prediction venue
  --format <FORMAT>         Output format: table, json, csv [default: table]
  --confidence-level <F>    CI confidence level [default: 0.95]
  --sharpe-periods <N>      Annualization factor for Sharpe [default: 252]
  -v, --verbose             Show per-line parse warnings
```

## Patterns to Follow

### Pattern 1: Date-Range File Enumeration

**What:** Iterate JSONL files matching a date range by constructing filenames directly from dates.
**When:** Any analysis operation that reads JSONL logs.
**Why:** Avoids filesystem scanning. Filename format `{YYYY-MM-DD}.jsonl` is stable convention across all loggers.

```rust
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

fn files_in_range(dir: &Path, from: NaiveDate, to: NaiveDate) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut date = from;
    while date <= to {
        let path = dir.join(format!("{}.jsonl", date.format("%Y-%m-%d")));
        if path.exists() {
            files.push(path);
        }
        date += chrono::Duration::days(1);
    }
    files
}
```

### Pattern 2: Tolerant JSONL Deserialization

**What:** Warn on malformed lines but continue processing. Count errors.
**When:** Reading any JSONL file that may have been written during a crash or schema evolution.
**Why:** A single corrupted line should not invalidate an entire day of data.

```rust
fn parse_jsonl<T: serde::de::DeserializeOwned>(
    path: &Path,
    verbose: bool,
) -> (Vec<T>, usize) {
    let file = File::open(path).expect("open file");
    let reader = BufReader::new(file);
    let mut results = Vec::new();
    let mut errors = 0;
    for (i, line) in reader.lines().enumerate() {
        match line {
            Ok(text) => match serde_json::from_str::<T>(&text) {
                Ok(val) => results.push(val),
                Err(e) => {
                    errors += 1;
                    if verbose {
                        eprintln!("WARN: {}:{}: {}", path.display(), i + 1, e);
                    }
                }
            },
            Err(e) => {
                errors += 1;
                if verbose {
                    eprintln!("WARN: {}:{}: IO error: {}", path.display(), i + 1, e);
                }
            }
        }
    }
    (results, errors)
}
```

### Pattern 3: Bucket Aggregation with Generic Key

**What:** Group records into time-keyed buckets, then compute per-bucket statistics.
**When:** Spread analytics hourly/daily rollups, signal scoring time-of-day analysis.
**Why:** Reusable across both CLIs with different record types.

```rust
use std::collections::BTreeMap;

fn bucket_by<T, K: Ord>(
    records: &[T],
    key_fn: impl Fn(&T) -> K,
) -> BTreeMap<K, Vec<&T>> {
    let mut buckets = BTreeMap::new();
    for record in records {
        buckets.entry(key_fn(record)).or_insert_with(Vec::new).push(record);
    }
    buckets
}
```

### Pattern 4: Statistics Module with Decimal Precision

**What:** Statistical functions that work on `Decimal` values (not `f64`) for precision, converting to `f64` only for output display.
**When:** Computing mean, stddev, percentiles on spread/edge values.
**Why:** The existing codebase uses `rust_decimal` everywhere. Converting to `f64` for intermediate calculations loses the precision guarantee that was the entire reason for choosing `rust_decimal`.

```rust
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

pub fn mean(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() { return None; }
    let sum: Decimal = values.iter().copied().sum();
    Some(sum / Decimal::from(values.len()))
}

pub fn stddev(values: &[Decimal]) -> Option<f64> {
    let m = mean(values)?.to_f64()?;
    let variance = values.iter()
        .map(|v| { let d = v.to_f64().unwrap_or(0.0) - m; d * d })
        .sum::<f64>() / values.len() as f64;
    Some(variance.sqrt())
}
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Async Runtime in CLI Tools

**What:** Using `#[tokio::main]` or any async runtime in the CLI binaries.
**Why bad:** The CLI tools perform no async I/O. File reads are synchronous. Adding tokio adds ~200KB to binary size and compilation time for zero benefit.
**Instead:** Use plain `fn main() -> Result<(), Box<dyn std::error::Error>>`. Use `std::io::BufReader` for file reading.

### Anti-Pattern 2: Loading AppConfig for CLI Tools

**What:** Requiring `config.toml`, `events.toml`, `venues.toml` to run analysis.
**Why bad:** The CLI tools should work on any machine with JSONL files. Config contains API credentials and venue-specific settings irrelevant to offline analysis.
**Instead:** Accept all parameters via CLI flags. Use sensible defaults.

### Anti-Pattern 3: Defining CLI-Specific Deserialization Types

**What:** Creating separate `CliSpreadResult` or `CliArbSignal` types with a subset of fields.
**Why bad:** Duplicates field definitions, diverges from source of truth, breaks when new fields are added.
**Instead:** Reuse `prediction::spread::patterns::SpreadResult` and `prediction::signal::types::ArbSignal` directly. The `#[serde(default)]` annotations already handle forward compatibility.

### Anti-Pattern 4: Loading Entire History Without Date Bounds

**What:** Not requiring `--from` or defaulting to "all files".
**Why bad:** As soak test history grows, loading all data becomes slow. Users will always want a specific window.
**Instead:** Require `--from`, default `--to` to today. Enumerate only files in the date range.

### Anti-Pattern 5: Putting Analysis Logic Inside the Binaries

**What:** Writing all computation directly in `src/bin/spread_analytics.rs`.
**Why bad:** Untestable without running the binary. Cannot be reused by the other CLI or future endpoints.
**Instead:** Thin binary that parses args and calls `prediction::analysis::*` functions.

## Build Order (Dependency-Respecting)

### Phase 1: Foundation -- `analysis::stats` Module

**What:** Shared statistical functions: mean, stddev, percentile, confidence intervals, Sharpe ratio formula.
**Why first:** Both CLIs depend on this. It has zero dependencies on other new code.
**Dependencies:** `rust_decimal`, `statrs` (both already in Cargo.toml).
**Deliverable:** `src/analysis/mod.rs`, `src/analysis/stats.rs` with comprehensive unit tests.

### Phase 2: Spread Analytics -- Computation Module

**What:** `analysis::spread_analytics` -- time bucketing, venue-pair grouping, spread distribution, threshold breakdown.
**Dependencies:** `analysis::stats`, `prediction::spread::patterns::SpreadResult`.
**Deliverable:** `src/analysis/spread_analytics.rs` with unit tests using constructed `SpreadResult` values.

### Phase 3: Signal Scoring -- Computation Module

**What:** `analysis::signal_scoring` -- hit rate, Sharpe ratio, max drawdown, cost-adjusted edge, confidence intervals, threshold effectiveness, optional settlement correlation.
**Dependencies:** `analysis::stats`, `prediction::signal::types::ArbSignal`.
**Deliverable:** `src/analysis/signal_scoring.rs` with unit tests using constructed `ArbSignal` values.

### Phase 4: JSONL Loading and Output Formatting

**What:** Shared JSONL file loading (date-range enumeration, tolerant parsing) and output formatter (table/json/csv).
**Dependencies:** `chrono`, `serde_json` (both already deps). Types from phases 2 and 3.
**Deliverable:** Added to `src/analysis/mod.rs` or `src/analysis/io.rs`.

### Phase 5: Spread Analytics Binary

**What:** `src/bin/spread_analytics.rs` -- clap arg parsing, calls loading and computation, prints output.
**Dependencies:** All of phases 1-4.
**Deliverable:** Working `spread-analytics` binary. Integration tested against sample JSONL files.

### Phase 6: Signal Scoring Binary

**What:** `src/bin/signal_scoring.rs` -- clap arg parsing, calls loading and computation, prints output.
**Dependencies:** All of phases 1-4.
**Deliverable:** Working `signal-scoring` binary. Integration tested against real `signal_logs/` data.

### Phase 7: Integration Verification and Documentation

**What:** End-to-end verification with real soak test data. Verify both CLIs produce correct output against hand-calculated expected values from known JSONL files.
**Dependencies:** All phases.
**Deliverable:** Verified output, any edge case fixes.

## Scalability Considerations

| Concern | Current (6 days) | At 30 days | At 180 days |
|---------|------------------|------------|-------------|
| Signal logs | ~550 KB total | ~3 MB | ~18 MB |
| Spread logs (est.) | Not yet in production | ~100-500 MB | ~1-3 GB |
| CLI memory usage | Trivial | < 50 MB | < 200 MB (date-bounded) |
| CLI execution time | < 1 second | < 5 seconds | < 30 seconds (date-bounded) |
| Approach | Load all in date range | Load all in date range | Load all in date range |

The streaming pattern with date-range bounding keeps memory and time proportional to the analysis window, not total history. Even at 180 days, if the user queries a 7-day window, they load ~7 days of data.

If spread logs grow very large (e.g., > 1 GB/day with high-frequency computation), a future optimization would be pre-aggregated daily summaries. But that is unnecessary for v1.4 and can be added non-disruptively later.

## Integration Points Summary

| Integration Point | Type | Direction | Details |
|-------------------|------|-----------|---------|
| `SpreadResult` serde type | Type reuse | CLI reads library types | `prediction::spread::patterns::SpreadResult` |
| `ArbSignal` serde type | Type reuse | CLI reads library types | `prediction::signal::types::ArbSignal` |
| `ThresholdStatus` enum | Type reuse | CLI reads library types | `prediction::signal::types::ThresholdStatus` |
| `SpreadPattern` enum | Type reuse | CLI reads library types | `prediction::spread::patterns::SpreadPattern` |
| JSONL file naming convention | Data contract | Service writes, CLI reads | `{YYYY-MM-DD}.jsonl` in configured directories |
| `rust_decimal` serde | Serialization compat | Shared | `serde(with = "rust_decimal::serde::str")` |
| `statrs` crate | Statistical functions | CLI computation | Normal distribution for confidence intervals |
| `clap` crate | CLI parsing | CLI entry points | Already v4.5 with derive feature |

## Sources

- Direct source analysis of `src/spread/patterns.rs` (SpreadResult schema, 583 lines)
- Direct source analysis of `src/signal/types.rs` (ArbSignal schema, 318 lines)
- Direct source analysis of `src/spread/logger.rs` (JSONL write pattern, 213 lines)
- Direct source analysis of `src/signal/logger.rs` (JSONL write pattern, 258 lines)
- Direct source analysis of `src/paper_trade/tracker.rs` (TradeLogger + SettlementLogger, 816+ lines)
- Direct source analysis of `src/paper_trade/analyzer.rs` (AccumulatorBucket, LifetimeSummary, FilteredSignalTracker)
- Direct source analysis of `src/main.rs` (CLI structure with clap Parser + Subcommand, 40,839 lines)
- Direct source analysis of `Cargo.toml` (dependency inventory)
- Direct inspection of `signal_logs/*.jsonl` (6 days of production soak data)
- Direct source analysis of `src/config/system.rs` (AnalysisConfig, PaperTradeConfig, all log dirs)
- Direct source analysis of `src/settlement/config.rs` (settlement_log_dir)
- Direct source analysis of `src/spread/config.rs` (spread log_dir default: "spread_logs")
- Direct source analysis of `src/signal/config.rs` (signal log_dir default: "signal_logs")
