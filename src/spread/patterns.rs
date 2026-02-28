//! Spread pattern detection types for cross-venue prediction market arbitrage.
//!
//! Defines the 4 directional spread patterns between Polymarket and Kalshi,
//! the gross spread computation function, and the full SpreadResult struct
//! that captures all computation metadata for JSONL logging and threshold
//! evaluation.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::signal::types::ThresholdStatus;
use crate::types::{MarketSnapshot, Venue};
#[cfg(test)]
use crate::types::Probability;

/// The 4 directional spread patterns between Polymarket and Kalshi.
///
/// Each pattern represents a specific buy/sell direction across two venues.
/// Patterns 3 and 4 are algebraically equivalent to 1 and 2 in gross spread,
/// but produce different net spreads when walk-the-book uses different depth
/// sides (asks vs bids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SpreadPattern {
    /// Buy Polymarket YES (at ask), Sell Kalshi YES (at bid).
    BuyPolyYesSellKalshiYes,
    /// Sell Polymarket YES (at bid), Buy Kalshi YES (at ask).
    SellPolyYesBuyKalshiYes,
    /// Buy Polymarket NO (at ask complement), Sell Kalshi NO (at bid complement).
    /// Complement of pattern 2.
    BuyPolyNoSellKalshiNo,
    /// Sell Polymarket NO (at bid complement), Buy Kalshi NO (at ask complement).
    /// Complement of pattern 1.
    SellPolyNoBuyKalshiNo,
}

impl SpreadPattern {
    /// Returns all 4 spread pattern variants.
    pub fn all() -> [SpreadPattern; 4] {
        [
            SpreadPattern::BuyPolyYesSellKalshiYes,
            SpreadPattern::SellPolyYesBuyKalshiYes,
            SpreadPattern::BuyPolyNoSellKalshiNo,
            SpreadPattern::SellPolyNoBuyKalshiNo,
        ]
    }

    /// Human-readable label for the pattern (used in metrics and logging).
    pub fn label(&self) -> &'static str {
        match self {
            SpreadPattern::BuyPolyYesSellKalshiYes => "buy_poly_yes_sell_kalshi_yes",
            SpreadPattern::SellPolyYesBuyKalshiYes => "sell_poly_yes_buy_kalshi_yes",
            SpreadPattern::BuyPolyNoSellKalshiNo => "buy_poly_no_sell_kalshi_no",
            SpreadPattern::SellPolyNoBuyKalshiNo => "sell_poly_no_buy_kalshi_no",
        }
    }

    /// The venue where we BUY in this pattern.
    pub fn buy_venue(&self) -> Venue {
        match self {
            SpreadPattern::BuyPolyYesSellKalshiYes => Venue::Polymarket,
            SpreadPattern::SellPolyYesBuyKalshiYes => Venue::Kalshi,
            SpreadPattern::BuyPolyNoSellKalshiNo => Venue::Polymarket,
            SpreadPattern::SellPolyNoBuyKalshiNo => Venue::Kalshi,
        }
    }

    /// Canonical label for the venue pair in this pattern (for metric keys).
    ///
    /// Returns a stable `&'static str` regardless of buy/sell direction, so
    /// Kalshi-Polymarket and Polymarket-Kalshi both produce the same key.
    pub fn venue_pair_label(&self) -> &'static str {
        match (self.buy_venue(), self.sell_venue()) {
            (Venue::Kalshi, Venue::Polymarket) | (Venue::Polymarket, Venue::Kalshi) => {
                "kalshi_polymarket"
            }
            (Venue::Deribit, Venue::Polymarket) | (Venue::Polymarket, Venue::Deribit) => {
                "deribit_polymarket"
            }
            (Venue::Deribit, Venue::Kalshi) | (Venue::Kalshi, Venue::Deribit) => "deribit_kalshi",
            _ => "unknown",
        }
    }

    /// The venue where we SELL in this pattern.
    pub fn sell_venue(&self) -> Venue {
        match self {
            SpreadPattern::BuyPolyYesSellKalshiYes => Venue::Kalshi,
            SpreadPattern::SellPolyYesBuyKalshiYes => Venue::Polymarket,
            SpreadPattern::BuyPolyNoSellKalshiNo => Venue::Kalshi,
            SpreadPattern::SellPolyNoBuyKalshiNo => Venue::Polymarket,
        }
    }
}

