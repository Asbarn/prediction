---
phase: 28-signal-scoring-cli
plan: 01
subsystem: analysis
tags: [statistics, scoring, sharpe-ratio, psr, drawdown, t-test, wilson-ci, statrs]

# Dependency graph
requires:
  - phase: 26-analysis-foundation
    provides: "stats.rs with mean_f64, stddev_f64, wilson_ci; output.rs; analysis::io"
provides:
  - "scoring.rs with five pure computation functions (hit rates, edge t-test, Sharpe, PSR, max drawdown)"
  - "ScoringResult composite struct assembling all scoring computations"
  - "skewness_f64 and kurtosis_f64 in stats.rs"
  - "AnalysisSettlementRecord with Deserialize for JSONL loading"
affects: [28-02-signal-scoring-cli-wiring, signal-scoring-binary]

# Tech tracking
tech-stack:
  added: [statrs (StudentsT, Normal distributions)]
  patterns: [pure-computation-functions-returning-option, extract-then-compute pattern]

key-files:
  created:
    - src/analysis/scoring.rs
  modified:
    - src/analysis/stats.rs
    - src/analysis/mod.rs
    - src/paper_trade/analyzer.rs

key-decisions:
  - "Use boolean gross_hit/net_hit fields directly (not P&L sign) for hit rate computation"
  - "365.25-day year for prediction market Sharpe annualization (not 252 trading days)"
  - "PSR uses Bailey & Lopez de Prado formula with Fisher bias-corrected skewness/kurtosis"
  - "statrs 0.18 StudentsT for t-test p-values and Normal for PSR CDF"

patterns-established:
  - "Pure computation: functions take slices, return Option<Result>, no side effects"
  - "Extract-then-compute: extract_pnl_series builds f64 vec from records, passed to all functions"
  - "Degenerate input safety: all functions return None for n=0, n=1, zero variance"

requirements-completed: [SIGNAL-01, SIGNAL-02, SIGNAL-03, SIGNAL-04, SIGNAL-05]

# Metrics
duration: 7min
completed: 2026-02-28
---

# Phase 28 Plan 01: Scoring Computation Layer Summary

**Five pure scoring functions (hit rates with Wilson CIs, edge t-test, Sharpe with PSR, max drawdown) plus skewness/kurtosis stats and AnalysisSettlementRecord Deserialize**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-28T21:45:20Z
- **Completed:** 2026-02-28T21:52:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- AnalysisSettlementRecord now derives Deserialize for JSONL loading in CLI tools
- stats.rs extended with Fisher's bias-corrected skewness and excess kurtosis functions
- scoring.rs created with five computation functions, five result structs, and ScoringResult composite
- 17 new unit tests (6 in stats, 11 in scoring) all passing; 605 total tests with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Deserialize to AnalysisSettlementRecord and add skewness/kurtosis to stats.rs** - `24e0d75` (feat)
2. **Task 2: Create scoring.rs with five computation functions, result structs, and ScoringResult composite** - `12c67f7` (feat)

## Files Created/Modified
- `src/analysis/scoring.rs` - Five scoring computation functions, result structs, ScoringResult composite, 11 unit tests
- `src/analysis/stats.rs` - Added skewness_f64 and kurtosis_f64 with bias correction, 6 unit tests
- `src/analysis/mod.rs` - Added `pub mod scoring;` export
- `src/paper_trade/analyzer.rs` - Added Deserialize derive to AnalysisSettlementRecord

## Decisions Made
- Use boolean gross_hit/net_hit fields directly for hit rate computation (not P&L sign -- per research anti-pattern analysis)
- 365.25-day year for prediction market Sharpe annualization (not 252 stock trading days)
- PSR formula uses Bailey & Lopez de Prado (2012) with Fisher bias-corrected skewness/kurtosis
- statrs 0.18 StudentsT CDF for p-values and Normal CDF for PSR z-score

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed borrow pattern in max drawdown recovery search**
- **Found during:** Task 2 (scoring.rs creation)
- **Issue:** Rust 2024 edition implicit borrowing rules reject `|(_, &c)|` pattern in `.find()` closure
- **Fix:** Changed to `|(_, c)| **c >= dd_peak_value` to double-dereference
- **Files modified:** src/analysis/scoring.rs
- **Verification:** cargo test passes, no compile errors
- **Committed in:** 12c67f7 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor syntax fix for Rust edition compatibility. No scope change.

## Issues Encountered
None beyond the borrow pattern fix documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All five scoring computation functions ready for CLI wiring in 28-02
- ScoringResult composite provides single entry point via compute_scoring()
- extract_pnl_series helper available for CLI to use directly
- AnalysisSettlementRecord can now be deserialized from JSONL settlement logs

## Self-Check: PASSED

- [x] src/analysis/scoring.rs exists
- [x] src/analysis/stats.rs exists
- [x] src/analysis/mod.rs exists
- [x] src/paper_trade/analyzer.rs exists
- [x] Commit 24e0d75 found
- [x] Commit 12c67f7 found
- [x] 605 tests pass, 0 failures

---
*Phase: 28-signal-scoring-cli*
*Completed: 2026-02-28*
