use prediction::config;
use prediction::error::*;
use prediction::types::*;
use rust_decimal::Decimal;
use std::path::Path;

#[test]
fn types_are_importable() {
    let _venue = Venue::Deribit;
    let _price = Price::new(Decimal::new(100, 0));
    let _prob = Probability::new(Decimal::new(5, 1)).expect("valid probability");
    let _notional = Notional::new(Decimal::new(1000, 0));
    let _event_id = EventId::new("BTC-100K-2025-06-30");
    let _instrument_id = InstrumentId::new("BTC-27JUN25-100000-C");
    let _trace_id = TraceId::new();
    let _ts = DualTimestamp::now();
}

#[test]
fn probability_rejects_invalid() {
    assert!(Probability::new(Decimal::new(15, 1)).is_err()); // 1.5 > 1
    assert!(Probability::new(Decimal::new(-1, 0)).is_err()); // -1 < 0
    assert!(Probability::new(Decimal::ZERO).is_ok());
    assert!(Probability::new(Decimal::ONE).is_ok());
}

#[test]
fn probability_complement() {
    let p = Probability::new(Decimal::new(3, 1)).expect("valid");
    let c = p.complement();
    assert_eq!(c.into_inner(), Decimal::new(7, 1));
}

#[test]
fn notional_times_probability() {
    let n = Notional::new(Decimal::new(100, 0));
    let p = Probability::new(Decimal::new(5, 1)).expect("valid");
    let result = n * p;
    assert_eq!(result.into_inner(), Decimal::new(50, 0));
}

#[test]
fn notional_times_price() {
    let n = Notional::new(Decimal::new(10, 0));
    let p = Price::new(Decimal::new(250, 0));
    let result = n * p;
    assert_eq!(result.into_inner(), Decimal::new(2500, 0));
}

#[test]
fn venue_display() {
    assert_eq!(Venue::Deribit.to_string(), "deribit");
    assert_eq!(Venue::Polymarket.to_string(), "polymarket");
    assert_eq!(Venue::Kalshi.to_string(), "kalshi");
}

#[test]
fn venue_env_prefix() {
    assert_eq!(Venue::Deribit.env_prefix(), "DERIBIT");
    assert_eq!(Venue::Polymarket.env_prefix(), "POLYMARKET");
    assert_eq!(Venue::Kalshi.env_prefix(), "KALSHI");
}

#[test]
fn error_severity_classification() {
    let err = VenueError::AuthFailure {
        venue: Venue::Deribit,
        message: "invalid key".into(),
    };
    assert_eq!(err.severity(), ErrorSeverity::Fatal);

    let err = VenueError::RateLimited {
        venue: Venue::Polymarket,
        backoff_ms: 1000,
    };
    assert_eq!(err.severity(), ErrorSeverity::Degraded);

    let err = VenueError::ConnectionTimeout {
        venue: Venue::Kalshi,
    };
    assert_eq!(err.severity(), ErrorSeverity::Transient);

    let err = VenueError::ParseError {
        venue: Venue::Deribit,
        message: "bad json".into(),
    };
    assert_eq!(err.severity(), ErrorSeverity::Transient);

    let err = VenueError::ConnectionClosed {
        venue: Venue::Kalshi,
        reason: "server restart".into(),
    };
    assert_eq!(err.severity(), ErrorSeverity::Transient);
}

#[test]
fn error_display_includes_severity_prefix() {
    let err = VenueError::AuthFailure {
        venue: Venue::Deribit,
        message: "test".into(),
    };
    assert!(err.to_string().starts_with("[FATAL]"));

    let err = VenueError::RateLimited {
        venue: Venue::Polymarket,
        backoff_ms: 500,
    };
    assert!(err.to_string().starts_with("[DEGRADED]"));

    let err = VenueError::ConnectionTimeout {
        venue: Venue::Kalshi,
    };
    assert!(err.to_string().starts_with("[TRANSIENT]"));
}

#[test]
fn config_error_variants_exist() {
    let _err = ConfigError::Validation {
        file: "test.toml".into(),
        message: "invalid field".into(),
    };
    let _err = ConfigError::MissingEnvVar {
        var: "DERIBIT_API_KEY".into(),
    };
}

