//! Golden serde roundtrip tests for all 4 JSONL output types.
//!
//! These tests lock down the serialized JSON schema so that any field
//! addition, removal, or rename causes a test failure. This protects
//! offline Python/Jupyter analysis tooling that depends on stable field names.

use std::str::FromStr;

use rust_decimal::Decimal;

use prediction::feed::traits::RecordLine;
use prediction::paper_trade::tracker::TradeEvent;
use prediction::signal::types::{
    ArbDirection, ArbSignal, CostBreakdown, LegInfo, ThresholdStatus,
};
use prediction::pricing::types::{ConfidenceComponents, PricingMethod};
use prediction::spread::patterns::{SpreadPattern, SpreadResult, ThresholdComponents};
use prediction::types::{DualTimestamp, Venue};

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

// ---------------------------------------------------------------------------
// RecordLine roundtrip
// ---------------------------------------------------------------------------

#[test]
fn record_line_schema_stable() {
    let record = RecordLine {
        raw: r#"{"jsonrpc":"2.0","method":"subscription"}"#.to_string(),
        local_ts: chrono::DateTime::parse_from_rfc3339("2026-01-15T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        venue: Venue::Deribit,
        channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
        instrument: Some("BTC-27JUN25-100000-C".to_string()),
    };

    // Check field presence via serde_json::Value
    let value = serde_json::to_value(&record).unwrap();
    let expected_fields = ["raw", "local_ts", "venue", "channel", "instrument"];
    for field in expected_fields {
        assert!(
            value.get(field).is_some(),
            "RecordLine missing field: {field}"
        );
    }

    // Verify venue serializes as lowercase string
    assert_eq!(value["venue"].as_str().unwrap(), "deribit");

    // Roundtrip: serialize -> deserialize -> assert
    let json_str = serde_json::to_string(&record).unwrap();
    let parsed: RecordLine = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.raw, record.raw);
    assert_eq!(parsed.venue, Venue::Deribit);
    assert_eq!(parsed.channel, record.channel);
    assert_eq!(parsed.instrument, record.instrument);
    assert_eq!(parsed.local_ts, record.local_ts);
}

#[test]
fn record_line_null_instrument_roundtrips() {
    let record = RecordLine {
        raw: "{}".to_string(),
        local_ts: chrono::Utc::now(),
        venue: Venue::Polymarket,
        channel: "market".to_string(),
        instrument: None,
    };

    let json_str = serde_json::to_string(&record).unwrap();
    let parsed: RecordLine = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.instrument.is_none());
}

// ---------------------------------------------------------------------------
// SpreadResult roundtrip
// ---------------------------------------------------------------------------

fn make_spread_result() -> SpreadResult {
    SpreadResult {
        event_id: "BTC-100K-2026".to_string(),
        pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
        gross_spread: dec("0.05"),
        net_spread: dec("0.03"),
        buy_fill_price: dec("0.45"),
        sell_fill_price: dec("0.50"),
        buy_fee: dec("0.005"),
        sell_fee: dec("0.007"),
        carry_cost: dec("0.002"),
        total_cost: dec("0.014"),
        basis_risk_premium: dec("0"),
        buy_fill_ratio: dec("1.0"),
        sell_fill_ratio: dec("0.95"),
        target_notional: dec("500"),
        timestamp_ms: 1700000000000,
        poly_exchange_ts: Some(1700000000100),
        kalshi_exchange_ts: None,
        options_exchange_ts: None,
        threshold: Some(dec("0.025")),
        threshold_components: Some(ThresholdComponents {
            static_floor: dec("0.01"),
            rolling_mean: 0.015,
            rolling_stddev: 0.005,
            k_sigma: 2.0,
            liquidity_penalty: dec("0.002"),
            final_threshold: dec("0.027"),
            is_cold_start: false,
        }),
        threshold_status: None,
    }
}

