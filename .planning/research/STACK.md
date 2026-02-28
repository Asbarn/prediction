# Technology Stack: v1.4 Analysis Tooling

**Project:** Prediction Market Arbitrage System -- CLI Analysis Tools
**Researched:** 2026-02-28
**Confidence:** HIGH

## Scope

This document covers ONLY the stack additions needed for v1.4 Analysis Tooling: two CLI binaries (spread analytics, signal scoring) that read existing JSONL log files and compute statistical summaries. The existing v1.0-v1.3 stack is validated and unchanged.

---

## Executive Finding: Two New Dependencies Required

v1.4 requires **two new crate dependencies**: `comfy-table` for terminal table rendering and `csv` for CSV export. All statistical computation is covered by the existing `statrs` 0.18 crate. CLI argument parsing is covered by the existing `clap` 4.5 with derive feature. Date/time handling is covered by the existing `chrono` 0.4 with serde feature. Decimal arithmetic is covered by the existing `rust_decimal` 1.40.

This is the minimal addition footprint. Both new crates are mature, widely-used, and have no transitive dependency conflicts with the existing tree.

---

## Recommended Stack

### New Dependencies

| Technology | Version | Purpose | Why This One |
|------------|---------|---------|--------------|
| comfy-table | 7.2 | Terminal table rendering for CLI output | Finished/stable project; 58M+ downloads; zero unsafe; automatic column width; no derive macro overhead unlike tabled. Minimal dependency tree (just unicode-width). |
| csv | 1.4 | CSV file export for analysis results | BurntSushi's canonical CSV crate; 129M+ downloads; serde integration out of the box; already the Rust ecosystem standard. |

### Existing Dependencies Covering v1.4 Needs

| Technology | Version | v1.4 Usage | Why Sufficient |
|------------|---------|------------|----------------|
| clap | 4.5 (derive) | CLI argument parsing for both binaries | Already in deps with derive feature. Subcommands, value parsing, help generation all available. |
| statrs | 0.18 | Normal distribution inverse CDF for confidence intervals; no additional statistical crates needed | Already used for Black-76 pricing. `Normal::standard().inverse_cdf(0.975)` gives z=1.96 for 95% CI. Has mean, variance, CDF, inverse CDF, PDF -- everything needed for Sharpe ratio CI and distribution analysis. |
| chrono | 0.4 (serde) | Parse timestamps from JSONL, hourly bucketing, date range filtering | Already in deps with serde feature. `NaiveDateTime::from_timestamp_millis()`, `DateTime::hour()`, date arithmetic all available. |
| rust_decimal | 1.40 (maths, serde-with-str) | Deserialize Decimal fields from JSONL spread logs; precise arithmetic for P&L calculations | Already in deps. All SpreadResult and SettlementRecord fields use `serde(with = "rust_decimal::serde::str")`. |
| serde + serde_json | 1.0 | Deserialize JSONL lines into SpreadResult, ArbSignal, SettlementRecord types | Already in deps. All log types derive Serialize + Deserialize. |
| anyhow | 1.0 | Error handling in CLI binaries | Already in deps. Standard for application-level error handling. |
| tracing | 0.1 | Optional verbose logging in CLI tools | Already in deps. Use `tracing::debug!` for diagnostic output when `--verbose` flag is set. |

---

## Why These Specific Choices

### comfy-table over tabled

| Criterion | comfy-table 7.2 | tabled 0.20 |
|-----------|-----------------|-------------|
| API style | Builder pattern: `table.add_row(vec![...])` | Derive macro on structs: `#[derive(Tabled)]` |
| Dependencies | 1 (unicode-width) | 4+ (papergrid, ansi-str, ansitok, tabled_derive) |
| Maturity | "Finished" -- author considers it feature-complete | Active development, API still evolving (0.x) |
| Downloads | 58M+ | Lower |
| Fit for CLI analysis | Dynamic rows from computed statistics -- builder pattern is natural | Derive macro suits struct-per-row patterns; our output rows are computed summaries, not struct instances |

**Decision:** comfy-table. The analysis CLIs output computed summary tables (hourly buckets, venue-pair breakdowns, Sharpe ratios), not direct struct serializations. comfy-table's builder API fits this use case better than tabled's derive macro. Fewer transitive dependencies.

### csv (BurntSushi) -- no alternatives considered

The `csv` crate is the canonical CSV implementation in Rust. 129M+ downloads, serde-aware `Writer` and `Reader`, streaming API. There is no credible alternative. Using `writeln!("{},{},{}", ...)` instead of a proper CSV crate would create quoting/escaping bugs on the first field containing a comma.

