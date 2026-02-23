//! Pipeline assembly: wires data source -> processor -> recorder -> downstream.
//!
//! Two entry points:
//! - `run_pipeline()` -- single-venue Deribit pipeline (used by Mock/Replay modes)
//! - `run_multi_venue_pipeline()` -- multi-venue fan-in pipeline (used by Live mode)
//!
//! ```text
//! Live mode (multi-venue):
//!
//! [DeribitSupervisor]      --RawMessage-->  [DeribitProcessor]      --+
//! [PolymarketSupervisor]   --RawMessage-->  [PolymarketProcessor]  --+--> shared mpsc --> [downstream]
//! [KalshiSupervisor]       --RawMessage-->  [KalshiProcessor]      --+
//!
//! Mock/Replay mode (single-venue Deribit):
//!
//! [DataSource]  --RawMessage-->  [DeribitProcessor]  --MarketSnapshot-->  [downstream]
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::config::{Credentials, VenuesConfig, DeribitConfig};
use crate::events::registry::EventRegistry;
use crate::feed::deribit::normalize::DeribitProcessor;
use crate::feed::deribit::supervisor::DeribitSupervisor;
use crate::feed::health::VenueHealth;
use crate::feed::kalshi::auth::load_kalshi_private_key;
use crate::feed::kalshi::normalize::KalshiProcessor;
use crate::feed::kalshi::supervisor::KalshiSupervisor;
use crate::feed::mock::replay::ReplayDataSource;
use crate::feed::mock::synthetic::SyntheticDataSource;
use crate::feed::polymarket::normalize::PolymarketProcessor;
use crate::feed::polymarket::supervisor::PolymarketSupervisor;
use crate::feed::recording::RecordingService;
use crate::feed::reliability::VenueRateLimiter;
use crate::feed::traits::{RawDataSource, RawMessage};
use crate::types::{MarketSnapshot, Venue};

/// Pipeline output handles containing the snapshot receiver and per-venue health trackers.
///
/// Returned by `run_multi_venue_pipeline()` so that `main.rs` can pass the
/// `VenueHealth` references to the health endpoint.
pub struct PipelineHandles {
    /// Receiver for normalized market snapshots from all venues.
    pub snapshot_rx: mpsc::Receiver<MarketSnapshot>,
    /// Per-venue health trackers (populated in Live mode, empty in Mock/Replay).
    pub venue_health: Vec<Arc<VenueHealth>>,
}

/// Selects how the pipeline receives raw market data.
#[derive(Debug)]
pub enum DataMode {
    /// Connect to real venue WebSockets (requires internet).
    Live,
    /// Replay from a JSONL recording file at configurable speed.
    Replay { path: PathBuf, speed: f64 },
    /// Synthetic data generation (no connection or files needed).
    Mock,
}

/// Fan-in buffer size for the shared multi-venue channel.
const FAN_IN_BUFFER: usize = 1024;

/// Assemble and start the multi-venue data pipeline (primary entry point).
///
/// In Live mode, spawns independent pipelines for Deribit, Polymarket, and
/// Kalshi, each with its own CancellationToken for crash isolation (RELY-04).
/// All venues publish through a shared fan-in channel.
///
/// In Mock/Replay modes, delegates to single-venue Deribit behavior.
///
/// The `event_registry` parameter threads the shared registry through for
/// future snapshot annotation (Phase 6). Currently a pass-through.
///
/// Missing credentials for a venue (e.g., Kalshi) produce a warning and that
/// venue is skipped -- remaining venues continue operating.
pub async fn run_multi_venue_pipeline(
    mode: DataMode,
    config: &VenuesConfig,
    credentials: &Credentials,
    recording_dir: PathBuf,
    cancel: CancellationToken,
    _event_registry: Option<Arc<RwLock<EventRegistry>>>,
) -> anyhow::Result<PipelineHandles> {
    match mode {
        DataMode::Live => {
            run_live_multi_venue(config, credentials, recording_dir, cancel).await
        }
        DataMode::Replay { path, speed } => {
            let snapshot_rx = run_pipeline(
                DataMode::Replay { path, speed },
                &config.deribit,
                recording_dir,
                cancel,
            )
            .await?;
            Ok(PipelineHandles {
                snapshot_rx,
                venue_health: vec![],
            })
        }
        DataMode::Mock => {
            let snapshot_rx =
                run_pipeline(DataMode::Mock, &config.deribit, recording_dir, cancel).await?;
            Ok(PipelineHandles {
                snapshot_rx,
                venue_health: vec![],
            })
        }
    }
}

