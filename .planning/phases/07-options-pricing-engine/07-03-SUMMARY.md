---
phase: 07-options-pricing-engine
plan: 03
subsystem: pricing
tags: [vol-surface, interpolation, smile, implied-volatility, quality-filtering]

# Dependency graph
requires:
  - phase: 07-options-pricing-engine
    provides: "PricingConfig with VolSurfaceConfig (min_usable_strikes, good_strike_count, max_iv_spread_filter)"
  - phase: 07-options-pricing-engine
    provides: "SmileQuality enum referenced in types.rs for downstream confidence"
provides:
  - "VolSmile struct with per-expiry IV interpolation, quality filtering, bracket finding"
  - "SmilePoint struct for observed (strike, IV) pairs with bid-ask metadata"
  - "SmileQuality enum (Good/Minimum/Degraded/Empty) for confidence tier decisions"
  - "Linear interpolation + flat extrapolation producing smooth non-negative IV"
  - "nearest_bracket for call spread replication epsilon selection"
  - "skew_at for N(d2) skew adjustment metadata"
affects: [07-04-probability, 07-05-integration, 08-signal-generation]

# Tech tracking
tech-stack:
  added: []
  patterns: ["per-expiry vol smile with linear interpolation", "quality filtering with exclusion logging", "flat extrapolation beyond boundaries"]

key-files:
  created:
    - src/pricing/vol_surface.rs
  modified:
    - src/pricing/mod.rs

key-decisions:
  - "partition_point binary search for O(log n) interpolation between observed strikes"
  - "Flat extrapolation beyond boundary strikes prevents negative or explosive IV"
  - "Degraded quality returns flat ATM vol for any strike (graceful fallback)"
  - "nearest_bracket on exact observed strike returns adjacent strikes (not self-bracket)"

patterns-established:
  - "Quality tier classification: Good (5+), Minimum (3-4), Degraded (<3), Empty (0)"
  - "Exclusion tracking: filtered strikes logged with reasons for data quality visibility"
  - "ATM IV as nearest-to-forward point for skew baseline"

requirements-completed: [PRIC-05]

# Metrics
duration: 6min
completed: 2026-02-23
---

# Phase 7 Plan 03: Vol Surface Summary

**Per-expiry implied volatility smile with linear interpolation, quality filtering, flat extrapolation, and bracket finding for call spread replication**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-23T14:17:37Z
- **Completed:** 2026-02-23T14:23:43Z
- **Tasks:** 2/2
- **Files modified:** 2

## Accomplishments
- VolSmile constructs from raw SmilePoint observations with quality filtering (IV spread and non-positive IV exclusion)
- Linear interpolation between observed strikes with flat extrapolation beyond boundaries produces smooth, non-negative IV
- nearest_bracket identifies surrounding observed strikes for call spread replication epsilon selection
- skew_at computes strike-level skew (strike_iv - atm_iv) for N(d2) adjustment
- Quality tiers (Good/Minimum/Degraded/Empty) with graceful degradation to flat ATM vol
- 18 unit tests covering construction, filtering, interpolation, extrapolation, bracket finding, monotonicity

## Task Commits

Each task was committed atomically:

1. **Task 1: VolSmile construction with quality filtering** - `256a30d` (feat)
2. **Task 2: Linear interpolation, flat extrapolation, and bracket finding** - `e3304c3` (feat)

## Files Created/Modified
- `src/pricing/vol_surface.rs` - VolSmile struct with SmilePoint, SmileQuality, interpolation, nearest_bracket, skew_at
- `src/pricing/mod.rs` - Added `pub mod vol_surface` registration

## Decisions Made
- Used `partition_point` for O(log n) binary search to find surrounding strikes during interpolation
- Flat extrapolation returns boundary IV (first or last observed) rather than returning None -- prevents callers from needing to handle missing IV at extreme strikes
- Degraded quality returns flat ATM vol for any strike, providing a usable (though lower-confidence) fallback
- nearest_bracket on exact observed strike uses adjacent strikes on both sides, not the strike itself -- ensures meaningful epsilon for call spread replication

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Vol surface ready for probability extraction (Plan 04) to price call spreads at non-traded strikes
- nearest_bracket provides epsilon selection for call spread replication: `(C(k_lower) - C(k_upper)) / (k_upper - k_lower)`
- skew_at provides skew metadata for N(d2) skew adjustment: `strike_iv - atm_iv`
- SmileQuality feeds into confidence scoring: Good = full confidence, Degraded = reduced confidence

---
*Phase: 07-options-pricing-engine*
*Completed: 2026-02-23*
