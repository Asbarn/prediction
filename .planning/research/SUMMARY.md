# Project Research Summary

**Project:** Prediction Market Arbitrage System — v1.4 Analysis Tooling
**Domain:** CLI-based statistical analysis for cross-venue prediction market arbitrage
**Researched:** 2026-02-28
**Confidence:** HIGH

## Executive Summary

v1.4 adds two offline CLI analysis tools — `spread-analytics` and `signal-scoring` — that read existing JSONL log files and produce the statistical summaries needed to make a go/no-go decision for v2 live execution. The research reveals a project in an enviable position: the data pipeline already exists (SpreadResult and ArbSignal JSONL files), the types already derive serde, and all but two new crate dependencies are already in Cargo.toml. The recommended implementation path is two separate `[[bin]]` targets sharing a new `src/analysis/` library module, with streaming line-by-line JSONL parsing, and date-range filtering at the file level. Total new dependencies: `comfy-table 7.2` (terminal tables) and `csv 1.4` (CSV export). Everything else is covered by existing crates.

The statistical surface is well-understood but contains one critical domain-specific trap: the standard Sharpe ratio annualization formula (mean/stddev * sqrt(252)) is mathematically invalid for binary event outcomes. Prediction market positions are bimodal, correlated, and irregularly-timed — every assumption behind sqrt(N) annualization is violated. The research mandates reporting per-trade Sharpe (no annualization) as the primary metric, accompanied by a Probabilistic Sharpe Ratio (PSR) that quantifies confidence given the small sample sizes that early soak testing will produce. Secondary metrics — Wilson score confidence intervals for hit rate, Student's t for mean edge, and absolute max drawdown annotated with settlement count — are all well-specified and implementable with existing dependencies.

The three highest-risk implementation choices are: (1) ensuring the data loading layer streams rather than bulk-loads JSONL files, because spread logs can reach 725 MB/day at the system's tick rate; (2) avoiding survivorship bias by reporting all position states (settled, open, timed-out) rather than only settled positions; and (3) always segmenting metrics by venue-pair type rather than aggregating prediction-vs-prediction with prediction-vs-options, as the two categories have incompatible cost structures and risk profiles. All three are architectural decisions that must be made before implementation begins.

---

## Key Findings

### Recommended Stack

The existing v1.0–v1.3 stack handles all v1.4 needs with two additions. `comfy-table 7.2` is the correct choice for terminal table output — it is feature-complete, has a single transitive dependency, and its builder API suits dynamic summary tables better than tabled's derive-macro approach. The canonical `csv 1.4` crate by BurntSushi handles CSV export with correct quoting/escaping and serde integration. All statistical computation (Sharpe CI, Wilson score, max drawdown) is covered by `statrs 0.18`, `rust_decimal 1.40`, and basic f64 arithmetic already in the project.

**Core technologies:**
- `comfy-table 7.2`: Terminal table rendering — builder API fits dynamic summary rows, "finished" project status, 58M+ downloads, 1 transitive dep
- `csv 1.4`: CSV export — canonical BurntSushi implementation, 129M+ downloads, serde-aware Writer, no credible alternative
- `statrs 0.18` (existing): Statistical functions — Normal distribution inverse CDF for Wilson score and PSR; already used for Black-76 pricing
- `clap 4.5` (existing): CLI argument parsing — already in deps with derive feature; subcommands, defaults, help all available
- `chrono 0.4` (existing): Timestamp handling — millisecond parsing, hour extraction, date arithmetic; all JSONL timestamps already use this
- `rust_decimal 1.40` (existing): Decimal arithmetic — all SpreadResult and ArbSignal financial fields already use this serde integration
- `serde_json 1.0` (existing): JSONL deserialization — all log types already derive Serialize + Deserialize

**What NOT to add:** polars (200+ transitive deps for Vec<f64> operations), ndarray/nalgebra (no matrix math needed), plotters (tables convey analysis information more precisely), tokio in analysis binaries (synchronous file I/O only; no async needed).

### Expected Features

All 9 table stakes features are required for the go/no-go decision. They naturally decompose into two CLIs: `spread-analytics` handles spread distribution analysis, and `signal-scoring` handles trade performance evaluation.

