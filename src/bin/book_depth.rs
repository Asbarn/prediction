use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

use prediction::analysis::book_depth::{book_depth_tables, compute_book_depth, BookDepthResult};
use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::{render_loading_summary, LoadingSummary, OutputFormat};
use prediction::signal::types::ArbSignal;

#[derive(Parser)]
#[command(name = "book-depth")]
#[command(about = "Analyze order book depth and fill quality from signal logs")]
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

    /// Break down results by event
    #[arg(long)]
    by_event: bool,

    /// Signal logs directory
    #[arg(long, default_value = "signal_logs")]
    log_dir: PathBuf,

    /// Target notional for fill simulation (default: 500)
    #[arg(long, default_value = "500.0")]
    target_notional: f64,
}

/// Full output wrapper for JSON serialization.
#[derive(Debug, Clone, Serialize)]
struct BookDepthCliOutput {
    loading: LoadingSummary,
    aggregate: BookDepthResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    by_event: Option<BTreeMap<String, BookDepthResult>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;
    let files = range.files_in_dir(&cli.log_dir);

    let result = load_jsonl::<ArbSignal>(&files);

    if result.errors > 0 {
        eprintln!("Warning: {} malformed JSONL lines skipped", result.errors);
    }

    // Count unique events for loading summary
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

    // Handle empty data
    if result.records.is_empty() {
        render_loading_summary(&summary, &cli.output);
        eprintln!("No signal data in range.");
        return Ok(());
    }

    // Compute aggregate
    let aggregate = compute_book_depth(&result.records, cli.target_notional);

    // Compute per-event if --by-event
    let by_event: Option<BTreeMap<String, BookDepthResult>> = if cli.by_event {
        let mut groups: BTreeMap<String, Vec<&ArbSignal>> = BTreeMap::new();
        for signal in &result.records {
            groups.entry(signal.event_id.clone()).or_default().push(signal);
        }
        let mut event_results = BTreeMap::new();
        for (event_id, sigs) in groups {
            let owned: Vec<ArbSignal> = sigs.into_iter().cloned().collect();
            event_results.insert(event_id, compute_book_depth(&owned, cli.target_notional));
        }
        Some(event_results)
    } else {
        None
    };

    // Build full output
    let full_output = BookDepthCliOutput {
        loading: summary.clone(),
        aggregate,
        by_event,
    };

    // Render
    match cli.output {
        OutputFormat::Table => {
            render_loading_summary(&full_output.loading, &cli.output);

            // Aggregate tables
            for (title, table) in book_depth_tables(&full_output.aggregate) {
                println!();
                println!("--- {title} ---");
                println!("{table}");
            }

            // Per-event tables
            if let Some(ref events) = full_output.by_event {
                for (event_id, event_result) in events {
                    println!("\n=== Event: {event_id} ===");
                    for (title, table) in book_depth_tables(event_result) {
                        println!();
                        println!("--- {title} ---");
                        println!("{table}");
                    }
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
