//! End-to-end integration tests for the signal-scoring CLI binary.
//!
//! Each test creates synthetic golden-value fixtures using AnalysisSettlementRecord,
//! writes them to a tempdir as JSONL, runs the signal-scoring binary via
//! `std::process::Command`, and asserts output correctness against hand-computed values.

use std::path::PathBuf;
use std::process::Command;

use prediction::paper_trade::analyzer::AnalysisSettlementRecord;
use prediction::signal::types::ThresholdStatus;

/// Resolve the path to a compiled binary in the same target directory as the test runner.
fn cargo_bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe()
        .expect("current_exe() should succeed")
        .parent()
        .expect("exe parent dir")
        .parent()
        .expect("exe grandparent dir (target/debug or target/release)")
        .to_path_buf();

    let mut bin_name = name.to_string();
    if cfg!(windows) {
        bin_name.push_str(".exe");
    }
    path.push(bin_name);
    path
}

/// Create an AnalysisSettlementRecord with the given key fields and sensible defaults.
fn make_settlement(
    event_id: &str,
    gross_hit: bool,
    net_hit: bool,
    total_raw_pnl: &str,
    total_net_pnl: &str,
    settled_at_ms: i64,
) -> AnalysisSettlementRecord {
    AnalysisSettlementRecord {
        event_id: event_id.to_string(),
        position_id: format!("pos-{}", settled_at_ms),
        venue_pair: "polymarket-kalshi".to_string(),
        pattern: "YES_NO".to_string(),
        threshold_status: Some(ThresholdStatus::PassedBoth),
        convergence_secs: 300.0,
        gross_hit,
        net_hit,
        total_raw_pnl: total_raw_pnl.to_string(),
        total_net_pnl: total_net_pnl.to_string(),
        total_fees: "0.00".to_string(),
        total_slippage: "0.00".to_string(),
        inter_leg_gap_ms: Some(100),
        stale_fill: false,
        running_gross_hit_rate: 0.0,
        running_net_hit_rate: 0.0,
        running_avg_net_edge: 0.0,
        running_false_positive_rate: 0.0,
        running_avg_convergence_secs: 0.0,
        settled_at_ms,
    }
}

/// The 10-record golden fixture used by Tests 1-4.
///
/// Records:
///   gross_hit: [T,T,T,T,T,T,T,F,F,F]  -> 7/10 = 0.70
///   net_hit:   [T,T,T,T,T,F,F,F,F,F]  -> 5/10 = 0.50
///   PnL:       [1.00, -0.50, 0.80, -0.30, 1.20, -0.10, 0.60, 0.40, -0.70, 0.90]
///   Mean PnL = 3.30 / 10 = 0.33
fn write_golden_fixture(dir: &std::path::Path) {
    let pnl_values = [
        "1.00", "-0.50", "0.80", "-0.30", "1.20", "-0.10", "0.60", "0.40", "-0.70", "0.90",
    ];
    let gross_hits = [true, true, true, true, true, true, true, false, false, false];
    let net_hits = [true, true, true, true, true, false, false, false, false, false];

    // 2026-01-15 00:00 UTC = 1769472000000
    let base_ts: i64 = 1769472000000;

    let mut lines = Vec::new();
    for i in 0..10 {
        let record = make_settlement(
            "evt-golden",
            gross_hits[i],
            net_hits[i],
            pnl_values[i],
            pnl_values[i],
            base_ts + (i as i64) * 3_600_000,
        );
        lines.push(serde_json::to_string(&record).expect("serialize record"));
    }

    let file_path = dir.join("settlements-2026-01-15.jsonl");
    std::fs::write(&file_path, lines.join("\n") + "\n").expect("write fixture file");
}

// ---------------------------------------------------------------------------
// Test 1: Golden value hit rates (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_hit_rates_json() {
    let tmp = tempfile::tempdir().unwrap();
    write_golden_fixture(tmp.path());

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "json",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(
        output.status.success(),
        "signal-scoring exited with error: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    let hit_rates = &json["hit_rates"];
    assert!(!hit_rates.is_null(), "hit_rates should be present");

    let gross_rate = hit_rates["gross_rate"].as_f64().expect("gross_rate f64");
    let net_rate = hit_rates["net_rate"].as_f64().expect("net_rate f64");
    let total = hit_rates["total"].as_u64().expect("total u64");

    assert!(
        (gross_rate - 0.70).abs() < 1e-4,
        "gross_rate should be ~0.70, got {gross_rate}"
    );
    assert!(
        (net_rate - 0.50).abs() < 1e-4,
        "net_rate should be ~0.50, got {net_rate}"
    );
    assert_eq!(total, 10, "total should be 10, got {total}");
}