**Must have (table stakes — v1.4 ship criteria):**
- TS-8: Date-range filtering (`--from`, `--to`, `--last N`) — foundation for all analysis; JSONL filename convention makes file-level filtering trivial
- TS-9: Terminal table output — `comfy-table` for aligned columns, headers, and section separators
- TS-1: Spread distribution summary stats — count, mean, median, stddev, min, max, p5/p25/p75/p95 per venue pair
- TS-2: Hourly time-bucket analysis — 24-row table showing when opportunities appear; essential for v2 scheduling decisions
- TS-3: Venue-pair breakdown — which pairs produce positive mean net_spread; direct capital allocation input
- TS-4: Hit rate with Wilson score confidence intervals — Wilson score mandatory (not Wald) because of small n; must show sample size alongside CI
- TS-5: Cost-adjusted edge with t-test significance — mean net edge, SE, t-statistic, p-value; answers "is the edge real after costs?"
- TS-6: Sharpe ratio — per-trade (non-annualized) as primary metric; PSR accompanies it
- TS-7: Maximum drawdown — absolute dollar terms with settlement count annotation; not percentage (arbitrary denominator)

**Should have (include if time permits):**
- DIFF-1: Probabilistic Sharpe Ratio — trivial marginal cost post-Sharpe; high value for small-sample decisions
- DIFF-3: Per-event breakdown — low cost, reveals edge concentration vs. distribution
- DIFF-5: JSON output mode — `--format json` for `jq` piping and analysis snapshots

**Defer to future milestones:**
- DIFF-2: Threshold effectiveness analysis — valuable but requires additional data pipeline work
- DIFF-4: Spread autocorrelation — optimization concern, not go/no-go
- DIFF-6: Comparative period analysis — manual two-run comparison is sufficient initially

**Explicit anti-features (do not build):**
- Real-time TUI dashboard — offline analysis tool; live monitoring is Prometheus + Grafana
- Database backend — JSONL at current scale loads in under 1 second; no schema/migration burden justified
- Automated go/no-go decision — human judgment required; CLI presents statistics, operator decides
- Charting/plotting — tables convey the same information more precisely; CSV + external tools for visualization

### Architecture Approach

The architecture is additive-only: two new `[[bin]]` targets, one new library module (`src/analysis/`), and two new crate dependencies. The main binary, all feed modules, spread engine, signal engine, paper trade tracker, and settlement system are untouched. The analysis binaries share existing serde types for deserialization (zero type duplication) and link against the `prediction` library crate for those types. Streaming line-by-line JSONL parsing with date-range file enumeration handles arbitrarily large log files without memory pressure.

**Major components:**
1. `src/bin/spread_analytics.rs` — CLI entry point: arg parsing (clap), date-range file enumeration, output routing (table/json/csv)
2. `src/bin/signal_scoring.rs` — CLI entry point: same pattern; links settlement data optionally
3. `src/analysis/stats.rs` — shared pure statistical functions: mean, stddev, percentile, Wilson score CI, Sharpe, PSR, max drawdown; fully unit-testable without file I/O
4. `src/analysis/spread_analytics.rs` — time bucketing, venue-pair slicing, spread distribution; takes `&[SpreadResult]`
5. `src/analysis/signal_scoring.rs` — hit rate, Sharpe, drawdown, edge CI, threshold effectiveness; takes `&[ArbSignal]`

**Key patterns:**
- Date-range file enumeration: construct filenames from dates (`{YYYY-MM-DD}.jsonl`), skip missing files — avoids filesystem scanning
- Tolerant JSONL deserialization: warn on malformed lines and continue; count errors; do not abort the analysis
- Bucket aggregation with BTreeMap: `O(hours * venue_pairs)` memory, not `O(records)` — critical for large spread logs
- `Decimal` for all financial computation; convert to `f64` only at the statistics boundary (statrs input) and display output

**Binary structure decision:** Separate `[[bin]]` targets, not subcommands of the main binary. Rationale: analysis tools do synchronous file I/O only (no tokio needed), require no config files or API credentials, and should not touch `src/main.rs` at all.

### Critical Pitfalls

1. **Invalid Sharpe annualization for binary events** — Binary outcomes are bimodal (not normal), correlated (same-expiry clustering), and irregularly-timed (no natural period). `sqrt(252)` or any fixed annualization factor is statistically invalid. Report per-trade Sharpe (no scaling) as the primary metric. Add PSR for confidence quantification. Add Sortino for downside-only comparison. Emit a CLI warning if annualized Sharpe > 3.0 with fewer than 100 trades.

