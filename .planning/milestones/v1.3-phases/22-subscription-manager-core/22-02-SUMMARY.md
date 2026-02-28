---
phase: 22-subscription-manager-core
plan: 02
subsystem: subscription
tags: [tokio-notify, watch-channel, pipeline-handles, main-wiring]

# Dependency graph
requires:
  - phase: 22-subscription-manager-core
    plan: 01
    provides: "SubscriptionManager struct with create_channels(), SubscriptionSenders, SubscriptionReceivers"
provides:
  - "SubscriptionManager wired into main.rs runtime with Notify ordering guarantee"
  - "Watch channel receivers accessible via PipelineHandles.subscription_rx"
  - "Config reload -> registry refresh -> drop(reg) -> notify_one() -> reconcile ordering"
affects: [23-supervisor-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: [notify-ordering-wiring, pipeline-handles-extension, explicit-lock-drop]

key-files:
  created: []
  modified:
    - src/main.rs
    - src/feed/pipeline.rs
    - src/replay/mod.rs

key-decisions:
  - "Subscription infrastructure guarded by is_live check -- Mock/Replay modes get subscription_rx: None"
  - "Explicit drop(reg) before notify_one() with CRITICAL comment to prevent future regression"
  - "sub_senders/sub_receivers wrapped in Option to flow out of is_live block without restructuring main.rs"

patterns-established:
  - "Lock-drop-notify: always drop write lock before Notify::notify_one() to prevent deadlock with read lock acquisition"
  - "PipelineHandles extension: add Option fields for live-only features, set None for Mock/Replay"

requirements-completed: [SUB-04, SUB-06]

# Metrics
duration: 5min
completed: 2026-02-27
---

# Phase 22 Plan 02: SubscriptionManager Wiring Summary

**SubscriptionManager wired into main.rs with Notify-based ordering guarantee and watch channel receivers on PipelineHandles**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-27T18:09:52Z
- **Completed:** 2026-02-27T18:15:16Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added subscription_rx: Option<SubscriptionReceivers> to PipelineHandles for Phase 23 supervisor consumption
- Wired SubscriptionManager into main.rs live mode: Notify creation, channel seeding from startup registry, spawning as tokio task
- Established Notify ordering guarantee: config reload -> registry.refresh() -> drop(reg) -> notify_one() -> reconcile reads fresh state
- All 4 PipelineHandles construction sites updated (Mock, Live, 2x Replay) with subscription_rx: None

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SubscriptionReceivers to PipelineHandles** - `190bda8` (feat)
2. **Task 2: Wire SubscriptionManager into main.rs with Notify ordering** - `ede98c0` (feat)

## Files Created/Modified
- `src/feed/pipeline.rs` - Added SubscriptionReceivers import and subscription_rx field to PipelineHandles; updated Mock and Live construction sites
- `src/replay/mod.rs` - Updated 2 Replay PipelineHandles construction sites with subscription_rx: None
- `src/main.rs` - Added Notify/SubscriptionManager imports, channel creation, config reload notify_one(), SubscriptionManager spawn, receiver attachment to PipelineHandles

## Decisions Made
- Wrapped sub_senders/sub_receivers in Option types to cleanly flow values out of the is_live conditional block without restructuring the existing main.rs control flow
- Added explicit `drop(reg)` with CRITICAL comment before `notify_one()` to make the deadlock-prevention ordering visible and protect against future regressions
- Placed SubscriptionManager spawn inside the config reload `if is_live` block since both are live-mode-only concerns

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 22 (Subscription Manager Core) is complete
- PipelineHandles.subscription_rx carries watch channel receivers ready for Phase 23 supervisor wiring
- SubscriptionManager runs as a tokio task, receiving Notify wakeups from config reload subscriber
- All existing tests pass (548 unit + 22 integration + 3 doc tests)

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 22-subscription-manager-core*
*Completed: 2026-02-27*
