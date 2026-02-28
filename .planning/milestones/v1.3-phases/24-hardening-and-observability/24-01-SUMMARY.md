---
phase: 24-hardening-and-observability
plan: 01
subsystem: subscription
tags: [prometheus, metrics, gauge, counter, dry-run, cleanup-event, mpsc]

# Dependency graph
requires:
  - phase: 22-subscription-manager-core
    provides: "SubscriptionManager with reconcile(), watch channel push, per-venue HashSet diff"
  - phase: 23-dynamic-supervisor-subscriptions
    provides: "Supervisor watch::Receiver wiring, pipeline threading"
provides:
  - "SubscriptionConfig with dry_run field in system.rs"
  - "CleanupEvent struct for downstream state cleanup after unsubscribe"
  - "Prometheus subscription_active gauge per venue"
  - "Prometheus subscription_activations_total and subscription_removals_total counters per venue"
  - "Dry-run reconciliation mode that logs diffs, updates internal state, skips side effects"
  - "cleanup_txs mpsc sender infrastructure in SubscriptionManager"
affects: [24-02-stale-state-cleanup, future-metrics-dashboards]

# Tech tracking
tech-stack:
  added: []
  patterns: [dry-run-guard-early-return, capture-lengths-before-move, best-effort-try-send]

key-files:
  created: []
  modified:
    - src/config/system.rs
    - src/subscription/manager.rs
    - src/subscription/mod.rs
    - src/main.rs

key-decisions:
  - "Metrics emitted after state update (not before) so gauges reflect actual current state"
  - "Diff lengths captured into local variables before CleanupEvent consumes removed vectors"
  - "Dry-run skips metrics emission (gauges/counters reflect actual state only, not hypothetical)"
  - "cleanup_txs uses Vec<mpsc::Sender> not broadcast -- fixed number of consumers known at compile time"
  - "try_send for cleanup events: best-effort, non-blocking, logged on failure"

patterns-established:
  - "Dry-run guard: early return after logging, updates internal state for meaningful subsequent diffs"
  - "Capture-before-move: extract lengths from diff vectors before transferring ownership to CleanupEvent"
  - "Best-effort try_send: cleanup channel sends are non-blocking with warn-level logging on failure"

requirements-completed: [OBS-01, OBS-02, OPS-01]

# Metrics
duration: 5min
completed: 2026-02-27
---

# Phase 24 Plan 01: Subscription Metrics, Dry-Run Mode, and CleanupEvent Infrastructure Summary

**Prometheus subscription gauges/counters per venue, dry-run reconciliation mode, and CleanupEvent mpsc infrastructure for downstream state cleanup**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-27T21:51:50Z
- **Completed:** 2026-02-27T21:57:35Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added SubscriptionConfig with dry_run field to system.rs, deserialized from [subscription] TOML section
- Implemented Prometheus metrics in reconcile(): subscription_active gauge (3 per venue) and subscription_activations_total/subscription_removals_total counters (6 per venue)
- Added dry-run guard that logs diffs and updates internal state but skips watch sends, cleanup events, and metrics emission
- Defined CleanupEvent struct and wired cleanup_txs Vec<mpsc::Sender> into SubscriptionManager for downstream state cleanup
- All 548 unit + 22 integration + 3 doc tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SubscriptionConfig and CleanupEvent struct** - `116b51f` (feat)
2. **Task 2: Add metrics emission, dry-run guard, and cleanup sender to reconcile()** - `bb95807` (feat)

## Files Created/Modified
- `src/config/system.rs` - Added SubscriptionConfig struct with dry_run field, added subscription field to SystemConfig
- `src/subscription/manager.rs` - Added CleanupEvent struct, dry_run/cleanup_txs fields, dry-run guard in reconcile(), metrics emission, cleanup event sending
- `src/subscription/mod.rs` - Re-exported CleanupEvent for downstream use
- `src/main.rs` - Pass dry_run config and empty cleanup_txs to SubscriptionManager::new()

## Decisions Made
- Metrics emitted after state update so gauges reflect actual current subscription counts, not pre-reconciliation state
- Diff lengths captured into local variables before CleanupEvent construction consumes the removed vectors (avoids borrow-after-move)
- Dry-run mode skips metrics emission entirely -- gauges and counters should reflect actual operational state only
- cleanup_txs uses Vec<mpsc::Sender<CleanupEvent>> rather than broadcast channel since the number of downstream consumers is fixed and known at construction time
- try_send used for cleanup events: non-blocking best-effort delivery with warn-level logging on failure (cleanup is not critical path)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- CleanupEvent infrastructure ready for Plan 02 to wire actual cleanup channel receivers to downstream engines
- Prometheus metrics active: subscription_active gauge and subscription_activations_total/subscription_removals_total counters
- Dry-run mode available via [subscription] dry_run = true in config.toml
- All existing tests pass with no regressions (548 unit + 22 integration + 3 doc tests)

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 24-hardening-and-observability*
*Completed: 2026-02-27*
