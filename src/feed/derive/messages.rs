//! Derive.xyz JSON-RPC 2.0 message types.
//!
//! Key differences from Deribit:
//! - No heartbeat variant (Derive uses WS-level PING/PONG)
//! - Prices and amounts are **strings**, not numbers
//! - `ticker_slim` uses abbreviated single-letter keys
//! - Response `id` is `i64` (not `u64`)

use serde::Deserialize;

/// Top-level JSON-RPC message from Derive.
///
/// Only two variants (no heartbeat -- Derive uses WS PING/PONG):
/// - `Response` -- reply to a request we sent (has `id`)
/// - `Notification` -- subscription data push (has `method`)
///
/// **Variant ordering matters**: serde tries each in order with `untagged`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DeriveMessage {
    /// Response to a request we sent (has `id` field).
    Response(DeriveRpcResponse),
    /// Subscription notification (has `method` field).
    Notification(DeriveRpcNotification),
}

/// JSON-RPC 2.0 response to a request.
#[derive(Debug, Deserialize)]
pub struct DeriveRpcResponse {
    pub id: i64,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<DeriveRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct DeriveRpcError {
    pub code: i64,
    pub message: String,
}

/// JSON-RPC 2.0 subscription notification.
#[derive(Debug, Deserialize)]
pub struct DeriveRpcNotification {
    pub method: String,
    pub params: DeriveNotificationParams,
}

/// Params of a subscription notification.
#[derive(Debug, Deserialize)]
pub struct DeriveNotificationParams {
    pub channel: String,
    /// Raw data -- parsed further based on channel kind.
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Channel-specific data structures
// ---------------------------------------------------------------------------

/// Orderbook channel data: `orderbook.{instrument}.{group}.{depth}`
///
/// Derive sends snapshot-only updates (~100ms). All prices and amounts are
/// **strings** (unlike Deribit which uses floats).
#[derive(Debug, Deserialize)]
pub struct DeriveBookData {
    /// Milliseconds since epoch.
    pub timestamp: i64,
    pub instrument_name: String,
    /// Monotonically increasing sequence number.
    pub publish_id: i64,
    /// Bid levels: `[[price_str, amount_str], ...]`
    pub bids: Vec<[String; 2]>,
    /// Ask levels: `[[price_str, amount_str], ...]`
    pub asks: Vec<[String; 2]>,
}

/// Ticker slim channel wrapper: `ticker_slim.{instrument}.{interval}`
///
/// The outer object wraps `instrument_ticker` which contains abbreviated keys.
/// This matches the live API structure (Pitfall 6 from research).
#[derive(Debug, Deserialize)]
pub struct DeriveTickerSlimWrapper {
    pub timestamp: i64,
    pub instrument_ticker: DeriveTickerSlimData,
}

/// Ticker slim inner data with abbreviated single-letter keys.
///
/// All numeric fields are `Option<String>` to handle nulls (Pitfall 1).
#[derive(Debug, Deserialize)]
pub struct DeriveTickerSlimData {
    /// Timestamp (ms).
    #[serde(rename = "t")]
    pub timestamp: Option<i64>,
    /// Best ask amount.
    #[serde(rename = "A")]
    pub best_ask_amount: Option<String>,
    /// Best ask price (USDC).
    #[serde(rename = "a")]
    pub best_ask_price: Option<String>,
    /// Best bid amount.
    #[serde(rename = "B")]
    pub best_bid_amount: Option<String>,
    /// Best bid price (USDC).
    #[serde(rename = "b")]
    pub best_bid_price: Option<String>,
    /// Index price.
    #[serde(rename = "I")]
    pub index_price: Option<String>,
    /// Mark price.
    #[serde(rename = "M")]
    pub mark_price: Option<String>,
    /// Forward price (can be null -- Pitfall 1).
    #[serde(rename = "f")]
    pub forward_price: Option<String>,
    /// Option greeks and IV data (present for options instruments).
    pub option_pricing: Option<DeriveOptionPricing>,
}

/// Option pricing data from ticker_slim, with abbreviated keys.
#[derive(Debug, Deserialize)]
pub struct DeriveOptionPricing {
    /// Delta.
    #[serde(rename = "d")]
    pub delta: Option<String>,
    /// Theta.
    #[serde(rename = "t")]
    pub theta: Option<String>,
    /// Gamma.
    #[serde(rename = "g")]
    pub gamma: Option<String>,
    /// Vega.
    #[serde(rename = "v")]
    pub vega: Option<String>,
    /// Implied volatility (mid).
    #[serde(rename = "i")]
    pub iv: Option<String>,
    /// Risk-free rate.
    #[serde(rename = "r")]
    pub rate: Option<String>,
    /// Forward price.
    #[serde(rename = "f")]
    pub forward: Option<String>,
    /// Mark price.
    #[serde(rename = "m")]
    pub mark_price: Option<String>,
    /// Discount factor.
    #[serde(rename = "df")]
    pub discount_factor: Option<String>,
    /// Bid IV.
    #[serde(rename = "bi")]
    pub bid_iv: Option<String>,
    /// Ask IV.
    #[serde(rename = "ai")]
    pub ask_iv: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_orderbook_from_live_api() {
        // Exact JSON from DERIVE-API-FINDINGS.md
        let json = r#"{
            "timestamp": 1772624842966,
            "instrument_name": "BTC-20260305-69500-P",
            "publish_id": 56593,
            "bids": [["340", "0.4"], ["320", "1"], ["280", "0.70343"]],
            "asks": [["420", "0.4"], ["520", "0.70343"]]
        }"#;

