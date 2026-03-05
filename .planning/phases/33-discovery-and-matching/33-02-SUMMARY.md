---
phase: 33-discovery-and-matching
plan: 02
subsystem: events
tags: [derive, discovery, lifecycle, polling, absence-tracking]

# Dependency graph
requires:
  - phase: 33-discovery-and-matching (plan 01)
    provides: discover_derive function in discovery.rs
provides:
  - Derive polling integrated into ContractLifecycleManager poll_cycle
  - derive_poll_interval_secs config field with default 300s
  - Derive absence checking for approved mappings
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Four-venue discovery loop (Deribit, Kalshi, Polymarket, Derive) in lifecycle poll_cycle"

key-files:
  created: []
  modified:
    - src/config/events.rs
    - src/events/lifecycle.rs

key-decisions:
  - "Derive poll interval 300s (same as Deribit -- both options exchanges with similar instrument churn)"
  - "REST URL derived from ws_url by stripping wss:// and /ws path (same pattern as Deribit)"

patterns-established:
  - "Venue polling block pattern: interval check, rate limiter, discover call, suspect detection, extend all_discovered"

requirements-completed: [DISC-04]

# Metrics
duration: 4min
completed: 2026-03-06
---

# Phase 33 Plan 02: Derive Lifecycle Integration Summary

**Derive discovery wired into lifecycle poll_cycle with configurable 300s interval, suspect detection, and four-venue absence tracking**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-05T23:25:15Z
- **Completed:** 2026-03-05T23:29:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- DiscoveryConfig gains derive_poll_interval_secs (default 300s) participating in min_poll_interval_secs
- poll_cycle discovers Derive instruments on its own interval with rate limiting, suspect detection, and error handling
- Approved mappings with Derive instruments are absence-checked against discovery results
- Complete four-venue discovery pipeline (Deribit, Kalshi, Polymarket, Derive)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add derive_poll_interval_secs to DiscoveryConfig** - `896452c` (feat)
2. **Task 2: Wire Derive polling and absence checking into lifecycle poll_cycle** - `56e1a7d` (feat)

## Files Created/Modified
- `src/config/events.rs` - Added derive_poll_interval_secs field, default fn, min_poll_interval_secs chain
- `src/events/lifecycle.rs` - Added discover_derive import, Derive polling block, step 1b presence check, step 4 absence tracking

## Decisions Made
- Derive poll interval set to 300s (same as Deribit) -- both are options exchanges with similar instrument churn rates
- REST URL construction reuses Deribit pattern: strip ws:// prefix, split on /ws, prepend https://

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- v1.5 Derive venue integration complete -- all four phases (30-33) shipped
- Four-venue discovery, feed, processing, and lifecycle pipeline fully operational

---
*Phase: 33-discovery-and-matching*
*Completed: 2026-03-06*
