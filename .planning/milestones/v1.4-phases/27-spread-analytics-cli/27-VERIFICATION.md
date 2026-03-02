---
phase: 27-spread-analytics-cli
verified: 2026-02-28T22:00:00Z
status: passed
score: 6/6 must-haves verified
re_verification: false
---

# Phase 27: Spread Analytics CLI Verification Report

**Phase Goal:** User can analyze spread distribution patterns, hourly opportunity clustering, and venue-pair performance from recorded spread data
**Verified:** 2026-02-28T22:00:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #  | Truth                                                                                                                                        | Status     | Evidence                                                                                              |
|----|----------------------------------------------------------------------------------------------------------------------------------------------|------------|-------------------------------------------------------------------------------------------------------|
| 1  | User can run `spread-analytics --from <date> --to <date>` and see distribution summary with count/mean/median/stddev/min/max/p5/p25/p75/p95 | VERIFIED   | `distribution_table()` renders 10-row table with all named stats; `--from`/`--to` wired via `DateRange::from_args` |
| 2  | User can see a 24-row hourly breakdown showing per-UTC-hour spread statistics                                                                | VERIFIED   | `compute_hourly` pre-populates all 24 buckets; `hourly_table` renders 24 rows; test confirms `.rows.len() == 24` |
| 3  | User can see spread statistics grouped by venue pair with directional detail, never mixed into a single aggregate                            | VERIFIED   | `compute_venue_pairs` uses `BTreeMap<SpreadPattern, Vec<f64>>` for per-direction rows within each pair; `venue_pair_table` adds a section header per pair |
| 4  | User can pass `--by-event` and see all three analyses additionally broken down per event_id                                                  | VERIFIED   | `group_by_event` called when `cli.by_event`; per-event loop calls `compute_analysis` and `analysis_tables` per event_id |
| 5  | User can pass `--output json` and receive valid JSON with distribution, hourly, venue_pairs, and by_event keys                               | VERIFIED   | `FullSpreadOutput` (derives `Serialize`) serialized via `serde_json::to_string_pretty`; struct fields: `loading`, `aggregate.distribution`, `aggregate.hourly`, `aggregate.venue_pairs`, `by_event` |
| 6  | Empty date ranges produce a graceful "No spread data in range" message rather than a panic                                                   | VERIFIED   | Binary checks `result.records.is_empty()`, renders loading summary, prints `eprintln!("No spread data in range.")`, returns `Ok(())` |

**Score:** 6/6 truths verified

---

### Required Artifacts

| Artifact                              | Expected                                           | Min Lines | Actual Lines | Status   | Details                                                                                       |
|---------------------------------------|----------------------------------------------------|-----------|--------------|----------|-----------------------------------------------------------------------------------------------|
| `src/analysis/spread_analytics.rs`   | Spread computation functions and table renderers   | 200       | 742          | VERIFIED | Contains all required structs, compute functions, table renderers, and 14 unit tests          |
| `src/bin/spread_analytics.rs`        | Complete CLI binary with full analysis output      | 90        | 134          | VERIFIED | Full output pipeline: load -> empty check -> compute aggregate -> optional by-event -> render |
| `src/analysis/mod.rs`                | Module declaration for spread_analytics            | —         | 4            | VERIFIED | `pub mod spread_analytics;` declared on line 4                                                |
| `src/spread/patterns.rs`             | SpreadPattern derives PartialOrd, Ord, Hash        | —         | —            | VERIFIED | Line 22: `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]` |

---

### Key Link Verification

