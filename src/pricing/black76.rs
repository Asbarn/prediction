//! Black-76 pricing model for futures options.
//!
//! Implements call/put pricing, vega, and intrinsic value using the
//! Black-76 (futures options) model. Uses `statrs` for Normal CDF/PDF.
//!
//! All functions operate in f64 space. Parameters:
//! - `f`: forward/futures price
//! - `k`: strike price
//! - `t`: time to expiry in years
//! - `sigma`: implied volatility (annualized)
//! - `r`: risk-free rate (typically 0.0 for crypto)

use statrs::distribution::{Continuous, ContinuousCDF, Normal};

// ---------------------------------------------------------------------------
// d1 / d2
// ---------------------------------------------------------------------------

/// Compute d1 and d2 for Black-76.
///
/// d1 = (ln(F/K) + 0.5 * sigma^2 * T) / (sigma * sqrt(T))
/// d2 = d1 - sigma * sqrt(T)
pub(crate) fn d1_d2(f: f64, k: f64, t: f64, sigma: f64) -> (f64, f64) {
    let sqrt_t = t.sqrt();
    let d1 = ((f / k).ln() + 0.5 * sigma * sigma * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    (d1, d2)
}

// ---------------------------------------------------------------------------
// Pricing functions
// ---------------------------------------------------------------------------

/// Black-76 call price.
///
/// C = df * (F * N(d1) - K * N(d2))
/// where df = exp(-r * T)
pub(crate) fn call_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return intrinsic_value(f, k, true);
    }
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * (f * norm.cdf(d1) - k * norm.cdf(d2))
}

/// Black-76 put price.
///
/// P = df * (K * N(-d2) - F * N(-d1))
/// where df = exp(-r * T)
pub(crate) fn put_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return intrinsic_value(f, k, false);
    }
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * (k * norm.cdf(-d2) - f * norm.cdf(-d1))
}

/// Dispatch to call_price or put_price based on option type.
pub(crate) fn price(f: f64, k: f64, t: f64, sigma: f64, r: f64, is_call: bool) -> f64 {
    if is_call {
        call_price(f, k, t, sigma, r)
    } else {
        put_price(f, k, t, sigma, r)
    }
}

// ---------------------------------------------------------------------------
// Greeks
// ---------------------------------------------------------------------------

/// Black-76 vega (sensitivity to volatility). Same for call and put.
///
/// vega = df * F * n(d1) * sqrt(T)
/// where n(d1) is the standard normal PDF at d1.
pub(crate) fn vega(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return 0.0;
    }
    let (d1, _) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * f * norm.pdf(d1) * t.sqrt()
}

// ---------------------------------------------------------------------------
// Intrinsic value
// ---------------------------------------------------------------------------

