---
phase: 46-diagnostic-cli-tools
verified: 2026-03-09T22:00:00Z
status: passed
score: 8/8 must-haves verified
---

# Phase 46: Diagnostic CLI Tools Verification Report

**Phase Goal:** Operator can decompose signal economics and book quality to answer "where does negative edge come from?"
**Verified:** 2026-03-09T22:00:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1   | Operator can run cost-audit CLI on signal_logs and see which cost components dominate negative edge | VERIFIED | `src/bin/cost_audit.rs` loads JSONL via `load_jsonl::<ArbSignal>`, calls `compute_cost_audit()`, renders table with 7 components sorted by mean magnitude descending. Compiles and runs. |
| 2   | cost-audit --by-event breaks down costs per event so operator can compare instrument quality | VERIFIED | `src/bin/cost_audit.rs:83-98` groups by `event_id` into BTreeMap, computes per-event `CostAuditResult`, renders per-event tables (lines 117-123). |
| 3   | cost-audit --output json produces machine-readable output matching the table data | VERIFIED | `src/bin/cost_audit.rs:125-128` serializes `CostAuditOutput` (which wraps loading, aggregate, by_event) via `serde_json::to_string_pretty`. All structs derive Serialize. |
| 4   | pearson_correlation and ks_test_two_sample functions are available in analysis::stats for downstream CLIs | VERIFIED | `src/analysis/stats.rs:137` `pub fn pearson_correlation`, `src/analysis/stats.rs:187` `pub fn ks_test_two_sample`. Both are substantive implementations (single-pass Pearson, merge-walk KS with asymptotic p-value). 8 dedicated tests all pass. |
| 5   | Operator can run book-depth CLI on signal_logs and see effective spread, fill quality, and depth scores per instrument | VERIFIED | `src/bin/book_depth.rs` loads JSONL, calls `compute_book_depth()`, renders aggregate table (spread, fill ratio, depth levels, quality score) and per-instrument table sorted worst-first. Compiles and runs. |
| 6   | book-depth --by-event groups depth metrics by event so operator can compare liquidity across instruments | VERIFIED | `src/bin/book_depth.rs:95-108` groups by `event_id`, computes per-event `BookDepthResult`, renders per-event tables (lines 130-138). |
| 7   | book-depth --output json produces machine-readable output consistent with other CLIs | VERIFIED | `src/bin/book_depth.rs:141-143` serializes `BookDepthCliOutput` via `serde_json::to_string_pretty`. Structure mirrors CostAuditOutput pattern. |
| 8   | Depth quality score combines fill_ratio and book_depth_levels into a single interpretable metric | VERIFIED | `src/analysis/book_depth.rs:149` implements `fill_ratio_mean * (depth_levels_mean / 10.0).min(1.0)`. Unit test at line 356 verifies: fill=0.90, depth=8 produces score=0.72. |

