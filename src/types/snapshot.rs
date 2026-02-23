use serde::Serialize;

use super::{
    DualTimestamp, EventId, InstrumentId, Notional, Price, Probability, TraceId, Venue,
};

/// Normalized market data snapshot from any venue.
///
/// This is the canonical representation that flows through the pipeline.
/// Each snapshot merges the latest book depth, ticker data (greeks, mark/index
/// prices), and staleness state for a single instrument.
#[derive(Debug, Clone, Serialize)]
pub struct MarketSnapshot {
    pub venue: Venue,
    pub instrument_id: InstrumentId,
    /// Mapped in Phase 5 (cross-venue event mapping).
    pub event_id: Option<EventId>,

    // -- Best bid/ask (top of book) --
    pub bid: Option<Price>,
    pub ask: Option<Price>,
    pub bid_size: Option<Notional>,
    pub ask_size: Option<Notional>,

    // -- Depth: top 20 levels --
    pub depth_bids: Vec<(Price, Notional)>,
    pub depth_asks: Vec<(Price, Notional)>,

    // -- Probability (for prediction markets; derived from price for options in Phase 7) --
    pub bid_probability: Option<Probability>,
    pub ask_probability: Option<Probability>,

    // -- Ticker data (from ticker channel) --
    pub last_price: Option<Price>,
    pub mark_price: Option<Price>,
    pub index_price: Option<Price>,
    /// Implied volatility from exchange.
    pub mark_iv: Option<f64>,
    pub open_interest: Option<Notional>,
    pub volume_24h: Option<Notional>,

    // -- Greeks (from ticker channel, options only) --
    pub greeks: Option<SnapshotGreeks>,

    // -- Options pricing data (Phase 7) --
    /// Exchange-computed bid implied volatility (from Deribit ticker).
    pub bid_iv: Option<f64>,
    /// Exchange-computed ask implied volatility (from Deribit ticker).
    pub ask_iv: Option<f64>,
    /// Forward/futures price used by Deribit pricer (USD).
    pub underlying_price: Option<f64>,
    /// Futures contract name (e.g., "BTC-27JUN25") identifying the forward.
    pub underlying_index: Option<String>,

    // -- Timestamps --
    /// Exchange-reported milliseconds since epoch (FEED-08).
    pub exchange_timestamp: Option<i64>,
    pub timestamp: DualTimestamp,
    pub sequence: u64,
    pub trace_id: TraceId,

    // -- Staleness flag (set on change_id gap) --
    pub is_stale: bool,
}

/// Greeks snapshot from ticker data (options only).
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}
