use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Top-level spread computation configuration.
///
/// Controls walk-the-book sizing, rolling statistics windows, and fee parameters.
/// All sub-configs use `#[serde(default)]` for graceful TOML loading.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SpreadConfig {
    /// Walk-the-book notional size (e.g., 500 USD).
    #[serde(with = "rust_decimal::serde::str")]
    pub target_notional: Decimal,

    /// How often to emit aggregate statistics (seconds).
    pub stats_emission_interval_secs: u64,

    /// Rolling window duration for spread statistics (seconds).
    /// Default: 14400 = 4 hours.
    pub rolling_window_secs: u64,

    /// Minimum samples before dynamic threshold activates.
    pub rolling_min_samples: usize,

    /// Staleness threshold for Polymarket snapshots (milliseconds).
    /// Snapshots with exchange_timestamp older than this are rejected.
    pub staleness_threshold_ms: u64,

    /// Staleness threshold for Kalshi snapshots (milliseconds).
    /// More permissive than Polymarket because Kalshi is REST-polled.
    pub kalshi_staleness_threshold_ms: u64,

    /// Directory for spread computation JSONL logs.
    pub log_dir: String,

    /// Signal threshold configuration.
    pub threshold: ThresholdConfig,

    /// Polymarket fee parameters.
    pub polymarket_fees: PolymarketFeeConfig,

    /// Kalshi fee parameters.
    pub kalshi_fees: KalshiFeeConfig,

    /// Carry cost parameters.
    pub carry: CarryConfig,
}

impl Default for SpreadConfig {
    fn default() -> Self {
        Self {
            target_notional: Decimal::new(500, 0),
            stats_emission_interval_secs: 60,
            rolling_window_secs: 14400,
            rolling_min_samples: 30,
            staleness_threshold_ms: 5_000,
            kalshi_staleness_threshold_ms: 15_000,
            log_dir: "spread_logs".to_string(),
            threshold: ThresholdConfig::default(),
            polymarket_fees: PolymarketFeeConfig::default(),
            kalshi_fees: KalshiFeeConfig::default(),
            carry: CarryConfig::default(),
        }
    }
}

/// Dynamic threshold configuration.
///
/// Threshold = max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ThresholdConfig {
    /// Static floor for spread threshold (e.g., 0.01 = 1%).
    #[serde(with = "rust_decimal::serde::str")]
    pub static_floor: Decimal,

    /// Multiplier for rolling standard deviation (e.g., 2.0).
    #[serde(with = "rust_decimal::serde::str")]
    pub k: Decimal,

    /// Scale factor for liquidity penalty when book is thin.
    #[serde(with = "rust_decimal::serde::str")]
    pub liquidity_penalty_scale: Decimal,

    /// Multiplier applied to static_floor during cold start
    /// (insufficient rolling window samples).
    #[serde(with = "rust_decimal::serde::str")]
    pub cold_start_multiplier: Decimal,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            static_floor: Decimal::new(1, 2),          // 0.01 = 1%
            k: Decimal::new(2, 0),                      // 2.0
            liquidity_penalty_scale: Decimal::new(2, 2), // 0.02
            cold_start_multiplier: Decimal::new(2, 0),   // 2.0
        }
    }
}

/// Polymarket dynamic fee configuration.
///
/// Fee formula: `shares * fee_rate * (price * (1 - price))^exponent`
/// With optional flat rate override for comparison testing.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PolymarketFeeConfig {
    /// Fee rate coefficient (e.g., 0.25 for crypto markets).
    #[serde(with = "rust_decimal::serde::str")]
    pub fee_rate: Decimal,

    /// Exponent: 1 for sports, 2 for crypto markets.
    pub exponent: u32,

    /// If set, overrides the dynamic formula with a flat per-share rate.
    /// `None` means use the dynamic formula.
    pub flat_rate_override: Option<Decimal>,
}

impl Default for PolymarketFeeConfig {
    fn default() -> Self {
        Self {
            fee_rate: Decimal::new(25, 2), // 0.25
            exponent: 2,
            flat_rate_override: None,
        }
    }
}

/// Kalshi taker fee configuration.
///
/// Fee formula: `coefficient * contracts * P * (1 - P)`
/// With ceiling rounding per Kalshi convention.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KalshiFeeConfig {
    /// Taker fee coefficient (default 0.07).
    #[serde(with = "rust_decimal::serde::str")]
    pub taker_coefficient: Decimal,

    /// Whether to apply ceiling rounding (Kalshi rounds up per contract).
    pub use_ceiling: bool,
}

