use crate::error::ConfigError;
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;

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
            || event.venues.kalshi.is_some()
            || event.venues.derive.is_some();

        if !has_venue {
            return Err(ConfigError::Validation {
                file: "events.toml".to_string(),
                message: format!(
                    "event '{}' has no venue mappings configured -- at least one venue is required",
                    event.id
                ),
            });
        }

        // Validate expiry date is parseable
        if NaiveDate::parse_from_str(&event.expiry, "%Y-%m-%d").is_err() {
            return Err(ConfigError::Validation {
                file: "events.toml".to_string(),
                message: format!(
                    "event '{}' has invalid expiry date '{}' -- expected YYYY-MM-DD format",
                    event.id, event.expiry
                ),
            });
        }

        // Validate strike is parseable to Decimal
        if Decimal::from_str(&event.strike).is_err() {
            return Err(ConfigError::Validation {
                file: "events.toml".to_string(),
                message: format!(
                    "event '{}' has invalid strike '{}' -- must be a valid decimal number",
                    event.id, event.strike
                ),
            });
        }

        // Approved mapping safety: at least 2 venues required for cross-venue arbitrage
        if event.approved {
            let venue_count = [
                event.venues.deribit.is_some(),
                event.venues.polymarket.is_some(),
                event.venues.kalshi.is_some(),
                event.venues.derive.is_some(),
            ]
            .iter()
            .filter(|&&v| v)
            .count();

            if venue_count < 2 {
                return Err(ConfigError::Validation {
                    file: "events.toml".to_string(),
                    message: format!(
                        "approved event '{}' has only {} venue(s) -- at least 2 required for cross-venue arbitrage",
                        event.id, venue_count
                    ),
                });
            }
        }

        // Approved mapping safety: expiry must not be in the past
        // (strict less-than: expiring today is still valid -- Deribit settles at 08:00 UTC)
        if event.approved {
            if let Ok(expiry_date) = NaiveDate::parse_from_str(&event.expiry, "%Y-%m-%d") {
                let today = Utc::now().date_naive();
                if expiry_date < today {
                    return Err(ConfigError::Validation {
                        file: "events.toml".to_string(),
                        message: format!(
                            "approved event '{}' has expired (expiry {} is before today {})",
                            event.id, expiry_date, today
                        ),
                    });
                }
            }
        }
    }

    // Validate expiry thresholds: risk_inflation_factor >= 1.0 and unique hours
    if !events.expiry_thresholds.is_empty() {
        for threshold in &events.expiry_thresholds {
            if threshold.risk_inflation_factor < 1.0 {
                return Err(ConfigError::Validation {
                    file: "events.toml".to_string(),
                    message: format!(
                        "expiry threshold '{}' has risk_inflation_factor {} < 1.0",
                        threshold.name, threshold.risk_inflation_factor
                    ),
                });
            }
        }

        // Ensure no duplicate hours_before_expiry values
        let mut seen_hours = std::collections::HashSet::new();
        for threshold in &events.expiry_thresholds {
            if !seen_hours.insert(threshold.hours_before_expiry) {
                return Err(ConfigError::Validation {
                    file: "events.toml".to_string(),
                    message: format!(
                        "expiry threshold '{}' has duplicate hours_before_expiry {}",
                        threshold.name, threshold.hours_before_expiry
                    ),
                });
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DeriveConfig, DeribitConfig, DeribitMapping, Direction, EventMapping, EventVenues,
        EventsConfig, KalshiConfig, KalshiMapping, LifecycleStatus, PolymarketConfig,
        PolymarketMapping, SystemConfig, VenuesConfig,
    };

    /// Construct a minimal valid SystemConfig for validation tests.
    fn make_system() -> SystemConfig {
        toml::from_str(
            r#"
            [logging]
            log_dir = "logs"
            stdout_level = "info"
            file_level = "debug"

            [staleness]
            threshold_ms = 5000

            [signals]
            min_spread_bps = 50
            cooldown_ms = 10000
            "#,
        )
        .unwrap()
    }

    /// Construct a minimal valid VenuesConfig for validation tests.
    fn make_venues() -> VenuesConfig {
        VenuesConfig {
            deribit: DeribitConfig {
                ws_url: "wss://www.deribit.com/ws/api/v2".to_string(),
                rate_limit_per_second: 10,
                heartbeat_interval_ms: 10_000,
                staleness_threshold_ms: 5000,
                reconnect: Default::default(),
                instruments: vec![],
                book_depth_levels: 20,
            },
            polymarket: PolymarketConfig {
                ws_url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string(),
                rest_url: "https://clob.polymarket.com".to_string(),
                chain_id: 137,
                gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
                staleness_threshold_ms: 5000,
                reconnect: Default::default(),
                assets: vec![],
                rate_limit_per_second: 10,
                ping_interval_ms: 10_000,
                data_timeout_secs: 120,
                rest_poll_interval_secs: 5,
                ws_recovery_check_secs: 60,
                ws_recovery_threshold: 3,
            },
            kalshi: KalshiConfig {
                rest_url: "https://trading-api.kalshi.com".to_string(),
                ws_url: "wss://trading-api.kalshi.com/trade-api/ws/v2".to_string(),
                staleness_threshold_ms: 5000,
                reconnect: Default::default(),
                rate_limit_per_second: 10,
                market_tickers: vec![],
                private_key_path: None,
                heartbeat_timeout_ms: 30_000,
            },
            derive: DeriveConfig {
                ws_url: "wss://api.lyra.finance/ws".to_string(),
                rate_limit_per_second: 10,
                book_depth_levels: 20,
                staleness_threshold_ms: 5000,
                reconnect: Default::default(),
                instruments: vec![],
            },
        }
    }

    /// Construct a minimal valid EventsConfig with a single event.
    fn make_events_with(events: Vec<EventMapping>) -> EventsConfig {
        EventsConfig {
            events,
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        }
    }

    /// Construct an EventMapping with configurable approved flag and venue count.
    fn make_mapping(
        id: &str,
        approved: bool,
        deribit: bool,
        polymarket: bool,
        kalshi: bool,
        expiry: &str,
    ) -> EventMapping {
        EventMapping {
            id: id.to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: expiry.to_string(),
            venues: EventVenues {
                deribit: if deribit {
                    Some(DeribitMapping {
                        instrument: "BTC-27JUN30-100000-C".to_string(),
                    })
                } else {
                    None
                },
                polymarket: if polymarket {
                    Some(PolymarketMapping {
                        condition_id: "0xabc123".to_string(),
                        token_id: "12345".to_string(),
                    })
                } else {
                    None
                },
                kalshi: if kalshi {
                    Some(KalshiMapping {
                        ticker: "KXBTC-30JUN30-T100000".to_string(),
                    })
                } else {
                    None
                },
                derive: None,
            },
            approved,
            status: LifecycleStatus::Active,
            discovered_at: None,
            settlement: None,
        }
    }

    #[test]
    fn test_approved_single_venue_rejected() {
        let system = make_system();
        let venues = make_venues();
        let events = make_events_with(vec![make_mapping(
            "BTC-100K-SINGLE",
            true,  // approved
            true,  // deribit only
            false,
            false,
            "2030-01-01",
        )]);

        let result = validate_config(&system, &events, &venues);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("at least 2 required"),
            "expected venue count error, got: {err}"
        );
    }

    #[test]
    fn test_approved_two_venues_accepted() {
        let system = make_system();
        let venues = make_venues();
        let events = make_events_with(vec![make_mapping(
            "BTC-100K-TWO-VENUES",
            true,  // approved
            true,  // deribit
            true,  // polymarket
            false,
            "2030-01-01",
        )]);

        let result = validate_config(&system, &events, &venues);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    }

    #[test]
    fn test_approved_expired_rejected() {
        let system = make_system();
        let venues = make_venues();
        let events = make_events_with(vec![make_mapping(
            "BTC-100K-EXPIRED",
            true,  // approved
            true,  // deribit
            true,  // polymarket
            false,
            "2020-01-01", // past expiry
        )]);

        let result = validate_config(&system, &events, &venues);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("has expired"),
            "expected expiry error, got: {err}"
        );
    }

    #[test]
    fn test_unapproved_single_venue_accepted() {
        let system = make_system();
        let venues = make_venues();
        let events = make_events_with(vec![make_mapping(
            "BTC-100K-CANDIDATE",
            false, // unapproved
            true,  // deribit only (1 venue)
            false,
            false,
            "2030-01-01",
        )]);

        let result = validate_config(&system, &events, &venues);
        assert!(result.is_ok(), "unapproved single-venue should pass, got: {:?}", result);
    }
}
