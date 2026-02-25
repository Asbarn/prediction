//! Settlement monitoring configuration.
//!
//! Loaded from TOML via serde(default). All fields have sensible defaults
//! matching the four-tier polling cadence from the CONTEXT.md specification.

use serde::{Deserialize, Serialize};

/// Configuration for the settlement monitoring subsystem.
///
/// All timing parameters are per-venue configurable. Defaults match the
/// polling cadence specification:
/// - Aggressive (0-4h): every 2 minutes
/// - Patient (4-96h): every 15 minutes
/// - Lazy (96h-7d): every 2 hours
/// - Timeout at 7 days
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettlementConfig {
    /// Whether settlement monitoring is enabled.
    pub enabled: bool,
    /// Base tick interval for the monitor loop (seconds).
    pub base_poll_interval_secs: u64,
    /// Aggressive tier polling interval (seconds). Default: 120 (2 min).
    pub aggressive_interval_secs: u64,
    /// Patient tier polling interval (seconds). Default: 900 (15 min).
    pub patient_interval_secs: u64,
    /// Lazy tier polling interval (seconds). Default: 7200 (2 hours).
    pub lazy_interval_secs: u64,
    /// Maximum polling duration before timeout (hours). Default: 168 (7 days).
    pub timeout_hours: u64,
    /// Duration of aggressive polling tier (hours). Default: 4.
    pub aggressive_duration_hours: u64,
    /// Duration of patient polling tier (hours). Default: 96.
    pub patient_duration_hours: u64,
    /// Polymarket price lock threshold for two-stage check. Default: 0.95.
    pub polymarket_price_lock_threshold: f64,
    /// Maximum age for backfill on startup (days). Default: 7.
    pub max_backfill_age_days: u64,
    /// Directory for settlement JSONL logs. Default: "settlement_logs".
    pub settlement_log_dir: String,
}

impl Default for SettlementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_poll_interval_secs: 60,
            aggressive_interval_secs: 120,
            patient_interval_secs: 900,
            lazy_interval_secs: 7200,
            timeout_hours: 168,
            aggressive_duration_hours: 4,
            patient_duration_hours: 96,
            polymarket_price_lock_threshold: 0.95,
            max_backfill_age_days: 7,
            settlement_log_dir: "settlement_logs".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_reasonable_values() {
        let config = SettlementConfig::default();

        assert!(config.enabled);
        assert_eq!(config.base_poll_interval_secs, 60);
        assert_eq!(config.aggressive_interval_secs, 120);
        assert_eq!(config.patient_interval_secs, 900);
        assert_eq!(config.lazy_interval_secs, 7200);
        assert_eq!(config.timeout_hours, 168); // 7 days
        assert_eq!(config.aggressive_duration_hours, 4);
        assert_eq!(config.patient_duration_hours, 96);
        assert!((config.polymarket_price_lock_threshold - 0.95).abs() < f64::EPSILON);
        assert_eq!(config.max_backfill_age_days, 7);
        assert_eq!(config.settlement_log_dir, "settlement_logs");
    }

    #[test]
    fn config_deserializes_from_empty_toml() {
        let config: SettlementConfig = toml::from_str("").expect("empty TOML");
        assert!(config.enabled);
        assert_eq!(config.base_poll_interval_secs, 60);
    }

    #[test]
    fn config_deserializes_partial_toml() {
        let toml_str = r#"
            enabled = false
            aggressive_interval_secs = 60
        "#;
        let config: SettlementConfig = toml::from_str(toml_str).expect("partial TOML");
        assert!(!config.enabled);
        assert_eq!(config.aggressive_interval_secs, 60);
        // Non-specified fields get defaults
        assert_eq!(config.patient_interval_secs, 900);
        assert_eq!(config.timeout_hours, 168);
    }

    #[test]
    fn config_serde_json_roundtrip() {
        let config = SettlementConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: SettlementConfig =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(config.enabled, deserialized.enabled);
        assert_eq!(
            config.base_poll_interval_secs,
            deserialized.base_poll_interval_secs
        );
        assert_eq!(config.timeout_hours, deserialized.timeout_hours);
    }
}
