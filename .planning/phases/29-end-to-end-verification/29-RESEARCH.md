# Phase 29: End-to-End Verification - Research

**Researched:** 2026-02-28
**Domain:** CLI integration testing, test fixture design, edge case verification in Rust
**Confidence:** HIGH

## Summary

Phase 29 is a verification phase, not a feature phase. The goal is to prove that the two CLI binaries (`spread-analytics` and `signal-scoring`) built in Phases 26-28 produce correct, trustworthy output when run against actual soak test data and synthetic edge-case data. All 12 v1.4 requirements (INFRA-01 through INFRA-04, SPREAD-01 through SPREAD-03, SIGNAL-01 through SIGNAL-05) have been implemented and passed code-level verification. What remains is running the binaries end-to-end with realistic data, hand-verifying a known subset of results, and confirming edge cases produce graceful output rather than panics or malformed tables.

The key challenge is that the project currently has **no spread_logs/ or settlement_logs/ directories** with real data, despite having signal_logs/ data from soak testing. The spread-analytics CLI reads from `spread_logs/` (SpreadResult JSONL) and signal-scoring reads from `settlement_logs/` (AnalysisSettlementRecord JSONL with "settlements-" prefix). These are different formats from the signal_logs/ that exist. End-to-end verification therefore requires: (1) creating representative test fixture JSONL files with known values, (2) running both CLIs against those fixtures, (3) hand-computing expected outputs for a small known subset, and (4) testing edge cases (empty ranges, zero records, malformed lines).

**Primary recommendation:** Create a `tests/fixtures/` directory with synthetic but realistic JSONL files for both CLIs. Write integration tests using `std::process::Command` to invoke the compiled binaries, capture stdout/stderr, and assert on expected outputs. Keep the `assert_cmd` crate out -- `std::process::Command` is sufficient for this scope and avoids adding dependencies. Include one hand-verified "golden" dataset with pre-computed expected values for spread stats, hit rate, and Sharpe ratio.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::process::Command | stdlib | Invoke CLI binaries and capture output | No external dependency needed for simple binary invocation |
| tempfile | 3 | Create temporary directories for test fixtures | Already a dev-dependency in the project |
| serde_json | (existing) | Parse JSON output from `--output json` for assertion | Already in project dependencies |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::fs | stdlib | Write test fixture JSONL files | Creating test data before CLI invocation |
| std::str | stdlib | Parse CLI stdout as text for table output assertions | Checking table format contains expected substrings |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| std::process::Command | assert_cmd (crate) | assert_cmd adds fluent assertions but is another dependency; project already uses no CLI test framework; std::process::Command is sufficient for 3-5 test cases |
| Hand-written fixtures | proptest/quickcheck | Property testing is overkill for verification of known-value golden data; this phase needs deterministic expected values, not random exploration |

**No installation needed:** All tools are either in stdlib or already in dev-dependencies.

## Architecture Patterns

### Recommended Test Structure
```
tests/
    cli_end_to_end.rs          # New: E2E tests for both CLI binaries
tests/fixtures/
    spread_logs/
        2026-01-15.jsonl       # Known-value spread data (hand-computable)
    settlement_logs/
        settlements-2026-01-15.jsonl  # Known-value settlement data
    malformed/
        2026-01-15.jsonl       # Mix of valid and invalid JSONL lines
```

### Pattern 1: CLI Binary Invocation Test
**What:** Use `cargo_bin` path resolution + `std::process::Command` to invoke the compiled binary
**When to use:** Every end-to-end CLI test
**Example:**
```rust
use std::process::Command;

fn cargo_bin(name: &str) -> std::path::PathBuf {
    let mut path = std::env::current_exe()
        .unwrap()
        .parent()  // deps/
        .unwrap()
        .parent()  // debug/ or release/
        .unwrap()
        .to_path_buf();
    path.push(name);
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

#[test]
fn spread_analytics_produces_table_output() {
    let dir = tempfile::tempdir().unwrap();
    // Write fixture JSONL to dir.path().join("2026-01-15.jsonl")
    // ...
    let output = Command::new(cargo_bin("spread-analytics"))
        .args(["--from", "2026-01-15", "--to", "2026-01-15", "--log-dir"])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Distribution Summary"));
}
```

