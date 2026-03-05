//! Per-venue health tracker for graceful degradation visibility.
//!
//! Each venue supervisor calls `mark_available()` / `mark_unavailable()` to
//! track connection state. Health state is observable via `metrics` gauges
//! (zero-cost no-ops until a Prometheus recorder is installed in Phase 6).
//!
//! In Phase 9, the `/health` HTTP endpoint reads `VenueHealth` to report
//! per-venue status.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::types::Venue;

/// Thread-safe per-venue health tracker.
///
/// Created via `VenueHealth::new(venue)` which returns an `Arc<Self>`.
/// All methods take `&self` and use interior mutability (atomics + mutex).
pub struct VenueHealth {
    venue: Venue,
    is_available: AtomicBool,
    last_error: Mutex<Option<String>>,
    pub(crate) last_message_at: Mutex<Option<DateTime<Utc>>>,
    connection_count: AtomicU64,
}

impl VenueHealth {
    /// Create a new health tracker for the given venue.
    ///
    /// Starts in unavailable state (no connection yet).
    pub fn new(venue: Venue) -> Arc<Self> {
        Arc::new(Self {
            venue,
            is_available: AtomicBool::new(false),
            last_error: Mutex::new(None),
            last_message_at: Mutex::new(None),
            connection_count: AtomicU64::new(0),
        })
    }

    /// Mark this venue as available (connected and streaming).
    ///
    /// Updates `last_message_at` to now and emits a metrics gauge.
    pub fn mark_available(&self) {
        self.is_available.store(true, Ordering::Release);
        *self.last_message_at.lock().unwrap() = Some(Utc::now());
        metrics::gauge!("feed_available", "venue" => self.venue.to_string()).set(1.0);
        tracing::info!(venue = %self.venue, "venue marked available");
    }

    /// Mark this venue as unavailable (disconnected or errored).
    ///
    /// Stores the error message and emits a metrics gauge.
    pub fn mark_unavailable(&self, error: String) {
        self.is_available.store(false, Ordering::Release);
        *self.last_error.lock().unwrap() = Some(error.clone());
        metrics::gauge!("feed_available", "venue" => self.venue.to_string()).set(0.0);
        tracing::warn!(venue = %self.venue, error = %error, "venue marked unavailable");
    }

    /// Check if this venue is currently available.
    pub fn is_available(&self) -> bool {
        self.is_available.load(Ordering::Acquire)
    }

    /// Record that a message was received from this venue.
    ///
    /// Updates `last_message_at` to now.
    pub fn record_message(&self) {
        *self.last_message_at.lock().unwrap() = Some(Utc::now());
    }

    /// Increment the connection attempt counter and emit reconnection metric.
    pub fn increment_connections(&self) {
        self.connection_count.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("feed_reconnections_total", "venue" => self.venue.to_string()).increment(1);
    }

    /// Get the venue this health tracker is for.
    pub fn venue(&self) -> Venue {
        self.venue
    }

    /// Get the last error message, if any.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    /// Get the timestamp of the last received message.
    pub fn last_message_at(&self) -> Option<DateTime<Utc>> {
        *self.last_message_at.lock().unwrap()
    }

    /// Get the total number of connection attempts.
    pub fn connection_count(&self) -> u64 {
        self.connection_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_unavailable() {
        let health = VenueHealth::new(Venue::Deribit);
        assert!(!health.is_available());
        assert!(health.last_error().is_none());
        assert!(health.last_message_at().is_none());
        assert_eq!(health.connection_count(), 0);
    }

    #[test]
    fn mark_available_sets_state() {
        let health = VenueHealth::new(Venue::Polymarket);
        health.mark_available();
        assert!(health.is_available());
        assert!(health.last_message_at().is_some());
    }

    #[test]
    fn mark_unavailable_sets_state_and_error() {
        let health = VenueHealth::new(Venue::Kalshi);
        health.mark_available();
        assert!(health.is_available());

        health.mark_unavailable("connection reset".to_string());
        assert!(!health.is_available());
        assert_eq!(health.last_error().unwrap(), "connection reset");
    }

    #[test]
    fn record_message_updates_timestamp() {
        let health = VenueHealth::new(Venue::Deribit);
        assert!(health.last_message_at().is_none());

        health.record_message();
        let ts1 = health.last_message_at().unwrap();

        // Second call should update to a later (or equal) time
        health.record_message();
        let ts2 = health.last_message_at().unwrap();
        assert!(ts2 >= ts1);
    }

    #[test]
    fn increment_connections_counts() {
        let health = VenueHealth::new(Venue::Deribit);
        assert_eq!(health.connection_count(), 0);

        health.increment_connections();
        assert_eq!(health.connection_count(), 1);

        health.increment_connections();
        health.increment_connections();
        assert_eq!(health.connection_count(), 3);
    }

    #[test]
    fn venue_accessor_returns_correct_venue() {
        let health = VenueHealth::new(Venue::Kalshi);
        assert_eq!(health.venue(), Venue::Kalshi);
    }

    #[test]
    fn available_unavailable_cycle() {
        let health = VenueHealth::new(Venue::Polymarket);

        // Start unavailable
        assert!(!health.is_available());

        // Connect
        health.mark_available();
        assert!(health.is_available());

        // Disconnect
        health.mark_unavailable("timeout".to_string());
        assert!(!health.is_available());

        // Reconnect
        health.mark_available();
        assert!(health.is_available());
        // Error should still be present from last failure
        assert_eq!(health.last_error().unwrap(), "timeout");
    }
}
