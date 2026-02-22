use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

use prediction::feed::pipeline::{self, DataMode};

#[derive(Parser)]
#[command(name = "prediction")]
#[command(about = "Cross-venue prediction market arbitrage signal generator")]
#[command(version)]
pub struct Cli {
    /// Directory containing config.toml, events.toml, venues.toml
    #[arg(long, default_value = "config")]
    pub config_dir: PathBuf,

    /// Run with synthetic mock data (no live connection)
    #[arg(long)]
    pub mock: bool,

    /// Replay from a JSONL recording file
    #[arg(long, value_name = "FILE")]
    pub replay: Option<PathBuf>,

    /// Replay speed multiplier (0=instant, 1=realtime, 10=fast)
    #[arg(long, default_value = "1.0")]
    pub speed: f64,

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
                    config.clone(),
                )?;

            // Determine data mode from CLI flags
            let mode = if let Some(ref path) = cli.replay {
                tracing::info!(
                    path = %path.display(),
                    speed = cli.speed,
                    "starting in replay mode"
                );
                DataMode::Replay {
                    path: path.clone(),
                    speed: cli.speed,
                }
            } else if cli.mock {
                tracing::info!("starting in mock mode (synthetic data)");
                DataMode::Mock
            } else {
                // Log which venues are configured for Live mode
                tracing::info!("starting in live mode");
                tracing::info!(
                    deribit = "available (public testnet)",
                    polymarket = "available (no auth for market data)",
                    kalshi = if config.credentials.kalshi_api_key_id.is_some()
                        && config.credentials.kalshi_private_key.is_some()
                    {
                        "available (credentials configured)"
                    } else {
                        "skipped (KALSHI_API_KEY_ID / KALSHI_PRIVATE_KEY not set)"
                    },
                    "venue availability"
                );
                DataMode::Live
            };

            // Start the multi-venue pipeline
            let recording_dir = PathBuf::from("recordings");
            let mut snapshot_rx = pipeline::run_multi_venue_pipeline(
                mode,
                &config.venues,
                &config.credentials,
                recording_dir,
                shutdown_token.clone(),
            )
            .await?;

            // Simple consumer: log snapshots to prove pipeline works end-to-end
            let consumer_cancel = shutdown_token.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = consumer_cancel.cancelled() => {
                            tracing::info!("snapshot consumer shutting down");
                            break;
                        }
                        snapshot = snapshot_rx.recv() => {
                            match snapshot {
                                Some(snap) => {
                                    tracing::info!(
                                        venue = %snap.venue,
                                        instrument = %snap.instrument_id,
                                        bid = ?snap.bid,
                                        ask = ?snap.ask,
                                        stale = snap.is_stale,
                                        seq = snap.sequence,
                                        "MarketSnapshot"
                                    );
                                }
                                None => {
                                    tracing::info!("snapshot channel closed");
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            // Wait for shutdown signal
            shutdown_token.cancelled().await;

            tracing::info!("shutdown complete");
            // _log_guard drops here, flushing remaining logs
            Ok(())
        }
    }
}
