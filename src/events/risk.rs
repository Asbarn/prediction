use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::{EventMapping, ExpiryThreshold, RiskWeightsConfig, SettlementMetadata};

/// Represents the combination of settlement sources between two venues.
///
/// Used for categorical risk scoring: index-index pairs have lowest basis risk,
/// while mixed source pairs (index-oracle) have higher risk due to potential
/// price divergence at settlement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourcePair {
    /// Both venues use an index price (lowest risk).
    IndexIndex,
    /// One uses index, the other uses an oracle.
    IndexOracle,
    /// Both use oracles (moderate risk -- different oracles may diverge).
    OracleOracle,
    /// Reverse of IndexOracle (same weight by convention).
    OracleIndex,
    /// Source pair could not be determined from available metadata.
    Unknown,
}

impl SourcePair {
    /// Map a pair of settlement source strings to a typed SourcePair.
    ///
    /// Recognizes "deribit_index", "index" as index sources and "oracle" as
    /// oracle sources. Unknown strings produce `SourcePair::Unknown`.
    pub fn from_sources(source_a: &str, source_b: &str) -> Self {
        let classify = |s: &str| -> Option<bool> {
            let lower = s.to_lowercase();
            if lower.contains("index") {
                Some(true) // is_index
            } else if lower.contains("oracle") {
                Some(false) // is_oracle
            } else {
                None
            }
        };

        match (classify(source_a), classify(source_b)) {
            (Some(true), Some(true)) => SourcePair::IndexIndex,
            (Some(true), Some(false)) => SourcePair::IndexOracle,
            (Some(false), Some(true)) => SourcePair::OracleIndex,
            (Some(false), Some(false)) => SourcePair::OracleOracle,
            _ => SourcePair::Unknown,
        }
    }
}

/// Quantified basis risk score for a cross-venue event mapping.
///
/// Three independent component scores plus a weighted composite:
/// - `settlement_time_risk`: Linear with hours of temporal mismatch
/// - `source_risk`: Categorical weight based on settlement source pair
/// - `criteria_risk`: Qualitative difference between resolution criteria
/// - `composite`: Weighted sum of the three components
///
/// Risk is annotation-only -- no automatic signal suppression based on level.
#[derive(Debug, Clone, Serialize)]
pub struct BasisRiskScore {
    /// Risk from temporal mismatch between venue settlements (linear with hours).
    pub settlement_time_risk: f64,
    /// Risk from differing settlement data sources (categorical weight).
    pub source_risk: f64,
    /// Risk from differing resolution criteria (0.0 = identical, 1.0 = very different).
    pub criteria_risk: f64,
    /// Weighted composite of all three risk components.
    pub composite: f64,
}

/// Compute basis risk score from individual components.
///
/// # Arguments
/// - `settlement_time_diff_hours`: Absolute hours between Deribit settlement and
///   prediction market resolution
/// - `source_pair`: Categorical source pair for the two venues
/// - `criteria_diff`: Qualitative criteria difference (0.0-1.0)
/// - `weights`: Risk weight configuration from events.toml
///
/// # Returns
/// A `BasisRiskScore` with computed component scores and weighted composite.
pub fn compute_basis_risk(
    settlement_time_diff_hours: f64,
    source_pair: &SourcePair,
    criteria_diff: f64,
    weights: &RiskWeightsConfig,
) -> BasisRiskScore {
    let settlement_time_risk = settlement_time_diff_hours * weights.time_per_hour;

    let source_risk = match source_pair {
        SourcePair::IndexIndex => weights.source_pairs.index_index,
        SourcePair::IndexOracle => weights.source_pairs.index_oracle,
        SourcePair::OracleOracle => weights.source_pairs.oracle_oracle,
        SourcePair::OracleIndex => weights.source_pairs.oracle_index,
        SourcePair::Unknown => weights.source_pairs.index_oracle, // conservative default
    };

    let criteria_risk = criteria_diff;

    let composite = weights.time_weight * settlement_time_risk
        + weights.source_weight * source_risk
        + weights.criteria_weight * criteria_risk;

    BasisRiskScore {
        settlement_time_risk,
        source_risk,
        criteria_risk,
        composite,
    }
}

