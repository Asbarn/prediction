# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.8 Signal Quality Validation -- Phase 45 (Instrument Quality and Event Mapping)

## Current Position

Phase: 45 of 48 (Instrument Quality and Event Mapping) -- COMPLETE
Plan: 2 of 2 in current phase (all complete)
Status: Phase 45 complete, ready for Phase 46
Last activity: 2026-03-09 -- Completed 45-02 (Near-the-Money BTC Instrument Mappings)

Progress (overall): 8 milestones shipped (v1.0-v1.7), 45 phases, 107 plans complete
Progress (v1.8): [████░░░░░░] 40%

## Performance Metrics

**Velocity:**
- Total plans completed: 105
- Total execution time: 8 milestones across 18 days
- Average: ~5.6 plans/day

**Recent Trend:**
- v1.7: 7 plans in 1 day
- Trend: Stable

*Updated after each plan completion*

## Accumulated Context

### Decisions

- v1.7/43-02: arb_signals_emitted_total=0 is expected (negative edge, all filtered by profitability threshold)
- v1.7/43-02: Spread logs empty is acceptable for v1.7 (spread logger fix deferred to v1.8)
- v1.8 research: Unit mismatch in cost subtraction confirmed as primary cause of -19.5 net_edge
- v1.8 research: Kalshi fee ceiling rounds to integers instead of cents (up to 57x overstatement)
- v1.8 research: events.toml empty in production -- historical signals from deep OTM strikes
- v1.8 research: One new dependency only (linregress = "0.5" for OLS regression)
- v1.8/44-01: Cents-precision ceiling (raw*100).ceil()/100 matches Kalshi's actual rounding
- v1.8/44-01: Dollar costs normalized by dividing by target_notional before probability-space subtraction
- v1.8/44-02: Cross-asset SpreadPattern variants separate from Kalshi variants to isolate SpreadEngine behavior
- v1.8/44-02: SpreadResult fee fields divided by target_notional for probability-space consistency
- v1.8/45-01: Gamma API returns bestBid/bestAsk/spread as JSON strings; custom serde deserializer handles string/number/null
- v1.8/45-01: Filter predicate extracted as testable helper for unit testing without async runtime
- v1.8/45-01: match-audit parses Deribit (DDMMMYY) and Derive (YYYYMMDD) expiry formats independently
- v1.8/45-02: Selected 4 BTC strikes ($60K, $65K, $75K, $80K) covering puts and calls around ~$68K spot
- v1.8/45-02: 4-day expiry gap (Deribit Friday vs Polymarket end-of-month) acceptable as WARN
- v1.8/45-02: All mappings include 3 venues (Polymarket + Deribit + Derive) for maximum coverage
- Decisions also logged in PROJECT.md Key Decisions table.

### Pending Todos

None.

### Blockers/Concerns

- Spread logger not producing output (spread_logs empty) -- FIXED in 44-02
- All signals show negative edge (-19.5) due to unit mismatch -- FIXED in 44-01
- events.toml empty in production -- FIXED in 45-02 (4 active BTC mappings)
- GitLab CI/CD minutes exhausted -- deploy manually via SSM

## Session Continuity

Last session: 2026-03-09
Stopped at: Completed 45-02-PLAN.md (Near-the-Money BTC Instrument Mappings)
Next action: Continue with Phase 46
