# Phase 26: Analysis Infrastructure - Research

**Researched:** 2026-02-28
**Domain:** CLI binary scaffolding, shared statistics module, JSONL data loading, and formatted output rendering for analysis tooling
**Confidence:** HIGH

## Summary

Phase 26 establishes the shared foundation that both `spread-analytics` (Phase 27) and `signal-scoring` (Phase 28) CLIs depend on. The scope is precisely defined by four requirements: two CLI binaries with date-range filtering (INFRA-01), terminal table output (INFRA-02), JSON output mode (INFRA-03), and per-event breakdown flag (INFRA-04). The phase does NOT implement any analysis computations -- those belong to Phases 27 and 28. Instead, it delivers the infrastructure: `[[bin]]` targets, `src/analysis/` module skeleton, shared statistical functions, JSONL file loading with date-range filtering, and output rendering (table + JSON).

The codebase is ideally positioned for this work. All serde types (`SpreadResult`, `ArbSignal`) already derive `Serialize + Deserialize`. The JSONL filename convention (`{YYYY-MM-DD}.jsonl`) makes date-range filtering trivial at the file level. `clap` 4.5 with derive feature is already a dependency. The only new crate dependency needed is `comfy-table 7` for terminal table rendering. The `csv` crate mentioned in earlier research can be deferred -- Phase 26 only requires table and JSON output per the success criteria.

One technical concern was verified: `DualTimestamp::deserialize` calls `tokio::time::Instant::now()`, which wraps `std::time::Instant::now()` and does NOT require a tokio runtime to be running. The CLI binaries will link tokio transitively through the library crate but do not need `#[tokio::main]`. Plain `fn main()` works.

**Primary recommendation:** Deliver thin CLI binaries in `src/bin/` that parse args and delegate to `src/analysis/` library code. Build stats module, data loading layer, and output formatter as the shared foundation. Defer all domain-specific computation to Phases 27-28.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INFRA-01 | User can run `spread-analytics` and `signal-scoring` as separate CLI binaries with `--from YYYY-MM-DD`, `--to YYYY-MM-DD`, and `--last N` date-range filtering | Two `[[bin]]` targets in Cargo.toml; `clap` 4.5 derive for arg parsing; `files_in_range()` function enumerates JSONL files by date; `--last N` computes `from = today - N days` |
| INFRA-02 | User sees analysis output as formatted terminal tables with aligned numeric columns and section headers | `comfy-table 7` crate; `CellAlignment::Right` for numeric columns; section headers via full-width separator rows |
| INFRA-03 | User can pass `--output json` to get machine-readable JSON output instead of terminal tables | `serde_json::to_string_pretty` on `#[derive(Serialize)]` output structs; same data model backs both table and JSON rendering |
| INFRA-04 | User can pass `--by-event` to see all analyses broken down by event_id in addition to aggregate view | Boolean flag in CLI args; data loading groups by `event_id` field (present on both `SpreadResult` and `ArbSignal`); output formatter renders per-event sections |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| comfy-table | 7 | Terminal table rendering with column alignment and section headers | "Finished" project; 58M+ downloads; builder API fits dynamic summary tables; 1 transitive dep (unicode-width); `CellAlignment::Right` for numeric columns |
| clap | 4.5 (existing) | CLI argument parsing with derive macros | Already in Cargo.toml with `derive` feature; `#[derive(Parser)]` pattern used in `src/main.rs` |
| serde + serde_json | 1.0 (existing) | JSONL deserialization and JSON output | All log types already derive `Serialize + Deserialize`; roundtrip tested |
| chrono | 0.4 (existing) | Date parsing for `--from`/`--to` args, timestamp extraction for bucketing | Already in deps with `serde` feature; `NaiveDate` for CLI args, `DateTime::from_timestamp_millis` for record timestamps |
| rust_decimal | 1.40 (existing) | Decimal arithmetic for all financial values | All `SpreadResult` and `ArbSignal` fields use `serde(with = "rust_decimal::serde::str")`; precision preserved through analysis |
| statrs | 0.18 (existing) | Normal distribution inverse CDF for confidence intervals (Wilson score, Sharpe CI) | Already used for Black-76 pricing; `Normal::standard().inverse_cdf()` provides z-scores |
| anyhow | 1.0 (existing) | Error handling in CLI binaries | Already in deps; standard for application-level error handling |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 (existing) | Verbose diagnostic output in CLI tools | When `--verbose` flag is set; `tracing::warn!` for malformed JSONL lines |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| comfy-table | tabled 0.20 | tabled's derive macro doesn't fit dynamic summary tables; 4+ transitive deps; 0.x semver |
| comfy-table | Manual `format!` with width specifiers | Alignment breaks with variable-width numbers; maintenance burden |
| comfy-table | prettytable-rs | Unmaintained since 2019 |
| Separate `[[bin]]` targets | Subcommands on main binary | Would pull tokio full runtime, venue clients, config loading -- none needed for offline analysis |