/// Intrinsic value of an option (payoff at expiry).
///
/// Call: max(0, F - K)
/// Put:  max(0, K - F)
pub(crate) fn intrinsic_value(f: f64, k: f64, is_call: bool) -> f64 {
    if is_call {
        (f - k).max(0.0)
    } else {
        (k - f).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test a: ATM call price matches known value.
    /// F=100, K=100, T=1.0, sigma=0.20, r=0.0 -> ~7.9656
    #[test]
    fn atm_call_price_known_value() {
        let c = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
        // Black-76 ATM call with sigma=0.20, T=1.0:
        // d1 = (0 + 0.02) / 0.20 = 0.10
        // d2 = 0.10 - 0.20 = -0.10
        // C = 100*N(0.10) - 100*N(-0.10) = 100*(N(0.10) - N(-0.10))
        // N(0.10) = 0.53983, N(-0.10) = 0.46017
        // C = 100 * 0.07966 = 7.966
        assert!(
            (c - 7.966).abs() < 0.01,
            "ATM call price should be ~7.97, got {c}"
        );
    }

    /// Test b: Put-call parity: C - P = df * (F - K) for various strikes.
    #[test]
    fn put_call_parity() {
        let f = 100.0_f64;
        let t = 0.5_f64;
        let sigma = 0.30_f64;
        let r = 0.05_f64;
        let df = (-r * t).exp();

        for &k in &[80.0, 90.0, 100.0, 110.0, 120.0] {
            let c = call_price(f, k, t, sigma, r);
            let p = put_price(f, k, t, sigma, r);
            let parity = df * (f - k);
            let diff = (c - p - parity).abs();
            assert!(
                diff < 1e-10,
                "Put-call parity violated at K={k}: C-P={}, df*(F-K)={parity}, diff={diff}",
                c - p
            );
        }
    }

    /// Test c: Vega is positive for non-degenerate inputs.
    #[test]
    fn vega_positive() {
        let v = vega(100.0, 100.0, 1.0, 0.20, 0.0);
        assert!(v > 0.0, "vega should be positive, got {v}");

        let v_otm = vega(100.0, 150.0, 0.5, 0.30, 0.0);
        assert!(v_otm > 0.0, "OTM vega should be positive, got {v_otm}");
    }

    /// Test d: Deep OTM call price near zero.
    #[test]
    fn deep_otm_call_near_zero() {
        // F=100, K=200, sigma=0.20, T=0.25 -> very deep OTM
        let c = call_price(100.0, 200.0, 0.25, 0.20, 0.0);
        assert!(
            c < 1e-6,
            "deep OTM call should be near zero, got {c}"
        );
    }

    /// Test e: Near-zero T returns intrinsic value.
    #[test]
    fn near_zero_t_returns_intrinsic() {
        // ITM call: F=110, K=100, T=0.0 -> intrinsic = 10
        let c = call_price(110.0, 100.0, 0.0, 0.20, 0.0);
        assert!(
            (c - 10.0).abs() < f64::EPSILON,
            "zero T call should return intrinsic, got {c}"
        );

        // OTM call: F=90, K=100, T=0.0 -> intrinsic = 0
        let c = call_price(90.0, 100.0, 0.0, 0.20, 0.0);
        assert!(
            c.abs() < f64::EPSILON,
            "zero T OTM call should return 0, got {c}"
        );

        // ITM put: F=90, K=100, T=0.0 -> intrinsic = 10
        let p = put_price(90.0, 100.0, 0.0, 0.20, 0.0);
        assert!(
            (p - 10.0).abs() < f64::EPSILON,
            "zero T put should return intrinsic, got {p}"
        );

        // Zero sigma also returns intrinsic
        let c = call_price(110.0, 100.0, 1.0, 0.0, 0.0);
        assert!(
            (c - 10.0).abs() < f64::EPSILON,
            "zero sigma call should return intrinsic, got {c}"
        );
    }

    /// Test f: Vega matches finite-difference approximation.
    /// vega_fd = (price(sigma+h) - price(sigma-h)) / (2*h)
    #[test]
    fn vega_finite_difference() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.0;
        let h = 1e-5;

        let v_analytic = vega(f, k, t, sigma, r);
        let price_up = call_price(f, k, t, sigma + h, r);
        let price_down = call_price(f, k, t, sigma - h, r);
        let v_fd = (price_up - price_down) / (2.0 * h);

        let diff = (v_analytic - v_fd).abs();
        assert!(
            diff < 1e-4,
            "vega analytic ({v_analytic}) vs finite-diff ({v_fd}) difference too large: {diff}"
        );
    }

    /// Additional: price() dispatches correctly.
    #[test]
    fn price_dispatch() {
        let c = price(100.0, 100.0, 1.0, 0.20, 0.0, true);
        let p = price(100.0, 100.0, 1.0, 0.20, 0.0, false);
        assert!((c - call_price(100.0, 100.0, 1.0, 0.20, 0.0)).abs() < f64::EPSILON);
        assert!((p - put_price(100.0, 100.0, 1.0, 0.20, 0.0)).abs() < f64::EPSILON);
    }

    /// Additional: vega is zero when T <= 0 or sigma <= 0.
    #[test]
    fn vega_degenerate_inputs() {
        assert!(vega(100.0, 100.0, 0.0, 0.20, 0.0).abs() < f64::EPSILON);
        assert!(vega(100.0, 100.0, 1.0, 0.0, 0.0).abs() < f64::EPSILON);
    }

    /// Additional: intrinsic value function correctness.
    #[test]
    fn intrinsic_value_correctness() {
        assert!((intrinsic_value(110.0, 100.0, true) - 10.0).abs() < f64::EPSILON);
        assert!((intrinsic_value(90.0, 100.0, true) - 0.0).abs() < f64::EPSILON);
        assert!((intrinsic_value(90.0, 100.0, false) - 10.0).abs() < f64::EPSILON);
        assert!((intrinsic_value(110.0, 100.0, false) - 0.0).abs() < f64::EPSILON);
    }
}
