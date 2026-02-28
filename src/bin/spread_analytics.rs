use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use std::path::PathBuf;

use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::{render_loading_summary, LoadingSummary, OutputFormat};
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

    // Count unique event_ids if --by-event is set
    let events_found = if cli.by_event {
        let mut event_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for record in &result.records {
            event_ids.insert(&record.event_id);
        }
        event_ids.len()
    } else {
        0
    };

    let summary = LoadingSummary {
        date_range: range.to_string(),
        files_loaded: result.files_loaded,
        files_missing: result.files_missing,
        records_loaded: result.records.len(),
        parse_errors: result.errors,
        events_found,
    };

    // Phase 27 will replace this with actual spread analysis rendering
    render_loading_summary(&summary, &cli.output);

    if cli.by_event && events_found > 0 {
        eprintln!(
            "Found {} unique events (per-event analysis added in Phase 27)",
            events_found
        );
    }

    Ok(())
}
