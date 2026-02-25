//! Kalshi resolution checker implementation.
//!
//! Queries the GET /markets/{ticker} REST API with RSA-PSS authentication
//! to determine market settlement status and outcome.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tracing::debug;

use crate::feed::kalshi::auth::sign_kalshi_request;
use crate::feed::reliability::rate_limiter::VenueRateLimiter;

use super::traits::CheckContext;
use super::types::{OutcomeKind, ResolutionResult};

/// Response wrapper from Kalshi GET /markets/{ticker}.
#[derive(Debug, serde::Deserialize)]
struct KalshiMarketResponse {
    market: KalshiMarketDetail,
}

/// Detailed market information from Kalshi.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct KalshiMarketDetail {
    ticker: String,
    status: String,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    settlement_value_dollars: Option<String>,
    #[serde(default)]
    settlement_ts: Option<String>,
}

/// Resolution checker for Kalshi markets.
///
/// Uses RSA-PSS signed requests (reusing the existing `sign_kalshi_request`
/// from `feed::kalshi::auth`) to query market settlement status.
pub struct KalshiResolutionChecker {
    client: reqwest::Client,
    rest_url: String,
    api_key_id: String,
    private_key: rsa::RsaPrivateKey,
    rate_limiter: VenueRateLimiter,
}

impl KalshiResolutionChecker {
    /// Create a new Kalshi resolution checker.
    pub fn new(
        client: reqwest::Client,
        rest_url: String,
        api_key_id: String,
        private_key: rsa::RsaPrivateKey,
        rate_limiter: VenueRateLimiter,
    ) -> Self {
        Self {
            client,
            rest_url,
            api_key_id,
            private_key,
            rate_limiter,
        }
    }

    /// Check resolution status for a Kalshi market.
    ///
    /// Queries GET /trade-api/v2/markets/{ticker} with RSA-PSS auth headers.
    /// Handles the multi-stage status lifecycle:
    /// - "determined" | "finalized" -> check result field
    /// - "disputed" -> Disputed
    /// - others -> NotYetResolved
    pub async fn check_resolution(
        &self,
        _event_id: &str,
        venue_instrument: &str,
        _context: &CheckContext,
    ) -> anyhow::Result<ResolutionResult> {
        let path = format!("/trade-api/v2/markets/{}", venue_instrument);
        let timestamp_ms = Utc::now().timestamp_millis();
        let signature = sign_kalshi_request(&self.private_key, timestamp_ms, "GET", &path)?;

        self.rate_limiter.wait().await;

        let resp = self
            .client
            .get(format!("{}{}", self.rest_url, path))
            .header("KALSHI-ACCESS-KEY", &self.api_key_id)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
            .send()
            .await?;

        let body = resp.text().await?;
        let parsed: KalshiMarketResponse = serde_json::from_str(&body)?;
        let market = &parsed.market;

        debug!(
            ticker = %market.ticker,
            status = %market.status,
            result = ?market.result,
            settlement_value = ?market.settlement_value_dollars,
            "Kalshi market status"
        );

        dispatch_kalshi_status(market)
    }
}

/// Determine resolution result from Kalshi market status and result fields.
fn dispatch_kalshi_status(market: &KalshiMarketDetail) -> anyhow::Result<ResolutionResult> {
    match market.status.as_str() {
        "determined" | "finalized" => {
            let settlement_price = parse_settlement_value(&market.settlement_value_dollars);
            let resolved_at = parse_settlement_ts(&market.settlement_ts);

            match market.result.as_deref() {
                Some("yes") => {
                    // Check for scalar-like settlement (value not binary 0 or 1)
                    if is_scalar_settlement(&settlement_price) {
                        return Ok(ResolutionResult::Ambiguous {
                            raw_data: serde_json::to_string(market).unwrap_or_default(),
                        });
                    }
                    Ok(ResolutionResult::Resolved {
                        outcome: OutcomeKind::Yes,
                        settlement_price,
                        resolved_at,
                    })
                }
                Some("no") => {
                    if is_scalar_settlement(&settlement_price) {
                        return Ok(ResolutionResult::Ambiguous {
                            raw_data: serde_json::to_string(market).unwrap_or_default(),
                        });
                    }
                    Ok(ResolutionResult::Resolved {
                        outcome: OutcomeKind::No,
                        settlement_price,
                        resolved_at,
                    })
                }
                Some("scalar") => {
                    // Kalshi Rule 6.3(c): ambiguous resolution at last-traded price
                    Ok(ResolutionResult::Ambiguous {
                        raw_data: serde_json::to_string(market).unwrap_or_default(),
                    })
                }
                _ => {
                    // Status is determined/finalized but result is empty or unexpected
                    Ok(ResolutionResult::NotYetResolved)
                }
            }
        }
        "disputed" => Ok(ResolutionResult::Disputed {
            dispute_started: Utc::now(),
        }),
        _ => Ok(ResolutionResult::NotYetResolved),
    }
}

