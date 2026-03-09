---
phase: 46-diagnostic-cli-tools
plan: 01
subsystem: analysis
tags: [stats, pearson-correlation, ks-test, cost-audit, cli, diagnostics]

# Dependency graph
requires:
  - phase: 44-cost-model-repair
    provides: "CostBreakdown with probability-space normalized costs"
provides:
  - "pearson_correlation() and ks_test_two_sample() in analysis::stats"
  - "cost-audit CLI for decomposing signal cost components"
  - "CostAuditResult, CostComponent, CostAuditOutput structs"
affects: [46-diagnostic-cli-tools, signal-quality-validation]

# Tech tracking
tech-stack:
  added: []
  patterns: [cost-decomposition-analysis, cli-bin-pattern]

key-files:
  created:
    - src/analysis/cost_audit.rs
    - src/bin/cost_audit.rs
  modified:
    - src/analysis/stats.rs
    - src/analysis/mod.rs
    - Cargo.toml

key-decisions:
  - "Use to_f64() from ToPrimitive trait for Decimal-to-f64 conversion (no lossy)"
  - "Components sorted by mean magnitude descending for immediate visibility of largest cost drivers"

patterns-established:
  - "Cost audit CLI follows same --from/--to/--last/--by-event/--output pattern as spread-analytics and signal-scoring"

requirements-completed: [DIAG-01, DIAG-03]

# Metrics
duration: 2min
completed: 2026-03-09
---

# Phase 46 Plan 01: Stats Extensions and Cost-Audit CLI Summary

**Pearson correlation and KS test in stats module, plus cost-audit CLI decomposing signal costs into 7 components sorted by magnitude**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-09T21:08:29Z
- **Completed:** 2026-03-09T21:10:04Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Extended stats module with pearson_correlation() and ks_test_two_sample() functions with 8 new unit tests
- Built cost-audit CLI that loads signal_logs JSONL and decomposes CostBreakdown into 7 components with descriptive statistics
- CLI supports --by-event grouping, --output json for machine-readable output, matching existing CLI conventions

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend stats module with Pearson correlation and KS test** - `1b58120` (feat)
2. **Task 2: Build cost-audit CLI with analysis module and bin entry point** - `1d9b870` (feat)

## Files Created/Modified
- `src/analysis/stats.rs` - Added pearson_correlation(), KsTestResult, ks_test_two_sample() with 8 tests
- `src/analysis/cost_audit.rs` - CostComponent, CostAuditResult, CostAuditOutput structs and compute/table functions
- `src/bin/cost_audit.rs` - CLI entry point with clap parser following spread-analytics pattern
- `src/analysis/mod.rs` - Added cost_audit module export
- `Cargo.toml` - Added cost-audit and book-depth binary entries

## Decisions Made
- Used to_f64() from rust_decimal::prelude::ToPrimitive for Decimal-to-f64 conversion
- Components sorted by mean magnitude descending so operator sees largest cost driver first
- Followed exact CLI pattern from spread-analytics (--from/--to/--last/--by-event/--output/--log-dir)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Stats module ready for downstream CLIs (book-depth in 46-02 can use pearson_correlation)
- cost-audit CLI operational, tested with 270 signals from signal_logs (5 days of data)
- Revealed that options_fee_estimate dominates costs at 102.7% of total (production insight)

---
*Phase: 46-diagnostic-cli-tools*
*Completed: 2026-03-09*
