//! Per-venue REST discovery and cross-venue candidate matching.
//!
//! Polls each venue's instrument list API, normalizes responses into
//! `DiscoveredInstrument` structs, and identifies cross-venue candidate
//! matches using exact four-field matching (asset + strike + expiry + direction).
//!
//! Polymarket structured discovery parses question text from the Gamma API
//! to extract asset, strike, direction, and uses `endDateIso` for expiry.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use anyhow::Context;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
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
    /// Optional extra venue-specific ID (e.g., Polymarket token_id).
    /// Set by discover_polymarket_structured for the Yes-outcome token_id.
    /// None for Deribit and Kalshi instruments.
    pub extra_venue_id: Option<String>,
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

/// Three-field match key for fuzzy (tolerance-based) cross-venue matching.
///
/// Groups instruments by asset, strike, and direction only -- ignoring expiry.
/// Expiry tolerance is checked separately in `find_cross_venue_candidates_fuzzy`.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FuzzyMatchKey {
    /// Asset ticker, uppercased.
    pub asset: String,
    /// Strike price, exact after normalization.
    pub strike: Decimal,
    /// Direction (above/below).
    pub direction: Direction,
}

impl FuzzyMatchKey {
    /// Build a fuzzy match key from a discovered instrument.
    pub fn from_discovered(d: &DiscoveredInstrument) -> Self {
        Self {
            asset: d.asset.to_uppercase(),
            strike: d.strike,
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
/// Replaces `{month}` with the current month name (lowercase),
/// `{year}` with the current four-digit year string, and
/// `{next_year}` with next year's four-digit string.
pub fn generate_polymarket_slugs(base_patterns: &[String]) -> Vec<String> {
    let now = chrono::Utc::now();
    let month_name = now.format("%B").to_string().to_lowercase();
    let year = now.format("%Y").to_string();
    let next_year = (now.date_naive().year() + 1).to_string();

    base_patterns
        .iter()
        .map(|pattern| {
            pattern
                .replace("{month}", &month_name)
                .replace("{next_year}", &next_year)
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
                extra_venue_id: None,
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
        extra_venue_id: None,
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
// Polymarket structured discovery (question text parsing + event slug polling)
// ---------------------------------------------------------------------------

/// Gamma API event-level response wrapping multiple markets.
#[derive(Debug, Clone, Deserialize)]
pub struct GammaEventResponse {
    pub title: Option<String>,
    pub markets: Vec<PolymarketMarketInfo>,
}

/// Normalize a Polymarket asset name to a standard ticker.
///
/// Maps known crypto asset names (case-insensitive) to their
/// standard tickers. Returns None for unrecognized assets.
pub fn normalize_polymarket_asset(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "bitcoin" => Some("BTC"),
        "ethereum" | "ether" => Some("ETH"),
        "solana" => Some("SOL"),
        _ => None,
    }
}

/// Parse a Polymarket question into structured fields.
///
/// Supports patterns:
///   "Will {Asset} reach ${Strike} by {Date}?" -> Above
///   "Will {Asset} dip to ${Strike} by {Date}?" -> Below
///   "Will {Asset} hit ${Strike} by {Date}?" -> Above (treat "hit" as upward)
///
/// Returns `(asset_ticker, strike, direction)` on success, None for
/// unparseable questions. Does NOT parse date from question text --
/// `endDateIso` from the API is the authoritative expiry source.
pub fn parse_polymarket_question(question: &str) -> Option<(String, Decimal, Direction)> {
    // Strip leading "Will " and trailing "?"
    let q = question.strip_prefix("Will ")?.strip_suffix('?')?.trim();

    // Find the asset name (first word after "Will ")
    let space_idx = q.find(' ')?;
    let asset_name = &q[..space_idx];
    let asset = normalize_polymarket_asset(asset_name)?.to_string();
    let rest = &q[space_idx + 1..];

    // Determine direction from verb
    let (direction, after_verb) = if let Some(r) = rest.strip_prefix("reach $") {
        (Direction::Above, r)
    } else if let Some(r) = rest.strip_prefix("hit $") {
        (Direction::Above, r)
    } else if let Some(r) = rest.strip_prefix("dip to $") {
        (Direction::Below, r)
    } else {
        return None;
    };

    // Extract strike: everything before " by "
    let by_idx = after_verb.find(" by ")?;
    let strike_str = &after_verb[..by_idx];
    // Remove commas from strike (e.g., "150,000" -> "150000")
    let strike_clean: String = strike_str.chars().filter(|c| *c != ',').collect();
    let strike = Decimal::from_str_exact(&strike_clean).ok()?;

    Some((asset, strike, direction))
}

/// Discover structured Polymarket instruments from Gamma API events.
///
/// Polls each configured event slug via `GET {gamma_api_url}/events?slug={slug}`,
/// parses market questions for structured fields (asset, strike, direction),
/// and returns `DiscoveredInstrument` entries for cross-venue matching.
///
/// Deduplicates by `conditionId` across slugs (Pitfall 4 from research).
/// Unparseable questions are logged at WARN and counted via
/// `polymarket_parse_failures` metric.
pub async fn discover_polymarket_structured(
    client: &reqwest::Client,
    gamma_api_url: &str,
    event_slugs: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all = Vec::new();
    let mut seen_conditions: HashSet<String> = HashSet::new();

    for slug in event_slugs {
        if let Some(limiter) = rate_limiter {
            limiter.wait().await;
        }

        let resp: Vec<GammaEventResponse> = client
            .get(format!("{}/events", gamma_api_url))
            .query(&[("slug", slug.as_str())])
            .send()
            .await?
            .json()
            .await?;

        for event in &resp {
            for market in &event.markets {
                // Deduplicate by conditionId across slugs
                if seen_conditions.contains(&market.condition_id) {
                    continue;
                }
                seen_conditions.insert(market.condition_id.clone());

                // Skip inactive/closed markets
                if !market.active || market.closed {
                    continue;
                }

                // Parse question for structured fields
                let (asset, strike, direction) = match parse_polymarket_question(&market.question) {
                    Some(fields) => fields,
                    None => {
                        tracing::warn!(
                            condition_id = %market.condition_id,
                            question = %market.question,
                            "unparseable Polymarket question, skipping"
                        );
                        metrics::counter!("polymarket_parse_failures").increment(1);
                        continue;
                    }
                };

                // Use endDateIso for expiry (authoritative source)
                let expiry = match &market.end_date_iso {
                    Some(d) => {
                        // Try YYYY-MM-DD format first, then full ISO 8601
                        if let Ok(date) = NaiveDate::parse_from_str(d, "%Y-%m-%d") {
                            date
                        } else if let Ok(dt) = DateTime::parse_from_rfc3339(d) {
                            dt.date_naive()
                        } else {
                            continue;
                        }
                    }
                    None => continue,
                };

                // Get token_id: prefer the "Yes" outcome token, fallback to first
                let token_id = market
                    .tokens
                    .iter()
                    .find(|t| t.outcome == "Yes")
                    .or_else(|| market.tokens.first())
                    .map(|t| t.token_id.clone())
                    .unwrap_or_default();

                all.push(DiscoveredInstrument {
                    venue: Venue::Polymarket,
                    instrument_id: market.condition_id.clone(),
                    asset,
                    strike,
                    expiry,
                    direction,
                    is_active: true,
                    raw_expiry_timestamp: 0, // Polymarket uses date, not millisecond timestamp
                    extra_venue_id: Some(token_id),
                });
            }
        }
    }

    tracing::info!(
        venue = "polymarket",
        slugs_polled = event_slugs.len(),
        instruments_discovered = all.len(),
        "Polymarket structured discovery complete"
    );

    Ok(all)
}

// ---------------------------------------------------------------------------
// Derive discovery (public endpoint, no auth)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeriveInstrumentsResponse {
    result: Vec<DeriveInstrumentInfo>,
}

#[derive(Debug, Deserialize)]
struct DeriveInstrumentInfo {
    instrument_name: String,
    is_active: bool,
    option_details: Option<DeriveOptionDetails>,
}

#[derive(Debug, Deserialize)]
struct DeriveOptionDetails {
    strike: String,
    expiry: u64,
    option_type: String,
}

/// Discover active Derive option instruments via the public REST API.
///
/// POST `{base_url}/public/get_instruments` with JSON body specifying
/// BTC options. No authentication required for public endpoints.
///
/// Uses `Decimal::from_str` for string strike prices (Derive returns strings,
/// not floats) per Phase 31 decision.
pub async fn discover_derive(
    client: &reqwest::Client,
    base_url: &str,
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    if let Some(limiter) = rate_limiter {
        limiter.wait().await;
    }

    let url = format!("{}/public/get_instruments", base_url);
    let body = serde_json::json!({
        "instrument_type": "option",
        "currency": "BTC",
        "expired": false
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Derive get_instruments request failed")?;

    let response: DeriveInstrumentsResponse = resp
        .json()
        .await
        .context("Failed to parse Derive instruments response")?;

    let mut all = Vec::new();

    for info in response.result {
        if !info.is_active {
            continue;
        }

        let details = match &info.option_details {
            Some(d) => d,
            None => continue,
        };

        let strike = match Decimal::from_str(&details.strike) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let direction = match details.option_type.as_str() {
            "C" | "call" => Direction::Above,
            "P" | "put" => Direction::Below,
            _ => continue,
        };

        // Detect seconds vs milliseconds: if > 10 billion, treat as millis
        let expiry_ms = if details.expiry > 10_000_000_000 {
            details.expiry as i64
        } else {
            (details.expiry as i64) * 1000
        };

        let expiry_dt = match DateTime::from_timestamp_millis(expiry_ms) {
            Some(dt) => dt,
            None => continue,
        };
        let expiry = expiry_dt.date_naive();

        all.push(DiscoveredInstrument {
            venue: Venue::Derive,
            instrument_id: info.instrument_name,
            asset: "BTC".to_string(),
            strike,
            expiry,
            direction,
            is_active: true,
            raw_expiry_timestamp: expiry_ms,
            extra_venue_id: None,
        });
    }

    let count = all.len();
    tracing::info!(venue = "derive", count, "discovered Derive instruments");

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
                Venue::Derive => {} // Derive matching deferred to v1.5 Phase 31
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
                derive: None, // Derive matching deferred to v1.5 Phase 31
            },
            expiry_confidence: ExpiryConfidence::High,
        });
    }

    new_candidates
}

// ---------------------------------------------------------------------------
// Fuzzy (tolerance-based) cross-venue matching
// ---------------------------------------------------------------------------

/// Group discovered instruments by asset/strike/direction with expiry tolerance.
///
/// Pass 1: Groups all instruments by `FuzzyMatchKey` (no expiry).
/// Pass 2: Filters to groups with 2+ different venues and expiry spread
/// within `expiry_tolerance_days`. Returns each qualifying group with its
/// computed `ExpiryConfidence`.
pub fn find_cross_venue_candidates_fuzzy(
    instruments: &[DiscoveredInstrument],
    expiry_tolerance_days: i64,
) -> Vec<(FuzzyMatchKey, Vec<&DiscoveredInstrument>, ExpiryConfidence)> {
    // Pass 1: Group by FuzzyMatchKey
    let mut groups: HashMap<FuzzyMatchKey, Vec<&DiscoveredInstrument>> = HashMap::new();
    for inst in instruments {
        let key = FuzzyMatchKey::from_discovered(inst);
        groups.entry(key).or_default().push(inst);
    }

    // Pass 2: Filter to multi-venue groups within expiry tolerance
    let mut results = Vec::new();
    for (key, insts) in groups {
        // Check 2+ different venues
        let venues: HashSet<Venue> = insts.iter().map(|i| i.venue).collect();
        if venues.len() < 2 {
            continue;
        }

        // Compute expiry spread
        let expiries: Vec<NaiveDate> = insts.iter().map(|i| i.expiry).collect();
        let min_expiry = expiries.iter().min().unwrap();
        let max_expiry = expiries.iter().max().unwrap();
        let spread_days = (*max_expiry - *min_expiry).num_days();

        if spread_days > expiry_tolerance_days {
            continue;
        }

        let confidence = compute_expiry_confidence(&expiries);
        results.push((key, insts, confidence));
    }

    results
}

/// Filter fuzzy candidate groups to only those not already in the registry.
///
/// For each candidate group, uses the earliest expiry as the representative
/// date (most conservative). Generates event_id as `{ASSET}-{STRIKE}-{earliest_expiry}`.
/// Builds `CandidateMapping` with all matched venue instruments including
/// Polymarket condition_id + token_id from `extra_venue_id`.
pub fn filter_new_candidates_fuzzy(
    candidates: &[(FuzzyMatchKey, Vec<&DiscoveredInstrument>, ExpiryConfidence)],
    existing_registry: &EventRegistry,
) -> Vec<CandidateMapping> {
    let mut new_candidates = Vec::new();

    for (key, instruments, confidence) in candidates {
        // Use earliest expiry as representative date
        let earliest_expiry = instruments
            .iter()
            .map(|i| i.expiry)
            .min()
            .unwrap();

        // Generate event_id with earliest expiry
        let event_id = format!("{}-{}-{}", key.asset, key.strike, earliest_expiry);

        // Check if this event already exists in the registry
        if existing_registry.lookup_by_event_id(&event_id).is_some() {
            continue;
        }

        // Check if any individual instrument is already mapped
        let any_already_mapped = instruments.iter().any(|inst| {
            existing_registry
                .lookup_by_instrument(inst.venue, &inst.instrument_id)
                .is_some()
        });
        if any_already_mapped {
            continue;
        }

        // Build CandidateVenues from all instruments in the group
        let mut deribit: Option<String> = None;
        let mut kalshi: Option<String> = None;
        let mut polymarket: Option<(String, String)> = None;

        for inst in instruments {
            match inst.venue {
                Venue::Deribit => deribit = Some(inst.instrument_id.clone()),
                Venue::Kalshi => kalshi = Some(inst.instrument_id.clone()),
                Venue::Polymarket => {
                    polymarket = Some((
                        inst.instrument_id.clone(),
                        inst.extra_venue_id.clone().unwrap_or_default(),
                    ));
                }
                Venue::Derive => {} // Derive matching deferred to v1.5 Phase 31
            }
        }

        new_candidates.push(CandidateMapping {
            id: event_id,
            asset: key.asset.clone(),
            strike: key.strike.to_string(),
            direction: key.direction.clone(),
            expiry: earliest_expiry.to_string(),
            venues: CandidateVenues {
                deribit,
                polymarket,
                kalshi,
                derive: None, // Derive matching deferred to v1.5 Phase 31
            },
            expiry_confidence: *confidence,
        });
    }

    new_candidates
}

/// Flag instruments that exist in only one venue and are not in any existing mapping.
///
/// Uses `FuzzyMatchKey` (asset/strike/direction without expiry) so that
/// instruments with different expiry dates but matching economic parameters
/// are not flagged as novel when a cross-venue match exists within tolerance.
pub fn flag_novel_instruments<'a>(
    discovered: &'a [DiscoveredInstrument],
    existing_registry: &EventRegistry,
) -> Vec<&'a DiscoveredInstrument> {
    // Build a set of all fuzzy match keys to find which are single-venue
    let mut key_venues: HashMap<FuzzyMatchKey, HashSet<Venue>> = HashMap::new();
    for inst in discovered {
        let key = FuzzyMatchKey::from_discovered(inst);
        key_venues.entry(key).or_default().insert(inst.venue);
    }

