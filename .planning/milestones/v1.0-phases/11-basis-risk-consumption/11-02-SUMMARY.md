---
phase: 11-basis-risk-consumption
plan: 02
subsystem: signal
tags: [basis-risk, spread-engine, signal-engine, cost-model, settlement-risk, near-expiry]

# Dependency graph
requires:
  - phase: 11-basis-risk-consumption
    provides: BasisRiskCache type, CachedRiskInfo, new_basis_risk_cache(), lifecycle cache population, basis_risk_scale config
  - phase: 08-cross-asset-signal-generation
    provides: CrossAssetEngine with CostBreakdown and threshold evaluation
  - phase: 06-prediction-market-spreads
    provides: SpreadEngine with SpreadResult and cost model
provides:
  - BasisRiskCache consumed by SpreadEngine (premium in total_cost)
  - BasisRiskCache consumed by CrossAssetEngine (premium in total_cost + near-expiry threshold inflation)
  - basis_risk_premium field in SpreadResult JSONL output
  - basis_risk_premium field in CostBreakdown (ArbSignal JSONL output)
  - Near-expiry threshold inflation via ExpiryWarning.risk_inflation_factor
  - Replay/mock mode pre-population of BasisRiskCache from EventRegistry
affects: [v1.0-milestone-complete]

# Tech tracking
tech-stack:
  added: []
  patterns: [builder-pattern-for-optional-cache, non-blocking-try-read-for-cache-access, replay-pre-populate-from-registry]

key-files:
  created: []
  modified:
    - src/spread/patterns.rs
    - src/spread/engine.rs
    - src/signal/types.rs
    - src/signal/engine.rs
    - src/main.rs
    - src/signal/logger.rs
    - src/spread/logger.rs
    - src/paper_trade/aggregator.rs
    - src/paper_trade/position.rs
    - src/paper_trade/tracker.rs
    - tests/schema_golden_test.rs

key-decisions:
  - "BasisRiskCache shared across all modes: created before pipeline, passed to lifecycle manager (live) and both engines"
  - "Non-blocking try_read() on cache: returns zero premium if lock is contended (never blocks engine hot path)"
  - "Replay/mock pre-populate uses compute_risk_for_mapping + check_expiry_warning from EventRegistry active_approved()"
  - "Near-expiry threshold inflation shadows threshold_value (multiplied by risk_inflation_factor) before status check"
  - "basis_risk_premium uses serde(default) for backward-compatible deserialization of existing JSONL logs"

patterns-established:
  - "Builder pattern for optional shared state: with_basis_risk_cache() returns Self, defaults to None"
  - "Non-blocking cache read: try_read() with zero-value fallback for contended locks"
  - "Replay pre-populate: iterate active_approved() mappings and insert CachedRiskInfo before engine start"

requirements-completed: [SGNL-02, EVNT-02, EVNT-03, EVNT-05]

# Metrics
duration: 10min
completed: 2026-02-24
---

# Phase 11 Plan 02: BasisRiskCache Engine Consumption Summary

**BasisRiskCache wired into SpreadEngine and CrossAssetEngine cost models with basis_risk_premium in JSONL output and near-expiry threshold inflation**

## Performance

- **Duration:** 10 min
- **Started:** 2026-02-24T13:34:02Z
- **Completed:** 2026-02-24T13:43:48Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Wired BasisRiskCache into SpreadEngine: lookup_basis_risk_premium adds settlement risk premium to total_cost
- Wired BasisRiskCache into CrossAssetEngine: premium in cost model + near-expiry threshold inflation via ExpiryWarning
- Added basis_risk_premium field to SpreadResult and CostBreakdown with serde(default) for backward compat
- main.rs creates shared BasisRiskCache, pre-populates for replay/mock, passes to lifecycle manager and both engines
- All 354 lib tests + 22 integration tests + 3 doc tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add basis_risk_premium to output types and wire cache into engines** - `08d7b28` (feat)
2. **Task 2: Wire BasisRiskCache in main.rs and pre-populate for replay** - `8cec30a` (feat)

## Files Created/Modified
- `src/spread/patterns.rs` - Added basis_risk_premium field to SpreadResult, updated JSONL schema docs
- `src/spread/engine.rs` - Added BasisRiskCache field, with_basis_risk_cache() builder, lookup_basis_risk_premium(), premium in total_cost
- `src/signal/types.rs` - Added basis_risk_premium field to CostBreakdown
- `src/signal/engine.rs` - Added BasisRiskCache field, with_basis_risk_cache() builder, lookup_basis_risk_premium(), lookup_expiry_threshold_inflation(), premium in total_cost, threshold inflation
- `src/main.rs` - Shared BasisRiskCache creation, replay/mock pre-populate, passed to lifecycle manager and both engines
- `src/signal/logger.rs` - Updated test constructor for new basis_risk_premium field
- `src/spread/logger.rs` - Updated test constructor for new basis_risk_premium field
- `src/paper_trade/aggregator.rs` - Updated test constructor for new basis_risk_premium field
- `src/paper_trade/position.rs` - Updated test constructor for new basis_risk_premium field
- `src/paper_trade/tracker.rs` - Updated test constructor for new basis_risk_premium field
- `tests/schema_golden_test.rs` - Updated integration test constructors for new fields

## Decisions Made
- BasisRiskCache created as shared instance before pipeline start, available to all modes (not just live)
- Non-blocking try_read() on cache returns zero premium if lock is contended, ensuring engine hot path is never blocked
- Replay/mock mode pre-populates cache from EventRegistry active_approved() mappings since lifecycle manager doesn't run
- Near-expiry threshold inflation shadows the original threshold_value via let-rebinding (threshold_value = threshold_value * expiry_inflation)
- basis_risk_premium field uses #[serde(default)] so existing JSONL logs can still be deserialized

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated test constructors across 6 additional files for new basis_risk_premium fields**
- **Found during:** Task 1 (adding basis_risk_premium to SpreadResult and CostBreakdown)
- **Issue:** 5 test files and 1 integration test construct SpreadResult/CostBreakdown literals and lacked the new required field
- **Fix:** Added `basis_risk_premium: dec("0")` to all constructors in paper_trade/aggregator.rs, paper_trade/position.rs, paper_trade/tracker.rs, signal/logger.rs, spread/logger.rs, and tests/schema_golden_test.rs
- **Files modified:** src/paper_trade/aggregator.rs, src/paper_trade/position.rs, src/paper_trade/tracker.rs, src/signal/logger.rs, src/spread/logger.rs, tests/schema_golden_test.rs
- **Verification:** `cargo test` -- all 379 tests pass
- **Committed in:** 08d7b28 (Task 1) and 8cec30a (Task 2 for schema_golden_test.rs)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Auto-fix was necessary for compilation. Plan listed 4 files but 11 files total needed the new field. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 11 is complete: BasisRiskCache is created, populated (live via lifecycle, replay/mock via pre-populate), and consumed by both engines
- basis_risk_premium appears in all JSONL output for post-hoc analysis
- Near-expiry events get inflated thresholds for more conservative signal generation
- v1.0 milestone requirements SGNL-02, EVNT-02, EVNT-03, EVNT-05 are fulfilled

## Self-Check: PASSED

- All key files exist (5/5 verified)
- Both commits verified in git log (08d7b28, 8cec30a)
- SUMMARY.md created at expected path

---
*Phase: 11-basis-risk-consumption*
*Completed: 2026-02-24*