2. **Survivorship bias from analyzing only settled positions** — Open positions (long-dated events still running), timed-out positions (168-hour polling timeout = probable loss), and filtered signals are all excluded if the CLI only processes settled records. Report total/settled/open/timed-out separately. Count timed-out positions as losses equal to entry cost. Require at least 80% settlement ratio before metrics are considered actionable.

3. **Confidence intervals mislead with small samples (n < 30)** — Standard Wald hit rate CI overshoots [0, 1] at small n. Use Wilson score interval for all proportions. Use Student's t (not normal) for mean edge, with a prominent sample-size warning when n < 20. Always display `(n=15)` next to every CI. Report the exact settled count needed to reach a target CI width.

4. **Look-ahead bias in threshold effectiveness analysis** — "What threshold would have maximized profit?" is in-sample optimization, not predictive. Exclude cold-start observations (where `is_cold_start == true`). Split data chronologically into train/test halves. Present threshold tradeoff curves, never a single "optimal" value.

5. **Mixing venue-pair types in aggregate metrics** — Prediction-vs-prediction (Kalshi/Polymarket) and prediction-vs-options (Deribit/Polymarket, Deribit/Kalshi) have incompatible cost structures, edge distributions, and risk profiles. Always report metrics separately by pair type. Never show a single aggregate hit rate. Go/no-go recommendation is per-pair-type, not aggregate.

---

## Implications for Roadmap

Based on the combined research, the implementation follows a clear dependency-respecting build order. The architecture's layered design (stats module -> computation modules -> CLI binaries) maps directly to phases.

### Phase 1: Foundation — Analysis Infrastructure
**Rationale:** The `src/analysis/stats.rs` module has zero dependencies on new code and is a prerequisite for both CLIs. Building it first with comprehensive unit tests de-risks the critical statistical correctness decisions (Wilson score formula, PSR formula, drawdown edge cases) before they are embedded in a binary. The Cargo.toml changes (two new `[[bin]]` entries, two new crate deps, `pub mod analysis` in lib.rs) also belong here — purely additive, no risk.
**Delivers:** `comfy-table` and `csv` added to Cargo.toml; `src/analysis/` module structure wired up; `stats.rs` with mean, stddev, percentile, Wilson score CI, Sharpe, PSR, max drawdown; all functions unit-tested with known inputs and expected outputs.
**Addresses:** TS-4 (Wilson score), TS-6 (Sharpe), TS-7 (drawdown), DIFF-1 (PSR) — statistical foundations
**Avoids:** Pitfall 1 (invalid annualization), Pitfall 3 (misleading CIs) — both are correctness decisions that must be made here

### Phase 2: Data Loading Layer
**Rationale:** Date-range file enumeration and tolerant JSONL deserialization are used by both CLIs identically. Building this as a shared, tested layer before the binaries ensures the streaming pattern is correct (Pitfall 5: large files) and that all position states are loaded (Pitfall 2: survivorship bias). This is the highest-risk performance decision in the project — get it wrong here and fixing it requires touching both binaries.
**Delivers:** `src/analysis/io.rs` with `files_in_range()`, `parse_jsonl<T>()`, and the position-state reporting (total/settled/open/timed-out counters); tested with sample JSONL fixture files.
**Addresses:** TS-8 (date filtering) — foundation for all analysis
**Avoids:** Pitfall 2 (survivorship bias from settled-only loading), Pitfall 5 (memory exhaustion from bulk loading), Pitfall 7 (canonical timestamp selection — `timestamp_ms` as bucketing key)

