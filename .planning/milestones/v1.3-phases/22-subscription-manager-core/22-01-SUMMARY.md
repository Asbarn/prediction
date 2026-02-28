---
phase: 22-subscription-manager-core
plan: 01
subsystem: subscription
tags: [tokio-watch, tokio-notify, hashset-diff, reconciliation, tracing]

# Dependency graph
requires:
  - phase: 05-event-mapping
    provides: "EventRegistry with active_approved() filtering and per-venue instrument indexes"
provides:
  - "SubscriptionManager struct with reconcile(), compute_desired_instruments(), run loop"
  - "PolymarketSubscription type carrying both condition_id and token_id"
  - "Per-venue watch channels (deribit, polymarket, kalshi) seeded with initial instrument state"
  - "SubscriptionSenders/SubscriptionReceivers helper structs for channel wiring"
  - "create_channels() factory that seeds watch channels from registry startup state"
affects: [22-02, 23-supervisor-wiring, main-rs-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: [notify-based-ordering, set-difference-reconciliation, watch-channel-seeding]

key-files:
  created:
    - src/subscription/mod.rs
    - src/subscription/manager.rs
  modified:
    - src/lib.rs

key-decisions:
  - "SubscriptionManager takes SubscriptionSenders struct rather than individual senders for cleaner constructor API"
  - "Polymarket diff logs use token_ids for readability rather than full PolymarketSubscription debug output"
  - "current_* sets initialized empty in constructor; first reconciliation detects all initial instruments as 'added'"

patterns-established:
  - "Notify-based ordering: config reload -> registry refresh -> notify_one -> reconcile reads fresh state"
  - "Lock-then-drop: acquire registry read lock, extract data, drop lock before channel send"
  - "Watch channel seeding: create_channels() computes initial values from registry to avoid empty initial state"

requirements-completed: [SUB-03, OBS-03, OPS-02]

# Metrics
duration: 4min
completed: 2026-02-27
---

# Phase 22 Plan 01: Subscription Manager Core Summary

**SubscriptionManager with per-venue HashSet diff reconciliation, watch channel push, and structured tracing output**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-27T18:01:15Z
- **Completed:** 2026-02-27T18:05:37Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments
- Created SubscriptionManager with full reconciliation logic computing per-venue instrument diffs via HashSet::difference()
- Defined PolymarketSubscription type carrying both condition_id and token_id (venue-specific subscription format)
- Implemented create_channels() factory that seeds watch channels with initial registry state (avoids empty initial value pitfall)
- Structured tracing logs per-venue diffs with added/removed counts and details; empty diffs logged at debug level

## Task Commits

Each task was committed atomically:

1. **Task 1: Create SubscriptionManager module with reconciliation logic** - `13846be` (feat)

## Files Created/Modified
- `src/subscription/mod.rs` - Module re-exports for SubscriptionManager, PolymarketSubscription, SubscriptionSenders, SubscriptionReceivers
- `src/subscription/manager.rs` - SubscriptionManager struct with reconcile(), compute_desired_instruments(), compute_diff(), run(), create_channels() (309 lines)
- `src/lib.rs` - Added `pub mod subscription` declaration

## Decisions Made
- Used SubscriptionSenders struct to bundle the three watch::Sender handles for a cleaner constructor API rather than passing six individual parameters
- Polymarket diff logging uses token_ids (human-readable) rather than full PolymarketSubscription debug output which would include redundant field names
- current_* sets initialized empty in the constructor; the first reconciliation from the run loop naturally detects all startup instruments as "added" (consistent with subsequent reconciliation behavior)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SubscriptionManager ready for wiring into main.rs config reload subscriber (Plan 22-02)
- Watch channel receivers ready for supervisor consumption (Phase 23)
- All existing tests pass (548 unit + 22 integration + 3 doc tests)

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 22-subscription-manager-core*
*Completed: 2026-02-27*
