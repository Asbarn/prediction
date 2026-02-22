use serde::{Deserialize, Serialize};

/// Venue connection configuration loaded from `venues.toml`.
///
/// Contains connection URLs, rate limits, and venue-specific settings.
/// Credentials are NOT stored here -- they come from environment variables
/// via `credentials.rs`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VenuesConfig {
    pub deribit: DeribitConfig,
    pub polymarket: PolymarketConfig,
    pub kalshi: KalshiConfig,
}

/// Reconnection configuration for exponential backoff.
///
/// Used by the reconnection supervisor (Plan 03-02) to control retry
/// behavior after connection drops. All values have sensible defaults.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReconnectConfig {
    /// Initial backoff delay in milliseconds before first retry.
    #[serde(default = "default_initial_backoff")]
    pub initial_backoff_ms: u64,
    /// Maximum backoff delay in milliseconds (caps exponential growth).
    #[serde(default = "default_max_backoff")]
    pub max_backoff_ms: u64,
    /// Randomization factor for jitter (+/- this fraction of the delay).
    #[serde(default = "default_randomization_factor")]
    pub randomization_factor: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_backoff_ms: default_initial_backoff(),
            max_backoff_ms: default_max_backoff(),
            randomization_factor: default_randomization_factor(),
        }
    }
}

fn default_initial_backoff() -> u64 {
    1000
}

fn default_max_backoff() -> u64 {
    60_000
}

fn default_randomization_factor() -> f64 {
    0.5
}

fn default_staleness_threshold() -> u64 {
    5000
}

fn default_heartbeat_interval() -> u64 {
    10_000
}

/// Deribit connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeribitConfig {
    /// WebSocket URL (e.g., "wss://www.deribit.com/ws/api/v2").
    pub ws_url: String,
    /// Maximum API requests per second.
    pub rate_limit_per_second: u32,
    /// Heartbeat interval in milliseconds (minimum 10000 per Deribit docs).
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,
    /// Staleness threshold in milliseconds. Data older than this is marked
    /// `is_stale = true` on `MarketSnapshot` (RELY-03).
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration for exponential backoff with jitter.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// Instrument names to subscribe to (e.g., ["BTC-27JUN25-100000-C"]).
    /// Dynamic -- comes from config in Phase 2, driven by event registry in Phase 5.
    #[serde(default)]
    pub instruments: Vec<String>,
}

/// Polymarket connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolymarketConfig {
    /// WebSocket URL for CLOB subscriptions.
    pub ws_url: String,
    /// REST API base URL.
    pub rest_url: String,
    /// Blockchain chain ID (e.g., 137 for Polygon mainnet).
    pub chain_id: u64,
}

/// Kalshi connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KalshiConfig {
    /// REST API base URL.
    pub rest_url: String,
    /// WebSocket URL.
    pub ws_url: String,
}
