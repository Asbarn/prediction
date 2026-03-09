---
phase: 41-signal-engine-generalization
plan: 01
subsystem: signal
tags: [rust, crossasset-engine, venue-generalization, implied-probability]

# Dependency graph
requires:
  - phase: 38-derive-options-feed
    provides: "Derive venue support in PricingEngine and MarketSnapshot"
provides:
  - "ImpliedProbability carries source_venue field from PricingEngine to CrossAssetEngine"
  - "CrossAssetEngine generates signals from any options venue (Deribit or Derive)"
  - "CrossAssetEngine generates signals with single prediction venue (no Kalshi required)"
affects: [signal-pipeline, pricing-engine, event-mapping]

# Tech tracking
tech-stack:
  added: []
  patterns: ["venue-generic signal generation via source_venue propagation"]

key-files:
  created: []
  modified:
    - src/pricing/types.rs
    - src/pricing/engine.rs
    - src/signal/engine.rs

key-decisions:
  - "Keep deribit_taker_fee_rate config name unchanged (Derive fees comparable, cosmetic rename unnecessary for v1.7)"
  - "Dynamic prediction venue iteration from cache keys instead of hardcoded venue list"
  - "Single latest_prob cache key per event_id (not per options venue) -- sufficient for v1.7 single-options-source model"

patterns-established:
  - "source_venue propagation: ImpliedProbability carries origin venue through pipeline"
  - "Dynamic venue iteration: iterate cache keys instead of hardcoding venue lists"

requirements-completed: [SIG-01, SIG-02, SIG-03]

# Metrics
duration: 8min
completed: 2026-03-09
---

# Phase 41 Plan 01: Signal Engine Generalization Summary

**Venue-generic signal generation: ImpliedProbability carries source_venue through pipeline, CrossAssetEngine uses it for registry lookup and signal attribution, prediction venue iteration is dynamic from cache**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T12:49:03Z
- **Completed:** 2026-03-09T12:57:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added source_venue field to ImpliedProbability, populated from snapshot.venue at both PricingEngine construction sites
- Replaced hardcoded Venue::Deribit in CrossAssetEngine with prob.source_venue for registry lookup and signal attribution
- Replaced hardcoded [Venue::Polymarket, Venue::Kalshi] loop with dynamic iteration over cached prediction venues
- Added 3 new tests covering Derive attribution, single-venue operation, and venue-specific registry lookup

## Task Commits

Each task was committed atomically:

1. **Task 1: Add source_venue to ImpliedProbability and wire through production code** - `4fbb352` (feat)
2. **Task 2: Add test coverage for venue-generic signal generation** - `339ce63` (test)

## Files Created/Modified
- `src/pricing/types.rs` - Added source_venue: Venue field to ImpliedProbability struct
- `src/pricing/engine.rs` - Populated source_venue from snapshot.venue at both construction sites (normal + near-expiry)
- `src/signal/engine.rs` - Used prob.source_venue for registry lookup and signal output; dynamic prediction venue iteration; 3 new tests

## Decisions Made
- Kept `deribit_taker_fee_rate` config name unchanged per research recommendation (Derive fees comparable)
- Used dynamic cache-key iteration for prediction venues instead of hardcoded list
- Maintained single `event_id` key for `latest_prob` cache (sufficient for v1.7 single-options-source model)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Signal engine now supports any options venue paired with any prediction venue
- Ready for further venue additions without CrossAssetEngine changes
- EventMapping TOML configs with Derive instruments will now generate correctly attributed signals

---
*Phase: 41-signal-engine-generalization*
*Completed: 2026-03-09*