### statrs -- already sufficient, no additional stats crate needed

The project already depends on `statrs` 0.18 for Black-76 pricing (`Normal` CDF/PDF). For v1.4 analysis:

| Statistical Need | statrs Coverage |
|-----------------|-----------------|
| Sharpe ratio | Manual: `(mean_return - risk_free) / std_dev` -- basic arithmetic, no crate needed |
| Confidence interval for Sharpe | `Normal::standard().inverse_cdf(0.975)` gives z = 1.96 for 95% CI. SE(Sharpe) = sqrt((1 + 0.5 * sharpe^2) / n). Already available in statrs 0.18. |
| Confidence interval for hit rate | Binomial proportion CI: `p +/- z * sqrt(p*(1-p)/n)`. z from `Normal::standard().inverse_cdf()`. |
| Max drawdown | Running minimum of cumulative P&L -- pure arithmetic, no crate needed |
| Distribution percentiles | Sort + interpolate (already implemented in `RollingStats::percentile`) or use statrs distribution quantiles |
| Mean, std dev | Trivial arithmetic. Already implemented in `RollingStats`. |

**No need for `ndarray`, `nalgebra`, or `polars`.** The analysis operates on 1D time series of Decimal/f64 values. Vector math libraries add compile time and complexity without benefit.

---

## Binary Structure

The project currently has a single binary (`src/main.rs`). v1.4 adds two new binaries using Cargo's `[[bin]]` mechanism. The new binaries are separate executables that link against the existing library crate (`src/lib.rs`) to reuse types.

### Cargo.toml Additions

```toml
[[bin]]
name = "spread-analytics"
path = "src/bin/spread_analytics.rs"

[[bin]]
name = "signal-scoring"
path = "src/bin/signal_scoring.rs"
```

### Why Separate Binaries (Not Subcommands)

| Approach | Pros | Cons |
|----------|------|------|
| **Separate binaries (recommended)** | Independent compilation; no runtime overhead from main binary's async runtime, feed infrastructure, venue clients; clear separation of concerns; can run without config files that main binary requires | Two binary targets in Cargo.toml |
| Subcommands on existing binary | Single binary deployment | Pulls in tokio full, all venue clients, config loading, settlement monitoring -- none of which analysis tools need. Main binary's `Cli` struct would grow with unrelated options. |

**Decision:** Separate binaries. The analysis tools are offline, synchronous, file-reading utilities. They should not carry the weight of the real-time feed infrastructure. They link against `prediction` as a library crate only for type definitions (`SpreadResult`, `ArbSignal`, `SettlementRecord`, `SpreadPattern`, `Venue`, etc.).

### Type Reuse from Library Crate

The analysis binaries deserialize JSONL lines into existing types. No new type definitions needed for deserialization:

| JSONL Source | Type | Module Path |
|-------------|------|-------------|
| Spread logs (`spread_logs/{YYYY-MM-DD}.jsonl`) | `SpreadResult` | `prediction::spread::patterns::SpreadResult` |
| Signal logs (`signal_logs/{YYYY-MM-DD}.jsonl`) | `ArbSignal` | `prediction::signal::types::ArbSignal` |
| Settlement logs (`settlement_log_dir/{YYYY-MM-DD}.jsonl`) | `AnalysisSettlementRecord` | `prediction::paper_trade::analyzer::AnalysisSettlementRecord` |
| Trade logs (`paper_trade/log_dir/{YYYY-MM-DD}.jsonl`) | Various trade event types | `prediction::paper_trade::tracker` |

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| polars | DataFrame library -- massive compile time addition (~200+ transitive deps); overkill for 1D time series analysis of thousands of JSONL lines | Manual iteration with `serde_json::from_str` per line + accumulator pattern (already proven in `AccumulatorBucket`) |
| ndarray / nalgebra | Linear algebra libraries -- no matrix operations needed; all statistics are scalar or 1D vector operations | `Vec<f64>` + manual mean/stddev/percentile (pattern already in `RollingStats`) |
| plotters / textplotter | Chart generation -- adds significant deps; CLI output should be tabular, not graphical; user can pipe CSV to external tools (gnuplot, Excel, Python matplotlib) | comfy-table for terminal display + csv for data export |
| prettytable-rs | Unmaintained since 2019; comfy-table is the maintained successor | comfy-table 7.2 |
| tabled | Derive-macro approach doesn't fit dynamic summary tables; more transitive dependencies | comfy-table 7.2 |
| indicatif | Progress bars -- analysis of JSONL files completes in milliseconds to seconds; progress indication is unnecessary | Simple `eprintln!("Processing {} files...", count)` |
| tokio (in analysis binaries) | Async runtime -- analysis tools do synchronous file I/O only; no network, no concurrency needed | `std::fs::File` + `std::io::BufReader` |
| Any database crate (rusqlite, diesel) | Data is in JSONL files, not databases; adding a DB layer creates a migration/schema burden with no benefit at this scale | Direct JSONL file reading |
| colored / owo-colors / yansi | Terminal coloring -- adds dependency for purely cosmetic value; comfy-table handles table borders | Plain text output; comfy-table's built-in styling if desired |