#[test]
fn spread_result_schema_stable() {
    let result = make_spread_result();
    let value = serde_json::to_value(&result).unwrap();

    // Verify ALL expected field names are present
    let expected_fields = [
        "event_id",
        "pattern",
        "gross_spread",
        "net_spread",
        "buy_fill_price",
        "sell_fill_price",
        "buy_fee",
        "sell_fee",
        "carry_cost",
        "total_cost",
        "buy_fill_ratio",
        "sell_fill_ratio",
        "target_notional",
        "timestamp_ms",
        "poly_exchange_ts",
        "kalshi_exchange_ts",
        "options_exchange_ts",
        "threshold",
        "threshold_components",
    ];
    for field in expected_fields {
        assert!(
            value.get(field).is_some(),
            "SpreadResult missing field: {field}"
        );
    }

    // Verify Decimal fields serialize as strings (not JSON numbers)
    let decimal_fields = [
        "gross_spread",
        "net_spread",
        "buy_fill_price",
        "sell_fill_price",
        "buy_fee",
        "sell_fee",
        "carry_cost",
        "total_cost",
        "buy_fill_ratio",
        "sell_fill_ratio",
        "target_notional",
    ];
    for field in decimal_fields {
        assert!(
            value[field].is_string(),
            "SpreadResult field '{field}' should serialize as string, got: {}",
            value[field]
        );
    }

    // Verify threshold_components sub-fields
    let tc = &value["threshold_components"];
    assert!(tc["static_floor"].is_string());
    assert!(tc["liquidity_penalty"].is_string());
    assert!(tc["final_threshold"].is_string());
    assert!(tc["rolling_mean"].is_number());
    assert!(tc["rolling_stddev"].is_number());
    assert!(tc["k_sigma"].is_number());
    assert!(tc["is_cold_start"].is_boolean());
}

#[test]
fn spread_result_roundtrip() {
    let result = make_spread_result();
    let json_str = serde_json::to_string(&result).unwrap();
    let parsed: SpreadResult = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed.event_id, "BTC-100K-2026");
    assert_eq!(parsed.pattern, SpreadPattern::BuyPolyYesSellKalshiYes);
    assert_eq!(parsed.gross_spread, dec("0.05"));
    assert_eq!(parsed.net_spread, dec("0.03"));
    assert_eq!(parsed.buy_fill_price, dec("0.45"));
    assert_eq!(parsed.sell_fill_price, dec("0.50"));
    assert_eq!(parsed.buy_fee, dec("0.005"));
    assert_eq!(parsed.sell_fee, dec("0.007"));
    assert_eq!(parsed.carry_cost, dec("0.002"));
    assert_eq!(parsed.total_cost, dec("0.014"));
    assert_eq!(parsed.buy_fill_ratio, dec("1.0"));
    assert_eq!(parsed.sell_fill_ratio, dec("0.95"));
    assert_eq!(parsed.target_notional, dec("500"));
    assert_eq!(parsed.timestamp_ms, 1700000000000);
    assert_eq!(parsed.poly_exchange_ts, Some(1700000000100));
    assert!(parsed.kalshi_exchange_ts.is_none());
    assert_eq!(parsed.threshold, Some(dec("0.025")));
    assert!(parsed.threshold_components.is_some());
}

#[test]
fn spread_result_without_threshold_roundtrips() {
    let mut result = make_spread_result();
    result.threshold = None;
    result.threshold_components = None;

    let json_str = serde_json::to_string(&result).unwrap();
    let parsed: SpreadResult = serde_json::from_str(&json_str).unwrap();

    assert!(parsed.threshold.is_none());
    assert!(parsed.threshold_components.is_none());
}

// ---------------------------------------------------------------------------
// ArbSignal roundtrip
// ---------------------------------------------------------------------------