/// Compute the absolute settlement time difference in hours between two
/// ISO 8601 (RFC 3339) datetime strings.
///
/// Returns `None` if either string fails to parse as a valid datetime.
pub fn settlement_time_diff_hours(
    deribit_settlement: &str,
    prediction_resolution: &str,
) -> Option<f64> {
    let dt_a = DateTime::parse_from_rfc3339(deribit_settlement).ok()?;
    let dt_b = DateTime::parse_from_rfc3339(prediction_resolution).ok()?;
    let diff = dt_a.signed_duration_since(dt_b);
    Some(diff.num_minutes().unsigned_abs() as f64 / 60.0)
}

/// Convert a Deribit expiry date string (e.g., "2025-06-27") to the standard
/// Deribit settlement datetime string "2025-06-27T08:00:00Z" (always Friday 08:00 UTC).
///
/// This is a convenience helper for mappings that only have the expiry date.
/// Returns `None` if the date string is not in YYYY-MM-DD format (basic validation).
pub fn deribit_settlement_time(expiry_date: &str) -> Option<String> {
    // Basic validation: must be exactly YYYY-MM-DD format
    if expiry_date.len() != 10 {
        return None;
    }
    // Verify it parses as a valid date via chrono
    chrono::NaiveDate::parse_from_str(expiry_date, "%Y-%m-%d").ok()?;
    Some(format!("{expiry_date}T08:00:00Z"))
}

/// Near-expiry warning annotation for an event mapping.
///
/// When a mapping is within a configured threshold of its expiry, an
/// ExpiryWarning captures the tier, time remaining, applicable flags,
/// and the risk inflation factor to apply to settlement_time_risk.
#[derive(Debug, Clone, Serialize)]
pub struct ExpiryWarning {
    /// Threshold tier name (e.g., "caution", "warning", "critical").
    pub tier_name: String,
    /// Hours remaining until expiry.
    pub hours_to_expiry: f64,
    /// Flags applicable at this tier (e.g., "pricing_character_change", "liquidity_warning").
    pub flags: Vec<String>,
    /// Factor by which to inflate settlement_time_risk.
    pub risk_inflation_factor: f64,
}

/// Check whether an expiry datetime falls within any configured warning threshold.
///
/// Finds the most severe (tightest) tier: the threshold with the smallest
/// `hours_before_expiry` that is still >= `hours_to_expiry`. This ensures
/// that e.g. 3 hours to expiry triggers "critical" (6h) rather than "caution" (48h).
///
/// Returns `None` if already expired (hours_to_expiry <= 0) or if no
/// threshold matches (hours_to_expiry > largest threshold).
pub fn check_expiry_warning(
    expiry_datetime: &DateTime<Utc>,
    now: &DateTime<Utc>,
    thresholds: &[ExpiryThreshold],
) -> Option<ExpiryWarning> {
    let diff = *expiry_datetime - *now;
    let hours_to_expiry = diff.num_minutes() as f64 / 60.0;

    // Already expired
    if hours_to_expiry <= 0.0 {
        return None;
    }

    // Find all thresholds where hours_to_expiry <= threshold.hours_before_expiry
    // Then pick the one with the smallest hours_before_expiry (most severe/tightest)
    let matching = thresholds
        .iter()
        .filter(|t| hours_to_expiry <= t.hours_before_expiry as f64)
        .min_by(|a, b| a.hours_before_expiry.cmp(&b.hours_before_expiry));

    matching.map(|t| ExpiryWarning {
        tier_name: t.name.clone(),
        hours_to_expiry,
        flags: t.flags.clone(),
        risk_inflation_factor: t.risk_inflation_factor,
    })
}