### Phase 3: Spread Analytics Computation and CLI
**Rationale:** The spread analytics CLI (`spread-analytics`) is the simpler of the two binaries. SpreadResult deserialization is simpler (no settlement correlation), the metrics are straightforward (distribution stats, hourly buckets, venue-pair breakdown), and there is no go/no-go pressure on correctness. Building it first validates the full pipeline (load -> compute -> output) before tackling the higher-stakes signal scoring metrics.
**Delivers:** `src/analysis/spread_analytics.rs` with time bucketing, venue-pair slicing, spread distribution; `src/bin/spread_analytics.rs` CLI with `--format table/json/csv`, `--from`, `--to`, `--event`, `--venue-pair`, `--threshold-breakdown` flags; comfy-table output; integration tested against actual `spread_logs/*.jsonl` data.
**Addresses:** TS-1 (spread distribution), TS-2 (hourly buckets), TS-3 (venue-pair breakdown), TS-9 (terminal output), DIFF-5 (JSON output)
**Avoids:** Pitfall 9 (output verbosity — tiered summary/verbose/json), Pitfall 10 (venue-pair type mixing — always separated), Pitfall 12 (Decimal display — Decimal::normalize() + fixed dp per field type)

### Phase 4: Signal Scoring Computation and CLI
**Rationale:** Signal scoring is the go/no-go decision tool — the metrics here determine whether v2 proceeds. Building after spread analytics means the output infrastructure, JSONL loading patterns, and stats module are all proven. This phase contains the most domain-specific complexity: PSR, per-trade Sharpe without annualization, Wilson score CIs, cost-adjusted edge significance, settlement correlation, and threshold effectiveness analysis.
**Delivers:** `src/analysis/signal_scoring.rs` with hit rate, Sharpe, PSR, max drawdown, cost-adjusted edge with t-test, threshold effectiveness (excluding cold-start observations, chronological train/test split); `src/bin/signal_scoring.rs` CLI with `--settlements`, `--confidence-level`, `--format`, `--venue-pair` flags; integration tested against real `signal_logs/` soak data; DIFF-3 (per-event breakdown) included.
**Addresses:** TS-4 (hit rate CI), TS-5 (cost-adjusted edge), TS-6 (Sharpe), TS-7 (drawdown), DIFF-1 (PSR), DIFF-2 (threshold effectiveness, basic), DIFF-3 (per-event)
**Avoids:** Pitfall 1 (Sharpe annualization), Pitfall 2 (all position states), Pitfall 3 (small-sample CIs), Pitfall 4 (look-ahead bias in threshold analysis), Pitfall 6 (drawdown edge cases), Pitfall 8 (cost model drift), Pitfall 10 (venue-pair type mixing)

### Phase 5: Verification and Milestone Completion
**Rationale:** End-to-end verification against actual soak test JSONL data with hand-calculated expected values for at least one known data subset. This catches implementation bugs that unit tests miss (e.g., off-by-one in hourly bucketing, incorrect UTC handling at midnight boundaries, settlement correlation mismatches).
**Delivers:** Both CLIs verified against real data; edge cases fixed (empty date ranges, all-filtered signals, zero settled positions, midnight boundary records); documented baseline metrics from soak test data for v2 planning.
**Avoids:** Pitfall 7 (timestamp boundary issues discovered here), Pitfall 11 (DualTimestamp elapsed() misuse caught in review)

### Phase Ordering Rationale

- Stats module before computation modules because all computation depends on it and pure-function unit tests here are the cheapest way to validate statistical formulas.
- Data loading layer before both CLIs because the streaming-vs-bulk-loading decision is architectural and affects both; deciding it once in a shared module prevents divergence.
- Spread analytics before signal scoring because it is the lower-stakes CLI with simpler metrics — it validates the full pipeline (load -> bucket -> aggregate -> format) without the go/no-go pressure that signal scoring carries.
- Verification phase is explicit and last because end-to-end tests against real JSONL data will find edge cases that unit tests cannot.

### Research Flags

Phases with clear, well-documented patterns — research-phase is not needed:
- **Phase 1 (stats module):** Wilson score, PSR, and Sharpe formulas are mathematically well-specified in PITFALLS.md with exact Rust code. comfy-table and csv crate APIs are straightforward.
- **Phase 2 (data loading):** `BufReader::lines()` + `serde_json::from_str` streaming pattern is standard Rust. JSONL filename convention is already established in the codebase.
- **Phase 3 (spread analytics):** All data types, field names, and serde annotations are verified from direct source analysis. chrono bucketing by hour is trivial.
- **Phase 5 (verification):** Standard integration testing against known data.

