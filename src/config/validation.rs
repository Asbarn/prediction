use crate::error::ConfigError;

use super::events::EventsConfig;
use super::system::SystemConfig;
use super::venues::VenuesConfig;

/// Validate cross-field configuration constraints.
///
/// Called after all config files are parsed. Catches semantic errors
/// that TOML deserialization alone cannot detect (e.g., zero thresholds,
/// events with no venue mappings, invalid URL schemes).
pub fn validate_config(
    system: &SystemConfig,
    events: &EventsConfig,
    venues: &VenuesConfig,
) -> Result<(), ConfigError> {
    // System validation
    if system.staleness.threshold_ms == 0 {
        return Err(ConfigError::Validation {
            file: "config.toml".to_string(),
            message: "staleness.threshold_ms must be greater than 0".to_string(),
        });
    }

    if system.signals.min_spread_bps == 0 {
        return Err(ConfigError::Validation {
            file: "config.toml".to_string(),
            message: "signals.min_spread_bps must be greater than 0".to_string(),
        });
    }

    if system.signals.cooldown_ms == 0 {
        return Err(ConfigError::Validation {
            file: "config.toml".to_string(),
            message: "signals.cooldown_ms must be greater than 0".to_string(),
        });
    }

    // Event validation: each event must have at least one venue mapping
    for event in &events.events {
        let has_venue = event.venues.deribit.is_some()
            || event.venues.polymarket.is_some()
            || event.venues.kalshi.is_some();

        if !has_venue {
            return Err(ConfigError::Validation {
                file: "events.toml".to_string(),
                message: format!(
                    "event '{}' has no venue mappings configured -- at least one venue is required",
                    event.id
                ),
            });
        }
    }

    // Venue URL validation
    validate_ws_url(&venues.deribit.ws_url, "venues.toml", "deribit.ws_url")?;
    validate_ws_url(&venues.polymarket.ws_url, "venues.toml", "polymarket.ws_url")?;
    validate_https_url(&venues.polymarket.rest_url, "venues.toml", "polymarket.rest_url")?;
    validate_https_url(&venues.kalshi.rest_url, "venues.toml", "kalshi.rest_url")?;
    validate_ws_url(&venues.kalshi.ws_url, "venues.toml", "kalshi.ws_url")?;

    Ok(())
}

/// Validate that a URL starts with "wss://".
fn validate_ws_url(url: &str, file: &str, field: &str) -> Result<(), ConfigError> {
    if !url.starts_with("wss://") {
        return Err(ConfigError::Validation {
            file: file.to_string(),
            message: format!("{field} must start with 'wss://', got '{url}'"),
        });
    }
    Ok(())
}

/// Validate that a URL starts with "https://".
fn validate_https_url(url: &str, file: &str, field: &str) -> Result<(), ConfigError> {
    if !url.starts_with("https://") {
        return Err(ConfigError::Validation {
            file: file.to_string(),
            message: format!("{field} must start with 'https://', got '{url}'"),
        });
    }
    Ok(())
}
