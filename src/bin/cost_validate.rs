use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use prediction::analysis::cost_validate::{validate_signal_config, validation_table};
use prediction::analysis::output::OutputFormat;
use prediction::signal::config::SignalGenerationConfig;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = if let Some(path) = &cli.config {
        load_config_from_file(path)?
    } else {
        SignalGenerationConfig::default()
    };

    let report = validate_signal_config(&config);

    match cli.output {
        OutputFormat::Table => {
            let table = validation_table(&report);
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
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
        }
    }

    if report.is_clean() {
        std::process::exit(0);
    } else {
        std::process::exit(1);
    }
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
