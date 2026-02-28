//! Core pricing types for the options pricing engine.
//!
//! All internal pricing math uses f64. Only the final probability output
//! (probability, prob_bid, prob_ask) uses `Decimal` via the `Probability`
//! newtype for pipeline consistency.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::types::{DualTimestamp, InstrumentId, Probability};

// ---------------------------------------------------------------------------
// Option type
// ---------------------------------------------------------------------------

/// Option type: Call or Put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptionType::Call => write!(f, "Call"),
            OptionType::Put => write!(f, "Put"),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsed instrument
// ---------------------------------------------------------------------------

/// Parsed Deribit instrument name components.
///
/// Extracted from names like "BTC-27JUN25-100000-C".
#[derive(Debug, Clone, Serialize)]
pub struct ParsedInstrument {
    /// Asset (e.g., "BTC", "ETH").
    pub asset: String,
    /// Expiry date.
    pub expiry: NaiveDate,
    /// Strike price in USD.
    pub strike: f64,
    /// Option type: Call or Put.
    pub option_type: OptionType,
}

// ---------------------------------------------------------------------------
// IV Solver types
// ---------------------------------------------------------------------------

/// Method used by the IV solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolverMethod {
    NewtonRaphson,
    Brent,
}

/// Result of an IV solve attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverResult {
    /// Solved implied volatility (annualized).
    pub iv: f64,
    /// Solver method used.
    pub method: SolverMethod,
    /// Number of iterations taken.
    pub iterations: u32,
    /// Whether the solver converged within tolerance.
    pub converged: bool,
    /// Residual |model_price - market_price| at the solution.
    pub residual: f64,
}

// ---------------------------------------------------------------------------
// Probability extraction types
// ---------------------------------------------------------------------------

/// Method used for probability extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingMethod {
    /// Call spread replication using real adjacent strikes.
    CallSpreadReplication,
    /// N(d2) with skew adjustment from vol surface.
    Nd2SkewAdjusted,
    /// Intrinsic value pricing (near-expiry fallback).
    IntrinsicOnly,
}

// ---------------------------------------------------------------------------
// Confidence scoring
// ---------------------------------------------------------------------------

/// Individual confidence component scores (each 0.0-1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceComponents {
    /// IV bid-ask spread score (tight spread = high).
    pub iv_spread: f64,
    /// Book depth score (deep book = high).
    pub book_depth: f64,
    /// Method agreement score (N(d2) vs call spread agree = high).
    pub method_agreement: f64,
    /// Solver convergence score (clean NR = high, Brent fallback = lower).
    pub solver_convergence: f64,
}

// ---------------------------------------------------------------------------
// Greeks
// ---------------------------------------------------------------------------

/// Per-instrument Greeks computed from Black-76.
///
/// Gamma is intentionally omitted per user decision (execution/hedging
/// concern, irrelevant without hedging in v1).
#[derive(Debug, Clone, Serialize)]
pub struct InstrumentGreeks {
    pub delta: f64,
    pub vega: f64,
    pub theta: f64,
}

// ---------------------------------------------------------------------------
// ImpliedProbability (main output)
// ---------------------------------------------------------------------------

/// Implied probability extracted from Deribit options market data.
///
/// This is the primary output of the pricing engine, carrying the probability
/// estimate along with confidence, method metadata, Greeks, and solver info.
#[derive(Debug, Clone, Serialize)]
pub struct ImpliedProbability {
    /// Instrument this probability was computed for.
    pub instrument_id: InstrumentId,

    /// Mid-price implied probability (primary estimate).
    pub probability: Probability,

    /// Bid-side implied probability (conservative bound).
    pub prob_bid: Option<Probability>,

    /// Ask-side implied probability (aggressive bound).
    pub prob_ask: Option<Probability>,

    /// Composite confidence score (0.0-1.0).
    pub confidence: f64,

    /// Individual confidence component scores.
    pub confidence_components: ConfidenceComponents,

    /// Probability extraction method used.
    pub method: PricingMethod,

    /// Skew adjustment magnitude (strike_iv - atm_iv).
    pub skew_adjustment: f64,

    /// Per-instrument Greeks (delta, vega, theta).
    pub greeks: InstrumentGreeks,

    /// IV solver metadata (for convergence logging).
    pub solver_meta: Option<SolverResult>,

    /// Epsilon used for call spread replication (distance between bracket strikes).
    pub epsilon_used: Option<f64>,

    /// Forward/underlying price used in pricing (USD).
    pub underlying_price: f64,

    /// Dual timestamp (wall + monotonic).
    pub timestamp: DualTimestamp,

    /// True if below near-expiry cutoff, using intrinsic pricing.
    pub near_expiry: bool,

    /// IV bid-ask spread (ask_iv - bid_iv) from IV solver.
    /// Zero for near-expiry intrinsic pricing (no IV solver runs).
    pub iv_spread: f64,
}
