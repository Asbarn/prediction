//! Per-venue rate limiter using the governor crate.
//!
//! Wraps governor::RateLimiter with a venue-specific quota.
//! Applied to all outbound WebSocket messages EXCEPT heartbeat responses
//! (per research pitfall 6: heartbeat responses must be prompt).

use governor::{
    Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use std::num::NonZeroU32;
use std::sync::Arc;

/// Type alias for the governor rate limiter used per venue.
pub type GovernorLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// Per-venue rate limiter.
///
/// Thread-safe (Arc-wrapped) rate limiter that enforces a maximum
/// requests-per-second quota on outbound API calls.
#[derive(Clone)]
pub struct VenueRateLimiter {
    limiter: Arc<GovernorLimiter>,
    venue: String,
}

impl VenueRateLimiter {
    /// Create a rate limiter for the given venue with the specified requests/second.
    pub fn new(venue: &str, requests_per_second: u32) -> Self {
        let quota = Quota::per_second(
            NonZeroU32::new(requests_per_second).expect("requests_per_second must be > 0"),
        );
        Self {
            limiter: Arc::new(RateLimiter::direct(quota)),
            venue: venue.to_string(),
        }
    }

    /// Wait until a request is allowed, then return.
    /// Call this before any outbound WebSocket/API message.
    pub async fn wait(&self) {
        self.limiter.until_ready().await;
    }

    /// Get the venue name.
    pub fn venue(&self) -> &str {
        &self.venue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_first_call_returns_immediately() {
        let limiter = VenueRateLimiter::new("deribit", 20);

        // First call should return immediately (burst capacity)
        let start = std::time::Instant::now();
        limiter.wait().await;
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "first wait() should be near-instant, took {}ms",
            elapsed.as_millis()
        );
        assert_eq!(limiter.venue(), "deribit");
    }
}
