---
phase: 21-lifecycle-management-and-integration
plan: 01
subsystem: events
tags: [toml, lifecycle, archival, cleanup, toml_edit, chrono]

requires:
  - phase: 20-proposal-workflow-and-operator-interface
    provides: "Lifecycle status enum, TOML writer batch functions, discovery config"
provides:
  - "LifecycleStatus::Retired variant for archived event distinction"
  - "DiscoveryConfig.archive_retention_days (default 30) for retention policy"
  - "collect_archivable_entries function for finding events past retention"
  - "collect_expired_unapproved_ids function for cleaning up stale candidates"
  - "remove_entries_by_id function for removing entries from [[events]] array"
  - "append_entries_to_archive_doc function for writing to archive document"
affects: [21-02-PLAN, events-archive-integration]

tech-stack:
  added: []
  patterns: ["Archive/cleanup helpers operate on DocumentMut (parse once, mutate, write once)", "Defensive TOML parsing with unwrap_or defaults"]

key-files:
  created: []
  modified:
    - src/config/events.rs
    - src/events/toml_writer.rs
    - config/events.toml

key-decisions:
  - "Retired variant added after Expired in enum order for logical lifecycle progression"
  - "Archive retention default 30 days matches typical financial data retention expectations"
  - "collect_archivable_entries uses unwrap_or(true) for approved to treat manually-authored entries as approved"
  - "collect_expired_unapproved_ids uses strict less-than for expiry check, consistent with validation.rs"

patterns-established:
  - "Archive helpers use same DocumentMut-in-place pattern as existing batch functions"
  - "Defensive TOML parsing: unwrap_or defaults for missing fields, skip on parse failure"

requirements-completed: [LIFE-03, LIFE-01, LIFE-02]

duration: 6min
completed: 2026-02-27
---

# Phase 21 Plan 01: Lifecycle Foundation Summary

**Retired lifecycle status, 30-day archive retention config, and four toml_writer archive/cleanup helper functions with unit tests**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-27T09:45:07Z
- **Completed:** 2026-02-27T09:51:10Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added Retired variant to LifecycleStatus enum with serde lowercase serialization and Display impl
- Added archive_retention_days field to DiscoveryConfig with default 30 days
- Implemented four archive/cleanup helper functions in toml_writer with defensive TOML parsing
- Added four comprehensive unit tests covering filtering, removal, and archive append operations

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Retired variant and archive_retention_days** - `56e1c35` (feat)
2. **Task 2: Add archive and cleanup helper functions** - `2ee52e8` (feat)

## Files Created/Modified
- `src/config/events.rs` - Added Retired variant to LifecycleStatus, archive_retention_days to DiscoveryConfig with default function
- `src/events/toml_writer.rs` - Added collect_archivable_entries, collect_expired_unapproved_ids, remove_entries_by_id, append_entries_to_archive_doc with NaiveDate import and 4 unit tests
- `config/events.toml` - Added archive_retention_days = 30 to [discovery] section

## Decisions Made
- Retired variant placed after Expired in enum for logical lifecycle ordering (Active -> Expiring -> Expired -> Retired)
- Archive retention default is 30 days, matching typical financial data retention periods
- Defensive parsing: approved defaults to true (manually-authored entries treated as approved), status defaults to "active"
- Strict less-than for expiry comparison in collect_expired_unapproved_ids, consistent with validation.rs pattern
- collect_archivable_entries compares expiry date against (today - retention_days) cutoff

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All four archive/cleanup helper functions are tested and ready for Plan 02 integration
- Plan 02 can import and use these functions to integrate archive-and-cleanup into the poll cycle
- LifecycleStatus::Retired is available for marking archived events

## Self-Check: PASSED

All files exist. All commits verified.

---
*Phase: 21-lifecycle-management-and-integration*
*Completed: 2026-02-27*
