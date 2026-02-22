//! Kalshi incremental order book management.
//!
//! Kalshi uses BTreeMap for price levels (ascending sort). Best bid is
//! accessed via `.last()` (Pitfall 3). Asks are derived from the
//! complementary side (Pitfall 2): YES ask = 100 - best NO bid.

use std::collections::BTreeMap;

/// Kalshi order book for a single market.
///
/// Maintains YES and NO bid levels as `BTreeMap<i64, i64>` (price_cents -> quantity).
/// BTreeMap sorts ascending, so best bid is `.last()`.
pub struct KalshiBook {
    /// YES side bids: price_cents -> quantity. Sorted ascending by BTreeMap.
    pub yes_bids: BTreeMap<i64, i64>,
    /// NO side bids: price_cents -> quantity. Sorted ascending by BTreeMap.
    pub no_bids: BTreeMap<i64, i64>,
}

impl KalshiBook {
    /// Create a new empty book.
    pub fn new() -> Self {
        Self {
            yes_bids: BTreeMap::new(),
            no_bids: BTreeMap::new(),
        }
    }

    /// Replace all levels with a full snapshot.
    pub fn apply_snapshot(&mut self, yes: &[[i64; 2]], no: &[[i64; 2]]) {
        self.yes_bids.clear();
        self.no_bids.clear();

        for &[price, qty] in yes {
            if qty > 0 {
                self.yes_bids.insert(price, qty);
            }
        }
        for &[price, qty] in no {
            if qty > 0 {
                self.no_bids.insert(price, qty);
            }
        }
    }

    /// Apply an incremental delta to a single price level.
    ///
    /// Adds `delta` to the entry. Removes the level if result <= 0.
    pub fn apply_delta(&mut self, side: &str, price: i64, delta: i64) {
        let map = match side {
            "yes" => &mut self.yes_bids,
            "no" => &mut self.no_bids,
            _ => {
                tracing::warn!(side = side, "unknown Kalshi order book side");
                return;
            }
        };

        let entry = map.entry(price).or_insert(0);
        *entry += delta;
        if *entry <= 0 {
            map.remove(&price);
        }
    }

    /// Best YES bid (highest price). BTreeMap is ascending, so `.last()`.
    pub fn best_yes_bid(&self) -> Option<(i64, i64)> {
        self.yes_bids.iter().last().map(|(&p, &q)| (p, q))
    }

    /// Best NO bid (highest price).
    pub fn best_no_bid(&self) -> Option<(i64, i64)> {
        self.no_bids.iter().last().map(|(&p, &q)| (p, q))
    }

    /// Derived YES ask from complementary NO bid (Pitfall 2).
    ///
    /// YES ask price = 100 - best NO bid price.
    pub fn best_yes_ask_from_no(&self) -> Option<i64> {
        self.best_no_bid().map(|(price, _)| 100 - price)
    }

    /// Derived NO ask from complementary YES bid.
    ///
    /// NO ask price = 100 - best YES bid price.
    pub fn best_no_ask_from_yes(&self) -> Option<i64> {
        self.best_yes_bid().map(|(price, _)| 100 - price)
    }

    /// All YES bids sorted descending by price (for depth display).
    pub fn yes_depth_descending(&self) -> Vec<(i64, i64)> {
        self.yes_bids.iter().rev().map(|(&p, &q)| (p, q)).collect()
    }

    /// All NO bids sorted descending by price (for depth display).
    pub fn no_depth_descending(&self) -> Vec<(i64, i64)> {
        self.no_bids.iter().rev().map(|(&p, &q)| (p, q)).collect()
    }

    /// Check if the book is empty.
    pub fn is_empty(&self) -> bool {
        self.yes_bids.is_empty() && self.no_bids.is_empty()
    }
}

