use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;

use prediction::analysis::cost_validate::{
    validate_signal_config, validation_table, ValidationReport,
};
use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::OutputFormat;
use prediction::analysis::sensitivity::{sensitivity_analysis, sensitivity_table, SensitivityReport};
use prediction::signal::config::SignalGenerationConfig;
use prediction::signal::types::ArbSignal;

#[derive(Parser)]
#[command(name = "cost-validate")]
#[command(about = "Validate cost model parameters against documented exchange fee schedules")]
struct Cli {
    /// Path to TOML config file (reads [signal_generation] section).
    /// If omitted, validates default configuration.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Output format: table (default) or json.
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Run sensitivity analysis on signal logs (perturbation-based impact ranking).
    #[arg(long)]
    sensitivity: bool,

    /// Signal logs directory (used with --sensitivity).
    #[arg(long, default_value = "signal_logs")]
    log_dir: PathBuf,

    /// Start date for signal loading (YYYY-MM-DD). Used with --sensitivity.
    #[arg(long)]
    from: Option<NaiveDate>,

    /// End date for signal loading (YYYY-MM-DD). Used with --sensitivity.
    #[arg(long)]
    to: Option<NaiveDate>,

    /// Analyze last N days of signals (alternative to --from/--to). Used with --sensitivity.
    #[arg(long)]
    last: Option<u32>,
}

/// Combined JSON output when --sensitivity is used.
#[derive(Serialize)]
struct CombinedOutput {
    validation: ValidationReport,
    sensitivity: SensitivityReport,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = if let Some(path) = &cli.config {
        load_config_from_file(path)?
    } else {
        SignalGenerationConfig::default()
    };

    let report = validate_signal_config(&config);

    if cli.sensitivity {
        run_with_sensitivity(&cli, &report)?;
    } else {
        run_validation_only(&cli, &report)?;
    }

    Ok(())
}

/// Run validation-only mode (original behavior).
fn run_validation_only(cli: &Cli, report: &ValidationReport) -> Result<()> {
    match cli.output {
        OutputFormat::Table => {
            let table = validation_table(report);
            println!("{table}");

            if report.is_clean() {
                eprintln!("All parameters validated (0 mismatches, 0 missing).");
            } else {
                eprintln!(
                    "VALIDATION ISSUES: {} mismatch(es), {} missing.",
                    report.mismatches, report.missing,
                );
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(report)?;
            println!("{json}");
        }
    }

    if !report.is_clean() {
        std::process::exit(1);
    }
    Ok(())
}

/// Run validation + sensitivity analysis mode.
fn run_with_sensitivity(cli: &Cli, report: &ValidationReport) -> Result<()> {
    // Resolve date range -- default to last 30 days if no range specified
    let range = DateRange::from_args(cli.from, cli.to, cli.last)
        .or_else(|_| DateRange::from_args(None, None, Some(30)))?;

    let files = range.files_in_dir(&cli.log_dir);
    let load_result = load_jsonl::<ArbSignal>(&files);

    if load_result.errors > 0 {
        eprintln!(
            "Warning: {} malformed JSONL lines skipped",
            load_result.errors
        );
    }

    let signals = load_result.records;

    match cli.output {
        OutputFormat::Table => {
            // Print validation report first
            let table = validation_table(report);
            println!("{table}");

            if report.is_clean() {
                eprintln!("All parameters validated (0 mismatches, 0 missing).");
            } else {
                eprintln!(
                    "VALIDATION ISSUES: {} mismatch(es), {} missing.",
                    report.mismatches, report.missing,
                );
            }

            // Then sensitivity analysis
            println!();
            if signals.is_empty() {
                eprintln!("No signal data found for sensitivity analysis.");
            } else {
                eprintln!(
                    "Sensitivity analysis: {} signals from {}",
                    signals.len(),
                    range
                );
                let sens_report = sensitivity_analysis(&signals);
                let table = sensitivity_table(&sens_report);
                println!("{table}");
            }
        }
        OutputFormat::Json => {
            if signals.is_empty() {
                // Just validation report when no signals
                let json = serde_json::to_string_pretty(report)?;
                println!("{json}");
                eprintln!("No signal data found for sensitivity analysis.");
            } else {
                let sens_report = sensitivity_analysis(&signals);
                let combined = CombinedOutput {
                    validation: report.clone(),
                    sensitivity: sens_report,
                };
                let json = serde_json::to_string_pretty(&combined)?;
                println!("{json}");
            }
        }
    }

    if !report.is_clean() {
        std::process::exit(1);
    }
    Ok(())
}

/// Load SignalGenerationConfig from a TOML file.
///
/// Expects either a `[signal_generation]` section in a full config file,
/// or a bare config at the top level.
fn load_config_from_file(path: &PathBuf) -> Result<SignalGenerationConfig> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading config: {}", path.display()))?;

    // Try parsing as a full config with [signal_generation] section first
    #[derive(serde::Deserialize)]
    struct FullConfig {
        #[serde(default)]
        signal_generation: SignalGenerationConfig,
    }

    // Try full config first, fall back to bare config
    if let Ok(full) = toml::from_str::<FullConfig>(&content) {
        Ok(full.signal_generation)
    } else {
        toml::from_str::<SignalGenerationConfig>(&content)
            .with_context(|| "parsing config as SignalGenerationConfig")
    }
}
