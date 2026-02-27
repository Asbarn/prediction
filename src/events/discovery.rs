//! Per-venue REST discovery and cross-venue candidate matching.
//!
//! Polls each venue's instrument list API, normalizes responses into
//! `DiscoveredInstrument` structs, and identifies cross-venue candidate
//! matches using exact four-field matching (asset + strike + expiry + direction).
//!
//! Polymarket structured discovery parses question text from the Gamma API
//! to extract asset, strike, direction, and uses `endDateIso` for expiry.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::config::Direction;
use crate::events::registry::EventRegistry;
use crate::events::toml_writer::{CandidateMapping, CandidateVenues};
use crate::feed::reliability::VenueRateLimiter;
use crate::types::Venue;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A normalized instrument discovered from a venue's REST API.
#[derive(Debug, Clone)]
pub struct DiscoveredInstrument {
    pub venue: Venue,
    pub instrument_id: String,
    pub asset: String,
    pub strike: Decimal,
    pub expiry: NaiveDate,
    pub direction: Direction,
    pub is_active: bool,
    /// Original milliseconds-since-epoch for precise comparison.
    pub raw_expiry_timestamp: i64,
}

/// Four-field match key for exact cross-venue matching.
///
/// All four fields must align for a candidate match (per user decision:
/// exact matching after normalization, no fuzzy tolerance).
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MatchKey {
    /// Asset ticker, uppercased.
    pub asset: String,
    /// Strike price, exact after normalization.
    pub strike: Decimal,
    /// Expiry date.
    pub expiry: NaiveDate,
    /// Direction (above/below).
    pub direction: Direction,
}

impl MatchKey {
    /// Build a match key from a discovered instrument.
    pub fn from_discovered(d: &DiscoveredInstrument) -> Self {
        Self {
            asset: d.asset.to_uppercase(),
            strike: d.strike,
            expiry: d.expiry,
            direction: d.direction.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Expiry confidence scoring
// ---------------------------------------------------------------------------

/// Confidence level for expiry alignment between matched venues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpiryConfidence {
    /// All venue expiries within 2 days of each other.
    High,
    /// All venue expiries within 7 days of each other.
    Medium,
    /// All venue expiries within the configured tolerance (>7 days).
    Low,
}

impl std::fmt::Display for ExpiryConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpiryConfidence::High => write!(f, "HIGH"),
            ExpiryConfidence::Medium => write!(f, "MEDIUM"),
            ExpiryConfidence::Low => write!(f, "LOW"),
        }
    }
}

/// Compute expiry confidence from the maximum date spread in a group.
pub fn compute_expiry_confidence(expiries: &[NaiveDate]) -> ExpiryConfidence {
    if expiries.len() <= 1 {
        return ExpiryConfidence::High;
    }
    let min = expiries.iter().min().unwrap();
    let max = expiries.iter().max().unwrap();
    let spread_days = (*max - *min).num_days();

    if spread_days <= 2 {
        ExpiryConfidence::High
    } else if spread_days <= 7 {
        ExpiryConfidence::Medium
    } else {
        ExpiryConfidence::Low
    }
}

