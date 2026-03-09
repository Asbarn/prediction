---
phase: 40-polymarket-ws-diagnosis-watchdog
plan: 01
subsystem: feed
tags: [polymarket, websocket, diagnostic, config, integration-test]

# Dependency graph
requires: []
provides:
  - "data_timeout_secs config field on PolymarketConfig (120s default)"
  - "Polymarket WS diagnostic integration test (diagnose_polymarket_ws_from_this_host)"
affects: [40-02-watchdog-implementation]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Ignored integration test as diagnostic tool for network-dependent failure modes"

key-files:
  created:
    - tests/polymarket_diag.rs
  modified:
    - src/config/venues.rs
    - config/venues.toml

key-decisions:
  - "30s data timeout for diagnostic test (shorter than 120s config default for fast diagnosis)"
  - "Fetch active token_id from Gamma API at runtime to avoid stale hardcoded IDs"
  - "Use println! instead of tracing for diagnostic output since it is a standalone tool"

patterns-established:
  - "Network diagnostic tests as #[ignore] integration tests runnable from EC2"

requirements-completed: [POLY-01, POLY-03]

# Metrics
duration: 3min
completed: 2026-03-09
---

# Phase 40 Plan 01: Polymarket WS Diagnosis Summary

**data_timeout_secs config field (120s default) and standalone WS diagnostic test covering 5 failure modes**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-09T12:19:19Z
- **Completed:** 2026-03-09T12:22:28Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added `data_timeout_secs: u64` to PolymarketConfig with serde default of 120 seconds
- Created diagnostic integration test that reports WS failure mode: WORKING, CONNECTION_FAILED, SILENT_FREEZE, READ_ERROR, CLOSED_BY_SERVER
- Diagnostic test fetches active token_id from Gamma API at runtime (no stale hardcoded IDs)
- REST /midpoint baseline check validates API reachability independently of WS

## Task Commits

Each task was committed atomically:

1. **Task 1: Add data_timeout_secs to PolymarketConfig and venues.toml** - `494af75` (feat)
2. **Task 2: Create Polymarket WS diagnostic integration test** - `96c1df5` (feat)

## Files Created/Modified
- `src/config/venues.rs` - Added data_timeout_secs field to PolymarketConfig with default_data_timeout_secs() function
- `config/venues.toml` - Added data_timeout_secs = 120 under [polymarket] section
- `tests/polymarket_diag.rs` - Standalone diagnostic test for Polymarket WS failure mode detection

## Decisions Made
- 30-second timeout in diagnostic test (vs 120s config default) for faster manual diagnosis
- Runtime token_id lookup via Gamma API avoids stale placeholder IDs from config
- Used println! instead of tracing since the test is a standalone diagnostic tool, not library code

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- data_timeout_secs field ready for Plan 02's watchdog supervisor implementation
- Diagnostic test can be run from EC2 to characterize the WS failure before implementing reconnection logic

---
*Phase: 40-polymarket-ws-diagnosis-watchdog*
*Completed: 2026-03-09*
