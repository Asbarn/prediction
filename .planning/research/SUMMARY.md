# Project Research Summary

**Project:** v1.8 Signal Quality Validation
**Domain:** Cross-venue crypto arbitrage signal quality analysis, cost model diagnostics, and instrument matching validation
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

The v1.8 milestone is a diagnostic investigation into why all 19,844 daily arbitrage signals show approximately -19.5 net edge. Research has identified two confirmed code-level bugs that together likely explain the majority of the problem: (1) a unit mismatch where probability-space spreads (0.08 = 8%) are compared against dollar-denominated costs ($26.33), making every signal appear massively unprofitable, and (2) a Kalshi fee ceiling rounding bug where `Decimal::ceil()` rounds to the nearest integer instead of the nearest cent, overstating per-contract fees by up to 57x. Beyond these bugs, the system is analyzing the wrong instruments -- events.toml is empty, and historical signals come from deep-OTM strikes (BTC $105K when spot is $85K) where prediction market prices and options-implied probabilities measure fundamentally different things.

The recommended approach is surgical: fix the two known bugs, populate event mappings with near-the-money instruments, then build offline CLI diagnostic tools to determine whether profitable arbitrage opportunities actually exist. This is NOT an architecture overhaul -- the pipeline is sound. The work is primarily new analysis binaries following the established CLI pattern (synchronous, JSONL-reading, table/JSON output), with one new crate dependency (`linregress` for OLS regression). Zero new runtime components, zero new async channels, zero infrastructure changes.

The key risk is "cost model overfitting" -- tuning parameters until signals look profitable without external validation. Every cost parameter change must be justified by exchange documentation or on-chain data, not by what makes the analysis look good. A secondary risk is drawing statistical conclusions from autocorrelated data (19,844 signals are the same handful of pairs recomputed thousands of times, not independent observations). The go/no-go decision on whether cross-venue arb is viable depends on honest analysis after bugs are fixed, not on making numbers work.

## Key Findings

### Recommended Stack

The existing Rust stack (v1.0-v1.7, 42,732 LOC) requires exactly one new dependency: `linregress = "0.5"` for OLS regression with R-squared, t-statistics, and p-values needed for cost model sensitivity analysis. All other work uses existing dependencies (`statrs`, `rust_decimal`, `clap`, `comfy-table`, `serde`) and extends the existing `analysis::stats` module with ~65 lines of new functions (Pearson correlation, two-sample KS test, weighted mean, coefficient of variation, IQR). See [STACK.md](STACK.md) for full details.

**Core technologies (all existing, no version changes):**
- `statrs 0.18`: Statistical distributions for KS test critical values and regression significance
- `rust_decimal 1.40`: All cost model computations remain in decimal arithmetic
- `clap 4.5`: New CLI binaries follow established derive-macro pattern
- `linregress 0.5` (NEW): OLS regression for cost parameter sensitivity analysis -- lightweight (~500 LOC), shares nalgebra transitive dep with statrs

**What NOT to add:** polars/datafusion (overkill for Vec-based analysis), plotters (JSON output + external tools per PROJECT.md), SQLite/DuckDB (JSONL sufficient at scale), any ML library (problem is a math bug, not prediction accuracy).

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape, dependency graph, and root cause analysis.

**Must have (table stakes):**
- Cost model unit fix -- normalize all costs to probability space (divide dollar costs by target_notional)
- Kalshi fee ceiling rounding fix -- round to cents, not integers
- Spread logger fix -- spread_logs is empty, blocking all spread-level analysis
- Event mapping population -- events.toml is empty; need 3-5 near-the-money BTC mappings
- Signal data diagnostic CLI (cost-audit) -- decompose the -19.5 edge into components

**Should have (differentiators):**
- Instrument matching quality audit CLI -- validate paired instruments represent the same economic bet
- Polymarket book depth analyzer -- distinguish real liquidity from phantom $0.001/$0.999 books
- Near-the-money strike selector -- filter discovery to instruments with actual liquidity
- Cost model sensitivity analyzer -- parameter sweeps to find breakeven conditions
- Options fee model calibration -- verify Deribit fee formula against actual fee schedule (cap rules)

