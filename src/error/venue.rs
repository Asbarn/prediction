use serde::Serialize;

use crate::types::Venue;

/// Machine-readable error severity classification.
///
/// Designed for automated alerting -- a monitoring system can match on severity
/// without parsing free-text error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorSeverity {
    /// Stop the feed, alert operator immediately.
    /// Examples: auth failure, account locked, API key revoked.
    Fatal,
    /// Backoff, continue with reduced capability.
    /// Examples: rate limited, partial data, slow responses.
    Degraded,
    /// Retry silently, log at debug level.
    /// Examples: timeout, single message parse error, temporary disconnect.
    Transient,
}

/// Venue-specific errors with severity classification.
///
/// Each variant carries the venue it originated from and a severity level
/// accessible via the `severity()` method. Error messages include a severity
/// prefix in brackets for log grep-ability.
#[derive(Debug, thiserror::Error)]
pub enum VenueError {
    #[error("[FATAL] authentication failed for {venue}: {message}")]
    AuthFailure {
        venue: Venue,
        message: String,
    },

    #[error("[DEGRADED] rate limited on {venue}, backing off {backoff_ms}ms")]
    RateLimited {
        venue: Venue,
        backoff_ms: u64,
    },

    #[error("[TRANSIENT] timeout connecting to {venue}")]
    ConnectionTimeout {
        venue: Venue,
    },

    #[error("[TRANSIENT] failed to parse message from {venue}: {message}")]
    ParseError {
        venue: Venue,
        message: String,
    },

    #[error("[TRANSIENT] connection closed for {venue}: {reason}")]
    ConnectionClosed {
        venue: Venue,
        reason: String,
    },
}

impl VenueError {
    /// Returns the machine-readable severity classification for this error.
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::AuthFailure { .. } => ErrorSeverity::Fatal,
            Self::RateLimited { .. } => ErrorSeverity::Degraded,
            Self::ConnectionTimeout { .. } => ErrorSeverity::Transient,
            Self::ParseError { .. } => ErrorSeverity::Transient,
            Self::ConnectionClosed { .. } => ErrorSeverity::Transient,
        }
    }
}
