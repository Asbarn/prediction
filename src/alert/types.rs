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
    /// When tracing::warn! was last emitted (for cooldown suppression).
    pub last_warned_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Display tests ---

    #[test]
    fn display_feed_silence_includes_venue_and_duration() {
        let alert = AlertCondition::FeedSilence {
            venue: "deribit".to_string(),
            silence_secs: 150,
            threshold_secs: 120,
        };
        let display = format!("{alert}");
        assert!(display.contains("deribit"), "should contain venue name");
        assert!(display.contains("150"), "should contain silence duration");
    }

    #[test]
    fn display_partial_coverage_includes_counts() {
        let alert = AlertCondition::PartialCoverage {
            active_venues: 1,
            expected_venues: 3,
        };
        let display = format!("{alert}");
        assert!(display.contains("1"), "should contain active count");
        assert!(display.contains("3"), "should contain expected count");
    }

    #[test]
    fn display_signal_gap_includes_gap_and_threshold() {
        let alert = AlertCondition::SignalGap {
            gap_secs: 400,
            threshold_secs: 300,
        };
        let display = format!("{alert}");
        assert!(display.contains("400"), "should contain gap");
        assert!(display.contains("300"), "should contain threshold");
    }

    #[test]
    fn display_stage_liveness_includes_stage_name() {
        let alert = AlertCondition::StageLiveness {
            stage: "spread".to_string(),
            gap_secs: 200,
            threshold_secs: 180,
        };
        let display = format!("{alert}");
        assert!(display.contains("spread"), "should contain stage name");
    }

    // --- Severity tests ---

    #[test]
    fn severity_feed_silence_is_warning() {
        let alert = AlertCondition::FeedSilence {
            venue: "deribit".to_string(),
            silence_secs: 150,
            threshold_secs: 120,
        };
        assert_eq!(alert.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn severity_stage_liveness_is_warning() {
        let alert = AlertCondition::StageLiveness {
            stage: "spread".to_string(),
            gap_secs: 200,
            threshold_secs: 180,
        };
        assert_eq!(alert.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn severity_partial_coverage_warning_within_threshold() {
        // 2 of 3 venues: more than half, so Warning
        let alert = AlertCondition::PartialCoverage {
            active_venues: 2,
            expected_venues: 3,
        };
        assert_eq!(alert.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn severity_partial_coverage_critical_beyond_threshold() {
        // 1 of 3 venues: less than half (1*2 = 2 < 3), so Critical
        let alert = AlertCondition::PartialCoverage {
            active_venues: 1,
            expected_venues: 3,
        };
        assert_eq!(alert.severity(), AlertSeverity::Critical);
    }

    #[test]
    fn severity_partial_coverage_zero_active_is_critical() {
        let alert = AlertCondition::PartialCoverage {
            active_venues: 0,
            expected_venues: 3,
        };
        assert_eq!(alert.severity(), AlertSeverity::Critical);
    }

    #[test]
    fn severity_signal_gap_warning_within_2x() {
        // gap=500, threshold=300: 500 <= 600, so Warning
        let alert = AlertCondition::SignalGap {
            gap_secs: 500,
            threshold_secs: 300,
        };
        assert_eq!(alert.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn severity_signal_gap_critical_beyond_2x() {
        // gap=700, threshold=300: 700 > 600, so Critical
        let alert = AlertCondition::SignalGap {
            gap_secs: 700,
            threshold_secs: 300,
        };
        assert_eq!(alert.severity(), AlertSeverity::Critical);
    }

    // --- Dedup key tests ---

    #[test]
    fn dedup_key_feed_silence() {
        let alert = AlertCondition::FeedSilence {
            venue: "deribit".to_string(),
            silence_secs: 150,
            threshold_secs: 120,
        };
        assert_eq!(alert.dedup_key(), "feed_silence:deribit");
    }

    #[test]
    fn dedup_key_partial_coverage() {
        let alert = AlertCondition::PartialCoverage {
            active_venues: 1,
            expected_venues: 3,
        };
        assert_eq!(alert.dedup_key(), "partial_coverage");
    }

    #[test]
    fn dedup_key_signal_gap() {
        let alert = AlertCondition::SignalGap {
            gap_secs: 400,
            threshold_secs: 300,
        };
        assert_eq!(alert.dedup_key(), "signal_gap");
    }

    #[test]
    fn dedup_key_stage_liveness() {
        let alert = AlertCondition::StageLiveness {
            stage: "spread".to_string(),
            gap_secs: 200,
            threshold_secs: 180,
        };
        assert_eq!(alert.dedup_key(), "stage_liveness:spread");
    }

    // --- Prometheus labels tests ---

    #[test]
    fn prometheus_labels_feed_silence() {
        let alert = AlertCondition::FeedSilence {
            venue: "deribit".to_string(),
            silence_secs: 150,
            threshold_secs: 120,
        };
        let labels = alert.prometheus_labels();
        assert!(labels.contains(&("alert_type", "feed_silence".to_string())));
        assert!(labels.contains(&("venue", "deribit".to_string())));
        assert!(labels.contains(&("silence_secs", "150".to_string())));
    }

    #[test]
    fn prometheus_labels_partial_coverage() {
        let alert = AlertCondition::PartialCoverage {
            active_venues: 1,
            expected_venues: 3,
        };
        let labels = alert.prometheus_labels();
        assert!(labels.contains(&("alert_type", "partial_coverage".to_string())));
        assert!(labels.contains(&("active_venues", "1".to_string())));
        assert!(labels.contains(&("expected_venues", "3".to_string())));
    }

    #[test]
    fn prometheus_labels_signal_gap() {
        let alert = AlertCondition::SignalGap {
            gap_secs: 400,
            threshold_secs: 300,
        };
        let labels = alert.prometheus_labels();
        assert!(labels.contains(&("alert_type", "signal_gap".to_string())));
        assert!(labels.contains(&("gap_secs", "400".to_string())));
        assert!(labels.contains(&("threshold_secs", "300".to_string())));
    }

    #[test]
    fn prometheus_labels_stage_liveness() {
        let alert = AlertCondition::StageLiveness {
            stage: "spread".to_string(),
            gap_secs: 200,
            threshold_secs: 180,
        };
        let labels = alert.prometheus_labels();
        assert!(labels.contains(&("alert_type", "stage_liveness".to_string())));
        assert!(labels.contains(&("stage", "spread".to_string())));
        assert!(labels.contains(&("gap_secs", "200".to_string())));
    }

    // --- ActiveAlert tests ---

    #[test]
    fn active_alert_creation() {
        let now = Utc::now();
        let alert = ActiveAlert {
            condition: AlertCondition::FeedSilence {
                venue: "deribit".to_string(),
                silence_secs: 150,
                threshold_secs: 120,
            },
            first_seen: now,
            last_seen: now,
            count: 1,
            last_warned_at: now,
        };
        assert_eq!(alert.count, 1);
        assert_eq!(alert.first_seen, alert.last_seen);
    }

    #[test]
    fn active_alert_count_incrementing() {
        let now = Utc::now();
        let mut alert = ActiveAlert {
            condition: AlertCondition::SignalGap {
                gap_secs: 400,
                threshold_secs: 300,
            },
            first_seen: now,
            last_seen: now,
            count: 1,
            last_warned_at: now,
        };
        // Simulate subsequent evaluations
        alert.count += 1;
        alert.last_seen = Utc::now();
        assert_eq!(alert.count, 2);
        assert!(alert.last_seen >= alert.first_seen);
    }

    // --- AlertSeverity display ---

    #[test]
    fn severity_display() {
        assert_eq!(format!("{}", AlertSeverity::Warning), "WARNING");
        assert_eq!(format!("{}", AlertSeverity::Critical), "CRITICAL");
    }
}
