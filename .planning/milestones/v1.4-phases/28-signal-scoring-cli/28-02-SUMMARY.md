---
phase: 28-signal-scoring-cli
plan: 02
subsystem: analysis
tags: [signal-scoring, cli, settlement-logs, hit-rates, sharpe, drawdown, edge-test, comfy-table, json-output]

# Dependency graph
requires:
  - phase: 28-signal-scoring-cli
    provides: "scoring.rs with compute_scoring, ScoringResult, five computation functions"
  - phase: 26-analysis-foundation
    provides: "output.rs render_output/new_table/section_header/set_numeric_columns, io.rs DateRange/load_jsonl"
provides:
  - "Complete signal-scoring CLI binary loading settlement data and computing all five scoring metrics"
  - "ScoringOutput wrapper for JSON output with aggregate and by-event breakdown"
  - "scoring_table function rendering five-section formatted terminal table"
affects: [signal-scoring-binary, soak-test-analysis]

# Tech tracking
tech-stack:
  added: []
  patterns: [section-header-table-rendering, by-event-grouping-with-btreemap, scoring-output-wrapper-struct]

key-files:
  created: []
  modified:
    - src/bin/signal_scoring.rs

key-decisions:
  - "ScoringOutput wrapper with skip_serializing_if for clean JSON when --by-event is not used"
  - "Loading summary printed to stderr to keep stdout clean for scoring output and JSON mode"
  - "BTreeMap for by-event grouping ensuring deterministic key ordering in both table and JSON output"

patterns-established:
  - "Section-based table rendering: scoring_table builds one table with four section_header calls for visual grouping"
  - "Dual output path: table mode renders inline, JSON mode serializes ScoringOutput wrapper"
  - "Stderr for metadata: file/record counts go to stderr, analysis output goes to stdout"

requirements-completed: [SIGNAL-01, SIGNAL-02, SIGNAL-03, SIGNAL-04, SIGNAL-05]

# Metrics
duration: 4min
completed: 2026-02-28
---

# Phase 28 Plan 02: Signal Scoring CLI Wiring Summary

**Complete signal-scoring binary with five-section scoring table (hit rates, edge t-test, Sharpe/PSR, drawdown), JSON output, and per-event breakdowns from settlement logs**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-28T21:55:28Z
- **Completed:** 2026-02-28T22:00:21Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Replaced Phase 26 placeholder loading summary with full five-section scoring analysis in signal-scoring CLI
- Added ScoringOutput wrapper struct for clean JSON output with optional by-event breakdown
- Table output renders four sections with section headers: hit rates with Wilson CIs, cost-adjusted edge t-test, Sharpe ratio with PSR, maximum drawdown with dates
- JSON output produces valid serialized ScoringResult with all fields from Plan 01's computation layer
- Empty data range gracefully prints "No settled positions in range" and exits 0
- All 605+ tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Rewrite signal_scoring.rs to load settlement data and compute scoring metrics** - `482101b` (feat)

## Files Created/Modified
- `src/bin/signal_scoring.rs` - Complete rewrite: loads AnalysisSettlementRecord from settlement_logs/, computes scoring via compute_scoring(), renders five-section table or JSON, supports --by-event grouping

## Decisions Made
- ScoringOutput wrapper uses `skip_serializing_if = "Option::is_none"` so JSON output is clean when --by-event is not specified
- Loading summary (file count, record count, errors) printed to stderr to avoid mixing metadata with scoring output in stdout (important for JSON mode piping)
- BTreeMap used for by-event grouping to ensure deterministic key ordering in both table rendering and JSON serialization

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Signal scoring CLI fully functional and ready for soak test analysis
- All five SIGNAL requirements (SIGNAL-01 through SIGNAL-05) addressed in the CLI binary
- Phase 28 complete: both plans (computation layer + CLI wiring) delivered

## Self-Check: PASSED

- [x] src/bin/signal_scoring.rs exists with full scoring implementation
- [x] Commit 482101b found
- [x] 605+ tests pass, 0 failures
- [x] `cargo build --bin signal-scoring` compiles without errors
- [x] `signal-scoring --help` shows --settlement-dir, --from, --to, --last, --output, --by-event
- [x] `signal-scoring --last 7` handles empty data gracefully

---
*Phase: 28-signal-scoring-cli*
*Completed: 2026-02-28*