/// Start all configured venue pipelines with a shared fan-in channel.
async fn run_live_multi_venue(
    config: &VenuesConfig,
    credentials: &Credentials,
    recording_dir: PathBuf,
    cancel: CancellationToken,
) -> anyhow::Result<PipelineHandles> {
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<MarketSnapshot>(FAN_IN_BUFFER);
    let mut venue_health_handles: Vec<Arc<VenueHealth>> = Vec::new();

    // --- Deribit pipeline ---
    {
        let health = VenueHealth::new(Venue::Deribit);
        venue_health_handles.push(health.clone());
        let venue_cancel = cancel.child_token();
        let deribit_recording = RecordingService::start(
            recording_dir.join("deribit"),
            Venue::Deribit,
            venue_cancel.clone(),
        );

        let rate_limiter =
            VenueRateLimiter::new("deribit", config.deribit.rate_limit_per_second);
        tracing::info!(
            venue = "deribit",
            rate_limit = config.deribit.rate_limit_per_second,
            "Deribit rate limiter configured"
        );

        let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);
        let supervisor = DeribitSupervisor::new(
            config.deribit.clone(),
            config.deribit.instruments.clone(),
            venue_cancel.clone(),
            rate_limiter,
        );
        tokio::spawn(supervisor.run(supervisor_tx));

        let (processor, venue_snapshot_rx) = DeribitProcessor::new(
            supervisor_rx,
            Some(deribit_recording.sender()),
            venue_cancel.clone(),
            config.deribit.staleness_threshold_ms,
        );
        tokio::spawn(processor.run());

        // Forwarding task: pipe from processor's snapshot receiver to shared fan-in
        let fan_in_tx = snapshot_tx.clone();
        tokio::spawn(forward_snapshots(
            venue_snapshot_rx,
            fan_in_tx,
            Venue::Deribit,
            venue_cancel,
        ));

        tracing::info!(venue = "deribit", "Deribit pipeline started");
    }

    // --- Polymarket pipeline ---
    {
        let health = VenueHealth::new(Venue::Polymarket);
        venue_health_handles.push(health.clone());

        let venue_cancel = cancel.child_token();
        let poly_recording = RecordingService::start(
            recording_dir.join("polymarket"),
            Venue::Polymarket,
            venue_cancel.clone(),
        );

        let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);
        let supervisor = PolymarketSupervisor::new(
            config.polymarket.clone(),
            venue_cancel.clone(),
        );
        tokio::spawn(supervisor.run(supervisor_tx));

        let (processor, venue_snapshot_rx) = PolymarketProcessor::new(
            supervisor_rx,
            Some(poly_recording.sender()),
            venue_cancel.clone(),
            config.polymarket.staleness_threshold_ms,
        );
        tokio::spawn(processor.run());

        let fan_in_tx = snapshot_tx.clone();
        tokio::spawn(forward_snapshots(
            venue_snapshot_rx,
            fan_in_tx,
            Venue::Polymarket,
            venue_cancel,
        ));

        tracing::info!(venue = "polymarket", "Polymarket pipeline started");
    }

    // --- Kalshi pipeline ---
    {
        let health = VenueHealth::new(Venue::Kalshi);
        venue_health_handles.push(health.clone());

        let api_key_id = credentials.kalshi_api_key_id.clone();
        let private_key_pem = credentials
            .kalshi_private_key
            .clone()
            .or_else(|| load_kalshi_key_from_file(config));

        match (api_key_id, private_key_pem) {
            (Some(key_id), Some(pem)) => {
                match load_kalshi_private_key(&pem) {
                    Ok(private_key) => {
                        let venue_cancel = cancel.child_token();
                        let kalshi_recording = RecordingService::start(
                            recording_dir.join("kalshi"),
                            Venue::Kalshi,
                            venue_cancel.clone(),
                        );

                        let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);
                        let supervisor = KalshiSupervisor::new(
                            config.kalshi.clone(),
                            key_id,
                            private_key,
                            venue_cancel.clone(),
                        );
                        tokio::spawn(supervisor.run(supervisor_tx));

                        let (processor, venue_snapshot_rx) = KalshiProcessor::new(
                            supervisor_rx,
                            Some(kalshi_recording.sender()),
                            venue_cancel.clone(),
                            config.kalshi.staleness_threshold_ms,
                        );
                        tokio::spawn(processor.run());

                        let fan_in_tx = snapshot_tx.clone();
                        tokio::spawn(forward_snapshots(
                            venue_snapshot_rx,
                            fan_in_tx,
                            Venue::Kalshi,
                            venue_cancel,
                        ));

                        tracing::info!(venue = "kalshi", "Kalshi pipeline started");
                    }
                    Err(e) => {
                        tracing::warn!(
                            venue = "kalshi",
                            error = %e,
                            "Kalshi RSA private key invalid, skipping Kalshi feed"
                        );
                    }
                }
            }
            _ => {
                tracing::warn!(
                    venue = "kalshi",
                    has_api_key = credentials.kalshi_api_key_id.is_some(),
                    has_private_key = credentials.kalshi_private_key.is_some(),
                    "Kalshi credentials not configured, skipping Kalshi feed \
                     (set KALSHI_API_KEY_ID and KALSHI_PRIVATE_KEY env vars)"
                );
            }
        }
    }

    // Drop the original sender so the channel closes when all venue tasks complete
    drop(snapshot_tx);

    tracing::info!("multi-venue pipeline started");
    Ok(PipelineHandles {
        snapshot_rx,
        venue_health: venue_health_handles,
    })
}