/// Result of a gross spread computation for a single pattern.
///
/// Captures the raw spread before fee and cost adjustments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrossSpread {
    /// Which directional pattern produced this spread.
    pub pattern: SpreadPattern,
    /// Gross spread in probability space (sell_price - buy_price).
    /// Positive means potentially profitable before costs.
    #[serde(with = "rust_decimal::serde::str")]
    pub gross_spread: Decimal,
    /// The probability price paid on the buy side.
    #[serde(with = "rust_decimal::serde::str")]
    pub buy_price: Decimal,
    /// The probability price received on the sell side.
    #[serde(with = "rust_decimal::serde::str")]
    pub sell_price: Decimal,
    /// Venue where we buy.
    pub buy_venue: Venue,
    /// Venue where we sell.
    pub sell_venue: Venue,
}

/// Compute the gross spread for a given pattern using top-of-book probabilities.
///
/// For YES patterns, uses bid_probability and ask_probability directly.
/// For NO patterns, uses Probability::complement() (1 - p).
///
/// Returns None if either snapshot lacks the required bid_probability or
/// ask_probability fields.
pub fn compute_gross_spread(
    pattern: SpreadPattern,
    poly: &MarketSnapshot,
    kalshi: &MarketSnapshot,
) -> Option<GrossSpread> {
    let poly_bid = poly.bid_probability?;
    let poly_ask = poly.ask_probability?;
    let kalshi_bid = kalshi.bid_probability?;
    let kalshi_ask = kalshi.ask_probability?;

    let (buy_price, sell_price) = match pattern {
        SpreadPattern::BuyPolyYesSellKalshiYes => {
            // Buy Poly at ask, Sell Kalshi at bid
            (poly_ask.into_inner(), kalshi_bid.into_inner())
        }
        SpreadPattern::SellPolyYesBuyKalshiYes => {
            // Buy Kalshi at ask, Sell Poly at bid
            (kalshi_ask.into_inner(), poly_bid.into_inner())
        }
        SpreadPattern::BuyPolyNoSellKalshiNo => {
            // Buy Poly NO (complement of ask) = 1 - poly_ask
            // Sell Kalshi NO (complement of bid) = 1 - kalshi_bid
            let buy = poly_ask.complement().into_inner();
            let sell = kalshi_bid.complement().into_inner();
            (buy, sell)
        }
        SpreadPattern::SellPolyNoBuyKalshiNo => {
            // Buy Kalshi NO (complement of ask) = 1 - kalshi_ask
            // Sell Poly NO (complement of bid) = 1 - poly_bid
            let buy = kalshi_ask.complement().into_inner();
            let sell = poly_bid.complement().into_inner();
            (buy, sell)
        }
    };

    let gross_spread = sell_price - buy_price;

    Some(GrossSpread {
        pattern,
        gross_spread,
        buy_price,
        sell_price,
        buy_venue: pattern.buy_venue(),
        sell_venue: pattern.sell_venue(),
    })
}

