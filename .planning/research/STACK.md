# Stack Research: v1.8 Signal Quality Validation

**Domain:** Signal quality analysis, cost model tuning, market microstructure analysis, liquidity assessment
**Researched:** 2026-03-09
**Confidence:** HIGH (minimal new dependencies; most work extends existing stats module and CLI patterns)

## Scope

This document covers ONLY the stack additions/changes needed for v1.8 Signal Quality Validation. The existing Rust application stack (v1.0-v1.7) is unchanged. This milestone is primarily a **data analysis milestone** -- the bulk of work is new CLI tools and analysis functions built on existing infrastructure.

---

## Executive Finding: Zero or One New Dependency

The v1.8 analysis work needs:
1. **Statistical functions** beyond what the existing `stats.rs` module provides (linear regression, correlation, Kolmogorov-Smirnov test)
2. **Order book liquidity metrics** (bid-ask spread, book depth, VWAP-like walk-the-book analysis)
3. **Cost model sensitivity analysis** (parameter sweeps over fee/slippage/carry parameters)
4. **Instrument matching quality audit** (comparison of paired contract terms)

Of these, items 2-4 require zero new dependencies -- they are pure Rust computation over existing data structures using `rust_decimal`, `statrs`, and the existing `stats.rs` module.

Item 1 (linear regression/correlation) can be implemented in ~50 lines of Rust for simple linear regression and Pearson correlation. For more rigorous regression with R-squared, standard errors, and p-values, the `linregress` crate (0.5.4) is a lightweight option at ~500 lines of code with minimal transitive deps.

**Recommendation: Add `linregress = "0.5"` as the single new dependency.** It provides OLS regression with R-squared, t-statistics, p-values, and standard errors -- exactly what cost model tuning needs. Hand-rolling regression with proper inference would duplicate this work.

---

## Recommended Stack

### New Dependencies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| linregress | 0.5.4 | OLS linear regression with R-squared, t-stats, p-values | Cost model parameter sensitivity analysis needs regression with inference. ~500 LOC library, minimal deps (nalgebra). Hand-rolling regression with proper standard errors and p-values is error-prone. |

### Existing Technologies (No Version Changes)

| Technology | Version | Purpose | Role in v1.8 |
|------------|---------|---------|--------------|
| statrs | 0.18 | Statistical distributions (Normal, T, Chi-Squared CDF/PDF) | Two-sample KS test critical values, t-distribution for regression significance, goodness-of-fit tests |
| rust_decimal | 1.40 | Decimal arithmetic | All cost model computations, fee breakdowns, spread calculations remain in Decimal |
| serde + serde_json | 1.0 | Serialization | JSONL loading for spread/signal logs, JSON output from new CLI tools |
| clap | 4.5 | CLI argument parsing | New CLI binaries follow existing `spread-analytics` and `signal-scoring` patterns |
| comfy-table | 7 | Terminal table rendering | Table output mode for new analysis tools |
| chrono | 0.4 | Timestamps and date ranges | Date-range file enumeration for JSONL loading (existing `DateRange` pattern) |
| tracing | 0.1 | Structured logging | Diagnostic output in analysis tools |

### What the Existing Stats Module Already Provides

The `src/analysis/stats.rs` module already has everything needed for basic statistical analysis:

| Function | Provided By | Used For in v1.8 |
|----------|------------|-----------------|
| `mean_f64`, `mean_decimal` | stats.rs | Cost component means, spread distribution centers |
| `stddev_f64` | stats.rs | Spread volatility, cost variability |
| `percentile_f64`, `median_f64` | stats.rs | Spread distribution quantiles (P5/P25/P50/P75/P95) |
| `wilson_ci` | stats.rs | Confidence intervals on hit rates |
| `skewness_f64`, `kurtosis_f64` | stats.rs | Distribution shape analysis for spreads |
| Normal CDF/PDF | statrs | Probability calculations, z-scores |
| T-distribution CDF | statrs | t-test p-values for edge significance |
| `RollingStats` | rolling_stats.rs | Windowed online statistics (if needed for streaming analysis) |

### What Needs to Be Added to Stats Module (Pure Rust, No New Deps)

