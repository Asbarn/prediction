use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;

use prediction::analysis::output::{new_table, set_numeric_columns, OutputFormat, Table};
use prediction::config::{
    Direction, EventMapping, EventsConfig, LifecycleStatus,
};

#[derive(Parser)]
#[command(name = "match-audit")]
#[command(about = "Audit event mappings for instrument quality and alignment issues")]
struct Cli {
    /// Directory containing events.toml
    #[arg(long, default_value = "config")]
    config_dir: PathBuf,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Only show mappings with issues
    #[arg(long)]
    issues_only: bool,

    /// BTC spot price for moneyness checks (omit to skip moneyness validation)
    #[arg(long)]
    spot: Option<f64>,

    /// Override default expiry tolerance in days (default 7)
    #[arg(long)]
    expiry_tolerance: Option<i64>,
}

// ---------------------------------------------------------------------------
// Audit result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct AuditResult {
    id: String,
    strike: String,
    direction: String,
    venue_count: usize,
    venue_labels: String,
    expiry_gap_days: Option<i64>,
    moneyness_pct: Option<f64>,
    status: AuditStatus,
    issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
enum AuditStatus {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "WARN")]
    Warn,
    #[serde(rename = "ERROR")]
    Error,
}

impl std::fmt::Display for AuditStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditStatus::Ok => write!(f, "OK"),
            AuditStatus::Warn => write!(f, "WARN"),
            AuditStatus::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditReport {
    events_checked: usize,
    passed: usize,
    issues: usize,
    results: Vec<AuditResult>,
}

// ---------------------------------------------------------------------------
// Venue helpers
// ---------------------------------------------------------------------------

/// Count venues and build a label string like "PM+DB" or "PM+DB+DV".
fn venue_info(mapping: &EventMapping) -> (usize, String) {
    let mut labels = Vec::new();
    if mapping.venues.polymarket.is_some() {
        labels.push("PM");
    }
    if mapping.venues.deribit.is_some() {
        labels.push("DB");
    }
    if mapping.venues.kalshi.is_some() {
        labels.push("KL");
    }
    if mapping.venues.derive.is_some() {
        labels.push("DV");
    }
    (labels.len(), labels.join("+"))
}

/// Parse expiry from a Deribit instrument name (e.g., "BTC-28MAR26-75000-C").
/// Returns the date component as a string for comparison.
fn parse_deribit_expiry(instrument: &str) -> Option<chrono::NaiveDate> {
    let parts: Vec<&str> = instrument.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    // parts[1] is like "28MAR26"
    let date_str = parts[1];
    // Try DDMMMYY format
    chrono::NaiveDate::parse_from_str(date_str, "%d%b%y").ok()
}

/// Parse expiry from a Derive instrument name (e.g., "BTC-20260328-75000-C").
/// Returns the date component.
fn parse_derive_expiry(instrument: &str) -> Option<chrono::NaiveDate> {
    let parts: Vec<&str> = instrument.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    // parts[1] is like "20260328"
    chrono::NaiveDate::parse_from_str(parts[1], "%Y%m%d").ok()
}

/// Parse option type from the last segment of a Deribit/Derive instrument name.
/// Returns 'C' (call) or 'P' (put).
fn parse_option_type(instrument: &str) -> Option<char> {
    let parts: Vec<&str> = instrument.split('-').collect();
    parts.last().and_then(|s| s.chars().next()).filter(|c| *c == 'C' || *c == 'P')
}

// ---------------------------------------------------------------------------
// Audit logic
// ---------------------------------------------------------------------------

