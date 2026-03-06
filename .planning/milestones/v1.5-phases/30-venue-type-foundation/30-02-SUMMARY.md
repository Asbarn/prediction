---
phase: 30-venue-type-foundation
plan: 02
subsystem: feed
tags: [derive, websocket, api-probe, live-verification, json-rpc]

# Dependency graph
requires:
  - phase: 30-venue-type-foundation (plan 01)
    provides: Venue::Derive enum variant and config structs
provides:
  - Confirmed Derive WebSocket channel subscription format (orderbook and ticker_slim)
  - Confirmed snapshot-only book update model (no delta reconciliation needed)
  - Confirmed WS-level PING/PONG heartbeat (no application-level heartbeat handler needed)
  - Confirmed no authentication required for public channels (k256 deferred to v2)
  - Reusable integration test for Derive API connectivity
  - Documented ticker_slim field mapping for Phase 31 parser
affects: [31-derive-feed-normalization, 32-pipeline-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: [live-api-probe-before-implementation]

key-files:
  created:
    - tests/derive_api_probe.rs
    - .planning/phases/30-venue-type-foundation/DERIVE-API-FINDINGS.md
  modified: []

key-decisions:
  - "ticker channel is deprecated; must use ticker_slim with abbreviated single-letter keys"
  - "Book model is snapshot-only (no delta logic needed, simplifies feed vs Deribit)"
  - "No k256/auth dependency for v1.5 (public channels work without authentication)"
  - "Prices and amounts are strings in Derive API (parser must convert to Decimal)"

patterns-established:
  - "Live API probe pattern: create ignored integration test, run manually, document findings before implementation"

requirements-completed: [PIPE-01, PIPE-02]

# Metrics
duration: 15min
completed: 2026-03-04
---

# Phase 30 Plan 02: Derive API Live Probe Summary

**Live WebSocket probe against Derive production API confirming channel format (orderbook + ticker_slim), snapshot-only book model, WS PING/PONG heartbeat, and no-auth requirement for public channels**

## Performance

- **Duration:** ~15 min (across two sessions with checkpoint)
- **Started:** 2026-03-04
- **Completed:** 2026-03-04
- **Tasks:** 2
- **Files created:** 2

## Accomplishments
- Connected to Derive production WebSocket and captured 30+ messages in 7 seconds
- Confirmed all four API unknowns that were at LOW confidence from documentation alone
- Documented exact channel formats, field mappings, and key differences from Deribit
- Discovered ticker channel deprecation (must use ticker_slim) -- would have caused errors in Phase 31

## Task Commits

Each task was committed atomically:

1. **Task 1: Create and run Derive API probe test** - `231d69d` (feat)
2. **Task 2: Document API findings** - `2e76a58` (docs)

## Files Created/Modified
- `tests/derive_api_probe.rs` - Integration test connecting to Derive WS, subscribing to orderbook and ticker channels, capturing raw messages
- `.planning/phases/30-venue-type-foundation/DERIVE-API-FINDINGS.md` - Comprehensive API findings with sample messages, field tables, and Phase 31 implementation guidance

## Decisions Made
- **ticker_slim over ticker**: The `ticker` channel is deprecated on Derive; `ticker_slim` uses abbreviated single-letter keys (A/a/B/b/I/M etc.)
- **No delta reconciliation**: Book model is snapshot-only with ~100ms updates, eliminating the snapshot+delta complexity present in Deribit client
- **No k256 dependency**: Public channels (orderbook, ticker_slim) require no authentication; k256 deferred to v2 private channel scope
- **String price parsing required**: All prices/amounts arrive as strings, unlike Deribit's numeric format

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All API unknowns resolved at CONFIRMED confidence -- Phase 31 can implement Derive feed without guesswork
- Channel formats, field mappings, and sample messages documented for direct reference
- Key simplifications identified: no delta logic, no heartbeat handler, no auth setup
- USDC price normalization remains the primary new logic challenge for Phase 31

## Self-Check: PASSED

- FOUND: tests/derive_api_probe.rs
- FOUND: .planning/phases/30-venue-type-foundation/DERIVE-API-FINDINGS.md
- FOUND: .planning/phases/30-venue-type-foundation/30-02-SUMMARY.md
- FOUND: commit 231d69d (Task 1)
- FOUND: commit 2e76a58 (Task 2)

---
*Phase: 30-venue-type-foundation*
*Completed: 2026-03-04*
