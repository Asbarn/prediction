---
phase: 06-prediction-market-spreads
verified: 2026-02-23T11:00:00Z
status: passed
score: 18/18 must-haves verified
re_verification: false
human_verification:
  - test: "Prometheus endpoint is reachable at http://localhost:9000/metrics during live run"
    expected: "Prometheus text format output with spread_net histograms, spread_computations_total counters, and spread_rolling_mean gauges visible after first snapshot pairs arrive"
    why_human: "Requires live network feeds or mock mode to be running; can't verify endpoint reachability in static analysis"
  - test: "JSONL spread_logs/ files contain valid JSON lines during live run"
    expected: "Each line parses to SpreadResult with all 17 fields populated, threshold_components non-null, pattern one of the 4 enum values"
    why_human: "Logger defers file creation until first write; static analysis cannot verify runtime I/O"
  - test: "JSONL paper_trades/ files contain tagged events (signal, entry, mtm) during live run"
    expected: "Entry event appears on snapshot N+1 after signal on snapshot N for same event; adverse_selection field non-zero"
    why_human: "Next-tick fill model behavior requires two sequential snapshots for the same event, which needs live or mock data"
---

# Phase 6: Prediction Market Spreads Verification Report

**Phase Goal:** The system detects cross-platform prediction market arbitrage (Polymarket vs Kalshi), computes fee-adjusted net spreads, logs every computation for analysis, tracks hypothetical paper trade P&L, and exports key metrics to Prometheus -- delivering the first actionable trading signals.
**Verified:** 2026-02-23T11:00:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | Polymarket dynamic fee formula computes correctly for both exponents (1 and 2) and supports flat rate override | VERIFIED | `src/spread/cost_model.rs:15-37` implements formula; 15 unit tests cover exponent=1, exponent=2, flat override, edge cases p=0/p=1 |
| 2  | Kalshi taker fee with ceiling rounding matches `0.07 * P * (1-P)` formula | VERIFIED | `src/spread/cost_model.rs:47-60` uses `Decimal::ceil()` per contract; tests at p=0.50 verify ceiling behavior |
| 3  | Walk-the-book produces correct average fill price and reports fill ratio when depth insufficient | VERIFIED | `src/spread/book_walker.rs` 150 lines; WalkResult with `fill_ratio()` method; 5 tests including partial fill and empty depth |
| 4  | Carry cost prorates annualized rate by holding period | VERIFIED | `src/spread/cost_model.rs` carry_cost function; `notional * annualized_rate * reference_holding_days / 365` |
| 5  | Rolling statistics compute accurate mean, stddev, and percentiles over configurable time window | VERIFIED | `src/spread/rolling_stats.rs` 177 lines; 6 tests including known sequence (mean=3.0, stddev~1.58), window eviction, percentile |
| 6  | All spread parameters are configurable via TOML | VERIFIED | `src/spread/config.rs` 293 lines with `#[serde(default)]`; `config/config.toml` has `[spread]`, `[paper_trade]`, `[prometheus]` sections |
| 7  | Prometheus metrics are scrape-able at configured port | VERIFIED (automated portion) | `src/metrics_export/mod.rs` 50 lines; `setup_prometheus()` called in `main.rs:92` before task spawning; spread/latency histogram buckets configured |
| 8  | Spread-specific histogram buckets configured for probability-space values | VERIFIED | `src/metrics_export/mod.rs:36-39` sets `Matcher::Prefix("spread_")` with buckets `[0.0001, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.10, 0.20]` |
| 9  | SpreadPattern enum enumerates all 4 directional patterns with human-readable labels | VERIFIED | `src/spread/patterns.rs:22-74` defines all 4 variants; `label()` returns distinct strings; `all()` returns `[SpreadPattern; 4]` |
| 10 | SpreadResult carries all computation metadata | VERIFIED | `src/spread/patterns.rs:160-208` has 17 fields including pattern, gross/net spread, fill prices, fees, carry_cost, fill ratios, timestamps, threshold, threshold_components |
| 11 | SpreadEngine consumes MarketSnapshot from fan-in channel and computes spreads for every mapped Polymarket+Kalshi event pair | VERIFIED | `src/spread/engine.rs:68-113` async run() with biased tokio::select; `process_snapshot` checks both Polymarket and Kalshi venue entries |
| 12 | Both legs must pass staleness gate before spread computation proceeds | VERIFIED | `src/spread/engine.rs:248-317` checks `is_stale` flag AND timestamp age with per-venue thresholds (5s Polymarket, 15s Kalshi); metrics emitted on rejection |
| 13 | All 4 directional patterns computed independently with correct depth sides per pattern | VERIFIED | `src/spread/engine.rs:335-354` `walk_both_sides()` matches each pattern to correct depth sides (poly.depth_asks vs depth_bids etc.); SpreadPattern::all() iteration at line 160 |
| 14 | Every spread computation logged as JSONL with full metadata | VERIFIED | `src/spread/engine.rs:222-225` calls `self.logger.log(&result).await` on EVERY computation before threshold check; `src/spread/logger.rs` BufWriter with daily rotation |
| 15 | Dynamic threshold: `max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty` | VERIFIED | `src/spread/threshold.rs:25-78` implements exactly this formula; 9 unit tests covering cold start, warm state, full/partial/empty book |
| 16 | Cold start mode uses elevated static floor when insufficient samples | VERIFIED | `src/spread/threshold.rs:37-43` uses `static_floor * cold_start_multiplier` when `stats.count() < config.min_samples` |
| 17 | Periodic aggregate statistics emitted to Prometheus and stdout | VERIFIED | `src/spread/engine.rs:407-432` `emit_aggregate_stats()` emits tracing::info with count/mean/stddev/p50/p95 and Prometheus gauges `spread_rolling_mean`, `spread_rolling_stddev` |
| 18 | Paper trade P&L tracker enters at next-tick price and tracks MTM and daily rollups | VERIFIED | `src/paper_trade/tracker.rs:297-378` next-tick fill model; MTM updated each snapshot; `src/paper_trade/aggregator.rs:260 lines` daily rollups; 16 unit tests pass |

