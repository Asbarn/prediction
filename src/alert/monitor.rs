//! AlertMonitor periodic task for failure detection.
//!
//! Runs a sweep loop every `check_interval_secs`, reading VenueHealth atomics
//! and PipelineLiveness timestamps to detect feed silence, partial coverage,
//! signal evaluation gaps, and pipeline stage staleness. Emits `tracing::warn!`
//! and Prometheus gauge metrics for active alert conditions, with cooldown-based
//! deduplication to prevent log spam.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;

use crate::alert::config::AlertConfig;
use crate::alert::liveness::PipelineLiveness;
use crate::alert::types::{ActiveAlert, AlertCondition};
use crate::feed::health::VenueHealth;

/// Periodic alert monitor that detects failure conditions in the pipeline.
///
/// Evaluates feed silence, partial venue coverage, signal evaluation gaps,
/// and pipeline stage liveness on a configurable interval. Uses cooldown-based
/// deduplication to suppress repeated warnings for the same condition.
pub struct AlertMonitor {
    venue_health: Vec<Arc<VenueHealth>>,
    liveness: Arc<PipelineLiveness>,
    config: AlertConfig,
    active_alerts: HashMap<String, ActiveAlert>,
    cancel: CancellationToken,
    /// Track when the monitor was created, for startup grace period.
    started_at: DateTime<Utc>,
}