### Pattern 2: Golden Value Verification
**What:** Pre-compute expected statistical values by hand, assert CLI output matches
**When to use:** The hand-verified known-value subset (Success Criterion #2)
**Example:**
```rust
// Given: 5 spread records with net spreads [0.02, 0.04, 0.01, 0.03, 0.05]
// Hand-computed: mean = 0.03, median = 0.03, min = 0.01, max = 0.05
// stddev = sqrt(((0.02-0.03)^2+...+(0.05-0.03)^2)/4) = sqrt(0.00025) = 0.01581...

let output = Command::new(cargo_bin("spread-analytics"))
    .args(["--from", "2026-01-15", "--to", "2026-01-15", "--output", "json", "--log-dir"])
    .arg(dir.path())
    .output()
    .unwrap();

let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
let mean = json["aggregate"]["distribution"]["net_spread"]["mean"]
    .as_f64().unwrap();
assert!((mean - 0.03).abs() < 1e-6, "mean should be 0.03, got {mean}");
```

### Pattern 3: Edge Case Testing
**What:** Verify graceful behavior for degenerate inputs
**When to use:** Empty date ranges, zero records, malformed lines, single-record sets
**Example:**
```rust
#[test]
fn spread_analytics_empty_range_shows_no_data() {
    let dir = tempfile::tempdir().unwrap();
    // No files in dir
    let output = Command::new(cargo_bin("spread-analytics"))
        .args(["--from", "2099-01-01", "--to", "2099-01-02", "--log-dir"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No spread data in range"));
}
```

### Anti-Patterns to Avoid
- **Testing against live soak data directories:** Tests must be self-contained with fixture data; never depend on `spread_logs/` or `settlement_logs/` existing at a particular path
- **Floating-point exact equality:** Always use epsilon comparisons (1e-4 to 1e-6) for statistical outputs; never assert `==` on f64 values
- **Asserting on table column alignment:** Table rendering details (exact spacing, border characters) are comfy-table implementation details; assert on content presence, not exact formatting
- **Testing computation logic in E2E tests:** Unit tests already cover `compute_*` functions exhaustively; E2E tests should verify wiring and output format, not re-test the math

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Temp directories for test fixtures | Manual `mkdir`/cleanup in tests | `tempfile::tempdir()` | Already available; auto-cleanup on drop; prevents test pollution |
| Binary path resolution | Hardcoded `target/debug/` paths | `cargo_bin()` helper function above | Works across debug/release and Windows/Unix |
| JSON output parsing | String matching on JSON text | `serde_json::from_slice` | Handles whitespace, ordering, escaping correctly |
| Expected value computation | Manually computing in test code | Pre-computed constants with derivation comments | Hand-computed values ARE the oracle; the point is to verify the code matches the hand computation |

**Key insight:** E2E verification tests are about testing the integration and output format, not re-implementing the math. The unit tests (605+ existing) already validate computation correctness. E2E tests verify the CLI plumbing -- argument parsing, file loading, output routing, and graceful error handling.

## Common Pitfalls

### Pitfall 1: Binary Not Built Before Test Runs
**What goes wrong:** `std::process::Command` fails to find the binary because `cargo test` doesn't automatically build all `[[bin]]` targets
**Why it happens:** Rust's test runner builds the test binary and its dependencies but not sibling binaries
**How to avoid:** Run `cargo build --bins` before `cargo test`, or use `cargo test --all-targets` which builds everything. Alternatively, the test can check for the binary and skip with a clear message.
**Warning signs:** "No such file or directory" errors in CI or fresh checkouts

### Pitfall 2: Windows Path Separators in Assertions
**What goes wrong:** Assertions on file paths fail on Windows because `\` vs `/`
**Why it happens:** CLI output may use OS-native separators
**How to avoid:** Don't assert on path strings in output; assert on content (statistics, section headers)
**Warning signs:** Tests pass on Linux/Mac but fail on Windows

### Pitfall 3: Floating-Point Formatting Precision
**What goes wrong:** CLI displays "0.0300" (4dp) but test expects "0.03"
**Why it happens:** The `fmt_f64(v, SPREAD_DP)` in spread_analytics.rs uses SPREAD_DP=4, so 0.03 displays as "0.0300"
**How to avoid:** For table output, assert substring presence with the exact formatted value (e.g., "0.0300"); for JSON output, parse as f64 and use epsilon comparison
**Warning signs:** Tests fail despite correct values because of trailing zeros or decimal places

### Pitfall 4: stderr vs stdout Confusion
**What goes wrong:** Test asserts on stdout but the message is on stderr, or vice versa
**Why it happens:** `spread-analytics` prints loading summary to stdout (in table mode) and "No spread data" to stderr. `signal-scoring` prints loading summary to stderr and scoring output to stdout.
**How to avoid:** Check the correct stream based on which CLI and which output mode:
  - `spread-analytics` table mode: loading + analysis tables on stdout, warnings on stderr
  - `signal-scoring`: loading summary on stderr, scoring output on stdout, "No settled positions" on stdout
**Warning signs:** Empty stdout when output was expected (it's on stderr)

### Pitfall 5: Test Data Schema Drift
**What goes wrong:** Test fixture JSONL doesn't match the current struct definition, causing all records to be counted as parse errors
**Why it happens:** SpreadResult has 18+ fields with `#[serde(with = "rust_decimal::serde::str")]` requiring string-encoded Decimals; AnalysisSettlementRecord has 16 fields
**How to avoid:** Generate test fixtures by serializing actual struct instances (using serde_json::to_string), not by hand-writing JSON. Include a "schema validation" test that serializes and deserializes a round-trip.
**Warning signs:** CLI reports "N malformed JSONL lines skipped" when all lines should be valid

### Pitfall 6: SpreadPattern Deserialization
**What goes wrong:** SpreadResult JSONL with a `pattern` field value like `"BuyPolyYesSellKalshiYes"` fails to deserialize
**Why it happens:** serde enum variant naming may not match what the serializer produces
**How to avoid:** Generate test data by serializing `SpreadResult` structs (reuse the test helper from `spread_analytics.rs::tests`) rather than hand-crafting JSON
**Warning signs:** 100% parse error rate on fixture files

## Code Examples

### Generating Fixture SpreadResult JSONL
```rust
// Reuse the test helper pattern from spread_analytics.rs
use prediction::spread::patterns::{SpreadPattern, SpreadResult};
use rust_decimal::Decimal;
use std::str::FromStr;

fn make_spread(event_id: &str, net: &str, gross: &str, ts_ms: i64) -> SpreadResult {
    SpreadResult {
        event_id: event_id.to_string(),
        pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
        gross_spread: Decimal::from_str(gross).unwrap(),
        net_spread: Decimal::from_str(net).unwrap(),
        buy_fill_price: Decimal::from_str("0.50").unwrap(),
        sell_fill_price: Decimal::from_str("0.55").unwrap(),
        buy_fee: Decimal::from_str("0").unwrap(),
        sell_fee: Decimal::from_str("0").unwrap(),
        carry_cost: Decimal::from_str("0").unwrap(),
        total_cost: Decimal::from_str("0").unwrap(),
        basis_risk_premium: Decimal::from_str("0").unwrap(),
        buy_fill_ratio: Decimal::from_str("1.0").unwrap(),
        sell_fill_ratio: Decimal::from_str("1.0").unwrap(),
        target_notional: Decimal::from_str("500").unwrap(),
        timestamp_ms: ts_ms,
        poly_exchange_ts: None,
        kalshi_exchange_ts: None,
        threshold: None,
        threshold_components: None,
        threshold_status: None,
    }
}

fn write_spread_fixtures(dir: &std::path::Path) {
    let records = vec![
        make_spread("evt1", "0.02", "0.05", 1705276800000), // 2024-01-15 00:00 UTC
        make_spread("evt1", "0.04", "0.07", 1705280400000), // 2024-01-15 01:00 UTC
        make_spread("evt1", "0.01", "0.04", 1705284000000), // 2024-01-15 02:00 UTC
        make_spread("evt1", "0.03", "0.06", 1705287600000), // 2024-01-15 03:00 UTC
        make_spread("evt1", "0.05", "0.08", 1705291200000), // 2024-01-15 04:00 UTC
    ];
    let path = dir.join("2026-01-15.jsonl");
    let lines: Vec<String> = records.iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}
```

### Generating Fixture AnalysisSettlementRecord JSONL
```rust
use prediction::paper_trade::analyzer::AnalysisSettlementRecord;
use prediction::signal::types::ThresholdStatus;

fn make_settlement(
    event_id: &str, gross_hit: bool, net_hit: bool,
    pnl: &str, settled_ms: i64,
) -> AnalysisSettlementRecord {
    AnalysisSettlementRecord {
        event_id: event_id.to_string(),
        position_id: format!("pos-{settled_ms}"),
        venue_pair: "polymarket-kalshi".to_string(),
        pattern: "YES_NO".to_string(),
        threshold_status: Some(ThresholdStatus::PassedBoth),
        convergence_secs: 300.0,
        gross_hit,
        net_hit,
        total_raw_pnl: pnl.to_string(),
        total_net_pnl: pnl.to_string(),
        total_fees: "0.00".to_string(),
        total_slippage: "0.00".to_string(),
        inter_leg_gap_ms: Some(100),
        stale_fill: false,
        running_gross_hit_rate: 0.0,
        running_net_hit_rate: 0.0,
        running_avg_net_edge: 0.0,
        running_false_positive_rate: 0.0,
        running_avg_convergence_secs: 0.0,
        settled_at_ms: settled_ms,
    }
}
```

### Hand-Verified Golden Values

For a dataset of 5 net spreads `[0.02, 0.04, 0.01, 0.03, 0.05]`:
- **count** = 5
- **mean** = (0.02+0.04+0.01+0.03+0.05)/5 = 0.15/5 = 0.03
- **sorted** = [0.01, 0.02, 0.03, 0.04, 0.05]
- **median** = 0.03 (middle value)
- **min** = 0.01, **max** = 0.05
- **stddev** = sqrt(((0.02-0.03)^2+(0.04-0.03)^2+(0.01-0.03)^2+(0.03-0.03)^2+(0.05-0.03)^2)/4) = sqrt(0.001/4) = sqrt(0.00025) = 0.015811...
- **p25** = rank 0.25*4=1.0 -> sorted[1] = 0.02
- **p75** = rank 0.75*4=3.0 -> sorted[3] = 0.04

For a settlement dataset of 10 records where 7 are gross_hit=true, 5 are net_hit=true, with PnL = [1.0, -0.5, 0.8, -0.3, 1.2, -0.1, 0.6, 0.4, -0.7, 0.9]:
- **gross_hit_rate** = 7/10 = 0.70
- **net_hit_rate** = 5/10 = 0.50
- **mean PnL** = (1.0-0.5+0.8-0.3+1.2-0.1+0.6+0.4-0.7+0.9)/10 = 3.3/10 = 0.33
- **stddev PnL** = sample stddev with n-1 denominator
- **per_trade_sharpe** = mean/stddev

These values should be hand-verified once and used as assertion constants.

### CLI Argument Names
Important: the CLI arguments use underscores in the struct but hyphens on the command line:
- `--log-dir` (not `--log_dir`) for spread-analytics
- `--settlement-dir` (not `--settlement_dir`) for signal-scoring
- `--by-event` (not `--by_event`)
- `--output json` or `--output table`

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual binary testing only | Automated E2E integration tests | This phase | Repeatable verification, regression protection |
| Trust unit tests for correctness | Hand-verified golden values | This phase | Known-value oracle proves end-to-end correctness |

**Current project test infrastructure:**
- 605+ lib unit tests across all modules
- 16+ integration tests in `tests/` directory
- `tempfile` crate already available for temp directories
- No CLI test framework (assert_cmd etc.) -- use stdlib

## Critical Data Observations

### No Real Spread or Settlement Data Exists Yet
- `spread_logs/` directory does NOT exist (no real spread data recorded)
- `settlement_logs/` directory does NOT exist (no settlements recorded yet)
- `signal_logs/` directory exists with 464 lines across 6 days, but this is signal log data (different schema from SpreadResult)
- `recordings/` directory exists with Deribit/Polymarket raw feed data (not directly usable by CLIs)

**Implication:** End-to-end verification MUST use synthetic test fixtures, not "real soak test data." The success criteria's mention of "real soak test data" should be interpreted as "realistic synthetic data that exercises the same code paths as real data." The system has been soak testing (signal_logs prove this), but spread_logs and settlement_logs are produced by different system components that generate data during live market hours.

### Existing Edge Case Handling
Both CLIs already handle edge cases in their current code:
- **Empty data:** `spread-analytics` checks `result.records.is_empty()` and prints "No spread data in range" to stderr. `signal-scoring` checks similarly and prints "No settled positions in range {range}" to stdout.
- **Malformed lines:** `load_jsonl` silently skips malformed lines, increments error counter, and the binaries emit `"Warning: {} malformed JSONL lines skipped"` to stderr.
- **Missing files:** `files_in_dir` only returns existing paths; `files_missing` is tracked but files that don't exist are simply not loaded.

What still needs verification:
1. **Division-by-zero paths:** When all P&L values are identical (zero stddev), `compute_edge_test` and `compute_sharpe` return None. But does `scoring_table` handle None sections gracefully? Yes -- it checks `if let Some(ref hr) = result.hit_rates` etc.
2. **Single record:** n=1 means stddev=None, edge_test=None, sharpe=None, PSR=None. The table should show only hit rates and potentially drawdown.
3. **All-negative P&L:** Should still compute correctly with negative Sharpe.

## Open Questions

1. **Real soak test data availability**
   - What we know: signal_logs/ exists but spread_logs/ and settlement_logs/ do not
   - What's unclear: Whether the soak test was expected to produce spread/settlement data by now, or if this is by design (the main binary hasn't been run with settlement tracking enabled)
   - Recommendation: Proceed with synthetic fixtures that exercise all code paths; note in verification that real data testing is deferred to when the system produces actual spread/settlement JSONL

