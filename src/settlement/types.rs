//! Settlement domain types for outcome tracking across all venues.
//!
//! All types are Serialize/Deserialize for JSONL logging and checkpoint persistence.
//! Decimal fields use `#[serde(with = "rust_decimal::serde::str")]` for human-readable
//! JSON serialization.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::Direction;
use crate::types::Venue;

use super::config::SettlementConfig;

/// The kind of outcome for a settled event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum OutcomeKind {
    /// Event resolved YES (e.g., BTC above strike at expiry).
    Yes,
    /// Event resolved NO (e.g., BTC below strike at expiry).
    No,
    /// Ambiguous resolution (e.g., Kalshi Rule 6.3(c) scalar settlement).
    Ambiguous {
        #[serde(with = "rust_decimal::serde::str")]
        settlement_price: Decimal,
    },
    /// Resolution timed out after maximum polling duration.
    Timeout,
}

/// Source of the resolution determination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionSource {
    /// Deribit public/get_delivery_prices TWAP settlement.
    DeribitDelivery,
    /// Kalshi GET /markets/{ticker} settlement status.
    KalshiSettlement,
    /// Polymarket Gamma API closed + price lock.
    GammaApi,
    /// Inferred from price data (not authoritative).
    PriceInference,
}

/// Internal resolution check result. NOT serialized -- used only within
/// the resolution checking pipeline.
#[derive(Debug, Clone)]
pub enum ResolutionResult {
    /// Event has not yet resolved; keep polling.
    NotYetResolved,
    /// Event resolved with a definitive outcome.
    Resolved {
        outcome: OutcomeKind,
        settlement_price: Option<Decimal>,
        resolved_at: DateTime<Utc>,
    },
    /// Event is under dispute (e.g., UMA DVM vote).
    Disputed {
        dispute_started: DateTime<Utc>,
    },
    /// Resolution data is ambiguous (e.g., Kalshi 6.3(c)).
    Ambiguous {
        raw_data: String,
    },
}

/// A fully resolved settlement outcome for a single venue.
///
/// This is the primary type communicated from the SettlementMonitor
/// to the PaperTradeTracker via mpsc channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementOutcome {
    /// Canonical event ID.
    pub event_id: String,
    /// Which venue this outcome is from.
    pub venue: Venue,
    /// The resolution outcome.
    pub outcome: OutcomeKind,
    /// Settlement price (e.g., Deribit TWAP, Kalshi scalar value).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::settlement::types::option_decimal_str"
    )]
    pub settlement_price: Option<Decimal>,
    /// When the venue confirmed the resolution.
    pub resolved_at: DateTime<Utc>,
    /// When the SettlementMonitor detected the resolution.
    pub detected_at: DateTime<Utc>,
    /// How the resolution was determined.
    pub resolution_source: ResolutionSource,
    /// Raw API response for debugging during paper trading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_response: Option<String>,
}

/// A single settled leg of a paper trade position.
///
/// Each venue leg is settled independently as its SettlementOutcome arrives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettledLeg {
    /// Which venue this leg is on.
    pub venue: Venue,
    /// The resolution outcome for this leg.
    pub outcome: OutcomeKind,
    /// Raw P&L before fees: (settlement_price - entry_price) * notional * direction.
    #[serde(with = "rust_decimal::serde::str")]
    pub raw_pnl: Decimal,
    /// Entry fee from SpreadEngine's fee model.
    #[serde(with = "rust_decimal::serde::str")]
    pub entry_fee: Decimal,
    /// Exit/settlement fee.
    #[serde(with = "rust_decimal::serde::str")]
    pub exit_fee: Decimal,
    /// Estimated slippage from entry adverse selection.
    #[serde(with = "rust_decimal::serde::str")]
    pub slippage_estimate: Decimal,
    /// Net P&L after fees and slippage.
    #[serde(with = "rust_decimal::serde::str")]
    pub net_pnl: Decimal,
    /// Fee model version for retroactive P&L recalculation.
    pub fee_model_version: String,
    /// When the venue confirmed the resolution.
    pub resolved_at: DateTime<Utc>,
    /// When the SettlementMonitor detected the resolution.
    pub detected_at: DateTime<Utc>,
    /// How the resolution was determined.
    pub resolution_source: ResolutionSource,
}

