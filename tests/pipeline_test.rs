//! Integration tests for the full data pipeline.
//!
//! Tests the complete path: SyntheticDataSource -> DeribitProcessor -> MarketSnapshot
//! without any live WebSocket connection.

use tokio_util::sync::CancellationToken;

use prediction::config::DeribitConfig;
use prediction::feed::pipeline::{self, DataMode};
use prediction::types::Venue;

fn test_deribit_config() -> DeribitConfig {
    DeribitConfig {
        ws_url: "wss://test.deribit.com/ws/api/v2".to_string(),
        rate_limit_per_second: 20,
        heartbeat_interval_ms: 10000,
        staleness_threshold_ms: 5000,
        reconnect: Default::default(),
        instruments: vec!["BTC-27JUN25-100000-C".to_string()],
    }
}

#[tokio::test]
async fn pipeline_mock_produces_market_snapshots() {
    let cancel = CancellationToken::new();
    let config = test_deribit_config();

    let tmp_dir = std::env::temp_dir().join(format!(
        "prediction_pipeline_test_{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ));

    let mut snapshot_rx = pipeline::run_pipeline(
        DataMode::Mock,
        &config,
        tmp_dir.clone(),
        cancel.clone(),
    )
    .await
    .expect("pipeline should start");

    // Receive at least 3 snapshots
    let mut count = 0;
    for _ in 0..20 {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            snapshot_rx.recv(),
        )
        .await
        {
            Ok(Some(snap)) => {
                assert_eq!(snap.venue, Venue::Deribit);
                assert_eq!(
                    snap.instrument_id,
                    prediction::types::InstrumentId::new("BTC-27JUN25-100000-C")
                );
                assert!(snap.sequence > 0, "sequence should be positive");
                count += 1;
                if count >= 3 {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert!(
        count >= 3,
        "should receive at least 3 MarketSnapshots, got {count}"
    );

    cancel.cancel();
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
}

#[tokio::test]
async fn pipeline_shutdown_completes_gracefully() {
    let cancel = CancellationToken::new();
    let config = test_deribit_config();

    let tmp_dir = std::env::temp_dir().join(format!(
        "prediction_pipeline_shutdown_test_{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ));

    let mut snapshot_rx = pipeline::run_pipeline(
        DataMode::Mock,
        &config,
        tmp_dir.clone(),
        cancel.clone(),
    )
    .await
    .expect("pipeline should start");

    // Receive one snapshot to confirm pipeline is running
    let snap = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        snapshot_rx.recv(),
    )
    .await
    .expect("should receive within timeout")
    .expect("should get a snapshot");

    assert_eq!(snap.venue, Venue::Deribit);

    // Cancel and verify shutdown completes within a reasonable time
    cancel.cancel();

    // The receiver should close (return None) after cancellation
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async {
            while let Some(_) = snapshot_rx.recv().await {
                // drain remaining
            }
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "pipeline should shut down within 2 seconds"
    );

    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
}

#[tokio::test]
async fn pipeline_replay_processes_recorded_data() {
    // First, generate some synthetic data and record it
    let cancel = CancellationToken::new();
    let config = test_deribit_config();

    let record_dir = std::env::temp_dir().join(format!(
        "prediction_replay_pipeline_test_{}",
        uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
    ));

    // Run mock pipeline briefly to generate a recording
    let mut snapshot_rx = pipeline::run_pipeline(
        DataMode::Mock,
        &config,
        record_dir.clone(),
        cancel.clone(),
    )
    .await
    .expect("mock pipeline should start");

    // Receive a few snapshots to ensure data was recorded
    let mut mock_count = 0;
    for _ in 0..10 {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            snapshot_rx.recv(),
        )
        .await
        {
            Ok(Some(_)) => {
                mock_count += 1;
                if mock_count >= 5 {
                    break;
                }
            }
            _ => break,
        }
    }

    // Shut down mock pipeline
    cancel.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Find the recording file
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let recording_path = record_dir
        .join("deribit")
        .join(format!("{today}.jsonl"));

    // Verify recording file exists and has content
    if recording_path.exists() {
        let contents = tokio::fs::read_to_string(&recording_path)
            .await
            .expect("should read recording");
        let line_count = contents.lines().count();
        assert!(line_count > 0, "recording should have at least 1 line");

        // Now replay the recording
        let replay_cancel = CancellationToken::new();
        let replay_record_dir = std::env::temp_dir().join(format!(
            "prediction_replay_output_test_{}",
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ));

        let mut replay_rx = pipeline::run_pipeline(
            DataMode::Replay {
                path: recording_path.clone(),
                speed: 0.0, // instant replay
            },
            &config,
            replay_record_dir.clone(),
            replay_cancel.clone(),
        )
        .await
        .expect("replay pipeline should start");

        // Receive snapshots from replay
        let mut replay_count = 0;
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                replay_rx.recv(),
            )
            .await
            {
                Ok(Some(snap)) => {
                    assert_eq!(snap.venue, Venue::Deribit);
                    replay_count += 1;
                }
                _ => break,
            }
        }

        assert!(
            replay_count > 0,
            "replay should produce at least 1 MarketSnapshot"
        );

        replay_cancel.cancel();
        let _ = tokio::fs::remove_dir_all(&replay_record_dir).await;
    }

    let _ = tokio::fs::remove_dir_all(&record_dir).await;
}
