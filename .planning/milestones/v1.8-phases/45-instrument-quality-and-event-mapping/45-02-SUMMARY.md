---
phase: 45-instrument-quality-and-event-mapping
plan: 02
subsystem: events
tags: [events-toml, instrument-mapping, polymarket, deribit, derive, btc, near-the-money]

requires:
  - phase: 45-instrument-quality-and-event-mapping
    provides: "match-audit CLI and Polymarket filtering from plan 01"
provides:
  - "4 active near-the-money BTC instrument mappings in events.toml"
  - "Cross-venue coverage: Polymarket + Deribit + Derive for each mapping"
  - "Operator-verified instrument mappings ready for production"
affects: [46-signal-pipeline, production-deployment]

tech-stack:
  added: []
  patterns: ["events.toml mapping pattern with multi-venue instrument references"]

key-files:
  created: []
  modified:
    - config/events.toml

key-decisions:
  - "Selected 4 BTC strikes: $60K, $65K, $75K, $80K -- covering both puts (below) and calls (above) around ~$68K spot"
  - "Deribit 27MAR26 expiry vs Polymarket end-of-month creates 4-day gap -- acceptable WARN, not ERROR"
  - "All 4 mappings include 3 venues (Polymarket + Deribit + Derive) for maximum cross-venue coverage"

patterns-established:
  - "Event mapping with 3 venues per strike: PM condition_id/token_id + Deribit instrument + Derive instrument"

requirements-completed: [INST-01]

duration: 3min
completed: 2026-03-09
---

# Phase 45 Plan 02: Near-the-Money BTC Instrument Mappings Summary

**4 active BTC instrument mappings ($60K-$80K strikes) with real Polymarket condition IDs matched to Deribit and Derive options, operator-verified**

## Performance

- **Duration:** 3 min (continuation from checkpoint)
- **Started:** 2026-03-09T18:38:57Z
- **Completed:** 2026-03-09T18:42:06Z
- **Tasks:** 2 (1 auto + 1 human-verify checkpoint)
- **Files modified:** 1

## Accomplishments
- Populated events.toml with 4 near-the-money BTC instrument mappings using real Polymarket condition_ids and token_ids from the Gamma API
- Each mapping covers 3 venues (Polymarket, Deribit, Derive) for maximum arbitrage detection surface
- match-audit reports 0 ERRORs across all 4 mappings (4 WARNs for expected expiry gap)
- Operator verified and approved all mappings

## Task Commits

Each task was committed atomically:

1. **Task 1: Populate events.toml with near-the-money BTC mappings** - `fff7b73` (feat)
2. **Task 2: Verify instrument mapping quality** - checkpoint resolved (operator approved)

## Files Created/Modified
- `config/events.toml` - Added 4 BTC event mappings with strikes at $60K, $65K, $75K, $80K; each with Polymarket, Deribit, and Derive venue data; added min_polymarket_price and max_polymarket_spread to discovery config

## Decisions Made
- Selected $60K and $65K as "below"/put strikes, $75K and $80K as "above"/call strikes -- bracketing BTC spot (~$68K) within 10%
- 4-day expiry gap (Deribit 27MAR26 Friday vs Polymarket 31MAR end-of-month) is within the 7-day tolerance and generates WARN not ERROR
- Included Derive as third venue for all mappings since matching instruments were available

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- events.toml now has active instrument mappings; the system will analyze real arbitrage opportunities on next run
- Phase 45 complete -- ready for Phase 46 (signal pipeline validation)
- All mappings approved and validated by match-audit

## Self-Check: PASSED

- FOUND: config/events.toml
- FOUND: 45-02-SUMMARY.md
- FOUND: commit fff7b73
- match-audit: 0 ERRORs, 4 WARNs (expected expiry gap)
- cargo test: all tests pass

---
*Phase: 45-instrument-quality-and-event-mapping*
*Completed: 2026-03-09*
