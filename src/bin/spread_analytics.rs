use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::PathBuf;

use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::{render_loading_summary, LoadingSummary, OutputFormat};
use prediction::analysis::spread_analytics::{
    analysis_tables, compute_analysis, group_by_event, FullSpreadOutput, SpreadAnalysis,
};
use prediction::spread::patterns::SpreadResult;

#[derive(Parser)]
#[command(name = "spread-analytics")]
#[command(about = "Analyze spread distribution patterns from recorded JSONL data")]
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

    /// Spread logs directory
    #[arg(long, default_value = "spread_logs")]
    log_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;
    let files = range.files_in_dir(&cli.log_dir);

    let result = load_jsonl::<SpreadResult>(&files);

    if result.errors > 0 {
        eprintln!("Warning: {} malformed JSONL lines skipped", result.errors);
    }

    // Always count unique events for LoadingSummary
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
        eprintln!("No spread data in range.");
        return Ok(());
    }

    // Compute aggregate analysis
    let aggregate = compute_analysis(&result.records);

    // Compute per-event analysis if --by-event
    let by_event: Option<BTreeMap<String, SpreadAnalysis>> = if cli.by_event {
        let groups = group_by_event(&result.records);
        let mut event_analyses = BTreeMap::new();
        for (event_id, refs) in groups {
            let owned: Vec<SpreadResult> = refs.into_iter().cloned().collect();
            event_analyses.insert(event_id, compute_analysis(&owned));
        }
        Some(event_analyses)
    } else {
        None
    };

    // Build full output
    let full_output = FullSpreadOutput {
        loading: summary.clone(),
        aggregate,
        by_event,
    };

    // Render output
    match cli.output {
        OutputFormat::Table => {
            render_loading_summary(&full_output.loading, &cli.output);

            // Aggregate analysis tables
            for (title, table) in analysis_tables(&full_output.aggregate) {
                println!();
                println!("--- {title} ---");
                println!("{table}");
            }

            // Per-event analysis if --by-event
            if let Some(ref events) = full_output.by_event {
                for (event_id, event_analysis) in events {
                    println!("\n=== Event: {event_id} ===");
                    for (title, table) in analysis_tables(event_analysis) {
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
