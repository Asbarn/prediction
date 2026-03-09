//! Signal generation configuration.
//!
//! Controls staleness thresholds, TTL, fee parameters, and logging for the
//! cross-asset signal engine. Uses `#[serde(default)]` on the struct for
//! backward-compatible TOML loading.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::spread::config::{CarryConfig, KalshiFeeConfig, PolymarketFeeConfig, ThresholdConfig};

/// Configuration for the cross-asset signal generation engine.
///
/// Loaded from TOML with `#[serde(default)]` for graceful backward
/// compatibility -- any missing field gets its default value.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SignalGenerationConfig {
    /// Options data staleness threshold (ms).
    /// More lenient than prediction markets because Deribit options update
    /// less frequently.
    pub options_staleness_ms: u64,

    /// Polymarket data staleness threshold (ms).
    pub polymarket_staleness_ms: u64,

    /// Kalshi data staleness threshold (ms).
    pub kalshi_staleness_ms: u64,

    /// Signal time-to-live (seconds). Fixed for v1.
    pub signal_ttl_secs: u64,

    /// Target notional for walk-the-book sizing (USD).
    #[serde(with = "rust_decimal::serde::str")]
    pub target_notional: Decimal,

    /// Deribit taker fee rate (e.g., 0.0003 = 0.03%).
    #[serde(with = "rust_decimal::serde::str")]
    pub deribit_taker_fee_rate: Decimal,

    /// Signal threshold configuration (reused from spread module).
    pub threshold: ThresholdConfig,

    /// Rolling window duration for statistics (seconds).
    pub rolling_window_secs: u64,

    /// Directory for signal JSONL log files.
    pub log_dir: String,

    /// Directory for cross-asset spread JSONL log files.
    /// When set, the CrossAssetEngine also writes SpreadResult entries here.
    pub spread_log_dir: String,

    /// Summary emission interval (seconds).
    pub summary_interval_secs: u64,

    /// Carry cost configuration.
    pub carry: CarryConfig,

    /// Polymarket fee configuration.
    pub polymarket_fees: PolymarketFeeConfig,

    /// Kalshi fee configuration.
    pub kalshi_fees: KalshiFeeConfig,

    /// Scale factor for settlement basis risk premium (same semantics as SpreadConfig).
    #[serde(default = "default_basis_risk_scale")]
    #[serde(with = "rust_decimal::serde::str")]
    pub basis_risk_scale: Decimal,
}

/// Default basis risk scale: 0.01 (1% of composite score).
fn default_basis_risk_scale() -> Decimal {
    Decimal::new(1, 2)
}

impl Default for SignalGenerationConfig {
    fn default() -> Self {
        Self {
            options_staleness_ms: 30_000,
            polymarket_staleness_ms: 5_000,
            kalshi_staleness_ms: 15_000,
            signal_ttl_secs: 30,
            target_notional: Decimal::new(500, 0),
            deribit_taker_fee_rate: Decimal::new(3, 4), // 0.0003
            threshold: ThresholdConfig::default(),
            rolling_window_secs: 14_400,
            log_dir: "signal_logs".to_string(),
            spread_log_dir: "spread_logs".to_string(),
            summary_interval_secs: 300,
            carry: CarryConfig::default(),
            polymarket_fees: PolymarketFeeConfig::default(),
            kalshi_fees: KalshiFeeConfig::default(),
            basis_risk_scale: default_basis_risk_scale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = SignalGenerationConfig::default();
        assert_eq!(cfg.options_staleness_ms, 30_000);
        assert_eq!(cfg.polymarket_staleness_ms, 5_000);
        assert_eq!(cfg.kalshi_staleness_ms, 15_000);
        assert_eq!(cfg.signal_ttl_secs, 30);
        assert_eq!(cfg.target_notional, Decimal::new(500, 0));
        assert_eq!(cfg.deribit_taker_fee_rate, Decimal::new(3, 4));
        assert_eq!(cfg.rolling_window_secs, 14_400);
        assert_eq!(cfg.log_dir, "signal_logs");
        assert_eq!(cfg.spread_log_dir, "spread_logs");
        assert_eq!(cfg.summary_interval_secs, 300);
    }

    #[test]
    fn config_deserializes_from_empty_toml() {
        let cfg: SignalGenerationConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.options_staleness_ms, 30_000);
        assert_eq!(cfg.signal_ttl_secs, 30);
        assert_eq!(cfg.target_notional, Decimal::new(500, 0));
        assert_eq!(cfg.log_dir, "signal_logs");
    }

    #[test]
    fn config_deserializes_from_partial_toml() {
        let toml_str = r#"
options_staleness_ms = 60000
signal_ttl_secs = 60
log_dir = "custom_signals"

[threshold]
static_floor = "0.02"
k = "1.5"

[carry]
annualized_rate = "0.08"
reference_holding_days = 14
"#;
        let cfg: SignalGenerationConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.options_staleness_ms, 60_000);
        assert_eq!(cfg.signal_ttl_secs, 60);
        assert_eq!(cfg.log_dir, "custom_signals");
        assert_eq!(cfg.threshold.static_floor, Decimal::new(2, 2));
        assert_eq!(cfg.threshold.k, Decimal::new(15, 1));
        // Unset fields use defaults
        assert_eq!(cfg.polymarket_staleness_ms, 5_000);
        assert_eq!(cfg.carry.annualized_rate, Decimal::new(8, 2));
        assert_eq!(cfg.carry.reference_holding_days, 14);
    }

    #[test]
    fn config_serializes_to_json_and_back() {
        let cfg = SignalGenerationConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: SignalGenerationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.options_staleness_ms, cfg.options_staleness_ms);
        assert_eq!(parsed.signal_ttl_secs, cfg.signal_ttl_secs);
        assert_eq!(parsed.target_notional, cfg.target_notional);
    }
}