---

## Installation

```toml
# Add to existing [dependencies] section in Cargo.toml:

# Terminal table output (v1.4 analysis CLIs)
comfy-table = "7"
# CSV export (v1.4 analysis CLIs)
csv = "1"
```

No feature flags needed for either crate. Both work with default features.

```bash
# No additional cargo install steps. The new binaries are built with:
cargo build --release --bin spread-analytics --bin signal-scoring
```

---

## Integration Points

### Data Flow: Spread Analytics CLI

```
spread_logs/{YYYY-MM-DD}.jsonl
        |
        v
  BufReader::lines()
        |
  serde_json::from_str::<SpreadResult>()
        |
  Group by: hour (chrono), venue_pair (SpreadPattern::venue_pair_label()),
            event_id, pattern
        |
  Compute per-bucket: mean, stddev, median, p5, p95, count
        |
  Output: comfy-table (terminal) or csv::Writer (--csv flag)
```

### Data Flow: Signal Scoring CLI

```
settlement logs + signal logs + trade logs
        |
        v
  BufReader::lines()
        |
  serde_json::from_str::<AnalysisSettlementRecord>()
  serde_json::from_str::<ArbSignal>()
        |
  Compute: hit_rate, sharpe_ratio, max_drawdown,
           confidence_intervals (statrs Normal inverse CDF),
           cost-adjusted edge, threshold effectiveness
        |
  Output: comfy-table (terminal) or csv::Writer (--csv flag)
```

### CLI Argument Structure (clap derive)

```rust
// spread-analytics binary
#[derive(Parser)]
#[command(name = "spread-analytics")]
struct Cli {
    /// Directory containing spread log JSONL files
    #[arg(long, default_value = "spread_logs")]
    log_dir: PathBuf,

    /// Start date (YYYY-MM-DD), defaults to earliest file
    #[arg(long)]
    from: Option<NaiveDate>,

    /// End date (YYYY-MM-DD), defaults to latest file
    #[arg(long)]
    to: Option<NaiveDate>,

    /// Filter by venue pair (e.g., "kalshi_polymarket")
    #[arg(long)]
    venue_pair: Option<String>,

    /// Filter by event ID
    #[arg(long)]
    event_id: Option<String>,

    /// Output as CSV instead of table
    #[arg(long)]
    csv: bool,
}
```

```rust
// signal-scoring binary
#[derive(Parser)]
#[command(name = "signal-scoring")]
struct Cli {
    /// Directory containing settlement log JSONL files
    #[arg(long, default_value = "state/settlements")]
    settlement_dir: PathBuf,

    /// Directory containing signal log JSONL files
    #[arg(long, default_value = "signal_logs")]
    signal_dir: PathBuf,

    /// Confidence level for intervals (default: 0.95)
    #[arg(long, default_value = "0.95")]
    confidence: f64,

    /// Risk-free rate for Sharpe calculation (annualized, default: 0.05)
    #[arg(long, default_value = "0.05")]
    risk_free_rate: f64,

    /// Filter by venue pair
    #[arg(long)]
    venue_pair: Option<String>,

    /// Output as CSV instead of table
    #[arg(long)]
    csv: bool,
}
```

---

## Statistical Formulas (Using Existing statrs)

### Sharpe Ratio
```rust
let sharpe = (mean_return - risk_free_per_period) / returns_stddev;
```

### Sharpe Ratio Confidence Interval
```rust
use statrs::distribution::ContinuousCDF;
let z = Normal::standard().inverse_cdf(1.0 - (1.0 - confidence) / 2.0);
// SE(Sharpe) approximation (Lo, 2002):
let se = ((1.0 + 0.5 * sharpe * sharpe) / n as f64).sqrt();
let ci_lower = sharpe - z * se;
let ci_upper = sharpe + z * se;
```

### Hit Rate Confidence Interval (Wilson Score)
```rust
let z = Normal::standard().inverse_cdf(1.0 - (1.0 - confidence) / 2.0);
let p = hits as f64 / n as f64;
let se = (p * (1.0 - p) / n as f64).sqrt();
let ci_lower = p - z * se;
let ci_upper = p + z * se;
```

