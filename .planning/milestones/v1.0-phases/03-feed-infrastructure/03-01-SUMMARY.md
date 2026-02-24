---
phase: 03-feed-infrastructure
plan: 01
subsystem: feed
tags: [websocket, deribit, heartbeat, staleness, reconnect-config, metrics, serde-untagged]

# Dependency graph
requires:
  - phase: 02-deribit-feed
    provides: "DeribitClient, DeribitMessage, DeribitProcessor, RawDataSource trait, DeribitConfig"
provides:
  - "ReconnectConfig struct with exponential backoff parameters (initial/max backoff, jitter)"
  - "Staleness threshold on DeribitConfig (staleness_threshold_ms, default 5000)"
  - "HeartbeatNotification/HeartbeatParams structs for Deribit heartbeat protocol parsing"
  - "Heartbeat variant in DeribitMessage enum (untagged serde, before Notification)"
  - "Bidirectional DeribitClient: set_heartbeat on connect, test_request response, timeout detection"
  - "Staleness gate in build_snapshot (exchange timestamp age check OR book staleness)"
  - "metrics facade instrumentation (feed_latency_ms histogram, gauge, counter)"
affects: [03-feed-infrastructure, 04-multi-venue]

# Tech tracking
tech-stack:
  added: [metrics 0.24 (facade crate)]
  patterns: [bidirectional WS with heartbeat protocol, staleness gate on exchange timestamp, fast string check for message routing]

key-files:
  created: []
  modified:
    - src/config/venues.rs
    - config/venues.toml
    - src/feed/deribit/messages.rs
    - src/feed/deribit/client.rs
    - src/feed/deribit/normalize.rs
    - src/feed/pipeline.rs
    - tests/pipeline_test.rs
    - Cargo.toml
    - .gitignore

key-decisions:
  - "Heartbeat detection via fast string check (contains method:heartbeat) rather than full serde parse -- more robust for untagged enum ordering"
  - "Heartbeat variant placed before Notification in DeribitMessage enum so serde tries it first"
  - "Heartbeat responses exempt from rate limiting per research pitfall 6"
  - "Heartbeat timeout at 2x interval (not 3x) -- aggressive enough to detect dead connections quickly"
  - "Staleness gate uses OR logic: book.is_stale || exchange_data_stale"
  - "metrics crate macros are zero-cost no-ops with no recorder installed -- safe to add before Phase 6 Prometheus exporter"
  - "#[cfg(test)] on DEFAULT_STALENESS_THRESHOLD_MS constant -- only tests use it, production uses config value"

patterns-established:
  - "Bidirectional WS pattern: single task owns both read/write halves, responds to protocol messages inline"
  - "Heartbeat protocol: set_heartbeat -> detect test_request -> respond public/test -> timeout on silence"
  - "Staleness gate: check exchange_timestamp age before publishing MarketSnapshot, OR with book staleness"
  - "Fast string check before serde parse: use contains() on raw text for message routing when full parse is expensive"

# Metrics
duration: 10min
completed: 2026-02-22
---

# Phase 03 Plan 01: Heartbeat Config and Bidirectional WS Client Summary

**DeribitConfig extended with ReconnectConfig/staleness fields, DeribitMessage heartbeat parsing, and bidirectional DeribitClient with set_heartbeat, test_request response, and heartbeat timeout detection**

## Performance

- **Duration:** 10 min
- **Started:** 2026-02-22T14:11:12Z
- **Completed:** 2026-02-22T14:21:22Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- DeribitConfig extended with ReconnectConfig (initial_backoff_ms, max_backoff_ms, randomization_factor), staleness_threshold_ms (default 5000), and heartbeat_interval_ms (default 10000)
- DeribitMessage enum gains Heartbeat variant that correctly deserializes heartbeat notifications without breaking existing Response/Notification variants
- DeribitClient refactored for bidirectional WS: sends public/set_heartbeat on connect, responds to test_request with public/test, detects dead connections via 2x heartbeat timeout
- Staleness gate in build_snapshot checks exchange timestamp age and marks snapshots is_stale=true when data is older than threshold
- metrics facade (histogram, gauge, counter) instrumented in build_snapshot for feed latency tracking (zero-cost until recorder installed in Phase 6)

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend DeribitConfig with reconnect/staleness fields and add heartbeat message variant** - `1293aaf` (feat)
2. **Task 2: Refactor DeribitClient for bidirectional WS with heartbeat protocol** - `5b6373b` (feat)
3. **Cleanup: Add .idea/.iml to gitignore** - `f4b12dd` (chore)