        let data: DeriveBookData = serde_json::from_str(json).unwrap();
        assert_eq!(data.timestamp, 1772624842966);
        assert_eq!(data.instrument_name, "BTC-20260305-69500-P");
        assert_eq!(data.publish_id, 56593);
        assert_eq!(data.bids.len(), 3);
        assert_eq!(data.asks.len(), 2);
        assert_eq!(data.bids[0][0], "340");
        assert_eq!(data.bids[0][1], "0.4");
        assert_eq!(data.asks[1][0], "520");
        assert_eq!(data.asks[1][1], "0.70343");
    }

    #[test]
    fn deserialize_ticker_slim_from_live_api() {
        // Exact JSON from DERIVE-API-FINDINGS.md (data portion)
        let json = r#"{
            "timestamp": 1772624842966,
            "instrument_ticker": {
                "t": 1772624842966,
                "A": "0.4",
                "a": "414",
                "B": "0.4",
                "b": "341",
                "f": null,
                "option_pricing": {
                    "d": "-0.24967",
                    "t": "-453.85103",
                    "g": "0.00013192",
                    "v": "10.84014",
                    "i": "0.70513",
                    "r": "0.84114",
                    "f": "71067",
                    "m": "364",
                    "df": "1",
                    "bi": "0.68323",
                    "ai": "0.75013"
                },
                "I": "71078",
                "M": "364"
            }
        }"#;

        let wrapper: DeriveTickerSlimWrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.timestamp, 1772624842966);

        let ticker = &wrapper.instrument_ticker;
        assert_eq!(ticker.timestamp, Some(1772624842966));
        assert_eq!(ticker.best_ask_price.as_deref(), Some("414"));
        assert_eq!(ticker.best_bid_price.as_deref(), Some("341"));
        assert_eq!(ticker.index_price.as_deref(), Some("71078"));
        assert_eq!(ticker.mark_price.as_deref(), Some("364"));

        let pricing = ticker.option_pricing.as_ref().unwrap();
        assert_eq!(pricing.delta.as_deref(), Some("-0.24967"));
        assert_eq!(pricing.iv.as_deref(), Some("0.70513"));
        assert_eq!(pricing.bid_iv.as_deref(), Some("0.68323"));
        assert_eq!(pricing.ask_iv.as_deref(), Some("0.75013"));
    }

    #[test]
    fn ticker_slim_null_forward_deserializes() {
        // Pitfall 1: `f` field at top level can be null
        let json = r#"{
            "timestamp": 1772624842966,
            "instrument_ticker": {
                "t": 1772624842966,
                "A": "0.4",
                "a": "414",
                "B": "0.4",
                "b": "341",
                "f": null,
                "I": "71078",
                "M": "364"
            }
        }"#;

        let wrapper: DeriveTickerSlimWrapper = serde_json::from_str(json).unwrap();
        assert!(wrapper.instrument_ticker.forward_price.is_none());
        assert!(wrapper.instrument_ticker.option_pricing.is_none());
    }

    #[test]
    fn derive_message_routes_response_correctly() {
        // Subscribe response from live API
        let json = r#"{
            "id": 1,
            "result": {
                "status": {
                    "orderbook.BTC-20260305-69500-P.10.10": "ok"
                },
                "current_subscriptions": [
                    "orderbook.BTC-20260305-69500-P.10.10"
                ]
            }
        }"#;

        let msg: DeriveMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeriveMessage::Response(resp) => {
                assert_eq!(resp.id, 1);
                assert!(resp.error.is_none());
                assert!(resp.result.is_object());
            }
            _ => panic!("expected Response variant"),
        }
    }

    #[test]
    fn derive_message_routes_notification_correctly() {
        // Subscription notification from live API
        let json = r#"{
            "method": "subscription",
            "params": {
                "channel": "orderbook.BTC-20260305-69500-P.10.10",
                "data": {
                    "timestamp": 1772624842966,
                    "instrument_name": "BTC-20260305-69500-P",
                    "publish_id": 56593,
                    "bids": [["340", "0.4"]],
                    "asks": [["420", "0.4"]]
                }
            }
        }"#;

        let msg: DeriveMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeriveMessage::Notification(notif) => {
                assert_eq!(notif.method, "subscription");
                assert_eq!(
                    notif.params.channel,
                    "orderbook.BTC-20260305-69500-P.10.10"
                );

                // Further deserialize data into DeriveBookData
                let book: DeriveBookData =
                    serde_json::from_value(notif.params.data).unwrap();
                assert_eq!(book.instrument_name, "BTC-20260305-69500-P");
                assert_eq!(book.publish_id, 56593);
            }
            _ => panic!("expected Notification variant"),
        }
    }

    #[test]
    fn derive_message_response_with_error() {
        let json = r#"{
            "id": 2,
            "error": {
                "code": -32602,
                "message": "Invalid params"
            }
        }"#;

        let msg: DeriveMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeriveMessage::Response(resp) => {
                assert_eq!(resp.id, 2);
                let err = resp.error.unwrap();
                assert_eq!(err.code, -32602);
                assert_eq!(err.message, "Invalid params");
            }
            _ => panic!("expected Response variant"),
        }
    }
}
