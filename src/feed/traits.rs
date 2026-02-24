use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

use crate::types::{DualTimestamp, Venue};

/// A raw WebSocket text frame with receive timestamp.
///
/// This is the fundamental unit of data from any venue's WebSocket connection.
/// Both live (`DeribitClient`) and replay data sources produce these.
#[derive(Debug, Clone)]
pub struct RawMessage {
    /// The exact text content of the WebSocket frame.
    pub text: String,
    /// When this frame was received (or replayed).
    pub received_at: DualTimestamp,
}

/// Raw WebSocket-level data source.
///
/// Produces text frames identical to the venue's WebSocket format.
/// Both live connections and replay data sources implement this trait.
/// The `start()` method spawns a background task that pushes raw messages
/// through the returned channel.
pub trait RawDataSource: Send + 'static {
    /// Start the data source and return a receiver for raw messages.
    ///
    /// The implementation spawns a background task that reads from the
    /// underlying source (WebSocket, file, generator) and sends messages
    /// through the channel. The task should respect cancellation.
    fn start(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<mpsc::Receiver<RawMessage>>> + Send;
}

/// A single line to be recorded to JSONL.
///
/// Contains both the raw WebSocket frame and parsed metadata for efficient
/// filtering without re-parsing.
///
/// ## JSONL Schema (v1.0)
///
/// | Field | JSON Type | Description |
/// |-------|-----------|-------------|
/// | `raw` | string | Exact WebSocket text frame |
/// | `local_ts` | string (ISO 8601) | Local wall-clock time when message was received |
/// | `venue` | string | Venue name: "deribit", "polymarket", or "kalshi" |
/// | `channel` | string | Channel name (e.g., "book.BTC-27JUN25-100000-C.none.20.100ms") |
/// | `instrument` | string\|null | Instrument name extracted from channel, if applicable |
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordLine {
    /// The exact WebSocket text frame.
    pub raw: String,
    /// Local wall-clock time when the message was received.
    pub local_ts: DateTime<Utc>,
    /// Which venue this message came from.
    pub venue: Venue,
    /// The channel name (e.g., "book.BTC-27JUN25-100000-C.none.20.100ms").
    pub channel: String,
    /// The instrument name extracted from the channel, if applicable.
    pub instrument: Option<String>,
}

/// Recording abstraction for writing raw messages to persistent storage.
///
/// Takes venue + raw text + local timestamp + channel + optional instrument.
/// Generic so that Polymarket and Kalshi can reuse the same recording
/// infrastructure in Phase 4.
pub trait Recorder: Send + 'static {
    /// Record a single line. Non-blocking -- implementations should use
    /// `try_send` to drop messages on buffer overflow rather than blocking.
    fn record(&self, line: RecordLine);
}