    discovered
        .iter()
        .filter(|inst| {
            let key = FuzzyMatchKey::from_discovered(inst);
            // Single-venue: only one venue has this fuzzy match key
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
            extra_venue_id: None,
        }
    }

    fn make_discovered_with_extra(
        venue: Venue,
        instrument_id: &str,
        asset: &str,
        strike: Decimal,
        expiry: NaiveDate,
        direction: Direction,
        extra_venue_id: Option<String>,
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
            extra_venue_id,
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
                derive: None,
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

    // --- parse_polymarket_question tests ---

    #[test]
    fn parse_question_reach_above() {
        let result =
            parse_polymarket_question("Will Bitcoin reach $150,000 by December 31, 2025?");
        assert!(result.is_some());
        let (asset, strike, direction) = result.unwrap();
        assert_eq!(asset, "BTC");
        assert_eq!(strike, Decimal::from_str("150000").unwrap());
        assert_eq!(direction, Direction::Above);
    }

    #[test]
    fn parse_question_hit_above() {
        let result =
            parse_polymarket_question("Will Bitcoin hit $100,000 by December 31, 2025?");
        assert!(result.is_some());
        let (asset, strike, direction) = result.unwrap();
        assert_eq!(asset, "BTC");
        assert_eq!(strike, Decimal::from_str("100000").unwrap());
        assert_eq!(direction, Direction::Above);
    }

    #[test]
    fn parse_question_dip_below() {
        let result =
            parse_polymarket_question("Will Bitcoin dip to $75,000 by February 28, 2025?");
        assert!(result.is_some());
        let (asset, strike, direction) = result.unwrap();
        assert_eq!(asset, "BTC");
        assert_eq!(strike, Decimal::from_str("75000").unwrap());
        assert_eq!(direction, Direction::Below);
    }

    #[test]
    fn parse_question_unknown_asset() {
        let result =
            parse_polymarket_question("Will Dogecoin reach $1 by December 31, 2025?");
        assert!(result.is_none());
    }

    #[test]
    fn parse_question_no_will_prefix() {
        let result = parse_polymarket_question("Bitcoin reaches $100,000");
        assert!(result.is_none());
    }

    #[test]
    fn parse_question_ethereum() {
        let result =
            parse_polymarket_question("Will Ethereum reach $5,000 by June 30, 2025?");
        assert!(result.is_some());
        let (asset, strike, direction) = result.unwrap();
        assert_eq!(asset, "ETH");
        assert_eq!(strike, Decimal::from_str("5000").unwrap());
        assert_eq!(direction, Direction::Above);
    }

    // --- normalize_polymarket_asset tests ---

    #[test]
    fn normalize_asset_cases() {
        assert_eq!(normalize_polymarket_asset("Bitcoin"), Some("BTC"));
        assert_eq!(normalize_polymarket_asset("BITCOIN"), Some("BTC"));
        assert_eq!(normalize_polymarket_asset("bitcoin"), Some("BTC"));
        assert_eq!(normalize_polymarket_asset("ethereum"), Some("ETH"));
        assert_eq!(normalize_polymarket_asset("Ether"), Some("ETH"));
        assert_eq!(normalize_polymarket_asset("solana"), Some("SOL"));
        assert_eq!(normalize_polymarket_asset("Unknown"), None);
    }

    // --- compute_expiry_confidence tests ---

    #[test]
    fn compute_expiry_confidence_tests() {
        // Single date -> High
        let single = vec![NaiveDate::from_ymd_opt(2025, 6, 27).unwrap()];
        assert_eq!(compute_expiry_confidence(&single), ExpiryConfidence::High);

        // 1 day spread -> High
        let high = vec![
            NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 28).unwrap(),
        ];
        assert_eq!(compute_expiry_confidence(&high), ExpiryConfidence::High);

        // 5 day spread -> Medium
        let medium = vec![
            NaiveDate::from_ymd_opt(2025, 6, 25).unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
        ];
        assert_eq!(compute_expiry_confidence(&medium), ExpiryConfidence::Medium);

        // 10 day spread -> Low
        let low = vec![
            NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
        ];
        assert_eq!(compute_expiry_confidence(&low), ExpiryConfidence::Low);
    }

