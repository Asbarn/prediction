---
phase: 17-signal-analysis-tooling
plan: 01
subsystem: analysis
tags: [signal-analysis, accumulators, hit-rate, convergence, prometheus, settlement]

# Dependency graph
requires:
  - phase: 16-settlement-outcome-tracking
    provides: SettledLeg, PaperPosition settlement lifecycle, settlement P&L computation
provides:
  - AccumulatorKey and AccumulatorBucket for keyed lifetime statistics
  - SignalAnalyzer with record_settlement(), Prometheus gauge emission, checkpoint export/import
  - AnalysisSettlementRecord enriched JSONL schema
  - ThresholdStatus on SpreadResult (PassedBoth/PassedStaticOnly/Filtered)
  - venue_pair_label() on SpreadPattern for canonical venue pair keys
  - inter_leg_gap_ms, stale_fill, exchange timestamps on PaperPosition
  - AnalysisConfig with enabled flag and max_leg_fill_gap_ms
affects: [17-02, 17-03, paper-trade-tracker, settlement-monitor, checkpoint-state]

# Tech tracking
tech-stack:
  added: []
  patterns: [keyed-accumulator-pattern, enriched-settlement-record, threshold-status-propagation]

key-files:
  created:
    - src/paper_trade/analyzer.rs
  modified:
    - src/spread/patterns.rs
    - src/spread/engine.rs
    - src/paper_trade/position.rs
    - src/paper_trade/mod.rs
    - src/config/system.rs
    - src/config/mod.rs
    - config/config.toml
    - src/signal/types.rs

key-decisions:
  - "Hash derive added to ThresholdStatus for AccumulatorKey HashMap usage"
  - "ThresholdStatus computed from static_floor comparison in SpreadEngine (PassedBoth vs PassedStaticOnly vs Filtered)"
  - "inter_leg_gap_ms computed at PaperPosition creation from exchange timestamps"
  - "False positive rate defined as gross_hit minus net_hit (fees ate the edge)"

patterns-established:
  - "Keyed accumulator pattern: AccumulatorKey -> AccumulatorBucket with safe_rate() division guard"
  - "Enriched settlement record: combine per-position data with running accumulator metrics"
  - "ThresholdStatus propagation: SpreadEngine -> SpreadResult -> PaperPosition -> SignalAnalyzer"

requirements-completed: [ANLZ-01, ANLZ-02, ANLZ-03, ANLZ-04, ANLZ-05]

# Metrics
duration: 12min
completed: 2026-02-26
---

# Phase 17 Plan 01: Signal Analysis Core Types Summary

**AccumulatorKey/Bucket keyed lifetime statistics with SignalAnalyzer settlement recording, ThresholdStatus propagation from SpreadEngine to PaperPosition, and AnalysisConfig with stale fill detection**

## Performance

- **Duration:** 12 min
- **Started:** 2026-02-26T00:38:52Z
- **Completed:** 2026-02-26T00:50:33Z
- **Tasks:** 2
- **Files modified:** 15

## Accomplishments
- ThresholdStatus now propagated end-to-end: SpreadEngine computes it, SpreadResult carries it, PaperPosition stores it, SignalAnalyzer accumulates by it
- SignalAnalyzer with keyed accumulators tracks hit rate, edge, convergence, false positive rate, stale fill count across all settlement dimensions
- AnalysisSettlementRecord provides enriched JSONL schema combining per-position data with running metrics
- Prometheus gauge emission for 7 signal analysis metrics with venue_pair/event_id/threshold_status labels
- 13 comprehensive unit tests covering all accumulator, rate computation, serialization, and edge cases

## Task Commits

Each task was committed atomically:

1. **Task 1: Add ThresholdStatus to SpreadResult, venue_pair_label to SpreadPattern, extend PaperPosition** - `bc2dba3` (feat)
2. **Task 2: Create SignalAnalyzer with AccumulatorKey, AccumulatorBucket, AnalysisConfig, AnalysisSettlementRecord** - `1479260` (feat)

