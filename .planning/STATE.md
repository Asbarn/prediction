# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-03)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.5 Derive.xyz Venue Integration -- Phase 30 complete, ready for Phase 31

## Current Position

Phase: 30 of 33 (Venue Type Foundation) -- COMPLETE
Plan: 2 of 2 in current phase (all plans complete)
Status: Phase complete -- ready for Phase 31
Last activity: 2026-03-04 -- Completed 30-02 Derive API live probe

Progress (v1.5): [##........] 20%
Progress (overall): 5 milestones shipped (v1.0-v1.4), 29 phases, 71 plans

## Performance Metrics

**Velocity:**
- Total plans completed: 69
- Total phases completed: 29
- Total execution time: ~10 days across 5 milestones

**By Milestone:**

| Milestone | Phases | Plans | Timeline |
|-----------|--------|-------|----------|
| v1.0 | 13 | 36 | 4 days |
| v1.1 | 4 | 11 | 5 days |
| v1.2 | 4 | 8 | 2 days |
| v1.3 | 4 | 7 | 2 days |
| v1.4 | 4 | 7 | 1 day |
| Phase 30 P01 | 21min | 2 tasks | 14 files |
| Phase 30 P02 | 15min | 2 tasks | 2 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- v1.5: Derive replaces Kalshi as active third data source (Kalshi inaccessible from Poland)
- v1.5: Copy-and-adapt Deribit feed stack pattern (6 new files, 5 modified files, 0 downstream changes)
- v1.5: USDC price normalization required (Derive linear/USDC vs Deribit inverse/BTC denomination)
- v1.5: Live API verification before implementation (channel names, book model, auth requirement all LOW confidence)
- [Phase 30]: Derive follows Deribit pattern for options venue (zero fee, 08:00 UTC expiry, DeribitDelivery resolution)
- [Phase 30]: ticker channel deprecated on Derive; must use ticker_slim with abbreviated keys
- [Phase 30]: Derive book model is snapshot-only (~100ms updates); no delta reconciliation needed
- [Phase 30]: No k256/auth dependency for v1.5; public channels work without authentication
- [Phase 30]: Derive prices/amounts are strings; parser must convert to Decimal

### Pending Todos

None.

### Blockers/Concerns

- USDC-to-BTC price normalization is new logic with no precedent in codebase -- correctness gate required

## Session Continuity

Last session: 2026-03-04
Stopped at: Completed 30-02-PLAN.md (Derive API live probe -- Phase 30 complete)
Next action: Plan and execute Phase 31 (Derive Feed and Normalization)
