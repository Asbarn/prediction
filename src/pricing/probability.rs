//! Probability extraction from options data.
//!
//! Two independent methods:
//! 1. **Call spread replication** (primary): Uses real adjacent strikes from the
//!    vol surface to compute P(S > K) via a tight call spread. Captures skew.
//! 2. **N(d2)** (baseline): Black-76 risk-neutral probability with skew adjustment
//!    (strike-specific IV vs ATM IV). Always computed for method disagreement.
//!
//! Both methods are always computed and logged. Method disagreement feeds into
//! downstream confidence scoring.

use serde::Serialize;
use statrs::distribution::ContinuousCDF;

use super::black76;
use super::config::PricingConfig;
use super::vol_surface::VolSmile;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of call spread replication probability extraction.
#[derive(Debug, Clone, Serialize)]
pub struct CallSpreadResult {
    /// Extracted probability P(S > K).
    pub probability: f64,
    /// Epsilon used (half the distance between bracket strikes).
    pub epsilon_used: f64,
    /// Lower bracket strike.
    pub k_lower: f64,
    /// Upper bracket strike.
    pub k_upper: f64,
}

/// Result of N(d2) probability extraction.
#[derive(Debug, Clone, Serialize)]
pub struct Nd2Result {
    /// Extracted probability P(S > K) via N(d2).
    pub probability: f64,
    /// Skew adjustment: strike_iv - atm_iv (0.0 if ATM IV unavailable).
    pub skew_adjustment: f64,
}

/// Combined probability extraction output.
#[derive(Debug, Clone, Serialize)]
pub struct ProbabilityExtraction {
    /// Primary probability estimate (from whichever method is primary).
    pub primary_probability: f64,
    /// Which method was used as primary.
    pub primary_method: ProbabilityMethod,
    /// Call spread replication result (None if epsilon too large).
    pub call_spread: Option<CallSpreadResult>,
    /// N(d2) result (always available when IV can be interpolated).
    pub nd2: Nd2Result,
    /// Absolute disagreement between methods (0.0 if only one available).
    pub method_disagreement: f64,
    /// Skew adjustment from N(d2) (strike_iv - atm_iv).
    pub skew_adjustment: f64,
}

/// Which probability method was used as primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProbabilityMethod {
    CallSpreadReplication,
    Nd2SkewAdjusted,
}

// ---------------------------------------------------------------------------
// Call spread replication
// ---------------------------------------------------------------------------

/// Compute P(S > K) using call spread replication with real adjacent strikes.
///
/// Uses the vol smile's nearest bracket to find (k_lower, k_upper) around the
/// target strike, prices calls at both using their respective interpolated IVs,
/// and computes the probability from the price difference.
///
/// Returns None if:
/// - No bracket available (target outside observed strike range)
/// - Epsilon exceeds `config.probability.max_epsilon_usd`
/// - IV interpolation fails at either bracket strike
fn call_spread_probability(
    target_strike: f64,
    smile: &VolSmile,
    forward: f64,
    time_to_expiry: f64,
    rate: f64,
    config: &PricingConfig,
) -> Option<CallSpreadResult> {
    let (k_lower, k_upper) = smile.nearest_bracket(target_strike)?;

    let epsilon = (k_upper - k_lower) / 2.0;

    if epsilon > config.probability.max_epsilon_usd {
        return None;
    }

    let iv_lower = smile.interpolate(k_lower)?;
    let iv_upper = smile.interpolate(k_upper)?;

    let c_lower = black76::call_price(forward, k_lower, time_to_expiry, iv_lower, rate);
    let c_upper = black76::call_price(forward, k_upper, time_to_expiry, iv_upper, rate);

    let spread = k_upper - k_lower;
    if spread <= 0.0 {
        return None;
    }

    let prob = ((c_lower - c_upper) / spread).clamp(0.0, 1.0);

    Some(CallSpreadResult {
        probability: prob,
        epsilon_used: epsilon,
        k_lower,
        k_upper,
    })
}

// ---------------------------------------------------------------------------
// N(d2) probability
// ---------------------------------------------------------------------------

