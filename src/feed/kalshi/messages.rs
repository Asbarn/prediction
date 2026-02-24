//! Serde types for Kalshi WebSocket events.
//!
//! Kalshi WS delivers `orderbook_snapshot` and `orderbook_delta` messages
//! for order book data. Messages are deserialized defensively: structured
//! parse attempted first, unknown messages logged as raw JSON.

use serde::Deserialize;

/// Top-level Kalshi WebSocket message.
///
/// Kalshi tags messages with a `type` field. We use `serde(tag = "type")`
/// for known types and fall back to raw JSON for unknown messages.
#[derive(Debug, Clone)]
pub enum KalshiMessage {
    /// Full order book snapshot for a market.
    OrderbookSnapshot(OrderbookSnapshotData),
    /// Incremental order book delta for a single price level.
    OrderbookDelta(OrderbookDeltaData),
    /// Subscription acknowledgment.
    Subscribed(SubscribedData),
    /// Error message from the server.
    Error(ErrorData),
    /// Unknown message type -- logged but not processed.
    Unknown(String),
}

/// Orderbook snapshot data.
///
/// Contains the full order book for a market. `yes` and `no` are arrays
/// of `[price_cents, quantity]` pairs.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookSnapshotData {
    pub market_ticker: String,
    /// YES side bid levels: each element is `[price_cents, quantity]`.
    #[serde(default)]
    pub yes: Vec<[i64; 2]>,
    /// NO side bid levels: each element is `[price_cents, quantity]`.
    #[serde(default)]
    pub no: Vec<[i64; 2]>,
    /// Sequence number for ordering.
    pub seq: Option<u64>,
}

/// Orderbook delta data.
///
/// Represents a single price-level change. `delta` is positive for
/// added quantity, negative for removed.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookDeltaData {
    /// Market ticker or ID.
    #[serde(alias = "market_id")]
    pub market_ticker: String,
    /// Price in cents (1-99).
    pub price: i64,
    /// Change in quantity at this price level.
    pub delta: i64,
    /// Which side: "yes" or "no".
    pub side: String,
    /// Sequence number for ordering.
    pub seq: Option<u64>,
    /// Exchange timestamp (ISO 8601), if provided by API.
    /// e.g., "2022-11-22T20:44:01Z". Second-precision only.
    pub ts: Option<String>,
}

/// Subscription acknowledgment.
#[derive(Debug, Clone, Deserialize)]
pub struct SubscribedData {
    pub id: i64,
    #[serde(default)]
    pub msg: String,
}

/// Error response from the server.
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorData {
    pub code: Option<i64>,
    pub msg: String,
}

impl KalshiMessage {
    /// Parse a raw JSON string into a KalshiMessage.
    ///
    /// Handles two formats:
    /// - **Nested (live API):** `{"type": "orderbook_delta", "sid": 1, "seq": 5, "msg": {...}}`
    ///   where the actual data fields are inside the `msg` object.
    /// - **Flat (recordings/older API):** `{"type": "orderbook_delta", "market_ticker": "...", ...}`
    ///   where data fields are at the top level alongside `type`.
    ///
    /// Falls back to `Unknown` for unrecognized message formats.
    pub fn parse(text: &str) -> Self {
        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return KalshiMessage::Unknown(text.to_string()),
        };

        // Determine if this is wrapped (has "msg" object) or flat format.
        // Note: SubscribedData also has a "msg" field, but it's a string, not an object.
        let (msg_type_str, payload) =
            if let Some(msg_obj) = value.get("msg").filter(|v| v.is_object()) {
                let t = value.get("type").and_then(|t| t.as_str());
                (t.map(String::from), msg_obj.clone())
            } else {
                let t = value.get("type").and_then(|t| t.as_str());
                (t.map(String::from), value.clone())
            };

