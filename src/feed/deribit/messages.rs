//! Deribit JSON-RPC 2.0 message types.
//!
//! All types use `f64` at the serde boundary intentionally -- conversion to
//! `rust_decimal::Decimal` happens in the normalization layer (Plan 02),
//! NOT here. This avoids precision pitfalls by keeping deserialization simple.

use serde::Deserialize;

/// Top-level JSON-RPC message from Deribit.
///
/// Could be a response to our request or a subscription notification.
/// Uses `#[serde(untagged)]` because responses have `id` while
/// notifications have `method: "subscription"`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DeribitMessage {
    /// Response to a request we sent (has `id` field).
    Response(RpcResponse),
    /// Subscription notification (has `method: "subscription"`).
    Notification(RpcNotification),
}

/// JSON-RPC 2.0 response to a request.
#[derive(Debug, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<RpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// JSON-RPC 2.0 subscription notification.
#[derive(Debug, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: NotificationParams,
}

/// Params of a subscription notification.
#[derive(Debug, Deserialize)]
pub struct NotificationParams {
    pub channel: String,
    /// Raw data -- parsed further based on channel type.
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Channel-specific data structures
// ---------------------------------------------------------------------------

/// Book channel: `book.{instrument}.none.20.100ms`
///
/// The grouped channel sends complete top-20 snapshots. Each message replaces
/// the previous state. `bids` and `asks` are `[[price, amount], ...]` arrays.
#[derive(Debug, Deserialize)]
pub struct BookData {
    /// Milliseconds since epoch.
    pub timestamp: i64,
    pub instrument_name: String,
    pub change_id: i64,
    #[serde(default)]
    pub prev_change_id: Option<i64>,
    /// "snapshot" or "change".
    #[serde(rename = "type")]
    pub update_type: Option<String>,
    pub bids: Vec<[f64; 2]>,
    pub asks: Vec<[f64; 2]>,
}

/// Ticker channel: `ticker.{instrument}.raw`
///
/// Contains last price, mark price, index price, greeks (options),
/// funding (perpetuals), and stats.
#[derive(Debug, Deserialize)]
pub struct TickerData {
    pub timestamp: i64,
    pub instrument_name: String,
    pub state: String,
    pub last_price: Option<f64>,
    pub mark_price: f64,
    pub index_price: f64,
    pub best_bid_price: Option<f64>,
    pub best_bid_amount: Option<f64>,
    pub best_ask_price: Option<f64>,
    pub best_ask_amount: Option<f64>,
    pub open_interest: f64,
    pub min_price: f64,
    pub max_price: f64,
    // Options-specific fields
    pub underlying_price: Option<f64>,
    pub underlying_index: Option<String>,
    pub mark_iv: Option<f64>,
    pub bid_iv: Option<f64>,
    pub ask_iv: Option<f64>,
    pub interest_rate: Option<f64>,
    pub greeks: Option<TickerGreeks>,
    // Perpetual-specific fields
    pub funding_8h: Option<f64>,
    pub current_funding: Option<f64>,
    // Stats
    pub stats: Option<TickerStats>,
    pub estimated_delivery_price: Option<f64>,
}

/// Greeks for options ticker data.
#[derive(Debug, Deserialize)]
pub struct TickerGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}

/// Stats for ticker data.
#[derive(Debug, Deserialize)]
pub struct TickerStats {
    pub volume: Option<f64>,
    pub volume_usd: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub price_change: Option<f64>,
}

/// Trades channel: `trades.{instrument}.raw`
///
/// Note: the trades channel sends `data` as a `Vec<TradeData>`, not a
/// single object. A single notification can contain multiple trades.
#[derive(Debug, Deserialize)]
pub struct TradeData {
    pub trade_id: String,
    pub instrument_name: String,
    pub timestamp: i64,
    /// "buy" or "sell".
    pub direction: String,
    pub price: f64,
    pub amount: f64,
    pub trade_seq: i64,
    pub tick_direction: Option<i32>,
    /// "M", "T", or "MT" for liquidation trades.
    pub liquidation: Option<String>,
    pub mark_price: Option<f64>,
    pub index_price: Option<f64>,
    /// Options only: implied volatility.
    pub iv: Option<f64>,
}

