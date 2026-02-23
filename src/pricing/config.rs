//! Pricing engine configuration.
//!
//! All parameters are TOML-configurable with sensible defaults.
//! `PricingConfig` is added to `SystemConfig` with `#[serde(default)]`
//! so existing config files load without changes.

use serde::{Deserialize, Serialize};

/// Top-level pricing configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PricingConfig {
    /// IV solver parameters.
    pub solver: SolverConfig,
    /// Near-expiry cutoff in hours. Below this, switch to intrinsic pricing.
    pub near_expiry_cutoff_hours: f64,
    /// Vol surface construction parameters.
    pub vol_surface: VolSurfaceConfig,
    /// Probability extraction parameters.
    pub probability: ProbabilityConfig,
    /// Confidence scoring parameters.
    pub confidence: ConfidenceConfig,
    /// Risk-free rate (typically ~0 for crypto).
    pub risk_free_rate: f64,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            solver: SolverConfig::default(),
            near_expiry_cutoff_hours: 2.0,
            vol_surface: VolSurfaceConfig::default(),
            probability: ProbabilityConfig::default(),
            confidence: ConfidenceConfig::default(),
            risk_free_rate: 0.0,
        }
    }
}

/// IV solver configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SolverConfig {
    /// Maximum Newton-Raphson iterations before switching to Brent.
    pub nr_max_iterations: u32,
    /// Price tolerance for convergence (|model - market| < tolerance).
    pub price_tolerance: f64,
    /// Minimum vega to continue NR iterations (avoid divergence).
    pub vega_floor: f64,
    /// Minimum allowed IV (annualized, e.g., 1%).
    pub iv_min: f64,
    /// Maximum allowed IV (annualized, e.g., 500%).
    pub iv_max: f64,
    /// Maximum Brent's method iterations.
    pub brent_max_iterations: u32,
    /// Near-expiry cutoff in hours. Below this, solver returns intrinsic pricing.
    /// Mirrors PricingConfig.near_expiry_cutoff_hours for solver-level access.
    pub near_expiry_cutoff_hours: f64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            nr_max_iterations: 50,
            price_tolerance: 1e-8,
            vega_floor: 1e-10,
            iv_min: 0.01,
            iv_max: 5.0,
            brent_max_iterations: 100,
            near_expiry_cutoff_hours: 2.0,
        }
    }
}

/// Vol surface construction configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VolSurfaceConfig {
    /// Minimum usable strikes per expiry to build a surface.
    pub min_usable_strikes: usize,
    /// "Good" strike count for confidence tiers.
    pub good_strike_count: usize,
    /// Maximum IV bid-ask spread filter. Strikes with wider spread are excluded.
    pub max_iv_spread_filter: f64,
}

impl Default for VolSurfaceConfig {
    fn default() -> Self {
        Self {
            min_usable_strikes: 3,
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        }
    }
}

/// Probability extraction configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProbabilityConfig {
    /// Maximum epsilon (USD) for call spread replication.
    /// Beyond this, fall back to N(d2).
    pub max_epsilon_usd: f64,
}

impl Default for ProbabilityConfig {
    fn default() -> Self {
        Self {
            max_epsilon_usd: 10000.0,
        }
    }
}

/// Confidence scoring configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ConfidenceConfig {
    /// Weight for IV bid-ask spread component.
    pub iv_weight: f64,
    /// Weight for book depth component.
    pub depth_weight: f64,
    /// Weight for method agreement component.
    pub agreement_weight: f64,
    /// Weight for solver convergence component.
    pub solver_weight: f64,
    /// IV spread (vol points) at which the IV component scores 0.
    pub iv_spread_max: f64,
    /// Book depth (USD) at which the depth component scores 1.0.
    pub depth_target: f64,
    /// Probability disagreement at which the agreement component scores 0.
    pub max_disagreement: f64,
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            iv_weight: 0.30,
            depth_weight: 0.20,
            agreement_weight: 0.30,
            solver_weight: 0.20,
            iv_spread_max: 20.0,
            depth_target: 100000.0,
            max_disagreement: 0.10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_config_deserializes_from_empty_toml() {
        let config: PricingConfig = toml::from_str("").unwrap();
        assert_eq!(config.solver.nr_max_iterations, 50);
        assert!((config.solver.price_tolerance - 1e-8).abs() < f64::EPSILON);
        assert!((config.near_expiry_cutoff_hours - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.vol_surface.min_usable_strikes, 3);
        assert!((config.probability.max_epsilon_usd - 10000.0).abs() < f64::EPSILON);
        assert!((config.confidence.iv_weight - 0.30).abs() < f64::EPSILON);
        assert!((config.risk_free_rate).abs() < f64::EPSILON);
    }

    #[test]
    fn pricing_config_partial_override() {
        let toml_str = r#"
            near_expiry_cutoff_hours = 4.0
            risk_free_rate = 0.05

            [solver]
            nr_max_iterations = 100
        "#;
        let config: PricingConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.solver.nr_max_iterations, 100);
        // Defaults preserved for unspecified fields
        assert!((config.solver.price_tolerance - 1e-8).abs() < f64::EPSILON);
        assert!((config.near_expiry_cutoff_hours - 4.0).abs() < f64::EPSILON);
        assert!((config.risk_free_rate - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_weights_sum_to_one() {
        let config = ConfidenceConfig::default();
        let sum = config.iv_weight + config.depth_weight
            + config.agreement_weight + config.solver_weight;
        assert!((sum - 1.0).abs() < f64::EPSILON);
    }
}