**Defer (v2+):**
- Execution engine / order placement -- fix signals before building execution
- Real-time Grafana dashboards for diagnostics -- CLI tools sufficient for batch analysis
- New venue integrations -- fix current pipeline first
- Automated parameter optimization -- premature until manual analysis confirms viable edge

### Architecture Approach

All new v1.8 functionality is offline CLI tools and config changes. The live pipeline receives only the spread logger bug fix. Three new binaries (`cost-audit`, `match-audit`, `book-depth`) follow the exact pattern of existing `spread-analytics` and `signal-scoring`: synchronous main, JSONL loading via `load_jsonl`, dual table/JSON output, clap-derived CLI arguments with date range filtering. New analysis logic lives in `src/analysis/` modules, reusing the battle-tested stats, io, and output infrastructure proven across 13 E2E golden-value tests. See [ARCHITECTURE.md](ARCHITECTURE.md) for component boundaries and data flow.

**Major components:**
1. `bin/cost-audit` -- Reads signal_logs, decomposes cost breakdown per event/component, runs parameter sensitivity
2. `bin/match-audit` -- Reads events.toml, validates strike/expiry/direction alignment across venues
3. `bin/book-depth` -- Reads signal_logs, analyzes fill ratio, book depth, and liquidity quality per instrument
4. `spread::logger` (fix) -- Diagnose and fix empty spread_logs output
5. `spread::cost_model` (fixes) -- Unit normalization and Kalshi ceiling rounding correction

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for all 8 pitfalls with recovery strategies and phase mapping.

1. **Unit mismatch in cost model (CONFIRMED BUG)** -- Probability-space spreads subtracted from dollar-denominated costs. Fix: divide all dollar costs by target_notional before comparison. This single fix changes net_edge from -11.88 to +0.04 (4% edge) on the same data.

2. **Kalshi fee ceiling rounding (CONFIRMED BUG)** -- `Decimal::ceil()` rounds $0.0175 to $1.00, not $0.02. Up to 57x cost overstatement. Fix: round to cents with `(raw * 100).ceil() / 100`.

3. **Instrument mismatch between prediction markets and options** -- Deep OTM instruments show 192x probability gaps (0.26% options-implied vs 50% Polymarket). These are not arbitrage opportunities; they are different instruments. Avoid by adding probability coherence gating (ratio < 5x) and focusing on near-the-money strikes.

4. **Prediction market liquidity illusion** -- Book depth at $0.001/$0.999 is phantom. Walk-the-book produces fill_ratio=1.0 on ghost books. Avoid by filtering depth levels outside $0.02-$0.98 and adding depth quality scoring.

5. **Statistical analysis errors from autocorrelated data** -- 19,844 daily signals are the same pairs recomputed thousands of times, not independent observations. T-tests and Sharpe ratios are meaningless without effective sample size correction. Avoid by deduplicating to unique market state changes before statistical analysis.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Critical Bug Fixes and Data Pipeline Repair

**Rationale:** Nothing downstream is valid until the two confirmed code bugs are fixed and the spread logger produces data. These are the highest-leverage changes in the entire milestone -- combined, they likely explain most of the -19.5 negative edge.
**Delivers:** Corrected cost computation (probability-space normalized), working spread logger, Kalshi fee rounding fix
**Addresses:** Cost model unit fix, Kalshi ceiling rounding fix, spread logger fix (table stakes)
**Avoids:** Pitfall 1 (instrument mismatch awareness), Pitfall 8 (Kalshi fee overstatement)
**Stack:** No new dependencies. Pure code fixes in `cost_model.rs`, `signal/engine.rs`, `spread/logger.rs`

### Phase 2: Event Mapping and Instrument Quality

