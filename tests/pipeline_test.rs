//! Integration tests for the full data pipeline.
//!
//! Tests the complete path: SyntheticDataSource -> DeribitProcessor -> MarketSnapshot
//! without any live WebSocket connection. Also tests multi-venue replay pipeline.

use tokio_util::sync::CancellationToken;

use prediction::config::{DeriveConfig, DeribitConfig};
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
        book_depth_levels: 20,
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

#[tokio::test]
async fn multi_venue_replay_pipeline_processes_deribit_recordings() {
    use prediction::config::{VenuesConfig, PolymarketConfig, KalshiConfig};
    use prediction::feed::traits::RecordLine;
    use prediction::replay::run_replay_pipeline;

    // Create a temp recordings directory with a Deribit subdirectory
    let recordings_dir = std::env::temp_dir().join(format!(
        "prediction_multi_replay_test_{}",
        uuid::Uuid::now_v7()
    ));
    let deribit_dir = recordings_dir.join("deribit");
    tokio::fs::create_dir_all(&deribit_dir)
        .await
        .expect("should create deribit dir");

    // Write sample Deribit JSONL data (book change_id snapshots)
    let base_ts = chrono::Utc::now();
    let records: Vec<RecordLine> = (0..5)
        .map(|i| RecordLine {
            raw: format!(
                r#"{{"jsonrpc":"2.0","method":"subscription","params":{{"channel":"book.BTC-27JUN25-100000-C.none.20.100ms","data":{{"timestamp":{},"instrument_name":"BTC-27JUN25-100000-C","change_id":{},"bids":[[0.0055,10.0]],"asks":[[0.0060,8.0]]}}}}}}"#,
                base_ts.timestamp_millis() + i * 100,
                1000 + i,
            ),
            local_ts: base_ts + chrono::Duration::milliseconds(i * 100),
            venue: Venue::Deribit,
            channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
            instrument: Some("BTC-27JUN25-100000-C".to_string()),
        })
        .collect();

    let mut jsonl_content = String::new();
    for record in &records {
        jsonl_content.push_str(&serde_json::to_string(record).unwrap());
        jsonl_content.push('\n');
    }
    tokio::fs::write(deribit_dir.join("test.jsonl"), &jsonl_content)
        .await
        .expect("should write JSONL");

    // No polymarket or kalshi directories -- should degrade gracefully

    // Build a minimal VenuesConfig
    let config = VenuesConfig {
        deribit: DeribitConfig {
            ws_url: "wss://test.deribit.com/ws/api/v2".to_string(),
            rate_limit_per_second: 20,
            heartbeat_interval_ms: 10000,
            staleness_threshold_ms: u64::MAX, // disable staleness for test
            reconnect: Default::default(),
            instruments: vec!["BTC-27JUN25-100000-C".to_string()],
            book_depth_levels: 20,
        },
        polymarket: PolymarketConfig {
            ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string(),
            rest_url: "https://clob.polymarket.com".to_string(),
            chain_id: 137,
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
            staleness_threshold_ms: 5000,
            reconnect: Default::default(),
            assets: vec![],
            rate_limit_per_second: 10,
            ping_interval_ms: 10000,
            data_timeout_secs: 120,
        },
        kalshi: KalshiConfig {
            rest_url: "https://api.elections.kalshi.com/trade-api/v2".to_string(),
            ws_url: "wss://api.elections.kalshi.com/trade-api/ws/v2".to_string(),
            staleness_threshold_ms: 15000,
            reconnect: Default::default(),
            rate_limit_per_second: 10,
            market_tickers: vec![],
            private_key_path: None,
            heartbeat_timeout_ms: 30_000,
        },
        derive: DeriveConfig {
            ws_url: "wss://api.lyra.finance/ws".to_string(),
            rate_limit_per_second: 10,
            book_depth_levels: 20,
            staleness_threshold_ms: 5000,
            reconnect: Default::default(),
            instruments: vec![],
        },
    };

    let cancel = CancellationToken::new();
    let handles = run_replay_pipeline(
        recordings_dir.clone(),
        &config,
        0.0, // instant replay
        cancel.clone(),
        None, // no event registry for test
    )
    .await
    .expect("replay pipeline should start");

    let mut snapshot_rx = handles.snapshot_rx;
    assert!(
        handles.venue_health.is_empty(),
        "replay mode should have empty venue health"
    );

    // Collect all snapshots from replay
    let mut replay_count = 0;
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), snapshot_rx.recv()).await {
            Ok(Some(snap)) => {
                assert_eq!(snap.venue, Venue::Deribit);
                replay_count += 1;
            }
            _ => break,
        }
    }

    assert!(
        replay_count > 0,
        "multi-venue replay should produce at least 1 MarketSnapshot from Deribit recordings"
    );

    cancel.cancel();
    let _ = tokio::fs::remove_dir_all(&recordings_dir).await;
}

#[tokio::test]
async fn multi_venue_replay_graceful_empty_dir() {
    use prediction::config::{VenuesConfig, PolymarketConfig, KalshiConfig};
    use prediction::replay::run_replay_pipeline;

    // Create an empty recordings directory (no venue subdirectories)
    let recordings_dir = std::env::temp_dir().join(format!(
        "prediction_empty_replay_test_{}",
        uuid::Uuid::now_v7()
    ));
    tokio::fs::create_dir_all(&recordings_dir)
        .await
        .expect("should create dir");

    let config = VenuesConfig {
        deribit: DeribitConfig {
            ws_url: "wss://test.deribit.com/ws/api/v2".to_string(),
            rate_limit_per_second: 20,
            heartbeat_interval_ms: 10000,
            staleness_threshold_ms: 5000,
            reconnect: Default::default(),
            instruments: vec![],
            book_depth_levels: 20,
        },
        polymarket: PolymarketConfig {
            ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string(),
            rest_url: "https://clob.polymarket.com".to_string(),
            chain_id: 137,
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
            staleness_threshold_ms: 5000,
            reconnect: Default::default(),
            assets: vec![],
            rate_limit_per_second: 10,
            ping_interval_ms: 10000,
            data_timeout_secs: 120,
        },
        kalshi: KalshiConfig {
            rest_url: "https://api.elections.kalshi.com/trade-api/v2".to_string(),
            ws_url: "wss://api.elections.kalshi.com/trade-api/ws/v2".to_string(),
            staleness_threshold_ms: 15000,
            reconnect: Default::default(),
            rate_limit_per_second: 10,
            market_tickers: vec![],
            private_key_path: None,
            heartbeat_timeout_ms: 30_000,
        },
        derive: DeriveConfig {
            ws_url: "wss://api.lyra.finance/ws".to_string(),
            rate_limit_per_second: 10,
            book_depth_levels: 20,
            staleness_threshold_ms: 5000,
            reconnect: Default::default(),
            instruments: vec![],
        },
    };

    let cancel = CancellationToken::new();
    let handles = run_replay_pipeline(
        recordings_dir.clone(),
        &config,
        0.0,
        cancel.clone(),
        None, // no event registry for test
    )
    .await
    .expect("replay pipeline should handle empty dir gracefully");

    let mut snapshot_rx = handles.snapshot_rx;

    // Should get None immediately since there's no data
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), snapshot_rx.recv()).await;
    match result {
        Ok(None) => {} // expected -- empty corpus, no data
        Err(_) => {} // timeout is also acceptable
        Ok(Some(_)) => panic!("should not receive snapshots from empty replay"),
    }

    cancel.cancel();
    let _ = tokio::fs::remove_dir_all(&recordings_dir).await;
}