| Function | Lines of Code | Used For |
|----------|--------------|----------|
| `pearson_correlation(x, y)` | ~15 | Correlation between cost components and spread, instrument price correlation |
| `two_sample_ks_test(a, b)` | ~30 | Compare spread distributions across instruments/venues |
| `weighted_mean_f64(values, weights)` | ~10 | Volume-weighted spread averages |
| `coefficient_of_variation(values)` | ~5 | Cost component stability metric |
| `iqr_f64(sorted)` | ~5 | Outlier detection in spread data |

These are trivial to implement and do not justify adding a dependency.

---

## Analysis Tools Architecture

### New CLI Binaries (Following Existing Pattern)

All new tools follow the exact pattern of `spread-analytics` and `signal-scoring`:
- Synchronous `fn main()` (no tokio runtime)
- JSONL file loading via `analysis::io::load_jsonl` with `DateRange` filtering
- Dual output mode: `--output table` (default, comfy-table) or `--output json` (serde_json)
- `--by-event` breakdown support
- `--from/--to/--last` date range arguments via clap derive

| Binary | Data Source | Purpose |
|--------|------------|---------|
| `cost-analyzer` | `spread_logs/*.jsonl` | Break down cost model components, parameter sensitivity |
| `liquidity-analyzer` | `spread_logs/*.jsonl` + recorded feed data | Book depth, bid-ask spread, fill simulation |
| `instrument-audit` | `events.toml` + `spread_logs/*.jsonl` | Matching quality, expiry alignment, price correlation |

### No New Infrastructure

No database, no new services, no new Prometheus metrics (beyond what analysis reveals should be added to the live pipeline). This is offline batch analysis.

---

## Cost Model Analysis Stack Details

### What Already Exists

The cost model (`src/spread/cost_model.rs`) already computes:
- Polymarket dynamic fees: `shares * fee_rate * (p * (1-p))^exponent`
- Kalshi taker fees: `coefficient * contracts * P * (1-P)` with optional ceiling rounding
- Carry cost: `notional * annualized_rate * holding_days / 365`
- Total one-way cost: `fee + carry`

The spread engine (`src/spread/engine.rs`) logs `SpreadResult` to JSONL with:
- Gross spread, net spread, cost breakdown
- Venue exchange timestamps
- Bid/ask prices used, book depth walked

### What v1.8 Adds (Code, Not Deps)

**Parameter sweep analysis:** Iterate over ranges of cost model parameters (fee_rate, exponent, carry_rate, slippage_bps) and recompute net spreads from historical gross spreads. This is pure arithmetic over `Vec<SpreadResult>` -- no simulation framework needed.

**Regression analysis (uses linregress):** Regress net spread against cost components to identify which cost term dominates the negative edge. Provides R-squared and p-values for each component's contribution.

**Break-even analysis:** Compute the cost parameter values where mean net spread crosses zero. Simple root-finding over the parameter sweep results.

---

## Liquidity Analysis Stack Details

### What Already Exists

The order book structure is already parsed and maintained in memory:
- `OrderBook` with `bids` and `asks` as `BTreeMap<Decimal, Decimal>` (price -> size)
- `book_walker.rs`: Walk-the-book slippage calculator that simulates filling an order through book levels
- `MarketSnapshot` captures best bid/ask and book depth

### What v1.8 Adds (Code, Not Deps)

All liquidity metrics are computed from the existing `OrderBook` and `MarketSnapshot` data structures:

| Metric | Computation | Source |
|--------|------------|--------|
| Bid-ask spread (absolute & bps) | `best_ask - best_bid` | MarketSnapshot |
| Book depth at N levels | Sum sizes at top N price levels | OrderBook BTreeMap |
| Cumulative depth to price | Sum sizes from best to target price | OrderBook BTreeMap |
| Effective spread (after slippage) | Walk-the-book for target size | Existing `book_walker.rs` |
| Book imbalance | `(bid_depth - ask_depth) / (bid_depth + ask_depth)` | OrderBook BTreeMap |
| Depth ratio at top-of-book | `bid_size_L1 / ask_size_L1` | OrderBook BTreeMap |
| Quote stability (time-weighted) | Track quote changes over time in JSONL | MarketSnapshot timestamps |

