use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use std::path::PathBuf;

use prediction::analysis::go_no_go::{go_no_go_table, run_go_no_go, GoNoGoDecision};
use prediction::analysis::io::{load_jsonl, train_test_split, DateRange};
use prediction::analysis::output::OutputFormat;
use prediction::signal::types::ArbSignal;

#[derive(Parser)]
#[command(name = "go-no-go")]
#[command(about = "Statistical validation and go/no-go assessment for arbitrage signals")]
struct Cli {
    /// Start date (YYYY-MM-DD).
    #[arg(long)]
    from: Option<NaiveDate>,

    /// End date (YYYY-MM-DD).
    #[arg(long)]
    to: Option<NaiveDate>,

    /// Analyze last N days of signals (alternative to --from/--to).
    #[arg(long)]
    last: Option<u32>,

    /// Fraction of data to hold out for testing (0.0-1.0).
    #[arg(long, default_value = "0.3")]
    test_fraction: f64,

    /// Minimum effective sample size for go decision.
    #[arg(long, default_value = "30")]
    min_effective_n: usize,

    /// Output format: table (default) or json.
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Signal logs directory.
    #[arg(long, default_value = "signal_logs")]
    signal_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve date range -- default to last 30 days if no range specified
    let range = DateRange::from_args(cli.from, cli.to, cli.last)
        .or_else(|_| DateRange::from_args(None, None, Some(30)))?;

    // Split into train/test ranges
    let (train_range, test_range) = train_test_split(&range, cli.test_fraction);

    // Load signals
    let files = range.files_in_dir(&cli.signal_dir);
    let load_result = load_jsonl::<ArbSignal>(&files);

    eprintln!(
        "Loaded {} signals from {} files ({} parse errors)",
        load_result.records.len(),
        load_result.files_loaded,
        load_result.errors
    );

    if load_result.records.is_empty() {
        eprintln!("Error: no signals found in {} for range {}", cli.signal_dir.display(), range);
        std::process::exit(1);
    }

    let signals = load_result.records;

    // Run analysis
    let report = run_go_no_go(&signals, &train_range, &test_range, cli.min_effective_n);

    // Render output
    match cli.output {
        OutputFormat::Table => {
            let table = go_no_go_table(&report);
            println!("{table}");

            // Print warnings to stderr
            for warning in &report.warnings {
                eprintln!("WARNING: {warning}");
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
        }
    }

    // Exit code reflects decision
    match report.decision {
        GoNoGoDecision::Proceed => std::process::exit(0),
        GoNoGoDecision::DoNotProceed => std::process::exit(1),
        GoNoGoDecision::InsufficientData => std::process::exit(2),
    }
}
