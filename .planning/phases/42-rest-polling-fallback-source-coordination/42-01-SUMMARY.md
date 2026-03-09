---
phase: 42-rest-polling-fallback-source-coordination
plan: 01
subsystem: feed
tags: [polymarket, rest, polling, midpoint, fallback]

requires:
  - phase: 40-polymarket-ws-diagnosis-watchdog
    provides: WS supervisor with data timeout detection
provides:
  - PolymarketRestPoller struct with fetch_midpoint and run methods
  - REST polling config fields (rest_poll_interval_secs, ws_recovery_check_secs, ws_recovery_threshold)
affects: [42-02 source coordinator, polymarket feed pipeline]

tech-stack:
  added: []
  patterns: [REST polling with rate limiting, midpoint-only price fetching]

key-files:
  created:
    - src/feed/polymarket/rest_poller.rs
  modified:
    - src/config/venues.rs
    - src/config/validation.rs
    - src/feed/polymarket/mod.rs
    - tests/pipeline_test.rs

key-decisions:
  - "Midpoint-only REST polling (no /book endpoint) per GitHub #180 stale ghost data issue"
  - "bid=ask=midpoint for REST snapshots since /midpoint provides single price point"

patterns-established:
  - "REST poller pattern: rate-limited polling loop with watch channel for dynamic subscriptions"

requirements-completed: [POLY-04]

duration: 4min
completed: 2026-03-09
---

# Phase 42 Plan 01: REST Polling Client Summary

**Polymarket REST poller fetching /midpoint prices with rate limiting, producing MarketSnapshot values on mpsc channel for WS fallback**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-09T13:12:09Z
- **Completed:** 2026-03-09T13:16:02Z
- **Tasks:** 1
- **Files modified:** 5

## Accomplishments
- Created PolymarketRestPoller with fetch_midpoint and run methods producing MarketSnapshot on mpsc channel
- Added three REST/recovery config fields to PolymarketConfig with serde defaults
- Registered rest_poller module and updated all struct literals across codebase for compilation
- All 648+ existing tests pass, cargo check clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Add REST config fields and create REST poller module** - `66dff85` (feat)

## Files Created/Modified
- `src/feed/polymarket/rest_poller.rs` - PolymarketRestPoller with fetch_midpoint/run methods, MidpointResponse type, metrics instrumentation
- `src/config/venues.rs` - Added rest_poll_interval_secs, ws_recovery_check_secs, ws_recovery_threshold fields with defaults
- `src/config/validation.rs` - Updated PolymarketConfig struct literal with new fields
- `src/feed/polymarket/mod.rs` - Registered rest_poller module
- `tests/pipeline_test.rs` - Updated two PolymarketConfig struct literals with new fields

## Decisions Made
- Used midpoint-only approach (no /book endpoint) per GitHub #180 stale ghost data concern
- Set bid=ask=midpoint for REST snapshots since /midpoint returns a single price point (no spread)
- Empty depth_bids/depth_asks for REST snapshots (REST has no order book depth)
- Sequence counter as AtomicU64 on the struct (consistent with PolymarketProcessor pattern)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- REST poller module ready for Plan 02 to wire into source coordinator
- Config fields ready for TOML configuration
- Module exported and accessible from polymarket feed module

---
*Phase: 42-rest-polling-fallback-source-coordination*
*Completed: 2026-03-09*