**No external order book library needed.** The codebase already parses, maintains, and walks the book for all 4 venues. The liquidity analyzer reads recorded feed data and computes metrics offline.

---

## Instrument Matching Audit Stack Details

### What Already Exists

- `events.toml` with event mappings across venues (instrument IDs, strikes, expiries)
- `strsim` crate for fuzzy string matching (used in discovery)
- `FuzzyMatchKey` (asset/strike/direction) matching logic

### What v1.8 Adds (Code, Not Deps)

- **Expiry alignment check:** Compare expiry timestamps across venues, flag mismatches beyond tolerance
- **Price correlation analysis:** Compute Pearson correlation between venue prices for each paired instrument over time (uses new `pearson_correlation` function in stats.rs)
- **Strike mapping verification:** For options-implied probabilities, verify the strike price used actually brackets the prediction market question
- **Coverage report:** Which instruments have sufficient data density for reliable signal generation

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `linregress` 0.5 | Hand-rolled OLS regression | `linregress` provides proper inference (standard errors, t-stats, p-values, R-squared) that would be error-prone to implement from scratch. ~500 LOC library with clean API. |
| `linregress` 0.5 | `ndarray-stats` + `ndarray` | Massive dependency tree (ndarray, blas bindings). Overkill for simple OLS on <100 variables. |
| `linregress` 0.5 | `polars` for dataframe analysis | 10+ MB compile-time addition, Python-style API. We process `Vec<T>` from JSONL -- no DataFrame needed. |
| Pure Rust liquidity metrics | `orderbook-rs` crate | External crate is for building order books from scratch. We already have fully functional order books with walk-the-book. Adding a crate would duplicate existing data structures. |
| Pure Rust stats extensions | `rs-stats` or `scirs2-stats` | These are comprehensive stats packages. We need only Pearson correlation and KS test -- trivial to add to existing stats.rs (~45 lines total). |
| Existing CLI pattern | `barter-rs` backtesting framework | v1.8 is analysis of historical signals, not backtesting. We compare actual spreads against cost model parameters. No simulation engine needed. |
| JSON output + external tools | Terminal charting crate (e.g., `textplots`) | PROJECT.md explicitly states "JSON output + external tools preferred" for visualization. Terminal charts are cosmetic, not analytical. |

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `polars` / `datafusion` | Massive compile-time cost (10+ MB binary size increase). Vec-based analysis is faster for expected data volumes (<1M records). | `Vec<T>` with iterator chains -- already proven in spread-analytics and signal-scoring |
| `plotters` / `textplots` | PROJECT.md: "Terminal charting -- JSON output + external tools preferred". Visualization belongs in Grafana or external notebooks. | `--output json` piped to external visualization tools (Python matplotlib, jq+gnuplot) |
| `ndarray` / `nalgebra` (directly) | Only needed as transitive dep of linregress. No matrix operations needed in our analysis code. | linregress handles matrix math internally |
| SQLite / DuckDB | PROJECT.md: "Database backend for analysis -- JSONL sufficient at current scale; Vec<T> faster for expected volumes". | Existing JSONL loading with `load_jsonl` |
| `ta-rs` (technical analysis) | Designed for candlestick/price indicators (RSI, MACD). Prediction market arb signals are not time-series price patterns. | Custom spread/cost analysis specific to the domain |
| Full backtesting framework | PROJECT.md: "Full backtesting engine -- settled data is stronger evidence than simulated backtests". v1.8 analyzes real production data. | Parameter sweep over historical spreads using offline CLI tools |
| Any ML/AI library | PROJECT.md: "AI/ML signal prediction -- arbs are event-driven, not pattern-driven". The signal quality problem is cost model calibration, not prediction. | Statistical analysis: regression, correlation, distribution tests |
| New Prometheus metrics crate | metrics 0.24 + metrics-exporter-prometheus 0.18 are already in the stack. If analysis reveals new metrics to track, they go in the existing pipeline. | Existing metrics infrastructure |

---

## Installation