// ---------------------------------------------------------------------------
// Test 2: Golden value edge and Sharpe (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_edge_and_sharpe_json() {
    let tmp = tempfile::tempdir().unwrap();
    write_golden_fixture(tmp.path());

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "json",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    // Edge test: mean of [1.0, -0.5, 0.8, -0.3, 1.2, -0.1, 0.6, 0.4, -0.7, 0.9] = 0.33
    let edge_test = &json["edge_test"];
    assert!(!edge_test.is_null(), "edge_test should be present");

    let mean_edge = edge_test["mean_edge"].as_f64().expect("mean_edge f64");
    assert!(
        (mean_edge - 0.33).abs() < 1e-4,
        "mean_edge should be ~0.33, got {mean_edge}"
    );

    let edge_n = edge_test["n"].as_u64().expect("edge n");
    assert_eq!(edge_n, 10, "edge n should be 10");

    // Sharpe: positive mean / positive stddev -> positive per_trade_sharpe
    let sharpe = &json["sharpe"];
    assert!(!sharpe.is_null(), "sharpe should be present");

    let per_trade_sharpe = sharpe["per_trade_sharpe"]
        .as_f64()
        .expect("per_trade_sharpe f64");
    assert!(
        per_trade_sharpe > 0.0,
        "per_trade_sharpe should be > 0, got {per_trade_sharpe}"
    );

    let sharpe_n = sharpe["n"].as_u64().expect("sharpe n");
    assert_eq!(sharpe_n, 10, "sharpe n should be 10");

    // PSR should be present since n >= 3
    assert!(
        !sharpe["psr"].is_null(),
        "psr should be present (not null) for n=10"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Golden value drawdown (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_drawdown_json() {
    let tmp = tempfile::tempdir().unwrap();
    write_golden_fixture(tmp.path());

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "json",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    let drawdown = &json["drawdown"];
    assert!(!drawdown.is_null(), "drawdown should be present");

    let max_dd_abs = drawdown["max_drawdown_abs"]
        .as_f64()
        .expect("max_drawdown_abs f64");
    assert!(
        max_dd_abs > 0.0,
        "max_drawdown_abs should be > 0, got {max_dd_abs}"
    );

    let peak_date = drawdown["peak_date"]
        .as_str()
        .expect("peak_date should be a string");
    assert!(!peak_date.is_empty(), "peak_date should be non-empty");

    let trough_date = drawdown["trough_date"]
        .as_str()
        .expect("trough_date should be a string");
    assert!(!trough_date.is_empty(), "trough_date should be non-empty");
}

// ---------------------------------------------------------------------------
// Test 4: Table output contains scoring section headers
// ---------------------------------------------------------------------------

#[test]
fn table_output_contains_scoring_sections() {
    let tmp = tempfile::tempdir().unwrap();
    write_golden_fixture(tmp.path());

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "table",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(
        output.status.success(),
        "signal-scoring table mode exited with error: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("HIT RATES"),
        "table output should contain HIT RATES section"
    );
    assert!(
        stdout.contains("COST-ADJUSTED EDGE"),
        "table output should contain COST-ADJUSTED EDGE section"
    );
    assert!(
        stdout.contains("SHARPE RATIO"),
        "table output should contain SHARPE RATIO section"
    );
    assert!(
        stdout.contains("MAXIMUM DRAWDOWN"),
        "table output should contain MAXIMUM DRAWDOWN section"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Empty range produces graceful message
// ---------------------------------------------------------------------------

#[test]
fn empty_range_no_positions() {
    let tmp = tempfile::tempdir().unwrap();
    // No JSONL files written -- the directory is empty

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2099-01-01",
            "--to",
            "2099-01-02",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(
        output.status.success(),
        "signal-scoring should exit 0 on empty range: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No settled positions in range"),
        "stdout should contain 'No settled positions in range', got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Malformed JSONL lines skipped with warning
// ---------------------------------------------------------------------------

#[test]
fn malformed_lines_skipped_with_warning() {
    let tmp = tempfile::tempdir().unwrap();

    // 2026-01-15 00:00 UTC = 1769472000000
    let base_ts: i64 = 1769472000000;

    let mut lines = Vec::new();
    // 3 valid records
    for i in 0..3 {
        let record = make_settlement(
            "evt-malformed-test",
            true,
            i < 2,
            "1.00",
            "1.00",
            base_ts + (i as i64) * 3_600_000,
        );
        lines.push(serde_json::to_string(&record).expect("serialize"));
    }
    // Insert 1 garbage line
    lines.push("this is not valid json at all!!!".to_string());

    let file_path = tmp.path().join("settlements-2026-01-15.jsonl");
    std::fs::write(&file_path, lines.join("\n") + "\n").expect("write fixture");

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "json",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(
        output.status.success(),
        "signal-scoring should exit 0 despite malformed lines: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("malformed JSONL") || stderr.contains("Warning"),
        "stderr should mention malformed JSONL or Warning, got: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");
    let total = json["hit_rates"]["total"].as_u64().expect("total u64");
    assert_eq!(total, 3, "should have 3 valid records, got {total}");
}

// ---------------------------------------------------------------------------
// Test 7: Single record degenerate case (no panics)
// ---------------------------------------------------------------------------

#[test]
fn single_record_degenerate_case() {
    let tmp = tempfile::tempdir().unwrap();

    let record = make_settlement("evt-single", true, false, "2.50", "2.50", 1769472000000);
    let json_line = serde_json::to_string(&record).expect("serialize");
    let file_path = tmp.path().join("settlements-2026-01-15.jsonl");
    std::fs::write(&file_path, json_line + "\n").expect("write fixture");

    let bin = cargo_bin("signal-scoring");
    let output = Command::new(&bin)
        .args([
            "--from",
            "2026-01-15",
            "--to",
            "2026-01-15",
            "--output",
            "json",
            "--settlement-dir",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute signal-scoring");

    assert!(
        output.status.success(),
        "signal-scoring should not panic on single record: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON output");

    // hit_rates should be present (n=1 is valid for hit rates)
    assert!(
        !json["hit_rates"].is_null(),
        "hit_rates should be present for n=1"
    );
    let total = json["hit_rates"]["total"].as_u64().expect("total u64");
    assert_eq!(total, 1, "should have 1 record");

    // edge_test should be null (n=1, stddev=0 -> t-test undefined)
    assert!(
        json["edge_test"].is_null(),
        "edge_test should be null for n=1 (stddev=0), got: {}",
        json["edge_test"]
    );

    // sharpe should be null (n=1, stddev=0)
    assert!(
        json["sharpe"].is_null(),
        "sharpe should be null for n=1 (stddev=0), got: {}",
        json["sharpe"]
    );
}