**Score:** 18/18 truths verified

### Required Artifacts

| Artifact | Min Lines | Actual Lines | Status | Notes |
|----------|-----------|--------------|--------|-------|
| `src/spread/config.rs` | 60 | 293 | VERIFIED | SpreadConfig, ThresholdConfig, PolymarketFeeConfig, KalshiFeeConfig, CarryConfig |
| `src/spread/cost_model.rs` | 80 | 255 | VERIFIED | polymarket_fee, kalshi_taker_fee, carry_cost, 15 unit tests |
| `src/spread/book_walker.rs` | 40 | 150 | VERIFIED | walk_the_book, WalkResult, fill_ratio(), 5 unit tests |
| `src/spread/rolling_stats.rs` | 60 | 177 | VERIFIED | RollingStats, push/mean/stddev/percentile, 6 unit tests |
| `src/metrics_export/mod.rs` | 25 | 50 | VERIFIED | setup_prometheus() with PrometheusBuilder, Matcher-configured buckets |
| `src/spread/patterns.rs` | 80 | 524 | VERIFIED | SpreadPattern, GrossSpread, compute_gross_spread, SpreadResult, ThresholdComponents |
| `src/spread/engine.rs` | 150 | 716 | VERIFIED | SpreadEngine, run(), process_snapshot(), staleness gate, 4-pattern dispatch |
| `src/spread/threshold.rs` | 40 | 282 | VERIFIED | compute_threshold(), ThresholdComponents, 9 unit tests |
| `src/spread/logger.rs` | 50 | 211 | VERIFIED | SpreadLogger with BufWriter, daily rotation, periodic flush |
| `src/paper_trade/tracker.rs` | 100 | 584 | VERIFIED | PaperTradeTracker, next-tick fill, MTM tracking, 5 unit tests |
| `src/paper_trade/position.rs` | 60 | 302 | VERIFIED | PaperPosition, PositionStatus, MtmSnapshot, 6 unit tests |
| `src/paper_trade/aggregator.rs` | 40 | 260 | VERIFIED | DailyAggregator, DailyRollup, 3 unit tests |

### Key Link Verification