**Installation:**
```toml
# Add to existing [dependencies] section in Cargo.toml:
comfy-table = "7"

# Add new [[bin]] entries:
[[bin]]
name = "spread-analytics"
path = "src/bin/spread_analytics.rs"

[[bin]]
name = "signal-scoring"
path = "src/bin/signal_scoring.rs"
```

## Architecture Patterns

### Recommended Project Structure

```
src/
  bin/
    spread_analytics.rs   # CLI entry point: arg parsing, delegates to analysis::
    signal_scoring.rs     # CLI entry point: arg parsing, delegates to analysis::
  analysis/
    mod.rs                # pub mod stats; pub mod io; pub mod output;
    stats.rs              # Pure statistical functions (mean, stddev, percentile, Wilson CI, Sharpe, PSR, max drawdown)
    io.rs                 # JSONL loading: files_in_range(), parse_jsonl<T>(), DateRange
    output.rs             # OutputFormat enum, table rendering (comfy-table), JSON rendering (serde_json)
  lib.rs                  # Add: pub mod analysis;
```

### Pattern 1: Separate `[[bin]]` Targets (Not Subcommands)

**What:** Each CLI tool is a separate binary that links the `prediction` library crate for type definitions only.

**When to use:** Always for this project -- analysis tools are offline, synchronous batch processors.

**Why:** The main binary requires tokio full runtime, WebSocket infrastructure, config files, and API credentials. Analysis CLIs need none of this. They should work on any machine with JSONL files -- no config.toml, no API keys.

**Example:**
```rust
// src/bin/spread_analytics.rs
use clap::Parser;
use prediction::analysis::io::DateRange;
use prediction::analysis::output::{OutputFormat, render_placeholder};

#[derive(Parser)]
#[command(name = "spread-analytics")]
#[command(about = "Analyze spread distribution patterns from recorded JSONL data")]
struct Cli {
    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    from: Option<chrono::NaiveDate>,

    /// End date (YYYY-MM-DD), defaults to today
    #[arg(long)]
    to: Option<chrono::NaiveDate>,

    /// Analyze last N days (alternative to --from/--to)
    #[arg(long)]
    last: Option<u32>,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Break down results by event_id
    #[arg(long)]
    by_event: bool,

    /// Spread logs directory
    #[arg(long, default_value = "spread_logs")]
    log_dir: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;
    // Phase 27 will add: load data, compute, render
    render_placeholder("spread-analytics", &range, &cli.output);
    Ok(())
}
```

### Pattern 2: Date-Range File Enumeration

**What:** Construct JSONL filenames from dates rather than scanning the filesystem.

**When to use:** All data loading operations.

**Why:** JSONL files are named `{YYYY-MM-DD}.jsonl`. Enumerating by date is O(days) regardless of directory contents. No filesystem scanning needed.

```rust
// src/analysis/io.rs
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl DateRange {
    pub fn from_args(
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        last: Option<u32>,
    ) -> anyhow::Result<Self> {
        let today = chrono::Utc::now().date_naive();
        match (from, to, last) {
            (Some(f), Some(t), None) => Ok(Self { from: f, to: t }),
            (Some(f), None, None) => Ok(Self { from: f, to: today }),
            (None, None, Some(n)) => Ok(Self {
                from: today - chrono::Duration::days(n as i64),
                to: today,
            }),
            (None, None, None) => anyhow::bail!("Specify --from/--to or --last N"),
            _ => anyhow::bail!("Use --from/--to OR --last, not both"),
        }
    }

    pub fn files_in_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut date = self.from;
        while date <= self.to {
            let path = dir.join(format!("{}.jsonl", date.format("%Y-%m-%d")));
            if path.exists() {
                files.push(path);
            }
            date += chrono::Duration::days(1);
        }
        files
    }
}
```

### Pattern 3: Tolerant JSONL Deserialization

**What:** Warn on malformed lines but continue processing. Count errors. Never abort analysis.

**When to use:** Every JSONL file read.

**Why:** A single corrupted line (e.g., incomplete write from crash) should not invalidate an entire day of data.

