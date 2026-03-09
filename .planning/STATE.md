# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.7 Prediction Market Signal Pipeline -- Phase 40

## Current Position

Phase: 40 of 43 (Polymarket WS Diagnosis and Data Watchdog)
Plan: 1 of 2 in current phase
Status: Executing
Last activity: 2026-03-09 -- Completed 40-01 (Polymarket WS diagnosis and config)

Progress (overall): 7 milestones shipped (v1.0-v1.6), 39 phases, 92 plans complete
Progress (v1.7): [█░░░░░░░░░] 10%

## Accumulated Context

### Decisions

- 40-01: 30s timeout in diagnostic test (vs 120s config default) for faster manual diagnosis
- 40-01: Runtime Gamma API token_id lookup avoids stale placeholder IDs
- Decisions also logged in PROJECT.md Key Decisions table.

### Pending Todos

None.

### Blockers/Concerns

- Polymarket WS "Connection reset by peer" from EC2 us-east-1 -- Phase 40 will diagnose
- Polymarket WS silent freeze (GitHub #292) -- server-side issue, may not be fixable
- REST `/book` endpoint returns stale ghost data (GitHub #180) -- use `/price` or `/midpoint` instead
- CrossAssetEngine hardcoded to Venue::Deribit -- Phase 41 will fix (2-line change)

## Session Continuity

Last session: 2026-03-09
Stopped at: Completed 40-01-PLAN.md
Next action: Execute 40-02-PLAN.md
