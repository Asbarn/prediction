//! Polymarket resolution checker implementation.
//!
//! Queries the Gamma API for market status and uses a two-stage check
//! (closed=true AND outcome price lock) to determine resolution.
//! No authentication required for Gamma API.

use tracing::debug;

use crate::feed::reliability::rate_limiter::VenueRateLimiter;

use super::traits::CheckContext;
use super::types::{OutcomeKind, ResolutionResult};

/// Response from the Polymarket Gamma API GET /markets endpoint.
///
/// Note: `outcomePrices` is a JSON string inside JSON (e.g., `"[\"0.95\", \"0.05\"]"`).
/// Must be parsed twice: first as String from serde, then the string content as JSON array.
#[derive(Debug, serde::Deserialize)]
struct GammaMarketResponse {
    #[serde(rename = "conditionId")]
    #[allow(dead_code)]
    condition_id: String,
    #[allow(dead_code)]
    active: bool,
    closed: bool,
    #[serde(rename = "outcomePrices", default)]
    outcome_prices: Option<String>,
    #[serde(rename = "umaResolutionStatuses", default)]
    uma_resolution_statuses: Option<String>,
}

/// Resolution checker for Polymarket markets.
///
/// Uses the Gamma API (no auth required) with a two-stage resolution check:
/// 1. Market must be `closed: true`
/// 2. Outcome prices must be locked (one >= threshold, other <= 1-threshold)
pub struct PolymarketResolutionChecker {
    client: reqwest::Client,
    gamma_api_url: String,
    price_lock_threshold: f64,
    rate_limiter: VenueRateLimiter,
}

impl PolymarketResolutionChecker {
    /// Create a new Polymarket resolution checker.
    pub fn new(
        client: reqwest::Client,
        gamma_api_url: String,
        price_lock_threshold: f64,
        rate_limiter: VenueRateLimiter,
    ) -> Self {
        Self {
            client,
            gamma_api_url,
            price_lock_threshold,
            rate_limiter,
        }
    }

    /// Check resolution status for a Polymarket market.
    ///
    /// `venue_instrument` is the condition_id for Polymarket.
    pub async fn check_resolution(
        &self,
        _event_id: &str,
        venue_instrument: &str,
        _context: &CheckContext,
    ) -> anyhow::Result<ResolutionResult> {
        self.rate_limiter.wait().await;

        let resp = self
            .client
            .get(format!("{}/markets", self.gamma_api_url))
            .query(&[("id", venue_instrument)])
            .send()
            .await?;

        let body = resp.text().await?;
        let markets: Vec<GammaMarketResponse> = serde_json::from_str(&body)?;

        let market = markets
            .first()
            .ok_or_else(|| anyhow::anyhow!("Polymarket market not found for condition_id {}", venue_instrument))?;

        debug!(
            condition_id = venue_instrument,
            closed = market.closed,
            outcome_prices = ?market.outcome_prices,
            uma_status = ?market.uma_resolution_statuses,
            "Polymarket market status"
        );

        resolve_from_gamma(market, self.price_lock_threshold)
    }
}

