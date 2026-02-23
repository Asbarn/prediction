use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use prediction::config::DiscoveryConfig;
use prediction::events::lifecycle::ContractLifecycleManager;
use prediction::events::registry::EventRegistry;
use prediction::feed::pipeline::{self, DataMode};
use prediction::paper_trade::tracker::PaperTradeTracker;
use prediction::pricing::engine::PricingEngine;
use prediction::pricing::types::ImpliedProbability;
use prediction::spread::{SpreadEngine, SpreadResult};
use prediction::types::MarketSnapshot;

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

            // Install Prometheus metrics recorder BEFORE any task spawning.
            // This activates all existing metrics::counter!/gauge!/histogram!
            // calls throughout the feed layer. If setup fails (e.g., port in
            // use), log a warning and continue -- metrics are valuable but not
            // critical enough to block startup.
            let prometheus_port = config.system.prometheus.port;
            if let Err(e) = prediction::metrics_export::setup_prometheus(prometheus_port) {
                tracing::warn!(
                    port = prometheus_port,
                    error = %e,
                    "failed to start Prometheus metrics exporter, continuing without metrics"
                );
            }

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
            let is_live = cli.replay.is_none() && !cli.mock;

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

            // Build the shared EventRegistry
            let event_registry = Arc::new(RwLock::new(
                EventRegistry::from_config(&config.events),
            ));

            // Start the multi-venue pipeline
            let recording_dir = PathBuf::from("recordings");
            let snapshot_rx = pipeline::run_multi_venue_pipeline(
                mode,
                &config.venues,
                &config.credentials,
                recording_dir,
                shutdown_token.clone(),
                Some(event_registry.clone()),
            )
            .await?;

            // Start ContractLifecycleManager in Live mode only
            // (Mock/Replay modes don't need REST polling)
            if is_live {
                let discovery_config = config
                    .events
                    .discovery
                    .clone()
                    .unwrap_or_else(DiscoveryConfig::default);
                let risk_weights = config
                    .events
                    .risk_weights
                    .clone()
                    .unwrap_or_default();
                let lifecycle_cancel = shutdown_token.child_token();
                let lifecycle_manager = ContractLifecycleManager::new(
                    event_registry.clone(),
                    cli.config_dir.join("events.toml"),
                    discovery_config,
                    config.events.expiry_thresholds.clone(),
                    risk_weights,
                    config.venues.clone(),
                    config.credentials.clone(),
                    lifecycle_cancel,
                );
                tokio::spawn(lifecycle_manager.run());
                tracing::info!("ContractLifecycleManager started");
            }

            // -- Phase 7 Pipeline: Fan-out -> SpreadEngine + PricingEngine --
            //
            // Pipeline flow:
            //   [Multi-venue feeds] --snapshot_rx--> [SnapshotFanOut]
            //                                              |
            //                                              +--> [SpreadEngine] --signal_tx--> [PaperTradeTracker]
            //                                              |        |
            //                                              |        +--MarketSnapshot (clone)--^
            //                                              |
            //                                              +--> [PricingEngine] --probability_tx--> (Phase 8)

            // Fan-out channels: snapshot_rx -> spread + pricing
            let (spread_snap_tx, spread_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);
            let (pricing_snap_tx, pricing_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);

            // Spawn fan-out task: clone each snapshot to both engines
            let fanout_cancel = shutdown_token.child_token();
            tokio::spawn(async move {
                let mut snapshot_rx = snapshot_rx;
                loop {
                    tokio::select! {
                        biased;

                        _ = fanout_cancel.cancelled() => {
                            tracing::debug!("snapshot fan-out shutting down");
                            break;
                        }

                        snapshot = snapshot_rx.recv() => {
                            let Some(snapshot) = snapshot else {
                                tracing::debug!("snapshot fan-out: source channel closed");
                                break;
                            };

                            // Send to SpreadEngine (blocking -- spread pipeline is primary)
                            let snapshot_clone = snapshot.clone();
                            if spread_snap_tx.send(snapshot).await.is_err() {
                                tracing::debug!("spread engine channel closed");
                                break;
                            }

                            // Send to PricingEngine (best-effort, never block spread pipeline)
                            if let Err(_e) = pricing_snap_tx.try_send(snapshot_clone) {
                                tracing::trace!("pricing engine channel full, dropping snapshot");
                            }
                        }
                    }
                }
            });

            // Signal channel: SpreadEngine -> PaperTradeTracker
            let (signal_tx, signal_rx) = mpsc::channel::<SpreadResult>(1024);

            // Snapshot forwarding channel: SpreadEngine -> PaperTradeTracker
            // Paper trade tracker needs snapshots to fill pending positions and update MTM.
            let (ptrade_snap_tx, ptrade_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);

            // Spawn SpreadEngine (receives from fan-out, not directly from pipeline)
            let spread_config = config.system.spread.clone();
            let spread_engine = SpreadEngine::new(spread_config);
            let spread_cancel = shutdown_token.child_token();
            tokio::spawn(spread_engine.run(
                spread_snap_rx,
                event_registry.clone(),
                spread_cancel,
                signal_tx,
                Some(ptrade_snap_tx),
            ));

            // Spawn PaperTradeTracker
            let paper_trade_config = config.system.paper_trade.clone();
            let paper_tracker = PaperTradeTracker::new(paper_trade_config);
            let ptrade_cancel = shutdown_token.child_token();
            tokio::spawn(paper_tracker.run(signal_rx, ptrade_snap_rx, ptrade_cancel));

            // Spawn PricingEngine (receives from fan-out, outputs ImpliedProbability)
            let pricing_config = config.system.pricing.clone();
            let pricing_engine = PricingEngine::new(pricing_config);
            let pricing_cancel = shutdown_token.child_token();
            // ImpliedProbability channel: held in scope so PricingEngine's try_send works.
            // Phase 8 will consume _probability_rx.
            let (probability_tx, _probability_rx) = mpsc::channel::<ImpliedProbability>(1024);
            tokio::spawn(pricing_engine.run(pricing_snap_rx, probability_tx, pricing_cancel));

            tracing::info!(
                near_expiry_cutoff_hours = config.system.pricing.near_expiry_cutoff_hours,
                iv_bounds = format!("[{}, {}]", config.system.pricing.solver.iv_min, config.system.pricing.solver.iv_max),
                "spread engine, paper trade tracker, and pricing engine started"
            );

            // Wait for shutdown signal
            shutdown_token.cancelled().await;

            tracing::info!("shutdown complete");
            // _log_guard drops here, flushing remaining logs
            Ok(())
        }
    }
}
