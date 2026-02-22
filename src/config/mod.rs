mod credentials;
mod events;
pub mod reload;
mod system;
mod validation;
mod venues;

pub use credentials::Credentials;
pub use events::{
    DeribitMapping, Direction, DiscoveryConfig, EventMapping, EventVenues, EventsConfig,
    ExpiryThreshold, KalshiMapping, LifecycleStatus, PolymarketMapping, RiskWeightsConfig,
    SettlementMetadata, SourcePairWeights,
};
pub use system::{LoggingConfig, SignalConfig, StalenessConfig, SystemConfig};
pub use venues::{DeribitConfig, KalshiConfig, PolymarketConfig, VenuesConfig};

use crate::error::ConfigError;
use serde::de::DeserializeOwned;
use std::path::Path;

/// Top-level application configuration.
///
/// Aggregates all configuration sources: three TOML files (system, events,
/// venues) and environment variable credentials. Constructed via `load_config()`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub system: SystemConfig,
    pub events: EventsConfig,
    pub venues: VenuesConfig,
    pub credentials: Credentials,
}

/// Load and validate all configuration from the given directory.
///
/// Reads `config.toml`, `events.toml`, and `venues.toml` from `config_dir`,
/// loads credentials from environment variables, and runs cross-field
/// validation. Returns `ConfigError` with precise error context on any failure.
///
/// # Errors
///
/// - `ConfigError::ReadFile` if any config file cannot be read
/// - `ConfigError::ParseToml` if any file has invalid TOML (includes line/column)
/// - `ConfigError::Validation` if cross-field validation fails
pub fn load_config(config_dir: &Path) -> Result<AppConfig, ConfigError> {
    let system = load_toml::<SystemConfig>(config_dir, "config.toml")?;
    let events = load_toml::<EventsConfig>(config_dir, "events.toml")?;
    let venues = load_toml::<VenuesConfig>(config_dir, "venues.toml")?;
    let credentials = credentials::load_credentials();

    validation::validate_config(&system, &events, &venues)?;

    Ok(AppConfig {
        system,
        events,
        venues,
        credentials,
    })
}

/// Load and deserialize a TOML file into the target type.
///
/// The `toml` 0.8 crate's error type includes line/column span information
/// in its Display output, satisfying the fail-fast requirement for precise
/// error messages without additional work.
fn load_toml<T: DeserializeOwned>(dir: &Path, filename: &str) -> Result<T, ConfigError> {
    let path = dir.join(filename);
    let content = std::fs::read_to_string(&path).map_err(|e| ConfigError::ReadFile {
        file: filename.to_string(),
        source: e,
    })?;
    toml::from_str(&content).map_err(|e| ConfigError::ParseToml {
        file: filename.to_string(),
        source: e,
    })
}