/// Full spread computation result with all metadata for JSONL logging
/// and threshold evaluation.
///
/// Produced by the SpreadEngine (Plan 03) after applying the full cost
/// model including walk-the-book, fees, and carry costs.
///
/// ## JSONL Schema (v1.0)
///
/// | Field | JSON Type | Description |
/// |-------|-----------|-------------|
/// | `event_id` | string | Mapped event ID linking both venues |
/// | `pattern` | string | Directional spread pattern enum variant name |
/// | `gross_spread` | string (decimal) | Gross spread before cost adjustments |
/// | `net_spread` | string (decimal) | Net spread after all costs |
/// | `buy_fill_price` | string (decimal) | Average fill price on buy side |
/// | `sell_fill_price` | string (decimal) | Average fill price on sell side |
/// | `buy_fee` | string (decimal) | Fee paid on buy side |
/// | `sell_fee` | string (decimal) | Fee paid on sell side |
/// | `carry_cost` | string (decimal) | Carry cost for holding the position |
/// | `total_cost` | string (decimal) | Total cost (buy_fee + sell_fee + carry_cost + basis_risk_premium) |
/// | `basis_risk_premium` | string (decimal) | Settlement basis risk premium from BasisRiskCache |
/// | `buy_fill_ratio` | string (decimal) | Ratio of filled vs target on buy side |
/// | `sell_fill_ratio` | string (decimal) | Ratio of filled vs target on sell side |
/// | `target_notional` | string (decimal) | Target notional for walk-the-book |
/// | `timestamp_ms` | integer | Local timestamp in milliseconds |
/// | `poly_exchange_ts` | integer\|null | Polymarket exchange timestamp |
/// | `kalshi_exchange_ts` | integer\|null | Kalshi exchange timestamp |
/// | `threshold` | string (decimal)\|null | Dynamic threshold at computation time |
/// | `threshold_components` | object\|null | Breakdown of threshold factors |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadResult {
    /// The mapped event ID linking both venues.
    pub event_id: String,
    /// Which directional pattern produced this result.
    pub pattern: SpreadPattern,
    /// Gross spread before any cost adjustments.
    #[serde(with = "rust_decimal::serde::str")]
    pub gross_spread: Decimal,
    /// Net spread after all costs (fees, carry, slippage).
    #[serde(with = "rust_decimal::serde::str")]
    pub net_spread: Decimal,
    /// Average fill price on the buy side (after walk-the-book).
    #[serde(with = "rust_decimal::serde::str")]
    pub buy_fill_price: Decimal,
    /// Average fill price on the sell side (after walk-the-book).
    #[serde(with = "rust_decimal::serde::str")]
    pub sell_fill_price: Decimal,
    /// Fee paid on the buy side.
    #[serde(with = "rust_decimal::serde::str")]
    pub buy_fee: Decimal,
    /// Fee paid on the sell side.
    #[serde(with = "rust_decimal::serde::str")]
    pub sell_fee: Decimal,
    /// Carry cost for holding the position.
    #[serde(with = "rust_decimal::serde::str")]
    pub carry_cost: Decimal,
    /// Total cost (buy_fee + sell_fee + carry_cost + basis_risk_premium).
    #[serde(with = "rust_decimal::serde::str")]
    pub total_cost: Decimal,
    /// Settlement basis risk premium (from BasisRiskCache).
    /// Zero if no risk data available for this event.
    #[serde(default)]
    #[serde(with = "rust_decimal::serde::str")]
    pub basis_risk_premium: Decimal,
    /// Ratio of filled vs target notional on buy side (1.0 = full fill).
    #[serde(with = "rust_decimal::serde::str")]
    pub buy_fill_ratio: Decimal,
    /// Ratio of filled vs target notional on sell side (1.0 = full fill).
    #[serde(with = "rust_decimal::serde::str")]
    pub sell_fill_ratio: Decimal,
    /// Target notional for walk-the-book.
    #[serde(with = "rust_decimal::serde::str")]
    pub target_notional: Decimal,
    /// Local timestamp in milliseconds when this computation occurred.
    pub timestamp_ms: i64,
    /// Exchange timestamp from Polymarket snapshot (if available).
    pub poly_exchange_ts: Option<i64>,
    /// Exchange timestamp from Kalshi snapshot (if available).
    pub kalshi_exchange_ts: Option<i64>,
    /// Dynamic threshold at the time of computation (if available).
    pub threshold: Option<Decimal>,
    /// Breakdown of threshold components for post-hoc analysis.
    pub threshold_components: Option<ThresholdComponents>,
    /// Threshold evaluation status for this spread result.
    #[serde(default)]
    pub threshold_status: Option<ThresholdStatus>,
}

