# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-03)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.5 Derive.xyz Venue Integration -- Phase 33 in progress

## Current Position

Phase: 33 of 33 (Discovery and Matching)
Plan: 1 of 1 in current phase (COMPLETE)
Status: Phase 33 Complete
Last activity: 2026-03-06 -- Completed 33-01 Derive discovery and cross-venue matching

Progress (v1.5): [##########] 100%
Progress (overall): 5 milestones shipped (v1.0-v1.4), 30 phases, 79 plans

## Performance Metrics

**Velocity:**
- Total plans completed: 77
- Total phases completed: 30
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
| Phase 31 P01 | 7min | 2 tasks | 5 files |
| Phase 31 P02 | 5min | 2 tasks | 2 files |
| Phase 31 P03 | 7min | 2 tasks | 3 files |
| Phase 31 P04 | 8min | 2 tasks | 3 files |
| Phase 32 P01 | 7min | 2 tasks | 4 files |
| Phase 32 P02 | 5min | 2 tasks | 3 files |
| Phase 33 P01 | 4min | 2 tasks | 1 files |

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
- [Phase 31]: DeriveBook uses Decimal::from_str (not f64) for string price precision
- [Phase 31]: DeriveMessage has 2 variants only (no heartbeat -- WS PING/PONG)
- [Phase 31]: Venue-aware parser routing in PricingEngine (match on snapshot.venue)
- [Phase 31]: Venue-gated price conversion: Deribit BTC-inverse (price*forward), Derive USDC pass-through
- [Phase 31]: process_near_expiry does not need venue gating (forward used for intrinsic, not price conversion)
- [Phase 31]: DeriveClient uses Option<VenueRateLimiter> constructor param (simpler than Deribit builder pattern)
- [Phase 31]: Empty instrument list in supervisor triggers 1s sleep+retry (avoids connecting with no subscriptions)
- [Phase 31]: DeriveProcessor dual-source gating: snapshot requires both book AND ticker data
- [Phase 31]: USDC prices pass through without conversion (no BTC-inverse transform needed)
- [Phase 31]: Stale Derive data skips snapshot emission entirely (not emitted with is_stale flag)
- [Phase 32]: Derive follows identical Deribit pattern for subscription management (HashSet diff, sorted Vec send, gauge+counter metrics)
- [Phase 32]: Pipeline.rs extracts _derive_rx with underscore prefix (supervisor wiring in Plan 02)
- [Phase 32]: Derive pipeline follows identical Deribit 7-step block pattern (health, cancel, recording, rate-limiter, supervisor, processor, forward)
- [Phase 32]: feed_reconnections_total is venue-generic counter (not Derive-specific), benefits all 4 venues
- [Phase 33]: Derive discovery uses POST method (not GET) per API requirement (405 on GET)
- [Phase 33]: String strikes parsed via Decimal::from_str for precision (not f64)
- [Phase 33]: Epoch expiry auto-detects seconds vs milliseconds (threshold 10 billion)

### Pending Todos

None.

### Blockers/Concerns

None.

## Session Continuity

Last session: 2026-03-06
Stopped at: Completed 33-01-PLAN.md (Derive discovery and cross-venue matching)
Next action: v1.5 integration complete -- all phases done
