//! Polymarket CLOB WebSocket message types.
//!
//! Polymarket's market channel sends `book` events (full order book snapshots)
//! and `price_change` events (incremental updates). Prices are strings in 0-1
//! probability space (e.g., "0.55" = 55% probability).
//!
//! **Important (Pitfall 5):** WebSocket frames can be JSON arrays `[{...}, {...}]`
//! or single objects `{...}`. The client handles this by checking the first byte.

use serde::Deserialize;

/// Top-level Polymarket event, tagged by `event_type`.
#[derive(Debug, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum PolymarketEvent {
    /// Full order book snapshot.
    Book(PolymarketBookEvent),
    /// Incremental price change notification.
    PriceChange(PolymarketPriceChange),
    /// Tick size change notification.
    #[serde(rename = "tick_size_change")]
    TickSizeChange(serde_json::Value),
    /// Any other event type we do not yet handle.
    #[serde(other)]
    Unknown,
}

/// Full order book snapshot from the market channel.
///
/// Polymarket sorts bids descending (best first) and asks ascending (best first).
/// Prices and sizes are decimal strings for exact precision.
#[derive(Debug, Deserialize)]
pub struct PolymarketBookEvent {
    /// Token/asset ID (outcome-level identifier).
    pub asset_id: String,
    /// Market/condition ID.
    pub market: String,
    /// Book verification hash.
    #[serde(default)]
    pub hash: String,
    /// Bid levels, sorted descending by price (best bid first).
    #[serde(default)]
    pub bids: Vec<PriceLevel>,
    /// Ask levels, sorted ascending by price (best ask first).
    #[serde(default)]
    pub asks: Vec<PriceLevel>,
    /// Timestamp as a string (milliseconds since epoch).
    #[serde(default)]
    pub timestamp: String,
}

/// A single price level in the order book.
#[derive(Debug, Deserialize)]
pub struct PriceLevel {
    /// Price as a decimal string in 0-1 range (e.g., "0.55").
    pub price: String,
    /// Size/quantity as a decimal string (e.g., "100.0").
    pub size: String,
}

/// Incremental price change event.
#[derive(Debug, Deserialize)]
pub struct PolymarketPriceChange {
    /// Market/condition ID.
    pub market: String,
    /// Individual price changes.
    pub price_changes: Vec<PriceChangeEntry>,
    /// Timestamp as a string (milliseconds since epoch).
    #[serde(default)]
    pub timestamp: String,
}

/// A single entry in a price change event.
#[derive(Debug, Deserialize)]
pub struct PriceChangeEntry {
    /// Token/asset ID.
    pub asset_id: String,
    /// New price at this level.
    pub price: String,
    /// New size at this level ("0" means level removed).
    pub size: String,
    /// "BUY" or "SELL".
    pub side: String,
    /// Updated best bid price.
    #[serde(default)]
    pub best_bid: String,
    /// Updated best ask price.
    #[serde(default)]
    pub best_ask: String,
}

/// Parse a raw WebSocket text frame into Polymarket events.
///
/// Handles both JSON array `[{...}, {...}]` and single object `{...}` formats
/// (Pitfall 5 from research). Returns a Vec of events in either case.
pub fn parse_events(text: &str) -> Vec<PolymarketEvent> {
    let trimmed = text.trim();
    if trimmed.starts_with('[') {
        // Array of events
        match serde_json::from_str::<Vec<PolymarketEvent>>(trimmed) {
            Ok(events) => events,
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse Polymarket event array");
                Vec::new()
            }
        }
    } else {
        // Single event
        match serde_json::from_str::<PolymarketEvent>(trimmed) {
            Ok(event) => vec![event],
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse Polymarket event");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_book_event() {
        let json = r#"{
            "event_type": "book",
            "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            "market": "0xabcdef1234567890",
            "hash": "abc123",
            "bids": [
                {"price": "0.55", "size": "100.0"},
                {"price": "0.54", "size": "200.0"}
            ],
            "asks": [
                {"price": "0.56", "size": "150.0"},
                {"price": "0.57", "size": "250.0"}
            ],
            "timestamp": "1703001600000"
        }"#;

        let events = parse_events(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            PolymarketEvent::Book(book) => {
                assert_eq!(book.asset_id, "71321045679252212594626385532706912750332728571942532289631379312455583992563");
                assert_eq!(book.market, "0xabcdef1234567890");
                assert_eq!(book.hash, "abc123");
                assert_eq!(book.bids.len(), 2);
                assert_eq!(book.asks.len(), 2);
                assert_eq!(book.bids[0].price, "0.55");
                assert_eq!(book.bids[0].size, "100.0");
                assert_eq!(book.asks[0].price, "0.56");
                assert_eq!(book.asks[0].size, "150.0");
                assert_eq!(book.timestamp, "1703001600000");
            }
            _ => panic!("expected Book event"),
        }
    }

    #[test]
    fn parse_price_change_event() {
        let json = r#"{
            "event_type": "price_change",
            "market": "0xabcdef1234567890",
            "price_changes": [
                {
                    "asset_id": "token123",
                    "price": "0.60",
                    "size": "50.0",
                    "side": "BUY",
                    "best_bid": "0.59",
                    "best_ask": "0.61"
                }
            ],
            "timestamp": "1703001600100"
        }"#;

        let events = parse_events(json);
        assert_eq!(events.len(), 1);
        match &events[0] {
            PolymarketEvent::PriceChange(pc) => {
                assert_eq!(pc.market, "0xabcdef1234567890");
                assert_eq!(pc.price_changes.len(), 1);
                assert_eq!(pc.price_changes[0].asset_id, "token123");
                assert_eq!(pc.price_changes[0].price, "0.60");
                assert_eq!(pc.price_changes[0].side, "BUY");
                assert_eq!(pc.timestamp, "1703001600100");
            }
            _ => panic!("expected PriceChange event"),
        }
    }

    #[test]
    fn parse_array_of_events() {
        let json = r#"[
            {
                "event_type": "book",
                "asset_id": "token1",
                "market": "market1",
                "hash": "h1",
                "bids": [{"price": "0.50", "size": "10.0"}],
                "asks": [{"price": "0.51", "size": "20.0"}],
                "timestamp": "1703001600000"
            },
            {
                "event_type": "book",
                "asset_id": "token2",
                "market": "market2",
                "hash": "h2",
                "bids": [],
                "asks": [{"price": "0.80", "size": "5.0"}],
                "timestamp": "1703001600100"
            }
        ]"#;

        let events = parse_events(json);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], PolymarketEvent::Book(_)));
        assert!(matches!(&events[1], PolymarketEvent::Book(_)));

        if let PolymarketEvent::Book(book) = &events[1] {
            assert_eq!(book.asset_id, "token2");
            assert!(book.bids.is_empty());
            assert_eq!(book.asks.len(), 1);
        }
    }

    #[test]
    fn parse_unknown_event_type() {
        let json = r#"{"event_type": "some_future_type", "data": 42}"#;
        let events = parse_events(json);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], PolymarketEvent::Unknown));
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        let events = parse_events("not json at all");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_empty_book_event() {
        let json = r#"{
            "event_type": "book",
            "asset_id": "token1",
            "market": "market1",
            "bids": [],
            "asks": [],
            "timestamp": "1703001600000"
        }"#;

        let events = parse_events(json);
        assert_eq!(events.len(), 1);
        if let PolymarketEvent::Book(book) = &events[0] {
            assert!(book.bids.is_empty());
            assert!(book.asks.is_empty());
            assert!(book.hash.is_empty()); // default
        } else {
            panic!("expected Book event");
        }
    }
}
