//! Deribit resolution checker implementation.
//!
//! Queries the public `get_delivery_prices` API to determine binary option
//! outcomes by comparing the TWAP delivery price against the strike.

use rust_decimal::Decimal;
use tracing::debug;

use crate::config::Direction;
use crate::feed::reliability::rate_limiter::VenueRateLimiter;

use super::traits::CheckContext;
use super::types::{OutcomeKind, ResolutionResult};

/// Response from Deribit public/get_delivery_prices endpoint.
#[derive(Debug, serde::Deserialize)]
struct DeribitDeliveryResponse {
    result: DeribitDeliveryResult,
}

/// Inner result containing delivery price data.
#[derive(Debug, serde::Deserialize)]
struct DeribitDeliveryResult {
    data: Vec<DeribitDeliveryEntry>,
}

/// A single delivery price entry keyed by date.
#[derive(Debug, serde::Deserialize)]
struct DeribitDeliveryEntry {
    /// Date string in "YYYY-MM-DD" or millisecond timestamp format.
    date: serde_json::Value,
    /// TWAP-based delivery/settlement price.
    delivery_price: f64,
}

/// Resolution checker for Deribit options settlement.
///
/// Queries the public `get_delivery_prices` endpoint (no auth required),
/// matches by expiry date, and determines binary outcome by comparing
/// delivery price against strike.
pub struct DeribitResolutionChecker {
    client: reqwest::Client,
    base_url: String,
    rate_limiter: VenueRateLimiter,
}

impl DeribitResolutionChecker {
    /// Create a new Deribit resolution checker.
    pub fn new(client: reqwest::Client, base_url: String, rate_limiter: VenueRateLimiter) -> Self {
        Self {
            client,
            base_url,
            rate_limiter,
        }
    }

    /// Check resolution status for a Deribit instrument.
    ///
    /// Derives `index_name` from the asset (e.g., "BTC" -> "btc_usd"),
    /// queries delivery prices, and matches by expiry date.
    pub async fn check_resolution(
        &self,
        _event_id: &str,
        _venue_instrument: &str,
        context: &CheckContext,
    ) -> anyhow::Result<ResolutionResult> {
        let index_name = format!("{}_usd", context.asset.to_lowercase());

        self.rate_limiter.wait().await;

        let url = format!(
            "{}/api/v2/public/get_delivery_prices",
            self.base_url
        );
        let resp = self
            .client
            .get(&url)
            .query(&[("index_name", &index_name), ("count", &"30".to_string())])
            .send()
            .await?;

        let body = resp.text().await?;
        let parsed: DeribitDeliveryResponse = serde_json::from_str(&body)?;

        // Match entry by expiry date
        for entry in &parsed.result.data {
            let entry_date = match &entry.date {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => {
                    // Deribit may return date as millisecond timestamp
                    if let Some(ms) = n.as_i64() {
                        let dt = chrono::DateTime::from_timestamp_millis(ms);
                        match dt {
                            Some(d) => d.format("%Y-%m-%d").to_string(),
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            if entry_date == context.expiry {
                let delivery = Decimal::from_f64_retain(entry.delivery_price)
                    .unwrap_or_default();
                let outcome = determine_outcome(delivery, context.strike, &context.direction);

                debug!(
                    expiry = %context.expiry,
                    delivery_price = %delivery,
                    strike = %context.strike,
                    direction = %context.direction,
                    outcome = ?outcome,
                    "Deribit delivery price matched"
                );

                return Ok(ResolutionResult::Resolved {
                    outcome,
                    settlement_price: Some(delivery),
                    resolved_at: chrono::Utc::now(),
                });
            }
        }

        debug!(
            expiry = %context.expiry,
            index_name = %index_name,
            entries = parsed.result.data.len(),
            "No matching Deribit delivery date found"
        );

        Ok(ResolutionResult::NotYetResolved)
    }
}

/// Determine the binary outcome by comparing delivery price against strike.
fn determine_outcome(delivery: Decimal, strike: Decimal, direction: &Direction) -> OutcomeKind {
    match direction {
        Direction::Above => {
            if delivery >= strike {
                OutcomeKind::Yes
            } else {
                OutcomeKind::No
            }
        }
        Direction::Below => {
            if delivery <= strike {
                OutcomeKind::Yes
            } else {
                OutcomeKind::No
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn delivery_above_strike_is_yes_for_above_direction() {
        let outcome = determine_outcome(dec!(105000), dec!(100000), &Direction::Above);
        assert_eq!(outcome, OutcomeKind::Yes);
    }

    #[test]
    fn delivery_below_strike_is_no_for_above_direction() {
        let outcome = determine_outcome(dec!(95000), dec!(100000), &Direction::Above);
        assert_eq!(outcome, OutcomeKind::No);
    }

    #[test]
    fn delivery_at_strike_is_yes_for_above_direction() {
        let outcome = determine_outcome(dec!(100000), dec!(100000), &Direction::Above);
        assert_eq!(outcome, OutcomeKind::Yes);
    }

    #[test]
    fn delivery_below_strike_is_yes_for_below_direction() {
        let outcome = determine_outcome(dec!(95000), dec!(100000), &Direction::Below);
        assert_eq!(outcome, OutcomeKind::Yes);
    }

    #[test]
    fn delivery_above_strike_is_no_for_below_direction() {
        let outcome = determine_outcome(dec!(105000), dec!(100000), &Direction::Below);
        assert_eq!(outcome, OutcomeKind::No);
    }

    #[test]
    fn delivery_at_strike_is_yes_for_below_direction() {
        let outcome = determine_outcome(dec!(100000), dec!(100000), &Direction::Below);
        assert_eq!(outcome, OutcomeKind::Yes);
    }

    #[test]
    fn parse_delivery_response_and_match_date() {
        let json = r#"{
            "result": {
                "data": [
                    {"date": "2025-06-28", "delivery_price": 98765.43},
                    {"date": "2025-06-27", "delivery_price": 102345.67},
                    {"date": "2025-06-26", "delivery_price": 99000.00}
                ]
            }
        }"#;

        let parsed: DeribitDeliveryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.result.data.len(), 3);

        // Find matching date
        let target_date = "2025-06-27";
        let matching = parsed
            .result
            .data
            .iter()
            .find(|e| {
                if let serde_json::Value::String(s) = &e.date {
                    s == target_date
                } else {
                    false
                }
            });

        assert!(matching.is_some());
        let entry = matching.unwrap();
        assert!((entry.delivery_price - 102345.67).abs() < 0.01);
    }

    #[test]
    fn no_matching_date_returns_none() {
        let json = r#"{
            "result": {
                "data": [
                    {"date": "2025-06-28", "delivery_price": 98765.43},
                    {"date": "2025-06-26", "delivery_price": 99000.00}
                ]
            }
        }"#;

        let parsed: DeribitDeliveryResponse = serde_json::from_str(json).unwrap();
        let target_date = "2025-06-27";
        let matching = parsed
            .result
            .data
            .iter()
            .find(|e| {
                if let serde_json::Value::String(s) = &e.date {
                    s == target_date
                } else {
                    false
                }
            });

        assert!(matching.is_none());
    }
}
