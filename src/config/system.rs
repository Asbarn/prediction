use serde::{Deserialize, Serialize};

use crate::pricing::config::PricingConfig;
use crate::signal::config::SignalGenerationConfig;
use crate::spread::config::SpreadConfig;

/// System-wide configuration loaded from `config.toml`.
///
/// Contains logging settings, staleness thresholds, signal generation
/// parameters, and spread computation configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemConfig {
    pub logging: LoggingConfig,
    pub staleness: StalenessConfig,
    pub signals: SignalConfig,
    /// Spread computation configuration (fee models, thresholds, rolling stats).
    /// Uses `#[serde(default)]` so existing config files without `[spread]` still load.
    #[serde(default)]
    pub spread: SpreadConfig,
    /// Prometheus metrics exporter configuration.
    #[serde(default)]
    pub prometheus: PrometheusConfig,
    /// Paper trade tracker configuration (placeholder for Plan 04).
    #[serde(default)]
    pub paper_trade: PaperTradeConfig,
    /// Options pricing engine configuration (Phase 7).
    #[serde(default)]
    pub pricing: PricingConfig,
    /// Cross-asset signal generation configuration (Phase 8).
    /// Uses `#[serde(default)]` so existing config files without `[signal_generation]` still load.
    #[serde(default)]
    pub signal_generation: SignalGenerationConfig,
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

/// Prometheus metrics exporter configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PrometheusConfig {
    /// Port for the Prometheus HTTP scrape endpoint.
    pub port: u16,
}

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self { port: 9000 }
    }
}

/// Paper trade tracker configuration (placeholder for Plan 04).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PaperTradeConfig {
    /// Fixed notional per paper trade.
    #[serde(with = "rust_decimal::serde::str")]
    pub notional_per_trade: rust_decimal::Decimal,
    /// Whether to log mark-to-market values over position lifetime.
    pub log_mtm: bool,
    /// Output directory for paper trade JSONL logs.
    pub log_dir: String,
}

impl Default for PaperTradeConfig {
    fn default() -> Self {
        Self {
            notional_per_trade: rust_decimal::Decimal::new(500, 0),
            log_mtm: true,
            log_dir: "paper_trades".to_string(),
        }
    }
}