/// Compute P(S > K) using N(d2) with skew adjustment.
///
/// Uses the Black-76 d2 and the standard normal CDF. The skew adjustment
/// is strike_iv - atm_iv, capturing how far the strike's IV departs from
/// the at-the-money level.
fn nd2_probability(
    forward: f64,
    strike: f64,
    time_to_expiry: f64,
    strike_iv: f64,
    atm_iv: Option<f64>,
) -> Nd2Result {
    let (_, d2) = black76::d1_d2(forward, strike, time_to_expiry, strike_iv);
    let norm = statrs::distribution::Normal::standard();
    let prob = norm.cdf(d2);
    let skew_adjustment = atm_iv.map(|atm| strike_iv - atm).unwrap_or(0.0);

    Nd2Result {
        probability: prob,
        skew_adjustment,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract probabilities using both methods and determine primary.
///
/// Call spread replication is primary when available (epsilon within bounds).
/// Falls back to N(d2) with skew adjustment when call spread is unavailable.
/// Method disagreement is computed when both methods produce results.
pub fn extract_probabilities(
    target_strike: f64,
    smile: &VolSmile,
    forward: f64,
    time_to_expiry: f64,
    rate: f64,
    config: &PricingConfig,
) -> Option<ProbabilityExtraction> {
    // Compute call spread (may return None)
    let call_spread = call_spread_probability(
        target_strike,
        smile,
        forward,
        time_to_expiry,
        rate,
        config,
    );

    // Get strike-specific IV for N(d2)
    let strike_iv = smile.interpolate(target_strike)?;

    // Compute N(d2) (always available when IV exists)
    let nd2 = nd2_probability(forward, target_strike, time_to_expiry, strike_iv, smile.atm_iv);

    // Determine primary method and probability
    let (primary_probability, primary_method) = if let Some(ref cs) = call_spread {
        (cs.probability, ProbabilityMethod::CallSpreadReplication)
    } else {
        (nd2.probability, ProbabilityMethod::Nd2SkewAdjusted)
    };

    // Method disagreement: |call_spread - nd2| when both available
    let method_disagreement = if let Some(ref cs) = call_spread {
        (cs.probability - nd2.probability).abs()
    } else {
        0.0
    };

    let skew_adjustment = nd2.skew_adjustment;

    Some(ProbabilityExtraction {
        primary_probability,
        primary_method,
        call_spread,
        nd2,
        method_disagreement,
        skew_adjustment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::config::{PricingConfig, ProbabilityConfig, VolSurfaceConfig};
    use crate::pricing::vol_surface::{SmilePoint, VolSmile};
    use chrono::NaiveDate;

    fn make_point(strike: f64, iv: f64, spread: f64) -> SmilePoint {
        SmilePoint {
            strike,
            iv,
            bid_iv: iv - spread / 2.0,
            ask_iv: iv + spread / 2.0,
            iv_spread: spread,
        }
    }

    fn flat_smile(iv: f64) -> VolSmile {
        let config = VolSurfaceConfig {
            min_usable_strikes: 3,
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        };
        let points = vec![
            make_point(90.0, iv, 0.01),
            make_point(95.0, iv, 0.01),
            make_point(100.0, iv, 0.01),
            make_point(105.0, iv, 0.01),
            make_point(110.0, iv, 0.01),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        VolSmile::new(expiry, points, &config, 100.0)
    }

    fn skewed_smile() -> VolSmile {
        let config = VolSurfaceConfig {
            min_usable_strikes: 3,
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        };
        // Typical put skew: lower strikes have higher IV
        let points = vec![
            make_point(90.0, 0.35, 0.02),
            make_point(95.0, 0.28, 0.02),
            make_point(100.0, 0.25, 0.02),
            make_point(105.0, 0.27, 0.02),
            make_point(110.0, 0.30, 0.02),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        VolSmile::new(expiry, points, &config, 100.0)
    }

    fn default_pricing_config() -> PricingConfig {
        PricingConfig::default()
    }

    /// Test a: Call spread replication with known vol smile produces expected probability.
    #[test]
    fn call_spread_known_smile() {
        let smile = flat_smile(0.20);
        let config = default_pricing_config();

        let result = call_spread_probability(100.0, &smile, 100.0, 1.0, 0.0, &config);

        assert!(result.is_some(), "call spread should succeed for ATM strike with good smile");
        let cs = result.unwrap();
        // On a flat smile, call spread should produce a probability close to 0.5 for ATM
        assert!(
            cs.probability > 0.3 && cs.probability < 0.7,
            "ATM call spread probability should be near 0.5, got {}",
            cs.probability
        );
        assert!(cs.epsilon_used > 0.0, "epsilon should be positive");
        assert!(cs.k_lower < 100.0 && cs.k_upper > 100.0, "bracket should surround target");
    }

    /// Test b: Call spread returns None when epsilon exceeds max_epsilon_usd.
    #[test]
    fn call_spread_none_when_epsilon_exceeds_max() {
        let smile = flat_smile(0.20);
        let mut config = default_pricing_config();
        // Set max epsilon very small so it will fail (bracket is 5.0 apart, epsilon = 2.5)
        config.probability = ProbabilityConfig {
            max_epsilon_usd: 1.0,
        };

        let result = call_spread_probability(100.0, &smile, 100.0, 1.0, 0.0, &config);
        assert!(result.is_none(), "should return None when epsilon exceeds max_epsilon_usd");
    }

    /// Test c: N(d2) with ATM vol (skew=0) matches call spread for flat smile.
    /// On a flat volatility surface, both methods should converge.
    #[test]
    fn nd2_matches_call_spread_on_flat_surface() {
        let smile = flat_smile(0.20);
        let config = default_pricing_config();

        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, &config).unwrap();

        // Both methods should agree on a flat surface
        if let Some(ref cs) = extraction.call_spread {
            let diff = (cs.probability - extraction.nd2.probability).abs();
            assert!(
                diff < 0.02,
                "on flat surface, call spread ({}) and N(d2) ({}) should agree within 2%, diff={}",
                cs.probability,
                extraction.nd2.probability,
                diff
            );
        }
    }

    /// Test d: N(d2) skew_adjustment computed correctly (strike_iv - atm_iv).
    #[test]
    fn nd2_skew_adjustment_correct() {
        let smile = skewed_smile();
        // ATM IV = 0.25 (at strike 100)
        // At strike 95: IV = 0.28, skew = 0.28 - 0.25 = 0.03
        let nd2 = nd2_probability(100.0, 95.0, 1.0, 0.28, smile.atm_iv);
        assert!(
            (nd2.skew_adjustment - 0.03).abs() < 1e-10,
            "skew adjustment should be 0.03, got {}",
            nd2.skew_adjustment
        );

        // At ATM: skew should be 0
        let nd2_atm = nd2_probability(100.0, 100.0, 1.0, 0.25, smile.atm_iv);
        assert!(
            nd2_atm.skew_adjustment.abs() < 1e-10,
            "ATM skew adjustment should be 0, got {}",
            nd2_atm.skew_adjustment
        );
    }

    /// Test e: extract_probabilities uses call spread as primary when available.
    #[test]
    fn extract_uses_call_spread_as_primary() {
        let smile = flat_smile(0.20);
        let config = default_pricing_config();

        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, &config).unwrap();

        assert_eq!(
            extraction.primary_method,
            ProbabilityMethod::CallSpreadReplication,
            "should use call spread as primary when available"
        );
        assert!(extraction.call_spread.is_some(), "call spread should be present");
    }

    /// Test f: extract_probabilities falls back to N(d2) when call spread unavailable.
    #[test]
    fn extract_falls_back_to_nd2() {
        let smile = flat_smile(0.20);
        let mut config = default_pricing_config();
        // Force call spread to fail by setting max epsilon very small
        config.probability = ProbabilityConfig {
            max_epsilon_usd: 0.01,
        };

        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, &config).unwrap();

        assert_eq!(
            extraction.primary_method,
            ProbabilityMethod::Nd2SkewAdjusted,
            "should fall back to N(d2) when call spread unavailable"
        );
        assert!(extraction.call_spread.is_none(), "call spread should be None");
        assert!(
            extraction.method_disagreement.abs() < f64::EPSILON,
            "disagreement should be 0 when only one method available"
        );
    }

    /// Test g: Method disagreement computed correctly when both methods produce different results.
    #[test]
    fn method_disagreement_on_skewed_surface() {
        let smile = skewed_smile();
        let config = default_pricing_config();

        // Use a strike away from ATM where skew matters
        let extraction = extract_probabilities(95.0, &smile, 100.0, 1.0, 0.0, &config).unwrap();

        if extraction.call_spread.is_some() {
            // On a skewed surface, methods may disagree
            // The key assertion: disagreement = |call_spread - nd2|
            let expected_disagreement = (extraction.call_spread.as_ref().unwrap().probability
                - extraction.nd2.probability)
                .abs();
            assert!(
                (extraction.method_disagreement - expected_disagreement).abs() < 1e-10,
                "method_disagreement should be |cs - nd2|, expected {}, got {}",
                expected_disagreement,
                extraction.method_disagreement
            );
        }
    }

    /// Test h: Probability is clamped to [0.0, 1.0].
    #[test]
    fn probability_clamped() {
        let smile = flat_smile(0.20);
        let config = default_pricing_config();

        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, &config).unwrap();
        assert!(
            extraction.primary_probability >= 0.0 && extraction.primary_probability <= 1.0,
            "probability should be in [0, 1], got {}",
            extraction.primary_probability
        );
        assert!(
            extraction.nd2.probability >= 0.0 && extraction.nd2.probability <= 1.0,
            "nd2 probability should be in [0, 1], got {}",
            extraction.nd2.probability
        );
    }

    /// Test i: N(d2) without ATM IV has zero skew adjustment.
    #[test]
    fn nd2_no_atm_iv_zero_skew() {
        let nd2 = nd2_probability(100.0, 100.0, 1.0, 0.20, None);
        assert!(
            nd2.skew_adjustment.abs() < f64::EPSILON,
            "skew adjustment should be 0 without ATM IV, got {}",
            nd2.skew_adjustment
        );
    }
}