| From                                  | To                        | Via                                                                 | Status   | Details                                                                                         |
|---------------------------------------|---------------------------|---------------------------------------------------------------------|----------|-------------------------------------------------------------------------------------------------|
| `src/analysis/spread_analytics.rs`   | `src/analysis/stats.rs`   | `mean_f64`, `stddev_f64`, `percentile_f64`, `median_f64` calls     | WIRED    | Imported on line 15; called in `compute_spread_stats` (lines 124-132) and `compute_hourly` (lines 211-213) |
| `src/analysis/spread_analytics.rs`   | `src/analysis/output.rs`  | `new_table`, `set_numeric_columns`, `section_header`, `render_output` | WIRED  | Imported on line 13; called in `distribution_table`, `hourly_table`, `venue_pair_table` (lines 297-394) |
| `src/bin/spread_analytics.rs`        | `src/analysis/spread_analytics.rs` | `compute_analysis`, `analysis_tables`, `group_by_event` calls | WIRED | Imported on lines 9-11; called at lines 80, 84, 88, 108, 118                                    |

---

### Requirements Coverage

| Requirement | Description                                                                                                               | Status    | Evidence                                                                                           |
|-------------|---------------------------------------------------------------------------------------------------------------------------|-----------|----------------------------------------------------------------------------------------------------|
| SPREAD-01   | User can view spread distribution summary statistics (count, mean, median, stddev, min, max, p5/p25/p75/p95) for net and gross spreads over a date range | SATISFIED | `compute_distribution` extracts both net and gross values; `distribution_table` renders 10-row comparison table with all required stats |
| SPREAD-02   | User can view hourly time-bucket analysis showing per-hour spread statistics across 24 UTC hours                         | SATISFIED | `compute_hourly` pre-populates 24-bucket BTreeMap and computes mean/median/stddev/pct_positive per hour; `test_compute_hourly_always_24_rows` verifies invariant |
| SPREAD-03   | User can view venue-pair breakdown showing spread statistics grouped by venue pair with directional detail               | SATISFIED | `compute_venue_pairs` groups by `venue_pair_label()` then sub-groups by `SpreadPattern`; directions stored in `BTreeMap<String, SpreadStats>` keeping them separate from TOTAL row |

All three requirement IDs declared in the PLAN frontmatter are accounted for. REQUIREMENTS.md maps all three exclusively to Phase 27. No orphaned requirements found.

---

### Anti-Patterns Found

| File | Pattern | Severity | Impact |
|------|---------|----------|--------|
| None | — | — | — |

No TODOs, FIXMEs, placeholder comments, empty return stubs, or console-log-only implementations found in any modified file.

---

### Build and Test Verification

| Check | Result |
|-------|--------|
| `cargo build --bin spread-analytics` | Finished with 0 errors, 1 pre-existing dead-code warning (unrelated to phase) |
| `cargo test --lib analysis::spread_analytics` | 14/14 tests pass |
| `cargo test` (full suite) | 588 lib tests pass + 57 integration/binary tests pass; 0 failures |
| `cargo clippy --bin spread-analytics` | 0 new warnings in spread-analytics binary or spread_analytics module |

---

### Human Verification Required

The following items cannot be verified programmatically and should be spot-checked if real JSONL spread data is available:

**1. Table visual formatting**
- **Test:** Run `spread-analytics --last 30` against a populated `spread_logs/` directory
- **Expected:** Three clearly separated table sections appear below the loading summary, with right-justified numeric columns and zero-padded hour labels (00-23)
- **Why human:** Terminal formatting, column alignment, and readability require visual inspection

**2. JSON output completeness**
- **Test:** Run `spread-analytics --last 30 --output json | python -m json.tool` (or `jq .`)
- **Expected:** Valid JSON with keys `loading`, `aggregate.distribution`, `aggregate.hourly`, `aggregate.venue_pairs`
- **Why human:** Requires actual JSONL data to produce non-null sections; can only test graceful empty path automatically

**3. By-event section headers**
- **Test:** Run `spread-analytics --last 30 --by-event` with multi-event data
- **Expected:** Each event produces `=== Event: <event_id> ===` header followed by its own three analysis sections
- **Why human:** Requires real multi-event data in spread_logs/

---

### Gaps Summary

No gaps. All six observable truths are verified, all three requirement IDs are satisfied with implementation evidence, all three key links are wired, the binary builds and the full test suite passes.

---

_Verified: 2026-02-28T22:00:00Z_
_Verifier: Claude (gsd-verifier)_