impl Default for KalshiBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_snapshot_populates_both_sides() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(
            &[[42, 100], [45, 200]],
            &[[55, 150], [58, 300]],
        );

        assert_eq!(book.yes_bids.len(), 2);
        assert_eq!(book.no_bids.len(), 2);
        assert_eq!(book.yes_bids[&42], 100);
        assert_eq!(book.yes_bids[&45], 200);
        assert_eq!(book.no_bids[&55], 150);
        assert_eq!(book.no_bids[&58], 300);
    }

    #[test]
    fn best_yes_bid_is_last_in_btreemap() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 100], [45, 200], [40, 50]], &[]);

        // BTreeMap sorts ascending: 40, 42, 45. Best bid = 45 (last).
        let (price, qty) = book.best_yes_bid().unwrap();
        assert_eq!(price, 45);
        assert_eq!(qty, 200);
    }

    #[test]
    fn best_no_bid_is_last_in_btreemap() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[], &[[55, 150], [58, 300], [50, 100]]);

        let (price, qty) = book.best_no_bid().unwrap();
        assert_eq!(price, 58);
        assert_eq!(qty, 300);
    }

    #[test]
    fn yes_ask_derived_from_no_bid() {
        let mut book = KalshiBook::new();
        // Best NO bid at 58 cents -> YES ask = 100 - 58 = 42 cents
        book.apply_snapshot(&[], &[[55, 150], [58, 300]]);

        assert_eq!(book.best_yes_ask_from_no(), Some(42));
    }

    #[test]
    fn no_ask_derived_from_yes_bid() {
        let mut book = KalshiBook::new();
        // Best YES bid at 45 cents -> NO ask = 100 - 45 = 55 cents
        book.apply_snapshot(&[[42, 100], [45, 200]], &[]);

        assert_eq!(book.best_no_ask_from_yes(), Some(55));
    }

    #[test]
    fn apply_delta_adds_quantity() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 100]], &[]);

        book.apply_delta("yes", 42, 50);
        assert_eq!(book.yes_bids[&42], 150);
    }

    #[test]
    fn apply_delta_creates_new_level() {
        let mut book = KalshiBook::new();
        book.apply_delta("yes", 42, 100);
        assert_eq!(book.yes_bids[&42], 100);
    }

    #[test]
    fn apply_delta_removes_level_when_zero_or_negative() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 100]], &[]);

        book.apply_delta("yes", 42, -100);
        assert!(book.yes_bids.get(&42).is_none());
    }

    #[test]
    fn apply_delta_removes_level_when_negative_result() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 50]], &[]);

        book.apply_delta("yes", 42, -100);
        assert!(book.yes_bids.get(&42).is_none());
    }

    #[test]
    fn snapshot_clears_previous_state() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 100], [45, 200]], &[[55, 150]]);
        assert_eq!(book.yes_bids.len(), 2);

        // New snapshot replaces everything
        book.apply_snapshot(&[[50, 300]], &[]);
        assert_eq!(book.yes_bids.len(), 1);
        assert_eq!(book.no_bids.len(), 0);
        assert_eq!(book.yes_bids[&50], 300);
    }

    #[test]
    fn empty_book_returns_none() {
        let book = KalshiBook::new();
        assert!(book.best_yes_bid().is_none());
        assert!(book.best_no_bid().is_none());
        assert!(book.best_yes_ask_from_no().is_none());
        assert!(book.best_no_ask_from_yes().is_none());
        assert!(book.is_empty());
    }

    #[test]
    fn yes_depth_descending_order() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[40, 50], [42, 100], [45, 200]], &[]);

        let depth = book.yes_depth_descending();
        assert_eq!(depth, vec![(45, 200), (42, 100), (40, 50)]);
    }

    #[test]
    fn snapshot_ignores_zero_quantity() {
        let mut book = KalshiBook::new();
        book.apply_snapshot(&[[42, 0], [45, 200]], &[]);
        assert_eq!(book.yes_bids.len(), 1);
        assert!(book.yes_bids.get(&42).is_none());
    }
}
