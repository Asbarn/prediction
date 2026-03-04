//! Derive per-instrument order book state (snapshot-only).
//!
//! Unlike Deribit's `InstrumentBook` which tracks `change_id` sequences and
//! handles delta reconciliation, `DeriveBook` is simpler: every message from
//! the `orderbook.{inst}.{group}.{depth}` channel is a full snapshot that
//! replaces the previous state entirely.
//!
//! Prices and amounts arrive as strings and are parsed to `rust_decimal::Decimal`
//! for precision (no f64 intermediate).

use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::debug;

use super::messages::DeriveBookData;

/// Per-instrument order book for Derive (snapshot-only model).
///
/// Each call to `apply_snapshot` fully replaces bids/asks. There is no
/// delta logic, no `change_id` sequencing -- just `publish_id` tracking
/// for diagnostics.
#[derive(Debug, Clone)]
pub struct DeriveBook {
    pub instrument: String,
    /// Top levels, sorted descending by price (highest first).
    pub bids: Vec<(Decimal, Decimal)>,
    /// Top levels, sorted ascending by price (lowest first).
    pub asks: Vec<(Decimal, Decimal)>,
    /// Last accepted `publish_id` from the API.
    pub last_publish_id: Option<i64>,
    /// Last accepted timestamp (ms) from the API.
    pub last_timestamp: Option<i64>,
    /// Whether this book is considered stale (e.g., connection lost).
    pub is_stale: bool,
}

impl DeriveBook {
    /// Create a new empty book for `instrument`.
    pub fn new(instrument: String) -> Self {
        Self {
            instrument,
            bids: Vec::new(),
            asks: Vec::new(),
            last_publish_id: None,
            last_timestamp: None,
            is_stale: false,
        }
    }

    /// Apply a full snapshot, replacing all book state.
    ///
    /// Parses string prices/amounts to `Decimal`. Invalid pairs are silently
    /// filtered out (logged at debug level). After parsing, bids are sorted
    /// descending and asks ascending by price.
    pub fn apply_snapshot(&mut self, data: &DeriveBookData) {
        self.bids = data
            .bids
            .iter()
            .filter_map(|pair| parse_level(pair))
            .collect();

        self.asks = data
            .asks
            .iter()
            .filter_map(|pair| parse_level(pair))
            .collect();

        // Sort bids descending by price (highest first)
        self.bids.sort_by(|a, b| b.0.cmp(&a.0));

        // Sort asks ascending by price (lowest first)
        self.asks.sort_by(|a, b| a.0.cmp(&b.0));

        self.last_publish_id = Some(data.publish_id);
        self.last_timestamp = Some(data.timestamp);
        self.is_stale = false;
    }

    /// Best bid (highest price).
    pub fn best_bid(&self) -> Option<(Decimal, Decimal)> {
        self.bids.first().copied()
    }

    /// Best ask (lowest price).
    pub fn best_ask(&self) -> Option<(Decimal, Decimal)> {
        self.asks.first().copied()
    }

    /// Mark this book as stale (e.g., before reconnect).
    pub fn mark_stale(&mut self) {
        self.is_stale = true;
    }
}

