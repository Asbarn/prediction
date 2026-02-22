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

/// Deribit connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeribitConfig {
    /// WebSocket URL (e.g., "wss://www.deribit.com/ws/api/v2").
    pub ws_url: String,
    /// Maximum API requests per second.
    pub rate_limit_per_second: u32,
    /// Heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: u64,
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
