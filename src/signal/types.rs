//! Cross-asset arbitrage signal types.
//!
//! Defines the `ArbSignal` struct and supporting types that carry all metadata
//! for a detected cross-asset arbitrage opportunity between prediction markets
//! and options-implied probabilities.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::pricing::types::{ConfidenceComponents, PricingMethod, SolverResult};
use crate::spread::patterns::ThresholdComponents;
use crate::types::{DualTimestamp, Venue};

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

/// Direction of a cross-asset arbitrage signal.
///
/// Indicates which side of the trade to take based on the relative pricing
/// between prediction markets and options-implied probabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbDirection {
    /// Prediction price < options-implied probability: buy prediction, sell options.
    BuyPredictionSellOptions,
    /// Prediction price > options-implied probability: sell prediction, buy options.
    SellPredictionBuyOptions,
}

// ---------------------------------------------------------------------------
// Threshold status
// ---------------------------------------------------------------------------

/// Threshold evaluation status for a signal.
///
/// All signals are logged with this status field for Phase 9 threshold
/// effectiveness analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThresholdStatus {
    /// Signal passed both static and dynamic thresholds.
    PassedBoth,
    /// Signal passed the static floor but not the dynamic threshold.
    PassedStaticOnly,
    /// Signal was below the static floor (filtered).
    Filtered,
}

// ---------------------------------------------------------------------------
// Cost breakdown
// ---------------------------------------------------------------------------

/// Detailed breakdown of all costs associated with executing an arb signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    /// Fee on the prediction market side.
    #[serde(with = "rust_decimal::serde::str")]
    pub prediction_fee: Decimal,
    /// Estimated options fee (Deribit taker fee).
    #[serde(with = "rust_decimal::serde::str")]
    pub options_fee_estimate: Decimal,
    /// Carry cost for the holding period.
    #[serde(with = "rust_decimal::serde::str")]
    pub carry_cost: Decimal,
    /// Slippage estimate on the prediction market side.
    #[serde(with = "rust_decimal::serde::str")]
    pub prediction_slippage: Decimal,
    /// Cost from options bid-ask spread.
    #[serde(with = "rust_decimal::serde::str")]
    pub options_spread_cost: Decimal,
    /// Liquidity adjustment factor (0.0-1.0 penalty for thin books).
    #[serde(with = "rust_decimal::serde::str")]
    pub liquidity_factor: Decimal,
    /// Total cost = sum of all components.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_cost: Decimal,
}

// ---------------------------------------------------------------------------
// Leg info
// ---------------------------------------------------------------------------

/// Information about one leg of a cross-asset arb trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegInfo {
    /// Venue for this leg.
    pub venue: Venue,
    /// Instrument identifier on this venue.
    pub instrument_id: String,
    /// Probability at this leg (mid or executable).
    #[serde(with = "rust_decimal::serde::str")]
    pub probability: Decimal,
    /// Executable price after walk-the-book or best available.
    #[serde(with = "rust_decimal::serde::str")]
    pub executable_price: Decimal,
    /// Number of order book levels available.
    pub book_depth_levels: usize,
    /// Ratio of fillable notional vs target (0.0-1.0).
    #[serde(with = "rust_decimal::serde::str")]
    pub fill_ratio: Decimal,
}

// ---------------------------------------------------------------------------
// ArbSignal
// ---------------------------------------------------------------------------

