---
phase: 44-critical-bug-fixes-and-data-pipeline-repair
plan: 01
subsystem: spread, signal
tags: [cost-model, kalshi, fees, ceiling-rounding, unit-normalization]

# Dependency graph
requires: []
provides:
  - "Cents-precision Kalshi fee ceiling rounding in cost_model.rs"
  - "Probability-space cost normalization in signal engine and spread engine"
affects: [45-data-pipeline, 46-signal-quality, 47-regression, 48-deployment]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Dollar costs divided by target_notional before subtraction from probability-space spreads"

key-files:
  created: []
  modified:
    - src/spread/cost_model.rs
    - src/signal/engine.rs
    - src/spread/engine.rs

key-decisions:
  - "Cents-precision ceiling: multiply by 100, ceil, divide by 100 -- matches Kalshi's actual rounding"
  - "Normalization divides (fee + carry) by target_notional to convert dollars to probability units"

patterns-established:
  - "Cost normalization: always divide dollar-denominated costs by target_notional before combining with probability-space values"

requirements-completed: [FIX-01, FIX-02]

# Metrics
duration: 4min
completed: 2026-03-09
---

# Phase 44 Plan 01: Cost Model Arithmetic Fixes Summary

**Fixed Kalshi fee ceiling from integer to cents rounding (57x overstatement) and normalized dollar costs to probability space in both engines (eliminated -19.5 net_edge bug)**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-09T17:36:38Z
- **Completed:** 2026-03-09T17:40:54Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Kalshi taker fee at p=0.25 now correctly computes to $0.02, not $1.00 (57x reduction)
- Signal engine net_edge values now in correct ~0.01-0.10 range instead of ~-19.5
- Spread engine net_spread uses identical normalization for consistency
- All 649 tests pass with no regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix Kalshi fee ceiling rounding to cents precision** - `5ac8f93` (fix)
2. **Task 2: Normalize dollar costs to probability space in both engines** - `bbbd0ca` (fix)

## Files Created/Modified
- `src/spread/cost_model.rs` - Cents-precision ceiling: (raw * 100).ceil() / 100 instead of raw.ceil()
- `src/signal/engine.rs` - Divide (prediction_fee + options_fee_estimate + carry) by target_notional
- `src/spread/engine.rs` - Divide (buy_fee + sell_fee + carry) by target_notional

## Decisions Made
- Used (raw * 100).ceil() / 100 pattern for cents rounding -- matches Kalshi's documented rounding behavior
- Added debug_assert for positive target_notional as safety guard in both engines
- Kept basis_risk_premium outside normalization since it is already in probability space

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Cost model arithmetic is correct; signals will now show realistic edge values
- Ready for Phase 44 Plan 02 (spread logger fixes) or Phase 45 (data pipeline)
- Production deployment will show immediate improvement in signal quality

---
*Phase: 44-critical-bug-fixes-and-data-pipeline-repair*
*Completed: 2026-03-09*
