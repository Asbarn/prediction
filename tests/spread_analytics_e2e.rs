//! End-to-end integration tests for the spread-analytics CLI binary.
//!
//! These tests invoke the compiled `spread-analytics` binary with synthetic
//! fixture data and verify outputs against hand-computed golden values.
//! Each test uses its own tempdir for isolation and parallel safety.

use prediction::spread::patterns::{SpreadPattern, SpreadResult};
use rust_decimal::Decimal;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the path to a compiled binary in the cargo target directory.
fn cargo_bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe()
        .unwrap()
        .parent() // deps/
        .unwrap()
        .parent() // debug/ or release/
        .unwrap()
        .to_path_buf();
    path.push(name);
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

/// Build a SpreadResult with the given fields and sensible defaults.
fn make_spread(
    event_id: &str,
    pattern: SpreadPattern,
    net: &str,
    gross: &str,
    ts_ms: i64,
) -> SpreadResult {
    SpreadResult {
        event_id: event_id.to_string(),
        pattern,
        gross_spread: dec(gross),
        net_spread: dec(net),
        buy_fill_price: dec("0.50"),
        sell_fill_price: dec("0.55"),
        buy_fee: dec("0"),
        sell_fee: dec("0"),
        carry_cost: dec("0"),
        total_cost: dec("0"),
        basis_risk_premium: dec("0"),
        buy_fill_ratio: dec("1.0"),
        sell_fill_ratio: dec("1.0"),
        target_notional: dec("500"),
        timestamp_ms: ts_ms,
        poly_exchange_ts: None,
        kalshi_exchange_ts: None,
        threshold: None,
        threshold_components: None,
        threshold_status: None,
    }
}

/// Write a set of SpreadResult records to a JSONL file in the given directory.
fn write_jsonl(dir: &std::path::Path, filename: &str, records: &[SpreadResult]) {
    let lines: Vec<String> = records
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    std::fs::write(dir.join(filename), lines.join("\n") + "\n").unwrap();
}

/// Create the standard 5-record fixture for distribution and hourly tests.
///
/// Net spreads: [0.02, 0.04, 0.01, 0.03, 0.05]
/// Gross spreads: [0.05, 0.07, 0.04, 0.06, 0.08]
/// Timestamps: 2026-01-15 hours 0-4 UTC
///
/// Hand-computed golden values:
///   net mean  = 0.03
///   net median = 0.03
///   net min = 0.01, max = 0.05
///   net stddev = sqrt(0.001/4) = 0.015811...
///   gross mean = 0.06
fn standard_fixture() -> Vec<SpreadResult> {
    vec![
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.02", "0.05", 1769472000000), // 2026-01-15 00:00 UTC
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.04", "0.07", 1769475600000), // 2026-01-15 01:00 UTC
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.01", "0.04", 1769479200000), // 2026-01-15 02:00 UTC
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.03", "0.06", 1769482800000), // 2026-01-15 03:00 UTC
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.05", "0.08", 1769486400000), // 2026-01-15 04:00 UTC
    ]
}

/// Assert that two f64 values are within the given epsilon of each other.
fn assert_approx(actual: f64, expected: f64, epsilon: f64, label: &str) {
    assert!(
        (actual - expected).abs() < epsilon,
        "{label}: expected {expected}, got {actual} (epsilon={epsilon})"
    );
}

// ---------------------------------------------------------------------------
// Test 1: Golden value distribution (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_distribution_json() {
    let dir = tempfile::tempdir().unwrap();
    let records = standard_fixture();
    write_jsonl(dir.path(), "2026-01-15.jsonl", &records);

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2026-01-15",
            "--to", "2026-01-15",
            "--output", "json",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(
        output.status.success(),
        "spread-analytics should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let dist = &json["aggregate"]["distribution"];
    let net = &dist["net_spread"];
    let gross = &dist["gross_spread"];

    // Net spread assertions
    assert_eq!(net["count"].as_u64().unwrap(), 5);
    assert_approx(net["mean"].as_f64().unwrap(), 0.03, 1e-6, "net mean");
    assert_approx(net["median"].as_f64().unwrap(), 0.03, 1e-6, "net median");
    assert_approx(net["min"].as_f64().unwrap(), 0.01, 1e-6, "net min");
    assert_approx(net["max"].as_f64().unwrap(), 0.05, 1e-6, "net max");
    assert_approx(
        net["stddev"].as_f64().unwrap(),
        0.015811,
        1e-4,
        "net stddev",
    );

    // Gross spread assertions
    assert_approx(gross["mean"].as_f64().unwrap(), 0.06, 1e-6, "gross mean");
}

// ---------------------------------------------------------------------------
// Test 2: Golden value hourly breakdown (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_hourly_json() {
    let dir = tempfile::tempdir().unwrap();
    let records = standard_fixture();
    write_jsonl(dir.path(), "2026-01-15.jsonl", &records);

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2026-01-15",
            "--to", "2026-01-15",
            "--output", "json",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let hourly = &json["aggregate"]["hourly"];
    let rows = hourly["rows"].as_array().expect("hourly.rows should be an array");

    assert_eq!(rows.len(), 24, "hourly breakdown should have 24 rows");

    // Hours 0-4 should each have count=1
    for h in 0..5u64 {
        let row = &rows[h as usize];
        assert_eq!(
            row["count"].as_u64().unwrap(),
            1,
            "hour {h} should have count=1"
        );
    }

    // Hours 5-23 should have count=0
    for h in 5..24u64 {
        let row = &rows[h as usize];
        assert_eq!(
            row["count"].as_u64().unwrap(),
            0,
            "hour {h} should have count=0"
        );
    }

    // Verify specific hour mean values
    // Hour 0: net_spread=0.02, so mean=0.02
    assert_approx(
        rows[0]["mean"].as_f64().unwrap(),
        0.02,
        1e-6,
        "hour 0 mean_net",
    );
    // Hour 4: net_spread=0.05, so mean=0.05
    assert_approx(
        rows[4]["mean"].as_f64().unwrap(),
        0.05,
        1e-6,
        "hour 4 mean_net",
    );
}

