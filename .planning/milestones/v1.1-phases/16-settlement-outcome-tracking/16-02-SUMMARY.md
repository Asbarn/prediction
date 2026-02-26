---
phase: 16-settlement-outcome-tracking
plan: 02
subsystem: settlement
tags: [settlement, monitor, polling, tokio, mpsc, backfill, tier-state-machine]

# Dependency graph
requires:
  - phase: 16-settlement-outcome-tracking
    provides: SettlementOutcome, ResolutionResult, PollingTier, VenueChecker, SettlementConfig, TrackedEvent
  - phase: 14-failure-alerting
    provides: AlertMonitor pattern (biased select, cancellation, interval tick), PipelineLiveness
  - phase: 06-event-mapping
    provides: EventRegistry with lookup_by_event_id and venue instrument mappings
provides:
  - SettlementMonitor async task with poll_cycle, tier management, and backfill
  - Per-event polling with four-tier cadence (Aggressive/Patient/Lazy/TimedOut)
  - Deribit trigger at 08:00 UTC on expiry day
  - Prediction market trigger anchored to Deribit settlement
  - Startup backfill with oldest-first processing and stale timeout
  - Channel-based SettlementOutcome delivery to PaperTradeTracker
affects: [16-03-paper-trade-integration, 17-signal-quality]

# Tech tracking
tech-stack:
  added: []
  patterns: [two-phase-borrow-split-poll-cycle, free-function-trigger-check, backfill-drain-pattern]

key-files:
  created:
    - src/settlement/monitor.rs
  modified:
    - src/settlement/mod.rs
    - src/settlement/types.rs

key-decisions:
  - "Free function check_trigger() instead of &self method to avoid borrow checker conflict in poll_cycle"
  - "Two-phase poll_cycle: immutable trigger pass then mutable tier advancement pass"
  - "TrackedEvent.is_backfill field with serde(default) for backward compatibility"
  - "Backfill timeouts stored in drain vec rather than sent during enqueue_backfill"

patterns-established:
  - "Two-phase poll cycle: collect trigger updates immutably, then apply mutably -- avoids &self/&mut self borrow conflicts"
  - "Drain pattern for backfill timeouts: store during initialization, caller drains after setup complete"

requirements-completed: [STTL-07]

# Metrics
duration: 7min
completed: 2026-02-25
---

# Phase 16 Plan 02: SettlementMonitor Task Summary

**SettlementMonitor long-running tokio task with four-tier polling cadence (Aggressive/Patient/Lazy/TimedOut), Deribit 08:00 UTC trigger, prediction market anchor-to-Deribit logic, and oldest-first backfill with stale timeout**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-25T22:52:16Z
- **Completed:** 2026-02-25T22:59:39Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments
- Complete SettlementMonitor async task following AlertMonitor pattern (biased select, cancellation token, interval tick)
- Per-event polling state machine: four-tier cadence drives individual event polling at tier-appropriate intervals while base loop ticks fast
- Trigger logic: Deribit fires at 08:00 UTC on expiry day; prediction markets fire when paired Deribit resolves or when past expiry date
- Startup backfill: oldest-first ordering, rate limiter awareness via is_backfill flag, stale positions (> 7 days) immediately marked as resolution_timeout
- Channel output: SettlementOutcome sent via mpsc::Sender for downstream PaperTradeTracker consumption
- 15 unit tests covering triggers, tier advancement, cleanup, backfill, and timeout handling

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement SettlementMonitor task with four-tier polling and channel output** - `9745e3f` (feat)

## Files Created/Modified
- `src/settlement/monitor.rs` - SettlementMonitor struct, run(), poll_cycle(), initialize_from_registry(), enqueue_backfill(), check_trigger(), cleanup_resolved() + 15 unit tests
- `src/settlement/mod.rs` - Added `pub mod monitor;`
- `src/settlement/types.rs` - Added `is_backfill: bool` field to TrackedEvent with `#[serde(default)]`

## Decisions Made
- **Free function check_trigger() over &self method:** The poll_cycle() method needs to mutably borrow `self.tracked_events` while checking triggers requires immutable access to the same map. Extracting check_trigger() as a free function taking `&HashMap<String, Vec<TrackedEvent>>` resolves the borrow checker conflict cleanly.
- **Two-phase poll_cycle:** Phase 1 collects trigger updates by reading tracked_events immutably, then applies them. Phase 2 does mutable tier advancement and API polling. This pattern avoids the simultaneous mutable+immutable borrow issue.
- **TrackedEvent.is_backfill with serde(default):** Adding the field with `#[serde(default)]` ensures backward compatibility with existing serialized TrackedEvent instances (they deserialize with is_backfill=false).
- **Drain pattern for backfill timeouts:** Rather than sending timeout outcomes on the channel during enqueue_backfill() (which would require async or holding the sender), stale timeouts are stored in a Vec and drained by the caller after initialization completes.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Refactored check_trigger from method to free function**
- **Found during:** Task 1 (initial compilation)
- **Issue:** `self.check_trigger()` borrows `self` immutably while `self.tracked_events` is already borrowed mutably in poll_cycle, causing E0502 borrow checker error
- **Fix:** Extracted check_trigger as a free function taking `&TrackedEvent`, `&str`, and `&HashMap<String, Vec<TrackedEvent>>` parameters. Restructured poll_cycle into two phases: immutable trigger collection pass, then mutable application pass.
- **Files modified:** src/settlement/monitor.rs
- **Verification:** `cargo check` succeeds, all 15 tests pass
- **Committed in:** 9745e3f (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Borrow checker required structural change to poll_cycle. The two-phase approach is actually cleaner than the single-pass design since it separates trigger evaluation from mutation. No scope creep.

## Issues Encountered
None beyond the auto-fixed borrow checker issue.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SettlementMonitor is ready for integration with PaperTradeTracker (Plan 03)
- Channel-based output (mpsc::Sender<SettlementOutcome>) matches the plan for Plan 03's receiver arm
- Initialize_from_registry() and enqueue_backfill() provide the startup API for wiring into the main run loop
- 477 total tests pass (15 new + 462 existing), no regressions

---
*Phase: 16-settlement-outcome-tracking*
*Completed: 2026-02-25*