/// Breakdown of threshold components for observability.
///
/// Logs each factor in the threshold formula:
/// max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty
///
/// This allows post-hoc analysis of which factor drives useful signals.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ThresholdComponents {
    /// Static minimum threshold.
    #[serde(with = "rust_decimal::serde::str")]
    pub static_floor: Decimal,
    /// Rolling mean of net spreads in the window.
    pub rolling_mean: f64,
    /// Rolling standard deviation of net spreads in the window.
    pub rolling_stddev: f64,
    /// k multiplier applied to stddev.
    pub k_sigma: f64,
    /// Penalty added for thin order books.
    #[serde(with = "rust_decimal::serde::str")]
    pub liquidity_penalty: Decimal,
    /// Final computed threshold.
    #[serde(with = "rust_decimal::serde::str")]
    pub final_threshold: Decimal,
    /// Whether the system is in cold-start mode (insufficient samples).
    pub is_cold_start: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DualTimestamp, InstrumentId, TraceId};
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn prob(s: &str) -> Probability {
        Probability::new(dec(s)).unwrap()
    }

    /// Create a minimal MarketSnapshot with probabilities set.
    fn make_snapshot(
        venue: Venue,
        bid_prob: Option<&str>,
        ask_prob: Option<&str>,
    ) -> MarketSnapshot {
        MarketSnapshot {
            venue,
            instrument_id: InstrumentId::new("TEST-INSTRUMENT"),
            event_id: None,
            bid: None,
            ask: None,
            bid_size: None,
            ask_size: None,
            depth_bids: vec![],
            depth_asks: vec![],
            bid_probability: bid_prob.map(prob),
            ask_probability: ask_prob.map(prob),
            last_price: None,
            mark_price: None,
            index_price: None,
            mark_iv: None,
            open_interest: None,
            volume_24h: None,
            greeks: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            underlying_index: None,
            exchange_timestamp: None,
            timestamp: DualTimestamp::now(),
            sequence: 1,
            trace_id: TraceId::new(),
            is_stale: false,
        }
    }

    // ---- Pattern basics ----

    #[test]
    fn all_returns_four_patterns() {
        assert_eq!(SpreadPattern::all().len(), 4);
    }

    #[test]
    fn pattern_labels_are_distinct() {
        let labels: Vec<&str> = SpreadPattern::all().iter().map(|p| p.label()).collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(labels.len(), unique.len(), "Labels must be distinct");
    }

    #[test]
    fn pattern_buy_sell_venues_are_correct() {
        let p1 = SpreadPattern::BuyPolyYesSellKalshiYes;
        assert_eq!(p1.buy_venue(), Venue::Polymarket);
        assert_eq!(p1.sell_venue(), Venue::Kalshi);

        let p2 = SpreadPattern::SellPolyYesBuyKalshiYes;
        assert_eq!(p2.buy_venue(), Venue::Kalshi);
        assert_eq!(p2.sell_venue(), Venue::Polymarket);

        let p3 = SpreadPattern::BuyPolyNoSellKalshiNo;
        assert_eq!(p3.buy_venue(), Venue::Polymarket);
        assert_eq!(p3.sell_venue(), Venue::Kalshi);

        let p4 = SpreadPattern::SellPolyNoBuyKalshiNo;
        assert_eq!(p4.buy_venue(), Venue::Kalshi);
        assert_eq!(p4.sell_venue(), Venue::Polymarket);
    }

    // ---- Gross spread computation ----

    #[test]
    fn pattern1_buy_poly_yes_sell_kalshi_yes() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::BuyPolyYesSellKalshiYes,
            &poly,
            &kalshi,
        )
        .unwrap();

        assert_eq!(result.gross_spread, dec("0.05"));
        assert_eq!(result.buy_price, dec("0.45"));
        assert_eq!(result.sell_price, dec("0.50"));
        assert_eq!(result.buy_venue, Venue::Polymarket);
        assert_eq!(result.sell_venue, Venue::Kalshi);
    }

    #[test]
    fn pattern2_sell_poly_yes_buy_kalshi_yes() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.56"), Some("0.59"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::SellPolyYesBuyKalshiYes,
            &poly,
            &kalshi,
        )
        .unwrap();

        assert_eq!(result.gross_spread, dec("0.03"));
        assert_eq!(result.buy_price, dec("0.53"));
        assert_eq!(result.sell_price, dec("0.56"));
    }

    #[test]
    fn pattern3_buy_poly_no_sell_kalshi_no() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::BuyPolyNoSellKalshiNo,
            &poly,
            &kalshi,
        )
        .unwrap();

        assert_eq!(result.buy_price, dec("0.55"));
        assert_eq!(result.sell_price, dec("0.50"));
        assert_eq!(result.gross_spread, dec("-0.05"));
    }

    #[test]
    fn pattern4_sell_poly_no_buy_kalshi_no() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::SellPolyNoBuyKalshiNo,
            &poly,
            &kalshi,
        )
        .unwrap();

        assert_eq!(result.buy_price, dec("0.47"));
        assert_eq!(result.sell_price, dec("0.58"));
        assert_eq!(result.gross_spread, dec("0.11"));
    }

    #[test]
    fn patterns_1_and_3_gross_algebraic_relationship() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let p1 = compute_gross_spread(SpreadPattern::BuyPolyYesSellKalshiYes, &poly, &kalshi).unwrap();
        let p3 = compute_gross_spread(SpreadPattern::BuyPolyNoSellKalshiNo, &poly, &kalshi).unwrap();

        assert_eq!(p1.gross_spread, -p3.gross_spread);
    }

    #[test]
    fn patterns_2_and_4_gross_algebraic_relationship() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let p2 = compute_gross_spread(SpreadPattern::SellPolyYesBuyKalshiYes, &poly, &kalshi).unwrap();
        let p4 = compute_gross_spread(SpreadPattern::SellPolyNoBuyKalshiNo, &poly, &kalshi).unwrap();

        assert_eq!(p2.gross_spread, -p4.gross_spread);
    }

    #[test]
    fn returns_none_when_poly_probabilities_missing() {
        let poly = make_snapshot(Venue::Polymarket, None, None);
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.50"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::BuyPolyYesSellKalshiYes,
            &poly,
            &kalshi,
        );
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_kalshi_probabilities_missing() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.42"), Some("0.45"));
        let kalshi = make_snapshot(Venue::Kalshi, None, None);

        let result = compute_gross_spread(
            SpreadPattern::BuyPolyYesSellKalshiYes,
            &poly,
            &kalshi,
        );
        assert!(result.is_none());
    }

    #[test]
    fn negative_spread_when_no_arbitrage() {
        let poly = make_snapshot(Venue::Polymarket, Some("0.48"), Some("0.52"));
        let kalshi = make_snapshot(Venue::Kalshi, Some("0.49"), Some("0.53"));

        let result = compute_gross_spread(
            SpreadPattern::BuyPolyYesSellKalshiYes,
            &poly,
            &kalshi,
        )
        .unwrap();

        assert!(result.gross_spread < Decimal::ZERO);
    }

    // ---- SpreadResult serialization ----

    #[test]
    fn spread_result_serializes_to_json() {
        let result = SpreadResult {
            event_id: "test-event-123".to_string(),
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
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-event-123"));
        assert!(json.contains("BuyPolyYesSellKalshiYes"));
        assert!(json.contains("threshold_components"));
        assert!(json.contains("is_cold_start"));

        // Verify the JSON is valid
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn spread_result_without_threshold_serializes() {
        let result = SpreadResult {
            event_id: "minimal-event".to_string(),
            pattern: SpreadPattern::SellPolyYesBuyKalshiYes,
            gross_spread: dec("0.02"),
            net_spread: dec("0.01"),
            buy_fill_price: dec("0.53"),
            sell_fill_price: dec("0.56"),
            buy_fee: dec("0.003"),
            sell_fee: dec("0.004"),
            carry_cost: dec("0.001"),
            total_cost: dec("0.008"),
            basis_risk_premium: dec("0"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("1.0"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: None,
            kalshi_exchange_ts: None,
            threshold: None,
            threshold_components: None,
            threshold_status: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"threshold\":null"));
        assert!(json.contains("\"threshold_components\":null"));
    }
}