/// Type of cross-venue divergence detected during settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DivergenceType {
    /// Venues disagree on binary outcome (one says Yes, other says No).
    BinaryDisagree,
    /// Settlement prices differ beyond acceptable threshold.
    PriceMismatch,
    /// Large timing gap between venue resolutions.
    TimingGap,
    /// At least one venue has ambiguous resolution.
    AmbiguousResolution,
}

/// Cross-venue divergence annotation for a settled position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementDivergence {
    /// What kind of divergence was detected.
    pub divergence_type: DivergenceType,
    /// Basis risk score at position entry time.
    #[serde(with = "rust_decimal::serde::str")]
    pub basis_risk_score_at_entry: Decimal,
    /// Actual P&L impact in basis points.
    #[serde(with = "rust_decimal::serde::str")]
    pub actual_impact_bps: Decimal,
}

/// Complete settlement record logged to JSONL.
///
/// This is the single source of truth for all historical settlement analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementRecord {
    /// Canonical event ID.
    pub event_id: String,
    /// Paper trade position ID.
    pub position_id: String,
    /// Per-venue settled legs.
    pub settled_legs: Vec<SettledLeg>,
    /// Sum of raw P&L across all legs.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_raw_pnl: Decimal,
    /// Sum of net P&L across all legs.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_net_pnl: Decimal,
    /// Sum of all fees across all legs.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_fees: Decimal,
    /// Sum of all slippage estimates across all legs.
    #[serde(with = "rust_decimal::serde::str")]
    pub total_slippage: Decimal,
    /// Net-to-gross ratio (total_net_pnl / total_raw_pnl).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::settlement::types::option_decimal_str"
    )]
    pub net_to_gross_ratio: Option<Decimal>,
    /// Cross-venue divergence annotation, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub divergence: Option<SettlementDivergence>,
    /// When the position was fully settled.
    pub settled_at: DateTime<Utc>,
}

/// Polling tier state machine for settlement monitoring.
///
/// Controls how aggressively we poll a venue for resolution status.
/// Transitions: Waiting -> Aggressive -> Patient -> Lazy -> TimedOut | Resolved
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "tier")]
pub enum PollingTier {
    /// Not yet triggered for polling (before expiry).
    Waiting,
    /// 0-4 hours post-trigger: aggressive polling (every 2 minutes).
    Aggressive {
        started_at: DateTime<Utc>,
    },
    /// 4-96 hours: patient polling (every 15 minutes, UMA DVM dispute window).
    Patient {
        started_at: DateTime<Utc>,
    },
    /// 96h-7d: lazy polling (every 2 hours, WARN level logging).
    Lazy {
        started_at: DateTime<Utc>,
    },
    /// Past timeout: stop polling, emit resolution_timeout.
    TimedOut,
    /// Resolved -- no more polling needed.
    Resolved,
}

impl PollingTier {
    /// Returns the polling interval for the current tier, or None for
    /// terminal/inactive tiers (Waiting, TimedOut, Resolved).
    pub fn interval(&self, config: &SettlementConfig) -> Option<Duration> {
        match self {
            PollingTier::Waiting => None,
            PollingTier::Aggressive { .. } => {
                Some(Duration::from_secs(config.aggressive_interval_secs))
            }
            PollingTier::Patient { .. } => {
                Some(Duration::from_secs(config.patient_interval_secs))
            }
            PollingTier::Lazy { .. } => {
                Some(Duration::from_secs(config.lazy_interval_secs))
            }
            PollingTier::TimedOut => None,
            PollingTier::Resolved => None,
        }
    }