## Files Created/Modified
- `src/paper_trade/analyzer.rs` - New: SignalAnalyzer, AccumulatorKey, AccumulatorBucket, AnalysisSettlementRecord, LifetimeSummary (461 lines)
- `src/spread/patterns.rs` - Added venue_pair_label() to SpreadPattern, threshold_status field to SpreadResult
- `src/spread/engine.rs` - ThresholdStatus computation (PassedBoth/PassedStaticOnly/Filtered) and propagation
- `src/paper_trade/position.rs` - threshold_status, inter_leg_gap_ms, stale_fill, exchange timestamps on PaperPosition; mark_stale_fill() method
- `src/paper_trade/mod.rs` - Added pub mod analyzer
- `src/config/system.rs` - AnalysisConfig struct with enabled and max_leg_fill_gap_ms fields
- `src/config/mod.rs` - Re-export AnalysisConfig
- `config/config.toml` - Commented [analysis] section with defaults
- `src/signal/types.rs` - Added Hash derive to ThresholdStatus
- `src/paper_trade/tracker.rs` - Updated PaperPosition construction with new fields
- `src/paper_trade/aggregator.rs` - Updated test SpreadResult construction
- `src/spread/logger.rs` - Updated test SpreadResult construction
- `src/persistence/checkpoint.rs` - Updated test SpreadResult construction
- `src/settlement/monitor.rs` - Updated test SpreadResult construction
- `tests/schema_golden_test.rs` - Updated integration test SpreadResult construction

## Decisions Made
- Added Hash derive to ThresholdStatus to enable use as HashMap key in AccumulatorKey (required for keyed accumulator pattern)
- ThresholdStatus computed in SpreadEngine by comparing net_spread against threshold_value and static_floor from components
- False positive rate defined as (gross_hits - net_hits) / total_settled, capturing positions where fees ate the edge
- inter_leg_gap_ms computed at position creation time from exchange timestamps rather than at fill time

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added Hash derive to ThresholdStatus**
- **Found during:** Task 1 (planning ahead for Task 2's AccumulatorKey which needs Hash)
- **Issue:** ThresholdStatus only had Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize -- Hash needed for HashMap key
- **Fix:** Added Hash to derive list
- **Files modified:** src/signal/types.rs
- **Verification:** cargo check passes, AccumulatorKey works as HashMap key in tests
- **Committed in:** bc2dba3 (Task 1 commit)

**2. [Rule 3 - Blocking] Updated PaperPosition direct construction in tracker.rs**
- **Found during:** Task 1 (cargo check revealed missing fields)
- **Issue:** tracker.rs line 969 constructs PaperPosition via struct literal, missing new fields
- **Fix:** Added threshold_status: None, inter_leg_gap_ms: None, stale_fill: false, poly/kalshi_exchange_ts: None
- **Files modified:** src/paper_trade/tracker.rs
- **Verification:** cargo check passes
- **Committed in:** bc2dba3 (Task 1 commit)

**3. [Rule 3 - Blocking] Updated integration test SpreadResult construction**
- **Found during:** Task 2 (full cargo test revealed missing field)
- **Issue:** tests/schema_golden_test.rs constructs SpreadResult without threshold_status
- **Fix:** Added threshold_status: None to test construction
- **Files modified:** tests/schema_golden_test.rs
- **Verification:** cargo test full suite passes (504 + 22 + 3 tests)
- **Committed in:** 1479260 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (3 blocking issues)
**Impact on plan:** All auto-fixes were necessary for compilation. No scope creep.

## Issues Encountered
None - plan executed smoothly after fixing compilation issues from new struct fields.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SignalAnalyzer is ready to be wired into the live settlement pipeline (Plan 02)
- All domain types (AccumulatorKey, AccumulatorBucket, AnalysisSettlementRecord) are defined and tested
- AnalysisConfig is on SystemConfig with serde(default) for backward compatibility
- Prometheus gauge emission logic is implemented and ready for integration
- Checkpoint export/import methods are ready for state persistence integration (Plan 03)

## Self-Check: PASSED

- All 8 key source files verified present
- Both task commits verified (bc2dba3, 1479260)
- analyzer.rs: 774 lines (min 150 required)
- venue_pair_label in patterns.rs: confirmed
- threshold_status in SpreadResult and PaperPosition: confirmed
- AnalysisConfig in SystemConfig: confirmed
- Full test suite: 504 lib + 22 integration + 3 doc-tests = 529 total, 0 failures

---
*Phase: 17-signal-analysis-tooling*
*Plan: 01*
*Completed: 2026-02-26*