        if let Some(ref msg_type) = msg_type_str {
            match msg_type.as_str() {
                "orderbook_snapshot" => {
                    match serde_json::from_value::<OrderbookSnapshotData>(payload) {
                        Ok(data) => return KalshiMessage::OrderbookSnapshot(data),
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "failed to parse orderbook_snapshot payload"
                            );
                            return KalshiMessage::Unknown(text.to_string());
                        }
                    }
                }
                "orderbook_delta" => {
                    match serde_json::from_value::<OrderbookDeltaData>(payload) {
                        Ok(data) => return KalshiMessage::OrderbookDelta(data),
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "failed to parse orderbook_delta payload"
                            );
                            return KalshiMessage::Unknown(text.to_string());
                        }
                    }
                }
                "error" => {
                    match serde_json::from_value::<ErrorData>(payload) {
                        Ok(data) => return KalshiMessage::Error(data),
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to parse error payload");
                            return KalshiMessage::Unknown(text.to_string());
                        }
                    }
                }
                _ => {
                    tracing::debug!(msg_type = %msg_type, "unknown Kalshi message type");
                }
            }
        }

        // Check for subscription acknowledgment (has "id" and "msg" fields, no "type").
        // SubscribedData.msg is a string (e.g., "subscribed"), not a nested object.
        if value.get("id").is_some() && value.get("msg").is_some() {
            match serde_json::from_value::<SubscribedData>(value) {
                Ok(data) => return KalshiMessage::Subscribed(data),
                Err(_) => {}
            }
        }

        KalshiMessage::Unknown(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_orderbook_snapshot() {
        let json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXBTC-26FEB22-T100000",
            "yes": [[42, 100], [45, 200]],
            "no": [[55, 150], [58, 300]],
            "seq": 1
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookSnapshot(data) => {
                assert_eq!(data.market_ticker, "KXBTC-26FEB22-T100000");
                assert_eq!(data.yes.len(), 2);
                assert_eq!(data.no.len(), 2);
                assert_eq!(data.yes[0], [42, 100]);
                assert_eq!(data.yes[1], [45, 200]);
                assert_eq!(data.no[0], [55, 150]);
                assert_eq!(data.no[1], [58, 300]);
                assert_eq!(data.seq, Some(1));
            }
            other => panic!("expected OrderbookSnapshot, got {:?}", other),
        }
    }

    #[test]
    fn parse_orderbook_delta() {
        let json = r#"{
            "type": "orderbook_delta",
            "market_ticker": "KXBTC-26FEB22-T100000",
            "price": 42,
            "delta": 50,
            "side": "yes",
            "seq": 2
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookDelta(data) => {
                assert_eq!(data.market_ticker, "KXBTC-26FEB22-T100000");
                assert_eq!(data.price, 42);
                assert_eq!(data.delta, 50);
                assert_eq!(data.side, "yes");
                assert_eq!(data.seq, Some(2));
            }
            other => panic!("expected OrderbookDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_orderbook_delta_with_market_id_alias() {
        let json = r#"{
            "type": "orderbook_delta",
            "market_id": "KXBTC-26FEB22-T100000",
            "price": 42,
            "delta": -30,
            "side": "no",
            "seq": 3
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookDelta(data) => {
                assert_eq!(data.market_ticker, "KXBTC-26FEB22-T100000");
                assert_eq!(data.delta, -30);
                assert_eq!(data.side, "no");
            }
            other => panic!("expected OrderbookDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_subscribed() {
        let json = r#"{
            "id": 1,
            "msg": "subscribed"
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::Subscribed(data) => {
                assert_eq!(data.id, 1);
                assert_eq!(data.msg, "subscribed");
            }
            other => panic!("expected Subscribed, got {:?}", other),
        }
    }

    #[test]
    fn parse_error_message() {
        let json = r#"{
            "type": "error",
            "code": 401,
            "msg": "unauthorized"
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::Error(data) => {
                assert_eq!(data.code, Some(401));
                assert_eq!(data.msg, "unauthorized");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_message_type() {
        let json = r#"{"type": "some_future_type", "data": 123}"#;
        match KalshiMessage::parse(json) {
            KalshiMessage::Unknown(_) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn parse_invalid_json_returns_unknown() {
        match KalshiMessage::parse("not valid json") {
            KalshiMessage::Unknown(_) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn parse_snapshot_with_empty_sides() {
        let json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXBTC-TEST",
            "yes": [],
            "no": []
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookSnapshot(data) => {
                assert_eq!(data.market_ticker, "KXBTC-TEST");
                assert!(data.yes.is_empty());
                assert!(data.no.is_empty());
                assert_eq!(data.seq, None);
            }
            other => panic!("expected OrderbookSnapshot, got {:?}", other),
        }
    }

    #[test]
    fn parse_nested_orderbook_delta_with_ts() {
        let json = r#"{
            "type": "orderbook_delta",
            "sid": 1,
            "seq": 5,
            "msg": {
                "market_ticker": "KXBTC-26FEB22-T100000",
                "price": 42,
                "delta": 50,
                "side": "yes",
                "ts": "2024-01-15T10:30:00Z"
            }
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookDelta(data) => {
                assert_eq!(data.market_ticker, "KXBTC-26FEB22-T100000");
                assert_eq!(data.price, 42);
                assert_eq!(data.delta, 50);
                assert_eq!(data.side, "yes");
                assert_eq!(data.ts, Some("2024-01-15T10:30:00Z".to_string()));
            }
            other => panic!("expected OrderbookDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_nested_orderbook_snapshot() {
        let json = r#"{
            "type": "orderbook_snapshot",
            "sid": 1,
            "seq": 1,
            "msg": {
                "market_ticker": "KXBTC-TEST",
                "yes": [[42, 100], [45, 200]],
                "no": [[55, 150]]
            }
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookSnapshot(data) => {
                assert_eq!(data.market_ticker, "KXBTC-TEST");
                assert_eq!(data.yes.len(), 2);
                assert_eq!(data.yes[0], [42, 100]);
                assert_eq!(data.no.len(), 1);
                assert_eq!(data.no[0], [55, 150]);
            }
            other => panic!("expected OrderbookSnapshot, got {:?}", other),
        }
    }

    #[test]
    fn parse_flat_delta_still_works() {
        // Existing flat format must continue to work (backward compat with recordings)
        let json = r#"{
            "type": "orderbook_delta",
            "market_ticker": "KXBTC-26FEB22-T100000",
            "price": 42,
            "delta": 50,
            "side": "yes",
            "seq": 2
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookDelta(data) => {
                assert_eq!(data.market_ticker, "KXBTC-26FEB22-T100000");
                assert_eq!(data.price, 42);
                assert_eq!(data.delta, 50);
                assert_eq!(data.side, "yes");
                assert_eq!(data.seq, Some(2));
                assert_eq!(data.ts, None);
            }
            other => panic!("expected OrderbookDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_delta_ts_field_optional() {
        // Flat delta without ts field should produce ts: None
        let json = r#"{
            "type": "orderbook_delta",
            "market_ticker": "KXTEST",
            "price": 55,
            "delta": -10,
            "side": "no"
        }"#;

        match KalshiMessage::parse(json) {
            KalshiMessage::OrderbookDelta(data) => {
                assert_eq!(data.market_ticker, "KXTEST");
                assert_eq!(data.ts, None);
            }
            other => panic!("expected OrderbookDelta, got {:?}", other),
        }
    }
}