    /// Advance to the next polling tier based on elapsed time since `started_at`.
    ///
    /// Transitions:
    /// - Aggressive -> Patient at `aggressive_duration_hours`
    /// - Patient -> Lazy at `patient_duration_hours`
    /// - Lazy -> TimedOut at `timeout_hours`
    /// - All other tiers remain unchanged.
    pub fn advance(&self, config: &SettlementConfig) -> PollingTier {
        let now = Utc::now();
        match self {
            PollingTier::Aggressive { started_at } => {
                let elapsed = now.signed_duration_since(*started_at);
                if elapsed.num_hours() >= config.aggressive_duration_hours as i64 {
                    PollingTier::Patient { started_at: now }
                } else {
                    self.clone()
                }
            }
            PollingTier::Patient { started_at } => {
                let elapsed = now.signed_duration_since(*started_at);
                if elapsed.num_hours() >= config.patient_duration_hours as i64 {
                    PollingTier::Lazy { started_at: now }
                } else {
                    self.clone()
                }
            }
            PollingTier::Lazy { started_at } => {
                let elapsed = now.signed_duration_since(*started_at);
                if elapsed.num_hours() >= config.timeout_hours as i64 {
                    PollingTier::TimedOut
                } else {
                    self.clone()
                }
            }
            // Terminal and inactive tiers don't advance.
            _ => self.clone(),
        }
    }
}

/// An event being tracked for settlement resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedEvent {
    /// Canonical event ID.
    pub event_id: String,
    /// Which venue this tracking entry is for.
    pub venue: Venue,
    /// Venue-specific instrument identifier.
    pub venue_instrument: String,
    /// Current polling tier.
    pub polling_tier: PollingTier,
    /// When this event was last checked for resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<DateTime<Utc>>,
    /// When polling was triggered (e.g., Deribit expiry time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_time: Option<DateTime<Utc>>,
    /// Expiry date string (e.g., "2025-06-27").
    pub expiry: String,
    /// Underlying asset (e.g., "BTC").
    pub asset: String,
    /// Strike price.
    #[serde(with = "rust_decimal::serde::str")]
    pub strike: Decimal,
    /// Direction (above/below).
    pub direction: Direction,
}

/// Custom serde module for Option<Decimal> using string representation.
pub mod option_decimal_str {
    use rust_decimal::Decimal;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Decimal>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_str(&d.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let d = s.parse::<Decimal>().map_err(serde::de::Error::custom)?;
                Ok(Some(d))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn serde_roundtrip_settlement_outcome() {
        let outcome = SettlementOutcome {
            event_id: "BTC-100K-2025-06-30".to_string(),
            venue: Venue::Deribit,
            outcome: OutcomeKind::Yes,
            settlement_price: Some(dec!(102345.67)),
            resolved_at: Utc::now(),
            detected_at: Utc::now(),
            resolution_source: ResolutionSource::DeribitDelivery,
            raw_response: Some(r#"{"delivery_price": 102345.67}"#.to_string()),
        };

        let json = serde_json::to_string(&outcome).expect("serialize");
        let deserialized: SettlementOutcome =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.event_id, "BTC-100K-2025-06-30");
        assert_eq!(deserialized.venue, Venue::Deribit);
        assert_eq!(deserialized.outcome, OutcomeKind::Yes);
        assert_eq!(deserialized.settlement_price, Some(dec!(102345.67)));
    }

    #[test]
    fn serde_roundtrip_settlement_outcome_no_price() {
        let outcome = SettlementOutcome {
            event_id: "BTC-100K-2025-06-30".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::No,
            settlement_price: None,
            resolved_at: Utc::now(),
            detected_at: Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };

        let json = serde_json::to_string(&outcome).expect("serialize");
        let deserialized: SettlementOutcome =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.settlement_price, None);
        assert_eq!(deserialized.raw_response, None);
    }