/// Create a new BasisRiskScore with settlement_time_risk inflated by the
/// given factor, and the composite score recalculated accordingly.
///
/// Per user decision: "Near-expiry warnings both annotate the mapping AND
/// inflate the settlement_time_risk component."
///
/// Only `settlement_time_risk` is multiplied; `source_risk` and `criteria_risk`
/// are preserved. The composite is recalculated using the same weight proportions
/// as the original computation.
pub fn inflate_risk_score(score: &BasisRiskScore, inflation_factor: f64) -> BasisRiskScore {
    let inflated_time_risk = score.settlement_time_risk * inflation_factor;

    // Recalculate composite using default weight proportions.
    // We use the standard weights since the original composite was computed with them.
    let weights = RiskWeightsConfig::default();
    let composite = weights.time_weight * inflated_time_risk
        + weights.source_weight * score.source_risk
        + weights.criteria_weight * score.criteria_risk;

    BasisRiskScore {
        settlement_time_risk: inflated_time_risk,
        source_risk: score.source_risk,
        criteria_risk: score.criteria_risk,
        composite,
    }
}

/// Compute basis risk for an EventMapping using its settlement metadata.
///
/// Extracts settlement time and source information from the mapping's
/// `SettlementMetadata`, computes the appropriate SourcePair, and calls
/// `compute_basis_risk`. Returns `None` if settlement metadata is missing
/// or if timestamps cannot be parsed.
///
/// Note: This uses `criteria_diff = 0.0` as default since criteria difference
/// requires manual assessment per mapping. Callers can override via
/// `compute_basis_risk` directly for mappings with known criteria differences.
pub fn compute_risk_for_mapping(
    mapping: &EventMapping,
    weights: &RiskWeightsConfig,
) -> Option<BasisRiskScore> {
    let settlement = mapping.settlement.as_ref()?;

    // Compute settlement time difference
    let deribit_time = settlement.deribit_settlement_time.as_deref()?;

    // Use the expiry date to derive a prediction market resolution time estimate.
    // Prediction markets typically resolve at midnight UTC on or after the expiry date.
    // If we had an explicit prediction_resolution field, we would use it.
    // For now, use the expiry date at 00:00:00 UTC as the best estimate.
    let prediction_resolution = format!("{}T00:00:00Z", mapping.expiry);
    let time_diff = settlement_time_diff_hours(deribit_time, &prediction_resolution)?;

    // Determine source pair from settlement metadata
    let source_pair = determine_source_pair(settlement);

    Some(compute_basis_risk(time_diff, &source_pair, 0.0, weights))
}

