---
phase: 42-rest-polling-fallback-source-coordination
plan: 02
subsystem: feed
tags: [polymarket, coordinator, ws-rest-switching, state-machine, fallback]

requires:
  - phase: 42-rest-polling-fallback-source-coordination
    provides: PolymarketRestPoller for REST fallback mode
  - phase: 40-polymarket-ws-diagnosis-watchdog
    provides: WS supervisor with data timeout detection and VenueHealth
provides:
  - SourceCoordinator state machine with exclusive WS/REST mode switching
  - Pipeline integration replacing direct supervisor spawn
  - WS recovery probe mechanism for automatic fallback recovery
  - Prometheus metrics for source mode and switch counts
affects: [polymarket feed pipeline, monitoring dashboards, operational runbooks]

tech-stack:
  added: []
  patterns: [exclusive-mode state machine, WS recovery probe with temporary channel]

key-files:
  created:
    - src/feed/polymarket/coordinator.rs
  modified:
    - src/feed/polymarket/mod.rs
    - src/feed/polymarket/rest_poller.rs
    - src/feed/pipeline.rs
    - config/venues.toml

key-decisions:
  - "5-second grace period before WS-to-REST switch (allows supervisor self-recovery via backoff)"
  - "WS probe uses separate temporary channel, never sends to snapshot_tx (isolation guarantee)"

patterns-established:
  - "Source coordinator pattern: state machine managing exclusive data source switching with probe-based recovery"

requirements-completed: [POLY-04, POLY-05]

duration: 5min
completed: 2026-03-09
---

# Phase 42 Plan 02: Source Coordinator Summary

**SourceCoordinator state machine managing exclusive WS/REST switching with probe-based WS recovery and Prometheus mode metrics**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-09T13:18:08Z
- **Completed:** 2026-03-09T13:23:17Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Created SourceCoordinator with SourceMode enum (WebSocket/Rest) and full state machine loop
- Implemented WS recovery probe using temporary client and separate channel (never pollutes snapshot stream)
- Wired coordinator into pipeline replacing direct PolymarketSupervisor+Processor spawn
- Added Prometheus metrics: feed_source_mode gauge (0=WS, 1=REST) and feed_source_switches_total counter

## Task Commits

Each task was committed atomically:

1. **Task 1: Create SourceCoordinator state machine** - `4c2ffcd` (feat)
2. **Task 2: Wire SourceCoordinator into pipeline and update config** - `6e721bb` (feat)

## Files Created/Modified
- `src/feed/polymarket/coordinator.rs` - SourceCoordinator with SourceMode enum, WS/REST mode arms, probe_ws_recovery, exclusive-mode guarantee
- `src/feed/polymarket/mod.rs` - Registered coordinator module
- `src/feed/polymarket/rest_poller.rs` - Fixed fetch_midpoint return type to Box<dyn Error + Send + Sync>
- `src/feed/pipeline.rs` - Replaced direct supervisor+processor spawn with SourceCoordinator::new
- `config/venues.toml` - Documented REST polling config fields (commented defaults)

## Decisions Made
- 5-second grace period before WS-to-REST switch allows supervisor to self-recover via its own backoff mechanism
- WS probe uses separate temporary client and channel -- never sends to the main snapshot channel
- Coordinator passes snapshot_tx clone to REST poller for direct writes (no intermediate channel needed)
- Health monitoring via periodic 1-second polling in WS mode (checks is_available + connection_count > 0)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed rest_poller fetch_midpoint return type for Send bound**
- **Found during:** Task 1 (coordinator compilation)
- **Issue:** `Box<dyn std::error::Error>` is not `Send`, preventing `tokio::spawn` of REST poller
- **Fix:** Changed return type to `Box<dyn std::error::Error + Send + Sync>`
- **Files modified:** src/feed/polymarket/rest_poller.rs
- **Verification:** cargo check passes
- **Committed in:** 4c2ffcd (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Required for correctness of async spawn. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Source coordinator fully integrated -- Polymarket feed now has automatic WS/REST fallback
- REST polling config fields available for operator tuning in venues.toml
- Prometheus metrics ready for Grafana dashboard panels (feed_source_mode, feed_source_switches_total)

---
*Phase: 42-rest-polling-fallback-source-coordination*
*Completed: 2026-03-09*
