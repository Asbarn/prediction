# Architecture Patterns

**Domain:** Signal quality validation and cost model tuning for cross-venue arbitrage pipeline
**Researched:** 2026-03-09

## Current Architecture Overview

The existing pipeline flows as follows:

```
Feed Supervisors (Deribit, Polymarket, Kalshi, Derive)
    |
    v
Normalizer (per-venue) -> MarketSnapshot
    |
    +---> fan-out to SpreadEngine (prediction-vs-prediction: Poly <-> Kalshi)
    +---> fan-out to PricingEngine (options IV solving, probability extraction)
              |
              v
          ImpliedProbability
              |
              +---> CrossAssetEngine (prediction-vs-options: Poly/Kalshi <-> Deribit/Derive)
              +---> SignalEngine
    |
    +---> fan-out to CrossAssetEngine (prediction snapshots)
    +---> PaperTradeTracker

SpreadEngine -> spread_logs/ (JSONL, currently empty due to bug)
CrossAssetEngine -> signal_logs/ (JSONL, all signals logged regardless of threshold)
```

**Key architectural properties:**
- All engines are stateful structs consuming from `mpsc` channels
- `EventRegistry` (TOML-driven, hot-reloadable) maps instruments across venues
- Cost model is split: `SpreadEngine` uses venue-specific fee functions directly; `CrossAssetEngine` builds a `CostBreakdown` with 7 components
- CLI analysis tools (`spread-analytics`, `signal-scoring`) are pure-function, synchronous binaries reading JSONL files
- No runtime dependency between analysis tools and the live pipeline

## Recommended Architecture for v1.8

### Principle: New CLI Tools, Minimal Pipeline Changes

The v1.8 features are primarily **diagnostic and analytical**. They should follow the established pattern: offline CLI tools reading JSONL logs and config files, with only targeted pipeline fixes (spread logger bug) and config tuning as runtime changes.

**Do NOT add new engines to the live pipeline for v1.8.** The pipeline architecture is sound. The problem is in cost model parameters and instrument matching quality, not in architecture.

### Component Boundaries

| Component | Responsibility | New/Modified | Communicates With |
|-----------|---------------|--------------|-------------------|
| `spread::logger` (SpreadLogger) | Write spread computations to JSONL | **Modified** -- fix empty output bug | SpreadEngine (called by) |
| `signal_logs/` JSONL files | Raw signal data with full cost breakdown | Existing (data source) | CLI tools (read by) |
| `bin/cost-audit` CLI | Analyze cost breakdowns, identify dominant components | **New** | Reads `signal_logs/`, `config.toml` |
| `bin/match-audit` CLI | Validate instrument pairing quality in events.toml | **New** | Reads `events.toml`, Deribit/Polymarket REST APIs |
| `bin/book-depth` CLI | Analyze Polymarket book depth and liquidity | **New** | Reads `signal_logs/` or live REST snapshots |
| `spread::config` / `signal::config` | Cost model parameters | **Modified** -- tuned values | Read by SpreadEngine, CrossAssetEngine |
| `events.toml` | Instrument mappings with strike/expiry data | **Modified** -- reviewed/corrected | Read by EventRegistry |

### New Components Detail

#### 1. Spread Logger Fix (Pipeline Modification)

The SpreadLogger is structurally correct but `spread_logs/` is empty in production. This is the highest-priority fix because it gates all downstream analysis of prediction-vs-prediction spreads.

**Root cause candidates (investigate in order):**
1. SpreadEngine's `process_snapshot` at line 228 requires BOTH `polymarket` AND `kalshi` venue entries in the mapping. If events.toml only has options venue mappings for currently approved events, this early-return silently drops all snapshots.
2. `log_dir` config mismatch -- if runtime config points to a different directory than expected.
3. SpreadEngine may not be receiving snapshots at all if the fan-out wiring was broken in a later milestone.

**Integration point:** SpreadEngine.process_snapshot() -> SpreadLogger.log(). No architectural change needed -- this is a bug fix.

#### 2. Cost Audit CLI (`bin/cost-audit`)

**What it does:** Reads signal_logs/ JSONL, parses CostBreakdown from each ArbSignal, computes statistics per cost component, identifies which costs dominate and whether parameters are realistic.

**Architecture pattern:** Follows exact pattern of `bin/spread-analytics`:
- `fn main()` (synchronous, no tokio)
- Uses `analysis::io::load_jsonl::<ArbSignal>()` for file loading
- Pure computation functions in `analysis::cost_audit.rs`
- Table + JSON output via `analysis::output`
- Clap CLI with `--from`, `--to`, `--last`, `--by-event`, `--output` flags

