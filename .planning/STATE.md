# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.7 Prediction Market Signal Pipeline -- Phase 42

## Current Position

Phase: 42 of 43 (REST Polling Fallback & Source Coordination) -- IN PROGRESS
Plan: 1 of 2 in current phase (42-01 complete)
Status: Executing
Last activity: 2026-03-09 -- Completed 42-01 (REST polling client)

Progress (overall): 7 milestones shipped (v1.0-v1.6), 41 phases, 96 plans complete
Progress (v1.7): [███░░░░░░░] 25%

## Accumulated Context

### Decisions

- 42-01: Midpoint-only REST polling (no /book endpoint) per GitHub #180 stale ghost data issue
- 42-01: bid=ask=midpoint for REST snapshots since /midpoint provides single price point
- 41-01: Keep deribit_taker_fee_rate config name unchanged (Derive fees comparable, cosmetic rename unnecessary for v1.7)
- 41-01: Dynamic prediction venue iteration from cache keys instead of hardcoded venue list
- 41-01: Single latest_prob cache key per event_id (sufficient for v1.7 single-options-source model)
- Decisions also logged in PROJECT.md Key Decisions table.

### Pending Todos

None.

### Blockers/Concerns

- Polymarket WS "Connection reset by peer" from EC2 us-east-1 -- Phase 40 will diagnose
- Polymarket WS silent freeze (GitHub #292) -- server-side issue, may not be fixable
- REST `/book` endpoint returns stale ghost data (GitHub #180) -- use `/price` or `/midpoint` instead
- CrossAssetEngine venue hardcoding RESOLVED in Phase 41

## Session Continuity

Last session: 2026-03-09
Stopped at: Completed 42-01-PLAN.md
Next action: Execute 42-02-PLAN.md (source coordinator)
