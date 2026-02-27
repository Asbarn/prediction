---
phase: 21-lifecycle-management-and-integration
plan: 02
subsystem: events
tags: [lifecycle, archival, cleanup, toml, prometheus, background-task]

requires:
  - phase: 21-lifecycle-management-and-integration
    provides: "Retired status, archive_retention_days config, 4 toml_writer archive/cleanup helpers"
provides:
  - "archive_and_cleanup method on ContractLifecycleManager with archive-then-remove safety"
  - "Poll cycle step 7c: archive expired events + clean unapproved candidates"
  - "Registry refresh after archive/cleanup modifications"
  - "INTG-01 verified: full discover-match-propose pipeline documented as periodic background task"
  - "Prometheus counters: lifecycle_events_archived, lifecycle_candidates_cleaned"
affects: [event-lifecycle-complete]

tech-stack:
  added: []
  patterns: ["Archive-then-remove safety: archive file written before removal from events.toml", "Separate read-modify-write cycle for archive/cleanup (independent of batched_toml_write)"]

key-files:
  created: []
  modified:
    - src/events/lifecycle.rs

key-decisions:
  - "archive_and_cleanup returns bool indicating whether events.toml was modified, OR-ed with needs_write for registry refresh"
  - "Archive-then-remove pattern: archive file written atomically before entries removed from events.toml"
  - "Integration test uses toml_writer functions directly (not full ContractLifecycleManager) to avoid HTTP client dependencies"

patterns-established:
  - "Poll cycle step ordering: discover -> match -> expire -> roll -> write -> warn -> cache -> archive+cleanup -> refresh -> gauge"
  - "Archive-then-remove: always persist archive before mutating source to prevent data loss on failure"

requirements-completed: [LIFE-01, LIFE-02, INTG-01]

duration: 5min
completed: 2026-02-27
---

# Phase 21 Plan 02: Archive-and-Cleanup Poll Cycle Integration Summary

**archive_and_cleanup wired into poll_cycle with archive-then-remove safety, Prometheus counters, and INTG-01 background pipeline verification**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-27T09:53:30Z
- **Completed:** 2026-02-27T09:58:03Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- Added archive_and_cleanup method implementing archive-then-remove safety pattern for expired events
- Wired archive_and_cleanup into poll_cycle between BasisRiskCache refresh and registry refresh
- Added INTG-01 doc comment documenting the complete 9-step periodic background pipeline
- Added integration test verifying the full archive-cleanup-retain sequence on TOML documents
- Added Prometheus counters for lifecycle_events_archived and lifecycle_candidates_cleaned

## Task Commits

Each task was committed atomically:

1. **Task 1: Add archive_and_cleanup method** - `6acbc47` (feat)
2. **Task 2: Wire into poll_cycle, INTG-01 docs, integration test** - `08584cd` (feat)

## Files Created/Modified
- `src/events/lifecycle.rs` - Added archive_and_cleanup method, wired into poll_cycle step 7c, INTG-01 doc comment, and archive_cleanup_integration_sequence test

## Decisions Made
- archive_and_cleanup returns `anyhow::Result<bool>` where bool indicates events.toml modification, enabling conditional registry refresh
- Integration test uses toml_writer functions directly rather than constructing full ContractLifecycleManager (avoids HTTP client, registry, and other heavy dependencies)
- Archive-then-remove pattern: if archive write fails, entries remain in events.toml until next cycle (no data loss)
- Unapproved expired candidates logged at WARN level before removal for operator visibility

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 21 (Lifecycle Management and Integration) is now complete with all plans executed
- The full event lifecycle loop is operational: discover -> match -> propose -> expire -> roll -> archive -> cleanup -> refresh
- INTG-01 verified: the complete pipeline runs as a periodic background task via tokio::spawn in main.rs

## Self-Check: PASSED

All files exist. All commits verified.

---
*Phase: 21-lifecycle-management-and-integration*
*Completed: 2026-02-27*
