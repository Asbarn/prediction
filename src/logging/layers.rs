use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Layer, Registry};

/// Initialize the dual-output logging system with per-layer filtering.
///
/// Creates two independent output layers:
/// - **Stdout**: Filtered to `stdout_level` (e.g., "info"). Emits structured
///   JSON when `stdout_json` is true (for CloudWatch ingestion via awslogs
///   driver), or human-readable format when false (local development).
/// - **File**: Structured JSON format, daily-rotating, filtered to `file_level`
///   (e.g., "debug"). Captures everything for post-hoc analysis.
///
/// Each layer has its own `EnvFilter`, composed via `Registry` (NOT a global
/// filter). This is the correct per-layer filtering pattern from
/// tracing-subscriber 0.3.x.
///
/// # Returns
///
/// Returns the `WorkerGuard` for the non-blocking file writer. The caller
/// (main.rs) **MUST** hold this guard alive for the entire program lifetime.
/// If dropped, buffered log entries are lost. This is the #1 pitfall with
/// tracing-appender.
///
/// # Errors
///
/// Returns an error if the log directory cannot be created or if the filter
/// strings are invalid.
pub fn init_logging(
    log_dir: &str,
    stdout_level: &str,
    file_level: &str,
    stdout_json: bool,
) -> anyhow::Result<WorkerGuard> {
    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(log_dir)?;

    // Build filter strings scoped to our crate
    let stdout_filter_str = format!("prediction={stdout_level}");
    let file_filter_str = format!("prediction={file_level}");

    // Stdout layer: conditional JSON or human-readable, per-layer filter.
    // Because `.json()` changes the concrete layer type, we must box both
    // branches to unify them as `Box<dyn Layer<_> + Send + Sync>`.
    let stdout_filter = EnvFilter::try_new(&stdout_filter_str).map_err(|e| {
        anyhow::anyhow!(
            "invalid stdout filter '{}': {}",
            stdout_filter_str,
            e
        )
    })?;
    let stdout_layer: Box<dyn Layer<_> + Send + Sync> = if stdout_json {
        Box::new(
            fmt::layer()
                .json()
                .with_target(true)
                .with_level(true)
                .with_filter(stdout_filter),
        )
    } else {
        Box::new(
            fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_filter(stdout_filter),
        )
    };

    // File layer: structured JSON, daily rotation, per-layer filter
    let file_appender = tracing_appender::rolling::daily(log_dir, "prediction.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_filter = EnvFilter::try_new(&file_filter_str).map_err(|e| {
        anyhow::anyhow!(
            "invalid file filter '{}': {}",
            file_filter_str,
            e
        )
    })?;
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_current_span(true)
        .with_span_list(true)
        .with_filter(file_filter);

    // Compose layers with Registry (per-layer filtering, NOT global)
    Registry::default()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}
