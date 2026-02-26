---
phase: 17-signal-analysis-tooling
plan: 03
subsystem: analysis
tags: [filtered-signals, threshold-effectiveness, settlement-correlation, prometheus, checkpoint]

# Dependency graph
requires:
  - phase: 17-signal-analysis-tooling/02
    provides: "SignalAnalyzer accumulators, SettlementLogger, Prometheus gauges"
provides:
  - "FilteredSignalTracker with event_id correlation and threshold effectiveness counters"
  - "Filtered signal emission channel from SpreadEngine (non-PassedBoth results)"
  - "Settlement correlation for filtered signals (hypothetical hit rate)"
  - "Checkpoint v4 with filtered_signals for restart survival"
affects: [paper-trade, spread-engine, settlement]

# Tech tracking
tech-stack:
  added: []
  patterns: ["filtered signal channel (mpsc try_send best-effort)", "settlement correlation for hypothetical outcomes"]

key-files:
  created: []
  modified:
    - src/paper_trade/analyzer.rs
    - src/spread/engine.rs
    - src/paper_trade/tracker.rs
    - src/main.rs
    - src/persistence/checkpoint.rs
    - src/persistence/recovery.rs

key-decisions:
  - "Pattern-based hypothetical hit: BuyPolyYes profits from Yes, SellPolyYes profits from No, etc."
  - "FilteredSignalTracker capped at 100 entries per event_id to prevent unbounded growth"
  - "Filtered signals sent via try_send (best-effort, non-blocking) to avoid backpressure on SpreadEngine"
  - "Correlation cleanup: remove_event called after correlate to prevent stale signal accumulation"

patterns-established:
  - "Filtered signal channel: SpreadEngine -> PaperTradeTracker via mpsc(512)"
  - "Settlement correlation: hypothetical outcome derived from pattern direction + OutcomeKind"

requirements-completed: [ANLZ-05, ANLZ-06, ANLZ-07]

# Metrics
duration: 9min
completed: 2026-02-26
---

# Phase 17 Plan 03: Filtered Signal Tracking Summary

**FilteredSignalTracker with settlement correlation for threshold effectiveness analysis -- operator can compare hit rates across PassedBoth vs PassedStaticOnly vs Filtered categories to answer "did I filter out winners?"**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-26T01:08:31Z
- **Completed:** 2026-02-26T01:17:28Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- FilteredSignalTracker records non-PassedBoth signals and correlates with settlement outcomes to determine hypothetical profitability
- SpreadEngine sends filtered signals (positive spread, below threshold) on a dedicated best-effort channel to PaperTradeTracker
- Settlement correlation determines hypothetical hit rate by mapping pattern direction to outcome (e.g., BuyPolyYes profits if outcome is Yes)
- Checkpoint v4 persists filtered signal state across restarts with backward-compatible serde(default)
- Prometheus gauge `signal_analysis_filtered_hypothetical_hit_rate` provides real-time threshold effectiveness visibility

## Task Commits

Each task was committed atomically:

1. **Task 1: Create FilteredSignalTracker and filtered signal channel from SpreadEngine** - `91c86e4` (feat)
2. **Task 2: Wire filtered signal channel in main.rs and PaperTradeTracker, complete checkpoint integration** - `ef4b06e` (feat)

## Files Created/Modified
- `src/paper_trade/analyzer.rs` - FilteredSignalEvent, FilteredSignalEntry, FilteredCorrelation types; FilteredSignalTracker with record/correlate/export/import; SignalAnalyzer integration methods
- `src/spread/engine.rs` - filtered_signal_tx field, with_filtered_signal_tx builder, try_send for non-PassedBoth positive-spread results
- `src/paper_trade/tracker.rs` - filtered_signal_rx parameter in run(), select! arm for filtered signals, settlement correlation in handle_settlement, checkpoint save/restore for filtered state
- `src/main.rs` - filtered signal channel creation (mpsc 512), wired to SpreadEngine and PaperTradeTracker
- `src/persistence/checkpoint.rs` - filtered_signals field on CheckpointState, version bump 3->4, v3 backward compat test, v4 roundtrip test
- `src/persistence/recovery.rs` - Updated test to include filtered_signals field

## Decisions Made
- Pattern-based hypothetical hit determination: each SpreadPattern has a clear profit direction (BuyPolyYesSellKalshiYes profits from Yes outcome, etc.), making correlation straightforward without needing fill prices
- FilteredSignalTracker capped at 100 entries per event_id to prevent unbounded memory growth on events that never settle
- Used try_send (best-effort, non-blocking) for filtered signal channel to avoid any backpressure impact on the primary SpreadEngine signal path
- Correlation automatically cleans up signals for the event after processing to prevent stale accumulation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed missing filtered_signals field in recovery.rs test**
- **Found during:** Task 2 (full test suite)
- **Issue:** recovery.rs test constructs CheckpointState directly, missing new filtered_signals field
- **Fix:** Added `filtered_signals: HashMap::new()` to the test construction
- **Files modified:** src/persistence/recovery.rs
- **Verification:** cargo test passes
- **Committed in:** ef4b06e (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Trivial fix to existing test that needed the new field. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 17 is now complete (all 3 plans done)
- Threshold effectiveness analysis loop is fully operational: operator can compare hit rates across PassedBoth, PassedStaticOnly, and Filtered categories
- All v1.1 signal analysis tooling is deployed and persisted across restarts

---
## Self-Check: PASSED

All 7 files verified present. Both task commits (91c86e4, ef4b06e) verified in git log.

---
*Phase: 17-signal-analysis-tooling*
*Completed: 2026-02-26*
