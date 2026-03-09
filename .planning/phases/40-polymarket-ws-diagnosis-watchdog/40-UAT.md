---
status: complete
phase: 40-polymarket-ws-diagnosis-watchdog
source: 40-01-SUMMARY.md, 40-02-SUMMARY.md
started: 2026-03-09T12:40:00Z
updated: 2026-03-09T12:50:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Config data_timeout_secs field
expected: `config/venues.toml` has `data_timeout_secs = 120` under `[polymarket]`. Running `cargo check` succeeds with the new field parsed into PolymarketConfig.
result: pass

### 2. Diagnostic test compiles and lists
expected: `cargo test polymarket_diag -- --list` shows `diagnose_polymarket_ws_from_this_host` as an ignored test. The test covers WS connection, REST midpoint baseline, and reports one of 5 verdicts.
result: pass

### 3. Diagnostic test runs from EC2
expected: Running `cargo test polymarket_diag -- --ignored --nocapture` on EC2 prints a clear verdict line identifying the Polymarket WS failure mode.
result: skipped
reason: Requires EC2 instance — will test after deploy

### 4. Supervisor watchdog triggers on data inactivity
expected: Supervisor forwarding loop wraps raw_rx.recv() with tokio::time::timeout, increments feed_data_timeout_total counter, marks VenueHealth unavailable, and breaks to reconnect.
result: pass

### 5. All tests pass
expected: `cargo test` passes all lib tests, integration tests, and doc-tests with no failures.
result: pass

## Summary

total: 5
passed: 4
issues: 0
pending: 0
skipped: 1

## Gaps

[none yet]