```rust
// src/analysis/io.rs
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct LoadResult<T> {
    pub records: Vec<T>,
    pub errors: usize,
    pub files_loaded: usize,
    pub files_missing: usize,
}

pub fn load_jsonl<T: DeserializeOwned>(
    files: &[std::path::PathBuf],
) -> LoadResult<T> {
    let mut result = LoadResult {
        records: Vec::new(),
        errors: 0,
        files_loaded: 0,
        files_missing: 0,
    };
    for path in files {
        if !path.exists() {
            result.files_missing += 1;
            continue;
        }
        result.files_loaded += 1;
        let file = File::open(path).expect("open JSONL file");
        let reader = BufReader::new(file);
        for line in reader.lines() {
            match line {
                Ok(text) if text.trim().is_empty() => continue,
                Ok(text) => match serde_json::from_str::<T>(&text) {
                    Ok(val) => result.records.push(val),
                    Err(_) => result.errors += 1,
                },
                Err(_) => result.errors += 1,
            }
        }
    }
    result
}
```

### Pattern 4: Dual Output Format (Table + JSON)

**What:** Single output data model that renders as either comfy-table or JSON.

**When to use:** All CLI output.

**Why:** Success criteria explicitly require both `--output table` (default) and `--output json`.

```rust
// src/analysis/output.rs
use comfy_table::{Table, CellAlignment, ContentArrangement};
use serde::Serialize;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
}

/// Render a serializable result as either table or JSON
pub fn render_output<T: Serialize>(
    data: &T,
    format: &OutputFormat,
    table_fn: impl FnOnce(&T) -> Table,
) {
    match format {
        OutputFormat::Table => println!("{}", table_fn(data)),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data).unwrap());
        }
    }
}

/// Create a comfy-table with right-justified numeric columns
pub fn new_table(headers: &[&str]) -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(headers);
    table
}
```

### Anti-Patterns to Avoid

- **Async runtime in CLI tools:** Use plain `fn main()`, not `#[tokio::main]`. File I/O is synchronous. `tokio::time::Instant::now()` in `DualTimestamp::deserialize` works without a tokio runtime (it wraps `std::time::Instant`).

- **Loading AppConfig:** The CLI tools must work without `config.toml`. Accept all parameters via CLI flags with sensible defaults (e.g., `--log-dir spread_logs`).

- **Defining CLI-specific deserialization types:** Reuse `prediction::spread::patterns::SpreadResult` and `prediction::signal::types::ArbSignal` directly. They already have `#[serde(default)]` on newer fields for forward compatibility.

- **Loading entire history without date bounds:** Always require `--from`/`--to` or `--last`. Never default to "all files" -- unbounded loading scales poorly.

- **Putting analysis logic in binaries:** Thin binary parses args and calls `prediction::analysis::*` functions. Logic in the library is unit-testable without running the binary.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Terminal table formatting | Manual `format!` width specifiers | comfy-table 7 | Variable-width numbers break manual alignment; column wrapping, borders, section headers are complex |
| JSON serialization | Manual string concatenation | `serde_json::to_string_pretty` | Escaping, nested objects, Decimal-as-string handling already solved by serde derives |
| CLI argument parsing | Manual `std::env::args` parsing | clap 4.5 derive | Type validation, help text, error messages, value parsing all handled |
| Date arithmetic | Manual day counting | chrono `NaiveDate + Duration::days` | Leap years, month boundaries, off-by-one errors |
| Percentile computation | Sort-and-index manually | Shared `stats::percentile` function | Linear interpolation edge cases, empty/single-element handling |
| Wilson score CI | Wald interval (p +/- z*sqrt(p*(1-p)/n)) | Wilson score formula | Wald overshoots [0,1] at small n; Wilson is correct for all n >= 10 |

**Key insight:** The infrastructure phase should create reusable primitives that Phases 27-28 consume without duplication.

## Common Pitfalls

### Pitfall 1: DualTimestamp Tokio Dependency

**What goes wrong:** `DualTimestamp::deserialize` calls `tokio::time::Instant::now()`. Developer assumes this requires `#[tokio::main]` and adds async runtime to CLI binary.

**Why it happens:** Natural assumption that tokio types need a tokio runtime.

**How to avoid:** `tokio::time::Instant::now()` wraps `std::time::Instant::now()` and does NOT panic without a runtime. The only difference is when tokio's test-time-pause feature is active. Plain `fn main()` works. Verified from tokio docs.

**Warning signs:** Adding `#[tokio::main]` or `tokio::runtime::Runtime::new()` to CLI binary.

### Pitfall 2: Missing `--last N` Flag

**What goes wrong:** Only implementing `--from`/`--to` but forgetting `--last N` which is listed in the success criteria.

**Why it happens:** `--last N` is syntactic sugar for `--from (today - N) --to today`, easy to overlook.