**Key analyses:**
- Per-component statistics: mean, median, p95 for each of the 7 cost fields
- Cost dominance: which component contributes most to total_cost
- Parameter sanity check: compare computed fees against known venue fee schedules
- Carry cost vs actual holding periods (if settlement data available)
- Sensitivity: what total_cost would be with different parameter values

**Data flow:**
```
signal_logs/*.jsonl -> load_jsonl::<ArbSignal>() -> cost_audit::compute() -> output
```

**Integration with existing code:** Reuses `signal::types::ArbSignal`, `signal::types::CostBreakdown`, `analysis::io`, `analysis::output`, `analysis::stats`. Zero new dependencies.

#### 3. Match Audit CLI (`bin/match-audit`)

**What it does:** Reads events.toml, validates that paired instruments actually represent the same economic bet, checks strike coverage, and identifies mismatches.

**Architecture pattern:** Slightly different from other CLIs because it needs to:
1. Parse events.toml directly (use existing `config::EventsConfig`)
2. Optionally fetch live instrument metadata from venue REST APIs to cross-check

**Key analyses:**
- Strike alignment: Does the Polymarket question's strike price match the Deribit option's strike?
- Expiry alignment: How close are the expiry timestamps across venues?
- Direction alignment: Is "BTC above $X" mapped to the correct call option?
- Coverage gap: Are we only matching deep OTM strikes where liquidity is thin?
- Listing quality: For each approved mapping, grade the match quality (A/B/C/F)

**Data flow (offline mode):**
```
events.toml -> EventsConfig -> match_audit::validate() -> output
```

**Data flow (with live check, optional):**
```
events.toml -> EventsConfig -> REST queries to venues -> match_audit::validate_live() -> output
```

**Integration:** Reuses `config::EventsConfig`, `config::EventMapping`, `events::registry`. For live checks, reuses existing REST client functions from `feed::deribit`, `feed::polymarket`, `feed::kalshi` discovery modules. The live check mode would need tokio runtime (like the main binary), but the offline mode stays synchronous.

**Recommendation:** Start with offline-only (synchronous). Live checks can be a follow-up if offline analysis reveals issues.

#### 4. Book Depth CLI (`bin/book-depth`)

**What it does:** Analyzes Polymarket order book depth and liquidity from signal log data (which includes fill_ratio, book_depth_levels, executable_price).

**Architecture pattern:** Same as cost-audit -- reads signal_logs/ JSONL.

**Key analyses:**
- Fill ratio distribution per event: what fraction of target notional actually fills?
- Book depth levels distribution: how many levels are typically available?
- Liquidity factor distribution: how much does thin liquidity penalize signals?
- Price impact: difference between mid-price probability and executable_price
- By-event breakdown: which instruments have adequate liquidity vs. ghost books?

**Data flow:**
```
signal_logs/*.jsonl -> load_jsonl::<ArbSignal>() -> book_depth::compute() -> output
```

**Integration:** Same as cost-audit. Reuses `signal::types::ArbSignal`, `signal::types::LegInfo`, and the analysis infrastructure.

### Modified Components Detail

#### 5. Cost Model Parameter Tuning (Config Change)

After cost-audit reveals which parameters dominate, update `config.toml`:

**Likely tuning targets based on the ~$20 total cost observation:**

1. **Carry cost (moderate suspect):** `annualized_rate = 0.05` with `reference_holding_days = 30` on `target_notional = 500` produces `500 * 0.05 * 30/365 = $2.05`. This is reasonable individually but may be miscalibrated for the actual expected holding period.

2. **Kalshi taker fee with ceiling rounding (primary suspect):** `use_ceiling = true` rounds per-contract fee UP using `Decimal::ceil()`. For low-probability contracts, `0.07 * 0.001 * 0.999 = 0.00007`, `Decimal::ceil()` rounds to `1` (integer). At 500 contracts, that is $500 in fees. This is almost certainly the dominant cost bug. Kalshi's ceiling rounding should be to cents (2 decimal places), not integers.

3. **Polymarket fee at extreme probabilities:** At p=0.001 (deep OTM like BTC-105000), `(0.001 * 0.999)^2 = ~0.000001`, so fee is negligible. This is correct.

4. **Options fee estimate:** `deribit_taker_fee_rate * underlying_price * |delta|` -- at BTC ~$80K with delta ~0.01, this is `0.0003 * 80000 * 0.01 = $0.24`. Reasonable.

5. **Basis risk premium and options spread cost:** Need signal log data to assess magnitudes.

