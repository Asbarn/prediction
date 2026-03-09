# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.8 Signal Quality Validation -- Phase 47 (Cost Model Validation)

## Current Position

Phase: 47 of 48 (Cost Model Validation)
Plan: 1 of 2 in current phase
Status: Executing phase 47
Last activity: 2026-03-09 -- Completed 47-01 (Cost Validate CLI + Venue Fee Config)

Progress (overall): 8 milestones shipped (v1.0-v1.7), 47 phases, 111 plans complete
Progress (v1.8): [██████████] 100%

## Performance Metrics

**Velocity:**
- Total plans completed: 110
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
- v1.8/46-01: Components sorted by mean magnitude descending for immediate visibility of largest cost drivers
- v1.8/46-01: cost-audit CLI follows same --from/--to/--last/--by-event/--output pattern as spread-analytics
- v1.8/46-02: Depth quality score: fill_ratio_mean * min(depth_levels_mean / 10.0, 1.0) combines fill and depth
- v1.8/46-02: Instruments sorted worst-first so operator immediately sees problem areas
- v1.8/47-01: On-chain costs fold into dollar normalization to avoid breaking CostBreakdown JSONL schema
- v1.8/47-01: Bridge cost defaults to zero (Undocumented) since it depends on operator bridging pattern
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
Stopped at: Completed 47-01-PLAN.md (Cost Validate CLI + Venue Fee Config)
Next action: Execute 47-02-PLAN.md.