    // --- generate_polymarket_slugs tests ---

    #[test]
    fn generate_slugs_test() {
        let patterns = vec![
            "{month}-{year}".to_string(),
            "before-{next_year}".to_string(),
        ];
        let slugs = generate_polymarket_slugs(&patterns);
        assert_eq!(slugs.len(), 2);

        let now = chrono::Utc::now();
        let expected_month = now.format("%B").to_string().to_lowercase();
        let expected_year = now.format("%Y").to_string();
        let expected_next_year = (chrono::Datelike::year(&now.date_naive()) + 1).to_string();

        assert_eq!(slugs[0], format!("{}-{}", expected_month, expected_year));
        assert_eq!(slugs[1], format!("before-{}", expected_next_year));
    }

    // --- ExpiryConfidence Display tests ---

    #[test]
    fn expiry_confidence_display() {
        assert_eq!(ExpiryConfidence::High.to_string(), "HIGH");
        assert_eq!(ExpiryConfidence::Medium.to_string(), "MEDIUM");
        assert_eq!(ExpiryConfidence::Low.to_string(), "LOW");
    }

    // --- GammaEventResponse deserialization test ---

    #[test]
    fn parse_gamma_event_response_json() {
        let json = r#"[{
            "title": "What price will Bitcoin hit in February?",
            "markets": [
                {
                    "conditionId": "0xabc123",
                    "question": "Will Bitcoin reach $150,000 by February 28, 2025?",
                    "endDateIso": "2025-02-28",
                    "active": true,
                    "closed": false,
                    "tokens": [
                        {"token_id": "tok1", "outcome": "Yes"},
                        {"token_id": "tok2", "outcome": "No"}
                    ],
                    "category": "Crypto"
                },
                {
                    "conditionId": "0xdef456",
                    "question": "Will Bitcoin dip to $75,000 by February 28, 2025?",
                    "endDateIso": "2025-02-28",
                    "active": true,
                    "closed": false,
                    "tokens": [
                        {"token_id": "tok3", "outcome": "Yes"},
                        {"token_id": "tok4", "outcome": "No"}
                    ],
                    "category": "Crypto"
                }
            ]
        }]"#;

        let resp: Vec<GammaEventResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0].markets.len(), 2);
        assert_eq!(resp[0].markets[0].condition_id, "0xabc123");
        assert_eq!(resp[0].markets[1].condition_id, "0xdef456");
    }

    // --- FuzzyMatchKey tests ---

    #[test]
    fn fuzzy_match_same_asset_strike_direction_different_expiry() {
        // Deribit Friday expiry + Kalshi end-of-month within 7-day tolerance
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
                "KXBTCD-25JUN30-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
            ),
        ];

        let results = find_cross_venue_candidates_fuzzy(&instruments, 7);
        assert_eq!(results.len(), 1);
        let (key, group, confidence) = &results[0];
        assert_eq!(key.asset, "BTC");
        assert_eq!(key.strike, Decimal::from_str("100000").unwrap());
        assert_eq!(key.direction, Direction::Above);
        assert_eq!(group.len(), 2);
        // 3-day spread -> Medium confidence
        assert_eq!(*confidence, ExpiryConfidence::Medium);
    }

    #[test]
    fn fuzzy_match_expiry_exceeds_tolerance() {
        // Same instruments but with 2-day tolerance -> 3-day spread exceeds it
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
                "KXBTCD-25JUN30-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
            ),
        ];

        let results = find_cross_venue_candidates_fuzzy(&instruments, 2);
        assert!(results.is_empty(), "3-day spread should exceed 2-day tolerance");
    }

    #[test]
    fn fuzzy_match_three_venues() {
        // Deribit + Kalshi + Polymarket all BTC-100000-Above within 5 days
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
                "KXBTCD-25JUN30-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
            ),
            make_discovered_with_extra(
                Venue::Polymarket,
                "0xabc123",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
                Some("tok_yes".to_string()),
            ),
        ];

        let results = find_cross_venue_candidates_fuzzy(&instruments, 7);
        assert_eq!(results.len(), 1);
        let (_, group, _) = &results[0];
        assert_eq!(group.len(), 3);
        let venues: HashSet<Venue> = group.iter().map(|i| i.venue).collect();
        assert!(venues.contains(&Venue::Deribit));
        assert!(venues.contains(&Venue::Kalshi));
        assert!(venues.contains(&Venue::Polymarket));
    }

    #[test]
    fn fuzzy_match_high_confidence() {
        // All venues expire on same date -> High confidence
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

        let results = find_cross_venue_candidates_fuzzy(&instruments, 7);
        assert_eq!(results.len(), 1);
        let (_, _, confidence) = &results[0];
        assert_eq!(*confidence, ExpiryConfidence::High);
    }

    #[test]
    fn fuzzy_match_excludes_single_venue() {
        // Only Deribit instruments -> empty results
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

        let results = find_cross_venue_candidates_fuzzy(&instruments, 7);
        assert!(results.is_empty());
    }

    #[test]
    fn filter_fuzzy_generates_correct_event_id() {
        // Verify event_id uses earliest expiry
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
                "KXBTCD-25JUN30-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
            ),
        ];

        let candidates = find_cross_venue_candidates_fuzzy(&instruments, 7);
        let registry = make_empty_registry();
        let new = filter_new_candidates_fuzzy(&candidates, &registry);

        assert_eq!(new.len(), 1);
        // Event ID should use June 27 (earliest expiry)
        assert_eq!(new[0].id, "BTC-100000-2025-06-27");
        assert_eq!(new[0].expiry, "2025-06-27");
    }

    #[test]
    fn filter_fuzzy_skips_existing() {
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
                "KXBTCD-25JUN30-T100000",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
            ),
        ];

        let candidates = find_cross_venue_candidates_fuzzy(&instruments, 7);
        let registry = make_registry_with(vec![make_mapping(
            "BTC-100000-2025-06-27",
            "BTC-27JUN25-100000-C",
        )]);

        let new = filter_new_candidates_fuzzy(&candidates, &registry);
        assert!(new.is_empty(), "should skip already-registered mapping");
    }

    #[test]
    fn filter_fuzzy_includes_polymarket_venue_ids() {
        let instruments = vec![
            make_discovered(
                Venue::Deribit,
                "BTC-27JUN25-100000-C",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 27).unwrap(),
                Direction::Above,
            ),
            make_discovered_with_extra(
                Venue::Polymarket,
                "0xabc123",
                "BTC",
                Decimal::from_str("100000").unwrap(),
                NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
                Direction::Above,
                Some("tok_yes_123".to_string()),
            ),
        ];

        let candidates = find_cross_venue_candidates_fuzzy(&instruments, 7);
        let registry = make_empty_registry();
        let new = filter_new_candidates_fuzzy(&candidates, &registry);

        assert_eq!(new.len(), 1);
        let polymarket = new[0].venues.polymarket.as_ref().unwrap();
        assert_eq!(polymarket.0, "0xabc123"); // condition_id
        assert_eq!(polymarket.1, "tok_yes_123"); // token_id from extra_venue_id
    }
}