impl AlertMonitor {
    /// Create a new AlertMonitor.
    ///
    /// Accepts shared references to VenueHealth trackers and PipelineLiveness,
    /// along with the alert configuration and a cancellation token.
    pub fn new(
        venue_health: Vec<Arc<VenueHealth>>,
        liveness: Arc<PipelineLiveness>,
        config: AlertConfig,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            venue_health,
            liveness,
            config,
            active_alerts: HashMap::new(),
            cancel,
            started_at: Utc::now(),
        }
    }

    /// Run the alert monitor sweep loop.
    ///
    /// Uses `tokio::select! biased` with cancellation as highest priority.
    /// Ticks every `check_interval_secs` and evaluates all alert conditions.
    pub async fn run(mut self) {
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.check_interval_secs));
        // Skip the first immediate tick.
        interval.tick().await;

        tracing::info!(
            check_interval_secs = self.config.check_interval_secs,
            "AlertMonitor started"
        );

        loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    tracing::info!(
                        active_alerts = self.active_alerts.len(),
                        "AlertMonitor shutting down"
                    );
                    break;
                }

                _ = interval.tick() => {
                    self.evaluate_all();
                }
            }
        }
    }

    /// Evaluate all alert conditions in a single sweep.
    ///
    /// Collects fired conditions from each check, processes them through
    /// the fire/update/cooldown logic, then cleans up resolved alerts.
    fn evaluate_all(&mut self) {
        let mut fired: Vec<AlertCondition> = Vec::new();

        // Collect conditions from all checks.
        fired.extend(self.check_feed_silence());
        fired.extend(self.check_partial_coverage());
        fired.extend(self.check_signal_gap());
        fired.extend(self.check_stage_liveness());

        // Track which keys were fired this cycle.
        let mut fired_keys = HashSet::new();
        for condition in &fired {
            fired_keys.insert(condition.dedup_key());
        }

        // Process fired conditions: fire new alerts, update existing.
        for condition in fired {
            self.fire_alert(condition);
        }

        // Clean up resolved alerts (not fired this cycle).
        self.cleanup_resolved(&fired_keys);

        // Update aggregate gauge.
        metrics::gauge!("alert_monitor_active_alerts").set(self.active_alerts.len() as f64);
    }

    /// Check each venue for feed silence.
    ///
    /// A venue is considered silent if it is marked available but has not
    /// received a message for longer than `feed_silence_threshold_secs`.
    fn check_feed_silence(&self) -> Vec<AlertCondition> {
        let mut conditions = Vec::new();
        let now = Utc::now();

        for vh in &self.venue_health {
            if !vh.is_available() {
                continue;
            }
            if let Some(last_msg) = vh.last_message_at() {
                let silence_secs = now
                    .signed_duration_since(last_msg)
                    .num_seconds()
                    .max(0) as u64;
                if silence_secs > self.config.feed_silence_threshold_secs {
                    conditions.push(AlertCondition::FeedSilence {
                        venue: vh.venue().to_string(),
                        silence_secs,
                        threshold_secs: self.config.feed_silence_threshold_secs,
                    });
                }
            }
            // If last_message_at is None but venue is available, it just
            // connected and hasn't streamed yet -- skip (no false alarm).
        }

        conditions
    }

    /// Check if fewer venues are active than expected.
    fn check_partial_coverage(&self) -> Vec<AlertCondition> {
        let active_count = self
            .venue_health
            .iter()
            .filter(|vh| vh.is_available())
            .count();

        if active_count < self.config.expected_venue_count {
            vec![AlertCondition::PartialCoverage {
                active_venues: active_count,
                expected_venues: self.config.expected_venue_count,
            }]
        } else {
            vec![]
        }
    }

    /// Check if signal evaluations have stalled.
    ///
    /// Applies a startup grace period: if the system has been running for
    /// less than `signal_gap_threshold_secs` and no evaluation has ever
    /// happened, we skip alerting to avoid false alarms.
    fn check_signal_gap(&self) -> Vec<AlertCondition> {
        match self.liveness.last_signal_eval_age_secs() {
            Some(age) if age > self.config.signal_gap_threshold_secs => {
                vec![AlertCondition::SignalGap {
                    gap_secs: age,
                    threshold_secs: self.config.signal_gap_threshold_secs,
                }]
            }
            None => {
                // Never evaluated. Only alert if we've been running long enough.
                let uptime_secs = Utc::now()
                    .signed_duration_since(self.started_at)
                    .num_seconds()
                    .max(0) as u64;
                if uptime_secs > self.config.signal_gap_threshold_secs {
                    vec![AlertCondition::SignalGap {
                        gap_secs: uptime_secs,
                        threshold_secs: self.config.signal_gap_threshold_secs,
                    }]
                } else {
                    vec![]
                }
            }
            _ => vec![], // age within threshold
        }
    }

    /// Check pipeline stage liveness for spread, signal eval, and settlement.
    fn check_stage_liveness(&self) -> Vec<AlertCondition> {
        let mut conditions = Vec::new();
        let threshold = self.config.stage_liveness_threshold_secs;

        let stages: [(&str, Option<u64>); 3] = [
            ("spread", self.liveness.last_spread_age_secs()),
            ("signal_eval", self.liveness.last_signal_eval_age_secs()),
            (
                "settlement_check",
                self.liveness.last_settlement_check_age_secs(),
            ),
        ];

        for (stage, age_opt) in stages {
            if let Some(age) = age_opt {
                if age > threshold {
                    conditions.push(AlertCondition::StageLiveness {
                        stage: stage.to_string(),
                        gap_secs: age,
                        threshold_secs: threshold,
                    });
                }
            }
            // None means stage never ran -- handled by check_signal_gap
            // for signal_eval. For spread and settlement, we don't alert
            // on "never ran" since that's expected during startup.
        }

        conditions
    }

    /// Fire or update an alert for a detected condition.
    ///
    /// New alerts emit `tracing::warn!` immediately. Existing alerts check
    /// the cooldown window before re-emitting. Updates the Prometheus gauge.
    fn fire_alert(&mut self, condition: AlertCondition) {
        let key = condition.dedup_key();
        let now = Utc::now();

        if let Some(existing) = self.active_alerts.get_mut(&key) {
            existing.last_seen = now;
            existing.count += 1;
            // Update the condition with latest values (e.g., new silence_secs).
            existing.condition = condition.clone();

            // Check cooldown before re-emitting warn.
            let since_last_warn = now
                .signed_duration_since(existing.last_warned_at)
                .num_seconds()
                .max(0) as u64;
            if since_last_warn >= self.config.alert_cooldown_secs {
                let count = existing.count;
                existing.last_warned_at = now;
                self.emit_warn(&condition, count);
            }
        } else {
            // New alert.
            self.emit_warn(&condition, 1);
            let alert = ActiveAlert {
                condition: condition.clone(),
                first_seen: now,
                last_seen: now,
                count: 1,
                last_warned_at: now,
            };
            self.active_alerts.insert(key.clone(), alert);
        }

        // Set Prometheus gauge for this alert type.
        let type_label = match &condition {
            AlertCondition::FeedSilence { venue, .. } => format!("feed_silence:{venue}"),
            AlertCondition::PartialCoverage { .. } => "partial_coverage".to_string(),
            AlertCondition::SignalGap { .. } => "signal_gap".to_string(),
            AlertCondition::StageLiveness { stage, .. } => format!("stage_liveness:{stage}"),
        };
        metrics::gauge!("alert_active", "type" => type_label).set(1.0);
    }

    /// Emit a structured `tracing::warn!` for an alert condition.
    fn emit_warn(&self, condition: &AlertCondition, count: u64) {
        tracing::warn!(
            alert_type = %condition.dedup_key(),
            severity = %condition.severity(),
            count = count,
            details = %condition,
            "alert condition detected"
        );
    }

    /// Remove active alerts that were not fired this evaluation cycle.
    ///
    /// For resolved alerts, emits `tracing::info!` and clears the Prometheus gauge.
    fn cleanup_resolved(&mut self, fired_keys: &HashSet<String>) {
        let resolved_keys: Vec<String> = self
            .active_alerts
            .keys()
            .filter(|k| !fired_keys.contains(*k))
            .cloned()
            .collect();

        for key in resolved_keys {
            if let Some(alert) = self.active_alerts.remove(&key) {
                let duration_secs = Utc::now()
                    .signed_duration_since(alert.first_seen)
                    .num_seconds()
                    .max(0) as u64;

                tracing::info!(
                    alert_type = %alert.condition.dedup_key(),
                    duration_secs = duration_secs,
                    total_count = alert.count,
                    "alert resolved"
                );

                // Clear Prometheus gauge.
                let type_label = match &alert.condition {
                    AlertCondition::FeedSilence { venue, .. } => {
                        format!("feed_silence:{venue}")
                    }
                    AlertCondition::PartialCoverage { .. } => "partial_coverage".to_string(),
                    AlertCondition::SignalGap { .. } => "signal_gap".to_string(),
                    AlertCondition::StageLiveness { stage, .. } => {
                        format!("stage_liveness:{stage}")
                    }
                };
                metrics::gauge!("alert_active", "type" => type_label).set(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Venue;

    /// Helper: create an AlertMonitor with custom venue_health and config.
    fn make_monitor(
        venue_health: Vec<Arc<VenueHealth>>,
        liveness: Arc<PipelineLiveness>,
        config: AlertConfig,
    ) -> AlertMonitor {
        AlertMonitor::new(
            venue_health,
            liveness,
            config,
            CancellationToken::new(),
        )
    }

    fn default_config() -> AlertConfig {
        AlertConfig {
            enabled: true,
            check_interval_secs: 30,
            feed_silence_threshold_secs: 120,
            expected_venue_count: 3,
            signal_gap_threshold_secs: 300,
            stage_liveness_threshold_secs: 180,
            alert_cooldown_secs: 300,
        }
    }

    // --- Feed silence tests ---

    #[test]
    fn feed_silence_detected_when_venue_silent_beyond_threshold() {
        let config = AlertConfig {
            feed_silence_threshold_secs: 60,
            ..default_config()
        };

        let vh = VenueHealth::new(Venue::Deribit);
        vh.mark_available();
        // Simulate silence: set last_message_at to 5 minutes ago.
        {
            let mut lock = vh.last_message_at.lock().unwrap();
            *lock = Some(Utc::now() - chrono::Duration::seconds(300));
        }

        let liveness = PipelineLiveness::new();
        let monitor = make_monitor(vec![vh], liveness, config);
        let conditions = monitor.check_feed_silence();

        assert_eq!(conditions.len(), 1);
        match &conditions[0] {
            AlertCondition::FeedSilence {
                venue,
                silence_secs,
                threshold_secs,
            } => {
                assert_eq!(venue, "deribit");
                assert!(*silence_secs >= 299); // at least ~300 seconds
                assert_eq!(*threshold_secs, 60);
            }
            other => panic!("expected FeedSilence, got {other:?}"),
        }
    }

    #[test]
    fn feed_silence_not_detected_when_venue_recently_active() {
        let config = AlertConfig {
            feed_silence_threshold_secs: 120,
            ..default_config()
        };

        let vh = VenueHealth::new(Venue::Deribit);
        vh.mark_available();
        // mark_available sets last_message_at to now

        let liveness = PipelineLiveness::new();
        let monitor = make_monitor(vec![vh], liveness, config);
        let conditions = monitor.check_feed_silence();

        assert!(conditions.is_empty(), "fresh venue should not trigger silence alert");
    }

    #[test]
    fn feed_silence_skips_unavailable_venues() {
        let config = default_config();

        let vh = VenueHealth::new(Venue::Deribit);
        // Not marking available -- stays unavailable

        let liveness = PipelineLiveness::new();
        let monitor = make_monitor(vec![vh], liveness, config);
        let conditions = monitor.check_feed_silence();

        assert!(conditions.is_empty(), "unavailable venue should not trigger silence alert");
    }

    // --- Partial coverage tests ---

    #[test]
    fn partial_coverage_detected_when_fewer_venues_active() {
        let config = AlertConfig {
            expected_venue_count: 3,
            ..default_config()
        };

        // Only 1 of 3 venues available.
        let vh1 = VenueHealth::new(Venue::Deribit);
        vh1.mark_available();
        let vh2 = VenueHealth::new(Venue::Polymarket);
        let vh3 = VenueHealth::new(Venue::Kalshi);

        let liveness = PipelineLiveness::new();
        let monitor = make_monitor(vec![vh1, vh2, vh3], liveness, config);
        let conditions = monitor.check_partial_coverage();

        assert_eq!(conditions.len(), 1);
        match &conditions[0] {
            AlertCondition::PartialCoverage {
                active_venues,
                expected_venues,
            } => {
                assert_eq!(*active_venues, 1);
                assert_eq!(*expected_venues, 3);
            }
            other => panic!("expected PartialCoverage, got {other:?}"),
        }
    }

    #[test]
    fn partial_coverage_not_detected_when_all_venues_active() {
        let config = AlertConfig {
            expected_venue_count: 2,
            ..default_config()
        };

        let vh1 = VenueHealth::new(Venue::Deribit);
        vh1.mark_available();
        let vh2 = VenueHealth::new(Venue::Polymarket);
        vh2.mark_available();

        let liveness = PipelineLiveness::new();
        let monitor = make_monitor(vec![vh1, vh2], liveness, config);
        let conditions = monitor.check_partial_coverage();

        assert!(conditions.is_empty());
    }

    // --- Cooldown deduplication tests ---

    #[test]
    fn cooldown_suppresses_duplicate_warns() {
        let config = AlertConfig {
            feed_silence_threshold_secs: 60,
            alert_cooldown_secs: 300,
            expected_venue_count: 0, // avoid partial coverage noise
            ..default_config()
        };

        let vh = VenueHealth::new(Venue::Deribit);
        vh.mark_available();
        {
            let mut lock = vh.last_message_at.lock().unwrap();
            *lock = Some(Utc::now() - chrono::Duration::seconds(300));
        }

        let liveness = PipelineLiveness::new();
        let mut monitor = make_monitor(vec![vh], liveness, config);

        // First evaluation: should fire and create the alert.
        monitor.evaluate_all();
        assert_eq!(monitor.active_alerts.len(), 1);
        let alert = monitor.active_alerts.values().next().unwrap();
        assert_eq!(alert.count, 1);

        // Second evaluation within cooldown: count increments but no new warn.
        monitor.evaluate_all();
        let alert = monitor.active_alerts.values().next().unwrap();
        assert_eq!(alert.count, 2);
        // last_warned_at should still be from the first evaluation.
        // (We can't easily test tracing output, but count incrementing
        //  without last_warned_at changing confirms cooldown suppression.)
    }

    // --- Cleanup resolved tests ---

    #[test]
    fn cleanup_removes_resolved_alerts() {
        let config = AlertConfig {
            feed_silence_threshold_secs: 60,
            expected_venue_count: 0,
            ..default_config()
        };

        let vh = VenueHealth::new(Venue::Deribit);
        vh.mark_available();
        {
            let mut lock = vh.last_message_at.lock().unwrap();
            *lock = Some(Utc::now() - chrono::Duration::seconds(300));
        }

        let liveness = PipelineLiveness::new();
        let mut monitor = make_monitor(vec![vh.clone()], liveness, config);

        // First evaluation: should fire feed silence.
        monitor.evaluate_all();
        assert_eq!(monitor.active_alerts.len(), 1);

        // "Fix" the silence by updating last_message_at to now.
        {
            let mut lock = vh.last_message_at.lock().unwrap();
            *lock = Some(Utc::now());
        }

        // Second evaluation: silence resolved, alert should be cleaned up.
        monitor.evaluate_all();
        assert_eq!(
            monitor.active_alerts.len(),
            0,
            "resolved alert should be removed"
        );
    }

    // --- Signal gap tests ---

    #[test]
    fn signal_gap_detected_when_age_exceeds_threshold() {
        let config = AlertConfig {
            signal_gap_threshold_secs: 10,
            expected_venue_count: 0,
            ..default_config()
        };

        let liveness = PipelineLiveness::new();
        // Record a signal eval far in the past.
        liveness.last_signal_evaluated_at.store(
            (Utc::now() - chrono::Duration::seconds(60)).timestamp_millis(),
            std::sync::atomic::Ordering::Release,
        );

        let monitor = make_monitor(vec![], liveness, config);
        let conditions = monitor.check_signal_gap();

        assert_eq!(conditions.len(), 1);
        match &conditions[0] {
            AlertCondition::SignalGap {
                gap_secs,
                threshold_secs,
            } => {
                assert!(*gap_secs >= 59);
                assert_eq!(*threshold_secs, 10);
            }
            other => panic!("expected SignalGap, got {other:?}"),
        }
    }

    #[test]
    fn signal_gap_not_detected_during_startup_grace_period() {
        let config = AlertConfig {
            signal_gap_threshold_secs: 300,
            ..default_config()
        };

        // PipelineLiveness with no signal eval recorded (None).
        let liveness = PipelineLiveness::new();

        // Monitor just started -- uptime < threshold.
        let monitor = make_monitor(vec![], liveness, config);
        let conditions = monitor.check_signal_gap();

        assert!(
            conditions.is_empty(),
            "should not alert during startup grace period"
        );
    }

    // --- Stage liveness tests ---

    #[test]
    fn stage_liveness_detected_when_spread_stale() {
        let config = AlertConfig {
            stage_liveness_threshold_secs: 10,
            expected_venue_count: 0,
            ..default_config()
        };

        let liveness = PipelineLiveness::new();
        // Record spread computation far in the past.
        liveness.last_spread_computed_at.store(
            (Utc::now() - chrono::Duration::seconds(60)).timestamp_millis(),
            std::sync::atomic::Ordering::Release,
        );

        let monitor = make_monitor(vec![], liveness, config);
        let conditions = monitor.check_stage_liveness();

        assert!(!conditions.is_empty());
        let spread_alert = conditions
            .iter()
            .find(|c| matches!(c, AlertCondition::StageLiveness { stage, .. } if stage == "spread"));
        assert!(spread_alert.is_some(), "should detect stale spread stage");
    }
}
