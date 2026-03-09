---
phase: 40-polymarket-ws-diagnosis-watchdog
plan: 02
subsystem: feed
tags: [websocket, polymarket, watchdog, tokio, prometheus, reconnection]

# Dependency graph
requires:
  - phase: 40-polymarket-ws-diagnosis-watchdog
    provides: "data_timeout_secs config field in PolymarketConfig"
provides:
  - "Data inactivity watchdog in Polymarket supervisor forwarding loop"
  - "feed_data_timeout_total Prometheus counter for silent freeze detection"
  - "Automatic reconnect on data inactivity with preserved backoff"
affects: [polymarket-feed, monitoring, alerting]

# Tech tracking
tech-stack:
  added: []
  patterns: [tokio::time::timeout wrapping channel recv for data inactivity detection]

key-files:
  created: []
  modified:
    - src/feed/polymarket/supervisor.rs
    - src/config/validation.rs
    - tests/pipeline_test.rs

key-decisions:
  - "Timeout wraps only raw_rx.recv(), not entire select! -- cancellation and subscription arms stay responsive"
  - "Backoff NOT reset on timeout -- silent freeze is a failure, backoff grows appropriately"

patterns-established:
  - "Data inactivity watchdog via tokio::time::timeout on channel recv in supervisor forwarding loop"

requirements-completed: [POLY-02, POLY-03]

# Metrics
duration: 3min
completed: 2026-03-09
---

# Phase 40 Plan 02: Data Inactivity Watchdog Summary

**tokio::time::timeout watchdog on Polymarket supervisor forwarding loop detecting silent freezes with Prometheus counter and automatic reconnect**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-09T12:24:33Z
- **Completed:** 2026-03-09T12:27:21Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments
- Polymarket supervisor detects data inactivity (no messages for data_timeout_secs) and forces reconnect
- Prometheus counter feed_data_timeout_total incremented on each data inactivity timeout
- VenueHealth marked unavailable with reason "data inactivity timeout" on silent freeze detection
- Reconnection after data timeout follows existing backoff pattern (backoff NOT reset on timeout)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add data inactivity timeout to supervisor forwarding loop** - `146e83f` (feat)

## Files Created/Modified
- `src/feed/polymarket/supervisor.rs` - Added tokio::time::timeout wrapping raw_rx.recv() with data_timeout_secs, Err arm emits metric and marks health unavailable
- `src/config/validation.rs` - Added missing data_timeout_secs field in test PolymarketConfig initializer
- `tests/pipeline_test.rs` - Added missing data_timeout_secs field in two test PolymarketConfig initializers

## Decisions Made
- Timeout wraps only `raw_rx.recv()`, not entire `select!` block -- cancellation and subscription-change arms remain responsive without delay
- Backoff is NOT reset on data inactivity timeout -- silent freeze is a failure condition, so backoff should grow if freezes recur

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added missing data_timeout_secs field to test PolymarketConfig initializers**
- **Found during:** Task 1 (verification - cargo test)
- **Issue:** The data_timeout_secs field (added in plan 40-01) was missing from PolymarketConfig struct literals in validation.rs and pipeline_test.rs, causing compilation failure in tests
- **Fix:** Added `data_timeout_secs: 120` to all three test initializers
- **Files modified:** src/config/validation.rs, tests/pipeline_test.rs
- **Verification:** cargo test passes with all 22 lib tests + 6 integration tests + 3 doc-tests
- **Committed in:** 146e83f (part of task commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Auto-fix necessary for test compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Polymarket WS diagnosis and watchdog phase complete
- Silent freeze detection (GitHub #292) is now handled automatically
- Ready for Phase 41 (CrossAssetEngine venue fix)

---
*Phase: 40-polymarket-ws-diagnosis-watchdog*
*Completed: 2026-03-09*
