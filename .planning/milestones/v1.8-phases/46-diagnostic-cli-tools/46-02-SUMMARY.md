---
phase: 46-diagnostic-cli-tools
plan: 02
subsystem: analysis
tags: [cli, book-depth, order-book, fill-quality, rust, clap]

requires:
  - phase: 46-diagnostic-cli-tools/01
    provides: "stats helpers (mean_f64, median_f64), analysis module patterns, Cargo.toml bin entry pattern"
provides:
  - "book-depth CLI binary for order book quality analysis"
  - "BookDepthResult and InstrumentDepth structs for depth metrics"
  - "compute_book_depth function for depth quality scoring"
affects: [production-deployment, operator-tooling]

tech-stack:
  added: []
  patterns: [composite-depth-quality-score, worst-first-instrument-sorting]

key-files:
  created:
    - src/analysis/book_depth.rs
    - src/bin/book_depth.rs
  modified:
    - src/analysis/mod.rs
    - Cargo.toml

key-decisions:
  - "Depth quality score formula: fill_ratio_mean * min(depth_levels_mean / 10.0, 1.0) combines fill and depth into single metric"
  - "Instruments sorted worst-first so operator immediately sees problem areas"
  - "BookDepthCliOutput duplicated in bin for decoupling from lib's BookDepthOutput"

patterns-established:
  - "Composite quality scoring: multiplicative combination of fill and depth factors"

requirements-completed: [DIAG-02]

duration: 5min
completed: 2026-03-09
---

# Phase 46 Plan 02: Book-Depth CLI Summary

**book-depth CLI analyzing order book quality from signal logs with effective spread, fill ratios, depth levels, and composite quality scores per instrument**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-09T20:33:00Z
- **Completed:** 2026-03-09T20:38:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Book depth analysis module with compute_book_depth computing per-instrument and aggregate depth metrics
- book-depth CLI binary with --from/--to/--last/--output/--by-event/--log-dir/--target-notional flags
- Instruments sorted worst-first for immediate problem visibility
- 4 unit tests covering empty, single, multi-instrument, and table rendering scenarios

## Task Commits

Each task was committed atomically:

1. **Task 1: Create book-depth analysis module** - `961b2ce` (feat)
2. **Task 2: Create book-depth CLI binary** - `b6bfb6a` (feat)

## Files Created/Modified
- `src/analysis/book_depth.rs` - InstrumentDepth, BookDepthResult structs, compute_book_depth, book_depth_tables
- `src/bin/book_depth.rs` - CLI entry point with clap parsing, table/JSON rendering
- `src/analysis/mod.rs` - Added `pub mod book_depth;`
- `Cargo.toml` - Added `[[bin]] name = "book-depth"` entry

## Decisions Made
- Depth quality score formula: `fill_ratio_mean * min(depth_levels_mean / 10.0, 1.0)` where 10 is the reference depth level
- Instruments sorted worst-first (ascending quality score) so operator sees problem areas first
- Duplicated BookDepthOutput as BookDepthCliOutput in the bin crate to keep the bin self-contained

## Deviations from Plan

None - plan executed exactly as written. Code was partially pre-committed from a prior session (Task 1 in 961b2ce), Task 2 committed fresh.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All phase 46 plans complete (46-01 cost-audit + 46-02 book-depth)
- 6 CLI binaries now available: prediction, spread-analytics, signal-scoring, match-audit, cost-audit, book-depth
- Ready to proceed to phase 47

---
*Phase: 46-diagnostic-cli-tools*
*Completed: 2026-03-09*
