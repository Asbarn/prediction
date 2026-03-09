# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.7 Prediction Market Signal Pipeline -- Phase 41

## Current Position

Phase: 41 of 43 (Signal Engine Generalization) -- COMPLETE
Plan: 1 of 1 in current phase (all plans complete)
Status: Phase Complete
Last activity: 2026-03-09 -- Completed 41-01 (Signal engine generalization)

Progress (overall): 7 milestones shipped (v1.0-v1.6), 41 phases, 95 plans complete
Progress (v1.7): [██░░░░░░░░] 20%

## Accumulated Context

### Decisions

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
Stopped at: Completed 41-01-PLAN.md (Phase 41 complete)
Next action: Begin Phase 42
