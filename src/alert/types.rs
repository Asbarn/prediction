//! Alert condition types for failure detection.
//!
//! Defines the vocabulary of alert conditions that the AlertMonitor evaluates:
//! feed silence, partial coverage, signal gaps, and pipeline stage liveness.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Severity level for an alert condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

impl fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Warning => write!(f, "WARNING"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// A detected alert condition with structured context for tracing and metrics.
#[derive(Debug, Clone, Serialize)]
pub enum AlertCondition {
    /// A venue is connected but has sent no messages for `silence_secs`.
    FeedSilence {
        venue: String,
        silence_secs: u64,
        threshold_secs: u64,
    },
    /// Fewer venues are active than expected.
    PartialCoverage {
        active_venues: usize,
        expected_venues: usize,
    },
    /// No signals have been evaluated for `gap_secs`.
    SignalGap {
        gap_secs: u64,
        threshold_secs: u64,
    },
    /// A pipeline stage has not updated for `gap_secs`.
    StageLiveness {
        stage: String,
        gap_secs: u64,
        threshold_secs: u64,
    },
}

impl fmt::Display for AlertCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FeedSilence {
                venue,
                silence_secs,
                threshold_secs,
            } => write!(
                f,
                "Feed silence: venue {venue} has been silent for {silence_secs}s (threshold: {threshold_secs}s)"
            ),
            Self::PartialCoverage {
                active_venues,
                expected_venues,
            } => write!(
                f,
                "Partial coverage: {active_venues}/{expected_venues} venues active"
            ),
            Self::SignalGap {
                gap_secs,
                threshold_secs,
            } => write!(
                f,
                "Signal gap: no signals evaluated for {gap_secs}s (threshold: {threshold_secs}s)"
            ),
            Self::StageLiveness {
                stage,
                gap_secs,
                threshold_secs,
            } => write!(
                f,
                "Stage liveness: {stage} has not updated for {gap_secs}s (threshold: {threshold_secs}s)"
            ),
        }
    }
}

impl AlertCondition {
    /// Determine the severity of this alert condition.
    ///
    /// - `FeedSilence` and `StageLiveness` are always `Warning`.
    /// - `PartialCoverage` is `Warning` if active >= expected/2, `Critical` otherwise.
    /// - `SignalGap` is `Warning` if gap <= 2x threshold, `Critical` if beyond.
    pub fn severity(&self) -> AlertSeverity {
        match self {
            Self::FeedSilence { .. } => AlertSeverity::Warning,
            Self::StageLiveness { .. } => AlertSeverity::Warning,
            Self::PartialCoverage {
                active_venues,
                expected_venues,
            } => {
                // Critical if fewer than half the expected venues are active
                if *expected_venues > 0 && *active_venues * 2 < *expected_venues {
                    AlertSeverity::Critical
                } else {
                    AlertSeverity::Warning
                }
            }
            Self::SignalGap {
                gap_secs,
                threshold_secs,
            } => {
                // Critical if gap exceeds 2x the threshold
                if *threshold_secs > 0 && *gap_secs > threshold_secs * 2 {
                    AlertSeverity::Critical
                } else {
                    AlertSeverity::Warning
                }
            }
        }
    }

    /// Unique key for alert deduplication and cooldown tracking.
    pub fn dedup_key(&self) -> String {
        match self {
            Self::FeedSilence { venue, .. } => format!("feed_silence:{venue}"),
            Self::PartialCoverage { .. } => "partial_coverage".to_string(),
            Self::SignalGap { .. } => "signal_gap".to_string(),
            Self::StageLiveness { stage, .. } => format!("stage_liveness:{stage}"),
        }
    }

    /// Label pairs for Prometheus metrics emission.
    pub fn prometheus_labels(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::FeedSilence {
                venue,
                silence_secs,
                ..
            } => vec![
                ("alert_type", "feed_silence".to_string()),
                ("venue", venue.clone()),
                ("silence_secs", silence_secs.to_string()),
            ],
            Self::PartialCoverage {
                active_venues,
                expected_venues,
            } => vec![
                ("alert_type", "partial_coverage".to_string()),
                ("active_venues", active_venues.to_string()),
                ("expected_venues", expected_venues.to_string()),
            ],
            Self::SignalGap {
                gap_secs,
                threshold_secs,
            } => vec![
                ("alert_type", "signal_gap".to_string()),
                ("gap_secs", gap_secs.to_string()),
                ("threshold_secs", threshold_secs.to_string()),
            ],
            Self::StageLiveness {
                stage, gap_secs, ..
            } => vec![
                ("alert_type", "stage_liveness".to_string()),
                ("stage", stage.clone()),
                ("gap_secs", gap_secs.to_string()),
            ],
        }
    }
}

/// A currently-active alert with tracking metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveAlert {
    /// The detected condition.
    pub condition: AlertCondition,
    /// When this alert was first detected.
    pub first_seen: DateTime<Utc>,
    /// Most recent evaluation that confirmed this condition.
    pub last_seen: DateTime<Utc>,
    /// Number of consecutive evaluations where the condition was true.
    pub count: u64,
}
