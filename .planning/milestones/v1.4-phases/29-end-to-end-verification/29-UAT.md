---
status: complete
phase: v1.4-analysis-tooling (phases 26-29)
source: 26-01-SUMMARY.md, 26-02-SUMMARY.md, 27-01-SUMMARY.md, 28-01-SUMMARY.md, 28-02-SUMMARY.md, 29-01-SUMMARY.md, 29-02-SUMMARY.md
started: 2026-03-02T12:00:00Z
updated: 2026-03-02T12:05:00Z
---

## Current Test

[testing complete]

## Tests

### 1. spread-analytics --help shows valid CLI usage
expected: Running `cargo run --bin spread-analytics -- --help` shows usage with --from, --to, --last, --output, --by-event, and --log-dir flags documented.
result: pass

### 2. signal-scoring --help shows valid CLI usage
expected: Running `cargo run --bin signal-scoring -- --help` shows usage with --from, --to, --last, --output, --by-event, and --settlement-dir flags documented.
result: pass

### 3. spread-analytics loads data and shows distribution summary
expected: Running spread-analytics with data shows a summary statistics table with count, mean, median, stddev, min, max, and percentiles for both net and gross spreads, with numbers right-justified.
result: skipped
reason: No spread_logs/ directory with SpreadResult JSONL data available. E2E tests (test 12) verify this with synthetic golden-value fixtures instead.

### 4. spread-analytics shows 24-row hourly breakdown
expected: The same command also shows a 24-row hourly breakdown table (hours 0-23) with per-hour spread statistics.
result: skipped
reason: No spread_logs/ data available. Covered by E2E test golden_value_hourly_json.

### 5. spread-analytics shows venue-pair breakdown
expected: Spread statistics grouped by venue pair with directional detail.
result: skipped
reason: No spread_logs/ data available. Covered by E2E test golden_value_venue_pair_json.

### 6. spread-analytics --output json produces valid JSON
expected: Running spread-analytics with --output json outputs valid parseable JSON.
result: skipped
reason: No spread_logs/ data available. Covered by E2E tests golden_value_distribution_json, golden_value_hourly_json, golden_value_venue_pair_json.

### 7. spread-analytics --by-event shows per-event breakdown
expected: Running spread-analytics with --by-event shows all three analyses broken down per event_id.
result: skipped
reason: No spread_logs/ data available. Covered by E2E test table_output_contains_expected_sections.

### 8. signal-scoring loads settlement data and shows scoring table
expected: Running signal-scoring with settlement data shows multi-section scoring table with hit rates, edge t-test, Sharpe/PSR, and drawdown.
result: skipped
reason: No settlement_logs/ directory with AnalysisSettlementRecord JSONL data available. Covered by E2E tests golden_value_hit_rates_json, golden_value_edge_and_sharpe_json, golden_value_drawdown_json, table_output_contains_scoring_sections.

### 9. signal-scoring --output json produces valid JSON
expected: Running signal-scoring with --output json outputs valid JSON with scoring result fields.
result: skipped
reason: No settlement_logs/ data available. Covered by E2E tests golden_value_hit_rates_json, golden_value_edge_and_sharpe_json, golden_value_drawdown_json.

### 10. spread-analytics handles empty date range gracefully
expected: Running spread-analytics with a date range matching no files shows "No spread data in range" without panicking.
result: pass
verified: Output shows loading summary table then "No spread data in range." — no panic, exit 0.

### 11. signal-scoring handles empty date range gracefully
expected: Running signal-scoring with a date range matching no files shows "No settled positions in range" without panicking.
result: pass
verified: Output shows "Loaded 0 records from 0 files" then "No settled positions in range 2020-01-01 to 2020-01-02" — no panic, exit 0.

### 12. E2E integration tests pass
expected: Running `cargo test --test spread_analytics_e2e --test signal_scoring_e2e` passes all 13 E2E tests (6 spread + 7 signal) with no failures.
result: pass
verified: "test result: ok. 7 passed" (signal) + "test result: ok. 6 passed" (spread) — 13/13 pass, 0 failures.

## Summary

total: 12
passed: 5
issues: 0
pending: 0
skipped: 7

## Gaps

[none yet]
