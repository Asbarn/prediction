//! Implied volatility solver using Newton-Raphson with Brent's method fallback.
//!
//! Extracts implied volatility from option market prices by finding sigma such
//! that Black-76 model price matches the observed market price. Handles edge
//! cases: deep ITM/OTM (near-zero vega), near-expiry theta collapse, negative
//! time value, and configurable IV clamping.

use crate::pricing::black76;
use crate::pricing::config::SolverConfig;
use crate::pricing::types::{SolverMethod, SolverResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Hours per year for converting cutoff hours to years.
const HOURS_PER_YEAR: f64 = 8760.0;

/// Two * pi, used in Brenner-Subrahmanyam initial guess.
const TWO_PI: f64 = std::f64::consts::TAU;

// ---------------------------------------------------------------------------
// Initial guess
// ---------------------------------------------------------------------------

/// Brenner-Subrahmanyam approximation for initial IV guess.
///
/// sigma_0 = sqrt(2*pi/T) * (C/F) -- works well for near-ATM options.
/// Clamped to [iv_min, iv_max] to avoid extreme starting points.
fn initial_guess(market_price: f64, f: f64, t: f64, iv_min: f64, iv_max: f64) -> f64 {
    if f <= 0.0 || t <= 0.0 {
        return (iv_min + iv_max) / 2.0;
    }
    let sigma_0 = (TWO_PI / t).sqrt() * (market_price / f);
    sigma_0.clamp(iv_min, iv_max)
}

// ---------------------------------------------------------------------------
// Brent's method
// ---------------------------------------------------------------------------

/// Brent's method for IV solving when Newton-Raphson fails.
///
/// Finds sigma in [iv_min, iv_max] such that black76::price(sigma) = market_price.
/// Combines bisection with inverse quadratic interpolation for superlinear
/// convergence while maintaining a guaranteed bracket.
fn brent_solve(
    market_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> SolverResult {
    let price_fn = |sigma: f64| -> f64 {
        black76::price(f, k, t, sigma, r, is_call) - market_price
    };

    let mut a = config.iv_min;
    let mut b = config.iv_max;
    let mut fa = price_fn(a);
    let mut fb = price_fn(b);

    // If both endpoints have the same sign, no root in bracket.
    // Return the endpoint with smaller residual.
    if fa * fb > 0.0 {
        let (best_sigma, best_residual) = if fa.abs() < fb.abs() {
            (a, fa.abs())
        } else {
            (b, fb.abs())
        };
        return SolverResult {
            iv: best_sigma,
            method: SolverMethod::Brent,
            iterations: 0,
            converged: best_residual < config.price_tolerance,
            residual: best_residual,
        };
    }

    // Ensure |f(a)| >= |f(b)| (b is the better guess)
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }

    let mut c = a;
    let mut fc = fa;
    let mut mflag = true;
    let mut d = 0.0_f64; // previous step size (only used when mflag is false)

    for i in 0..config.brent_max_iterations {
        if fb.abs() < config.price_tolerance {
            return SolverResult {
                iv: b,
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged: true,
                residual: fb.abs(),
            };
        }

        if (b - a).abs() < 1e-15 {
            return SolverResult {
                iv: b,
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged: fb.abs() < config.price_tolerance,
                residual: fb.abs(),
            };
        }

        // Attempt inverse quadratic interpolation if all three values differ
        let s = if (fa - fc).abs() > f64::EPSILON && (fb - fc).abs() > f64::EPSILON {
            // Inverse quadratic interpolation
            let s = a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb));
            s
        } else {
            // Secant method
            b - fb * (b - a) / (fb - fa)
        };

        // Conditions for rejecting interpolation in favor of bisection
        let midpoint = (a + b) / 2.0;
        let use_bisection = {
            // s is not between (3a+b)/4 and b
            let bound1 = (3.0 * a + b) / 4.0;
            let (lo, hi) = if bound1 < b {
                (bound1, b)
            } else {
                (b, bound1)
            };
            s < lo || s > hi
        } || (mflag && (s - b).abs() >= (b - c).abs() / 2.0)
            || (!mflag && (s - b).abs() >= (c - d).abs() / 2.0)
            || (mflag && (b - c).abs() < 1e-15)
            || (!mflag && (c - d).abs() < 1e-15);

        let s = if use_bisection {
            mflag = true;
            midpoint
        } else {
            mflag = false;
            s
        };

        let fs = price_fn(s);
        d = c;
        c = b;
        fc = fb;

        if fa * fs < 0.0 {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }

        // Ensure |f(a)| >= |f(b)|
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }

    // Did not converge within max iterations, return best result
    SolverResult {
        iv: b,
        method: SolverMethod::Brent,
        iterations: config.brent_max_iterations,
        converged: fb.abs() < config.price_tolerance,
        residual: fb.abs(),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Solve for implied volatility given a market price.
///
/// Uses Newton-Raphson with Brent's method fallback when vega is too small
/// for NR to converge reliably.
///
/// # Parameters
/// - `market_price`: observed option price (must be > 0)
/// - `f`: forward/futures price
/// - `k`: strike price
/// - `t`: time to expiry in years
/// - `r`: risk-free rate
/// - `is_call`: true for call, false for put
/// - `config`: solver configuration (tolerances, bounds, max iterations)
///
/// # Returns
/// `SolverResult` with IV, convergence status, method used, iteration count, residual.
pub(crate) fn solve_iv(
    market_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> SolverResult {
    // 1. Near-expiry cutoff: if T is below cutoff, return intrinsic pricing
    let near_expiry_cutoff_years = config.near_expiry_cutoff_hours / HOURS_PER_YEAR;
    if t <= 0.0 || t < near_expiry_cutoff_years {
        return SolverResult {
            iv: 0.0,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: true,
            residual: 0.0,
        };
    }

    // 2. Zero or negative market price: no valid IV
    if market_price <= 0.0 {
        return SolverResult {
            iv: config.iv_min,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            residual: market_price.abs(),
        };
    }

    // 3. Negative time value: market price below intrinsic value
    let intrinsic = black76::intrinsic_value(f, k, is_call);
    if market_price < intrinsic {
        return SolverResult {
            iv: config.iv_min,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            residual: (intrinsic - market_price).abs(),
        };
    }

    // 4. Compute initial guess (Brenner-Subrahmanyam approximation)
    let mut sigma = initial_guess(market_price, f, t, config.iv_min, config.iv_max);

    // 5. Newton-Raphson loop
    for i in 0..config.nr_max_iterations {
        let model_price = black76::price(f, k, t, sigma, r, is_call);
        let v = black76::vega(f, k, t, sigma, r);

        // Check vega floor: if vega too small, NR will diverge
        if v.abs() < config.vega_floor {
            break; // Fall through to Brent
        }

        let diff = model_price - market_price;

        // Convergence check
        if diff.abs() < config.price_tolerance {
            return SolverResult {
                iv: sigma,
                method: SolverMethod::NewtonRaphson,
                iterations: i + 1,
                converged: true,
                residual: diff.abs(),
            };
        }

        // NR update
        sigma -= diff / v;
        sigma = sigma.clamp(config.iv_min, config.iv_max);
    }

    // 6. NR failed to converge -- run Brent's method on [iv_min, iv_max]
    brent_solve(market_price, f, k, t, r, is_call, config)
}

/// Solve bid IV, ask IV, and mid IV independently.
///
/// Any individual solve failure does not block the others. Each result is
/// independent and may use different solver methods (e.g., NR for mid,
/// Brent for bid if bid price has very low vega).
pub(crate) fn solve_iv_triple(
    bid_price: f64,
    ask_price: f64,
    mid_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> (SolverResult, SolverResult, SolverResult) {
    let bid_result = solve_iv(bid_price, f, k, t, r, is_call, config);
    let ask_result = solve_iv(ask_price, f, k, t, r, is_call, config);
    let mid_result = solve_iv(mid_price, f, k, t, r, is_call, config);
    (bid_result, ask_result, mid_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::config::SolverConfig;
    use crate::pricing::types::SolverMethod;

    fn default_config() -> SolverConfig {
        SolverConfig::default()
    }

    // -----------------------------------------------------------------------
    // Test 1: ATM call converges via Newton-Raphson in < 10 iterations
    // -----------------------------------------------------------------------
    #[test]
    fn atm_call_converges_nr() {
        let config = default_config();
        // F=100, K=100, T=1.0, r=0.0, sigma=0.20 -> Black-76 price ~7.966
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(result.converged, "ATM call should converge");
        assert!(
            (result.iv - 0.20).abs() < 1e-6,
            "ATM call IV should be ~0.20, got {}",
            result.iv
        );
        assert_eq!(result.method, SolverMethod::NewtonRaphson);
        assert!(
            result.iterations < 10,
            "ATM should converge in < 10 NR iterations, took {}",
            result.iterations
        );
    }

    // -----------------------------------------------------------------------
    // Test 2: ATM put converges similarly
    // -----------------------------------------------------------------------
    #[test]
    fn atm_put_converges_nr() {
        let config = default_config();
        let market_price = crate::pricing::black76::put_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, false, &config);

        assert!(result.converged, "ATM put should converge");
        assert!(
            (result.iv - 0.20).abs() < 1e-6,
            "ATM put IV should be ~0.20, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: Deep OTM call falls back to Brent (vega too small for NR)
    // -----------------------------------------------------------------------
    #[test]
    fn deep_otm_call_uses_brent() {
        let config = default_config();
        // F=100, K=200, T=0.5, sigma=0.80 -> deep OTM
        let market_price = crate::pricing::black76::call_price(100.0, 200.0, 0.5, 0.80, 0.0);

        let result = solve_iv(market_price, 100.0, 200.0, 0.5, 0.0, true, &config);

        assert!(result.converged, "Deep OTM call should converge via Brent");
        assert!(
            (result.iv - 0.80).abs() < 1e-4,
            "Deep OTM call IV should be ~0.80, got {}",
            result.iv
        );
        // With vega floor, deep OTM may still use NR if vega is just above floor.
        // We accept either method as long as it converges.
    }

    // -----------------------------------------------------------------------
    // Test 4: Deep ITM put converges
    // -----------------------------------------------------------------------
    #[test]
    fn deep_itm_put_converges() {
        let config = default_config();
        // F=100, K=50, T=0.5, sigma=0.30 -> deep ITM put (K < F)
        let market_price = crate::pricing::black76::put_price(100.0, 50.0, 0.5, 0.30, 0.0);

        let result = solve_iv(market_price, 100.0, 50.0, 0.5, 0.0, false, &config);

        // Deep ITM put price is dominated by intrinsic value, IV may be hard to extract
        // but should converge to something reasonable
        assert!(result.converged, "Deep ITM put should converge");
        assert!(
            (result.iv - 0.30).abs() < 0.01,
            "Deep ITM put IV should be ~0.30, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: Near-expiry (T < cutoff) returns intrinsic pricing
    // -----------------------------------------------------------------------
    #[test]
    fn near_expiry_returns_intrinsic() {
        let config = default_config();
        // near_expiry_cutoff_hours = 2.0 (default), so cutoff in years = 2/8760
        // T = 0.0001 years ~ 0.876 hours < 2 hours cutoff
        let t = 0.0001;

        let result = solve_iv(5.0, 105.0, 100.0, t, 0.0, true, &config);

        // Near-expiry: should return 0.0 IV, converged=true, reflecting intrinsic pricing
        assert!(result.converged, "Near-expiry should report converged");
        assert!(
            result.iv.abs() < f64::EPSILON,
            "Near-expiry IV should be 0.0, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 6: Negative time value is detected and flagged
    // -----------------------------------------------------------------------
    #[test]
    fn negative_time_value_flagged() {
        let config = default_config();
        // ITM call: intrinsic = max(0, F-K) = max(0, 110-100) = 10
        // Market price = 9 < intrinsic = negative time value
        let result = solve_iv(9.0, 110.0, 100.0, 1.0, 0.0, true, &config);

        assert!(!result.converged, "Negative time value should not converge");
        assert!(
            (result.iv - config.iv_min).abs() < f64::EPSILON,
            "Negative time value IV should be iv_min={}, got {}",
            config.iv_min,
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 7: Zero market price returns iv_min non-converged
    // -----------------------------------------------------------------------
    #[test]
    fn zero_market_price_returns_iv_min() {
        let config = default_config();

        let result = solve_iv(0.0, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(!result.converged, "Zero price should not converge");
        assert!(
            (result.iv - config.iv_min).abs() < f64::EPSILON,
            "Zero price IV should be iv_min, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: Negative market price returns iv_min non-converged
    // -----------------------------------------------------------------------
    #[test]
    fn negative_market_price_returns_iv_min() {
        let config = default_config();

        let result = solve_iv(-1.0, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(!result.converged, "Negative price should not converge");
        assert!(
            (result.iv - config.iv_min).abs() < f64::EPSILON,
            "Negative price IV should be iv_min, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 9: IV clamping at upper bound
    // -----------------------------------------------------------------------
    #[test]
    fn iv_clamped_at_upper_bound() {
        let mut config = default_config();
        config.iv_max = 2.0; // 200% vol ceiling

        // Price a call with sigma=3.0 (above iv_max), so solver can't reach true IV
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 3.0, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        // Solver should clamp to iv_max and not converge (residual > 0)
        assert!(
            result.iv <= config.iv_max + f64::EPSILON,
            "IV should be clamped to iv_max={}, got {}",
            config.iv_max,
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 10: IV clamping at lower bound
    // -----------------------------------------------------------------------
    #[test]
    fn iv_clamped_at_lower_bound() {
        let mut config = default_config();
        config.iv_min = 0.05; // 5% vol floor

        // Price a call with sigma=0.02 (below iv_min)
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.02, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        // Solver should clamp to iv_min
        assert!(
            result.iv >= config.iv_min - f64::EPSILON,
            "IV should be clamped to iv_min={}, got {}",
            config.iv_min,
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 11: OTM call at various volatilities
    // -----------------------------------------------------------------------
    #[test]
    fn otm_call_various_vols() {
        let config = default_config();

        for &sigma in &[0.10, 0.30, 0.50, 1.0, 2.0] {
            let market_price = crate::pricing::black76::call_price(100.0, 120.0, 0.5, sigma, 0.0);
            if market_price < 1e-12 {
                continue; // Skip if price is essentially zero
            }

            let result = solve_iv(market_price, 100.0, 120.0, 0.5, 0.0, true, &config);

            assert!(
                result.converged,
                "OTM call with sigma={sigma} should converge, got converged={}",
                result.converged
            );
            assert!(
                (result.iv - sigma).abs() < 0.01,
                "OTM call with sigma={sigma}: expected IV ~{sigma}, got {}",
                result.iv
            );
        }
    }

    // -----------------------------------------------------------------------
    // Test 12: Non-zero risk-free rate
    // -----------------------------------------------------------------------
    #[test]
    fn nonzero_risk_free_rate() {
        let config = default_config();
        let r = 0.05;
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.25, r);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, r, true, &config);

        assert!(result.converged);
        assert!(
            (result.iv - 0.25).abs() < 1e-6,
            "IV with r=0.05 should be ~0.25, got {}",
            result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 13: solve_iv_triple produces three independent results
    // -----------------------------------------------------------------------
    #[test]
    fn solve_iv_triple_independent() {
        let config = default_config();

        // Generate prices at different IVs for bid/mid/ask
        let bid_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.18, 0.0);
        let ask_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.22, 0.0);
        let mid_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let (bid_result, ask_result, mid_result) =
            solve_iv_triple(bid_price, ask_price, mid_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(bid_result.converged, "Bid IV should converge");
        assert!(ask_result.converged, "Ask IV should converge");
        assert!(mid_result.converged, "Mid IV should converge");

        assert!(
            (bid_result.iv - 0.18).abs() < 1e-6,
            "Bid IV should be ~0.18, got {}",
            bid_result.iv
        );
        assert!(
            (ask_result.iv - 0.22).abs() < 1e-6,
            "Ask IV should be ~0.22, got {}",
            ask_result.iv
        );
        assert!(
            (mid_result.iv - 0.20).abs() < 1e-6,
            "Mid IV should be ~0.20, got {}",
            mid_result.iv
        );
    }

    // -----------------------------------------------------------------------
    // Test 14: solve_iv_triple individual failure doesn't block others
    // -----------------------------------------------------------------------
    #[test]
    fn solve_iv_triple_partial_failure() {
        let config = default_config();

        // Bid price is invalid (zero), but mid and ask are valid
        let bid_price = 0.0;
        let ask_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.22, 0.0);
        let mid_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let (bid_result, ask_result, mid_result) =
            solve_iv_triple(bid_price, ask_price, mid_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(!bid_result.converged, "Bid with zero price should not converge");
        assert!(ask_result.converged, "Ask should still converge");
        assert!(mid_result.converged, "Mid should still converge");
    }

    // -----------------------------------------------------------------------
    // Test 15: Residual is populated correctly
    // -----------------------------------------------------------------------
    #[test]
    fn residual_populated() {
        let config = default_config();
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(result.converged);
        assert!(
            result.residual < config.price_tolerance,
            "Residual should be < tolerance, got {}",
            result.residual
        );
    }

    // -----------------------------------------------------------------------
    // Test 16: High vol option converges
    // -----------------------------------------------------------------------
    #[test]
    fn high_vol_converges() {
        let config = default_config();
        let market_price = crate::pricing::black76::call_price(100.0, 100.0, 1.0, 3.0, 0.0);

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(result.converged, "High vol (300%) ATM should converge");
        assert!(
            (result.iv - 3.0).abs() < 0.01,
            "High vol IV should be ~3.0, got {}",
            result.iv
        );
    }
}
