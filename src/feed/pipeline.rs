//! Pipeline assembly: wires data source -> processor -> recorder -> downstream.
//!
//! ```text
//! [DataSource]  --RawMessage-->  [DeribitProcessor]  --MarketSnapshot-->  [downstream]
//!    (live/mock/replay)               |
//!                              [RecordingService]  --RecordLine-->  [JSONL disk]
//! ```

use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::DeribitConfig;
use crate::feed::deribit::normalize::DeribitProcessor;
use crate::feed::deribit::supervisor::DeribitSupervisor;
use crate::feed::mock::replay::ReplayDataSource;
use crate::feed::mock::synthetic::SyntheticDataSource;
use crate::feed::recording::RecordingService;
use crate::feed::reliability::VenueRateLimiter;
use crate::feed::traits::{RawDataSource, RawMessage};
use crate::types::{MarketSnapshot, Venue};

/// Selects how the pipeline receives raw market data.
#[derive(Debug)]
pub enum DataMode {
    /// Connect to real Deribit WebSocket (requires internet).
    Live,
    /// Replay from a JSONL recording file at configurable speed.
    Replay { path: PathBuf, speed: f64 },
    /// Synthetic data generation (no connection or files needed).
    Mock,
}

/// Assemble and start the complete data pipeline.
///
/// Wires together:
/// 1. A data source (live, replay, or synthetic) producing `RawMessage`
/// 2. A `RecordingService` persisting raw frames to JSONL
/// 3. A `DeribitProcessor` normalizing raw frames into `MarketSnapshot`
///
/// Returns a receiver for downstream `MarketSnapshot` consumption.
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
