//! Pipeline liveness timestamp infrastructure.
//!
//! `PipelineLiveness` tracks the last time each pipeline stage ran,
//! enabling the AlertMonitor to detect stale pipeline stages.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use chrono::Utc;

/// Shared atomic timestamp tracker for pipeline stage liveness.
///
/// Each pipeline stage calls the corresponding `record_*` method after
/// completing work. The AlertMonitor reads `last_*_age_secs()` to detect
/// stages that have gone silent.
///
/// All timestamps are stored as epoch milliseconds in `AtomicI64`.
/// A value of 0 means "never recorded".
pub struct PipelineLiveness {
    last_spread_computed_at: AtomicI64,
    last_signal_evaluated_at: AtomicI64,
    last_settlement_checked_at: AtomicI64,
}

impl PipelineLiveness {
    /// Create a new liveness tracker wrapped in `Arc` for shared ownership.
    ///
    /// All timestamps start at 0 (never recorded).
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_spread_computed_at: AtomicI64::new(0),
            last_signal_evaluated_at: AtomicI64::new(0),
            last_settlement_checked_at: AtomicI64::new(0),
        })
    }

    /// Record that a spread computation just completed.
    pub fn record_spread(&self) {
        self.last_spread_computed_at
            .store(Utc::now().timestamp_millis(), Ordering::Release);
    }

    /// Record that a signal evaluation just completed.
    pub fn record_signal_eval(&self) {
        self.last_signal_evaluated_at
            .store(Utc::now().timestamp_millis(), Ordering::Release);
    }

    /// Record that a settlement check just completed.
    pub fn record_settlement_check(&self) {
        self.last_settlement_checked_at
            .store(Utc::now().timestamp_millis(), Ordering::Release);
    }

    /// Seconds since last spread computation, or `None` if never recorded.
    pub fn last_spread_age_secs(&self) -> Option<u64> {
        self.age_secs(&self.last_spread_computed_at)
    }

    /// Seconds since last signal evaluation, or `None` if never recorded.
    pub fn last_signal_eval_age_secs(&self) -> Option<u64> {
        self.age_secs(&self.last_signal_evaluated_at)
    }

    /// Seconds since last settlement check, or `None` if never recorded.
    pub fn last_settlement_check_age_secs(&self) -> Option<u64> {
        self.age_secs(&self.last_settlement_checked_at)
    }

    /// Compute age in seconds from an atomic timestamp. Returns `None` if 0 (never).
    fn age_secs(&self, atomic: &AtomicI64) -> Option<u64> {
        let stored = atomic.load(Ordering::Acquire);
        if stored == 0 {
            return None;
        }
        let now = Utc::now().timestamp_millis();
        let diff_ms = now.saturating_sub(stored);
        Some((diff_ms / 1000) as u64)
    }
}

impl fmt::Debug for PipelineLiveness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineLiveness")
            .field("spread_age_secs", &self.last_spread_age_secs())
            .field("signal_eval_age_secs", &self.last_signal_eval_age_secs())
            .field(
                "settlement_check_age_secs",
                &self.last_settlement_check_age_secs(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_all_ages_as_none() {
        let liveness = PipelineLiveness::new();
        assert!(liveness.last_spread_age_secs().is_none());
        assert!(liveness.last_signal_eval_age_secs().is_none());
        assert!(liveness.last_settlement_check_age_secs().is_none());
    }

    #[test]
    fn record_spread_then_age_is_small() {
        let liveness = PipelineLiveness::new();
        liveness.record_spread();
        let age = liveness.last_spread_age_secs().expect("should be Some after recording");
        // Just recorded, so age should be 0 or 1 second at most
        assert!(age <= 1, "spread age should be <= 1s, got {age}");
    }

    #[test]
    fn record_signal_eval_then_age_is_small() {
        let liveness = PipelineLiveness::new();
        liveness.record_signal_eval();
        let age = liveness
            .last_signal_eval_age_secs()
            .expect("should be Some after recording");
        assert!(age <= 1, "signal eval age should be <= 1s, got {age}");
    }

    #[test]
    fn record_settlement_check_then_age_is_small() {
        let liveness = PipelineLiveness::new();
        liveness.record_settlement_check();
        let age = liveness
            .last_settlement_check_age_secs()
            .expect("should be Some after recording");
        assert!(age <= 1, "settlement check age should be <= 1s, got {age}");
    }

    #[test]
    fn recording_one_stage_does_not_affect_others() {
        let liveness = PipelineLiveness::new();

        // Record only spread
        liveness.record_spread();

        // Spread should be Some, others should remain None
        assert!(liveness.last_spread_age_secs().is_some());
        assert!(liveness.last_signal_eval_age_secs().is_none());
        assert!(liveness.last_settlement_check_age_secs().is_none());

        // Record signal eval
        liveness.record_signal_eval();

        // Now spread and signal should be Some, settlement still None
        assert!(liveness.last_spread_age_secs().is_some());
        assert!(liveness.last_signal_eval_age_secs().is_some());
        assert!(liveness.last_settlement_check_age_secs().is_none());
    }

    #[test]
    fn debug_format_shows_ages() {
        let liveness = PipelineLiveness::new();
        let debug = format!("{:?}", liveness);
        assert!(debug.contains("PipelineLiveness"));
        assert!(debug.contains("spread_age_secs"));
        assert!(debug.contains("None"));
    }
}
