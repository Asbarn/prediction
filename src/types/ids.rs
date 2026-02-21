use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Canonical event identifier (e.g. "BTC-100K-2025-06-30").
/// Maps to venue-specific `InstrumentId` per venue.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[display("{_0}")]
pub struct EventId(String);

impl EventId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Venue-specific instrument identifier (e.g. "BTC-27JUN25-100000-C" on Deribit).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[display("{_0}")]
pub struct InstrumentId(String);

impl InstrumentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Unique trace ID assigned to each market data event.
/// Follows the event through normalization -> spread calc -> signal.
/// Uses UUID v7 for timestamp-sorted ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(Uuid);

impl TraceId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