**Rationale:** With bugs fixed, the system needs valid instruments to analyze. Empty events.toml means no real market data flows through the corrected pipeline. This phase populates mappings and validates they represent genuine economic equivalents.
**Delivers:** Populated events.toml with 3-5 near-the-money BTC pairs, match-audit CLI for ongoing validation, moneyness filtering in discovery
**Addresses:** Event mapping population (table stakes), instrument matching quality audit (differentiator), near-the-money strike selector (differentiator)
**Avoids:** Pitfall 1 (instrument mismatch), Pitfall 4 (survivorship bias)
**Stack:** No new dependencies. Reuses existing `config::EventsConfig`, discovery modules

### Phase 3: Diagnostic CLI Tools

**Rationale:** With correct cost math and valid instruments generating data, build the analysis tools that answer the go/no-go question: "Do profitable arbitrage opportunities exist?"
**Delivers:** cost-audit CLI, book-depth CLI, cost model sensitivity analysis
**Addresses:** Signal data diagnostic CLI (table stakes), Polymarket book depth analyzer (differentiator), cost model sensitivity analyzer (differentiator)
**Avoids:** Pitfall 2 (cost model overfitting -- tools provide evidence before tuning), Pitfall 3 (liquidity illusion -- book-depth CLI quantifies real vs phantom depth)
**Uses:** `linregress` crate (NEW), existing analysis infrastructure, new stats functions (Pearson correlation, KS test)

### Phase 4: Cost Model Tuning and Validation

**Rationale:** With diagnostic data from Phase 3, tune cost parameters based on evidence. Each parameter change must cite external data (exchange docs, on-chain data). This is where the go/no-go decision happens.
**Delivers:** Calibrated cost model parameters in config.toml, options fee model refinement, validated go/no-go assessment
**Addresses:** Options fee model calibration (differentiator), cost breakdown logging enhancement (table stakes)
**Avoids:** Pitfall 2 (overfitting -- each change requires external evidence), Pitfall 5 (prediction market premium confusion), Pitfall 6 (missing on-chain costs)
**Stack:** Config changes only. No code changes beyond what Phase 3 built.

### Phase 5: Statistical Validation and Conclusions

**Rationale:** After tuning, run rigorous statistical analysis on post-fix data. Correct for autocorrelation, apply multiple comparison corrections, use out-of-sample validation. This produces the final assessment.
**Delivers:** Statistically valid signal quality report, effective sample size analysis, Sharpe ratio with autocorrelation correction, final go/no-go recommendation
**Addresses:** Time-of-day/expiry-proximity analysis (deferred differentiator)
**Avoids:** Pitfall 7 (statistical analysis errors -- methodology fixed before drawing conclusions)

### Phase Ordering Rationale

- Phase 1 before everything: Two confirmed bugs make all current data meaningless. Fixing them is the single highest-ROI action.
- Phase 2 before Phase 3: Analysis tools need real market data flowing through the corrected pipeline. Without valid event mappings, CLI tools analyze test fixtures.
- Phase 3 before Phase 4: Diagnostic tools must exist before tuning. Tuning without measurement is the primary pitfall identified in research.
- Phase 4 before Phase 5: Statistical validation should run on the final calibrated system, not on intermediate states.
- Phases 1 and 2 are sequential (fixes must land before real data collection). Phase 3 tools can be built in parallel (cost-audit, book-depth, match-audit are independent). Phase 4 depends on Phase 3 output. Phase 5 depends on Phase 4 deployment.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2:** Needs investigation of what Polymarket Gamma API and Deribit currently offer for near-the-money BTC strikes. The available instrument universe determines what can be mapped.
- **Phase 4:** Needs Kalshi exchange fee documentation to validate the ceiling rounding fix. Needs Deribit fee schedule verification (cap rules: 0.03% of underlying OR 12.5% of option price, whichever is lower).