/// Check if a settlement value is scalar (not clean binary 0 or 1).
///
/// Kalshi Rule 6.3(c): When settlement_value_dollars is neither $0.00 nor $1.00,
/// it indicates a non-binary settlement at last-traded price.
fn is_scalar_settlement(settlement_price: &Option<Decimal>) -> bool {
    if let Some(price) = settlement_price {
        let zero = Decimal::ZERO;
        let one = Decimal::ONE;
        *price != zero && *price != one
    } else {
        false
    }
}

/// Parse settlement_value_dollars from Kalshi's FixedPointDollars format.
fn parse_settlement_value(value: &Option<String>) -> Option<Decimal> {
    value.as_deref().and_then(|s| s.parse::<Decimal>().ok())
}

/// Parse settlement_ts from Kalshi's ISO8601 datetime string.
fn parse_settlement_ts(ts: &Option<String>) -> DateTime<Utc> {
    ts.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use rust_decimal_macros::dec;

    fn make_market(status: &str, result: Option<&str>, value: Option<&str>, ts: Option<&str>) -> KalshiMarketDetail {
        KalshiMarketDetail {
            ticker: "KXBTCD-25JUN30-T100000".to_string(),
            status: status.to_string(),
            result: result.map(|s| s.to_string()),
            settlement_value_dollars: value.map(|s| s.to_string()),
            settlement_ts: ts.map(|s| s.to_string()),
        }
    }

    #[test]
    fn determined_yes_resolves_yes() {
        let market = make_market("determined", Some("yes"), Some("1.00"), None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, .. } => {
                assert_eq!(outcome, OutcomeKind::Yes);
            }
            other => panic!("expected Resolved(Yes), got {:?}", other),
        }
    }

    #[test]
    fn finalized_no_resolves_no() {
        let market = make_market("finalized", Some("no"), Some("0.00"), None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, .. } => {
                assert_eq!(outcome, OutcomeKind::No);
            }
            other => panic!("expected Resolved(No), got {:?}", other),
        }
    }

    #[test]
    fn determined_scalar_is_ambiguous() {
        let market = make_market("determined", Some("scalar"), Some("0.42"), None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Ambiguous { raw_data } => {
                assert!(raw_data.contains("scalar"));
                assert!(raw_data.contains("0.42"));
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn yes_with_fractional_value_is_ambiguous() {
        // Kalshi Rule 6.3(c): result says "yes" but settlement value is fractional
        let market = make_market("determined", Some("yes"), Some("0.65"), None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Ambiguous { .. } => {} // expected
            other => panic!("expected Ambiguous for fractional value, got {:?}", other),
        }
    }

    #[test]
    fn disputed_returns_disputed() {
        let market = make_market("disputed", None, None, None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Disputed { .. } => {} // expected
            other => panic!("expected Disputed, got {:?}", other),
        }
    }

    #[test]
    fn active_status_is_not_yet_resolved() {
        let market = make_market("active", None, None, None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved, got {:?}", other),
        }
    }

    #[test]
    fn closed_status_is_not_yet_resolved() {
        let market = make_market("closed", None, None, None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved, got {:?}", other),
        }
    }

    #[test]
    fn determined_with_empty_result_is_not_yet_resolved() {
        let market = make_market("determined", None, None, None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved for empty result, got {:?}", other),
        }
    }

    #[test]
    fn parse_settlement_value_works() {
        assert_eq!(parse_settlement_value(&Some("1.00".to_string())), Some(dec!(1.00)));
        assert_eq!(parse_settlement_value(&Some("0.42".to_string())), Some(dec!(0.42)));
        assert_eq!(parse_settlement_value(&None), None);
        assert_eq!(parse_settlement_value(&Some("invalid".to_string())), None);
    }

    #[test]
    fn parse_settlement_ts_works() {
        let ts = parse_settlement_ts(&Some("2025-06-27T08:00:00Z".to_string()));
        assert_eq!(ts.year(), 2025);

        // Fallback to now for invalid/missing
        let ts_now = parse_settlement_ts(&None);
        assert!(ts_now.year() >= 2025);
    }

    #[test]
    fn is_scalar_checks_correctly() {
        assert!(!is_scalar_settlement(&None));
        assert!(!is_scalar_settlement(&Some(Decimal::ZERO)));
        assert!(!is_scalar_settlement(&Some(Decimal::ONE)));
        assert!(is_scalar_settlement(&Some(dec!(0.42))));
        assert!(is_scalar_settlement(&Some(dec!(0.65))));
    }

    #[test]
    fn no_with_zero_value_resolves_cleanly() {
        let market = make_market("finalized", Some("no"), Some("0"), None);
        let result = dispatch_kalshi_status(&market).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, .. } => {
                assert_eq!(outcome, OutcomeKind::No);
            }
            other => panic!("expected Resolved(No), got {:?}", other),
        }
    }
}
