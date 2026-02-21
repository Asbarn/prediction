use serde::{Deserialize, Serialize};

/// Event mapping configuration loaded from `events.toml`.
///
/// Maps canonical event IDs to venue-specific instrument identifiers.
/// Each event can be tracked across one or more venues.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventsConfig {
    /// List of event mappings, each with a canonical ID and venue-specific
    /// instrument identifiers.
    pub events: Vec<EventMapping>,
}

/// A single event mapping from canonical ID to venue instruments.
///
/// Strike and expiry are stored as strings to preserve precision and
/// formatting -- they will be parsed to `Decimal` and `NaiveDate`
/// downstream when needed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventMapping {
    /// Canonical event ID (e.g., "BTC-100K-2025-06-30").
    pub id: String,
    /// Underlying asset (e.g., "BTC").
    pub asset: String,
    /// Strike price as string to preserve precision.
    pub strike: String,
    /// Direction: "above" or "below".
    pub direction: String,
    /// Expiry date as string (e.g., "2025-06-30").
    pub expiry: String,
    /// Venue-specific instrument mappings.
    pub venues: EventVenues,
}

/// Venue-specific instrument identifiers for a single event.
///
/// All fields are optional -- an event needs at least one venue mapping
/// to be useful, which is validated in `validation.rs`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventVenues {
    pub deribit: Option<DeribitMapping>,
    pub polymarket: Option<PolymarketMapping>,
    pub kalshi: Option<KalshiMapping>,
}

/// Deribit instrument mapping.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeribitMapping {
    /// Deribit instrument name (e.g., "BTC-27JUN25-100000-C").
    pub instrument: String,
}

/// Polymarket instrument mapping.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PolymarketMapping {
    /// Polymarket condition ID (hex string).
    pub condition_id: String,
    /// Polymarket token ID.
    pub token_id: String,
}

/// Kalshi instrument mapping.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KalshiMapping {
    /// Kalshi ticker (e.g., "KXBTCD-25JUN30-T100000").
    pub ticker: String,
}
