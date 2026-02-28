---
phase: 27-spread-analytics-cli
plan: 01
subsystem: analysis
tags: [spread-analytics, statistics, cli, comfy-table, serde-json, btreemap]

# Dependency graph
requires:
  - phase: 26-analysis-infrastructure
    provides: stats module (mean_f64, stddev_f64, percentile_f64, median_f64), io module (DateRange, load_jsonl), output module (OutputFormat, render_output, new_table, set_numeric_columns, section_header, LoadingSummary), CLI binary skeleton
provides:
  - spread_analytics module with compute_distribution, compute_hourly, compute_venue_pairs, compute_analysis functions
  - FullSpreadOutput, SpreadAnalysis, DistributionSummary, HourlyBreakdown, VenuePairBreakdown serializable result structs
  - Table rendering functions for all three analysis sections
  - group_by_event helper for per-event analysis
  - Fully wired spread-analytics CLI binary with --by-event and --output json support
affects: [28-signal-scoring-cli, analysis-tooling]

# Tech tracking
tech-stack:
  added: []
  patterns: [pure-computation-with-serializable-results, btreemap-bucketing-for-ordered-output, dual-layer-grouping-for-by-event]

key-files:
  created: [src/analysis/spread_analytics.rs]
  modified: [src/analysis/mod.rs, src/spread/patterns.rs, src/bin/spread_analytics.rs]

key-decisions:
  - "Aggregate distribution (SPREAD-01) shown first as overview, venue-pair breakdown (SPREAD-03) repeats per-pair detail -- not mutually exclusive"
  - "Hourly table uses net spread only (primary actionable metric), gross available via JSON output"
  - "Clone SpreadResult refs for per-event computation rather than dual-signature compute functions"
  - "SpreadPattern derives Ord+Hash for BTreeMap key use in venue-pair sub-grouping"

patterns-established:
  - "Pure computation functions accept &[SpreadResult] and return Serialize-deriving result structs"
  - "Table rendering functions are separate from computation for clean separation of concerns"
  - "BTreeMap bucketing for deterministic ordered output (hours 0-23, venue pairs alphabetical)"
  - "Pre-populate all 24 hour buckets before data insertion to guarantee 24-row output"

requirements-completed: [SPREAD-01, SPREAD-02, SPREAD-03]

# Metrics
duration: 8min
completed: 2026-02-28
---

# Phase 27 Plan 01: Spread Analytics CLI Summary

**Spread analytics computation module with distribution, hourly, and venue-pair analysis wired into CLI binary with table and JSON output**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-28T21:18:46Z
- **Completed:** 2026-02-28T21:26:21Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Created spread_analytics.rs module (742 lines) with complete computation and rendering layer
- Wired all three analysis sections into the spread-analytics CLI binary replacing placeholder output
- Implemented distribution summary with 10-row net/gross comparison (count, mean, median, stddev, min, max, p5/p25/p75/p95)
- Implemented 24-row hourly breakdown with per-hour count, mean, median, stddev, and percent-positive
- Implemented venue-pair breakdown with per-direction rows and total per pair
- Added --by-event support repeating all analyses per event_id
- Added --output json support serializing FullSpreadOutput as complete JSON
- Graceful empty data handling ("No spread data in range" message)
- 14 unit tests covering all computation functions, empty cases, and table rendering

## Task Commits

Each task was committed atomically:

1. **Task 1: Create spread_analytics.rs module** - `ce6538b` (feat)
2. **Task 2: Wire analytics into CLI binary** - `be6ff3a` (feat)

## Files Created/Modified
- `src/analysis/spread_analytics.rs` - New module with SpreadStats, DistributionSummary, HourlyBreakdown, VenuePairBreakdown structs, compute functions, table renderers, and 14 unit tests
- `src/analysis/mod.rs` - Added `pub mod spread_analytics` declaration
- `src/spread/patterns.rs` - Added PartialOrd, Ord, Hash derives to SpreadPattern enum
- `src/bin/spread_analytics.rs` - Replaced placeholder with full analysis rendering, --by-event grouping, JSON output support

## Decisions Made
- Aggregate distribution shown first as overview before venue-pair breakdown -- SPREAD-01 covers aggregate, SPREAD-03 covers per-pair detail
- Hourly table shows net spread only (actionable metric), gross spread available via JSON output for external tooling
- Used clone approach for per-event computation (`refs.into_iter().cloned().collect()`) rather than dual-signature functions -- SpreadResult is not large enough to warrant the complexity
- Added PartialOrd, Ord, Hash to SpreadPattern as non-breaking additive change for BTreeMap key use

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Spread analytics CLI is fully functional with all three analysis sections
- Phase 27 has only this single plan, so phase is complete
- Phase 28 (signal scoring CLI) can proceed -- it follows the same pattern of computation module + CLI wiring
- All 588 existing tests + 14 new tests pass (602 total)
- No new clippy warnings introduced

---
*Phase: 27-spread-analytics-cli*
*Completed: 2026-02-28*