/// Price index channel: `deribit_price_index.btc_usd`
#[derive(Debug, Deserialize)]
pub struct PriceIndexData {
    pub timestamp: i64,
    pub price: f64,
    pub index_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_rpc_response_success() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": ["ticker.BTC-27JUN25-100000-C.raw", "book.BTC-27JUN25-100000-C.none.20.100ms"]
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Response(resp) => {
                assert_eq!(resp.id, 1);
                assert_eq!(resp.jsonrpc, "2.0");
                assert!(resp.error.is_none());
                assert!(resp.result.is_array());
            }
            _ => panic!("expected Response variant"),
        }
    }

    #[test]
    fn deserialize_rpc_response_error() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 2,
            "error": {
                "code": -32602,
                "message": "Invalid params"
            }
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Response(resp) => {
                assert_eq!(resp.id, 2);
                let err = resp.error.unwrap();
                assert_eq!(err.code, -32602);
                assert_eq!(err.message, "Invalid params");
            }
            _ => panic!("expected Response variant"),
        }
    }

    #[test]
    fn deserialize_book_data_snapshot() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-27JUN25-100000-C.none.20.100ms",
                "data": {
                    "timestamp": 1703001600000,
                    "instrument_name": "BTC-27JUN25-100000-C",
                    "change_id": 12345678,
                    "type": "snapshot",
                    "bids": [[0.0055, 10.0], [0.0050, 25.0], [0.0045, 15.0]],
                    "asks": [[0.0060, 8.0], [0.0065, 12.0], [0.0070, 20.0]]
                }
            }
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Notification(notif) => {
                assert_eq!(notif.method, "subscription");
                assert_eq!(
                    notif.params.channel,
                    "book.BTC-27JUN25-100000-C.none.20.100ms"
                );

                let book: BookData = serde_json::from_value(notif.params.data).unwrap();
                assert_eq!(book.instrument_name, "BTC-27JUN25-100000-C");
                assert_eq!(book.change_id, 12345678);
                assert!(book.prev_change_id.is_none());
                assert_eq!(book.update_type.as_deref(), Some("snapshot"));
                assert_eq!(book.bids.len(), 3);
                assert_eq!(book.asks.len(), 3);
                assert!((book.bids[0][0] - 0.0055).abs() < f64::EPSILON);
                assert!((book.bids[0][1] - 10.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected Notification variant"),
        }
    }

    #[test]
    fn deserialize_book_data_with_prev_change_id() {
        let json = r#"{
            "timestamp": 1703001600100,
            "instrument_name": "BTC-27JUN25-100000-C",
            "change_id": 12345679,
            "prev_change_id": 12345678,
            "type": "snapshot",
            "bids": [[0.0056, 11.0]],
            "asks": [[0.0059, 9.0]]
        }"#;

        let book: BookData = serde_json::from_str(json).unwrap();
        assert_eq!(book.prev_change_id, Some(12345678));
        assert_eq!(book.change_id, 12345679);
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
    }

    #[test]
    fn deserialize_ticker_data_with_greeks() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "ticker.BTC-27JUN25-100000-C.raw",
                "data": {
                    "timestamp": 1703001600000,
                    "instrument_name": "BTC-27JUN25-100000-C",
                    "state": "open",
                    "last_price": 0.0055,
                    "mark_price": 0.0057,
                    "index_price": 43500.0,
                    "best_bid_price": 0.0055,
                    "best_bid_amount": 10.0,
                    "best_ask_price": 0.0060,
                    "best_ask_amount": 8.0,
                    "open_interest": 500.0,
                    "min_price": 0.0001,
                    "max_price": 0.5,
                    "underlying_price": 43500.0,
                    "underlying_index": "BTC-27JUN25",
                    "mark_iv": 65.5,
                    "bid_iv": 64.0,
                    "ask_iv": 67.0,
                    "interest_rate": 0.0,
                    "greeks": {
                        "delta": 0.05,
                        "gamma": 0.00001,
                        "vega": 5.5,
                        "theta": -0.5,
                        "rho": 0.001
                    },
                    "stats": {
                        "volume": 100.0,
                        "volume_usd": 5500.0,
                        "high": 0.0065,
                        "low": 0.0045,
                        "price_change": 2.5
                    },
                    "estimated_delivery_price": 43500.0
                }
            }
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Notification(notif) => {
                let ticker: TickerData = serde_json::from_value(notif.params.data).unwrap();
                assert_eq!(ticker.instrument_name, "BTC-27JUN25-100000-C");
                assert_eq!(ticker.state, "open");
                assert!((ticker.mark_price - 0.0057).abs() < f64::EPSILON);
                assert!((ticker.index_price - 43500.0).abs() < f64::EPSILON);

                let greeks = ticker.greeks.unwrap();
                assert!((greeks.delta - 0.05).abs() < f64::EPSILON);
                assert!((greeks.vega - 5.5).abs() < f64::EPSILON);

                let stats = ticker.stats.unwrap();
                assert!((stats.volume.unwrap() - 100.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected Notification variant"),
        }
    }

    #[test]
    fn deserialize_ticker_data_perpetual() {
        let json = r#"{
            "timestamp": 1703001600000,
            "instrument_name": "BTC-PERPETUAL",
            "state": "open",
            "last_price": 43500.0,
            "mark_price": 43501.5,
            "index_price": 43499.0,
            "best_bid_price": 43500.0,
            "best_bid_amount": 5.0,
            "best_ask_price": 43501.0,
            "best_ask_amount": 3.0,
            "open_interest": 25000.0,
            "min_price": 43000.0,
            "max_price": 44000.0,
            "funding_8h": 0.0001,
            "current_funding": 0.00005,
            "stats": {
                "volume": 1500.0,
                "volume_usd": 65250000.0,
                "high": 44000.0,
                "low": 43000.0,
                "price_change": 1.2
            }
        }"#;

        let ticker: TickerData = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.instrument_name, "BTC-PERPETUAL");
        assert!(ticker.greeks.is_none());
        assert!((ticker.funding_8h.unwrap() - 0.0001).abs() < f64::EPSILON);
        assert!((ticker.current_funding.unwrap() - 0.00005).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_trade_data_array() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "trades.BTC-27JUN25-100000-C.raw",
                "data": [
                    {
                        "trade_id": "123456",
                        "instrument_name": "BTC-27JUN25-100000-C",
                        "timestamp": 1703001600000,
                        "direction": "buy",
                        "price": 0.0055,
                        "amount": 5.0,
                        "trade_seq": 100,
                        "tick_direction": 0,
                        "mark_price": 0.0057,
                        "index_price": 43500.0,
                        "iv": 65.5
                    },
                    {
                        "trade_id": "123457",
                        "instrument_name": "BTC-27JUN25-100000-C",
                        "timestamp": 1703001600050,
                        "direction": "sell",
                        "price": 0.0054,
                        "amount": 3.0,
                        "trade_seq": 101,
                        "tick_direction": 3,
                        "mark_price": 0.0057,
                        "index_price": 43500.0,
                        "iv": 65.0
                    }
                ]
            }
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Notification(notif) => {
                assert_eq!(notif.params.channel, "trades.BTC-27JUN25-100000-C.raw");

                // Trades come as an array
                let trades: Vec<TradeData> = serde_json::from_value(notif.params.data).unwrap();
                assert_eq!(trades.len(), 2);
                assert_eq!(trades[0].trade_id, "123456");
                assert_eq!(trades[0].direction, "buy");
                assert!((trades[0].price - 0.0055).abs() < f64::EPSILON);
                assert_eq!(trades[1].trade_id, "123457");
                assert_eq!(trades[1].direction, "sell");
                assert!((trades[1].iv.unwrap() - 65.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected Notification variant"),
        }
    }

    #[test]
    fn deserialize_trade_data_with_liquidation() {
        let json = r#"{
            "trade_id": "789012",
            "instrument_name": "BTC-PERPETUAL",
            "timestamp": 1703001600000,
            "direction": "sell",
            "price": 43400.0,
            "amount": 10.0,
            "trade_seq": 200,
            "liquidation": "M",
            "mark_price": 43401.0,
            "index_price": 43399.0
        }"#;

        let trade: TradeData = serde_json::from_str(json).unwrap();
        assert_eq!(trade.liquidation.as_deref(), Some("M"));
        assert!(trade.iv.is_none());
        assert!(trade.tick_direction.is_none());
    }

    #[test]
    fn deserialize_price_index_data() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "deribit_price_index.btc_usd",
                "data": {
                    "timestamp": 1703001600000,
                    "price": 43500.25,
                    "index_name": "btc_usd"
                }
            }
        }"#;

        let msg: DeribitMessage = serde_json::from_str(json).unwrap();
        match msg {
            DeribitMessage::Notification(notif) => {
                assert_eq!(notif.params.channel, "deribit_price_index.btc_usd");

                let index: PriceIndexData = serde_json::from_value(notif.params.data).unwrap();
                assert_eq!(index.index_name, "btc_usd");
                assert!((index.price - 43500.25).abs() < f64::EPSILON);
                assert_eq!(index.timestamp, 1703001600000);
            }
            _ => panic!("expected Notification variant"),
        }
    }

    #[test]
    fn deserialize_notification_distinguishes_from_response() {
        // A notification has method + params, no id
        let notification = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "deribit_price_index.btc_usd",
                "data": {"timestamp": 1703001600000, "price": 43500.0, "index_name": "btc_usd"}
            }
        }"#;

        // A response has id + result, no method
        let response = r#"{
            "jsonrpc": "2.0",
            "id": 42,
            "result": ["deribit_price_index.btc_usd"]
        }"#;

        let notif_msg: DeribitMessage = serde_json::from_str(notification).unwrap();
        let resp_msg: DeribitMessage = serde_json::from_str(response).unwrap();

        assert!(matches!(notif_msg, DeribitMessage::Notification(_)));
        assert!(matches!(resp_msg, DeribitMessage::Response(_)));
    }

    #[test]
    fn deserialize_book_data_empty_levels() {
        let json = r#"{
            "timestamp": 1703001600000,
            "instrument_name": "BTC-27JUN25-200000-C",
            "change_id": 99999,
            "type": "snapshot",
            "bids": [],
            "asks": [[0.0001, 5.0]]
        }"#;

        let book: BookData = serde_json::from_str(json).unwrap();
        assert!(book.bids.is_empty());
        assert_eq!(book.asks.len(), 1);
    }

    #[test]
    fn deserialize_ticker_minimal_fields() {
        // Test with only required fields, no optional ones
        let json = r#"{
            "timestamp": 1703001600000,
            "instrument_name": "BTC-27JUN25-100000-C",
            "state": "open",
            "mark_price": 0.0057,
            "index_price": 43500.0,
            "open_interest": 500.0,
            "min_price": 0.0001,
            "max_price": 0.5
        }"#;

        let ticker: TickerData = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.instrument_name, "BTC-27JUN25-100000-C");
        assert!(ticker.last_price.is_none());
        assert!(ticker.greeks.is_none());
        assert!(ticker.stats.is_none());
        assert!(ticker.funding_8h.is_none());
    }
}