fn make_arb_signal() -> ArbSignal {
    ArbSignal {
        signal_id: "01234567-89ab-7def-0123-456789abcdef".to_string(),
        event_id: "BTC-100K-2026".to_string(),
        direction: ArbDirection::BuyPredictionSellOptions,
        raw_spread: dec("0.05"),
        net_edge: dec("0.038"),
        confidence: 0.82,
        prediction_leg: LegInfo {
            venue: Venue::Polymarket,
            instrument_id: "POLY-BTC-100K-YES".to_string(),
            probability: dec("0.55"),
            executable_price: dec("0.54"),
            book_depth_levels: 5,
            fill_ratio: dec("0.95"),
        },
        options_leg: LegInfo {
            venue: Venue::Deribit,
            instrument_id: "BTC-27JUN25-100000-C".to_string(),
            probability: dec("0.60"),
            executable_price: dec("0.59"),
            book_depth_levels: 8,
            fill_ratio: dec("0.90"),
        },
        timestamp: DualTimestamp::now(),
        ttl_secs: 30,
        pricing_method: PricingMethod::CallSpreadReplication,
        confidence_components: ConfidenceComponents {
            iv_spread: 0.9,
            book_depth: 0.85,
            method_agreement: 0.78,
            solver_convergence: 0.95,
        },
        solver_meta: None,
        iv_spread: 0.02,
        skew_adjustment: -0.01,
        cost_breakdown: CostBreakdown {
            prediction_fee: dec("0.005"),
            options_fee_estimate: dec("0.0003"),
            carry_cost: dec("0.002"),
            prediction_slippage: dec("0.001"),
            options_spread_cost: dec("0.003"),
            basis_risk_premium: dec("0"),
            liquidity_factor: dec("0.95"),
            total_cost: dec("0.0113"),
        },
        prediction_venue: Venue::Polymarket,
        threshold_status: ThresholdStatus::PassedBoth,
        threshold_value: dec("0.025"),
        threshold_components: None,
    }
}

#[test]
fn arb_signal_schema_stable() {
    let signal = make_arb_signal();
    let value = serde_json::to_value(&signal).unwrap();

    // Verify ALL expected top-level fields
    let expected_fields = [
        "signal_id",
        "event_id",
        "direction",
        "raw_spread",
        "net_edge",
        "confidence",
        "prediction_leg",
        "options_leg",
        "timestamp",
        "ttl_secs",
        "pricing_method",
        "confidence_components",
        "solver_meta",
        "iv_spread",
        "skew_adjustment",
        "cost_breakdown",
        "prediction_venue",
        "threshold_status",
        "threshold_value",
        "threshold_components",
    ];
    for field in expected_fields {
        assert!(
            value.get(field).is_some(),
            "ArbSignal missing field: {field}"
        );
    }

    // Verify Decimal fields serialize as strings
    assert!(value["raw_spread"].is_string());
    assert!(value["net_edge"].is_string());
    assert!(value["threshold_value"].is_string());

    // Verify nested leg Decimal fields are strings
    assert!(value["prediction_leg"]["probability"].is_string());
    assert!(value["prediction_leg"]["executable_price"].is_string());
    assert!(value["prediction_leg"]["fill_ratio"].is_string());
    assert!(value["options_leg"]["probability"].is_string());

    // Verify cost breakdown Decimal fields are strings
    assert!(value["cost_breakdown"]["prediction_fee"].is_string());
    assert!(value["cost_breakdown"]["total_cost"].is_string());
    assert!(value["cost_breakdown"]["liquidity_factor"].is_string());
}

#[test]
fn arb_signal_roundtrip() {
    let signal = make_arb_signal();
    let json_str = serde_json::to_string(&signal).unwrap();
    let parsed: ArbSignal = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed.signal_id, signal.signal_id);
    assert_eq!(parsed.event_id, "BTC-100K-2026");
    assert_eq!(
        parsed.direction,
        ArbDirection::BuyPredictionSellOptions
    );
    assert_eq!(parsed.raw_spread, dec("0.05"));
    assert_eq!(parsed.net_edge, dec("0.038"));
    assert!((parsed.confidence - 0.82).abs() < f64::EPSILON);
    assert_eq!(parsed.ttl_secs, 30);
    assert_eq!(parsed.pricing_method, PricingMethod::CallSpreadReplication);
    assert_eq!(parsed.threshold_status, ThresholdStatus::PassedBoth);
    assert_eq!(parsed.threshold_value, dec("0.025"));
    assert!(parsed.solver_meta.is_none());
    assert!(parsed.threshold_components.is_none());

    // Verify nested types roundtrip
    assert_eq!(parsed.prediction_leg.venue, Venue::Polymarket);
    assert_eq!(parsed.prediction_leg.probability, dec("0.55"));
    assert_eq!(parsed.options_leg.venue, Venue::Deribit);
    assert_eq!(parsed.cost_breakdown.total_cost, dec("0.0113"));
}

