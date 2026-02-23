//! Prometheus metrics exporter setup.
//!
//! Installs the `metrics-exporter-prometheus` recorder as the global metrics
//! sink. This activates all existing `metrics::counter!`, `metrics::gauge!`,
//! and `metrics::histogram!` calls throughout the feed layer with zero code
//! changes. Spread-specific histogram buckets are pre-configured for
//! probability-space values.

use std::net::SocketAddr;

use metrics_exporter_prometheus::{Matcher, PrometheusBuilder};

/// Install the Prometheus metrics recorder and start the HTTP scrape endpoint.
///
/// Must be called BEFORE spawning any feed or spread tasks, so all metric
/// emissions are captured (not lost to the no-op recorder).
///
/// Configures custom histogram buckets:
/// - `spread_*` metrics: probability-space buckets (0.0001 to 0.20)
/// - `feed_latency_ms`: millisecond buckets (1ms to 10s)
///
/// # Arguments
///
/// * `port` - TCP port for the Prometheus scrape endpoint (default: 9000)
///
/// # Errors
///
/// Returns an error if the recorder cannot be installed (e.g., port already
/// in use, or a recorder is already set).
pub fn setup_prometheus(port: u16) -> anyhow::Result<()> {
    let listen_addr = SocketAddr::from(([0, 0, 0, 0], port));

    PrometheusBuilder::new()
        .with_http_listener(listen_addr)
        // Spread histogram buckets: probability-space values (0.01% to 20%)
        .set_buckets_for_metric(
            Matcher::Prefix("spread_".to_string()),
            &[0.0001, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.10, 0.20],
        )?
        // Feed latency buckets: milliseconds (1ms to 10s)
        .set_buckets_for_metric(
            Matcher::Full("feed_latency_ms".to_string()),
            &[1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0, 10000.0],
        )?
        .install()?;

    tracing::info!(port = port, "Prometheus metrics exporter started");

    Ok(())
}
