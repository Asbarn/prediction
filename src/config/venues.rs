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
    pub derive: DeriveConfig,
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

fn default_book_depth_levels() -> u32 {
    20
}

fn default_staleness_threshold() -> u64 {
    5000
}

fn default_heartbeat_interval() -> u64 {
    10_000
}

fn default_gamma_api_url() -> String {
    "https://gamma-api.polymarket.com".to_string()
}

fn default_polymarket_rate_limit() -> u32 {
    10
}

fn default_polymarket_ping_interval() -> u64 {
    10_000
}

fn default_data_timeout_secs() -> u64 {
    120
}

fn default_rest_poll_interval() -> u64 {
    5
}

fn default_ws_recovery_check_secs() -> u64 {
    60
}

fn default_ws_recovery_threshold() -> u32 {
    3
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
    /// Number of book depth levels for grouped book subscription.
    /// Valid Deribit values: 1, 10, 20. Default: 20.
    #[serde(default = "default_book_depth_levels")]
    pub book_depth_levels: u32,
}

/// A Polymarket asset (token) to subscribe to.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolymarketAsset {
    /// Condition ID (market-level identifier).
    pub condition_id: String,
    /// Token/asset ID (outcome-level identifier, used for WS subscription).
    pub token_id: String,
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
    /// Gamma API URL for condition_id to token_id resolution.
    #[serde(default = "default_gamma_api_url")]
    pub gamma_api_url: String,
    /// Staleness threshold in milliseconds. Data older than this is marked
    /// `is_stale = true` on `MarketSnapshot`.
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration for exponential backoff with jitter.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// Assets (token IDs) to subscribe to on the market channel.
    #[serde(default)]
    pub assets: Vec<PolymarketAsset>,
    /// Maximum API requests per second.
    #[serde(default = "default_polymarket_rate_limit")]
    pub rate_limit_per_second: u32,
    /// PING interval in milliseconds (Polymarket requires PING every 10s).
    #[serde(default = "default_polymarket_ping_interval")]
    pub ping_interval_ms: u64,
    /// Data inactivity timeout in seconds. If no order book data arrives within
    /// this period, the supervisor forces a reconnect. Detects silent freezes
    /// (GitHub #292).
    #[serde(default = "default_data_timeout_secs")]
    pub data_timeout_secs: u64,
    /// REST polling interval in seconds (how often to poll /midpoint when in REST mode).
    #[serde(default = "default_rest_poll_interval")]
    pub rest_poll_interval_secs: u64,
    /// How often to attempt WS reconnection while in REST mode (seconds).
    #[serde(default = "default_ws_recovery_check_secs")]
    pub ws_recovery_check_secs: u64,
    /// Number of WS messages needed to confirm WS is recovered before switching back.
    #[serde(default = "default_ws_recovery_threshold")]
    pub ws_recovery_threshold: u32,
}

/// Kalshi connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KalshiConfig {
    /// REST API base URL.
    pub rest_url: String,
    /// WebSocket URL.
    pub ws_url: String,
    /// Staleness threshold in milliseconds. Data older than this is marked
    /// `is_stale = true` on `MarketSnapshot` (RELY-03).
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration for exponential backoff with jitter.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// Maximum API requests per second.
    #[serde(default = "default_kalshi_rate_limit")]
    pub rate_limit_per_second: u32,
    /// Kalshi market tickers to subscribe (e.g., ["KXBTC-26FEB22-T100000"]).
    #[serde(default)]
    pub market_tickers: Vec<String>,
    /// Path to RSA private key PEM file (alternative to KALSHI_PRIVATE_KEY env var).
    pub private_key_path: Option<String>,
    /// Heartbeat timeout in milliseconds. If no message (including WS Ping/Pong)
    /// is received within this duration, the connection is assumed dead. Default
    /// 30000ms (3x Kalshi's 10s Ping interval).
    #[serde(default = "default_kalshi_heartbeat_timeout")]
    pub heartbeat_timeout_ms: u64,
}

fn default_kalshi_rate_limit() -> u32 {
    10
}

fn default_kalshi_heartbeat_timeout() -> u64 {
    30_000
}

/// Derive connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveConfig {
    /// WebSocket URL (e.g., "wss://api.lyra.finance/ws").
    pub ws_url: String,
    /// Maximum API requests per second.
    #[serde(default = "default_derive_rate_limit")]
    pub rate_limit_per_second: u32,
    /// Number of book depth levels for order book subscription.
    /// Default: 20.
    #[serde(default = "default_book_depth_levels")]
    pub book_depth_levels: u32,
    /// Staleness threshold in milliseconds. Data older than this is marked
    /// `is_stale = true` on `MarketSnapshot`.
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration for exponential backoff with jitter.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// Instrument names to subscribe to (e.g., ["BTC-20250627-100000-C"]).
    #[serde(default)]
    pub instruments: Vec<String>,
}

fn default_derive_rate_limit() -> u32 {
    10
}
