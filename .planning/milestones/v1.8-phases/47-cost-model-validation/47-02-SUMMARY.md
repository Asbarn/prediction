---
phase: 47-cost-model-validation
plan: 02
subsystem: analysis
tags: [sensitivity, perturbation, cost-model, cli]

requires:
  - phase: 47-01
    provides: "cost-validate CLI binary and ValidationReport types"
provides:
  - "Perturbation-based sensitivity analysis module ranking cost components by net-edge impact"
  - "cost-validate --sensitivity CLI integration with table and JSON output"
affects: [cost-model-validation, signal-quality]

tech-stack:
  added: []
  patterns: [perturbation-sensitivity-analysis, combined-json-output]

key-files:
  created: [src/analysis/sensitivity.rs]
  modified: [src/analysis/mod.rs, src/bin/cost_validate.rs]

key-decisions:
  - "Default perturbation factors [0.5, 0.75, 1.0, 1.25, 1.5] cover +/-50% range in quarter steps"
  - "Slope computed as finite difference (y_last - y_first) / (factor_last - factor_first) for simplicity"
  - "Sensitivity defaults to last 30 days when no date range specified with --sensitivity"

patterns-established:
  - "Combined CLI output: validation report first, sensitivity analysis below"
  - "Combined JSON envelope: { validation: ..., sensitivity: ... } when multiple analyses"

requirements-completed: [COST-02]

duration: 4min
completed: 2026-03-09
---

# Phase 47 Plan 02: Sensitivity Analysis Summary

**Perturbation-based sensitivity analysis ranking cost components by net-edge impact via slope across 5 scaling factors**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-09T21:58:28Z
- **Completed:** 2026-03-09T22:02:41Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Sensitivity module perturbs each of 6 cost components at 5 factors (0.5x-1.5x) and ranks by |slope|
- CLI integration via --sensitivity flag produces ranked impact tables or combined JSON
- Real data validation: 310 signals show options_fee_estimate as top impact (slope -7.11), prediction_fee second (slope -2.94)
- Graceful handling of missing/empty signal data with clear messages

## Task Commits

Each task was committed atomically:

1. **Task 1: Create sensitivity analysis module** - `3dedd6f` (feat)
2. **Task 2: Integrate sensitivity analysis into cost-validate CLI** - `ac2d245` (feat)

## Files Created/Modified
- `src/analysis/sensitivity.rs` - Perturbation-based sensitivity analysis with SensitivityResult/Report types, 6 unit tests
- `src/analysis/mod.rs` - Register sensitivity module
- `src/bin/cost_validate.rs` - Add --sensitivity, --log-dir, --from/--to/--last flags; combined output modes

## Decisions Made
- Default perturbation factors [0.5, 0.75, 1.0, 1.25, 1.5] cover +/-50% range -- sufficient for ranking without excessive computation
- Slope via finite difference rather than linear regression -- simpler and correct since the relationship is exactly linear
- When --sensitivity is used without explicit date range, defaults to --last 30 rather than erroring

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Cost model validation phase complete (both plans shipped)
- Operator can validate fee parameters and see which cost components impact net edge most
- Ready for next phase

---
*Phase: 47-cost-model-validation*
*Completed: 2026-03-09*
