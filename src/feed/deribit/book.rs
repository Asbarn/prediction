//! Per-instrument order book state with change_id sequence verification.
//!
//! Each `InstrumentBook` replaces its full depth on every grouped snapshot
//! from the Deribit `book.{instrument}.none.20.100ms` channel. The `change_id`
//! field ensures continuity; a gap marks the instrument as stale.

use rust_decimal::Decimal;

use crate::feed::deribit::messages::BookData;
use crate::types::{DualTimestamp, InstrumentId, Notional, Price};

/// Sequence gap error for change_id verification.
#[derive(Debug, thiserror::Error)]
pub enum SequenceError {
    #[error("change_id sequence gap: expected {expected}, got {got}")]
    Gap { expected: i64, got: i64 },
}

/// Per-instrument order book.
///
/// Maintains the top-N bid and ask levels, verifying that consecutive
/// snapshots have contiguous `change_id` values. On a gap the book is
/// marked stale so downstream consumers can discard it until re-sync.
#[derive(Debug, Clone)]
pub struct InstrumentBook {
    pub instrument: InstrumentId,
    /// Top levels, sorted descending by price.
    pub bids: Vec<(Price, Notional)>,
    /// Top levels, sorted ascending by price.
    pub asks: Vec<(Price, Notional)>,
    /// Last accepted change_id. `None` before first message.
    pub last_change_id: Option<i64>,
    /// Timestamp of the last accepted snapshot.
    pub timestamp: Option<DualTimestamp>,
    /// Whether this book is considered stale (sequence gap detected).
    pub is_stale: bool,
}

impl InstrumentBook {
    /// Create a new empty book for `instrument`.
    pub fn new(instrument: InstrumentId) -> Self {
        Self {
            instrument,
            bids: Vec::new(),
            asks: Vec::new(),
            last_change_id: None,
            timestamp: None,
            is_stale: false,
        }
    }

    /// Apply a grouped book snapshot.
    ///
    /// - First message (`last_change_id` is `None`): accepted unconditionally.
    /// - Subsequent messages: `data.prev_change_id` must equal `self.last_change_id`.
    ///   On mismatch the book is marked stale and `Err(SequenceError::Gap)` is returned.
    /// - On success: bids/asks are fully replaced, sorted, and `is_stale` is cleared.
    pub fn apply_snapshot(
        &mut self,
        data: &BookData,
        received_at: DualTimestamp,
    ) -> Result<(), SequenceError> {
        // Sequence verification (skip for first message)
        if let Some(last_id) = self.last_change_id {
            let prev = data.prev_change_id.unwrap_or(-1);
            if prev != last_id {
                self.is_stale = true;
                return Err(SequenceError::Gap {
                    expected: last_id,
                    got: prev,
                });
            }
        }

        // Convert f64 pairs to (Price, Notional)
        self.bids = data
            .bids
            .iter()
            .map(|level| f64_pair_to_level(level))
            .collect();

        self.asks = data
            .asks
            .iter()
            .map(|level| f64_pair_to_level(level))
            .collect();

        // Sort bids descending by price
        self.bids
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Sort asks ascending by price
        self.asks
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        self.last_change_id = Some(data.change_id);
        self.timestamp = Some(received_at);
        self.is_stale = false;

        Ok(())
    }

    /// Mark this book as stale (e.g. before a re-subscribe).
    pub fn mark_stale(&mut self) {
        self.is_stale = true;
    }

    /// Best bid (highest price level).
    pub fn best_bid(&self) -> Option<(Price, Notional)> {
        self.bids.first().copied()
    }

    /// Best ask (lowest price level).
    pub fn best_ask(&self) -> Option<(Price, Notional)> {
        self.asks.first().copied()
    }
}

