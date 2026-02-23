//! Per-instrument Greeks computation from Black-76 analytics.
//!
//! Computes delta, vega, and theta for individual options. Gamma is
//! intentionally omitted per user decision (execution/hedging concern,
//! irrelevant without hedging in v1).

use statrs::distribution::{Continuous, ContinuousCDF, Normal};

use super::black76;
use super::types::InstrumentGreeks;

/// Days per year for theta normalization.
const DAYS_PER_YEAR: f64 = 365.25;

/// Compute delta, vega, and theta for a single option.
///
/// # Parameters
/// - `f`: forward/futures price
/// - `k`: strike price
/// - `t`: time to expiry in years
/// - `sigma`: implied volatility (annualized)
/// - `r`: risk-free rate
/// - `is_call`: true for call, false for put
///
/// # Edge cases
/// - `t <= 0.0`: returns intrinsic delta (1.0 ITM, -1.0 ITM put, 0.0 OTM),
///   vega=0, theta=0.
pub fn compute_greeks(f: f64, k: f64, t: f64, sigma: f64, r: f64, is_call: bool) -> InstrumentGreeks {
    // Degenerate case: expired or zero time
    if t <= 0.0 {
        let delta = if is_call {
            if f > k { 1.0 } else { 0.0 }
        } else if k > f {
            -1.0
        } else {
            0.0
        };
        return InstrumentGreeks {
            delta,
            vega: 0.0,
            theta: 0.0,
        };
    }

    let (d1, d2) = black76::d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();

    // Delta
    let delta = if is_call {
        df * norm.cdf(d1)
    } else {
        df * (norm.cdf(d1) - 1.0)
    };

    // Vega: sensitivity to 1% vol move
    let raw_vega = black76::vega(f, k, t, sigma, r);
    let vega = raw_vega / 100.0;

    // Theta (per day)
    // Black-76 theta for call:
    //   Theta = -df * F * n(d1) * sigma / (2*sqrt(T)) - r * df * (F * N(d1) - K * N(d2))
    // For put:
    //   Theta = -df * F * n(d1) * sigma / (2*sqrt(T)) - r * df * (-F * N(-d1) + K * N(-d2))
    // Note: when r=0, the second term vanishes.
    let sqrt_t = t.sqrt();
    let n_d1 = norm.pdf(d1);

    let time_decay = -df * f * n_d1 * sigma / (2.0 * sqrt_t);

    let carry_cost = if is_call {
        -r * df * (f * norm.cdf(d1) - k * norm.cdf(d2))
    } else {
        -r * df * (-f * norm.cdf(-d1) + k * norm.cdf(-d2))
    };

    let theta_per_year = time_decay + carry_cost;
    let theta = theta_per_year / DAYS_PER_YEAR;

    InstrumentGreeks { delta, vega, theta }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test a: ATM call delta ~0.5 (for Black-76 with r=0).
    /// Black-76 ATM call delta = N(d1) where d1 = sigma*sqrt(T)/2 = 0.10.
    /// N(0.10) = 0.5398, so delta is slightly above 0.5 for positive vol.
    #[test]
    fn atm_call_delta_near_half() {
        let greeks = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(
            (greeks.delta - 0.5).abs() < 0.05,
            "ATM call delta should be ~0.5, got {}",
            greeks.delta
        );
    }

    /// Test b: Deep ITM call delta ~1.0.
    #[test]
    fn deep_itm_call_delta_near_one() {
        // F=150, K=100 -> deep ITM call
        let greeks = compute_greeks(150.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(
            greeks.delta > 0.95,
            "deep ITM call delta should be ~1.0, got {}",
            greeks.delta
        );
    }

    /// Test c: Vega is positive for non-degenerate options.
    #[test]
    fn vega_positive() {
        let greeks = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(
            greeks.vega > 0.0,
            "vega should be positive, got {}",
            greeks.vega
        );

        // Also for puts
        let greeks_put = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, false);
        assert!(
            greeks_put.vega > 0.0,
            "put vega should be positive, got {}",
            greeks_put.vega
        );

        // Vega is the same for calls and puts
        assert!(
            (greeks.vega - greeks_put.vega).abs() < 1e-10,
            "call vega ({}) and put vega ({}) should be equal",
            greeks.vega,
            greeks_put.vega
        );
    }

    /// Test d: Put delta is negative (for OTM put).
    #[test]
    fn otm_put_delta_negative() {
        // F=100, K=90 -> OTM put
        let greeks = compute_greeks(100.0, 90.0, 1.0, 0.20, 0.0, false);
        assert!(
            greeks.delta < 0.0,
            "OTM put delta should be negative, got {}",
            greeks.delta
        );
    }

    /// Test e: Theta is negative for long options (time decay).
    #[test]
    fn theta_negative() {
        let greeks = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(
            greeks.theta < 0.0,
            "theta should be negative (time decay), got {}",
            greeks.theta
        );
    }

    /// Test f: Expired option returns intrinsic delta.
    #[test]
    fn expired_returns_intrinsic() {
        // ITM call
        let greeks = compute_greeks(110.0, 100.0, 0.0, 0.20, 0.0, true);
        assert!(
            (greeks.delta - 1.0).abs() < f64::EPSILON,
            "expired ITM call delta should be 1.0, got {}",
            greeks.delta
        );
        assert!(greeks.vega.abs() < f64::EPSILON, "expired vega should be 0");
        assert!(greeks.theta.abs() < f64::EPSILON, "expired theta should be 0");

        // OTM call
        let greeks = compute_greeks(90.0, 100.0, 0.0, 0.20, 0.0, true);
        assert!(
            greeks.delta.abs() < f64::EPSILON,
            "expired OTM call delta should be 0, got {}",
            greeks.delta
        );

        // ITM put
        let greeks = compute_greeks(90.0, 100.0, 0.0, 0.20, 0.0, false);
        assert!(
            (greeks.delta - (-1.0)).abs() < f64::EPSILON,
            "expired ITM put delta should be -1.0, got {}",
            greeks.delta
        );
    }

    /// Test g: Non-zero risk-free rate affects delta via discount factor.
    #[test]
    fn nonzero_rate_affects_delta() {
        let greeks_r0 = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        let greeks_r5 = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.05, true);
        // With positive rate, discounted delta should be slightly less
        assert!(
            greeks_r5.delta < greeks_r0.delta,
            "delta with r=0.05 ({}) should be less than delta with r=0.0 ({})",
            greeks_r5.delta,
            greeks_r0.delta
        );
    }
}