/// A detected cross-asset arbitrage signal.
///
/// Carries all required fields (per SGNL-05) plus rich metadata for logging,
/// threshold analysis, and downstream consumption by the execution layer.
///
/// ## JSONL Schema (v1.0)
///
/// | Field | JSON Type | Description |
/// |-------|-----------|-------------|
/// | `signal_id` | string | UUID v7 signal identifier |
/// | `event_id` | string | Mapped event ID linking prediction and options |
/// | `direction` | string | "BuyPredictionSellOptions" or "SellPredictionBuyOptions" |
/// | `raw_spread` | string (decimal) | Raw spread before cost adjustments |
/// | `net_edge` | string (decimal) | Net edge after all costs |
/// | `confidence` | number (f64) | Composite confidence score (0.0-1.0) |
/// | `prediction_leg` | object | Prediction market leg details |
/// | `options_leg` | object | Options market leg details |
/// | `timestamp` | string (ISO 8601) | Wall-clock timestamp |
/// | `ttl_secs` | integer | Time-to-live in seconds |
/// | `pricing_method` | string | Probability extraction method |
/// | `confidence_components` | object | Individual confidence scores |
/// | `solver_meta` | object\|null | IV solver metadata |
/// | `iv_spread` | number (f64) | IV bid-ask spread |
/// | `skew_adjustment` | number (f64) | Skew adjustment magnitude |
/// | `cost_breakdown` | object | Full cost breakdown |
/// | `prediction_venue` | string | Prediction venue name |
/// | `threshold_status` | string | Threshold evaluation result |
/// | `threshold_value` | string (decimal) | Threshold value used |
/// | `threshold_components` | object\|null | Threshold component breakdown |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbSignal {
    // -- Required fields (SGNL-05) --
    /// Unique signal identifier (UUID v7, time-ordered).
    pub signal_id: String,
    /// The mapped event ID linking prediction and options markets.
    pub event_id: String,
    /// Trade direction.
    pub direction: ArbDirection,
    /// Raw spread before cost adjustments (probability space).
    #[serde(with = "rust_decimal::serde::str")]
    pub raw_spread: Decimal,
    /// Net edge after all costs.
    #[serde(with = "rust_decimal::serde::str")]
    pub net_edge: Decimal,
    /// Composite confidence score (0.0-1.0).
    pub confidence: f64,
    /// Prediction market leg details.
    pub prediction_leg: LegInfo,
    /// Options market leg details.
    pub options_leg: LegInfo,
    /// Dual timestamp (wall + monotonic).
    pub timestamp: DualTimestamp,
    /// Time-to-live in seconds before this signal is stale.
    pub ttl_secs: u64,

    // -- Rich metadata --
    /// Probability extraction method used for the options leg.
    pub pricing_method: PricingMethod,
    /// Individual confidence component scores.
    pub confidence_components: ConfidenceComponents,
    /// IV solver metadata (if IV solve was performed).
    pub solver_meta: Option<SolverResult>,
    /// IV bid-ask spread (options market).
    pub iv_spread: f64,
    /// Skew adjustment magnitude (strike IV - ATM IV).
    pub skew_adjustment: f64,
    /// Full cost breakdown.
    pub cost_breakdown: CostBreakdown,
    /// Which prediction venue is being used.
    pub prediction_venue: Venue,
    /// Threshold evaluation result.
    pub threshold_status: ThresholdStatus,
    /// Threshold value used for evaluation.
    #[serde(with = "rust_decimal::serde::str")]
    pub threshold_value: Decimal,
    /// Threshold component breakdown (if dynamic threshold was computed).
    pub threshold_components: Option<ThresholdComponents>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{ConfidenceComponents, PricingMethod};
    use crate::types::DualTimestamp;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_leg(venue: Venue) -> LegInfo {
        LegInfo {
            venue,
            instrument_id: "TEST-INST".to_string(),
            probability: dec("0.55"),
            executable_price: dec("0.54"),
            book_depth_levels: 5,
            fill_ratio: dec("0.95"),
        }
    }

    fn make_cost_breakdown() -> CostBreakdown {
        CostBreakdown {
            prediction_fee: dec("0.005"),
            options_fee_estimate: dec("0.0003"),
            carry_cost: dec("0.002"),
            prediction_slippage: dec("0.001"),
            options_spread_cost: dec("0.003"),
            liquidity_factor: dec("0.95"),
            total_cost: dec("0.0113"),
        }
    }

    fn make_signal() -> ArbSignal {
        ArbSignal {
            signal_id: uuid::Uuid::now_v7().to_string(),
            event_id: "evt-btc-100k-2026".to_string(),
            direction: ArbDirection::BuyPredictionSellOptions,
            raw_spread: dec("0.05"),
            net_edge: dec("0.038"),
            confidence: 0.82,
            prediction_leg: make_leg(Venue::Polymarket),
            options_leg: make_leg(Venue::Deribit),
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
            cost_breakdown: make_cost_breakdown(),
            prediction_venue: Venue::Polymarket,
            threshold_status: ThresholdStatus::PassedBoth,
            threshold_value: dec("0.025"),
            threshold_components: None,
        }
    }

    #[test]
    fn arb_signal_serializes_to_json_and_back() {
        let signal = make_signal();
        let json = serde_json::to_string(&signal).unwrap();

        // Verify key fields are present in the JSON
        assert!(json.contains("evt-btc-100k-2026"));
        assert!(json.contains("BuyPredictionSellOptions"));
        assert!(json.contains("CallSpreadReplication"));
        assert!(json.contains("PassedBoth"));

        // Roundtrip: deserialize back
        let parsed: ArbSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_id, "evt-btc-100k-2026");
        assert_eq!(parsed.direction, ArbDirection::BuyPredictionSellOptions);
        assert_eq!(parsed.raw_spread, dec("0.05"));
        assert_eq!(parsed.net_edge, dec("0.038"));
        assert_eq!(parsed.ttl_secs, 30);
        assert_eq!(parsed.threshold_status, ThresholdStatus::PassedBoth);
    }

    #[test]
    fn threshold_status_variants_are_distinct() {
        assert_ne!(ThresholdStatus::PassedBoth, ThresholdStatus::PassedStaticOnly);
        assert_ne!(ThresholdStatus::PassedStaticOnly, ThresholdStatus::Filtered);
        assert_ne!(ThresholdStatus::PassedBoth, ThresholdStatus::Filtered);
    }

    #[test]
    fn leg_info_serializes_with_string_decimals() {
        let leg = make_leg(Venue::Polymarket);
        let json = serde_json::to_string(&leg).unwrap();

        // Decimal fields should serialize as strings (not numbers)
        assert!(json.contains("\"0.55\""));
        assert!(json.contains("\"0.54\""));
        assert!(json.contains("\"0.95\""));

        // Roundtrip
        let parsed: LegInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.probability, dec("0.55"));
        assert_eq!(parsed.executable_price, dec("0.54"));
        assert_eq!(parsed.fill_ratio, dec("0.95"));
    }

    #[test]
    fn cost_breakdown_serializes_with_string_decimals() {
        let costs = make_cost_breakdown();
        let json = serde_json::to_string(&costs).unwrap();

        // All Decimal fields should be strings
        assert!(json.contains("\"0.005\""));
        assert!(json.contains("\"0.0003\""));
        assert!(json.contains("\"0.002\""));
        assert!(json.contains("\"0.0113\""));

        // Roundtrip
        let parsed: CostBreakdown = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prediction_fee, dec("0.005"));
        assert_eq!(parsed.total_cost, dec("0.0113"));
    }
}