/// Convert an `[f64; 2]` price/size pair to `(Price, Notional)`.
///
/// Uses `Decimal::from_f64_retain` which never fails (unlike `try_from`).
fn f64_pair_to_level(pair: &[f64; 2]) -> (Price, Notional) {
    let price = Decimal::from_f64_retain(pair[0]).unwrap_or(Decimal::ZERO);
    let size = Decimal::from_f64_retain(pair[1]).unwrap_or(Decimal::ZERO);
    (Price::new(price), Notional::new(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_book_data(
        change_id: i64,
        prev_change_id: Option<i64>,
        bids: Vec<[f64; 2]>,
        asks: Vec<[f64; 2]>,
    ) -> BookData {
        BookData {
            timestamp: 1703001600000,
            instrument_name: "BTC-27JUN25-100000-C".to_string(),
            change_id,
            prev_change_id,
            update_type: Some("snapshot".to_string()),
            bids,
            asks,
        }
    }

    fn ts() -> DualTimestamp {
        DualTimestamp::now()
    }

    #[test]
    fn first_message_accepted_without_prev_check() {
        let mut book = InstrumentBook::new(InstrumentId::new("BTC-27JUN25-100000-C"));
        let data = make_book_data(
            100,
            None,
            vec![[0.0055, 10.0], [0.0050, 25.0]],
            vec![[0.0060, 8.0], [0.0065, 12.0]],
        );

        let result = book.apply_snapshot(&data, ts());
        assert!(result.is_ok());
        assert_eq!(book.last_change_id, Some(100));
        assert!(!book.is_stale);
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks.len(), 2);
    }

    #[test]
    fn sequential_messages_accepted() {
        let mut book = InstrumentBook::new(InstrumentId::new("BTC-27JUN25-100000-C"));

        // First message
        let data1 = make_book_data(100, None, vec![[0.0055, 10.0]], vec![[0.0060, 8.0]]);
        book.apply_snapshot(&data1, ts()).unwrap();

        // Second message with matching prev_change_id
        let data2 = make_book_data(
            101,
            Some(100),
            vec![[0.0056, 11.0]],
            vec![[0.0059, 9.0]],
        );
        let result = book.apply_snapshot(&data2, ts());
        assert!(result.is_ok());
        assert_eq!(book.last_change_id, Some(101));
        assert!(!book.is_stale);
    }

    #[test]
    fn sequence_gap_returns_error_and_marks_stale() {
        let mut book = InstrumentBook::new(InstrumentId::new("BTC-27JUN25-100000-C"));

        // First message
        let data1 = make_book_data(100, None, vec![[0.0055, 10.0]], vec![[0.0060, 8.0]]);
        book.apply_snapshot(&data1, ts()).unwrap();

        // Message with wrong prev_change_id (gap)
        let data2 = make_book_data(
            105,
            Some(103), // expected 100, got 103
            vec![[0.0056, 11.0]],
            vec![[0.0059, 9.0]],
        );
        let result = book.apply_snapshot(&data2, ts());
        assert!(result.is_err());
        assert!(book.is_stale);

        // Original data should NOT be replaced on error
        assert_eq!(book.last_change_id, Some(100));
    }

    #[test]
    fn bids_sorted_descending_asks_sorted_ascending() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));

        // Provide bids in random order
        let data = make_book_data(
            1,
            None,
            vec![[0.0050, 5.0], [0.0055, 10.0], [0.0045, 15.0]],
            vec![[0.0070, 20.0], [0.0060, 8.0], [0.0065, 12.0]],
        );
        book.apply_snapshot(&data, ts()).unwrap();

        // Bids: highest price first
        let bid_prices: Vec<Decimal> =
            book.bids.iter().map(|(p, _)| p.into_inner()).collect();
        assert!(bid_prices[0] > bid_prices[1]);
        assert!(bid_prices[1] > bid_prices[2]);

        // Asks: lowest price first
        let ask_prices: Vec<Decimal> =
            book.asks.iter().map(|(p, _)| p.into_inner()).collect();
        assert!(ask_prices[0] < ask_prices[1]);
        assert!(ask_prices[1] < ask_prices[2]);
    }

    #[test]
    fn best_bid_best_ask_correct() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));

        let data = make_book_data(
            1,
            None,
            vec![[0.0050, 5.0], [0.0055, 10.0], [0.0045, 15.0]],
            vec![[0.0070, 20.0], [0.0060, 8.0], [0.0065, 12.0]],
        );
        book.apply_snapshot(&data, ts()).unwrap();

        let (best_bid_price, best_bid_size) = book.best_bid().unwrap();
        assert_eq!(
            best_bid_price.into_inner(),
            Decimal::from_f64_retain(0.0055).unwrap()
        );
        assert_eq!(
            best_bid_size.into_inner(),
            Decimal::from_f64_retain(10.0).unwrap()
        );

        let (best_ask_price, best_ask_size) = book.best_ask().unwrap();
        assert_eq!(
            best_ask_price.into_inner(),
            Decimal::from_f64_retain(0.0060).unwrap()
        );
        assert_eq!(
            best_ask_size.into_inner(),
            Decimal::from_f64_retain(8.0).unwrap()
        );
    }

    #[test]
    fn mark_stale_sets_flag() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));
        assert!(!book.is_stale);
        book.mark_stale();
        assert!(book.is_stale);
    }

    #[test]
    fn empty_book_returns_none_for_best() {
        let book = InstrumentBook::new(InstrumentId::new("TEST"));
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn snapshot_replaces_previous_state() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));

        // First snapshot: 3 levels
        let data1 = make_book_data(
            1,
            None,
            vec![[0.0050, 5.0], [0.0055, 10.0], [0.0045, 15.0]],
            vec![[0.0060, 8.0], [0.0065, 12.0], [0.0070, 20.0]],
        );
        book.apply_snapshot(&data1, ts()).unwrap();
        assert_eq!(book.bids.len(), 3);
        assert_eq!(book.asks.len(), 3);

        // Second snapshot: only 1 level (complete replacement)
        let data2 = make_book_data(
            2,
            Some(1),
            vec![[0.0056, 11.0]],
            vec![[0.0059, 9.0]],
        );
        book.apply_snapshot(&data2, ts()).unwrap();
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
    }

    #[test]
    fn stale_cleared_on_successful_apply() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));
        book.mark_stale();
        assert!(book.is_stale);

        let data = make_book_data(1, None, vec![[0.0055, 10.0]], vec![[0.0060, 8.0]]);
        book.apply_snapshot(&data, ts()).unwrap();
        assert!(!book.is_stale);
    }
}
