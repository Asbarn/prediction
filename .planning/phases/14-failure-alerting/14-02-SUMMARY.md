---
phase: 14-failure-alerting
plan: 02
subsystem: alerting
tags: [alerts, monitor, sweep-loop, pipeline-liveness, prometheus, tracing]

# Dependency graph
requires:
  - phase: 14-failure-alerting
    provides: "AlertCondition/AlertConfig/PipelineLiveness types from Plan 01"
  - phase: 09-health-endpoint
    provides: "VenueHealth per-venue atomic health tracker"
provides:
  - "AlertMonitor periodic task detecting feed silence, partial coverage, signal gap, stage liveness"
  - "SpreadEngine records last_spread_computed_at on each computation"
  - "CrossAssetEngine records last_signal_evaluated_at on each evaluation"
  - "Pipeline wiring: AlertMonitor spawned as tokio task in main.rs"
affects: [15-backtester, 16-settlement, 17-paper-trading]

# Tech tracking
tech-stack:
  added: []
  patterns: ["AlertMonitor sweep loop with collect-then-process-then-cleanup pattern", "Optional builder with_liveness() for pipeline stage liveness tracking"]

key-files:
  created:
    - src/alert/monitor.rs
  modified:
    - src/alert/mod.rs
    - src/alert/types.rs
    - src/alert/liveness.rs
    - src/feed/health.rs
    - src/spread/engine.rs
    - src/signal/engine.rs
    - src/main.rs

key-decisions:
  - "AlertMonitor collects all conditions into Vec before processing, enabling clean cleanup of resolved alerts"
  - "Liveness recording placed after pattern loop (spread) and direction loop (signal) to capture full evaluation cycles"
  - "VenueHealth.last_message_at and PipelineLiveness fields promoted to pub(crate) for test access"
  - "Startup grace period: signal gap alert suppressed for first signal_gap_threshold_secs of runtime"

patterns-established:
  - "AlertMonitor evaluate_all: collect conditions -> fire/update alerts -> cleanup resolved -> update gauge"
  - "Optional with_liveness() builder pattern on pipeline engines for opt-in liveness tracking"

requirements-completed: [ALRT-01, ALRT-02, ALRT-03, ALRT-04, ALRT-05, ALRT-06]

# Metrics
duration: 14min
completed: 2026-02-24
---

# Phase 14 Plan 02: AlertMonitor Sweep Loop Summary

**AlertMonitor periodic task with feed silence, partial coverage, signal gap, and stage liveness detection, cooldown-based dedup, and full pipeline wiring**

## Performance

- **Duration:** 14 min
- **Started:** 2026-02-24T18:31:21Z
- **Completed:** 2026-02-24T18:45:18Z
- **Tasks:** 3
- **Files modified:** 8

## Accomplishments
- AlertMonitor sweep loop detecting all 4 alert conditions with Prometheus gauges and structured tracing::warn!
- Cooldown-based dedup preventing log spam, with automatic cleanup of resolved alerts
- SpreadEngine and CrossAssetEngine recording liveness timestamps on each computation/evaluation cycle
- Full pipeline wiring: AlertMonitor spawned as tokio task with shared VenueHealth and PipelineLiveness
- 10 new unit tests covering feed silence, partial coverage, signal gap, stage liveness, cooldown, and cleanup
- 402 total tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement AlertMonitor periodic task** - `fdc8e4e` (feat)
2. **Task 2: Wire PipelineLiveness into SpreadEngine and CrossAssetEngine** - `f13c344` (feat)
3. **Task 3: Wire AlertMonitor into main.rs pipeline** - `58039ec` (feat)

## Files Created/Modified
- `src/alert/monitor.rs` - AlertMonitor with sweep loop, all 4 alert checks, fire/cooldown/cleanup logic, 10 tests
- `src/alert/mod.rs` - Added `pub mod monitor` and `pub use monitor::AlertMonitor`
- `src/alert/types.rs` - Added `last_warned_at` field to ActiveAlert for cooldown tracking
- `src/alert/liveness.rs` - Promoted atomic fields to `pub(crate)` for test access
- `src/feed/health.rs` - Promoted `last_message_at` to `pub(crate)` for test access
- `src/spread/engine.rs` - Added `liveness` field, `with_liveness()` builder, `record_spread()` call
- `src/signal/engine.rs` - Added `liveness` field, `with_liveness()` builder, `record_signal_eval()` call
- `src/main.rs` - PipelineLiveness creation, AlertMonitor spawning, liveness wiring to both engines

## Decisions Made
- AlertMonitor collects all conditions into a Vec before processing, enabling clean separation of fire/update vs cleanup phases
- Liveness recording at end of computation loop (not per-pattern), capturing that a full spread/signal evaluation cycle completed
- VenueHealth.last_message_at promoted to pub(crate) rather than adding a setter method, since it's only needed for tests within the crate
- Startup grace period for signal gap: avoids false alarms during initial pipeline warmup when signal eval hasn't started yet

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed borrow checker conflict in fire_alert**
- **Found during:** Task 1 (AlertMonitor implementation)
- **Issue:** Calling `self.emit_warn()` while holding a mutable borrow on `self.active_alerts` via `get_mut()` violates Rust borrow rules
- **Fix:** Extracted `count` into a local variable and updated `last_warned_at` before calling `emit_warn()`
- **Files modified:** src/alert/monitor.rs
- **Verification:** cargo check compiles cleanly
- **Committed in:** fdc8e4e (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Standard Rust borrow checker accommodation. No scope change.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Complete alerting system operational: types, config, liveness, and monitor all wired up
- Phase 15 (backtester) can proceed independently
- Phase 16 (settlement) can wire settlement_check liveness recording into its stage
- Phase 17 (paper trading enhancements) benefits from alert monitoring during development

## Self-Check: PASSED

All 8 modified/created files verified on disk. All 3 task commits verified in git log.

---
*Phase: 14-failure-alerting*
*Completed: 2026-02-24*