#[test]
fn market_snapshot_constructs() {
    let snap = MarketSnapshot {
        venue: Venue::Deribit,
        instrument_id: InstrumentId::new("BTC-27JUN25-100000-C"),
        event_id: Some(EventId::new("BTC-100K-2025-06-30")),
        bid: Some(Price::new(Decimal::new(100, 0))),
        ask: Some(Price::new(Decimal::new(105, 0))),
        bid_size: Some(Notional::new(Decimal::new(10, 0))),
        ask_size: Some(Notional::new(Decimal::new(5, 0))),
        depth_bids: vec![],
        depth_asks: vec![],
        bid_probability: None,
        ask_probability: None,
        last_price: None,
        mark_price: None,
        index_price: None,
        mark_iv: None,
        open_interest: None,
        volume_24h: None,
        greeks: None,
        bid_iv: None,
        ask_iv: None,
        underlying_price: None,
        underlying_index: None,
        exchange_timestamp: None,
        timestamp: DualTimestamp::now(),
        sequence: 42,
        trace_id: TraceId::new(),
        is_stale: false,
    };
    assert_eq!(snap.venue, Venue::Deribit);
    assert_eq!(snap.sequence, 42);
}

#[test]
fn trace_id_display() {
    let id = TraceId::new();
    let s = id.to_string();
    // UUID v7 format: 8-4-4-4-12 hex
    assert_eq!(s.len(), 36);
    assert_eq!(&s[8..9], "-");
}

#[test]
fn dual_timestamp_elapsed() {
    let ts = DualTimestamp::now();
    // Just verify it doesn't panic and returns a reasonable duration
    let elapsed = ts.elapsed();
    assert!(elapsed.as_secs() < 1);
}

#[test]
fn dual_timestamp_serializes_wall_only() {
    let ts = DualTimestamp::now();
    let json = serde_json::to_string(&ts).expect("serialize DualTimestamp");
    // Should be a quoted datetime string, not an object with mono field
    assert!(json.starts_with('"'));
    assert!(!json.contains("mono"));
}