## Files Created/Modified
- `src/config/venues.rs` - ReconnectConfig struct, staleness_threshold_ms and reconnect fields on DeribitConfig
- `config/venues.toml` - New staleness and reconnect config sections with comments
- `src/feed/deribit/messages.rs` - HeartbeatNotification/HeartbeatParams structs, Heartbeat variant in DeribitMessage enum, 4 heartbeat tests
- `src/feed/deribit/client.rs` - Bidirectional WS loop: set_heartbeat, test_request response, heartbeat timeout, write half moved into task
- `src/feed/deribit/normalize.rs` - Heartbeat arm in message match, staleness gate in build_snapshot, staleness gate tests
- `src/feed/pipeline.rs` - Pass staleness_threshold_ms from config to DeribitProcessor
- `tests/pipeline_test.rs` - Add new DeribitConfig fields to test config constructor
- `Cargo.toml` - Added metrics 0.24 dependency
- `.gitignore` - Added .idea/ and *.iml patterns

## Decisions Made
- Used fast string check (`contains("method":"heartbeat"`) for heartbeat detection instead of relying solely on serde untagged enum ordering -- more robust against JSON key ordering variations
- Heartbeat responses sent immediately without rate limiting (Deribit closes connection if test_request response is delayed)
- Heartbeat timeout set to 2x interval (20s for 10s interval) per research recommendation
- Staleness gate uses OR logic: snapshot is stale if either book.is_stale OR exchange data is older than threshold
- metrics facade added in Phase 3 with no recorder (zero-cost no-ops) -- Prometheus recorder deferred to Phase 6

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed DeribitProcessor::new and build_snapshot call sites**
- **Found during:** Task 1 (compilation after adding staleness_threshold_ms)
- **Issue:** Uncommitted changes from prior session added staleness_threshold_ms parameter to DeribitProcessor::new and build_snapshot, but callers (normalize.rs tests, pipeline.rs, pipeline_test.rs) were not updated
- **Fix:** Updated all callers to pass the new staleness_threshold_ms parameter; added reconnect and staleness fields to pipeline_test.rs config constructor
- **Files modified:** src/feed/deribit/normalize.rs, src/feed/pipeline.rs, tests/pipeline_test.rs
- **Verification:** cargo build + cargo test pass
- **Committed in:** 1293aaf (Task 1 commit)

**2. [Rule 1 - Bug] Fixed processor_handles_book_message test assertion**
- **Found during:** Task 1 (test assertion after staleness gate activation)
- **Issue:** Test used 2023 exchange timestamp (1703001600000) which is correctly flagged as stale by the new staleness gate, but test asserted `!snap.is_stale`
- **Fix:** Updated assertion to `assert!(snap.is_stale)` with comment explaining the 2023 timestamp is correctly stale; later linter improved tests to use fresh timestamps
- **Files modified:** src/feed/deribit/normalize.rs
- **Verification:** All 108 tests pass
- **Committed in:** 1293aaf, 5b6373b (across both commits)

**3. [Rule 2 - Missing Critical] Added .gitignore entries for IDE files**
- **Found during:** Task 2 (IDE files accidentally committed)
- **Issue:** .idea/ directory and .iml file were tracked by git
- **Fix:** Added .idea/ and *.iml to .gitignore, removed from tracking
- **Files modified:** .gitignore
- **Committed in:** f4b12dd

---

**Total deviations:** 3 auto-fixed (1 blocking, 1 bug, 1 missing critical)
**Impact on plan:** All auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
- Uncommitted partial changes from a prior session added staleness_threshold_ms to DeribitProcessor/build_snapshot but didn't update all callers -- resolved by fixing all call sites systematically
- Test data with historical timestamps (2023) triggered the new staleness gate -- resolved by asserting stale=true for old data and adding fresh timestamp test helpers

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Heartbeat protocol handling complete and ready for live connection testing
- ReconnectConfig ready for Plan 03-02 (reconnection supervisor with exponential backoff)
- Staleness gate active and correctly marks old data -- ready for Plan 03-02 to use with reconnect-on-stale logic
- DeribitClient exits cleanly on heartbeat timeout, ready for supervisor to detect and reconnect

## Self-Check: PASSED

- All 9 modified files verified present on disk
- Commit 1293aaf (Task 1) verified in git log
- Commit 5b6373b (Task 2) verified in git log
- Commit f4b12dd (cleanup) verified in git log
- 108 tests pass (64 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc)
- Zero compiler warnings

---
*Phase: 03-feed-infrastructure*
*Completed: 2026-02-22*