/// Determine the SourcePair for a settlement metadata entry.
///
/// Checks Polymarket resolution source first (most common cross-venue pair),
/// then Kalshi. Falls back to Unknown if no prediction market source is available.
fn determine_source_pair(settlement: &SettlementMetadata) -> SourcePair {
    let deribit_source = match settlement.deribit_settlement_source.as_deref() {
        Some(s) => s,
        None => return SourcePair::Unknown,
    };

    // Prefer Polymarket source, fall back to Kalshi
    if let Some(ref poly_source) = settlement.polymarket_resolution_source {
        return SourcePair::from_sources(deribit_source, poly_source);
    }
    if let Some(ref kalshi_source) = settlement.kalshi_resolution_source {
        return SourcePair::from_sources(deribit_source, kalshi_source);
    }

    SourcePair::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DeribitMapping, Direction, EventMapping, EventVenues, ExpiryThreshold, RiskWeightsConfig,
        SettlementMetadata,
    };
    use chrono::{TimeZone, Utc};

    fn default_weights() -> RiskWeightsConfig {
        RiskWeightsConfig::default()
    }

    fn make_thresholds() -> Vec<ExpiryThreshold> {
        vec![
            ExpiryThreshold {
                name: "caution".to_string(),
                hours_before_expiry: 48,
                flags: vec!["pricing_character_change".to_string()],
                risk_inflation_factor: 1.2,
            },
            ExpiryThreshold {
                name: "warning".to_string(),
                hours_before_expiry: 24,
                flags: vec![
                    "pricing_character_change".to_string(),
                    "liquidity_warning".to_string(),
                ],
                risk_inflation_factor: 1.5,
            },
            ExpiryThreshold {
                name: "critical".to_string(),
                hours_before_expiry: 6,
                flags: vec![
                    "pricing_character_change".to_string(),
                    "liquidity_warning".to_string(),
                    "elevated_settlement_risk".to_string(),
                ],
                risk_inflation_factor: 2.0,
            },
        ]
    }

    // --- compute_basis_risk tests ---

    #[test]
    fn compute_basis_risk_known_inputs() {
        let weights = default_weights();
        // 10 hours diff, index-oracle, criteria_diff = 0.3
        let score = compute_basis_risk(10.0, &SourcePair::IndexOracle, 0.3, &weights);

        // settlement_time_risk = 10.0 * 0.05 = 0.5
        assert!((score.settlement_time_risk - 0.5).abs() < 1e-10);
        // source_risk = 0.5 (index_oracle weight)
        assert!((score.source_risk - 0.5).abs() < 1e-10);
        // criteria_risk = 0.3
        assert!((score.criteria_risk - 0.3).abs() < 1e-10);
        // composite = 0.4*0.5 + 0.4*0.5 + 0.2*0.3 = 0.2 + 0.2 + 0.06 = 0.46
        assert!((score.composite - 0.46).abs() < 1e-10);
    }

    #[test]
    fn compute_basis_risk_zero_time_difference() {
        let weights = default_weights();
        let score = compute_basis_risk(0.0, &SourcePair::IndexIndex, 0.0, &weights);

        assert!((score.settlement_time_risk - 0.0).abs() < 1e-10);
        assert!((score.source_risk - 0.0).abs() < 1e-10);
        assert!((score.criteria_risk - 0.0).abs() < 1e-10);
        assert!((score.composite - 0.0).abs() < 1e-10);
    }

    #[test]
    fn compute_basis_risk_default_weights_produce_expected_scores() {
        let weights = default_weights();
        // 24 hours diff, oracle-oracle pair, criteria 0.5
        let score = compute_basis_risk(24.0, &SourcePair::OracleOracle, 0.5, &weights);

        // settlement_time_risk = 24.0 * 0.05 = 1.2
        assert!((score.settlement_time_risk - 1.2).abs() < 1e-10);
        // source_risk = 0.2 (oracle_oracle)
        assert!((score.source_risk - 0.2).abs() < 1e-10);
        // criteria_risk = 0.5
        assert!((score.criteria_risk - 0.5).abs() < 1e-10);
        // composite = 0.4*1.2 + 0.4*0.2 + 0.2*0.5 = 0.48 + 0.08 + 0.10 = 0.66
        assert!((score.composite - 0.66).abs() < 1e-10);
    }

    #[test]
    fn compute_basis_risk_unknown_source_pair_uses_conservative_default() {
        let weights = default_weights();
        let score = compute_basis_risk(5.0, &SourcePair::Unknown, 0.0, &weights);

        // Unknown uses index_oracle weight (0.5) as conservative default
        assert!((score.source_risk - 0.5).abs() < 1e-10);
    }

    // --- settlement_time_diff_hours tests ---

    #[test]
    fn settlement_time_diff_same_day() {
        let result = settlement_time_diff_hours(
            "2025-06-27T08:00:00Z",
            "2025-06-27T00:00:00Z",
        );
        assert!(result.is_some());
        assert!((result.unwrap() - 8.0).abs() < 1e-10);
    }

    #[test]
    fn settlement_time_diff_multi_day_gap() {
        let result = settlement_time_diff_hours(
            "2025-06-27T08:00:00Z",
            "2025-06-30T12:00:00Z",
        );
        assert!(result.is_some());
        // 3 days + 4 hours = 76 hours
        assert!((result.unwrap() - 76.0).abs() < 1e-10);
    }

    #[test]
    fn settlement_time_diff_unparseable_input_returns_none() {
        assert!(settlement_time_diff_hours("not-a-date", "2025-06-27T08:00:00Z").is_none());
        assert!(settlement_time_diff_hours("2025-06-27T08:00:00Z", "garbage").is_none());
        assert!(settlement_time_diff_hours("", "").is_none());
    }

    // --- SourcePair::from_sources tests ---

    #[test]
    fn source_pair_from_sources_all_combinations() {
        assert_eq!(
            SourcePair::from_sources("deribit_index", "index"),
            SourcePair::IndexIndex
        );
        assert_eq!(
            SourcePair::from_sources("deribit_index", "oracle"),
            SourcePair::IndexOracle
        );
        assert_eq!(
            SourcePair::from_sources("oracle", "deribit_index"),
            SourcePair::OracleIndex
        );
        assert_eq!(
            SourcePair::from_sources("oracle", "oracle"),
            SourcePair::OracleOracle
        );
        assert_eq!(
            SourcePair::from_sources("index", "INDEX"),
            SourcePair::IndexIndex
        );
        assert_eq!(
            SourcePair::from_sources("unknown_source", "oracle"),
            SourcePair::Unknown
        );
        assert_eq!(
            SourcePair::from_sources("random", "other"),
            SourcePair::Unknown
        );
    }

    // --- check_expiry_warning tests ---

    #[test]
    fn expiry_warning_none_when_outside_all_thresholds() {
        let thresholds = make_thresholds();
        let expiry = Utc.with_ymd_and_hms(2025, 6, 27, 8, 0, 0).unwrap();
        // 100 hours before expiry -- outside all thresholds
        let now = expiry - chrono::Duration::hours(100);
        assert!(check_expiry_warning(&expiry, &now, &thresholds).is_none());
    }

    #[test]
    fn expiry_warning_caution_tier_at_40_hours() {
        let thresholds = make_thresholds();
        let expiry = Utc.with_ymd_and_hms(2025, 6, 27, 8, 0, 0).unwrap();
        // 40 hours before expiry -- inside caution (48h) but outside warning (24h)
        let now = expiry - chrono::Duration::hours(40);
        let warning = check_expiry_warning(&expiry, &now, &thresholds);

        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.tier_name, "caution");
        assert!((w.hours_to_expiry - 40.0).abs() < 1e-10);
        assert_eq!(w.flags, vec!["pricing_character_change"]);
        assert!((w.risk_inflation_factor - 1.2).abs() < 1e-10);
    }

    #[test]
    fn expiry_warning_critical_tier_at_3_hours() {
        let thresholds = make_thresholds();
        let expiry = Utc.with_ymd_and_hms(2025, 6, 27, 8, 0, 0).unwrap();
        // 3 hours before expiry -- inside critical (6h)
        let now = expiry - chrono::Duration::hours(3);
        let warning = check_expiry_warning(&expiry, &now, &thresholds);

        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.tier_name, "critical");
        assert!((w.hours_to_expiry - 3.0).abs() < 1e-10);
        assert_eq!(
            w.flags,
            vec![
                "pricing_character_change",
                "liquidity_warning",
                "elevated_settlement_risk"
            ]
        );
        assert!((w.risk_inflation_factor - 2.0).abs() < 1e-10);
    }

    #[test]
    fn expiry_warning_none_when_already_expired() {
        let thresholds = make_thresholds();
        let expiry = Utc.with_ymd_and_hms(2025, 6, 27, 8, 0, 0).unwrap();
        // 2 hours AFTER expiry
        let now = expiry + chrono::Duration::hours(2);
        assert!(check_expiry_warning(&expiry, &now, &thresholds).is_none());
    }

    #[test]
    fn expiry_warning_empty_thresholds_returns_none() {
        let expiry = Utc.with_ymd_and_hms(2025, 6, 27, 8, 0, 0).unwrap();
        let now = expiry - chrono::Duration::hours(3);
        assert!(check_expiry_warning(&expiry, &now, &[]).is_none());
    }

    // --- inflate_risk_score tests ---

    #[test]
    fn inflate_risk_score_multiplies_settlement_time_risk() {
        let weights = default_weights();
        let score = compute_basis_risk(10.0, &SourcePair::IndexOracle, 0.3, &weights);
        let inflated = inflate_risk_score(&score, 2.0);

        // settlement_time_risk should be doubled: 0.5 * 2.0 = 1.0
        assert!((inflated.settlement_time_risk - 1.0).abs() < 1e-10);
        // source_risk and criteria_risk unchanged
        assert!((inflated.source_risk - 0.5).abs() < 1e-10);
        assert!((inflated.criteria_risk - 0.3).abs() < 1e-10);
    }

    #[test]
    fn inflate_risk_score_recalculates_composite() {
        let weights = default_weights();
        let score = compute_basis_risk(10.0, &SourcePair::IndexOracle, 0.3, &weights);
        let inflated = inflate_risk_score(&score, 2.0);

        // composite = 0.4*1.0 + 0.4*0.5 + 0.2*0.3 = 0.40 + 0.20 + 0.06 = 0.66
        assert!((inflated.composite - 0.66).abs() < 1e-10);
    }

    // --- deribit_settlement_time tests ---

    #[test]
    fn deribit_settlement_time_valid_date() {
        let result = deribit_settlement_time("2025-06-27");
        assert_eq!(result.unwrap(), "2025-06-27T08:00:00Z");
    }

    #[test]
    fn deribit_settlement_time_invalid_date() {
        assert!(deribit_settlement_time("not-a-date").is_none());
        assert!(deribit_settlement_time("2025-13-01").is_none());
        assert!(deribit_settlement_time("").is_none());
    }

    // --- compute_risk_for_mapping tests ---

    #[test]
    fn compute_risk_for_mapping_with_settlement_metadata() {
        let mapping = EventMapping {
            id: "BTC-100K-2025-06-27".to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-06-27".to_string(),
            venues: EventVenues {
                deribit: Some(DeribitMapping {
                    instrument: "BTC-27JUN25-100000-C".to_string(),
                }),
                polymarket: None,
                kalshi: None,
            },
            approved: true,
            status: crate::config::LifecycleStatus::Active,
            discovered_at: None,
            settlement: Some(SettlementMetadata {
                deribit_settlement_time: Some("2025-06-27T08:00:00Z".to_string()),
                deribit_settlement_source: Some("deribit_index".to_string()),
                polymarket_resolution_source: Some("oracle".to_string()),
                kalshi_resolution_source: None,
            }),
        };

        let weights = default_weights();
        let result = compute_risk_for_mapping(&mapping, &weights);
        assert!(result.is_some());
        let score = result.unwrap();
        // Time diff: 2025-06-27T08:00:00Z vs 2025-06-27T00:00:00Z = 8 hours
        assert!((score.settlement_time_risk - (8.0 * 0.05)).abs() < 1e-10);
        // Source: deribit_index vs oracle = IndexOracle = 0.5
        assert!((score.source_risk - 0.5).abs() < 1e-10);
    }

    #[test]
    fn compute_risk_for_mapping_missing_settlement_returns_none() {
        let mapping = EventMapping {
            id: "BTC-100K-2025-06-27".to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-06-27".to_string(),
            venues: EventVenues {
                deribit: None,
                polymarket: None,
                kalshi: None,
            },
            approved: true,
            status: crate::config::LifecycleStatus::Active,
            discovered_at: None,
            settlement: None,
        };

        let weights = default_weights();
        assert!(compute_risk_for_mapping(&mapping, &weights).is_none());
    }
}