```toml
# Add to [dependencies] in Cargo.toml
linregress = "0.5"
```

```bash
# No other installation steps. cargo build picks up the new dep.
cargo build --release
```

### New Binaries (add to Cargo.toml)

```toml
[[bin]]
name = "cost-analyzer"
path = "src/bin/cost_analyzer.rs"

[[bin]]
name = "liquidity-analyzer"
path = "src/bin/liquidity_analyzer.rs"

[[bin]]
name = "instrument-audit"
path = "src/bin/instrument_audit.rs"
```

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| linregress 0.5.4 | nalgebra (transitive) | nalgebra is already a transitive dep via statrs 0.18 |
| linregress 0.5.4 | Rust 2024 edition | Verified: uses standard Rust, no edition-specific issues |
| statrs 0.18 | linregress 0.5.4 | Both depend on nalgebra; versions should unify in dependency resolution |

**Risk assessment:** linregress 0.5.4 is a stable crate (last update Oct 2024, 888K downloads). Its nalgebra dependency should unify with statrs's nalgebra dependency, meaning near-zero compile time increase.

---

## Integration Points

### How New Tools Integrate with Existing Architecture

```
Existing data flow (live pipeline):
  Feeds -> SpreadEngine -> spread_logs/*.jsonl
  Feeds -> SignalEngine -> signal_logs/*.jsonl
  Settlements -> settlement_logs/*.jsonl

Existing analysis tools:
  spread_logs/ -> spread-analytics CLI -> table/JSON
  settlement_logs/ -> signal-scoring CLI -> table/JSON

New v1.8 analysis tools:
  spread_logs/ -> cost-analyzer CLI -> table/JSON (cost breakdowns, parameter sweeps, regression)
  spread_logs/ + feed recordings -> liquidity-analyzer CLI -> table/JSON (book depth, spreads, fill sim)
  events.toml + spread_logs/ -> instrument-audit CLI -> table/JSON (matching quality, correlation)
```

All new tools are **read-only, offline, batch analysis**. They consume the same JSONL files produced by the live pipeline. No changes to the live pipeline are needed for the analysis phase.

If analysis reveals that the cost model needs tuning, the changes are to `config.toml` parameters (not code). If analysis reveals that certain instruments should be dropped, the changes are to `events.toml` (not code). If analysis reveals new metrics to track, those are added to the live pipeline in a subsequent phase.

### Shared Analysis Infrastructure

All three new tools reuse:
- `analysis::io::load_jsonl` and `DateRange` for file loading
- `analysis::output::{OutputFormat, new_table, render_output}` for rendering
- `analysis::stats::*` for statistical computations
- `spread::patterns::SpreadResult` as the primary data record type

This is the same pattern established in v1.4 and proven across 13 E2E golden-value tests.

---

## Spread Logger Fix (Prerequisite)

PROJECT.md notes: "Spread logger not producing output (spread_logs empty)". This must be fixed before any analysis tools can run. The fix is a code change in `src/spread/logger.rs` -- no new dependencies needed.

**This is the highest-priority item in v1.8.** Without spread data, all analysis tools have no input.

---

## Sources

- [linregress on crates.io](https://crates.io/crates/linregress) -- v0.5.4, 888K downloads, OLS regression with inference (HIGH confidence, verified via crates.io API)
- [statrs on crates.io](https://crates.io/crates/statrs) -- v0.18.0, latest stable, already in Cargo.toml (HIGH confidence, verified via crates.io API)
- [linregress docs](https://docs.rs/linregress) -- API reference for FormulaRegressionBuilder (MEDIUM confidence, training data)
- Existing codebase analysis: `Cargo.toml`, `src/analysis/stats.rs`, `src/spread/cost_model.rs`, `src/spread/book_walker.rs`, `src/bin/spread_analytics.rs`, `src/bin/signal_scoring.rs` -- direct code inspection (HIGH confidence)
- PROJECT.md constraints: "JSONL sufficient at current scale", "JSON output + external tools preferred", "settled data is stronger evidence than simulated backtests" (HIGH confidence, project decisions)

---
*Stack research for: v1.8 Signal Quality Validation*
*Researched: 2026-03-09*
