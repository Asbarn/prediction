//! Alert system configuration.
//!
//! `AlertConfig` is loaded from the `[alerting]` section of `config.toml`.
//! All fields have sensible defaults so existing configs without `[alerting]`
//! continue to work via `#[serde(default)]`.

use serde::{Deserialize, Serialize};

/// Configuration for the failure alerting system.
///
/// Controls detection thresholds, check frequency, and cooldown periods
/// for the `AlertMonitor` sweep loop.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct AlertConfig {
    /// Whether the alerting system is enabled.
    pub enabled: bool,
    /// How often the AlertMonitor runs its evaluation sweep (seconds).
    pub check_interval_secs: u64,
    /// Seconds of no messages from a connected venue before alerting.
    pub feed_silence_threshold_secs: u64,
    /// Number of venues expected to be active.
    pub expected_venue_count: usize,
    /// Seconds of no signal evaluations before alerting.
    pub signal_gap_threshold_secs: u64,
    /// Seconds since last pipeline stage update before alerting.
    pub stage_liveness_threshold_secs: u64,
    /// Minimum seconds between repeated `tracing::warn!` for the same condition.
    pub alert_cooldown_secs: u64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 30,
            feed_silence_threshold_secs: 120,
            expected_venue_count: 3,
            signal_gap_threshold_secs: 300,
            stage_liveness_threshold_secs: 180,
            alert_cooldown_secs: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let config = AlertConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.feed_silence_threshold_secs, 120);
        assert_eq!(config.expected_venue_count, 3);
        assert_eq!(config.signal_gap_threshold_secs, 300);
        assert_eq!(config.stage_liveness_threshold_secs, 180);
        assert_eq!(config.alert_cooldown_secs, 300);
    }

    #[test]
    fn serde_round_trip() {
        let config = AlertConfig::default();
        let toml_str = toml::to_string(&config).expect("serialize");
        let deserialized: AlertConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml_str = r#"enabled = false"#;
        let config: AlertConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(!config.enabled);
        // All other fields should get defaults
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.feed_silence_threshold_secs, 120);
        assert_eq!(config.expected_venue_count, 3);
        assert_eq!(config.signal_gap_threshold_secs, 300);
        assert_eq!(config.stage_liveness_threshold_secs, 180);
        assert_eq!(config.alert_cooldown_secs, 300);
    }

    #[test]
    fn empty_toml_uses_all_defaults() {
        let toml_str = "";
        let config: AlertConfig = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(config, AlertConfig::default());
    }
}
