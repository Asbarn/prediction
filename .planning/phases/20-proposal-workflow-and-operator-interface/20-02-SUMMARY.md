---
phase: 20-proposal-workflow-and-operator-interface
plan: 02
subsystem: events
tags: [validation, safety-gate, lifecycle, approved-mapping, discovery]

# Dependency graph
requires:
  - phase: 20-proposal-workflow-and-operator-interface
    plan: 01
    provides: "proposal logging + metrics (proposals_total, proposals_pending)"
  - phase: 18-lifecycle-toml-persistence
    provides: "batched TOML writes with approved=false, atomic_write"
  - phase: 19-polymarket-discovery-and-cross-venue-matching
    provides: "cross-venue fuzzy matching, DiscoveredInstrument with venue field"
provides:
  - "Approved-mapping validation: venue count >= 2 and expiry not in past"
  - "Async instrument-activity warning for approved mappings absent from discovery"
  - "4 unit tests for approved-mapping validation edge cases"
affects: [21-end-to-end-integration-testing, operator-dashboards]

# Tech tracking
tech-stack:
  added: []
  patterns: ["approved-only validation guards: if event.approved { ... } to avoid false-rejecting candidates"]

key-files:
  created: []
  modified:
    - "src/config/validation.rs"
    - "src/events/lifecycle.rs"
    - "config/events.toml"
    - "tests/smoke_test.rs"

key-decisions:
  - "Strict less-than for expiry check: events expiring today are still valid (Deribit settles at 08:00 UTC)"
  - "Venue activity check gated behind non-empty discovery data per venue to avoid false warnings on empty API responses"
  - "Updated example events.toml to use far-future dates (2030) to avoid config validation failure in integration tests"

patterns-established:
  - "Approved-mapping safety gate pattern: synchronous validation on config reload + async discovery check per poll cycle"

requirements-completed: [PROP-04]

# Metrics
duration: 7min
completed: 2026-02-27
---

# Phase 20 Plan 02: Approved-Mapping Validation and Instrument Activity Warnings Summary

**Safety-gate validation rejecting approved mappings with < 2 venues or expired dates, plus async per-cycle WARN log when approved instruments are absent from discovery data**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-27T08:25:48Z
- **Completed:** 2026-02-27T08:33:20Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Extended validate_config() with two approved-mapping checks: minimum 2 venues for cross-venue arbitrage and expiry not in the past
- Added async instrument-activity check in lifecycle poll_cycle that warns when approved mapping instruments are absent from latest discovery data
- Wrote 4 unit tests covering: single-venue rejected, two-venues accepted, expired rejected, unapproved single-venue accepted
- All 600+ existing tests continue to pass (543 unit + 54 integration/smoke/schema + 3 doc tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add approved-mapping validation rules to validate_config()** - `85308fd` (feat)
2. **Task 2: Add async instrument-activity warning in lifecycle poll cycle** - `c908c22` (feat)

## Files Created/Modified
- `src/config/validation.rs` - Added venue_count >= 2 and expiry < today checks for approved mappings, plus 4 unit tests
- `src/events/lifecycle.rs` - Added instrument-activity check after discovery, gated behind venue poll success + non-empty data
- `config/events.toml` - Updated example events to use future expiry dates (2030) to pass new validation
- `tests/smoke_test.rs` - Updated expected event ID to match new example config

## Decisions Made
- Used strict less-than (`<`) for expiry comparison so events expiring today are still valid (Deribit settlement happens at 08:00 UTC, so an event with today's date should remain active until settlement)
- Gated per-venue instrument-activity checks behind both the `_polled` flag AND non-empty discovery results for that venue, preventing false warnings when a venue API returns successfully but with zero instruments
- Updated example events.toml to 2030 dates rather than adding test-specific config overrides, keeping the example files realistic

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated example events.toml with future expiry dates**
- **Found during:** Task 2 (integration test failure)
- **Issue:** Example config/events.toml had approved event with expiry 2025-06-27 which is now in the past, causing the new approved-mapping expiry validation to reject it
- **Fix:** Updated all event dates in config/events.toml to 2030, updated instrument IDs to match, updated smoke_test.rs assertion
- **Files modified:** config/events.toml, tests/smoke_test.rs
- **Verification:** All integration and smoke tests pass
- **Committed in:** c908c22 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary fix to keep example config valid under new validation rules. No scope creep.

## Issues Encountered
None beyond the deviation above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All approved-mapping safety gates are in place (synchronous validation + async discovery warnings)
- Phase 20 (Proposal Workflow and Operator Interface) is now complete
- Ready for Phase 21 (End-to-End Integration Testing)
- Config reload preserves previous valid config on validation failure (existing behavior, unmodified)

---
*Phase: 20-proposal-workflow-and-operator-interface*
*Completed: 2026-02-27*