#[test]
fn price_serializes_as_string() {
    let p = Price::new(Decimal::new(12345, 2)); // 123.45
    let json = serde_json::to_string(&p).expect("serialize Price");
    assert_eq!(json, r#""123.45""#);
}

#[test]
fn venue_serde_roundtrip() {
    let v = Venue::Deribit;
    let json = serde_json::to_string(&v).expect("serialize Venue");
    assert_eq!(json, r#""deribit""#);
    let parsed: Venue = serde_json::from_str(&json).expect("deserialize Venue");
    assert_eq!(parsed, Venue::Deribit);
}

// --- Config loading tests ---

#[test]
fn config_loads_from_example_dir() {
    let config = config::load_config(Path::new("config")).expect("load example config");
    assert_eq!(config.system.logging.log_dir, "logs");
    assert_eq!(config.system.staleness.threshold_ms, 5000);
    assert_eq!(config.system.signals.min_spread_bps, 100);
    // events may be empty when discovery hasn't populated yet
    assert_eq!(config.venues.deribit.ws_url, "wss://www.deribit.com/ws/api/v2");
    assert_eq!(config.venues.polymarket.chain_id, 137);
}

#[test]
fn config_read_error_on_missing_dir() {
    let err = config::load_config(Path::new("nonexistent")).unwrap_err();
    match err {
        ConfigError::ReadFile { file, .. } => assert_eq!(file, "config.toml"),
        other => panic!("expected ReadFile, got: {other}"),
    }
}

#[test]
fn config_parse_error_includes_line_column() {
    // Write a temporary invalid TOML file and try to parse it
    let tmp_dir = std::env::temp_dir().join("prediction_test_invalid_toml");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    // Write invalid config.toml (missing closing quote)
    std::fs::write(
        tmp_dir.join("config.toml"),
        "[logging]\nlog_dir = \"logs\nstdout_level = \"info\"\n",
    )
    .expect("write invalid toml");
    std::fs::write(tmp_dir.join("events.toml"), "events = []\n").expect("write events");
    std::fs::write(tmp_dir.join("venues.toml"), "").expect("write venues");

    let err = config::load_config(&tmp_dir).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("config.toml"),
        "error should reference the file: {msg}"
    );
    // toml 0.8 errors include line/column info
    assert!(
        msg.contains("line") || msg.contains("TOML parse error"),
        "error should include parse details: {msg}"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn config_validation_rejects_zero_threshold() {
    let tmp_dir = std::env::temp_dir().join("prediction_test_zero_threshold");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    std::fs::write(
        tmp_dir.join("config.toml"),
        "[logging]\nlog_dir = \"logs\"\nstdout_level = \"info\"\nfile_level = \"debug\"\n\n\
         [staleness]\nthreshold_ms = 0\n\n\
         [signals]\nmin_spread_bps = 100\ncooldown_ms = 5000\n",
    )
    .expect("write config");
    std::fs::write(
        tmp_dir.join("events.toml"),
        "[[events]]\nid = \"test\"\nasset = \"BTC\"\nstrike = \"100000\"\n\
         direction = \"above\"\nexpiry = \"2025-06-30\"\n\n\
         [events.venues.deribit]\ninstrument = \"BTC-27JUN25-100000-C\"\n",
    )
    .expect("write events");
    std::fs::write(
        tmp_dir.join("venues.toml"),
        "[deribit]\nws_url = \"wss://deribit.com/ws\"\nrate_limit_per_second = 20\n\
         heartbeat_interval_ms = 10000\n\n\
         [polymarket]\nws_url = \"wss://poly.com/ws\"\nrest_url = \"https://poly.com\"\n\
         chain_id = 137\n\n\
         [kalshi]\nrest_url = \"https://kalshi.com\"\nws_url = \"wss://kalshi.com/ws\"\n",
    )
    .expect("write venues");

    let err = config::load_config(&tmp_dir).unwrap_err();
    match &err {
        ConfigError::Validation { message, .. } => {
            assert!(
                message.contains("threshold_ms"),
                "should mention threshold_ms: {message}"
            );
        }
        other => panic!("expected Validation error, got: {other}"),
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn config_validation_rejects_event_without_venues() {
    let tmp_dir = std::env::temp_dir().join("prediction_test_no_venues");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    std::fs::write(
        tmp_dir.join("config.toml"),
        "[logging]\nlog_dir = \"logs\"\nstdout_level = \"info\"\nfile_level = \"debug\"\n\n\
         [staleness]\nthreshold_ms = 5000\n\n\
         [signals]\nmin_spread_bps = 100\ncooldown_ms = 5000\n",
    )
    .expect("write config");
    // Event with NO venue mappings
    std::fs::write(
        tmp_dir.join("events.toml"),
        "[[events]]\nid = \"test\"\nasset = \"BTC\"\nstrike = \"100000\"\n\
         direction = \"above\"\nexpiry = \"2025-06-30\"\n\n\
         [events.venues]\n",
    )
    .expect("write events");
    std::fs::write(
        tmp_dir.join("venues.toml"),
        "[deribit]\nws_url = \"wss://deribit.com/ws\"\nrate_limit_per_second = 20\n\
         heartbeat_interval_ms = 10000\n\n\
         [polymarket]\nws_url = \"wss://poly.com/ws\"\nrest_url = \"https://poly.com\"\n\
         chain_id = 137\n\n\
         [kalshi]\nrest_url = \"https://kalshi.com\"\nws_url = \"wss://kalshi.com/ws\"\n",
    )
    .expect("write venues");

    let err = config::load_config(&tmp_dir).unwrap_err();
    match &err {
        ConfigError::Validation { message, .. } => {
            assert!(
                message.contains("no venue mappings"),
                "should mention missing venues: {message}"
            );
        }
        other => panic!("expected Validation error, got: {other}"),
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn credentials_debug_redacts_secrets() {
    let creds = config::Credentials {
        deribit_api_key: Some("secret_key".to_string()),
        deribit_api_secret: None,
        polymarket_private_key: Some("0xprivate".to_string()),
        kalshi_api_key_id: None,
        kalshi_private_key: None,
    };
    let debug = format!("{creds:?}");
    assert!(!debug.contains("secret_key"), "should not expose key: {debug}");
    assert!(!debug.contains("0xprivate"), "should not expose key: {debug}");
    assert!(debug.contains("***"), "should show redacted marker: {debug}");
    assert!(debug.contains("None"), "should show None for absent: {debug}");
}
