---
phase: 18-discovery-infrastructure-hardening
plan: 01
subsystem: config, events
tags: [toml_edit, serde, discovery, batch-writes, DocumentMut]

# Dependency graph
requires:
  - phase: 16-settlement-verification
    provides: "toml_writer with single-candidate append and mark-expired functions"
provides:
  - "DiscoveryConfig with consecutive_absence_threshold (default 3) and partial_response_threshold (default 0.2)"
  - "Batch TOML mutation functions: append_candidates_to_doc, mark_expired_batch_in_doc"
  - "Private build_candidate_table helper for deduplicated table construction"
affects: [18-02, lifecycle, poll-cycle]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "DocumentMut in-place batch mutation (no file I/O per mutation)"
    - "Shared helper function for TOML table construction (build_candidate_table)"

key-files:
  created: []
  modified:
    - "src/config/events.rs"
    - "src/events/toml_writer.rs"

key-decisions:
  - "Factored build_candidate_table as private helper to deduplicate table construction between single and batch functions"
  - "Batch functions return Ok(()) rather than String -- caller controls serialization timing"

patterns-established:
  - "batch DocumentMut mutation: parse once, mutate N times, serialize once"
  - "serde default free functions for new config fields to maintain backward compatibility"

requirements-completed: [LIFE-04, INTG-03]

# Metrics
duration: 4min
completed: 2026-02-26
---

# Phase 18 Plan 01: Foundation Types Summary

**DiscoveryConfig extended with absence/partial-response thresholds, batch TOML mutation via append_candidates_to_doc and mark_expired_batch_in_doc operating on DocumentMut in-place**

## Performance

- **Duration:** 3m 45s
- **Started:** 2026-02-26T21:53:48Z
- **Completed:** 2026-02-26T21:57:33Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Extended DiscoveryConfig with consecutive_absence_threshold (u32, default 3) and partial_response_threshold (f64, default 0.2) with serde defaults for backward compatibility
- Added batch append_candidates_to_doc and mark_expired_batch_in_doc functions that mutate DocumentMut in-place without file I/O
- Refactored existing append_candidate_to_toml to use shared build_candidate_table helper, eliminating code duplication

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend DiscoveryConfig with absence and partial-response thresholds** - `83265b1` (feat)
2. **Task 2: Add batch TOML mutation functions to toml_writer** - `3472051` (feat)

## Files Created/Modified
- `src/config/events.rs` - Added consecutive_absence_threshold and partial_response_threshold fields to DiscoveryConfig with serde defaults and Default impl
- `src/events/toml_writer.rs` - Added build_candidate_table helper, append_candidates_to_doc batch append, mark_expired_batch_in_doc batch expiry

## Decisions Made
- Factored build_candidate_table as a private helper to deduplicate the table construction logic between the single append_candidate_to_toml and the new batch append_candidates_to_doc -- both now use the same field-population code
- Batch functions take &mut DocumentMut and return Result<()> rather than returning a String -- this lets the caller decide when to serialize, supporting the "parse once, mutate N times, write once" pattern

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- DiscoveryConfig threshold fields are ready for Plan 02 to wire into ContractLifecycleManager's absence tracker and partial-response detection
- Batch TOML functions are ready for Plan 02 to call from the refactored poll_cycle (parse events.toml once, apply all mutations, write once)
- All existing tests pass (576 total), backward compatibility confirmed

---
*Phase: 18-discovery-infrastructure-hardening*
*Completed: 2026-02-26*