**How to avoid:** `DateRange::from_args` handles all three: `(from, to)`, `(last)`, and validates mutual exclusivity.

**Warning signs:** Running `spread-analytics --last 7` produces an error.

### Pitfall 3: Spread Log File Prefix Differs from Signal Log

**What goes wrong:** Spread logs are `{YYYY-MM-DD}.jsonl` (no prefix) but paper trade logs are `trades-{YYYY-MM-DD}.jsonl` (with prefix). Developer assumes all log types use the same naming.

**Why it happens:** Different loggers evolved independently.

**How to avoid:** The `files_in_range` function should accept a filename format pattern or prefix parameter. For Phase 26 scope (spread_logs and signal_logs), both use `{YYYY-MM-DD}.jsonl` with no prefix. But paper_trade logs use `trades-{YYYY-MM-DD}.jsonl`, and settlement logs use `settlements-{YYYY-MM-DD}.jsonl`. Build the function to handle both patterns from the start.

**Warning signs:** Paper trade JSONL files not found when signal-scoring CLI tries to load them in Phase 28.

### Pitfall 4: OutputFormat Enum Not Clap-Compatible

**What goes wrong:** Defining `OutputFormat` as a regular enum and then needing custom parsing for clap.

**Why it happens:** Forgetting that clap needs `ValueEnum` derive.

**How to avoid:** Derive `clap::ValueEnum` on `OutputFormat` so `--output table` and `--output json` parse automatically.

**Warning signs:** Compile error about `ValueEnum` not implemented.

### Pitfall 5: Decimal Display Inconsistency

**What goes wrong:** `rust_decimal` preserves trailing zeros. The value `"0.0100"` deserializes as `Decimal` and displays as `0.0100`, not `0.01`. Different fields show different decimal places in the table.

**Why it happens:** Decimal's Display trait preserves the internal precision.

**How to avoid:** Use `Decimal::normalize()` to strip trailing zeros before display, OR define fixed decimal place formatting per field type (4 dp for probabilities/edges, 2 dp for dollar amounts). In Phase 26, establish the formatting convention in the output module so Phases 27-28 inherit it.

**Warning signs:** Table columns with inconsistent decimal places (e.g., `0.0500` next to `0.03`).

## Code Examples

Verified patterns from codebase analysis and official docs:

### CLI Binary Structure (clap derive)
```rust
// src/bin/spread_analytics.rs -- Phase 26 delivers this skeleton
use clap::Parser;
use chrono::NaiveDate;
use std::path::PathBuf;
use prediction::analysis::io::DateRange;
use prediction::analysis::output::OutputFormat;

#[derive(Parser)]
#[command(name = "spread-analytics")]
#[command(about = "Analyze spread distribution patterns from recorded JSONL data")]
struct Cli {
    #[arg(long)]
    from: Option<NaiveDate>,

    #[arg(long)]
    to: Option<NaiveDate>,

    #[arg(long)]
    last: Option<u32>,

    #[arg(long, default_value = "table")]
    output: OutputFormat,

    #[arg(long)]
    by_event: bool,

    #[arg(long, default_value = "spread_logs")]
    log_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;

    // Phase 27 adds: actual spread computation and rendering
    // Phase 26 delivers: skeleton that shows --help, loads files, renders placeholder
    let files = range.files_in_dir(&cli.log_dir);
    eprintln!("Loading {} files in range {} to {}", files.len(), range.from, range.to);

    Ok(())
}
```

### comfy-table with Right-Justified Numerics
```rust
// Source: comfy-table docs.rs CellAlignment
use comfy_table::{Table, Cell, CellAlignment, ContentArrangement};

fn render_summary_table(stats: &SummaryStats) -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Metric", "Value"]);

    // Right-justify the Value column
    if let Some(column) = table.column_mut(1) {
        column.set_cell_alignment(CellAlignment::Right);
    }

    table.add_row(vec!["Count", &stats.count.to_string()]);
    table.add_row(vec!["Mean", &format!("{:.4}", stats.mean)]);
    table.add_row(vec!["Median", &format!("{:.4}", stats.median)]);

    table
}
```

