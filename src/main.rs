use std::path::PathBuf;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime, TimeZone};
use clap::{Parser, Subcommand};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use prediction::alert::{AlertMonitor, PipelineLiveness};
use prediction::config::DiscoveryConfig;
use prediction::events::lifecycle::ContractLifecycleManager;
use prediction::events::registry::EventRegistry;
use prediction::events::new_basis_risk_cache;
use prediction::events::risk::{compute_risk_for_mapping, check_expiry_warning, inflate_risk_score, CachedRiskInfo};
use prediction::feed::pipeline::{self, DataMode};
use prediction::health::{HealthState, start_health_server};
use prediction::paper_trade::tracker::PaperTradeTracker;
use prediction::pricing::engine::PricingEngine;
use prediction::pricing::types::ImpliedProbability;
use prediction::signal::{ArbSignal, CrossAssetEngine};
use prediction::spread::{SpreadEngine, SpreadResult};
use prediction::types::{MarketSnapshot, Venue};

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

    /// Replay from a recordings directory (e.g., recordings/) containing
    /// per-venue subdirectories (deribit/, polymarket/, kalshi/) with JSONL files
    #[arg(long, value_name = "DIR")]
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
            let (_config_reloader, config_rx) =
                prediction::config::reload::ConfigReloader::start(
                    cli.config_dir.clone(),
                    config.clone(),
                )?;

            // Determine data mode from CLI flags
            let is_replay = cli.replay.is_some();
            let is_live = !is_replay && !cli.mock;

            let mode = if let Some(ref path) = cli.replay {
                tracing::info!(
                    path = %path.display(),
                    speed = cli.speed,
                    replay_mode = true,
                    staleness_bypass = true,
                    "starting in replay mode (multi-venue recordings directory)"
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

            // Create shared BasisRiskCache for settlement risk data
            let basis_risk_cache = new_basis_risk_cache();

            // Create shared pipeline liveness tracker for alert monitoring (Phase 14)
            let pipeline_liveness = PipelineLiveness::new();

            // Pre-populate cache for replay/mock mode (lifecycle manager doesn't run)
            if !is_live {
                let risk_weights = config.events.risk_weights.clone().unwrap_or_default();
                let expiry_thresholds = &config.events.expiry_thresholds;
                let reg = event_registry.read().await;
                let mut cache = basis_risk_cache.write().await;
                let now = chrono::Utc::now();
                for mapping in reg.active_approved() {
                    if let Some(base_score) = compute_risk_for_mapping(mapping, &risk_weights) {
                        let expiry_date = NaiveDate::parse_from_str(&mapping.expiry, "%Y-%m-%d").ok();
                        let expiry_warning = expiry_date.and_then(|d| {
                            let t = NaiveTime::from_hms_opt(8, 0, 0)?;
                            let dt = chrono::Utc.from_local_datetime(&d.and_time(t)).single()?;
                            check_expiry_warning(&dt, &now, expiry_thresholds)
                        });
                        let effective_composite = match &expiry_warning {
                            Some(w) => inflate_risk_score(&base_score, w.risk_inflation_factor).composite,
                            None => base_score.composite,
                        };
                        let temporal_mismatch_hours = base_score.settlement_time_risk
                            / risk_weights.time_per_hour.max(0.001);
                        cache.insert(mapping.id.clone(), CachedRiskInfo {
                            base_score,
                            expiry_warning,
                            effective_composite,
                            temporal_mismatch_hours,
                            updated_at: now,
                        });
                    }
                }
                if !cache.is_empty() {
                    tracing::info!(entries = cache.len(), "BasisRiskCache pre-populated for replay/mock mode");
                }
                drop(cache);
                drop(reg);
            }

            // Start the multi-venue pipeline
            let recording_dir = PathBuf::from("recordings");
            let pipeline_handles = pipeline::run_multi_venue_pipeline(
                mode,
                &config.venues,
                &config.credentials,
                recording_dir,
                shutdown_token.clone(),
                Some(event_registry.clone()),
            )
            .await?;
            let snapshot_rx = pipeline_handles.snapshot_rx;

            // Clone venue_health before health_state takes ownership (needed for AlertMonitor)
            let venue_health_for_alerts = pipeline_handles.venue_health.clone();

            // Start HTTP /health endpoint (Phase 9)
            if config.system.health.enabled {
                let health_state = HealthState {
                    venue_health: pipeline_handles.venue_health,
                    event_registry: event_registry.clone(),
                    started_at: chrono::Utc::now(),
                };
                let health_port = config.system.health.port;
                tokio::spawn(start_health_server(health_state, health_port));
                tracing::info!(port = health_port, "health endpoint enabled");
            }

            // Start AlertMonitor for failure detection (Phase 14)
            if config.system.alerting.enabled {
                let alert_config = config.system.alerting.clone();
                let alert_cancel = shutdown_token.child_token();
                let alert_monitor = AlertMonitor::new(
                    venue_health_for_alerts,
                    pipeline_liveness.clone(),
                    alert_config,
                    alert_cancel,
                );
                tokio::spawn(alert_monitor.run());
                tracing::info!(
                    check_interval_secs = config.system.alerting.check_interval_secs,
                    "AlertMonitor started"
                );
            }

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
                    basis_risk_cache.clone(),
                );
                tokio::spawn(lifecycle_manager.run());
                tracing::info!("ContractLifecycleManager started");
            }

            // Config hot-reload: refresh EventRegistry on TOML changes (live mode only)
            // Replay must be deterministic so we skip config watching in non-live modes.
            if is_live {
                let config_cancel = shutdown_token.child_token();
                let config_registry = event_registry.clone();
                tokio::spawn(async move {
                    let mut config_rx = config_rx;
                    loop {
                        tokio::select! {
                            biased;
                            _ = config_cancel.cancelled() => {
                                tracing::debug!("config watch subscriber shutting down");
                                break;
                            }
                            result = config_rx.changed() => {
                                match result {
                                    Ok(()) => {
                                        let new_config = config_rx.borrow_and_update().clone();
                                        let mut reg = config_registry.write().await;
                                        reg.refresh(&new_config.events);
                                        tracing::info!(
                                            mappings = reg.mapping_count(),
                                            "EventRegistry refreshed from config hot-reload"
                                        );
                                    }
                                    Err(_) => {
                                        tracing::debug!("config watch channel closed");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                });
                tracing::info!("config hot-reload subscriber started");
            }

            // -- Phase 8 Pipeline: 3-way Fan-out -> SpreadEngine + PricingEngine + CrossAssetEngine --
            //
            // Pipeline flow:
            //   [Multi-venue feeds] --snapshot_rx--> [SnapshotFanOut]
            //                                              |
            //                                              +--> [SpreadEngine] --signal_tx--> [PaperTradeTracker]
            //                                              |        |
            //                                              |        +--MarketSnapshot (clone)--^
            //                                              |
            //                                              +--> [PricingEngine] --probability_tx--> [CrossAssetEngine]
            //                                              |                                              |
            //                                              +--MarketSnapshot (clone)--------------------->+
            //                                                                                             |
            //                                                                              arb_signal_tx --> (Phase 9)

            // Fan-out channels: snapshot_rx -> spread + pricing + signal engine
            let (spread_snap_tx, spread_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);
            let (pricing_snap_tx, pricing_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);
            let (signal_pred_snap_tx, signal_pred_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);

            // Spawn fan-out task: clone each snapshot to all three engines
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

                            // Clone for PricingEngine and CrossAssetEngine before
                            // blocking send to SpreadEngine
                            let snap_for_pricing = snapshot.clone();
                            let snap_for_signal = snapshot.clone();

                            // Send to SpreadEngine (blocking -- spread pipeline is primary)
                            if spread_snap_tx.send(snapshot).await.is_err() {
                                tracing::debug!("spread engine channel closed");
                                break;
                            }

                            // Send to PricingEngine (best-effort, never block spread pipeline)
                            if let Err(_e) = pricing_snap_tx.try_send(snap_for_pricing) {
                                tracing::trace!("pricing engine channel full, dropping snapshot");
                            }

                            // Send to CrossAssetEngine (best-effort, same as PricingEngine)
                            if let Err(_e) = signal_pred_snap_tx.try_send(snap_for_signal) {
                                tracing::trace!("signal engine channel full, dropping snapshot");
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
            let spread_engine = SpreadEngine::new(spread_config)
                .with_replay_mode(is_replay)
                .with_basis_risk_cache(basis_risk_cache.clone())
                .with_liveness(pipeline_liveness.clone());
            let spread_cancel = shutdown_token.child_token();
            tokio::spawn(spread_engine.run(
                spread_snap_rx,
                event_registry.clone(),
                spread_cancel,
                signal_tx,
                Some(ptrade_snap_tx),
            ));

            // -- State Persistence Recovery (Phase 15) --
            let paper_trade_config = config.system.paper_trade.clone();
            let persistence_config = config.system.persistence.clone();
            let settlement_config = config.system.settlement.clone();
            let settlement_log_dir = settlement_config.settlement_log_dir.clone();
            let analysis_config = config.system.analysis.clone();
            let mut paper_tracker =
                PaperTradeTracker::new(paper_trade_config, &settlement_log_dir, analysis_config);

            // Track checkpoint data for SettlementMonitor restore
            let mut recovered_checkpoint_ts: Option<i64> = None;
            let mut recovered_checkpoint: Option<
                prediction::persistence::checkpoint::CheckpointState,
            > = None;

            if persistence_config.enabled {
                let checkpoint_dir = std::path::Path::new(&persistence_config.checkpoint_dir);

                // Load checkpoint if exists
                match prediction::persistence::recovery::load_checkpoint(checkpoint_dir) {
                    Ok(Some(state)) => {
                        let checkpoint_ts = state.checkpoint_timestamp_ms;
                        let open_count = state.open.len();
                        let trades = state.total_trades;

                        // Save for settlement restore
                        recovered_checkpoint_ts = Some(checkpoint_ts);
                        recovered_checkpoint = Some(state.clone());

                        paper_tracker.restore_state(state);

                        tracing::info!(
                            checkpoint_timestamp_ms = checkpoint_ts,
                            open_positions = open_count,
                            total_trades = trades,
                            "restored paper trade state from checkpoint"
                        );

                        // Replay JSONL trade events after checkpoint
                        let log_dir = std::path::Path::new(&config.system.paper_trade.log_dir);
                        match prediction::persistence::recovery::replay_trade_events(
                            log_dir,
                            checkpoint_ts,
                        ) {
                            Ok(events) => {
                                let replay_count = events.len();
                                for event in &events {
                                    paper_tracker.apply_trade_event(event);
                                }
                                if replay_count > 0 {
                                    tracing::info!(
                                        replayed = replay_count,
                                        "JSONL trade event replay complete"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "failed to replay trade events, continuing with checkpoint state only"
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::info!("no checkpoint found, starting with fresh state");
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "failed to load checkpoint, starting with fresh state"
                        );
                    }
                }

                // Enable periodic checkpointing
                paper_tracker = paper_tracker.with_persistence(
                    checkpoint_dir.to_path_buf(),
                    persistence_config.checkpoint_interval_secs,
                );

                tracing::info!(
                    checkpoint_dir = %persistence_config.checkpoint_dir,
                    interval_secs = persistence_config.checkpoint_interval_secs,
                    "state persistence enabled"
                );
            }

            // -- Phase 16: Settlement Outcome Tracking --
            let (settlement_tx, settlement_rx) =
                mpsc::channel::<prediction::settlement::types::SettlementOutcome>(256);

            // Shared settlement tracking state for checkpoint persistence
            let settlement_tracking_state = Arc::new(RwLock::new(
                std::collections::HashMap::<
                    String,
                    Vec<prediction::persistence::checkpoint::SettlementTrackingEntry>,
                >::new(),
            ));

            // Wire shared tracking state to paper tracker for checkpoint inclusion
            paper_tracker.set_settlement_tracking_state(settlement_tracking_state.clone());

            // Extract open positions before run() consumes self
            let paper_tracker_open_positions = paper_tracker.open_positions().to_vec();

            if settlement_config.enabled && is_live {
                use prediction::settlement::monitor::SettlementMonitor;
                use prediction::settlement::deribit::DeribitResolutionChecker;
                use prediction::settlement::kalshi::KalshiResolutionChecker;
                use prediction::settlement::polymarket::PolymarketResolutionChecker;
                use prediction::settlement::traits::VenueChecker;
                use prediction::feed::reliability::VenueRateLimiter;

                let settlement_cancel = shutdown_token.child_token();

                // Shared HTTP client for all settlement REST calls
                let settlement_http_client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .expect("settlement HTTP client");

                let mut checkers = std::collections::HashMap::new();

                // -- Deribit checker (always available, public API, no auth) --
                {
                    // Derive REST base URL from WS URL:
                    //   wss://test.deribit.com/ws/api/v2 -> https://test.deribit.com
                    let deribit_rest_url = {
                        let ws = &config.venues.deribit.ws_url;
                        let base = ws
                            .replace("wss://", "https://")
                            .replace("ws://", "http://");
                        match base.find("/ws/") {
                            Some(pos) => base[..pos].to_string(),
                            None => base,
                        }
                    };
                    let deribit_limiter = pipeline_handles
                        .venue_rate_limiters
                        .get(&Venue::Deribit)
                        .cloned()
                        .unwrap_or_else(|| VenueRateLimiter::new("deribit_settlement", 5));
                    let deribit_checker = DeribitResolutionChecker::new(
                        settlement_http_client.clone(),
                        deribit_rest_url,
                        deribit_limiter,
                    );
                    checkers.insert(Venue::Deribit, VenueChecker::Deribit(deribit_checker));
                }

                // -- Kalshi checker (conditional on credentials) --
                {
                    let api_key_id = config.credentials.kalshi_api_key_id.clone();
                    let private_key_pem = config
                        .credentials
                        .kalshi_private_key
                        .clone()
                        .or_else(|| {
                            config
                                .venues
                                .kalshi
                                .private_key_path
                                .as_ref()
                                .and_then(|path| match std::fs::read_to_string(path) {
                                    Ok(content) => Some(content),
                                    Err(e) => {
                                        tracing::warn!(
                                            path = %path,
                                            error = %e,
                                            "failed to read Kalshi private key file for settlement"
                                        );
                                        None
                                    }
                                })
                        });

                    match (api_key_id, private_key_pem) {
                        (Some(key_id), Some(pem)) => {
                            match prediction::feed::kalshi::auth::load_kalshi_private_key(&pem) {
                                Ok(private_key) => {
                                    let kalshi_limiter = pipeline_handles
                                        .venue_rate_limiters
                                        .get(&Venue::Kalshi)
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            VenueRateLimiter::new("kalshi_settlement", 5)
                                        });
                                    let kalshi_checker = KalshiResolutionChecker::new(
                                        settlement_http_client.clone(),
                                        config.venues.kalshi.rest_url.clone(),
                                        key_id,
                                        private_key,
                                        kalshi_limiter,
                                    );
                                    checkers.insert(
                                        Venue::Kalshi,
                                        VenueChecker::Kalshi(kalshi_checker),
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "Kalshi RSA key invalid, settlement checker skipped"
                                    );
                                }
                            }
                        }
                        _ => {
                            tracing::warn!(
                                "Kalshi credentials not configured, settlement checker skipped"
                            );
                        }
                    }
                }

                // -- Polymarket checker (always available, no auth) --
                {
                    let poly_limiter = pipeline_handles
                        .venue_rate_limiters
                        .get(&Venue::Polymarket)
                        .cloned()
                        .unwrap_or_else(|| VenueRateLimiter::new("polymarket_settlement", 5));
                    let poly_checker = PolymarketResolutionChecker::new(
                        settlement_http_client.clone(),
                        config.venues.polymarket.gamma_api_url.clone(),
                        settlement_config.polymarket_price_lock_threshold,
                        poly_limiter,
                    );
                    checkers.insert(
                        Venue::Polymarket,
                        VenueChecker::Polymarket(poly_checker),
                    );
                }

                tracing::info!(
                    checkers = checkers.len(),
                    deribit = checkers.contains_key(&Venue::Deribit),
                    kalshi = checkers.contains_key(&Venue::Kalshi),
                    polymarket = checkers.contains_key(&Venue::Polymarket),
                    "settlement venue checkers registered"
                );

                let mut settlement_monitor = SettlementMonitor::new(
                    event_registry.clone(),
                    checkers,
                    settlement_tx.clone(),
                    pipeline_liveness.clone(),
                    settlement_config.clone(),
                    settlement_cancel,
                    Some(settlement_tracking_state.clone()),
                );

                // Restore settlement tracking from checkpoint (preserves polling tiers)
                if let Some(ref checkpoint) = recovered_checkpoint {
                    if !checkpoint.settlement_tracking.is_empty() {
                        settlement_monitor
                            .restore_tracking(checkpoint.settlement_tracking.clone());
                        tracing::info!(
                            events = checkpoint.settlement_tracking.len(),
                            "restored settlement tracking from checkpoint"
                        );
                    }
                }

                // Initialize from open positions (adds new events not in checkpoint)
                settlement_monitor.initialize_from_registry(&paper_tracker_open_positions);

                // Enqueue backfill if checkpoint was loaded
                if let Some(checkpoint_ts) = recovered_checkpoint_ts {
                    settlement_monitor
                        .enqueue_backfill(&paper_tracker_open_positions, checkpoint_ts);

                    // Drain and send backfill timeouts
                    let timeouts = settlement_monitor.drain_backfill_timeouts();
                    for timeout in timeouts {
                        if let Err(e) = settlement_tx.send(timeout).await {
                            tracing::warn!(error = %e, "failed to send backfill timeout");
                        }
                    }
                }

                tokio::spawn(settlement_monitor.run());
                tracing::info!("SettlementMonitor started");
            } else {
                if !settlement_config.enabled {
                    tracing::info!("settlement monitoring disabled");
                }
                // Keep settlement_tx alive so settlement_rx doesn't close immediately.
                // The _settlement_tx_hold prevents the channel from closing.
                // (settlement_tx is held by this scope until shutdown)
            }

            // Hold settlement_tx to keep channel open even when monitor is disabled
            let _settlement_tx_hold = settlement_tx;

            let ptrade_cancel = shutdown_token.child_token();
            tokio::spawn(paper_tracker.run(
                signal_rx,
                ptrade_snap_rx,
                settlement_rx,
                ptrade_cancel,
            ));

            // Spawn PricingEngine (receives from fan-out, outputs ImpliedProbability)
            let pricing_config = config.system.pricing.clone();
            let pricing_engine = PricingEngine::new(pricing_config);
            let pricing_cancel = shutdown_token.child_token();
            // Probability channel: PricingEngine -> CrossAssetEngine
            let (probability_tx, probability_rx) = mpsc::channel::<ImpliedProbability>(1024);
            tokio::spawn(pricing_engine.run(pricing_snap_rx, probability_tx, pricing_cancel));

            // ArbSignal output channel: CrossAssetEngine -> downstream consumer
            let (arb_signal_tx, arb_signal_rx) = mpsc::channel::<ArbSignal>(1024);

            // Spawn CrossAssetEngine (consumes probabilities + prediction market snapshots)
            let signal_config = config.system.signal_generation.clone();
            let signal_engine = CrossAssetEngine::new(signal_config)
                .with_replay_mode(is_replay)
                .with_basis_risk_cache(basis_risk_cache.clone())
                .with_liveness(pipeline_liveness.clone());
            let signal_cancel = shutdown_token.child_token();
            tokio::spawn(signal_engine.run(
                probability_rx,
                signal_pred_snap_rx,
                event_registry.clone(),
                signal_cancel,
                arb_signal_tx,
            ));

            // ArbSignal consumer: log and meter signals (execution is v2)
            let arb_cancel = shutdown_token.child_token();
            tokio::spawn(async move {
                let mut arb_signal_rx = arb_signal_rx;
                loop {
                    tokio::select! {
                        biased;
                        _ = arb_cancel.cancelled() => {
                            tracing::debug!("ArbSignal consumer shutting down");
                            break;
                        }
                        signal = arb_signal_rx.recv() => {
                            match signal {
                                Some(sig) => {
                                    tracing::info!(
                                        event_id = %sig.event_id,
                                        direction = ?sig.direction,
                                        net_edge = %sig.net_edge,
                                        confidence = sig.confidence,
                                        signal_id = %sig.signal_id,
                                        "ArbSignal received"
                                    );
                                    metrics::counter!("arb_signals_consumed_total",
                                        "direction" => format!("{:?}", sig.direction)
                                    ).increment(1);
                                }
                                None => {
                                    tracing::debug!("ArbSignal channel closed");
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            tracing::info!(
                near_expiry_cutoff_hours = config.system.pricing.near_expiry_cutoff_hours,
                iv_bounds = format!("[{}, {}]", config.system.pricing.solver.iv_min, config.system.pricing.solver.iv_max),
                replay_mode = is_replay,
                "spread engine, paper trade tracker, pricing engine, and signal engine started"
            );

            // Wait for shutdown signal
            shutdown_token.cancelled().await;

            tracing::info!("shutdown complete");
            // _log_guard drops here, flushing remaining logs
            Ok(())
        }
    }
}
