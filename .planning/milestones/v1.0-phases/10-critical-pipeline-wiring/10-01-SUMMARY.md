---
phase: 10-critical-pipeline-wiring
plan: 01
subsystem: pipeline
tags: [event-registry, arb-signal, config-reload, forward-snapshots, prometheus]

# Dependency graph
requires:
  - phase: 05-event-mapping
    provides: EventRegistry with lookup_by_instrument
  - phase: 08-cross-asset-signal-generation
    provides: ArbSignal type and CrossAssetEngine output channel
  - phase: 01-foundation
    provides: ConfigReloader watch channel
provides:
  - event_id annotation on MarketSnapshot in forward_snapshots (live + replay)
  - ArbSignal consumer task with INFO logging and Prometheus metering
  - Config hot-reload subscriber refreshing EventRegistry on TOML changes
affects: [paper-trade, signal-execution, monitoring]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "EventRegistry read-lock in forward_snapshots for event_id annotation"
    - "ArbSignal consumer with tokio::select biased + CancellationToken"
    - "Config watch subscriber using borrow_and_update for changed() loop"

key-files:
  created: []
  modified:
    - src/feed/pipeline.rs
    - src/replay/mod.rs
    - src/main.rs
    - tests/pipeline_test.rs

key-decisions:
  - "borrow_and_update() instead of borrow().clone() in config watch to mark seen"
  - "Config hot-reload subscriber only spawned in live mode (replay must be deterministic)"
  - "ArbSignal consumer uses metrics::counter! with direction label for Prometheus"

patterns-established:
  - "forward_snapshots annotates event_id via registry lookup before fan-in send"
  - "Orphaned channel receivers wired with consumer tasks following shutdown token pattern"

requirements-completed: [OBSV-04, SGNL-05, OBSV-01]

# Metrics
duration: 6min
completed: 2026-02-24
---

# Phase 10 Plan 01: Critical Pipeline Wiring Summary

**Wired three orphaned channels: event_id annotation on MarketSnapshot via EventRegistry, ArbSignal consumer with INFO logging + Prometheus counter, and config hot-reload subscriber refreshing EventRegistry on TOML changes**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-24T08:51:09Z
- **Completed:** 2026-02-24T08:57:33Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- MarketSnapshot.event_id is now populated in forward_snapshots via EventRegistry lookup, enabling PaperTradeTracker to correlate snapshots with mapped events
- ArbSignal outputs from CrossAssetEngine are consumed, logged at INFO level (event_id, direction, net_edge, confidence, signal_id), and counted via Prometheus `arb_signals_consumed_total` counter
- Config hot-reload now propagates TOML changes to EventRegistry in live mode, with deterministic replay mode exempted
- Both live and replay pipelines thread EventRegistry through for event_id annotation

## Task Commits

Each task was committed atomically:

1. **Task 1: Annotate event_id on MarketSnapshot in forward_snapshots** - `ed628a5` (feat)
2. **Task 2: Wire ArbSignal consumer and config hot-reload subscriber** - `7b3d57d` (feat)

## Files Created/Modified
- `src/feed/pipeline.rs` - Added EventRegistry parameter to forward_snapshots, event_id annotation logic, threading through run_live_multi_venue
- `src/replay/mod.rs` - Added EventRegistry parameter to run_replay_pipeline, threading to forward_snapshots calls
- `src/main.rs` - ArbSignal consumer task, config hot-reload subscriber, renamed orphaned _arb_signal_rx and _config_rx
- `tests/pipeline_test.rs` - Updated run_replay_pipeline call sites with new event_registry parameter

## Decisions Made
- Used `borrow_and_update()` instead of `borrow().clone()` in config watch subscriber to properly mark the value as seen after each change notification
- Config hot-reload subscriber only spawned in live mode since replay must be deterministic
- ArbSignal consumer uses `metrics::counter!` with direction label for per-direction Prometheus counting

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated integration test call sites for run_replay_pipeline**
- **Found during:** Task 2 (cargo test)
- **Issue:** Two integration tests in `tests/pipeline_test.rs` called `run_replay_pipeline` with 4 arguments but the signature now requires 5 (added event_registry)
- **Fix:** Added `None` as fifth argument to both call sites
- **Files modified:** tests/pipeline_test.rs
- **Verification:** `cargo test` passes all 354 unit + 22 integration + 3 doc tests
- **Committed in:** 7b3d57d (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Auto-fix was necessary for test compilation after signature change. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three critical E2E flows are now connected: paper trade P&L, ArbSignal consumption, config hot-reload
- No orphaned channels remain (_arb_signal_rx, _config_rx, _event_registry all wired)
- System ready for end-to-end integration testing with live or replay data

## Self-Check: PASSED

All files exist, all commits verified, SUMMARY.md created.

---
*Phase: 10-critical-pipeline-wiring*
*Completed: 2026-02-24*