// ---------------------------------------------------------------------------
// Test 3: Golden value venue-pair breakdown (JSON output)
// ---------------------------------------------------------------------------

#[test]
fn golden_value_venue_pair_json() {
    let dir = tempfile::tempdir().unwrap();

    // 3 records with BuyPolyYesSellKalshiYes, 2 records with BuyKalshiYesSellPolyYes
    // (Both map to venue_pair_label "kalshi_polymarket" but are different directions)
    let records = vec![
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.02", "0.05", 1769472000000),
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.04", "0.07", 1769475600000),
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.01", "0.04", 1769479200000),
        make_spread("evt1", SpreadPattern::SellPolyYesBuyKalshiYes, "0.03", "0.06", 1769482800000),
        make_spread("evt1", SpreadPattern::SellPolyYesBuyKalshiYes, "0.05", "0.08", 1769486400000),
    ];
    write_jsonl(dir.path(), "2026-01-15.jsonl", &records);

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2026-01-15",
            "--to", "2026-01-15",
            "--output", "json",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let venue_pairs = &json["aggregate"]["venue_pairs"];
    let pairs = venue_pairs["pairs"]
        .as_array()
        .expect("venue_pairs.pairs should be an array");

    // All 5 records are kalshi_polymarket, so there should be 1 pair
    assert!(
        !pairs.is_empty(),
        "should have at least one venue pair entry"
    );

    let pair = &pairs[0];
    assert_eq!(pair["pair_label"].as_str().unwrap(), "kalshi_polymarket");

    // The pair should have two direction entries
    let directions = pair["directions"].as_object().unwrap();
    assert_eq!(
        directions.len(),
        2,
        "should have 2 direction entries (buy_poly_yes_sell_kalshi_yes and sell_poly_yes_buy_kalshi_yes)"
    );

    // Verify direction counts
    let buy_poly = &directions["buy_poly_yes_sell_kalshi_yes"];
    assert_eq!(buy_poly["count"].as_u64().unwrap(), 3);

    let sell_poly = &directions["sell_poly_yes_buy_kalshi_yes"];
    assert_eq!(sell_poly["count"].as_u64().unwrap(), 2);

    // Total should be 5
    assert_eq!(pair["total"]["count"].as_u64().unwrap(), 5);
}

// ---------------------------------------------------------------------------
// Test 4: Table output contains expected sections
// ---------------------------------------------------------------------------

#[test]
fn table_output_contains_expected_sections() {
    let dir = tempfile::tempdir().unwrap();
    let records = standard_fixture();
    write_jsonl(dir.path(), "2026-01-15.jsonl", &records);

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2026-01-15",
            "--to", "2026-01-15",
            "--output", "table",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(
        output.status.success(),
        "exit code should be 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Distribution Summary"),
        "table output should contain 'Distribution Summary' section header"
    );

    // Mean is 0.03, with SPREAD_DP=4 it should display as "0.0300"
    assert!(
        stdout.contains("0.0300"),
        "table output should contain formatted mean '0.0300'"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Empty range produces "No spread data" message
// ---------------------------------------------------------------------------

#[test]
fn empty_range_no_data() {
    let dir = tempfile::tempdir().unwrap();
    // No JSONL files in the directory

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2099-01-01",
            "--to", "2099-01-02",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(output.status.success(), "exit code should be 0 even with no data");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No spread data in range"),
        "stderr should contain 'No spread data in range', got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Malformed lines skipped with warning
// ---------------------------------------------------------------------------

#[test]
fn malformed_lines_skipped_with_warning() {
    let dir = tempfile::tempdir().unwrap();

    // Create 2 valid SpreadResult records
    let valid_records = vec![
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.02", "0.05", 1769472000000),
        make_spread("evt1", SpreadPattern::BuyPolyYesSellKalshiYes, "0.04", "0.07", 1769475600000),
    ];
    let valid_lines: Vec<String> = valid_records
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();

    // Interleave one garbage line
    let content = format!(
        "{}\nthis is not json\n{}\n",
        valid_lines[0], valid_lines[1]
    );
    std::fs::write(dir.path().join("2026-01-15.jsonl"), content).unwrap();

    let output = Command::new(cargo_bin("spread-analytics"))
        .args([
            "--from", "2026-01-15",
            "--to", "2026-01-15",
            "--output", "json",
            "--log-dir",
        ])
        .arg(dir.path())
        .output()
        .expect("failed to run spread-analytics");

    assert!(
        output.status.success(),
        "exit code should be 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // stderr should contain malformed warning
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("malformed JSONL") || stderr.contains("Warning"),
        "stderr should mention malformed lines, got: {stderr}"
    );

    // JSON output should show count=2 (only the 2 valid records)
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should be valid JSON despite malformed input lines");

    let count = json["aggregate"]["distribution"]["net_spread"]["count"]
        .as_u64()
        .unwrap();
    assert_eq!(count, 2, "should process only the 2 valid records, got {count}");
}
