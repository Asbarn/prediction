use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::PathBuf;

use prediction::analysis::cost_audit::{
    compute_cost_audit, cost_audit_table, CostAuditOutput, CostAuditResult,
};
use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::{render_loading_summary, LoadingSummary, OutputFormat};
use prediction::signal::types::ArbSignal;

#[derive(Parser)]
#[command(name = "cost-audit")]
#[command(about = "Decompose cost components per signal to identify negative edge sources")]
struct Cli {
    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    from: Option<NaiveDate>,

    /// End date (YYYY-MM-DD), defaults to today
    #[arg(long)]
    to: Option<NaiveDate>,

    /// Analyze last N days (alternative to --from/--to)
    #[arg(long)]
    last: Option<u32>,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Break down results by event_id
    #[arg(long)]
    by_event: bool,

    /// Signal logs directory
    #[arg(long, default_value = "signal_logs")]
    log_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;
    let files = range.files_in_dir(&cli.log_dir);

    let result = load_jsonl::<ArbSignal>(&files);

    if result.errors > 0 {
        eprintln!("Warning: {} malformed JSONL lines skipped", result.errors);
    }

    // Count unique events for LoadingSummary
    let events_found = {
        let mut event_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for record in &result.records {
            event_ids.insert(&record.event_id);
        }
        event_ids.len()
    };

    let summary = LoadingSummary {
        date_range: range.to_string(),
        files_loaded: result.files_loaded,
        files_missing: result.files_missing,
        records_loaded: result.records.len(),
        parse_errors: result.errors,
        events_found,
    };

    // Handle empty data case
    if result.records.is_empty() {
        render_loading_summary(&summary, &cli.output);
        eprintln!("No signal data in range.");
        return Ok(());
    }

    // Compute aggregate analysis
    let aggregate = compute_cost_audit(&result.records);

    // Compute per-event analysis if --by-event
    let by_event: Option<BTreeMap<String, CostAuditResult>> = if cli.by_event {
        let mut groups: BTreeMap<String, Vec<ArbSignal>> = BTreeMap::new();
        for record in &result.records {
            groups
                .entry(record.event_id.clone())
                .or_default()
                .push(record.clone());
        }
        let mut event_results = BTreeMap::new();
        for (event_id, signals) in &groups {
            event_results.insert(event_id.clone(), compute_cost_audit(signals));
        }
        Some(event_results)
    } else {
        None
    };

    // Build full output
    let full_output = CostAuditOutput {
        loading: summary.clone(),
        aggregate,
        by_event,
    };

    // Render output
    match cli.output {
        OutputFormat::Table => {
            render_loading_summary(&full_output.loading, &cli.output);

            println!();
            let table = cost_audit_table(&full_output.aggregate);
            println!("{table}");

            // Per-event tables if --by-event
            if let Some(ref events) = full_output.by_event {
                for (event_id, event_result) in events {
                    println!("\n=== Event: {event_id} ===");
                    let table = cost_audit_table(event_result);
                    println!("{table}");
                }
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&full_output)?;
            println!("{json}");
        }
    }

    Ok(())
}
