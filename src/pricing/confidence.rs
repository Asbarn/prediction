//! Confidence scoring for implied probability estimates.
//!
//! Combines 4 components into a 0.0-1.0 composite score:
//! 1. **IV spread** -- tight bid-ask spread = high confidence
//! 2. **Book depth** -- deep order book = high confidence
//! 3. **Method agreement** -- call spread and N(d2) agree = high confidence
//! 4. **Solver convergence** -- clean NR convergence = high confidence
//!
//! All 4 components are individually logged alongside the composite for
//! analysis and debugging.

use super::config::ConfidenceConfig;
use super::types::{ConfidenceComponents, SolverMethod, SolverResult};

/// Compute composite confidence score from 4 weighted components.
///
/// Each component is normalized to 0.0-1.0, then combined via weighted sum.
/// The composite is clamped to [0.0, 1.0].
///
/// # Parameters
/// - `iv_bid_ask_spread`: IV bid-ask spread (vol points, e.g., 0.05 = 5 vol points)
/// - `book_depth_usd`: Total book depth in USD
/// - `method_disagreement`: |call_spread_prob - nd2_prob| (0.0 if only one method)
/// - `solver_quality`: Solver quality score (0.0-1.0, from `solver_quality_score`)
/// - `config`: Weight and scaling parameters
///
/// # Returns
/// (composite_score, individual_components)
pub fn compute_confidence(
    iv_bid_ask_spread: f64,
    book_depth_usd: f64,
    method_disagreement: f64,
    solver_quality: f64,
    config: &ConfidenceConfig,
) -> (f64, ConfidenceComponents) {
    // IV spread score: 1.0 = zero spread, 0.0 = spread >= iv_spread_max
    let iv_score = 1.0 - (iv_bid_ask_spread / config.iv_spread_max).min(1.0);

    // Depth score: 0.0 = no depth, 1.0 = depth >= depth_target
    let depth_score = (book_depth_usd / config.depth_target).min(1.0);

    // Agreement score: 1.0 = methods agree, 0.0 = disagreement >= max_disagreement
    let agreement_score = 1.0 - (method_disagreement / config.max_disagreement).min(1.0);

    // Solver score: pass-through (already 0.0-1.0)
    let solver_score = solver_quality;

    // Weighted composite
    let composite = config.iv_weight * iv_score
        + config.depth_weight * depth_score
        + config.agreement_weight * agreement_score
        + config.solver_weight * solver_score;

    let composite = composite.clamp(0.0, 1.0);

    let components = ConfidenceComponents {
        iv_spread: iv_score,
        book_depth: depth_score,
        method_agreement: agreement_score,
        solver_convergence: solver_score,
    };

    (composite, components)
}