Phases with standard patterns (skip research-phase):
- **Phase 1:** Bug fixes are fully specified by code inspection. The unit mismatch and ceiling rounding issues are unambiguous.
- **Phase 3:** CLI tools follow the exact pattern of existing `spread-analytics` and `signal-scoring` binaries. 13 E2E golden-value tests prove the pattern.
- **Phase 5:** Standard statistical methodology (effective sample size, block bootstrap, Bonferroni correction). Well-documented in literature.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | One new dependency (linregress). Existing stack sufficient. Verified via crates.io, code inspection. |
| Features | HIGH | Root cause identified by direct code inspection. Cost model unit mismatch and Kalshi ceiling rounding confirmed in source. Feature priorities clear from dependency analysis. |
| Architecture | HIGH | New components follow proven CLI pattern with 13 E2E tests. Zero runtime changes beyond logger bug fix. All researchers agree: no architectural overhaul needed. |
| Pitfalls | HIGH | Two confirmed bugs from code inspection. Statistical methodology pitfalls grounded in standard practice. Liquidity illusion documented in Polymarket GitHub issues. |

**Overall confidence:** HIGH

All four research files converge on the same diagnosis: the system's architecture and data pipeline are sound, but two code bugs (unit mismatch, ceiling rounding) and poor instrument selection (deep OTM strikes, empty events.toml) produce garbage signals. The fix is targeted: correct the math, select better instruments, build diagnostic tools, validate rigorously.

### Gaps to Address

- **Kalshi actual fee schedule:** The ceiling rounding fix assumes Kalshi rounds to cents. This needs verification against their exchange documentation during Phase 4 planning.
- **Available near-the-money instruments:** Whether Polymarket currently lists BTC contracts at strikes near spot ($80K-$90K range) with meaningful liquidity is unknown. Phase 2 planning needs to check.
- **On-chain execution costs:** Gas costs ($2-$5 per transaction) and bridging costs are not modeled. For signal validation (v1.8), this affects go/no-go threshold but not the immediate bug fixes. Must be addressed before any capital deployment decision.
- **Prediction market premium magnitude:** The non-probability component of prediction market prices (risk premium, liquidity premium, favorite-longshot bias) is acknowledged but not quantified. Phase 4 should estimate this from cross-venue comparison (Polymarket vs Kalshi on identical events).
- **Settlement data coverage:** Signal scoring requires settled outcomes. Whether sufficient settled signals exist for statistical validation is unknown until Phase 2 generates data through the corrected pipeline.

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis: `src/signal/engine.rs` lines 396-471, `src/spread/cost_model.rs` lines 52-61, `src/spread/engine.rs`, `src/analysis/stats.rs`, `src/bin/spread_analytics.rs` -- confirmed unit mismatch and ceiling rounding bugs
- [linregress on crates.io](https://crates.io/crates/linregress) -- v0.5.4, 888K downloads, verified API
- PROJECT.md constraints -- "JSONL sufficient at current scale", "JSON output + external tools preferred", "settled data is stronger evidence than simulated backtests"
- Deribit taker fee: 0.03% (0.0003) for options -- matches public fee schedule

### Secondary (MEDIUM confidence)
- [Polymarket CLOB Introduction](https://docs.polymarket.com/developers/CLOB/introduction) -- order book architecture
- [Polymarket /book stale data issue #180](https://github.com/Polymarket/py-clob-client/issues/180) -- known stale order book data
- [Arbitrage in Prediction Markets (IMDEA)](https://arxiv.org/abs/2508.03474) -- academic analysis of prediction market arb
- Polygon gas costs: $0.01-$5.00 per transaction -- network-condition-dependent
- Kalshi fee structure: taker coefficient 0.07 with ceiling rounding -- from code, needs exchange doc verification

### Tertiary (LOW confidence)
- Prediction market pricing biases (Wolfers & Zitzewitz 2004, Manski 2006) -- bias existence confirmed, magnitude in current crypto prediction markets unquantified
- Available near-the-money Polymarket BTC contracts -- needs live API check

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*
