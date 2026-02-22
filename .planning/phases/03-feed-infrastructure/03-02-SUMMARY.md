---
phase: 03-feed-infrastructure
plan: 02
subsystem: feed
tags: [staleness, metrics, latency, recording, flush, deribit]

# Dependency graph
requires:
  - phase: 02-deribit-feed
    provides: "DeribitProcessor normalization pipeline, InstrumentBook, JsonlWriter, RecordingService"
  - phase: 03-feed-infrastructure (plan 01)
    provides: "DeribitConfig.staleness_threshold_ms, ReconnectConfig, heartbeat protocol"
provides:
  - "Staleness gate on MarketSnapshot based on exchange_timestamp age (RELY-03)"
  - "Feed latency metrics (histogram, gauge, counter) via metrics crate"
  - "write_line_no_flush method on JsonlWriter for periodic flush"
  - "Periodic 1-second flush in recording_task for throughput"
affects: [06-monitoring, 03-feed-infrastructure]

# Tech tracking
tech-stack:
  added: [metrics 0.24]
  patterns: [staleness-gating, periodic-flush, metrics-facade-noop]

key-files:
  created: []
  modified:
    - src/feed/deribit/normalize.rs
    - src/feed/recording/writer.rs
    - src/feed/recording/mod.rs
    - Cargo.toml

key-decisions:
  - "Staleness gate uses OR logic: is_stale = book.is_stale || exchange_data_stale"
  - "metrics crate macros are zero-cost no-ops without a recorder (Phase 6 adds Prometheus exporter)"
  - "Processor async tests use u64::MAX threshold since JSON payloads have hardcoded 2023 timestamps"
  - "biased select in recording_task: cancel > recv > flush tick"

patterns-established:
  - "Staleness gating: check exchange_timestamp age before publishing snapshots"
  - "Metrics facade: instrument code now, install recorder later (Phase 6)"
  - "Periodic flush: write_line_no_flush + interval-based flush for throughput"

# Metrics
duration: 14min
completed: 2026-02-22
---

# Phase 3 Plan 2: Staleness/Latency/Recording Summary

**Staleness gate marking old exchange data as is_stale=true, feed latency metrics via metrics crate, and periodic 1s flush replacing per-write flush in recording writer**

## Performance

- **Duration:** 14 min
- **Started:** 2026-02-22T14:11:16Z
- **Completed:** 2026-02-22T14:25:49Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Staleness gate checks exchange_timestamp age against configurable threshold (default 5s), OR'd with book.is_stale, logs warning on stale data
- Feed latency metrics (histogram, gauge, counter) recorded on every snapshot with exchange_timestamp via metrics crate macros (zero-cost no-ops until Phase 6 recorder)
- Recording writer now has `write_line_no_flush` method; recording_task uses 1-second periodic flush instead of flush-per-write, eliminating the per-write disk I/O bottleneck
- 5 new staleness tests covering old data, fresh data, no exchange_ts, OR logic, and the is_exchange_data_stale function

## Task Commits

Each task was committed atomically:

1. **Task 1: Add staleness gate and latency metrics to normalization pipeline** - `5b6373b` (feat)
2. **Task 2: Switch recording writer to periodic flush for throughput** - `f6ae533` (feat)

## Files Created/Modified
- `Cargo.toml` - Added metrics 0.24 dependency (Phase 3 latency tracking)
- `src/feed/deribit/normalize.rs` - Staleness gate (is_exchange_data_stale), latency metrics (histogram/gauge/counter), staleness_threshold_ms on DeribitProcessor and build_snapshot, 5 new staleness tests
- `src/feed/recording/writer.rs` - Added write_line_no_flush method, updated doc comments
- `src/feed/recording/mod.rs` - Rewrote recording_task with periodic 1s flush interval, biased select, messages_since_flush counter

## Decisions Made
- **Staleness gate OR logic:** `is_stale = book.is_stale || exchange_data_stale` -- both book sequence gaps and exchange timestamp age contribute to staleness
- **metrics crate no-op pattern:** Instrumented code with metrics macros that compile to no-ops without a recorder installed. Prometheus exporter comes in Phase 6 -- this is intentional decoupling per research recommendation
- **Hardcoded test timestamps:** Processor async tests use `u64::MAX` staleness threshold since JSON payloads contain hardcoded 2023 timestamps that cannot be made dynamic. Staleness gate logic is thoroughly tested in dedicated unit tests with fresh timestamps
- **biased select order:** cancel > recv > flush tick -- ensures shutdown is always checked first, messages are processed before flush ticks

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Updated existing unit tests with fresh timestamps**
- **Found during:** Task 1 (staleness gate implementation)
- **Issue:** Existing `build_snapshot_from_book_only` and `build_snapshot_with_ticker_state` tests used hardcoded 2023 timestamps (`1703001600000`) that would now be correctly flagged as stale by the new gate, breaking `assert!(!snap.is_stale)` assertions
- **Fix:** Replaced hardcoded timestamps with `fresh_exchange_ts()` helper that returns current time minus 100ms, and updated exchange_timestamp assertions to match
- **Files modified:** src/feed/deribit/normalize.rs
- **Verification:** All 15 normalize tests pass
- **Committed in:** 5b6373b (Task 1 commit)

**2. [Rule 1 - Bug] Updated processor async test staleness assertion**
- **Found during:** Task 1 (staleness gate implementation)
- **Issue:** `processor_handles_book_message` test asserted `!snap.is_stale` but the JSON payload has a 2023 timestamp which is correctly stale
- **Fix:** Changed assertion to `assert!(snap.is_stale)` with explanatory comment that the 2023 test data is correctly flagged
- **Files modified:** src/feed/deribit/normalize.rs
- **Verification:** Test passes with correct stale assertion
- **Committed in:** 5b6373b (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 bugs -- test assertions incompatible with new staleness gate)
**Impact on plan:** Both fixes were necessary for test correctness after adding the staleness gate. No scope creep.

## Issues Encountered
- Plan 01 was already partially executed (config extensions + client refactor committed), so `staleness_threshold_ms` and `metrics` were already in Cargo.toml and DeribitConfig. This simplified Task 1 but required careful verification of what was already committed vs. what still needed doing.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Staleness-aware normalization ready for downstream consumers
- Feed latency metrics instrumented, ready for Prometheus exporter in Phase 6
- Recording throughput optimized, ready for production-volume data
- Plan 03-03 (rate limiter) can proceed independently

## Self-Check: PASSED

- All 5 key files exist on disk
- Both commit hashes (5b6373b, f6ae533) found in git log
- Content markers verified: is_exchange_data_stale (7 refs), feed_latency_ms (1 ref), write_line_no_flush (3 refs), flush_interval (2 refs)
- 108 tests pass, zero warnings

---
*Phase: 03-feed-infrastructure*
*Completed: 2026-02-22*