/// Determine resolution from Gamma API market response.
///
/// Two-stage check:
/// 1. Market must be closed
/// 2. Outcome prices must be locked (one near 1.0, other near 0.0)
fn resolve_from_gamma(
    market: &GammaMarketResponse,
    threshold: f64,
) -> anyhow::Result<ResolutionResult> {
    // Stage 1: Must be closed
    if !market.closed {
        return Ok(ResolutionResult::NotYetResolved);
    }

    // Stage 2: Parse outcome prices and check for lock
    if let Some(ref prices_str) = market.outcome_prices {
        // outcomePrices is a JSON string inside JSON: "[\"0.95\", \"0.05\"]"
        let prices: Vec<String> = match serde_json::from_str(prices_str) {
            Ok(p) => p,
            Err(_) => {
                debug!(
                    raw_prices = %prices_str,
                    "Failed to parse Polymarket outcome prices"
                );
                return Ok(ResolutionResult::NotYetResolved);
            }
        };

        if prices.len() >= 2 {
            let yes_price: f64 = prices[0].parse().unwrap_or(0.0);
            let no_price: f64 = prices[1].parse().unwrap_or(0.0);
            let inv_threshold = 1.0 - threshold;

            if yes_price >= threshold && no_price <= inv_threshold {
                return Ok(ResolutionResult::Resolved {
                    outcome: OutcomeKind::Yes,
                    settlement_price: None, // Binary, no continuous price
                    resolved_at: chrono::Utc::now(),
                });
            }

            if no_price >= threshold && yes_price <= inv_threshold {
                return Ok(ResolutionResult::Resolved {
                    outcome: OutcomeKind::No,
                    settlement_price: None,
                    resolved_at: chrono::Utc::now(),
                });
            }
        }
    }

    // Check for UMA dispute indicators
    if let Some(ref uma_status) = market.uma_resolution_statuses {
        if uma_status.contains("disputed") || uma_status.contains("DVM") {
            return Ok(ResolutionResult::Disputed {
                dispute_started: chrono::Utc::now(),
            });
        }
    }

    // Closed but prices not yet locked -- keep polling
    Ok(ResolutionResult::NotYetResolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_market(
        closed: bool,
        prices: Option<&str>,
        uma_status: Option<&str>,
    ) -> GammaMarketResponse {
        GammaMarketResponse {
            condition_id: "0xabc123".to_string(),
            active: !closed,
            closed,
            outcome_prices: prices.map(|s| s.to_string()),
            uma_resolution_statuses: uma_status.map(|s| s.to_string()),
        }
    }

    const THRESHOLD: f64 = 0.95;

    #[test]
    fn not_closed_is_not_resolved() {
        let market = make_market(false, None, None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved, got {:?}", other),
        }
    }

    #[test]
    fn closed_with_yes_locked_prices_resolves_yes() {
        // outcomePrices is a JSON string: ["0.98", "0.02"]
        let market = make_market(true, Some(r#"["0.98", "0.02"]"#), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, settlement_price, .. } => {
                assert_eq!(outcome, OutcomeKind::Yes);
                assert_eq!(settlement_price, None); // Binary
            }
            other => panic!("expected Resolved(Yes), got {:?}", other),
        }
    }

    #[test]
    fn closed_with_no_locked_prices_resolves_no() {
        let market = make_market(true, Some(r#"["0.02", "0.98"]"#), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, .. } => {
                assert_eq!(outcome, OutcomeKind::No);
            }
            other => panic!("expected Resolved(No), got {:?}", other),
        }
    }

    #[test]
    fn closed_with_unlocked_prices_is_not_resolved() {
        // Prices not yet locked: 60/40 split
        let market = make_market(true, Some(r#"["0.60", "0.40"]"#), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved for unlocked prices, got {:?}", other),
        }
    }

    #[test]
    fn closed_with_boundary_threshold_prices() {
        // Exactly at threshold
        let market = make_market(true, Some(r#"["0.95", "0.05"]"#), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::Resolved { outcome, .. } => {
                assert_eq!(outcome, OutcomeKind::Yes);
            }
            other => panic!("expected Resolved(Yes) at threshold, got {:?}", other),
        }
    }

    #[test]
    fn closed_just_below_threshold_not_resolved() {
        let market = make_market(true, Some(r#"["0.94", "0.06"]"#), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved below threshold, got {:?}", other),
        }
    }

    #[test]
    fn uma_disputed_returns_disputed() {
        let market = make_market(true, Some(r#"["0.60", "0.40"]"#), Some("disputed"));
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::Disputed { .. } => {} // expected
            other => panic!("expected Disputed, got {:?}", other),
        }
    }

    #[test]
    fn uma_dvm_returns_disputed() {
        let market = make_market(true, Some(r#"["0.50", "0.50"]"#), Some("DVM vote pending"));
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::Disputed { .. } => {} // expected
            other => panic!("expected Disputed for DVM, got {:?}", other),
        }
    }

    #[test]
    fn closed_no_prices_no_dispute_stays_not_resolved() {
        let market = make_market(true, None, None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved with no prices, got {:?}", other),
        }
    }

    #[test]
    fn malformed_prices_string_stays_not_resolved() {
        let market = make_market(true, Some("not valid json"), None);
        let result = resolve_from_gamma(&market, THRESHOLD).unwrap();
        match result {
            ResolutionResult::NotYetResolved => {} // expected
            other => panic!("expected NotYetResolved for malformed prices, got {:?}", other),
        }
    }

    #[test]
    fn parse_double_encoded_json_prices() {
        // Real-world format: JSON string inside JSON
        let raw = r#"["0.98","0.02"]"#;
        let prices: Vec<String> = serde_json::from_str(raw).unwrap();
        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0], "0.98");
        assert_eq!(prices[1], "0.02");
    }
}
