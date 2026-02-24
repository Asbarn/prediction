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
