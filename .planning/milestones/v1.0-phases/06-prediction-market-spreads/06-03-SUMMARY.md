---
phase: 06-prediction-market-spreads
plan: 03
subsystem: spread
tags: [spread-engine, staleness-gate, threshold, jsonl-logger, tokio-select, rolling-stats, walk-the-book, metrics]

# Dependency graph
requires:
  - phase: 06-prediction-market-spreads
    provides: "SpreadConfig, cost model (polymarket_fee, kalshi_taker_fee, carry_cost), walk_the_book, RollingStats, SpreadPattern, SpreadResult"
  - phase: 05-event-mapping
    provides: "EventRegistry with lookup_by_instrument, EventMapping with venue entries"
provides:
  - "SpreadEngine consuming MarketSnapshot and producing SpreadResult signals"
  - "Staleness gate rejecting stale snapshots from both venues"
  - "4-pattern spread computation with walk-the-book fill prices and fee-adjusted costs"
  - "Dynamic threshold: max(floor, mean + k*stddev) + liquidity_penalty with cold start"
  - "SpreadLogger writing every computation as JSONL with daily file rotation"
  - "Periodic aggregate stats emission to Prometheus gauges and tracing"
affects: [06-04-PLAN, 07-options-implied-probability]

# Tech tracking
tech-stack:
  added: []
  patterns: [biased-tokio-select event loop, staleness-gate-pattern, daily-rotating-jsonl-logger, dynamic-threshold-with-cold-start]

key-files:
  created:
    - src/spread/engine.rs
    - src/spread/logger.rs
    - src/spread/threshold.rs
  modified:
    - src/spread/config.rs
    - src/spread/mod.rs

key-decisions:
  - "Staleness thresholds are per-venue: 5s for Polymarket (WebSocket), 15s for Kalshi (REST-polled)"
  - "min_samples added to ThresholdConfig (not hard-coded) for configurable cold start transition"
  - "SpreadLogger uses BufWriter with periodic flush (every 100 writes) for write performance"
  - "process_snapshot clones EventMapping to avoid holding registry read lock during computation"
  - "Signal delivery uses try_send (non-blocking) to paper trade tracker -- best effort, never blocks engine"

patterns-established:
  - "SpreadEngine pattern: stateful context + async run() consuming channel via biased select"
  - "Staleness gate: dual check (is_stale flag + timestamp age) with per-venue thresholds"
  - "JSONL daily rotation: date-stamped files in append mode, BufWriter with periodic flush"
  - "Dynamic threshold: cold start -> warm state transition based on configurable sample count"

requirements-completed: [SGNL-02, SGNL-03, SGNL-04, SGNL-07, SGNL-08]

# Metrics
duration: 9min
completed: 2026-02-23
---

# Phase 6 Plan 03: Spread Computation Engine Summary

**SpreadEngine with staleness-gated snapshot pairing, 4-pattern walk-the-book fill pricing, dynamic threshold with cold start, and JSONL computation logging**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-23T09:27:04Z
- **Completed:** 2026-02-23T09:36:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- SpreadEngine processes MarketSnapshot from fan-in channel, pairs by event ID via EventRegistry, enforces staleness gate on both legs, computes all 4 directional patterns with walk-the-book fill prices and fee-adjusted costs
- Dynamic threshold implementing max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty with cold start fallback when insufficient rolling window samples
- SpreadLogger writes every spread computation to JSONL with daily file rotation and periodic flushing
- Staleness gate checks both is_stale flag and timestamp age with per-venue thresholds (5s Polymarket, 15s Kalshi)
- Periodic aggregate statistics (mean, stddev, p50, p95) emitted to Prometheus gauges and tracing::info
- 18 new unit tests (6 engine, 3 logger, 9 threshold) bringing spread module total to 66

## Task Commits

Each task was committed atomically:

1. **Task 1: SpreadEngine with snapshot pairing, staleness gate, and 4-pattern cost computation** - `1b3af6f` (feat)
2. **Task 2: Dynamic threshold with cold start and liquidity penalty** - `e6b9822` (feat)

## Files Created/Modified
- `src/spread/engine.rs` - SpreadEngine struct, run() event loop, process_snapshot(), staleness gate, walk_both_sides(), compute_fees(), emit_aggregate_stats() (707 lines)
- `src/spread/logger.rs` - SpreadLogger with BufWriter, daily file rotation, periodic flush (211 lines)
- `src/spread/threshold.rs` - compute_threshold() returning (Decimal, ThresholdComponents) with cold start and liquidity penalty (282 lines)
- `src/spread/config.rs` - Added staleness_threshold_ms, kalshi_staleness_threshold_ms to SpreadConfig; added min_samples to ThresholdConfig
- `src/spread/mod.rs` - Added engine, logger, threshold module declarations

## Decisions Made
- Staleness thresholds are per-venue configurable: 5s default for Polymarket (WebSocket streaming, tight staleness), 15s for Kalshi (REST-polled, more permissive)
- min_samples moved to ThresholdConfig as a configurable field instead of a hard-coded constant, enabling per-deployment tuning of cold start duration
- SpreadLogger uses BufWriter with flush every 100 writes, balancing I/O performance with data durability (matching feed recording pattern)
- EventMapping is cloned after registry lookup to avoid holding the read lock during the entire computation pipeline
- Signal channel uses try_send (non-blocking) to paper trade tracker -- engine never blocks on slow downstream consumers

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added min_samples field to ThresholdConfig**
- **Found during:** Task 2 (threshold implementation)
- **Issue:** Plan referenced config.min_samples_for_dynamic but ThresholdConfig had no such field; hard-coding 30 would prevent per-deployment tuning
- **Fix:** Added min_samples: usize field to ThresholdConfig with default 30, removed hard-coded inherent impl
- **Files modified:** src/spread/config.rs, src/spread/threshold.rs
- **Verification:** All 66 spread tests pass, config deserializes correctly with defaults
- **Committed in:** e6b9822

---

**Total deviations:** 1 auto-fixed (1 missing critical)
**Impact on plan:** Making min_samples configurable improves operational flexibility. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SpreadEngine is ready for integration into main.rs (replacing the simple snapshot consumer)
- signal_tx channel output is ready for Plan 04's paper trade tracker
- All spread module components (config, cost model, book walker, patterns, rolling stats, engine, logger, threshold) are unit-tested and compose correctly
- 66 spread module tests passing

## Self-Check: PASSED

- All 5 key files verified present on disk
- Both commits (1b3af6f, e6b9822) verified in git log
- src/spread/engine.rs: 707 lines (min 150 required)
- src/spread/threshold.rs: 282 lines (min 40 required)
- src/spread/logger.rs: 211 lines (min 50 required)
- cargo build: passes with no new warnings
- cargo test --lib spread: 66/66 tests pass

---
*Phase: 06-prediction-market-spreads*
*Completed: 2026-02-23*
