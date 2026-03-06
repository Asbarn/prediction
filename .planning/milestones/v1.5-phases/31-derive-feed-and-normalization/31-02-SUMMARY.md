---
phase: 31-derive-feed-and-normalization
plan: 02
subsystem: pricing
tags: [instrument-parser, usdc, derive, normalization, pricing-engine]

requires:
  - phase: 30-venue-type-foundation
    provides: "Venue::Derive variant and live API findings (instrument format, USDC denomination)"
provides:
  - "parse_derive_instrument() for BTC-YYYYMMDD-STRIKE-C/P format"
  - "PricingEngine venue-gated price conversion (Deribit BTC-inverse vs Derive USDC pass-through)"
affects: [31-derive-feed-and-normalization, pricing, derive-feed]

tech-stack:
  added: []
  patterns: [venue-gated-price-conversion, dual-instrument-parser-routing]

key-files:
  created: []
  modified:
    - src/pricing/instrument.rs
    - src/pricing/engine.rs

key-decisions:
  - "Keep _btc variable names in engine.rs with comment explaining misnomer for Derive (minimal diff over rename)"
  - "process_near_expiry does not need venue gating (uses forward for intrinsic comparison, not price conversion)"

patterns-established:
  - "Venue-aware parser routing: match on snapshot.venue to select instrument parser"
  - "Venue-gated price conversion: Deribit multiplies by forward, Derive passes through"

requirements-completed: [NORM-01, NORM-02]

duration: 5min
completed: 2026-03-04
---

# Phase 31 Plan 02: Instrument Parser and Price Normalization Summary

**Derive instrument parser (YYYYMMDD format) and venue-gated USDC price normalization in PricingEngine**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-04T16:04:19Z
- **Completed:** 2026-03-04T16:09:13Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added parse_derive_instrument() alongside existing parse_deribit_instrument() with cross-format rejection
- PricingEngine now routes to venue-specific parser and gates BTC-inverse price conversion
- 8 new unit tests for Derive parser including cross-format rejection and malformed input handling
- All 88 pricing tests pass (14 instrument + 74 existing)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Derive instrument name parser** - `5005258` (feat)
2. **Task 2: Gate PricingEngine for Derive USDC snapshots** - `aa095cd` (feat)

## Files Created/Modified
- `src/pricing/instrument.rs` - Added parse_derive_instrument() for BTC-YYYYMMDD-STRIKE-C/P format with 8 unit tests
- `src/pricing/engine.rs` - Venue-aware parser routing and USDC price pass-through for Derive snapshots

## Decisions Made
- Kept `_btc` variable naming in engine.rs with explanatory comment rather than renaming (minimal diff, clear comment)
- Confirmed process_near_expiry() does not need venue gating -- it uses forward for intrinsic value comparison, not price conversion

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Derive instrument parsing and price normalization ready for feed integration
- PricingEngine accepts both Deribit and Derive snapshots with correct price handling
- Ready for Plan 03 (Derive WebSocket feed client) to start producing Derive MarketSnapshots

## Self-Check: PASSED

- [x] src/pricing/instrument.rs exists
- [x] src/pricing/engine.rs exists
- [x] 31-02-SUMMARY.md exists
- [x] Commit 5005258 found
- [x] Commit aa095cd found
- [x] All 88 pricing tests pass

---
*Phase: 31-derive-feed-and-normalization*
*Completed: 2026-03-04*