### Shared Statistics Module (Pure Functions)
```rust
// src/analysis/stats.rs -- Phase 26 delivers these
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

pub fn mean_decimal(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() { return None; }
    let sum: Decimal = values.iter().copied().sum();
    Some(sum / Decimal::from(values.len()))
}

pub fn stddev_f64(values: &[f64]) -> Option<f64> {
    if values.len() < 2 { return None; }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter()
        .map(|v| { let d = v - mean; d * d })
        .sum::<f64>() / (n - 1.0); // sample stddev
    Some(variance.sqrt())
}

pub fn percentile_f64(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() { return None; }
    if sorted.len() == 1 { return Some(sorted[0]); }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let frac = rank - lower as f64;
    if lower == upper || upper >= sorted.len() {
        Some(sorted[lower.min(sorted.len() - 1)])
    } else {
        Some(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
    }
}

/// Wilson score confidence interval for a proportion.
/// Returns (lower, upper) bounds.
pub fn wilson_ci(successes: usize, total: usize, z: f64) -> Option<(f64, f64)> {
    if total == 0 { return None; }
    let n = total as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = (z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / denom;
    Some((center - margin, center + margin))
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| prettytable-rs | comfy-table 7 | 2019 (prettytable unmaintained) | comfy-table is the maintained, feature-complete replacement |
| Manual CLI arg parsing | clap 4.x with derive | clap 3 -> 4 (2022) | Derive macros reduce boilerplate; already in project |
| csv crate for all export | serde_json for JSON, comfy-table for tables | Project decision | Phase 26 scope requires only table + JSON; csv deferred |

**Deprecated/outdated:**
- prettytable-rs: unmaintained since 2019, use comfy-table
- clap 2/3: project already on clap 4.5

## Open Questions

1. **Should stats module use `Decimal` or `f64` for computation?**
   - What we know: All JSONL fields use `Decimal`. `statrs` functions require `f64`. Existing `RollingStats` uses `f64` throughout.
   - What's unclear: Whether to accept `&[Decimal]` and convert internally, or convert at the caller boundary.
   - Recommendation: Accept `&[Decimal]` for functions that accumulate sums (mean) to preserve precision. Accept `&[f64]` for statistical functions that inherently need floating-point (stddev, percentile, Wilson CI). Conversion boundary is at the stats module API surface. This matches the existing pattern where `rust_decimal` handles financial values and `f64` handles statistical computation.

2. **What to render when Phase 26 is complete but Phase 27/28 have not added computations?**
   - What we know: Success criteria say `--help` must work, date filtering must work, table/JSON output must work.
   - What's unclear: What the "table output" contains before any analysis is implemented.
   - Recommendation: Phase 26 delivers a "loading summary" placeholder that shows date range, file count, record count, and error count. This validates all four INFRA requirements without needing Phase 27/28 computations. The actual analysis sections are added in those phases.

## Sources

### Primary (HIGH confidence)
- Direct source analysis: `src/spread/patterns.rs` (SpreadResult type, 258 lines)
- Direct source analysis: `src/signal/types.rs` (ArbSignal type, 318 lines)
- Direct source analysis: `src/types/timestamp.rs` (DualTimestamp, 54 lines) -- confirmed `tokio::time::Instant::now()` wraps std::Instant
- Direct source analysis: `Cargo.toml` -- dependency inventory, existing clap/statrs/chrono/rust_decimal
- Direct source analysis: `src/main.rs` -- existing clap Parser/Subcommand pattern
- Direct source analysis: `src/spread/rolling_stats.rs` -- existing mean/stddev/percentile pattern (f64-based)
- Direct inspection: `signal_logs/*.jsonl` -- 6 days of live soak data, confirmed JSONL schema and filename convention
- [comfy-table docs.rs](https://docs.rs/comfy-table/latest/comfy_table/) -- CellAlignment::Right, builder API, version 7.x
- [comfy-table CellAlignment](https://docs.rs/comfy-table/latest/comfy_table/enum.CellAlignment.html) -- Left/Right/Center alignment options
- [tokio::time::Instant docs](https://docs.rs/tokio/latest/tokio/time/struct.Instant.html) -- wraps std Instant, no runtime required for `now()`

### Secondary (MEDIUM confidence)
- Project research documents: `.planning/research/STACK.md`, `ARCHITECTURE.md`, `PITFALLS.md`, `FEATURES.md`, `SUMMARY.md` -- comprehensive pre-milestone research with codebase analysis

### Tertiary (LOW confidence)
- None. All findings verified from primary sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies verified from Cargo.toml and docs.rs; comfy-table is the only new crate
- Architecture: HIGH -- based on direct codebase analysis; patterns follow existing conventions; `[[bin]]` structure verified against clap and Cargo docs
- Pitfalls: HIGH -- DualTimestamp/tokio issue verified from source code; filename convention verified from logger source and actual files
- Data model: HIGH -- SpreadResult and ArbSignal types verified from source with serde roundtrip tests

**Research date:** 2026-02-28
**Valid until:** 2026-03-28 (stable -- infrastructure patterns, no fast-moving dependencies)