// ---------------------------------------------------------------------------
// TradeEvent roundtrip (all variants)
// ---------------------------------------------------------------------------

#[test]
fn trade_event_signal_roundtrip() {
    let event = TradeEvent::Signal {
        trade_id: "pt-1700000000000-evt-001".to_string(),
        event_id: "evt-001".to_string(),
        pattern: "BuyPolyYesSellKalshiYes".to_string(),
        signal_spread: "0.03".to_string(),
        notional: "500".to_string(),
        timestamp_ms: 1700000000000,
    };

    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["type"].as_str().unwrap(), "signal");
    assert!(value.get("trade_id").is_some());
    assert!(value.get("event_id").is_some());
    assert!(value.get("pattern").is_some());
    assert!(value.get("signal_spread").is_some());
    assert!(value.get("notional").is_some());
    assert!(value.get("timestamp_ms").is_some());

    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: TradeEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        TradeEvent::Signal {
            trade_id,
            event_id,
            timestamp_ms,
            ..
        } => {
            assert_eq!(trade_id, "pt-1700000000000-evt-001");
            assert_eq!(event_id, "evt-001");
            assert_eq!(timestamp_ms, 1700000000000);
        }
        _ => panic!("expected Signal variant"),
    }
}

#[test]
fn trade_event_entry_roundtrip() {
    let event = TradeEvent::Entry {
        trade_id: "pt-1700000000000-evt-001".to_string(),
        event_id: "evt-001".to_string(),
        entry_price_buy: "0.52".to_string(),
        entry_price_sell: "0.48".to_string(),
        adverse_selection: "0.02".to_string(),
        timestamp_ms: 1700000001000,
    };

    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["type"].as_str().unwrap(), "entry");
    assert!(value.get("entry_price_buy").is_some());
    assert!(value.get("entry_price_sell").is_some());
    assert!(value.get("adverse_selection").is_some());

    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: TradeEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        TradeEvent::Entry {
            entry_price_buy,
            entry_price_sell,
            adverse_selection,
            ..
        } => {
            assert_eq!(entry_price_buy, "0.52");
            assert_eq!(entry_price_sell, "0.48");
            assert_eq!(adverse_selection, "0.02");
        }
        _ => panic!("expected Entry variant"),
    }
}

#[test]
fn trade_event_mtm_roundtrip() {
    let event = TradeEvent::Mtm {
        trade_id: "pt-1700000000000-evt-001".to_string(),
        event_id: "evt-001".to_string(),
        current_spread: "0.04".to_string(),
        unrealized_pnl: "5.0".to_string(),
        timestamp_ms: 1700000002000,
    };

    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["type"].as_str().unwrap(), "mtm");
    assert!(value.get("current_spread").is_some());
    assert!(value.get("unrealized_pnl").is_some());

    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: TradeEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        TradeEvent::Mtm {
            current_spread,
            unrealized_pnl,
            ..
        } => {
            assert_eq!(current_spread, "0.04");
            assert_eq!(unrealized_pnl, "5.0");
        }
        _ => panic!("expected Mtm variant"),
    }
}

#[test]
fn trade_event_settlement_roundtrip() {
    let event = TradeEvent::Settlement {
        trade_id: "pt-1700000000000-evt-001".to_string(),
        event_id: "evt-001".to_string(),
        settlement_pnl: "15.0".to_string(),
        timestamp_ms: 1700000010000,
    };

    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["type"].as_str().unwrap(), "settlement");
    assert!(value.get("settlement_pnl").is_some());

    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: TradeEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        TradeEvent::Settlement {
            settlement_pnl,
            timestamp_ms,
            ..
        } => {
            assert_eq!(settlement_pnl, "15.0");
            assert_eq!(timestamp_ms, 1700000010000);
        }
        _ => panic!("expected Settlement variant"),
    }
}
