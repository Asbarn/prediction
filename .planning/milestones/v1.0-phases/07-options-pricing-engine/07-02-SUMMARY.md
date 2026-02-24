---
phase: 07-options-pricing-engine
plan: 02
subsystem: pricing
tags: [iv-solver, newton-raphson, brent, implied-volatility, black76, tdd]

# Dependency graph
requires:
  - phase: 07-options-pricing-engine
    provides: "Black-76 pricer (price, vega, intrinsic_value), SolverConfig, SolverResult/SolverMethod types"
provides:
  - "solve_iv function: NR + Brent fallback IV solver for single option prices"
  - "solve_iv_triple function: independent bid/ask/mid IV solving"
  - "Brenner-Subrahmanyam initial guess for fast ATM convergence"
  - "Edge case handling: near-expiry, negative time value, zero price, IV clamping"
  - "near_expiry_cutoff_hours added to SolverConfig"
affects: [07-03-vol-surface, 07-04-probability, 07-05-integration, 08-signal-generation]

# Tech tracking
tech-stack:
  added: []
  patterns: ["NR + Brent fallback for guaranteed IV convergence", "Brenner-Subrahmanyam initial guess", "tracing log points at solver decision points for confidence scoring"]

key-files:
  created:
    - src/pricing/iv_solver.rs
  modified:
    - src/pricing/config.rs
    - src/pricing/mod.rs

key-decisions:
  - "near_expiry_cutoff_hours added to SolverConfig (duplicated from PricingConfig) for solver-level access without requiring full PricingConfig"
  - "Brent fallback uses full [iv_min, iv_max] bracket -- not narrowed from NR progress -- for maximum robustness"
  - "Tracing log points at all solver decision boundaries for downstream confidence scoring analysis"

patterns-established:
  - "TDD cycle: RED (16 failing tests) -> GREEN (implementation) -> REFACTOR (tracing + naming)"
  - "Solver edge cases return early with appropriate metadata rather than attempting partial solving"
  - "Brenner-Subrahmanyam initial guess clamped to [iv_min, iv_max] for safe starting point"

requirements-completed: [PRIC-01, PRIC-02]

# Metrics
duration: 8min
completed: 2026-02-23
---

# Phase 7 Plan 02: Implied Volatility Solver Summary

**Newton-Raphson + Brent fallback IV solver with Brenner-Subrahmanyam initial guess, handling deep OTM/ITM, near-expiry, negative time value, and configurable IV clamping**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-23T14:17:27Z
- **Completed:** 2026-02-23T14:26:13Z
- **Tasks:** 3/3 (TDD: RED, GREEN, REFACTOR)
- **Files modified:** 3

## Accomplishments
- IV solver converges for ATM options in < 10 NR iterations (verified by test)
- Brent's method provides guaranteed convergence when NR fails due to near-zero vega (deep OTM/ITM)
- All edge cases handled gracefully: near-expiry intrinsic pricing, negative time value detection, zero/negative price, IV clamping at configurable bounds
- solve_iv_triple enables independent bid/ask/mid IV solving (individual failures don't block others)
- Full solver metadata (method, iterations, residual, converged) populated for downstream confidence scoring

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): Write failing IV solver tests** - `256a30d` (test)
2. **Task 2 (GREEN): Implement NR + Brent IV solver** - `5bc6718` (feat)
3. **Task 3 (REFACTOR): Add tracing, rename functions** - `0be16f9` (refactor)

## Files Created/Modified
- `src/pricing/iv_solver.rs` - IV solver with solve_iv (NR + Brent) and solve_iv_triple, 16 TDD tests
- `src/pricing/config.rs` - Added near_expiry_cutoff_hours to SolverConfig
- `src/pricing/mod.rs` - Added iv_solver module export

## Decisions Made
- Added `near_expiry_cutoff_hours` to `SolverConfig` (mirrors `PricingConfig` value) so the solver function can check near-expiry without needing the full PricingConfig
- Brent's method uses the full [iv_min, iv_max] bracket rather than narrowing based on NR progress, maximizing robustness for edge cases
- Initial guess function named `brenner_subrahmanyam_guess` for self-documenting code

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added near_expiry_cutoff_hours to SolverConfig**
- **Found during:** Task 2 (GREEN phase implementation)
- **Issue:** The near-expiry cutoff was only in PricingConfig but solve_iv takes SolverConfig. The solver needs this parameter to decide when to bypass NR/Brent and return intrinsic pricing.
- **Fix:** Added `near_expiry_cutoff_hours: f64` field to SolverConfig with default 2.0 (matching PricingConfig)
- **Files modified:** src/pricing/config.rs
- **Verification:** All config tests pass, near_expiry_returns_intrinsic test passes
- **Committed in:** 5bc6718 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical)
**Impact on plan:** Necessary for solver to access near-expiry cutoff. No scope creep -- this is a config field duplication, not an architectural change.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- IV solver ready for vol surface construction (Plan 03) to call solve_iv per instrument
- solve_iv_triple ready for pricing engine integration to produce bid/ask/mid IV simultaneously
- SolverResult metadata (method, iterations, converged, residual) ready for confidence scoring (Plan 05)

## Self-Check: PASSED

- [x] src/pricing/iv_solver.rs exists
- [x] src/pricing/config.rs exists
- [x] src/pricing/mod.rs exists
- [x] Commit 256a30d (RED) exists
- [x] Commit 5bc6718 (GREEN) exists
- [x] Commit 0be16f9 (REFACTOR) exists
- [x] All 16 IV solver tests pass
- [x] All 53 pricing tests pass (no regressions)

---
*Phase: 07-options-pricing-engine*
*Completed: 2026-02-23*