/// Try to load Kalshi private key from a file path specified in config.
fn load_kalshi_key_from_file(config: &VenuesConfig) -> Option<String> {
    config
        .kalshi
        .private_key_path
        .as_ref()
        .and_then(|path| match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(e) => {
                tracing::warn!(
                    path = %path,
                    error = %e,
                    "failed to read Kalshi private key file"
                );
                None
            }
        })
}

/// Forward snapshots from a per-venue receiver to the shared fan-in sender.
///
/// Exits when either the venue's receiver closes (venue processor stopped)
/// or the venue's CancellationToken is cancelled. The shared sender is dropped
/// on exit, which is critical for the fan-in channel to eventually close.
pub async fn forward_snapshots(
    mut venue_rx: mpsc::Receiver<MarketSnapshot>,
    fan_in_tx: mpsc::Sender<MarketSnapshot>,
    venue: Venue,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::info!(venue = %venue, "snapshot forwarder cancelled");
                break;
            }

            snapshot = venue_rx.recv() => {
                match snapshot {
                    Some(snap) => {
                        if fan_in_tx.send(snap).await.is_err() {
                            tracing::warn!(
                                venue = %venue,
                                "fan-in receiver dropped, stopping forwarder"
                            );
                            break;
                        }
                    }
                    None => {
                        tracing::info!(
                            venue = %venue,
                            "venue snapshot channel closed, forwarder exiting"
                        );
                        break;
                    }
                }
            }
        }
    }
}

/// Assemble and start a single-venue Deribit pipeline.
///
/// Used by Mock and Replay modes. Kept for backward compatibility.
pub async fn run_pipeline(
    mode: DataMode,
    config: &DeribitConfig,
    recording_dir: PathBuf,
    cancel: CancellationToken,
) -> anyhow::Result<mpsc::Receiver<MarketSnapshot>> {
    tracing::info!(mode = ?mode, "starting pipeline");

    // 1. Start the recording service
    let recording_svc = RecordingService::start(
        recording_dir,
        Venue::Deribit,
        cancel.clone(),
    );

    // 2. Start the data source based on mode
    let raw_rx: mpsc::Receiver<RawMessage> = match mode {
        DataMode::Live => {
            let rate_limiter = VenueRateLimiter::new("deribit", config.rate_limit_per_second);
            tracing::info!(
                venue = "deribit",
                rate_limit = config.rate_limit_per_second,
                "rate limiter configured"
            );
            let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);
            let supervisor = DeribitSupervisor::new(
                config.clone(),
                config.instruments.clone(),
                cancel.clone(),
                rate_limiter,
            );
            tokio::spawn(supervisor.run(supervisor_tx));
            supervisor_rx
        }
        DataMode::Replay { path, speed } => {
            let source = ReplayDataSource::new(path, speed, cancel.clone());
            source.start().await?
        }
        DataMode::Mock => {
            let source = SyntheticDataSource::new(
                config.instruments.clone(),
                cancel.clone(),
            )
            .with_interval(100);
            source.start().await?
        }
    };

    // 3. Create processor with recording sender
    let (processor, snapshot_rx) = DeribitProcessor::new(
        raw_rx,
        Some(recording_svc.sender()),
        cancel.clone(),
        config.staleness_threshold_ms,
    );

    // 4. Spawn processor task
    tokio::spawn(processor.run());

    tracing::info!("pipeline started");

    Ok(snapshot_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_mode_debug() {
        let live = DataMode::Live;
        let mock = DataMode::Mock;
        let replay = DataMode::Replay {
            path: PathBuf::from("test.jsonl"),
            speed: 2.0,
        };
        // Just ensure Debug works without panic
        let _ = format!("{:?} {:?} {:?}", live, mock, replay);
    }
}
