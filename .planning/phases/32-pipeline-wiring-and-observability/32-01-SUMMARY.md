---
phase: 32-pipeline-wiring-and-observability
plan: 01
subsystem: subscription
tags: [watch-channel, hashset-diff, metrics, derive, reconciliation]

# Dependency graph
requires:
  - phase: 31-derive-feed-and-normalization
    provides: DeriveProcessor, DeriveClient, DeriveMapping in EventVenues
provides:
  - Derive venue support in SubscriptionManager (4-venue reconciliation)
  - Derive watch channel (Sender/Receiver) for supervisor wiring
  - CleanupEvent.derive_instruments populated from actual diff
  - Subscription metrics with venue=derive label
affects: [32-02-PLAN, pipeline wiring, derive supervisor]

# Tech tracking
tech-stack:
  added: []
  patterns: [4-venue subscription reconciliation extending 3-venue pattern]

key-files:
  created: []
  modified:
    - src/subscription/manager.rs
    - src/feed/pipeline.rs
    - tests/integration.rs
    - tests/smoke_test.rs

key-decisions:
  - "Derive follows identical pattern to Deribit for subscription management (HashSet diff, sorted Vec send, gauge+counter metrics)"
  - "Pipeline.rs derives _derive_rx (underscore prefix) since supervisor wiring is Plan 02"

patterns-established:
  - "4-venue subscription pattern: all new venues extend SubscriptionSenders/Receivers/Manager with watch channel + HashSet diff + metrics"

requirements-completed: [PIPE-03]

# Metrics
duration: 7min
completed: 2026-03-05
---

# Phase 32 Plan 01: SubscriptionManager Derive Venue Support Summary

**4-venue subscription reconciliation with Derive HashSet diff, watch channel push, and venue=derive metrics emission**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-05T22:43:56Z
- **Completed:** 2026-03-05T22:50:54Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Extended SubscriptionManager from 3-venue to 4-venue with full Derive support
- Derive reconciliation uses identical HashSet diff pattern as Deribit/Polymarket/Kalshi
- CleanupEvent.derive_instruments now populated from actual removed instruments (was hardcoded Vec::new())
- Subscription metrics emit with venue=derive label (gauge, activations counter, removals counter)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Derive fields to SubscriptionSenders, SubscriptionReceivers, and SubscriptionManager** - `df5a4ad` (feat)
2. **Task 2: Add Derive diff/reconcile block and populate CleanupEvent.derive_instruments** - `251ecd9` (feat)
3. **Fix: Add missing [derive] section to test venues.toml fixtures** - `263854b` (fix)

## Files Created/Modified
- `src/subscription/manager.rs` - Added derive fields to all structs, 4-tuple compute_desired_instruments, Derive diff/reconcile block with metrics
- `src/feed/pipeline.rs` - Updated SubscriptionReceivers destructuring for 4-venue support
- `tests/integration.rs` - Added [derive] section to inline venues.toml fixture
- `tests/smoke_test.rs` - Added [derive] section to inline venues.toml fixtures (2 locations)

## Decisions Made
- Derive follows identical pattern to Deribit for subscription management (HashSet diff, sorted Vec send, gauge+counter metrics)
- Pipeline.rs extracts _derive_rx with underscore prefix since supervisor wiring happens in Plan 02

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated pipeline.rs SubscriptionReceivers destructuring**
- **Found during:** Task 1 (Adding derive field to SubscriptionReceivers)
- **Issue:** pipeline.rs destructures SubscriptionReceivers into 3 fields; adding a 4th field would cause compile error
- **Fix:** Extended destructuring to include _derive_rx (underscore prefix for now, wired in Plan 02)
- **Files modified:** src/feed/pipeline.rs
- **Verification:** cargo check passes
- **Committed in:** df5a4ad (Task 1 commit)

**2. [Rule 3 - Blocking] Added [derive] section to test venues.toml fixtures**
- **Found during:** Task 2 verification (cargo test -p prediction)
- **Issue:** Test inline venues.toml strings missing [derive] section, causing TOML parse error (DeriveConfig is required in VenuesConfig since Phase 30)
- **Fix:** Added `[derive]\nws_url = "wss://api.lyra.finance/ws"\n` to 3 test fixtures
- **Files modified:** tests/integration.rs, tests/smoke_test.rs
- **Verification:** All 642+ tests pass
- **Committed in:** 263854b (separate fix commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both auto-fixes necessary for compilation and test passing. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SubscriptionManager ready with 4-venue support
- Derive watch channel receivers available for supervisor wiring in Plan 02
- CleanupEvent carries Derive removed instruments for processor state eviction

---
*Phase: 32-pipeline-wiring-and-observability*
*Completed: 2026-03-05*