    #[test]
    fn serde_roundtrip_settlement_outcome_ambiguous() {
        let outcome = SettlementOutcome {
            event_id: "BTC-100K-2025-06-30".to_string(),
            venue: Venue::Kalshi,
            outcome: OutcomeKind::Ambiguous {
                settlement_price: dec!(0.42),
            },
            settlement_price: Some(dec!(0.42)),
            resolved_at: Utc::now(),
            detected_at: Utc::now(),
            resolution_source: ResolutionSource::KalshiSettlement,
            raw_response: None,
        };

        let json = serde_json::to_string(&outcome).expect("serialize");
        let deserialized: SettlementOutcome =
            serde_json::from_str(&json).expect("deserialize");

        match &deserialized.outcome {
            OutcomeKind::Ambiguous { settlement_price } => {
                assert_eq!(*settlement_price, dec!(0.42));
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn serde_roundtrip_settlement_record() {
        let record = SettlementRecord {
            event_id: "BTC-100K-2025-06-30".to_string(),
            position_id: "pos-001".to_string(),
            settled_legs: vec![
                SettledLeg {
                    venue: Venue::Deribit,
                    outcome: OutcomeKind::Yes,
                    raw_pnl: dec!(150.00),
                    entry_fee: dec!(2.50),
                    exit_fee: dec!(0.00),
                    slippage_estimate: dec!(1.25),
                    net_pnl: dec!(146.25),
                    fee_model_version: "v1.0".to_string(),
                    resolved_at: Utc::now(),
                    detected_at: Utc::now(),
                    resolution_source: ResolutionSource::DeribitDelivery,
                },
                SettledLeg {
                    venue: Venue::Polymarket,
                    outcome: OutcomeKind::Yes,
                    raw_pnl: dec!(-50.00),
                    entry_fee: dec!(1.00),
                    exit_fee: dec!(0.50),
                    slippage_estimate: dec!(0.75),
                    net_pnl: dec!(-52.25),
                    fee_model_version: "v1.0".to_string(),
                    resolved_at: Utc::now(),
                    detected_at: Utc::now(),
                    resolution_source: ResolutionSource::GammaApi,
                },
            ],
            total_raw_pnl: dec!(100.00),
            total_net_pnl: dec!(94.00),
            total_fees: dec!(4.00),
            total_slippage: dec!(2.00),
            net_to_gross_ratio: Some(dec!(0.94)),
            divergence: None,
            settled_at: Utc::now(),
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let deserialized: SettlementRecord =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.event_id, "BTC-100K-2025-06-30");
        assert_eq!(deserialized.settled_legs.len(), 2);
        assert_eq!(deserialized.total_raw_pnl, dec!(100.00));
        assert_eq!(deserialized.total_net_pnl, dec!(94.00));
        assert_eq!(deserialized.net_to_gross_ratio, Some(dec!(0.94)));
    }

    #[test]
    fn serde_roundtrip_settlement_record_with_divergence() {
        let record = SettlementRecord {
            event_id: "BTC-100K-2025-06-30".to_string(),
            position_id: "pos-002".to_string(),
            settled_legs: vec![],
            total_raw_pnl: dec!(0),
            total_net_pnl: dec!(0),
            total_fees: dec!(0),
            total_slippage: dec!(0),
            net_to_gross_ratio: None,
            divergence: Some(SettlementDivergence {
                divergence_type: DivergenceType::BinaryDisagree,
                basis_risk_score_at_entry: dec!(0.35),
                actual_impact_bps: dec!(7400),
            }),
            settled_at: Utc::now(),
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let deserialized: SettlementRecord =
            serde_json::from_str(&json).expect("deserialize");

        let div = deserialized.divergence.expect("should have divergence");
        assert_eq!(div.divergence_type, DivergenceType::BinaryDisagree);
        assert_eq!(div.basis_risk_score_at_entry, dec!(0.35));
        assert_eq!(div.actual_impact_bps, dec!(7400));
    }

    #[test]
    fn serde_roundtrip_polling_tier_all_variants() {
        let now = Utc::now();
        let tiers = vec![
            PollingTier::Waiting,
            PollingTier::Aggressive { started_at: now },
            PollingTier::Patient { started_at: now },
            PollingTier::Lazy { started_at: now },
            PollingTier::TimedOut,
            PollingTier::Resolved,
        ];

        for tier in &tiers {
            let json = serde_json::to_string(tier).expect("serialize");
            let deserialized: PollingTier =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&deserialized, tier, "roundtrip failed for {:?}", tier);
        }
    }

    #[test]
    fn polling_tier_advance_aggressive_to_patient() {
        let config = SettlementConfig::default();
        // Started 5 hours ago (past the 4-hour aggressive threshold)
        let started = Utc::now() - chrono::Duration::hours(5);
        let tier = PollingTier::Aggressive { started_at: started };

        let advanced = tier.advance(&config);
        match advanced {
            PollingTier::Patient { .. } => {} // expected
            other => panic!("expected Patient, got {:?}", other),
        }
    }

    #[test]
    fn polling_tier_stays_aggressive_if_within_window() {
        let config = SettlementConfig::default();
        // Started 1 hour ago (within 4-hour window)
        let started = Utc::now() - chrono::Duration::hours(1);
        let tier = PollingTier::Aggressive { started_at: started };

        let advanced = tier.advance(&config);
        match advanced {
            PollingTier::Aggressive { .. } => {} // expected
            other => panic!("expected Aggressive, got {:?}", other),
        }
    }

    #[test]
    fn polling_tier_advance_patient_to_lazy() {
        let config = SettlementConfig::default();
        // Started 100 hours ago (past the 96-hour patient threshold)
        let started = Utc::now() - chrono::Duration::hours(100);
        let tier = PollingTier::Patient { started_at: started };

        let advanced = tier.advance(&config);
        match advanced {
            PollingTier::Lazy { .. } => {} // expected
            other => panic!("expected Lazy, got {:?}", other),
        }
    }

    #[test]
    fn polling_tier_advance_lazy_to_timed_out() {
        let config = SettlementConfig::default();
        // Started 170 hours ago (past the 168-hour timeout threshold)
        let started = Utc::now() - chrono::Duration::hours(170);
        let tier = PollingTier::Lazy { started_at: started };

        let advanced = tier.advance(&config);
        assert_eq!(advanced, PollingTier::TimedOut);
    }

    #[test]
    fn polling_tier_terminal_tiers_dont_advance() {
        let config = SettlementConfig::default();

        let waiting = PollingTier::Waiting;
        assert_eq!(waiting.advance(&config), PollingTier::Waiting);

        let timed_out = PollingTier::TimedOut;
        assert_eq!(timed_out.advance(&config), PollingTier::TimedOut);

        let resolved = PollingTier::Resolved;
        assert_eq!(resolved.advance(&config), PollingTier::Resolved);
    }

    #[test]
    fn polling_tier_interval_returns_correct_durations() {
        let config = SettlementConfig::default();
        let now = Utc::now();

        assert_eq!(PollingTier::Waiting.interval(&config), None);
        assert_eq!(
            PollingTier::Aggressive { started_at: now }.interval(&config),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            PollingTier::Patient { started_at: now }.interval(&config),
            Some(Duration::from_secs(900))
        );
        assert_eq!(
            PollingTier::Lazy { started_at: now }.interval(&config),
            Some(Duration::from_secs(7200))
        );
        assert_eq!(PollingTier::TimedOut.interval(&config), None);
        assert_eq!(PollingTier::Resolved.interval(&config), None);
    }

    #[test]
    fn serde_roundtrip_tracked_event() {
        let event = TrackedEvent {
            event_id: "BTC-100K-2025-06-30".to_string(),
            venue: Venue::Deribit,
            venue_instrument: "BTC-27JUN25-100000-C".to_string(),
            polling_tier: PollingTier::Aggressive {
                started_at: Utc::now(),
            },
            last_checked: Some(Utc::now()),
            trigger_time: Some(Utc::now()),
            expiry: "2025-06-27".to_string(),
            asset: "BTC".to_string(),
            strike: dec!(100000),
            direction: Direction::Above,
        };

        let json = serde_json::to_string(&event).expect("serialize");
        let deserialized: TrackedEvent =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.event_id, "BTC-100K-2025-06-30");
        assert_eq!(deserialized.venue, Venue::Deribit);
        assert_eq!(deserialized.strike, dec!(100000));
        assert_eq!(deserialized.direction, Direction::Above);
    }

    #[test]
    fn outcome_kind_tagged_serialization() {
        // Verify tagged serde format
        let yes = OutcomeKind::Yes;
        let json = serde_json::to_string(&yes).unwrap();
        assert!(json.contains(r#""kind":"Yes"#));

        let ambiguous = OutcomeKind::Ambiguous {
            settlement_price: dec!(0.42),
        };
        let json = serde_json::to_string(&ambiguous).unwrap();
        assert!(json.contains(r#""kind":"Ambiguous"#));
        assert!(json.contains(r#""settlement_price":"0.42"#));
    }
}
