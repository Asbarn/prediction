---
phase: 23-dynamic-supervisor-subscriptions
plan: 01
subsystem: feed
tags: [tokio-watch, watch-receiver, supervisor-reconnect, dynamic-subscription]

# Dependency graph
requires:
  - phase: 22-subscription-manager-core
    provides: "SubscriptionManager with watch::Sender channels, SubscriptionReceivers struct, create_channels() factory"
provides:
  - "All three venue supervisors accept watch::Receiver for dynamic instrument list updates"
  - "changed() select branch in each supervisor triggers graceful reconnection on subscription changes"
  - "Pipeline threading passes subscription receivers from main.rs through to supervisor constructors"
  - "One-shot watch channels for Mock/Replay modes maintain uniform supervisor interface"
affects: [24-stale-state-cleanup, future-subscription-introspection]

# Tech tracking
tech-stack:
  added: []
  patterns: [watch-receiver-in-supervisor, borrow-and-update-init, borrow-clone-at-reconnect-top, changed-select-branch]

key-files:
  created: []
  modified:
    - src/feed/deribit/supervisor.rs
    - src/feed/polymarket/supervisor.rs
    - src/feed/kalshi/supervisor.rs
    - src/feed/pipeline.rs
    - src/main.rs
    - src/config/mod.rs

key-decisions:
  - "PolymarketAsset re-exported from config/mod.rs to enable import in polymarket supervisor (was private module)"
  - "One-shot watch channels with _tx prefix (dropped immediately) used for Mock/Replay modes -- changed() returns Err which is handled gracefully"
  - "Subscription receivers consumed by pipeline function, not post-hoc attached to PipelineHandles"

patterns-established:
  - "borrow_and_update() at run() init: prevents spurious startup reconnect from unseen initial value"
  - "borrow().clone() at reconnect loop top: reads latest value with immediate Ref drop before any .await"
  - "changed() Ok branch: log + backoff.reset() + break to outer loop for intentional reconnection"
  - "changed() Err branch: warn log + continue -- channel closed means SubscriptionManager dropped, supervisor keeps running with current instruments"

requirements-completed: [SUB-01, SUB-02]

# Metrics
duration: 6min
completed: 2026-02-27
---

# Phase 23 Plan 01: Dynamic Supervisor Subscriptions Summary

**Watch channel receivers wired into all three venue supervisors with changed() reconnection trigger and pipeline threading from main.rs**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-27T18:57:17Z
- **Completed:** 2026-02-27T19:04:04Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- All three venue supervisors (Deribit, Polymarket, Kalshi) accept watch::Receiver and respond to instrument list changes with graceful reconnection
- Pipeline function threads subscription receivers from main.rs through to each supervisor constructor, eliminating the post-hoc attachment pattern
- Mock/Replay modes create one-shot watch channels seeded with config values, maintaining uniform supervisor interface without behavioral change
- Full test suite passes with zero regressions (548 unit + 22 integration + 3 doc tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add watch::Receiver to all three venue supervisors with changed() select branch** - `8377e48` (feat)
2. **Task 2: Thread subscription receivers from main.rs through pipeline.rs to supervisor constructors** - `b6fada2` (feat)

## Files Created/Modified
- `src/feed/deribit/supervisor.rs` - instruments_rx watch::Receiver replacing static Vec<String>, changed() select branch, borrow_and_update() at init
- `src/feed/polymarket/supervisor.rs` - assets_rx watch::Receiver with PolymarketSubscription-to-PolymarketAsset conversion, changed() select branch
- `src/feed/kalshi/supervisor.rs` - tickers_rx watch::Receiver with config injection at reconnect top, changed() select branch
- `src/feed/pipeline.rs` - run_multi_venue_pipeline() and run_live_multi_venue() accept Option<SubscriptionReceivers>, per-venue destructuring, one-shot fallback channels
- `src/main.rs` - Pass sub_receivers into pipeline function, removed post-hoc PipelineHandles attachment
- `src/config/mod.rs` - Re-export PolymarketAsset from venues module

## Decisions Made
- Re-exported PolymarketAsset from config/mod.rs since the venues module is private and the polymarket supervisor needs the type for subscription-to-config conversion
- One-shot watch channels for Mock/Replay modes use `let (_tx, rx)` pattern where sender is dropped immediately; this is correct because changed() returns Err which the supervisor handles gracefully with a warning log
- Subscription receivers are consumed by the pipeline function rather than post-hoc attached, which is cleaner and ensures supervisors receive their receivers at construction time

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Re-exported PolymarketAsset from config/mod.rs**
- **Found during:** Task 1 (PolymarketSupervisor modifications)
- **Issue:** Plan specified `use crate::config::PolymarketAsset` but the `venues` module in config is private, so `PolymarketAsset` was not accessible
- **Fix:** Added `PolymarketAsset` to the `pub use venues::{...}` re-export in `src/config/mod.rs`
- **Files modified:** `src/config/mod.rs`
- **Verification:** `cargo check` compiles without errors
- **Committed in:** `8377e48` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minimal -- single re-export addition necessary for module visibility. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three venue supervisors now dynamically read instrument lists from watch channels
- SubscriptionManager pushes to watch::Sender -> supervisors receive via watch::Receiver -> reconnect with updated instruments
- Phase 24 (stale state cleanup) can proceed -- supervisors will unsubscribe correctly but downstream state (SpreadEngine/processor HashMaps) still grows monotonically
- All existing tests pass with no regressions

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 23-dynamic-supervisor-subscriptions*
*Completed: 2026-02-27*
