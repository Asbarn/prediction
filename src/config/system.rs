use serde::{Deserialize, Serialize};

use crate::alert::config::AlertConfig;
use crate::pricing::config::PricingConfig;
use crate::settlement::config::SettlementConfig;
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
    /// Health endpoint configuration (Phase 9).
    /// Uses `#[serde(default)]` so existing config files without `[health]` still load.
    #[serde(default)]
    pub health: HealthConfig,
    /// Failure alerting configuration (Phase 14).
    /// Uses `#[serde(default)]` so existing config files without `[alerting]` still load.
    #[serde(default)]
    pub alerting: AlertConfig,
    /// State persistence configuration (Phase 15).
    /// Uses `#[serde(default)]` so existing config files without `[persistence]` still load.
    #[serde(default)]
    pub persistence: PersistenceConfig,
    /// Settlement outcome tracking configuration (Phase 16).
    /// Uses `#[serde(default)]` so existing config files without `[settlement]` still load.
    #[serde(default)]
    pub settlement: SettlementConfig,
    /// Signal analysis configuration (Phase 17).
    /// Uses `#[serde(default)]` so existing config files without `[analysis]` still load.
    #[serde(default)]
    pub analysis: AnalysisConfig,
    /// Subscription management configuration (Phase 24).
    /// Uses `#[serde(default)]` so existing config files without `[subscription]` still load.
    #[serde(default)]
    pub subscription: SubscriptionConfig,
}

/// State persistence configuration for checkpoint-based recovery.
///
/// Controls whether periodic checkpoints are written to disk, where they are
/// stored, and how frequently they are generated.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PersistenceConfig {
    /// Whether state persistence is enabled.
    pub enabled: bool,
    /// Directory for checkpoint files.
    pub checkpoint_dir: String,
    /// How often to write checkpoints (seconds).
    pub checkpoint_interval_secs: u64,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checkpoint_dir: "state".to_string(),
            checkpoint_interval_secs: 60,
        }
    }
}

/// Health endpoint configuration.
///
/// Controls the HTTP `/health` endpoint that reports per-feed connection
/// status, active event count, and system uptime.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HealthConfig {
    /// Port for the HTTP /health endpoint. Default: 9001.
    /// Separate from Prometheus metrics exporter (default port 9000).
    pub port: u16,
    /// Whether to enable the health endpoint.
    pub enabled: bool,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            port: 9001,
            enabled: true,
        }
    }
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

/// Signal analysis configuration (Phase 17).
///
/// Controls whether signal analysis accumulation is active and defines
/// the maximum acceptable inter-leg fill gap for stale fill detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    /// Whether signal analysis is enabled.
    pub enabled: bool,
    /// Maximum acceptable inter-leg fill gap in milliseconds.
    /// Positions with inter_leg_gap_ms exceeding this are flagged as stale fills.
    pub max_leg_fill_gap_ms: i64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_leg_fill_gap_ms: 2000,
        }
    }
}

/// Subscription management configuration (Phase 24).
///
/// Controls dry-run mode for subscription reconciliation. When dry-run is
/// enabled, reconciliation logs diffs and updates internal state but does
/// not send watch channel updates, cleanup events, or emit metrics.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SubscriptionConfig {
    /// Whether dry-run mode is enabled for subscription reconciliation.
    /// When true, diffs are logged and internal state is updated, but no
    /// watch channel sends, cleanup events, or metrics are emitted.
    pub dry_run: bool,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self { dry_run: false }
    }
}