/// Parse a `[price_str, amount_str]` pair into `(Decimal, Decimal)`.
///
/// Returns `None` if either string fails to parse, logging at debug level.
fn parse_level(pair: &[String; 2]) -> Option<(Decimal, Decimal)> {
    let price = match Decimal::from_str(&pair[0]) {
        Ok(d) => d,
        Err(e) => {
            debug!(price = %pair[0], error = %e, "failed to parse book price");
            return None;
        }
    };
    let amount = match Decimal::from_str(&pair[1]) {
        Ok(d) => d,
        Err(e) => {
            debug!(amount = %pair[1], error = %e, "failed to parse book amount");
            return None;
        }
    };
    Some((price, amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_book_data(
        bids: Vec<[String; 2]>,
        asks: Vec<[String; 2]>,
    ) -> DeriveBookData {
        DeriveBookData {
            timestamp: 1772624842966,
            instrument_name: "BTC-20260305-69500-P".to_string(),
            publish_id: 56593,
            bids,
            asks,
        }
    }

    fn s(val: &str) -> String {
        val.to_string()
    }

    #[test]
    fn apply_snapshot_parses_strings_to_decimal_and_sorts() {
        let mut book = DeriveBook::new("BTC-20260305-69500-P".to_string());
        let data = make_book_data(
            vec![
                [s("320"), s("1")],
                [s("340"), s("0.4")],
                [s("280"), s("0.70343")],
            ],
            vec![[s("520"), s("0.70343")], [s("420"), s("0.4")]],
        );

        book.apply_snapshot(&data);

        // Bids sorted descending
        assert_eq!(book.bids.len(), 3);
        assert_eq!(book.bids[0].0, dec!(340));
        assert_eq!(book.bids[1].0, dec!(320));
        assert_eq!(book.bids[2].0, dec!(280));

        // Asks sorted ascending
        assert_eq!(book.asks.len(), 2);
        assert_eq!(book.asks[0].0, dec!(420));
        assert_eq!(book.asks[1].0, dec!(520));

        // Amounts preserved
        assert_eq!(book.bids[0].1, dec!(0.4));
        assert_eq!(book.asks[1].1, dec!(0.70343));
    }

    #[test]
    fn best_bid_best_ask_correct() {
        let mut book = DeriveBook::new("TEST".to_string());
        let data = make_book_data(
            vec![
                [s("320"), s("1")],
                [s("340"), s("0.4")],
                [s("280"), s("0.7")],
            ],
            vec![[s("520"), s("0.7")], [s("420"), s("0.4")]],
        );
        book.apply_snapshot(&data);

        let (bid_price, bid_amount) = book.best_bid().unwrap();
        assert_eq!(bid_price, dec!(340));
        assert_eq!(bid_amount, dec!(0.4));

        let (ask_price, ask_amount) = book.best_ask().unwrap();
        assert_eq!(ask_price, dec!(420));
        assert_eq!(ask_amount, dec!(0.4));
    }

    #[test]
    fn invalid_price_strings_filtered_gracefully() {
        let mut book = DeriveBook::new("TEST".to_string());
        let data = make_book_data(
            vec![
                [s("340"), s("0.4")],
                [s("not_a_number"), s("1.0")], // invalid price
                [s("320"), s("bad_amount")],    // invalid amount
            ],
            vec![[s("420"), s("0.4")]],
        );

        book.apply_snapshot(&data);

        // Only the valid bid should remain
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.bids[0].0, dec!(340));
        assert_eq!(book.asks.len(), 1);
    }

    #[test]
    fn empty_book_returns_none_for_best() {
        let book = DeriveBook::new("TEST".to_string());
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn successive_snapshots_fully_replace_state() {
        let mut book = DeriveBook::new("TEST".to_string());

        // First snapshot: 3 bid levels
        let data1 = DeriveBookData {
            timestamp: 1000,
            instrument_name: "TEST".to_string(),
            publish_id: 1,
            bids: vec![
                [s("340"), s("0.4")],
                [s("320"), s("1")],
                [s("280"), s("0.7")],
            ],
            asks: vec![[s("420"), s("0.4")], [s("520"), s("0.7")]],
        };
        book.apply_snapshot(&data1);
        assert_eq!(book.bids.len(), 3);
        assert_eq!(book.asks.len(), 2);
        assert_eq!(book.last_publish_id, Some(1));

        // Second snapshot: only 1 bid level (complete replacement)
        let data2 = DeriveBookData {
            timestamp: 2000,
            instrument_name: "TEST".to_string(),
            publish_id: 2,
            bids: vec![[s("350"), s("0.5")]],
            asks: vec![[s("410"), s("0.3")]],
        };
        book.apply_snapshot(&data2);
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
        assert_eq!(book.bids[0].0, dec!(350));
        assert_eq!(book.asks[0].0, dec!(410));
        assert_eq!(book.last_publish_id, Some(2));
        assert_eq!(book.last_timestamp, Some(2000));
    }

    #[test]
    fn mark_stale_sets_flag() {
        let mut book = DeriveBook::new("TEST".to_string());
        assert!(!book.is_stale);
        book.mark_stale();
        assert!(book.is_stale);
    }

    #[test]
    fn apply_snapshot_clears_stale_flag() {
        let mut book = DeriveBook::new("TEST".to_string());
        book.mark_stale();
        assert!(book.is_stale);

        let data = make_book_data(
            vec![[s("340"), s("0.4")]],
            vec![[s("420"), s("0.4")]],
        );
        book.apply_snapshot(&data);
        assert!(!book.is_stale);
    }
}