### Max Drawdown
```rust
let mut peak = 0.0_f64;
let mut max_dd = 0.0_f64;
for &cumulative_pnl in &equity_curve {
    peak = peak.max(cumulative_pnl);
    let dd = peak - cumulative_pnl;
    max_dd = max_dd.max(dd);
}
```

All of these use only `statrs::distribution::Normal` (already imported in 5 files) and basic f64 arithmetic. Zero new statistical dependencies.

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Table output | comfy-table 7.2 | tabled 0.20 | tabled's derive macro doesn't fit dynamic summary tables; more deps; 0.x semver |
| Table output | comfy-table 7.2 | prettytable-rs | Unmaintained since 2019 |
| CSV output | csv 1.4 | Manual `write!("{},{}", ...)` | No proper escaping of commas/quotes in field values |
| Statistics | statrs 0.18 (existing) | statrs 0.18 + additional stats crate | statrs already covers Normal distribution CDF/inverse CDF; Sharpe and drawdown are manual arithmetic |
| Statistics | statrs 0.18 (existing) | polars | 200+ transitive deps for what amounts to mean/stddev/percentile on Vec<f64> |
| Binary structure | Separate `[[bin]]` targets | Subcommands on main binary | Analysis tools don't need async runtime, venue clients, or config infrastructure |
| Date handling | chrono 0.4 (existing) | time 0.3 | chrono already in deps; all JSONL timestamps use chrono types |
| Error handling | anyhow 1.0 (existing) | eyre | anyhow already in deps and used throughout |

---

## Version Compatibility Verification

| Crate | Version | Rust 2024 Edition | Status |
|-------|---------|-------------------|--------|
| comfy-table | 7.2 | Compatible (MSRV 1.63) | NEW |
| csv | 1.4 | Compatible (MSRV 1.63) | NEW |
| statrs | 0.18 | Compatible | Unchanged (already in deps) |
| clap | 4.5 | Compatible | Unchanged (already in deps) |
| chrono | 0.4 | Compatible | Unchanged (already in deps) |
| rust_decimal | 1.40 | Compatible | Unchanged (already in deps) |
| serde + serde_json | 1.0 | Compatible | Unchanged (already in deps) |
| anyhow | 1.0 | Compatible | Unchanged (already in deps) |

**Rust compiler:** 1.85+ (2024 edition) -- no issues with any dependency (new or existing).

---

## Dependency Growth Summary

| Milestone | New Crates Added | Rationale |
|-----------|-----------------|-----------|
| v1.0 | Baseline (19 direct deps) | Core system |
| v1.1 | 0 | All built on existing deps |
| v1.2 | 1 (strsim -- already transitively compiled) | Fuzzy matching |
| v1.3 | 0 | All built on existing deps |
| **v1.4** | **2 (comfy-table, csv)** | **Output formatting for CLI tools -- cannot be reasonably avoided** |

Both new crates serve output formatting purposes that would be buggy or ugly to implement manually. This is the minimum viable addition.

---

## Sources

- [comfy-table docs.rs](https://docs.rs/comfy-table/latest/comfy_table/) -- Version 7.2.2, API reference, builder pattern (HIGH confidence)
- [comfy-table GitHub](https://github.com/Nukesor/comfy-table) -- "Finished" project status, feature completeness (HIGH confidence)
- [csv crate docs.rs](https://docs.rs/csv/latest/csv/) -- Version 1.4.0, serde integration, Writer/Reader API (HIGH confidence)
- [csv crate GitHub](https://github.com/BurntSushi/rust-csv) -- BurntSushi canonical implementation (HIGH confidence)
- [statrs Normal distribution docs](https://docs.rs/statrs/latest/statrs/distribution/struct.Normal.html) -- inverse_cdf confirmed available in 0.18, ContinuousCDF trait (HIGH confidence)
- [tabled crate docs.rs](https://docs.rs/tabled/latest/tabled/) -- Version 0.20.0, derive macro approach, dependency tree comparison (HIGH confidence)
- Existing codebase analysis: `Cargo.toml`, `src/spread/patterns.rs` (SpreadResult type), `src/signal/types.rs` (ArbSignal type), `src/settlement/types.rs` (SettlementRecord type), `src/paper_trade/analyzer.rs` (AccumulatorBucket, AnalysisSettlementRecord), `src/spread/rolling_stats.rs` (mean/stddev/percentile pattern), `src/main.rs` (current binary structure) (HIGH confidence)

---
*Stack research for: v1.4 Analysis Tooling*
*Researched: 2026-02-28*