May benefit from targeted investigation before planning:
- **Phase 4 (signal scoring):** Settlement correlation (matching signal_logs to settlement_logs by event_id) is underspecified — the exact join key and handling of multi-leg partial settlements needs a 30-minute inspection of actual `state/checkpoint.json` schema and real settlement log structure before the computation module is designed.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Verified from Cargo.toml, docs.rs, and direct source analysis. Two new crates have no version conflicts. All existing crates confirmed sufficient for their v1.4 roles. |
| Features | HIGH | Statistical methods are established domain knowledge with well-known formulas. Feature boundaries are clear from go/no-go requirements. Anti-features are well-reasoned. |
| Architecture | HIGH | Based on direct source analysis of 35,580 LOC codebase. All serde types verified to derive Deserialize. JSONL schemas confirmed from live soak data. Streaming pattern is standard Rust. |
| Pitfalls | HIGH (statistical methodology), MEDIUM (scaling under full soak volume) | Sharpe/CI/survivorship pitfalls validated against quantitative finance literature. Spread log volume estimate (725 MB/day) is extrapolated from architecture — actual volume depends on tick rate and active events. |

**Overall confidence:** HIGH

### Gaps to Address

- **Settlement correlation join logic:** How signal_log entries correlate to settlement_log entries needs inspection of actual `state/settlements/*.jsonl` file structure before Phase 4 implementation. The join field (likely `event_id` + `direction`) should be confirmed. 30-minute investigation, not a research gap.

- **Actual spread log volume:** The 725 MB/day estimate is theoretical. Actual volume depends on active event count and tick rate during the soak test. The streaming architecture handles any volume, but the real number informs whether a pre-aggregated daily summary optimization is needed.

- **DualTimestamp deserialization and tokio in CLI binaries:** `DualTimestamp::deserialize` calls `tokio::time::Instant::now()`, which may pull a minimal tokio dependency into analysis binaries even without `#[tokio::main]`. Confirm whether this is acceptable or whether `DualTimestamp` deserialization needs restructuring before Phase 4.

---

## Sources

### Primary (HIGH confidence)
- Direct source analysis: `src/spread/patterns.rs` (SpreadResult, 583 lines), `src/signal/types.rs` (ArbSignal, 318 lines), `src/paper_trade/analyzer.rs` (AccumulatorBucket, FilteredSignalTracker), `Cargo.toml`, `src/main.rs` (CLI structure, 40,839 lines)
- Direct inspection: `signal_logs/*.jsonl` (6 days live soak data), `src/config/system.rs`, `src/spread/config.rs`, `src/signal/config.rs`
- [comfy-table docs.rs](https://docs.rs/comfy-table/latest/comfy_table/) — API reference, builder pattern, version 7.2.2
- [csv crate docs.rs](https://docs.rs/csv/latest/csv/) — serde integration, Writer/Reader API, version 1.4.0
- [statrs::distribution::Normal](https://docs.rs/statrs/latest/statrs/distribution/struct.Normal.html) — inverse_cdf confirmed in 0.18

### Secondary (MEDIUM confidence)
- [Binomial proportion CI — Wilson score](https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval) — formula and comparison with Wald
- [Sharpe Ratio for Algorithmic Trading](https://www.quantstart.com/articles/Sharpe-Ratio-for-Algorithmic-Trading-Performance-Measurement/) — annualization methodology and interpretation thresholds
- [Probabilistic Sharpe Ratio — QuantConnect](https://www.quantconnect.com/research/17112/probabilistic-sharpe-ratio/) — PSR formula and implementation
- [Two Sigma Sharpe CI Research Paper](https://www.twosigma.com/wp-content/uploads/sharpe-tr-1.pdf) — confidence interval estimation for Sharpe ratio
- [Survivorship bias in backtesting](https://www.quantifiedstrategies.com/survivorship-bias-in-backtesting/) — methodology for avoiding settled-only analysis
- [Maximum drawdown — LuxAlgo](https://www.luxalgo.com/blog/maximum-drawdown-metric-calculation-and-use-cases/) — edge cases and variants
- [McInish & Wood — Intraday bid/ask spread patterns](https://digitalcommons.memphis.edu/facpubs/11507/) — academic evidence for time-of-day bucket analysis value

### Tertiary (LOW confidence)
- [Streaming JSON in Rust — Rust Forum](https://users.rust-lang.org/t/reading-json-sequentially/57708) — confirms BufReader pattern for large JSONL; actual performance depends on hardware

---
*Research completed: 2026-02-28*
*Ready for roadmap: yes*