fn audit_mapping(
    mapping: &EventMapping,
    spot: Option<f64>,
    expiry_tolerance: i64,
) -> AuditResult {
    let mut issues = Vec::new();
    let mut status = AuditStatus::Ok;

    let (venue_count, venue_labels) = venue_info(mapping);

    // Check 1: Venue count
    if venue_count < 2 {
        issues.push("fewer than 2 venues".to_string());
        status = AuditStatus::Error;
    }

    // Parse mapping expiry
    let mapping_expiry = chrono::NaiveDate::parse_from_str(&mapping.expiry, "%Y-%m-%d").ok();

    // Check 2: Expiry alignment
    let mut venue_expiries: Vec<(String, chrono::NaiveDate)> = Vec::new();
    if let Some(ref db) = mapping.venues.deribit {
        if let Some(d) = parse_deribit_expiry(&db.instrument) {
            venue_expiries.push(("Deribit".to_string(), d));
        }
    }
    if let Some(ref dv) = mapping.venues.derive {
        if let Some(d) = parse_derive_expiry(&dv.instrument) {
            venue_expiries.push(("Derive".to_string(), d));
        }
    }
    // Polymarket/Kalshi use the mapping expiry as their expiry (no instrument-level date)

    let mut max_gap_days: Option<i64> = None;
    if let Some(ref map_exp) = mapping_expiry {
        for (venue_name, venue_exp) in &venue_expiries {
            let gap = (*venue_exp - *map_exp).num_days().abs();
            max_gap_days = Some(max_gap_days.map_or(gap, |prev: i64| prev.max(gap)));
            if gap > expiry_tolerance {
                issues.push(format!(
                    "{} expiry gap {} days exceeds tolerance {}",
                    venue_name, gap, expiry_tolerance
                ));
                status = AuditStatus::Error;
            } else if gap > 3 {
                issues.push(format!("{} expiry gap {} days", venue_name, gap));
                if status != AuditStatus::Error {
                    status = AuditStatus::Warn;
                }
            }
        }
    }

    // Check 3: Direction consistency
    let expected_type = match mapping.direction {
        Direction::Above => 'C',
        Direction::Below => 'P',
    };
    for instrument_name in [
        mapping.venues.deribit.as_ref().map(|d| d.instrument.as_str()),
        mapping.venues.derive.as_ref().map(|d| d.instrument.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(opt_type) = parse_option_type(instrument_name) {
            if opt_type != expected_type {
                issues.push(format!(
                    "direction mismatch: mapping={} but instrument {} has type {}",
                    mapping.direction, instrument_name, opt_type
                ));
                status = AuditStatus::Error;
            }
        }
    }

    // Check 4: Moneyness
    let moneyness_pct = spot.and_then(|s| {
        mapping
            .strike
            .parse::<f64>()
            .ok()
            .map(|strike| ((strike - s) / s * 100.0).abs())
    });
    if let Some(pct) = moneyness_pct {
        if pct > 25.0 {
            issues.push(format!("deep OTM: {:.1}% from spot", pct));
            status = AuditStatus::Error;
        } else if pct > 10.0 {
            issues.push(format!("slightly OTM: {:.1}% from spot", pct));
            if status != AuditStatus::Error {
                status = AuditStatus::Warn;
            }
        }
    }

    AuditResult {
        id: mapping.id.clone(),
        strike: mapping.strike.clone(),
        direction: mapping.direction.to_string(),
        venue_count,
        venue_labels,
        expiry_gap_days: max_gap_days,
        moneyness_pct,
        status,
        issues,
    }
}

fn build_report_table(report: &AuditReport) -> Table {
    let mut table = new_table(&[
        "ID", "Strike", "Dir", "Venues", "Expiry Gap", "Moneyness", "Status", "Issues",
    ]);
    set_numeric_columns(&mut table, &[1, 4, 5]);

    for r in &report.results {
        table.add_row(vec![
            r.id.clone(),
            r.strike.clone(),
            r.direction.clone(),
            r.venue_labels.clone(),
            r.expiry_gap_days
                .map(|d| format!("{} days", d))
                .unwrap_or_else(|| "-".to_string()),
            r.moneyness_pct
                .map(|p| format!("{:.1}%", p))
                .unwrap_or_else(|| "-".to_string()),
            r.status.to_string(),
            if r.issues.is_empty() {
                "-".to_string()
            } else {
                r.issues.join("; ")
            },
        ]);
    }

    table
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();
    let expiry_tolerance = cli.expiry_tolerance.unwrap_or(7);

    // Load events.toml
    let events_path = cli.config_dir.join("events.toml");
    let events_config: EventsConfig = if events_path.exists() {
        let content = std::fs::read_to_string(&events_path)?;
        toml::from_str(&content)?
    } else {
        eprintln!(
            "Warning: {} not found, using empty events list",
            events_path.display()
        );
        EventsConfig {
            events: vec![],
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        }
    };

    // Filter to approved + active events
    let active_events: Vec<&EventMapping> = events_config
        .events
        .iter()
        .filter(|e| e.approved && e.status == LifecycleStatus::Active)
        .collect();

    // Audit each event
    let mut results: Vec<AuditResult> = Vec::new();
    for event in &active_events {
        let result = audit_mapping(event, cli.spot, expiry_tolerance);
        results.push(result);
    }

    // Filter to issues only if requested
    if cli.issues_only {
        results.retain(|r| r.status != AuditStatus::Ok);
    }

    let passed = results.iter().filter(|r| r.status == AuditStatus::Ok).count();
    let issues = results.iter().filter(|r| r.status != AuditStatus::Ok).count();
    let has_errors = results.iter().any(|r| r.status == AuditStatus::Error);

    let report = AuditReport {
        events_checked: active_events.len(),
        passed,
        issues,
        results,
    };

    match cli.output {
        OutputFormat::Table => {
            println!("Match Audit Report");
            println!("==================");
            println!("Events checked: {}", report.events_checked);
            println!("  Passed: {}", report.passed);
            println!("  Issues: {}", report.issues);
            println!();
            if !report.results.is_empty() {
                let table = build_report_table(&report);
                println!("{table}");
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
        }
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}
