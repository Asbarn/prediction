use serde::{Deserialize, Serialize};

/// Event mapping configuration loaded from `events.toml`.
///
/// Maps canonical event IDs to venue-specific instrument identifiers.
/// Each event can be tracked across one or more venues.
/// Extended in Phase 5 with risk weights, discovery config, expiry thresholds,
/// and per-mapping approval/lifecycle/settlement metadata.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventsConfig {
    /// List of event mappings, each with a canonical ID and venue-specific
    /// instrument identifiers.
    pub events: Vec<EventMapping>,

    /// Risk weight configuration for basis risk scoring.
    #[serde(default)]
    pub risk_weights: Option<RiskWeightsConfig>,

    /// Discovery configuration for periodic venue polling.
    #[serde(default)]
    pub discovery: Option<DiscoveryConfig>,

    /// Expiry warning thresholds (caution, warning, critical).
    #[serde(default)]
    pub expiry_thresholds: Vec<ExpiryThreshold>,
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
    /// Direction: above or below.
    #[serde(default = "default_direction")]
    pub direction: Direction,
    /// Expiry date as string (e.g., "2025-06-30").
    pub expiry: String,
    /// Venue-specific instrument mappings.
    pub venues: EventVenues,

    /// Whether this mapping is approved for active use.
    /// Default true for backward compatibility with existing mappings.
    #[serde(default = "default_true")]
    pub approved: bool,

    /// Lifecycle status of this mapping.
    #[serde(default)]
    pub status: LifecycleStatus,

    /// RFC3339 timestamp when this mapping was auto-discovered.
    /// None for user-authored mappings.
    #[serde(default)]
    pub discovered_at: Option<String>,

    /// Settlement metadata for basis risk scoring.
    #[serde(default)]
    pub settlement: Option<SettlementMetadata>,
}

fn default_true() -> bool {
    true
}

fn default_direction() -> Direction {
    Direction::Above
}

/// Direction of the prediction/option (above or below a strike).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Above,
    Below,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Above => write!(f, "above"),
            Direction::Below => write!(f, "below"),
        }
    }
}

/// Lifecycle status of an event mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStatus {
    Active,
    Expiring,
    Expired,
}

impl Default for LifecycleStatus {
    fn default() -> Self {
        LifecycleStatus::Active
    }
}

impl std::fmt::Display for LifecycleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleStatus::Active => write!(f, "active"),
            LifecycleStatus::Expiring => write!(f, "expiring"),
            LifecycleStatus::Expired => write!(f, "expired"),
        }
    }
}

/// Settlement metadata for basis risk scoring per mapping.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SettlementMetadata {
    /// Deribit settlement time (ISO 8601 datetime, e.g., "2025-06-27T08:00:00Z").
    #[serde(default)]
    pub deribit_settlement_time: Option<String>,
    /// Deribit settlement source (e.g., "deribit_index").
    #[serde(default)]
    pub deribit_settlement_source: Option<String>,
    /// Polymarket resolution source (e.g., "oracle").
    #[serde(default)]
    pub polymarket_resolution_source: Option<String>,
    /// Kalshi resolution source (e.g., "index").
    #[serde(default)]
    pub kalshi_resolution_source: Option<String>,
}

/// Risk weight configuration for basis risk composite scoring.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RiskWeightsConfig {
    /// Risk per hour of settlement time difference.
    #[serde(default = "default_time_per_hour")]
    pub time_per_hour: f64,
    /// Weight of time risk in composite score.
    #[serde(default = "default_time_weight")]
    pub time_weight: f64,
    /// Weight of source risk in composite score.
    #[serde(default = "default_source_weight")]
    pub source_weight: f64,
    /// Weight of criteria risk in composite score.
    #[serde(default = "default_criteria_weight")]
    pub criteria_weight: f64,
    /// Source pair risk weights (categorical).
    #[serde(default)]
    pub source_pairs: SourcePairWeights,
}

impl Default for RiskWeightsConfig {
    fn default() -> Self {
        Self {
            time_per_hour: 0.05,
            time_weight: 0.4,
            source_weight: 0.4,
            criteria_weight: 0.2,
            source_pairs: SourcePairWeights::default(),
        }
    }
}

fn default_time_per_hour() -> f64 { 0.05 }
fn default_time_weight() -> f64 { 0.4 }
fn default_source_weight() -> f64 { 0.4 }
fn default_criteria_weight() -> f64 { 0.2 }

