use rust_decimal::Decimal;

use crate::types::{Notional, Price};

/// Result of walking an order book to fill a target notional.
#[derive(Debug, Clone)]
pub struct WalkResult {
    /// Weighted average fill price across consumed levels.
    pub avg_fill_price: Decimal,
    /// Total notional actually filled (may be < target if depth insufficient).
    pub filled_notional: Decimal,
    /// The original target notional requested.
    pub target_notional: Decimal,
    /// Number of depth levels consumed.
    pub levels_consumed: usize,
}

impl WalkResult {
    /// Fill ratio: filled / target. Returns 1.0 if fully filled, < 1.0 if
    /// depth was insufficient.
    pub fn fill_ratio(&self) -> Decimal {
        if self.target_notional.is_zero() {
            return Decimal::ONE;
        }
        self.filled_notional / self.target_notional
    }
}

/// Walk order book depth to compute average fill price for a target notional.
///
/// Iterates through depth levels (best to worst), accumulating fill quantity
/// and weighted cost. If depth is insufficient, returns partial fill with
/// the actual filled amount.
///
/// Returns a `WalkResult` with average fill price, filled notional, and
/// fill ratio for liquidity assessment.
pub fn walk_the_book(depth: &[(Price, Notional)], target_notional: Decimal) -> WalkResult {
    if target_notional.is_zero() {
        return WalkResult {
            avg_fill_price: Decimal::ZERO,
            filled_notional: Decimal::ZERO,
            target_notional,
            levels_consumed: 0,
        };
    }

    let mut remaining = target_notional;
    let mut total_cost = Decimal::ZERO;
    let mut total_filled = Decimal::ZERO;
    let mut levels = 0;

    for &(price, size) in depth {
        if remaining <= Decimal::ZERO {
            break;
        }
        let fill_at_level = remaining.min(size.into_inner());
        total_cost += fill_at_level * price.into_inner();
        total_filled += fill_at_level;
        remaining -= fill_at_level;
        levels += 1;
    }

    let avg_fill_price = if total_filled > Decimal::ZERO {
        total_cost / total_filled
    } else {
        Decimal::ZERO
    };

    WalkResult {
        avg_fill_price,
        filled_notional: total_filled,
        target_notional,
        levels_consumed: levels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn price(v: Decimal) -> Price {
        Price::new(v)
    }

    fn notional(v: Decimal) -> Notional {
        Notional::new(v)
    }

    #[test]
    fn full_fill_across_multiple_levels() {
        let depth = vec![
            (price(dec("10")), notional(dec("100"))),
            (price(dec("11")), notional(dec("100"))),
            (price(dec("12")), notional(dec("100"))),
        ];
        let result = walk_the_book(&depth, dec("250"));
        // Fill 100@10 + 100@11 + 50@12 = 1000+1100+600 = 2700 / 250 = 10.8
        assert_eq!(result.filled_notional, dec("250"));
        assert_eq!(result.avg_fill_price, dec("2700") / dec("250"));
        assert_eq!(result.fill_ratio(), Decimal::ONE);
        assert_eq!(result.levels_consumed, 3);
    }

    #[test]
    fn partial_fill_insufficient_depth() {
        let depth = vec![
            (price(dec("10")), notional(dec("50"))),
            (price(dec("11")), notional(dec("50"))),
        ];
        let result = walk_the_book(&depth, dec("200"));
        // Only 100 filled out of 200 target
        assert_eq!(result.filled_notional, dec("100"));
        assert_eq!(result.fill_ratio(), dec("0.5"));
        assert_eq!(result.levels_consumed, 2);
    }

    #[test]
    fn single_level_exact_fill() {
        let depth = vec![(price(dec("42")), notional(dec("100")))];
        let result = walk_the_book(&depth, dec("100"));
        assert_eq!(result.filled_notional, dec("100"));
        assert_eq!(result.avg_fill_price, dec("42"));
        assert_eq!(result.fill_ratio(), Decimal::ONE);
        assert_eq!(result.levels_consumed, 1);
    }

    #[test]
    fn empty_depth_returns_zero() {
        let depth: Vec<(Price, Notional)> = vec![];
        let result = walk_the_book(&depth, dec("100"));
        assert_eq!(result.filled_notional, Decimal::ZERO);
        assert_eq!(result.avg_fill_price, Decimal::ZERO);
        assert_eq!(result.levels_consumed, 0);
    }

    #[test]
    fn zero_target_returns_zero() {
        let depth = vec![(price(dec("10")), notional(dec("100")))];
        let result = walk_the_book(&depth, Decimal::ZERO);
        assert_eq!(result.filled_notional, Decimal::ZERO);
        assert_eq!(result.avg_fill_price, Decimal::ZERO);
        assert_eq!(result.fill_ratio(), Decimal::ONE); // 0/0 treated as full
        assert_eq!(result.levels_consumed, 0);
    }
}
