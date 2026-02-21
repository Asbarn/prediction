//! Integration tests verifying Phase 1 foundation components work together.
//!
//! These tests verify end-to-end contracts: type safety (Price/Probability/Notional
//! cannot be mixed), error severity classification, config loading from example
//! files, and config validation of invalid values.

use prediction::config;
use prediction::error::*;
use prediction::types::*;
use rust_decimal::Decimal;
use std::path::Path;

// --- Type safety tests ---

#[test]
fn price_displays_correctly() {
    let p = Price::new(Decimal::new(42000, 0));
    assert_eq!(p.to_string(), "42000");
}

#[test]
fn probability_valid_construction() {
    let p = Probability::new(Decimal::new(42, 2)).expect("test: 0.42 is valid probability");
    assert_eq!(p.into_inner(), Decimal::new(42, 2));
}

#[test]
fn probability_rejects_above_one() {
    let result = Probability::new(Decimal::new(150, 2)); // 1.50
    assert!(result.is_err(), "test: 1.50 should be rejected");
}

#[test]
fn probability_rejects_negative() {
    let result = Probability::new(Decimal::new(-1, 0)); // -1
    assert!(result.is_err(), "test: -1 should be rejected");
}

#[test]
fn notional_construction() {
    let n = Notional::new(Decimal::new(100, 0));
    assert_eq!(n.into_inner(), Decimal::new(100, 0));
}

#[test]
fn probability_complement_returns_one_minus_p() {
    let p = Probability::new(Decimal::new(42, 2)).expect("test: valid probability");
    let c = p.complement();
    assert_eq!(c.into_inner(), Decimal::new(58, 2));
}

#[test]
fn notional_times_probability_produces_correct_result() {
    let n = Notional::new(Decimal::new(100, 0));
    let p = Probability::new(Decimal::new(42, 2)).expect("test: valid probability");
    let result = n * p;
    // 100 * 0.42 = 42.00
    assert_eq!(result.into_inner(), Decimal::new(4200, 2));
}

#[test]
fn all_id_types_constructible() {
    let _event_id = EventId::new("BTC-100K-2025-06-30");
    let _instrument_id = InstrumentId::new("BTC-27JUN25-100000-C");
    let _trace_id = TraceId::new();
}

#[tokio::test]
async fn dual_timestamp_now_returns_valid_elapsed() {
    let ts = DualTimestamp::now();
    let elapsed = ts.elapsed();
    assert!(
        elapsed.as_secs() < 1,
        "test: freshly created DualTimestamp should have sub-second elapsed"
    );
}

#[test]
fn all_venue_variants_display() {
    let venues = [Venue::Deribit, Venue::Polymarket, Venue::Kalshi];
    let expected = ["deribit", "polymarket", "kalshi"];
    for (venue, exp) in venues.iter().zip(expected.iter()) {
        assert_eq!(venue.to_string(), *exp);
    }
}

// --- Error type tests ---

#[test]
fn auth_failure_severity_is_fatal() {
    let err = VenueError::AuthFailure {
        venue: Venue::Deribit,
        message: "invalid key".into(),
    };
    assert_eq!(err.severity(), ErrorSeverity::Fatal);
}

#[test]
fn rate_limited_severity_is_degraded() {
    let err = VenueError::RateLimited {
        venue: Venue::Polymarket,
        backoff_ms: 1000,
    };
    assert_eq!(err.severity(), ErrorSeverity::Degraded);
}

#[test]
fn connection_timeout_severity_is_transient() {
    let err = VenueError::ConnectionTimeout {
        venue: Venue::Kalshi,
    };
    assert_eq!(err.severity(), ErrorSeverity::Transient);
}

#[test]
fn error_display_contains_severity_bracket_prefix() {
    let fatal = VenueError::AuthFailure {
        venue: Venue::Deribit,
        message: "test".into(),
    };
    assert!(
        fatal.to_string().contains("[FATAL]"),
        "test: display should include [FATAL]"
    );

    let degraded = VenueError::RateLimited {
        venue: Venue::Polymarket,
        backoff_ms: 500,
    };
    assert!(
        degraded.to_string().contains("[DEGRADED]"),
        "test: display should include [DEGRADED]"
    );

    let transient = VenueError::ConnectionTimeout {
        venue: Venue::Kalshi,
    };
    assert!(
        transient.to_string().contains("[TRANSIENT]"),
        "test: display should include [TRANSIENT]"
    );
}

// --- Config loading tests ---

#[test]
fn config_loads_from_example_files() {
    let config =
        config::load_config(Path::new("config")).expect("test: example config should load");

    // System config fields are populated
    assert!(
        config.system.staleness.threshold_ms > 0,
        "test: threshold_ms should be positive"
    );
    assert!(
        config.system.signals.min_spread_bps > 0,
        "test: min_spread_bps should be positive"
    );
    assert!(
        config.system.signals.cooldown_ms > 0,
        "test: cooldown_ms should be positive"
    );

    // Events config has at least one event mapping
    assert!(
        !config.events.events.is_empty(),
        "test: events config should have at least one event"
    );

    // Venues config has all three venue sections
    assert!(
        !config.venues.deribit.ws_url.is_empty(),
        "test: deribit ws_url should be set"
    );
    assert!(
        !config.venues.polymarket.ws_url.is_empty(),
        "test: polymarket ws_url should be set"
    );
    assert!(
        !config.venues.kalshi.ws_url.is_empty(),
        "test: kalshi ws_url should be set"
    );
}

// --- Config validation test ---

#[test]
fn config_validation_rejects_zero_staleness_threshold() {
    let tmp_dir = std::env::temp_dir().join("prediction_integration_test_zero_threshold");
    std::fs::create_dir_all(&tmp_dir).expect("test: create temp dir");

    std::fs::write(
        tmp_dir.join("config.toml"),
        "[logging]\nlog_dir = \"logs\"\nstdout_level = \"info\"\nfile_level = \"debug\"\n\n\
         [staleness]\nthreshold_ms = 0\n\n\
         [signals]\nmin_spread_bps = 100\ncooldown_ms = 5000\n",
    )
    .expect("test: write config");
    std::fs::write(
        tmp_dir.join("events.toml"),
        "[[events]]\nid = \"test\"\nasset = \"BTC\"\nstrike = \"100000\"\n\
         direction = \"above\"\nexpiry = \"2025-06-30\"\n\n\
         [events.venues.deribit]\ninstrument = \"BTC-27JUN25-100000-C\"\n",
    )
    .expect("test: write events");
    std::fs::write(
        tmp_dir.join("venues.toml"),
        "[deribit]\nws_url = \"wss://deribit.com/ws\"\nrate_limit_per_second = 20\n\
         heartbeat_interval_ms = 10000\n\n\
         [polymarket]\nws_url = \"wss://poly.com/ws\"\nrest_url = \"https://poly.com\"\n\
         chain_id = 137\n\n\
         [kalshi]\nrest_url = \"https://kalshi.com\"\nws_url = \"wss://kalshi.com/ws\"\n",
    )
    .expect("test: write venues");

    let err = config::load_config(&tmp_dir).expect_err("test: zero threshold should fail");
    match &err {
        ConfigError::Validation { message, .. } => {
            assert!(
                message.contains("threshold_ms"),
                "test: error should mention threshold_ms: {message}"
            );
        }
        other => panic!("test: expected Validation error, got: {other}"),
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
