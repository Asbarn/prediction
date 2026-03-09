# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.7 Prediction Market Signal Pipeline -- Phase 43

## Current Position

Phase: 43 of 43 (E2E Production Verification) -- COMPLETE
Plan: 2 of 2 in current phase (all plans complete)
Status: Phase complete
Last activity: 2026-03-09 -- Completed 43-02 (e2e production verification)

Progress (overall): 7 milestones shipped (v1.0-v1.6), 43 phases, 101 plans complete
Progress (v1.7): [██████████] 100%

## Accumulated Context

### Decisions

- 43-02: arb_signals_emitted_total=0 is expected (negative edge, all filtered by profitability threshold)
- 43-02: Spread logs empty is acceptable (spread logger not actively writing, not a pipeline failure)
- 43-01: Placed signal_logs mount between spread_logs and settlement_logs for consistent ordering
- 42-02: 5-second grace period before WS-to-REST switch (allows supervisor self-recovery via backoff)
- 42-02: WS probe uses separate temporary channel, never sends to snapshot_tx (isolation guarantee)
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
Stopped at: Completed 43-02-PLAN.md -- v1.7 milestone complete
Next action: v1.7 complete. All phases 40-43 shipped. Pipeline operational in production.