| From | To | Via | Status | Evidence |
|------|----|-----|--------|---------|
| `src/spread/cost_model.rs` | `src/spread/config.rs` | PolymarketFeeConfig, KalshiFeeConfig | WIRED | Line 3: `use super::config::{CarryConfig, KalshiFeeConfig, PolymarketFeeConfig};` |
| `src/spread/book_walker.rs` | `src/types/decimal.rs` | Price, Notional types | WIRED | depth level types imported; walk_the_book operates on typed depth slices |
| `config/config.toml` | `src/spread/config.rs` | TOML `[spread]` section | WIRED | `[spread]` section present with all subsections; SpreadConfig in SystemConfig with `#[serde(default)]` |
| `src/main.rs` | `src/metrics_export/mod.rs` | setup_prometheus() before task spawning | WIRED | `main.rs:92` calls `prediction::metrics_export::setup_prometheus(prometheus_port)` before any feed/spread tasks |
| `src/spread/patterns.rs` | `src/types/snapshot.rs` | bid_probability/ask_probability | WIRED | `patterns.rs:112-115` reads `poly.bid_probability`, `poly.ask_probability`, `kalshi.bid_probability`, `kalshi.ask_probability` |
| `src/spread/engine.rs` | `src/events/registry.rs` | EventRegistry::lookup_by_instrument | WIRED | `engine.rs:125` calls `reg.lookup_by_instrument(snap.venue, ...)` |
| `src/spread/engine.rs` | `src/spread/cost_model.rs` | polymarket_fee, kalshi_taker_fee, carry_cost | WIRED | `engine.rs:20` imports all 3 functions; called at lines 175, 392, 400 |
| `src/spread/engine.rs` | `src/spread/book_walker.rs` | walk_the_book | WIRED | `engine.rs:18` imports walk_the_book; called at lines 351, 352 |
| `src/spread/engine.rs` | `src/spread/patterns.rs` | SpreadPattern::all(), compute_gross_spread | WIRED | `engine.rs:22` imports both; SpreadPattern::all() at line 160; compute_gross_spread at line 162 |
| `src/spread/engine.rs` | `src/spread/rolling_stats.rs` | RollingStats per event | WIRED | `engine.rs:23` imports RollingStats; HashMap<String, RollingStats> at line 36; inserted at line 187 |
| `src/spread/engine.rs` | `src/spread/logger.rs` | SpreadLogger::log for every computation | WIRED | `engine.rs:21` imports SpreadLogger; `self.logger.log(&result).await` at line 223, before threshold check |
| `src/paper_trade/tracker.rs` | `src/spread/patterns.rs` | SpreadResult as signal input | WIRED | `tracker.rs:24` imports SpreadResult; `signal_rx: mpsc::Receiver<SpreadResult>` at line 191 |
| `src/paper_trade/tracker.rs` | `src/types/snapshot.rs` | MarketSnapshot for next-tick fill and MTM | WIRED | `tracker.rs:25` imports MarketSnapshot; `snapshot_rx: mpsc::Receiver<MarketSnapshot>` at line 192 |
| `src/main.rs` | `src/spread/engine.rs` | SpreadEngine::run replaces simple snapshot consumer | WIRED | `main.rs:13` imports SpreadEngine; spawned at line 208 consuming snapshot_rx |
| `src/main.rs` | `src/paper_trade/tracker.rs` | PaperTradeTracker::run consuming signal channel | WIRED | `main.rs:12` imports PaperTradeTracker; spawned at line 218 consuming signal_rx and ptrade_snap_rx |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|---------|
| SGNL-02 | 06-01, 06-03 | Spread calculation adjusts for transaction fees (Polymarket dynamic fees, Kalshi 7% profit fee), slippage from depth, carry cost | SATISFIED | cost_model.rs implements Polymarket dynamic fee (exponent 1/2) and Kalshi taker fee with ceiling; walk_the_book provides slippage; carry_cost prorates annualized rate; all applied per-computation in engine.rs |
| SGNL-03 | 06-03 | Every spread calculation validates both sides are fresh (staleness gate) and rejects with logging if either exceeds threshold | SATISFIED | engine.rs:248-317 `passes_staleness_gate()` checks is_stale flag AND timestamp age with per-venue thresholds (5s Poly, 15s Kalshi); tracing::debug + metrics counter on rejection |
| SGNL-04 | 06-02, 06-03 | Cross-platform prediction market spread detection (Polymarket vs Kalshi) for 4 patterns | SATISFIED | SpreadPattern enum with all 4 variants; SpreadPattern::all() iteration in engine.rs; walk_both_sides() uses correct depth sides per pattern |
| SGNL-07 | 06-03 | Every spread computation logged to file (not just signals above threshold) for distribution analysis | SATISFIED | engine.rs:222-225 calls logger.log(&result) BEFORE threshold check; SpreadLogger with daily JSONL rotation |
| SGNL-08 | 06-03 | Periodic aggregate spread statistics (mean, stddev, percentiles) emitted to metrics and stdout | SATISFIED | engine.rs:407-432 emit_aggregate_stats() emits tracing::info with count/mean/stddev/p50/p95 and Prometheus gauges |
| OBSV-03 | 06-02 | Prometheus metrics exporter with key metrics: spread by event (histogram), signal count, fill rate proxy, feed-to-signal latency, feed health | SATISFIED | metrics_export/mod.rs installs global Prometheus recorder; spread_net histogram, spread_computations_total counter, spread_signals_total counter, spread_rolling_mean/stddev gauges all emitted |
| OBSV-04 | 06-04 | Paper trade P&L tracking: hypothetical entry/exit at signal time, per-signal P&L, daily/weekly aggregates | SATISFIED | PaperTradeTracker with next-tick fill model (not signal-time price); adverse_selection field quantifies slippage; DailyAggregator produces per-day trade count, win/loss, total/avg P&L |

