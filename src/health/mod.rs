//! HTTP health endpoint reporting per-feed connection status and system uptime.
//!
//! Serves a JSON response at `GET /health` on a configurable port (default 9001),
//! separate from the Prometheus metrics exporter (port 9000). Reads lightweight
//! atomic/mutex state from `VenueHealth` trackers -- never blocks on pipeline channels.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::events::registry::EventRegistry;
use crate::feed::health::VenueHealth;

/// Shared state for the health endpoint handler.
///
/// Cloneable (all fields are `Arc` or `Copy`), passed to axum via `with_state`.
#[derive(Clone)]
pub struct HealthState {
    /// Per-venue health trackers from the pipeline.
    pub venue_health: Vec<Arc<VenueHealth>>,
    /// Shared event registry for active event count.
    pub event_registry: Arc<RwLock<EventRegistry>>,
    /// System startup time for uptime calculation.
    pub started_at: DateTime<Utc>,
}

/// JSON response body for `GET /health`.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// "ok" if at least one feed is connected, "degraded" if none.
    pub status: String,
    /// Seconds since system startup.
    pub uptime_secs: i64,
    /// Per-feed connection status.
    pub feeds: Vec<FeedStatus>,
    /// Number of registered event mappings.
    pub active_event_count: usize,
}

/// Per-feed status within the health response.
#[derive(Debug, Serialize)]
pub struct FeedStatus {
    /// Venue name (e.g., "deribit", "polymarket", "kalshi").
    pub venue: String,
    /// Whether the venue is currently connected and streaming.
    pub connected: bool,
    /// Timestamp of the last received message from this venue.
    pub last_message_at: Option<DateTime<Utc>>,
    /// Last error message from this venue, if any.
    pub last_error: Option<String>,
    /// Total number of connection attempts for this venue.
    pub connection_count: u64,
}

/// Handler for `GET /health`.
///
/// Reads from VenueHealth atomics/mutexes (lightweight, non-blocking on pipeline
/// channels) and reads event_count from the registry.
async fn health_handler(State(state): State<HealthState>) -> Json<HealthResponse> {
    let uptime = Utc::now()
        .signed_duration_since(state.started_at)
        .num_seconds();

    let feeds: Vec<FeedStatus> = state
        .venue_health
        .iter()
        .map(|vh| FeedStatus {
            venue: vh.venue().to_string(),
            connected: vh.is_available(),
            last_message_at: vh.last_message_at(),
            last_error: vh.last_error(),
            connection_count: vh.connection_count(),
        })
        .collect();

    let active_event_count = state.event_registry.read().await.event_count();

    let status = if feeds.iter().any(|f| f.connected) {
        "ok".to_string()
    } else {
        "degraded".to_string()
    };

    Json(HealthResponse {
        status,
        uptime_secs: uptime,
        feeds,
        active_event_count,
    })
}

/// Start the health HTTP server on the given port.
///
/// Binds to `0.0.0.0:{port}` and serves `GET /health`. This function runs
/// indefinitely until the tokio runtime shuts down.
pub async fn start_health_server(state: HealthState, port: u16) {
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(port = port, error = %e, "failed to bind health server");
            return;
        }
    };

    tracing::info!(port = port, "health endpoint listening on /health");

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!(error = %e, "health server error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EventsConfig;
    use crate::types::Venue;

    fn make_empty_registry() -> Arc<RwLock<EventRegistry>> {
        let config = EventsConfig {
            events: vec![],
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        };
        Arc::new(RwLock::new(EventRegistry::from_config(&config)))
    }

    #[tokio::test]
    async fn health_handler_returns_correct_structure() {
        let deribit = VenueHealth::new(Venue::Deribit);
        deribit.mark_available();
        let polymarket = VenueHealth::new(Venue::Polymarket);
        polymarket.mark_available();

        let state = HealthState {
            venue_health: vec![deribit, polymarket],
            event_registry: make_empty_registry(),
            started_at: Utc::now() - chrono::Duration::seconds(120),
        };

        let Json(response) = health_handler(State(state)).await;

        assert_eq!(response.status, "ok");
        assert!(response.uptime_secs >= 119); // allow 1 second tolerance
        assert_eq!(response.feeds.len(), 2);
        assert!(response.feeds[0].connected);
        assert!(response.feeds[1].connected);
        assert_eq!(response.active_event_count, 0);
    }

    #[tokio::test]
    async fn health_handler_returns_degraded_when_no_feeds_connected() {
        let deribit = VenueHealth::new(Venue::Deribit);
        // Not marking available -- stays in default unavailable state

        let state = HealthState {
            venue_health: vec![deribit],
            event_registry: make_empty_registry(),
            started_at: Utc::now(),
        };

        let Json(response) = health_handler(State(state)).await;

        assert_eq!(response.status, "degraded");
        assert!(!response.feeds[0].connected);
    }
}