/// Generate concrete Polymarket event slugs from base patterns.
///
/// Replaces `{month}` with the current month name (lowercase) and
/// `{year}` with the current four-digit year string.
pub fn generate_polymarket_slugs(base_patterns: &[String]) -> Vec<String> {
    let now = chrono::Utc::now();
    let month_name = now.format("%B").to_string().to_lowercase();
    let year = now.format("%Y").to_string();

    base_patterns
        .iter()
        .map(|pattern| {
            pattern
                .replace("{month}", &month_name)
                .replace("{year}", &year)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Deribit discovery (public endpoint, no auth)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeribitInstrumentsResponse {
    result: Vec<DeribitInstrumentInfo>,
}

#[derive(Debug, Deserialize)]
struct DeribitInstrumentInfo {
    instrument_name: String,
    #[allow(dead_code)]
    kind: String,
    base_currency: String,
    strike: Option<f64>,
    option_type: Option<String>,
    expiration_timestamp: i64,
    is_active: bool,
}

/// Discover active Deribit option instruments via the public REST API.
///
/// GET `{base_url}/api/v2/public/get_instruments?currency={currency}&kind=option`
/// for each configured currency. No authentication required.
///
/// Uses the structured API response fields (not instrument name parsing)
/// per research "Don't Hand-Roll" recommendation.
pub async fn discover_deribit(
    client: &reqwest::Client,
    base_url: &str,
    currencies: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all = Vec::new();

    for currency in currencies {
        if let Some(limiter) = rate_limiter {
            limiter.wait().await;
        }
        let url = format!("{}/api/v2/public/get_instruments", base_url);
        let resp = client
            .get(&url)
            .query(&[("currency", currency.as_str()), ("kind", "option")])
            .send()
            .await?;

        let body: DeribitInstrumentsResponse = resp.json().await?;

        for info in body.result {
            if !info.is_active {
                continue;
            }
            let strike = match info.strike {
                Some(s) => match Decimal::from_f64_retain(s) {
                    Some(d) => d,
                    None => continue,
                },
                None => continue,
            };
            let direction = match info.option_type.as_deref() {
                Some("call") => Direction::Above,
                Some("put") => Direction::Below,
                _ => continue,
            };
            let expiry_dt = match DateTime::from_timestamp_millis(info.expiration_timestamp) {
                Some(dt) => dt,
                None => continue,
            };
            let expiry = expiry_dt.date_naive();

            all.push(DiscoveredInstrument {
                venue: Venue::Deribit,
                instrument_id: info.instrument_name,
                asset: info.base_currency,
                strike,
                expiry,
                direction,
                is_active: info.is_active,
                raw_expiry_timestamp: info.expiration_timestamp,
            });
        }
    }

    Ok(all)
}

// ---------------------------------------------------------------------------
// Kalshi discovery (requires auth)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct KalshiMarketsResponse {
    markets: Vec<KalshiMarketInfo>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KalshiMarketInfo {
    ticker: String,
    #[allow(dead_code)]
    event_ticker: Option<String>,
    #[allow(dead_code)]
    title: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
    close_time: Option<String>,
    /// Kalshi floor strike in dollars (post-Feb 2026 migration).
    floor_strike: Option<f64>,
    /// Kalshi cap strike in dollars (post-Feb 2026 migration).
    cap_strike: Option<f64>,
}

/// Discover open Kalshi markets for configured series tickers.
///
/// GET `{rest_url}/trade-api/v2/markets?series_ticker={series}&status=open&limit=200`
/// Requires RSA-PSS signed authentication headers.
/// Paginates using cursor from response.
///
/// Uses `floor_strike`/`cap_strike` _dollars fields per research
/// (Kalshi deprecated integer fields Feb 2026).
pub async fn discover_kalshi(
    client: &reqwest::Client,
    rest_url: &str,
    api_key_id: &str,
    private_key: &rsa::RsaPrivateKey,
    series_tickers: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    use crate::feed::kalshi::auth::sign_kalshi_request;

    let mut all = Vec::new();

    for series in series_tickers {
        let mut cursor: Option<String> = None;

        loop {
            if let Some(limiter) = rate_limiter {
                limiter.wait().await;
            }
            let timestamp_ms = Utc::now().timestamp_millis();
            let path = "/trade-api/v2/markets";
            let signature = sign_kalshi_request(private_key, timestamp_ms, "GET", path)?;

            let mut req = client
                .get(format!("{}{}", rest_url, path))
                .header("KALSHI-ACCESS-KEY", api_key_id)
                .header("KALSHI-ACCESS-SIGNATURE", &signature)
                .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
                .query(&[
                    ("series_ticker", series.as_str()),
                    ("status", "open"),
                    ("limit", "200"),
                ]);

            if let Some(ref c) = cursor {
                req = req.query(&[("cursor", c.as_str())]);
            }

            let resp: KalshiMarketsResponse = req.send().await?.json().await?;

            for market in &resp.markets {
                if let Some(inst) = parse_kalshi_market(market) {
                    all.push(inst);
                }
            }

            match resp.cursor {
                Some(ref c) if !c.is_empty() => cursor = Some(c.clone()),
                _ => break,
            }
        }
    }

    Ok(all)
}

/// Parse a Kalshi market into a DiscoveredInstrument.
///
/// Maps floor_strike -> Direction::Above, cap_strike -> Direction::Below.
/// Skips markets where neither is available (non-structured binary markets).
fn parse_kalshi_market(market: &KalshiMarketInfo) -> Option<DiscoveredInstrument> {
    let (strike_f64, direction) = if let Some(floor) = market.floor_strike {
        (floor, Direction::Above)
    } else if let Some(cap) = market.cap_strike {
        (cap, Direction::Below)
    } else {
        return None;
    };

    let strike = Decimal::from_f64_retain(strike_f64)?;

    // Parse close_time as ISO 8601 datetime for expiry
    let close_time_str = market.close_time.as_deref()?;
    let expiry_dt = DateTime::parse_from_rfc3339(close_time_str).ok()?;
    let expiry = expiry_dt.date_naive();
    let raw_ts = expiry_dt.timestamp_millis();

    // Extract asset from ticker prefix (e.g., "KXBTCD-..." -> "BTC")
    // Kalshi crypto tickers start with "KX{ASSET}" pattern
    let asset = extract_kalshi_asset(&market.ticker)?;

    Some(DiscoveredInstrument {
        venue: Venue::Kalshi,
        instrument_id: market.ticker.clone(),
        asset,
        strike,
        expiry,
        direction,
        is_active: true,
        raw_expiry_timestamp: raw_ts,
    })
}

/// Extract asset ticker from a Kalshi market ticker.
///
/// Kalshi crypto tickers follow patterns like "KXBTCD-...", "KXETH-...".
/// We strip the "KX" prefix and take the asset portion before any
/// non-alpha character or known suffix.
fn extract_kalshi_asset(ticker: &str) -> Option<String> {
    if !ticker.starts_with("KX") {
        return None;
    }
    // Skip "KX", take uppercase alpha chars as asset
    let rest = &ticker[2..];
    let asset: String = rest.chars().take_while(|c| c.is_ascii_uppercase()).collect();
    if asset.is_empty() {
        return None;
    }
    // Strip common suffixes: "D" for daily, etc.
    // Common pattern: KXBTCD -> BTC, KXETH -> ETH
    let asset = asset.strip_suffix('D').unwrap_or(&asset);
    let asset = asset.strip_suffix("MAXY").unwrap_or(asset);
    Some(asset.to_string())
}

// ---------------------------------------------------------------------------
// Polymarket discovery (limited -- deactivation monitoring only in v1)
// ---------------------------------------------------------------------------

/// Raw Polymarket market info from the Gamma API.
///
/// In v1, Polymarket discovery is for monitoring deactivation/resolution of
/// existing markets, NOT for proposing new matches (structured field extraction
/// from free-form questions is deferred).
#[derive(Debug, Clone, Deserialize)]
pub struct PolymarketMarketInfo {
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    pub question: String,
    #[serde(rename = "endDateIso")]
    pub end_date_iso: Option<String>,
    pub active: bool,
    pub closed: bool,
    #[serde(default)]
    pub tokens: Vec<PolymarketToken>,
    pub category: Option<String>,
}

/// Token info from Polymarket Gamma API.
#[derive(Debug, Clone, Deserialize)]
pub struct PolymarketToken {
    pub token_id: String,
    pub outcome: String,
}

/// Discover active Polymarket markets via the Gamma API.
///
/// GET `{gamma_api_url}/markets?active=true&limit=100&offset={offset}`
/// Returns raw PolymarketMarketInfo -- does NOT extract structured fields
/// from question text in v1 (per research recommendation).
pub async fn discover_polymarket(
    client: &reqwest::Client,
    gamma_api_url: &str,
) -> anyhow::Result<Vec<PolymarketMarketInfo>> {
    let mut all = Vec::new();
    let mut offset: usize = 0;
    let limit: usize = 100;

    loop {
        let resp: Vec<PolymarketMarketInfo> = client
            .get(format!("{}/markets", gamma_api_url))
            .query(&[
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("active", &"true".to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;

        let count = resp.len();
        all.extend(resp);

        if count < limit {
            break;
        }
        offset += limit;
    }

    Ok(all)
}

// ---------------------------------------------------------------------------
// Cross-venue candidate matching
// ---------------------------------------------------------------------------

/// Group discovered instruments by exact four-field match key across venues.
///
/// Only returns groups with instruments from 2+ different venues.
/// Called with combined Deribit + Kalshi instruments (Polymarket excluded
/// from auto-matching in v1).
pub fn find_cross_venue_candidates(
    instruments: &[DiscoveredInstrument],
) -> HashMap<MatchKey, Vec<&DiscoveredInstrument>> {
    let mut groups: HashMap<MatchKey, Vec<&DiscoveredInstrument>> = HashMap::new();

    for inst in instruments {
        let key = MatchKey::from_discovered(inst);
        groups.entry(key).or_default().push(inst);
    }

    // Only return groups with instruments from 2+ different venues
    groups.retain(|_, v| {
        let venues: HashSet<Venue> = v.iter().map(|i| i.venue).collect();
        venues.len() >= 2
    });

    groups
}

/// Filter candidate groups to only those not already in the registry.
///
/// For each candidate group, checks if a mapping already exists in the
/// registry (by event_id pattern or instrument_id). Only returns candidates
/// not already registered. Generates event_id as `{ASSET}-{STRIKE}-{EXPIRY}`.
pub fn filter_new_candidates(
    candidates: &HashMap<MatchKey, Vec<&DiscoveredInstrument>>,
    existing_registry: &EventRegistry,
) -> Vec<CandidateMapping> {
    let mut new_candidates = Vec::new();

    for (key, instruments) in candidates {
        // Generate event_id as {ASSET}-{STRIKE}-{EXPIRY}
        let event_id = format!("{}-{}-{}", key.asset, key.strike, key.expiry);

        // Check if this event already exists in the registry
        if existing_registry.lookup_by_event_id(&event_id).is_some() {
            continue;
        }

        // Also check if any of the individual instruments are already mapped
        let any_already_mapped = instruments.iter().any(|inst| {
            existing_registry
                .lookup_by_instrument(inst.venue, &inst.instrument_id)
                .is_some()
        });
        if any_already_mapped {
            continue;
        }

        // Build CandidateMapping from the group
        let mut deribit: Option<String> = None;
        let mut kalshi: Option<String> = None;

        for inst in instruments {
            match inst.venue {
                Venue::Deribit => deribit = Some(inst.instrument_id.clone()),
                Venue::Kalshi => kalshi = Some(inst.instrument_id.clone()),
                Venue::Polymarket => {} // Excluded from auto-matching in v1
            }
        }

        new_candidates.push(CandidateMapping {
            id: event_id,
            asset: key.asset.clone(),
            strike: key.strike.to_string(),
            direction: key.direction.clone(),
            expiry: key.expiry.to_string(),
            venues: CandidateVenues {
                deribit,
                polymarket: None, // Polymarket excluded in v1
                kalshi,
            },
            expiry_confidence: ExpiryConfidence::High,
        });
    }

    new_candidates
}

/// Flag instruments that exist in only one venue and are not in any existing mapping.
///
/// These are logged for user attention as potential new opportunity types
/// (novel assets, event structures, etc.).
pub fn flag_novel_instruments<'a>(
    discovered: &'a [DiscoveredInstrument],
    existing_registry: &EventRegistry,
) -> Vec<&'a DiscoveredInstrument> {
    // Build a set of all match keys to find which are single-venue
    let mut key_venues: HashMap<MatchKey, HashSet<Venue>> = HashMap::new();
    for inst in discovered {
        let key = MatchKey::from_discovered(inst);
        key_venues.entry(key).or_default().insert(inst.venue);
    }

    discovered
        .iter()
        .filter(|inst| {
            let key = MatchKey::from_discovered(inst);
            // Single-venue: only one venue has this match key
            let is_single_venue = key_venues
                .get(&key)
                .map_or(true, |venues| venues.len() == 1);
            // Not already mapped
            let not_mapped = existing_registry
                .lookup_by_instrument(inst.venue, &inst.instrument_id)
                .is_none();
            is_single_venue && not_mapped
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DeribitMapping, Direction, EventMapping, EventVenues, EventsConfig, LifecycleStatus,
    };
    use crate::events::registry::EventRegistry;
    use chrono::NaiveDate;
    use std::str::FromStr;

    fn make_discovered(
        venue: Venue,
        instrument_id: &str,
        asset: &str,
        strike: Decimal,
        expiry: NaiveDate,
        direction: Direction,
    ) -> DiscoveredInstrument {
        DiscoveredInstrument {
            venue,
            instrument_id: instrument_id.to_string(),
            asset: asset.to_string(),
            strike,
            expiry,
            direction,
            is_active: true,
            raw_expiry_timestamp: 0,
        }
    }

    fn make_empty_registry() -> EventRegistry {
        EventRegistry::from_config(&EventsConfig {
            events: vec![],
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        })
    }

    fn make_registry_with(events: Vec<EventMapping>) -> EventRegistry {
        EventRegistry::from_config(&EventsConfig {
            events,
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        })
    }

    fn make_mapping(id: &str, deribit_instrument: &str) -> EventMapping {
        EventMapping {
            id: id.to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-06-27".to_string(),
            venues: EventVenues {
                deribit: Some(DeribitMapping {
                    instrument: deribit_instrument.to_string(),
                }),
                polymarket: None,
                kalshi: None,
            },
            approved: true,
            status: LifecycleStatus::Active,
            discovered_at: None,
            settlement: None,
        }
    }

    // --- MatchKey tests ---

    #[test]
    fn match_key_same_fields_match() {
        let d1 = make_discovered(
            Venue::Deribit,
            "BTC-27JUN25-100000-C",
            "BTC",
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        );
        let d2 = make_discovered(
            Venue::Kalshi,
            "KXBTCD-25JUN27-T100000",
            "btc", // lowercase -- should still match after uppercasing
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        );

        assert_eq!(
            MatchKey::from_discovered(&d1),
            MatchKey::from_discovered(&d2)
        );
    }

    #[test]
    fn match_key_different_strike_no_match() {
        let d1 = make_discovered(
            Venue::Deribit,
            "I1",
            "BTC",
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        );
        let d2 = make_discovered(
            Venue::Deribit,
            "I2",
            "BTC",
            Decimal::from_str("110000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        );

        assert_ne!(
            MatchKey::from_discovered(&d1),
            MatchKey::from_discovered(&d2)
        );
    }

    #[test]
    fn match_key_different_direction_no_match() {
        let d1 = make_discovered(
            Venue::Deribit,
            "I1",
            "BTC",
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        );
        let d2 = make_discovered(
            Venue::Deribit,
            "I2",
            "BTC",
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Below,
        );

        assert_ne!(
            MatchKey::from_discovered(&d1),
            MatchKey::from_discovered(&d2)
        );
    }

    // --- find_cross_venue_candidates tests ---

    #[test]
    fn cross_venue_groups_correctly() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            make_discovered(
                Venue::Kalshi,
                "KXBTCD-25JUN27-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
        ];

        let candidates = find_cross_venue_candidates(&instruments);
        assert_eq!(candidates.len(), 1);

        let group = candidates.values().next().unwrap();
        assert_eq!(group.len(), 2);
        let venues: HashSet<Venue> = group.iter().map(|i| i.venue).collect();
        assert!(venues.contains(&Venue::Deribit));
        assert!(venues.contains(&Venue::Kalshi));
    }

    #[test]
    fn cross_venue_excludes_single_venue_groups() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-P",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Below,
            ),
        ];

        let candidates = find_cross_venue_candidates(&instruments);
        assert!(candidates.is_empty());
    }

    // --- filter_new_candidates tests ---

    #[test]
    fn filter_skips_already_registered() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            make_discovered(
                Venue::Kalshi,
                "KXBTCD-25JUN27-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
        ];

        let candidates = find_cross_venue_candidates(&instruments);
        let registry = make_registry_with(vec![make_mapping(
            "BTC-100000-2025-06-27",
            "BTC-27JUN25-100000-C",
        )]);

        let new = filter_new_candidates(&candidates, &registry);
        assert!(new.is_empty(), "should skip already-registered mapping");
    }

    #[test]
    fn filter_returns_genuinely_new() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-25JUL25-120000-C",
                "BTC",
                Decimal::from_str("120000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 7, 25).unwrap(),
                Direction::Above,
            ),
            make_discovered(
                Venue::Kalshi,
                "KXBTCD-25JUL25-T120000",
                "BTC",
                Decimal::from_str("120000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 7, 25).unwrap(),
                Direction::Above,
            ),
        ];

        let candidates = find_cross_venue_candidates(&instruments);
        let registry = make_empty_registry();

        let new = filter_new_candidates(&candidates, &registry);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].id, "BTC-120000-2025-07-25");
        assert!(!new[0].venues.deribit.is_none());
        assert!(!new[0].venues.kalshi.is_none());
    }

    // --- flag_novel_instruments tests ---

    #[test]
    fn flag_novel_returns_unmatched_single_venue() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            // Kalshi has a DIFFERENT strike -- no cross-venue match
            make_discovered(
                Venue::Kalshi,
                "KXBTCD-25JUN27-T110000",
                "BTC",
                Decimal::from_str("110000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
        ];

        let registry = make_empty_registry();
        let novel = flag_novel_instruments(&instruments, &registry);

        // Both are single-venue (no cross-match) and not mapped
        assert_eq!(novel.len(), 2);
    }

    #[test]
    fn flag_novel_excludes_already_mapped() {
        let instruments = vec![make_discovered(
            Venue::Deribit,
            "BTC-27JUN25-100000-C",
            "BTC",
            Decimal::from_str("100000").unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            Direction::Above,
        )];

        let registry = make_registry_with(vec![make_mapping(
            "BTC-100000-2025-06-27",
            "BTC-27JUN25-100000-C",
        )]);

        let novel = flag_novel_instruments(&instruments, &registry);
        assert!(novel.is_empty(), "already-mapped should not be flagged");
    }

    #[test]
    fn flag_novel_excludes_cross_venue_matched() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            make_discovered(
                Venue::Kalshi,
                "KXBTCD-25JUN27-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
        ];

        let registry = make_empty_registry();
        let novel = flag_novel_instruments(&instruments, &registry);

        // Both match across venues, so neither is "novel"
        assert!(novel.is_empty());
    }

    // --- Deribit response parsing test ---

    #[test]
    fn parse_deribit_response_json() {
        let json = r#"{
            "result": [
                {
                    "instrument_name": "BTC-27JUN25-100000-C",
                    "kind": "option",
                    "base_currency": "BTC",
                    "strike": 100000.0,
                    "option_type": "call",
                    "expiration_timestamp": 1751011200000,
                    "is_active": true,
                    "settlement_period": "month"
                },
                {
                    "instrument_name": "BTC-27JUN25-100000-P",
                    "kind": "option",
                    "base_currency": "BTC",
                    "strike": 100000.0,
                    "option_type": "put",
                    "expiration_timestamp": 1751011200000,
                    "is_active": true,
                    "settlement_period": "month"
                },
                {
                    "instrument_name": "BTC-27JUN25-120000-C",
                    "kind": "option",
                    "base_currency": "BTC",
                    "strike": 120000.0,
                    "option_type": "call",
                    "expiration_timestamp": 1751011200000,
                    "is_active": false,
                    "settlement_period": "month"
                }
            ]
        }"#;

        let resp: DeribitInstrumentsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.result.len(), 3);

        // Filter active and parse
        let active: Vec<_> = resp
            .result
            .iter()
            .filter(|i| i.is_active)
            .collect();
        assert_eq!(active.len(), 2);

        // Verify strike parsing
        let strike = Decimal::from_f64_retain(active[0].strike.unwrap()).unwrap();
        assert_eq!(strike, Decimal::from_str("100000").unwrap());

        // Verify direction mapping
        assert_eq!(active[0].option_type.as_deref(), Some("call"));
        assert_eq!(active[1].option_type.as_deref(), Some("put"));

        // Verify expiry parsing
        let dt = DateTime::from_timestamp_millis(active[0].expiration_timestamp).unwrap();
        let date = dt.date_naive();
        assert_eq!(date, NaiveDate::from_ymd_opt(2025, 6, 27).unwrap());
    }

    // --- Kalshi response parsing test ---

    #[test]
    fn parse_kalshi_response_json() {
        let json = r#"{
            "markets": [
                {
                    "ticker": "KXBTCD-25JUN27-T100000",
                    "event_ticker": "KXBTC-25JUN27",
                    "title": "BTC above $100,000?",
                    "status": "open",
                    "close_time": "2025-06-27T23:59:59Z",
                    "floor_strike": 100000.0,
                    "cap_strike": null
                },
                {
                    "ticker": "KXBTCD-25JUN27-B90000",
                    "event_ticker": "KXBTC-25JUN27",
                    "title": "BTC below $90,000?",
                    "status": "open",
                    "close_time": "2025-06-27T23:59:59Z",
                    "floor_strike": null,
                    "cap_strike": 90000.0
                },
                {
                    "ticker": "KXBTCD-25JUN27-BINARY",
                    "event_ticker": "KXBTC-25JUN27",
                    "title": "BTC yes/no?",
                    "status": "open",
                    "close_time": "2025-06-27T23:59:59Z",
                    "floor_strike": null,
                    "cap_strike": null
                }
            ],
            "cursor": ""
        }"#;

        let resp: KalshiMarketsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.markets.len(), 3);

        // Parse each market
        let parsed: Vec<_> = resp
            .markets
            .iter()
            .filter_map(parse_kalshi_market)
            .collect();

        // Third market (no floor/cap) should be skipped
        assert_eq!(parsed.len(), 2);

        // First: floor_strike -> Above
        assert_eq!(parsed[0].direction, Direction::Above);
        assert_eq!(parsed[0].strike, Decimal::from_str("100000").unwrap());
        assert_eq!(parsed[0].instrument_id, "KXBTCD-25JUN27-T100000");

        // Second: cap_strike -> Below
        assert_eq!(parsed[1].direction, Direction::Below);
        assert_eq!(parsed[1].strike, Decimal::from_str("90000").unwrap());
    }

    // --- extract_kalshi_asset tests ---

    #[test]
    fn extract_asset_from_kalshi_ticker() {
        assert_eq!(extract_kalshi_asset("KXBTCD-25JUN27-T100000"), Some("BTC".to_string()));
        assert_eq!(extract_kalshi_asset("KXETH-25JUN27-T5000"), Some("ETH".to_string()));
        assert_eq!(extract_kalshi_asset("INVALID"), None);
        assert_eq!(extract_kalshi_asset("KX"), None);
    }

    // --- DiscoveryConfig min_poll_interval_secs test ---

    #[test]
    fn discovery_config_min_poll_interval() {
        use crate::config::DiscoveryConfig;

        let config = DiscoveryConfig {
            deribit_poll_interval_secs: 300,
            kalshi_poll_interval_secs: 600,
            polymarket_poll_interval_secs: 600,
            ..Default::default()
        };
        assert_eq!(config.min_poll_interval_secs(), 300);
    }
}