**Critical code-level finding:** The `kalshi_taker_fee()` function in `spread::cost_model` line 57 calls `per_contract_raw.ceil()`. `Decimal::ceil()` rounds to the nearest **integer ceiling**, not to cents. A per-contract raw fee of $0.0175 gets ceiling'd to $1.00, then multiplied by contract count. This makes Kalshi-leg trades appear absurdly expensive for any non-trivial contract count.

**Fix:** `(per_contract_raw * Decimal::new(100, 0)).ceil() / Decimal::new(100, 0)` to round to cents, matching Kalshi's actual fee structure which rounds to the nearest cent.

#### 6. Events.toml Review (Config Change)

Currently `events = []` in production -- no approved mappings. This means:
- Discovery found candidates but none were approved
- OR events expired and were archived

This is a data/operational issue, not architectural. The match-audit CLI will help validate any new mappings before approval.

## Data Flow for New Features

### Complete v1.8 Data Flow

```
EXISTING (pipeline, running in production):
  Feed Supervisors -> Normalizer -> fan-out
    -> SpreadEngine -> spread_logs/ (BROKEN, needs fix)
    -> PricingEngine -> CrossAssetEngine -> signal_logs/ (working)
    -> PaperTradeTracker

NEW (offline CLI tools, run on-demand):
  signal_logs/ ----> cost-audit CLI ----> cost breakdown analysis
  signal_logs/ ----> book-depth CLI ----> liquidity analysis
  events.toml ----> match-audit CLI ----> instrument pairing quality
  spread_logs/ ---> spread-analytics CLI (existing, needs data from fix)

MODIFIED (config, deployed via restart):
  config.toml ----> tuned cost parameters ----> SpreadEngine, CrossAssetEngine
```

### No New Channels or Runtime Components

The v1.8 architecture adds **zero new async channels** and **zero new tokio tasks**. All new functionality is offline CLI tools or config changes. The only runtime change is the SpreadLogger bug fix.

## Patterns to Follow

### Pattern 1: CLI Analysis Tool

**What:** Synchronous binary reading JSONL, computing statistics, outputting table/JSON.
**When:** Any new offline analysis need.
**Why:** Proven pattern from v1.4 with 13 E2E golden-value tests. No runtime risk.

```rust
// src/bin/cost_audit.rs
use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use std::path::PathBuf;
use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::OutputFormat;
use prediction::signal::types::ArbSignal;

#[derive(Parser)]
struct Cli {
    #[arg(long)] from: Option<NaiveDate>,
    #[arg(long)] to: Option<NaiveDate>,
    #[arg(long)] last: Option<u32>,
    #[arg(long, default_value = "table")] output: OutputFormat,
    #[arg(long)] by_event: bool,
    #[arg(long, default_value = "signal_logs")] log_dir: PathBuf,
}

fn main() -> Result<()> {
    // ... load, compute, output (pure functions)
}
```

### Pattern 2: Config-Driven Tuning

**What:** All cost model parameters in TOML, adjustable without recompilation.
**When:** Parameter changes based on analysis findings.
**Why:** Established pattern since v1.0. Hot-reloadable for events, restart-required for spread/signal config.

```toml
# config.toml adjustments after analysis
[spread.kalshi_fees]
taker_coefficient = "0.07"
use_ceiling = true          # Keep: but fix ceiling implementation to round to cents

[spread.carry]
annualized_rate = "0.05"
reference_holding_days = 7  # Tuned down from 30 based on actual holding periods
```

### Pattern 3: Reuse Analysis Infrastructure

**What:** New analysis modules in `src/analysis/` using existing stats, io, output modules.
**When:** Any new analytical computation.
**Why:** DRY. The stats module (mean, stddev, percentile, wilson_ci, skewness, kurtosis) and io module (tolerant JSONL loader, date-range file enumeration) are battle-tested.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Adding Live Pipeline Components for Diagnostics

**What:** Inserting new engines or channels into the live pipeline for analysis purposes.
**Why bad:** Increases runtime complexity, potential for backpressure issues, harder to test.
**Instead:** Offline CLI tools reading JSONL logs. The pipeline already logs everything needed.

### Anti-Pattern 2: Database for Analysis

**What:** Introducing SQLite/DuckDB for querying signal data.
**Why bad:** Explicitly out of scope per PROJECT.md. JSONL + Vec<T> is faster at expected volumes.
**Instead:** Continue with JSONL + in-memory analysis in CLI tools.

### Anti-Pattern 3: Modifying SpreadResult/ArbSignal Schema for Analysis

