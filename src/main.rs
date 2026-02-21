use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "prediction")]
#[command(about = "Cross-venue prediction market arbitrage signal generator")]
#[command(version)]
pub struct Cli {
    /// Directory containing config.toml, events.toml, venues.toml
    #[arg(long, default_value = "config")]
    pub config_dir: PathBuf,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the main application (default if no subcommand given)
    Run,
    /// Validate configuration files without starting
    CheckConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::CheckConfig => {
            match prediction::config::load_config(&cli.config_dir) {
                Ok(_config) => {
                    println!("Configuration valid.");
                    println!("  System: {}", cli.config_dir.join("config.toml").display());
                    println!("  Events: {}", cli.config_dir.join("events.toml").display());
                    println!("  Venues: {}", cli.config_dir.join("venues.toml").display());
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Configuration error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Run => {
            // Load config (fail fast)
            let config = prediction::config::load_config(&cli.config_dir)?;

            // Initialize logging (must happen before anything else that logs)
            let _log_guard = prediction::logging::init_logging(
                &config.system.logging.log_dir,
                &config.system.logging.stdout_level,
                &config.system.logging.file_level,
            )?;

            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                config_dir = %cli.config_dir.display(),
                "prediction system starting"
            );

            // Setup graceful shutdown
            let shutdown_token = CancellationToken::new();
            tokio::spawn(prediction::shutdown::shutdown_signal(shutdown_token.clone()));

            // Start config hot-reload
            let (_config_reloader, _config_rx) =
                prediction::config::reload::ConfigReloader::start(
                    cli.config_dir.clone(),
                    config,
                )?;

            // Wait for shutdown signal
            shutdown_token.cancelled().await;

            tracing::info!("shutdown complete");
            // _log_guard drops here, flushing remaining logs
            Ok(())
        }
    }
}
