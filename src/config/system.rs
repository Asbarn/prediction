use serde::{Deserialize, Serialize};

/// System-wide configuration loaded from `config.toml`.
///
/// Contains logging settings, staleness thresholds, and signal generation
/// parameters. All required fields must be present -- no `#[serde(default)]`
/// to enforce fail-fast behavior.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemConfig {
    pub logging: LoggingConfig,
    pub staleness: StalenessConfig,
    pub signals: SignalConfig,
}

/// Logging output configuration.
///
/// Controls the dual-output logging system: human-readable stdout
/// and structured JSON to a daily-rotating file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    /// Path to the log directory (relative to working dir or absolute).
    pub log_dir: String,
    /// Filter level for stdout output (e.g., "info", "warn").
    pub stdout_level: String,
    /// Filter level for file output (e.g., "debug", "trace").
    pub file_level: String,
}

/// Data staleness detection configuration.
///
/// Controls how old market data can be before it is considered stale
/// and excluded from spread calculations.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StalenessConfig {
    /// Default staleness threshold in milliseconds.
    /// Data older than this is considered stale.
    pub threshold_ms: u64,
    /// Optional clock skew tolerance in milliseconds.
    /// If set, allows for this much difference between local and venue clocks.
    pub max_skew_ms: Option<u64>,
}

/// Signal generation configuration.
///
/// Controls thresholds and rate limiting for arbitrage signal generation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignalConfig {
    /// Minimum spread in basis points to trigger a signal.
    pub min_spread_bps: u64,
    /// Cooldown period in milliseconds -- don't re-signal the same event
    /// within this window.
    pub cooldown_ms: u64,
}
