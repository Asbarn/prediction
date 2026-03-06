---
phase: 31-derive-feed-and-normalization
plan: 03
subsystem: feed
tags: [websocket, derive, tokio-tungstenite, reconnection, backoff]

# Dependency graph
requires:
  - phase: 31-01
    provides: "DeriveMessage types, channel helpers, DeriveBook"
provides:
  - "DeriveClient WebSocket client with subscribe and raw frame forwarding"
  - "DeriveSupervisor reconnection wrapper with exponential backoff"
affects: [31-04, 32-derive-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: ["60s dead connection timeout (no app heartbeat)", "child token for per-connection cancellation"]

key-files:
  created:
    - src/feed/derive/client.rs
    - src/feed/derive/supervisor.rs
  modified:
    - src/feed/derive/mod.rs

key-decisions:
  - "DeriveClient uses Option<VenueRateLimiter> constructor param (vs Deribit's builder pattern) for simpler API"
  - "Empty instrument list in supervisor triggers 1s sleep+retry loop (avoids connecting with no subscriptions)"

patterns-established:
  - "Derive client: no heartbeat protocol, 60s message timeout for dead connection detection"
  - "Supervisor child_token pattern: each client gets cancel.child_token() so supervisor can drop individual connections"

requirements-completed: [FEED-01, FEED-04]

# Metrics
duration: 7min
completed: 2026-03-04
---

# Phase 31 Plan 03: Derive WebSocket Client and Supervisor Summary

**DeriveClient connects to wss://api.lyra.finance/ws with JSON-RPC subscribe, DeriveSupervisor wraps it with exponential backoff and watch-channel instrument updates**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-04T16:16:58Z
- **Completed:** 2026-03-04T16:23:58Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- DeriveClient connects, subscribes to orderbook+ticker_slim channels, forwards raw text frames via mpsc
- DeriveSupervisor reconnects with exponential backoff, resets on first message, handles dynamic instrument list updates
- Both compile cleanly with all 642 existing tests passing

## Task Commits

Each task was committed atomically:

1. **Task 1: Create DeriveClient WebSocket client** - `39ec94a` (feat)
2. **Task 2: Create DeriveSupervisor reconnection wrapper** - `5023474` (feat)

**Plan metadata:** pending (docs: complete plan)

## Files Created/Modified
- `src/feed/derive/client.rs` - WebSocket client: connect, subscribe, read loop with 60s timeout
- `src/feed/derive/supervisor.rs` - Reconnection wrapper: backoff, watch channel, health reporting
- `src/feed/derive/mod.rs` - Added client and supervisor module declarations

## Decisions Made
- DeriveClient constructor takes `Option<VenueRateLimiter>` directly instead of builder pattern (simpler than Deribit's `with_rate_limiter`)
- Supervisor handles empty instrument list by sleeping 1s and retrying (prevents connecting with no subscriptions)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Client and supervisor ready for 31-04 (processor/normalization wiring)
- DeriveProcessor (in normalize.rs) already exists as untracked file from 31-02, ready to be committed in 31-04

---
*Phase: 31-derive-feed-and-normalization*
*Completed: 2026-03-04*