/// Categorical risk weights for settlement source pairs.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourcePairWeights {
    /// Index vs. index: both using official exchange index.
    #[serde(default)]
    pub index_index: f64,
    /// Index vs. oracle: one uses exchange index, one uses oracle.
    #[serde(default = "default_index_oracle")]
    pub index_oracle: f64,
    /// Oracle vs. oracle: both use independent oracles.
    #[serde(default = "default_oracle_oracle")]
    pub oracle_oracle: f64,
    /// Oracle vs. index (reverse of index_oracle, same weight).
    #[serde(default = "default_index_oracle")]
    pub oracle_index: f64,
}

impl Default for SourcePairWeights {
    fn default() -> Self {
        Self {
            index_index: 0.0,
            index_oracle: 0.5,
            oracle_oracle: 0.2,
            oracle_index: 0.5,
        }
    }
}

fn default_index_oracle() -> f64 { 0.5 }
fn default_oracle_oracle() -> f64 { 0.2 }

/// Discovery configuration for periodic venue polling.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
    /// Poll interval for Deribit instrument discovery (seconds).
    #[serde(default = "default_deribit_poll")]
    pub deribit_poll_interval_secs: u64,
    /// Poll interval for Kalshi market discovery (seconds).
    #[serde(default = "default_kalshi_poll")]
    pub kalshi_poll_interval_secs: u64,
    /// Poll interval for Polymarket market discovery (seconds).
    #[serde(default = "default_polymarket_poll")]
    pub polymarket_poll_interval_secs: u64,
    /// Deribit currencies to discover options for.
    #[serde(default = "default_deribit_currencies")]
    pub deribit_currencies: Vec<String>,
    /// Kalshi series tickers to monitor.
    #[serde(default = "default_kalshi_series")]
    pub kalshi_series_tickers: Vec<String>,
    /// Number of consecutive polls where an instrument must be absent
    /// before it is marked expired. Prevents false expirations from
    /// partial API responses. Default 3.
    #[serde(default = "default_consecutive_absence_threshold")]
    pub consecutive_absence_threshold: u32,
    /// Fractional drop threshold (0.0-1.0) that triggers suspect partial
    /// response detection. If instrument count drops by more than this
    /// fraction vs. the previous poll, expiry evaluation is skipped for
    /// that venue. Default 0.2 (20%).
    #[serde(default = "default_partial_response_threshold")]
    pub partial_response_threshold: f64,
}

impl DiscoveryConfig {
    /// Return the minimum poll interval across all venues.
    ///
    /// Used by the lifecycle manager as its tick interval; individual venues
    /// are polled only when their own interval has elapsed.
    pub fn min_poll_interval_secs(&self) -> u64 {
        self.deribit_poll_interval_secs
            .min(self.kalshi_poll_interval_secs)
            .min(self.polymarket_poll_interval_secs)
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            deribit_poll_interval_secs: 300,
            kalshi_poll_interval_secs: 600,
            polymarket_poll_interval_secs: 600,
            deribit_currencies: vec!["BTC".to_string()],
            kalshi_series_tickers: vec!["KXBTC".to_string()],
            consecutive_absence_threshold: default_consecutive_absence_threshold(),
            partial_response_threshold: default_partial_response_threshold(),
        }
    }
}

fn default_deribit_poll() -> u64 { 300 }
fn default_kalshi_poll() -> u64 { 600 }
fn default_polymarket_poll() -> u64 { 600 }
fn default_deribit_currencies() -> Vec<String> { vec!["BTC".to_string()] }
fn default_kalshi_series() -> Vec<String> { vec!["KXBTC".to_string()] }
fn default_consecutive_absence_threshold() -> u32 { 3 }
fn default_partial_response_threshold() -> f64 { 0.2 }

/// Expiry warning threshold configuration.
///
/// Multiple tiers (caution, warning, critical) each with escalating
/// flags and risk inflation factors.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpiryThreshold {
    /// Threshold name (e.g., "caution", "warning", "critical").
    pub name: String,
    /// Hours before expiry at which this threshold activates.
    pub hours_before_expiry: u64,
    /// Flags to apply when this threshold is active.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Factor by which to inflate settlement_time_risk.
    #[serde(default = "default_inflation")]
    pub risk_inflation_factor: f64,
}

fn default_inflation() -> f64 { 1.0 }

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