/// Map solver result to a quality score (0.0-1.0).
///
/// - Converged via Newton-Raphson: 1.0 (best)
/// - Converged via Brent: 0.6 (reliable but slower convergence)
/// - Non-converged (clamped): 0.2 (low confidence in IV)
pub fn solver_quality_score(result: &SolverResult) -> f64 {
    if !result.converged {
        return 0.2;
    }
    match result.method {
        SolverMethod::NewtonRaphson => 1.0,
        SolverMethod::Brent => 0.6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::config::ConfidenceConfig;
    use crate::pricing::types::{SolverMethod, SolverResult};

    fn default_config() -> ConfidenceConfig {
        ConfidenceConfig::default()
    }

    /// Test a: Tight market -> confidence > 0.9.
    #[test]
    fn tight_market_high_confidence() {
        let config = default_config();
        let (score, components) = compute_confidence(
            0.5,      // low IV spread (0.5 vol pts out of 20 max)
            120000.0, // good depth (above 100k target)
            0.005,    // methods nearly agree
            1.0,      // NR converged
            &config,
        );

        assert!(
            score > 0.9,
            "tight market confidence should be > 0.9, got {}",
            score
        );
        // All components should be high
        assert!(components.iv_spread > 0.9, "iv_spread component should be high");
        assert!(components.book_depth > 0.9, "book_depth component should be high");
        assert!(components.method_agreement > 0.9, "method_agreement should be high");
        assert!(
            (components.solver_convergence - 1.0).abs() < f64::EPSILON,
            "solver should be 1.0 for NR"
        );
    }

    /// Test b: Wide spread tanks iv_score component.
    #[test]
    fn wide_spread_tanks_iv_score() {
        let config = default_config();
        let (score_tight, comp_tight) = compute_confidence(0.5, 100000.0, 0.0, 1.0, &config);
        let (score_wide, comp_wide) = compute_confidence(15.0, 100000.0, 0.0, 1.0, &config);

        assert!(
            comp_wide.iv_spread < comp_tight.iv_spread,
            "wide spread iv_score ({}) should be less than tight ({})",
            comp_wide.iv_spread,
            comp_tight.iv_spread
        );
        assert!(
            score_wide < score_tight,
            "wide spread composite ({}) should be less than tight ({})",
            score_wide,
            score_tight
        );
    }

    /// Test c: Large method disagreement tanks agreement_score.
    #[test]
    fn large_disagreement_tanks_agreement() {
        let config = default_config();
        let (_, comp_agree) = compute_confidence(1.0, 100000.0, 0.0, 1.0, &config);
        let (_, comp_disagree) = compute_confidence(1.0, 100000.0, 0.08, 1.0, &config);

        assert!(
            comp_disagree.method_agreement < comp_agree.method_agreement,
            "disagreeing methods agreement_score ({}) should be less than agreeing ({})",
            comp_disagree.method_agreement,
            comp_agree.method_agreement
        );
    }

    /// Test d: Brent fallback reduces solver_score to 0.6.
    #[test]
    fn brent_fallback_solver_score() {
        let nr_result = SolverResult {
            iv: 0.25,
            method: SolverMethod::NewtonRaphson,
            iterations: 5,
            converged: true,
            residual: 1e-10,
        };
        let brent_result = SolverResult {
            iv: 0.25,
            method: SolverMethod::Brent,
            iterations: 20,
            converged: true,
            residual: 1e-8,
        };
        let non_converged = SolverResult {
            iv: 0.25,
            method: SolverMethod::Brent,
            iterations: 100,
            converged: false,
            residual: 0.1,
        };

        assert!(
            (solver_quality_score(&nr_result) - 1.0).abs() < f64::EPSILON,
            "NR converged should be 1.0"
        );
        assert!(
            (solver_quality_score(&brent_result) - 0.6).abs() < f64::EPSILON,
            "Brent converged should be 0.6"
        );
        assert!(
            (solver_quality_score(&non_converged) - 0.2).abs() < f64::EPSILON,
            "non-converged should be 0.2"
        );
    }

    /// Test e: All weights sum to ~1.0 by default config.
    #[test]
    fn weights_sum_to_one() {
        let config = default_config();
        let sum = config.iv_weight + config.depth_weight
            + config.agreement_weight + config.solver_weight;
        assert!(
            (sum - 1.0).abs() < f64::EPSILON,
            "default weights should sum to 1.0, got {}",
            sum
        );
    }

    /// Test f: Wide market (bad everything) -> confidence < 0.5.
    #[test]
    fn wide_market_low_confidence() {
        let config = default_config();
        let (score, _) = compute_confidence(
            18.0,    // wide IV spread (close to max)
            5000.0,  // thin book
            0.08,    // methods disagree significantly
            0.2,     // non-converged solver
            &config,
        );

        assert!(
            score < 0.5,
            "wide market confidence should be < 0.5, got {}",
            score
        );
    }

    /// Test g: Perfect inputs produce confidence = 1.0.
    #[test]
    fn perfect_inputs_max_confidence() {
        let config = default_config();
        let (score, _) = compute_confidence(
            0.0,       // zero spread
            200000.0,  // well above depth target
            0.0,       // perfect agreement
            1.0,       // NR converged
            &config,
        );

        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "perfect inputs should produce confidence = 1.0, got {}",
            score
        );
    }

    /// Test h: Composite is clamped to [0.0, 1.0].
    #[test]
    fn composite_clamped() {
        let config = default_config();
        // Even with extreme inputs, result should be in [0, 1]
        let (score, _) = compute_confidence(100.0, 0.0, 1.0, 0.0, &config);
        assert!(score >= 0.0 && score <= 1.0, "score should be in [0, 1], got {}", score);

        let (score, _) = compute_confidence(0.0, 1e9, 0.0, 1.0, &config);
        assert!(score >= 0.0 && score <= 1.0, "score should be in [0, 1], got {}", score);
    }
}