**Score:** 8/8 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `src/analysis/stats.rs` | Pearson correlation and KS test | VERIFIED | `pub fn pearson_correlation` at L137 (30 lines), `pub struct KsTestResult` at L171, `pub fn ks_test_two_sample` at L187 (45 lines). Substantive implementations with proper edge-case handling. |
| `src/analysis/cost_audit.rs` | Cost decomposition logic | VERIFIED | 222 lines. Exports `compute_cost_audit`, `CostAuditResult`, `CostComponent`, `CostAuditOutput`, `cost_audit_table`. Extracts 7 cost components, computes mean/median/stddev/pct_of_total, sorts by magnitude. 2 tests. |
| `src/bin/cost_audit.rs` | cost-audit CLI entry point | VERIFIED | 133 lines (>40 min). Clap parser with --from/--to/--last/--output/--by-event/--log-dir flags. Loads JSONL, computes aggregate, optional by-event grouping, table/JSON rendering. |
| `src/analysis/book_depth.rs` | Book depth computation logic | VERIFIED | 400 lines. Exports `compute_book_depth`, `BookDepthResult`, `InstrumentDepth`, `BookDepthOutput`, `book_depth_tables`. Per-instrument breakdown with composite quality score. 4 tests. |
| `src/bin/book_depth.rs` | book-depth CLI entry point | VERIFIED | 149 lines (>40 min). Clap parser with --from/--to/--last/--output/--by-event/--log-dir/--target-notional flags. Same pattern as cost-audit. |
| `Cargo.toml` | Binary entries for both CLIs | VERIFIED | `name = "cost-audit"` at L103, `name = "book-depth"` at L107. |
| `src/analysis/mod.rs` | Module exports | VERIFIED | Contains `pub mod book_depth;` and `pub mod cost_audit;`. |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| `src/bin/cost_audit.rs` | `src/analysis/cost_audit.rs` | `compute_cost_audit()` call | WIRED | L80: `let aggregate = compute_cost_audit(&result.records);` |
| `src/bin/cost_audit.rs` | `src/analysis/io.rs` | `load_jsonl` and `DateRange` | WIRED | L48: `let result = load_jsonl::<ArbSignal>(&files);` |
| `src/bin/book_depth.rs` | `src/analysis/book_depth.rs` | `compute_book_depth()` call | WIRED | L92: `let aggregate = compute_book_depth(&result.records, cli.target_notional);` |
| `src/bin/book_depth.rs` | `src/analysis/io.rs` | `load_jsonl` and `DateRange` | WIRED | L60: `let result = load_jsonl::<ArbSignal>(&files);` |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ---------- | ----------- | ------ | -------- |
| DIAG-01 | 46-01 | Cost-audit CLI breaks down cost components per signal and identifies which costs dominate negative edge | SATISFIED | `cost_audit.rs` decomposes CostBreakdown into 7 named components with descriptive stats, sorted by mean magnitude. CLI renders table and JSON. |
| DIAG-02 | 46-02 | Book-depth CLI analyzes Polymarket order book quality (effective spread, fill simulation, depth at price levels) | SATISFIED | `book_depth.rs` computes effective spread, fill ratios, depth levels, composite quality score, and estimated max fill per instrument. Sorted worst-first. |
| DIAG-03 | 46-01 | Stats module extended with Pearson correlation and KS test for signal analysis | SATISFIED | `stats.rs` exports `pearson_correlation()` and `ks_test_two_sample()` with `KsTestResult` struct. 8 unit tests covering edge cases all pass (26 total stats tests pass). |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| - | - | No TODOs, FIXMEs, placeholders, or stub implementations found | - | - |

### Compilation and Tests

Both binaries compile successfully after clean build (stale cache initially caused false failure). All tests pass:

- `cargo test --lib analysis::stats`: 26 passed (including 8 new: 5 Pearson + 3 KS)
- `cargo test --lib analysis::cost_audit`: 2 passed
- `cargo test --lib analysis::book_depth`: 4 passed

### Human Verification Required

### 1. Cost Audit with Real Data

**Test:** Run `cargo run --bin cost-audit -- --last 7` on a machine with signal_logs data
**Expected:** Table showing 7 cost components with Mean, Median, Std Dev, % of Total columns, sorted by largest contributor first. Summary section shows signal count, mean raw spread, mean net edge, mean total cost.
**Why human:** Requires production signal_logs data to verify output is meaningful and correctly identifies dominant cost drivers.

### 2. Book Depth with Real Data

**Test:** Run `cargo run --bin book-depth -- --last 7` on a machine with signal_logs data
**Expected:** Aggregate table with effective spread, fill ratio, depth levels, quality score. Per-instrument table sorted worst-first showing problem instruments at top.
**Why human:** Requires production data to verify quality scores are interpretable and worst-first ordering highlights actual problem areas.

### 3. JSON Output Validity

**Test:** Run `cargo run --bin cost-audit -- --last 7 --output json | jq .` and `cargo run --bin book-depth -- --last 7 --output json | jq .`
**Expected:** Valid JSON that parses without errors, contains loading/aggregate/by_event structure.
**Why human:** Requires signal_logs data and jq to validate structure.

### Gaps Summary

No gaps found. All 8 observable truths verified through code inspection and compilation/test validation. All 3 requirements (DIAG-01, DIAG-02, DIAG-03) are satisfied with substantive implementations. No anti-patterns detected. Both CLIs follow established patterns (--from/--to/--last/--output/--by-event/--log-dir) consistent with existing spread-analytics and signal-scoring tools.

---

_Verified: 2026-03-09T22:00:00Z_
_Verifier: Claude (gsd-verifier)_