**All 7 required requirement IDs accounted for and satisfied.**

### Anti-Patterns Found

No blockers or warnings found.

| File | Pattern | Severity | Notes |
|------|---------|----------|-------|
| None | -- | -- | No TODO/FIXME/placeholder comments, no empty implementations, no stub returns found in any Phase 6 file |

One pre-existing warning (`dead_code` in `src/feed/kalshi/mod.rs:43` for `staleness_threshold_ms`) is not related to Phase 6 work and was present before this phase.

### Human Verification Required

#### 1. Prometheus Endpoint Accessibility

**Test:** Start the application (`cargo run`) and execute `curl http://localhost:9000/metrics`
**Expected:** Prometheus text format response with at minimum `spread_net` histogram buckets visible (even if 0 count), confirming the global recorder is active and the HTTP listener bound successfully
**Why human:** Requires the process to be running with the port available; static analysis confirms the code path but not runtime binding

#### 2. JSONL Spread Log File Content

**Test:** Run in mock mode and inspect `spread_logs/YYYY-MM-DD.jsonl`
**Expected:** Each line is valid JSON with all SpreadResult fields present: `event_id`, `pattern` (one of 4 values), `gross_spread`, `net_spread`, `buy_fill_price`, `sell_fill_price`, `buy_fee`, `sell_fee`, `carry_cost`, `total_cost`, `buy_fill_ratio`, `sell_fill_ratio`, `target_notional`, `timestamp_ms`, `threshold`, `threshold_components` (non-null with all 7 sub-fields)
**Why human:** Logger defers file creation to first write at runtime; the file will not exist until at least one matched snapshot pair passes the staleness gate

#### 3. Next-Tick Fill Adverse Selection

**Test:** In mock mode with two sequential snapshots for the same event, observe paper_trades JSONL
**Expected:** Signal event appears on tick N, Entry event appears on tick N+1 with entry prices from the N+1 snapshot (not N), `adverse_selection` field is non-zero if prices changed between ticks
**Why human:** Requires controlled sequential snapshot delivery to verify the next-tick fill model; logic is verified by unit tests but the integration scenario needs observation

### Gaps Summary

No gaps found. All 18 observable truths are verified, all 12 required artifacts pass all 3 levels (exists, substantive, wired), all 15 key links are confirmed wired, and all 7 requirement IDs are satisfied with implementation evidence.

The phase delivered exactly what was planned across 4 sub-plans:
- **Plan 01:** SpreadConfig, Polymarket/Kalshi fee calculators, walk-the-book, rolling statistics (66 tests by end of plan 03)
- **Plan 02:** Prometheus global recorder with probability-space histogram buckets, 4-pattern SpreadPattern enum, SpreadResult with all metadata
- **Plan 03:** SpreadEngine with staleness gate, 4-pattern dispatch with correct depth sides, JSONL logger, dynamic threshold with cold start
- **Plan 04:** PaperTradeTracker with next-tick fill, MTM history, DailyAggregator, full pipeline wired in main.rs

Total: 253 lib tests + 16 integration + 22 smoke = 297 tests passing. Build succeeds with no Phase 6 warnings.

---

_Verified: 2026-02-23T11:00:00Z_
_Verifier: Claude (gsd-verifier)_