2. **Test execution order**
   - What we know: `cargo test` runs tests in parallel by default
   - What's unclear: Whether any test might interfere with others if they share temp directory paths
   - Recommendation: Use unique `tempfile::tempdir()` per test (already the pattern in the codebase)

## Sources

### Primary (HIGH confidence)
- Direct codebase inspection: `src/bin/spread_analytics.rs`, `src/bin/signal_scoring.rs` -- CLI argument names, output routing, error handling
- Direct codebase inspection: `src/analysis/io.rs` -- `load_jsonl` tolerance behavior, `DateRange` file enumeration
- Direct codebase inspection: `src/analysis/scoring.rs` -- all Option return types for degenerate inputs
- Direct codebase inspection: `src/analysis/spread_analytics.rs` -- compute functions and table renderers
- Direct codebase inspection: `Cargo.toml` -- only `tempfile = "3"` in dev-dependencies, no CLI test framework
- Direct codebase inspection: `tests/` -- existing test patterns (smoke_test, integration, pipeline_test, schema_golden_test)

### Secondary (MEDIUM confidence)
- Rust std::process::Command documentation -- binary invocation and output capture pattern

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- using stdlib + existing dev-dependencies only
- Architecture: HIGH -- follows existing test patterns in the project
- Pitfalls: HIGH -- identified from direct code inspection of actual CLI behavior (stderr/stdout routing, formatting constants, serde requirements)
- Edge cases: HIGH -- traced through actual code paths in scoring.rs and spread_analytics.rs

**Research date:** 2026-02-28
**Valid until:** 2026-03-28 (stable -- no external dependencies or APIs to change)
