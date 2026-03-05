---
phase: 33-discovery-and-matching
plan: 01
subsystem: discovery
tags: [derive, rest-api, instrument-discovery, cross-venue-matching, decimal]

# Dependency graph
requires:
  - phase: 30-derive-foundation
    provides: Venue::Derive enum variant and CandidateVenues.derive field
  - phase: 31-derive-feed
    provides: Decimal-based price parsing decision
provides:
  - discover_derive() function for REST-based BTC options discovery
  - Active Derive matching in both filter_new_candidates and filter_new_candidates_fuzzy
affects: [33-discovery-and-matching]

# Tech tracking
tech-stack:
  added: []
  patterns: [POST-based REST discovery (Derive uses POST not GET), string-to-Decimal strike parsing]

key-files:
  created: []
  modified: [src/events/discovery.rs]

key-decisions:
  - "Derive discovery uses POST method (not GET) per API requirement (405 on GET)"
  - "String strikes parsed via Decimal::from_str for precision (not f64)"
  - "Epoch expiry auto-detects seconds vs milliseconds (threshold 10 billion)"

patterns-established:
  - "Derive REST discovery: POST with JSON body to /public/get_instruments"

requirements-completed: [DISC-01, DISC-02, DISC-03]

# Metrics
duration: 4min
completed: 2026-03-06
---

# Phase 33 Plan 01: Discovery and Matching Summary

**Derive REST discovery with POST-based instrument fetch, Decimal strike parsing, and active cross-venue matching in both filter functions**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-05T23:18:38Z
- **Completed:** 2026-03-05T23:22:44Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- Implemented discover_derive() with DeriveInstrumentsResponse, DeriveInstrumentInfo, and DeriveOptionDetails structs
- Both filter_new_candidates() and filter_new_candidates_fuzzy() now actively route Derive instruments into CandidateVenues.derive
- All existing tests pass with no regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement discover_derive() function with REST API response structs** - `b5b5bcb` (feat)
2. **Task 2: Replace Venue::Derive stubs with active matching in filter functions** - `1711d8b` (feat)

## Files Created/Modified
- `src/events/discovery.rs` - Added Derive response structs, discover_derive() function, and active Venue::Derive match arms in both filter functions

## Decisions Made
- Used POST (not GET) for Derive API per their 405 response on GET requests
- Parsed string strikes with Decimal::from_str (not f64) per Phase 31 precision decision
- Auto-detect seconds vs milliseconds for expiry timestamps (threshold: 10 billion)
- Handle both "C"/"call" and "P"/"put" option_type formats for robustness

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- discover_derive() is ready to be called from lifecycle.rs in Plan 02
- Both filter functions will include Derive instruments in cross-venue candidate matching
- No blockers for integration testing

---
*Phase: 33-discovery-and-matching*
*Completed: 2026-03-06*
