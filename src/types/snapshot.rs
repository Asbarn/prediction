use serde::Serialize;

use super::{
    DualTimestamp, EventId, InstrumentId, Notional, Price, TraceId, Venue,
};

/// Normalized market data snapshot from any venue.
///
/// This is the canonical representation that flows through the pipeline.
/// Skeleton for Phase 2 -- fields may expand but the structure is established.
#[derive(Debug, Clone, Serialize)]
pub struct MarketSnapshot {
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    /// Mapped in Phase 5 (cross-venue event mapping).
    pub event_id: Option<EventId>,
    pub bid: Option<Price>,
    pub ask: Option<Price>,
    pub bid_size: Option<Notional>,
    pub ask_size: Option<Notional>,
    pub timestamp: DualTimestamp,
    pub sequence: u64,
    pub trace_id: TraceId,
}