**What:** Adding fields to the serialized types to carry analysis metadata.
**Why bad:** Schema changes break existing log files, golden tests, and any downstream consumers.
**Instead:** Analysis CLIs compute derived metrics from existing fields. If new pipeline data is needed, add it as a separate log stream.

### Anti-Pattern 4: One Mega-Analysis CLI

**What:** Single binary with subcommands for all analysis tasks.
**Why bad:** Violates single-responsibility. Harder to test. Longer compile times for changes.
**Instead:** Separate binaries per analysis domain (established pattern: `spread-analytics`, `signal-scoring`, now `cost-audit`, `match-audit`, `book-depth`).

## Suggested Build Order

The build order is driven by three dependency chains:

**Chain A: Spread Logger Fix (gates all spread analysis)**
1. Fix SpreadLogger bug (investigate + fix)
2. Verify spread_logs/ populated
3. Run existing spread-analytics CLI on new data

**Chain B: Signal Analysis (can start immediately, signal_logs/ already has data)**
1. Build cost-audit CLI
2. Build book-depth CLI
3. Analyze results, identify parameter issues

**Chain C: Instrument Matching (independent of A and B)**
1. Build match-audit CLI (offline mode)
2. Audit current/recent events.toml entries
3. Identify strike coverage gaps

**Chain D: Cost Model Tuning (depends on B and C results)**
1. Fix Kalshi ceiling rounding bug (suspected primary issue)
2. Tune config.toml parameters based on cost-audit findings
3. Deploy updated config, monitor signal quality improvement

**Recommended phase ordering:**

```
Phase 1: Spread Logger Fix + Cost Audit CLI    [A1-A2, B1]
         (parallel: fix enables future analysis; cost-audit uses existing signal_logs)

Phase 2: Book Depth + Match Audit CLIs         [B2, C1-C2]
         (parallel: both are independent analysis tools)

Phase 3: Cost Model Fixes + Tuning             [D1-D3]
         (depends on: cost-audit results from Phase 1, match-audit from Phase 2)

Phase 4: Verification                          [A3]
         (run all CLIs on post-tuning data, verify improvement)
```

**Rationale:** The Kalshi ceiling rounding fix (D1) could theoretically be done in Phase 1 since the code bug is apparent from reading the source, but the disciplined approach is to confirm with cost-audit data first. If speed is preferred, D1 can be moved to Phase 1.

## Scalability Considerations

Not applicable for v1.8. All new components are offline CLI tools processing JSONL files that accumulate at ~5 records/second (one per signal computation cycle). At this rate, a month of data is ~13M records, easily fitting in memory for Vec<T> analysis.

## Component Interaction Diagram

```
+------------------+     +-----------------+     +------------------+
| config.toml      |---->| SpreadEngine    |---->| spread_logs/     |
| [spread.*]       |     | (fix logger)    |     | (currently empty)|
+------------------+     +-----------------+     +--------+---------+
                                                          |
+------------------+     +-----------------+     +--------v---------+
| config.toml      |---->| CrossAssetEngine|---->| signal_logs/     |
| [signals.*]      |     | (existing)      |     | (has data)       |
+------------------+     +-----------------+     +--------+---------+
                                                          |
                          +--------------------+          |
                          | cost-audit CLI     |<---------+
                          | (NEW)              |          |
                          +--------------------+          |
                                                          |
                          +--------------------+          |
                          | book-depth CLI     |<---------+
                          | (NEW)              |
                          +--------------------+

+------------------+     +--------------------+
| events.toml      |---->| match-audit CLI    |
|                  |     | (NEW)              |
+------------------+     +--------------------+

+------------------+     +--------------------+     +------------------+
| cost-audit       |---->| Parameter tuning   |---->| config.toml      |
| results          |     | (human decision)   |     | (updated values) |
+------------------+     +--------------------+     +------------------+
```

## Sources

- All findings based on direct source code analysis of the production codebase
- `src/spread/cost_model.rs` -- Kalshi ceiling rounding implementation (lines 52-61)
- `src/spread/engine.rs` -- SpreadEngine snapshot processing with Poly+Kalshi gate (line 228)
- `src/signal/engine.rs` -- CrossAssetEngine cost model (lines 412-471)
- `src/spread/config.rs` -- Cost model configuration structs and defaults
- `config/config.toml` -- Production configuration values
- `config/events.toml` -- Empty events array (no approved mappings)
- `src/analysis/` -- Existing analysis infrastructure (io, output, stats modules)
- `src/bin/spread_analytics.rs` -- Reference CLI tool pattern