impl Default for KalshiFeeConfig {
    fn default() -> Self {
        Self {
            taker_coefficient: Decimal::new(7, 2), // 0.07
            use_ceiling: true,
        }
    }
}

/// Carry cost configuration.
///
/// Models the opportunity cost of capital locked in a position.
/// Cost = notional * annualized_rate * reference_holding_days / 365
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CarryConfig {
    /// Annualized carry cost rate (e.g., 0.05 = 5%).
    #[serde(with = "rust_decimal::serde::str")]
    pub annualized_rate: Decimal,

    /// Expected holding period in days for carry computation.
    pub reference_holding_days: u32,
}

impl Default for CarryConfig {
    fn default() -> Self {
        Self {
            annualized_rate: Decimal::new(5, 2), // 0.05
            reference_holding_days: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_spread_config_has_sensible_values() {
        let cfg = SpreadConfig::default();
        assert_eq!(cfg.target_notional, Decimal::new(500, 0));
        assert_eq!(cfg.stats_emission_interval_secs, 60);
        assert_eq!(cfg.rolling_window_secs, 14400);
        assert_eq!(cfg.rolling_min_samples, 30);
        assert_eq!(cfg.log_dir, "spread_logs");
    }

    #[test]
    fn default_threshold_config() {
        let cfg = ThresholdConfig::default();
        assert_eq!(cfg.static_floor, Decimal::new(1, 2));
        assert_eq!(cfg.k, Decimal::new(2, 0));
        assert_eq!(cfg.liquidity_penalty_scale, Decimal::new(2, 2));
        assert_eq!(cfg.cold_start_multiplier, Decimal::new(2, 0));
    }

    #[test]
    fn default_polymarket_fee_config() {
        let cfg = PolymarketFeeConfig::default();
        assert_eq!(cfg.fee_rate, Decimal::new(25, 2));
        assert_eq!(cfg.exponent, 2);
        assert!(cfg.flat_rate_override.is_none());
    }

    #[test]
    fn default_kalshi_fee_config() {
        let cfg = KalshiFeeConfig::default();
        assert_eq!(cfg.taker_coefficient, Decimal::new(7, 2));
        assert!(cfg.use_ceiling);
    }

    #[test]
    fn default_carry_config() {
        let cfg = CarryConfig::default();
        assert_eq!(cfg.annualized_rate, Decimal::new(5, 2));
        assert_eq!(cfg.reference_holding_days, 30);
    }

    #[test]
    fn spread_config_deserializes_from_toml() {
        let toml_str = r#"
target_notional = "500.0"
stats_emission_interval_secs = 30
rolling_window_secs = 7200
rolling_min_samples = 20
log_dir = "custom_logs"

[threshold]
static_floor = "0.02"
k = "1.5"
liquidity_penalty_scale = "0.03"
cold_start_multiplier = "3.0"

[polymarket_fees]
fee_rate = "0.0175"
exponent = 1

[kalshi_fees]
taker_coefficient = "0.07"
use_ceiling = false

[carry]
annualized_rate = "0.08"
reference_holding_days = 14
"#;
        let cfg: SpreadConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.stats_emission_interval_secs, 30);
        assert_eq!(cfg.rolling_window_secs, 7200);
        assert_eq!(cfg.rolling_min_samples, 20);
        assert_eq!(cfg.log_dir, "custom_logs");
        assert_eq!(cfg.threshold.static_floor, Decimal::new(2, 2));
        assert_eq!(cfg.threshold.k, Decimal::new(15, 1));
        assert_eq!(cfg.polymarket_fees.fee_rate, Decimal::new(175, 4));
        assert_eq!(cfg.polymarket_fees.exponent, 1);
        assert!(!cfg.kalshi_fees.use_ceiling);
        assert_eq!(cfg.carry.annualized_rate, Decimal::new(8, 2));
        assert_eq!(cfg.carry.reference_holding_days, 14);
    }

    #[test]
    fn spread_config_deserializes_with_defaults() {
        // Empty TOML should deserialize with all defaults
        let cfg: SpreadConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.target_notional, Decimal::new(500, 0));
        assert_eq!(cfg.polymarket_fees.exponent, 2);
        assert!(cfg.kalshi_fees.use_ceiling);
    }

    #[test]
    fn spread_config_with_flat_rate_override() {
        let toml_str = r#"
[polymarket_fees]
fee_rate = "0.25"
exponent = 2
flat_rate_override = "0.01"
"#;
        let cfg: SpreadConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.polymarket_fees.flat_rate_override,
            Some(Decimal::new(1, 2))
        );
    }
}
