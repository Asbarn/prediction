//! Deribit message processor and normalization pipeline.
//!
//! Receives raw WebSocket frames via an mpsc channel, parses them as
//! `DeribitMessage`, routes by channel type, updates per-instrument book
//! and ticker state, and publishes `MarketSnapshot` events downstream.
//!
//! ```text
//! [WS Reader] --raw frames--> mpsc(1024) --> [DeribitProcessor]
//!                                                  |
//!                                mpsc(256) <-------+-----> mpsc (RecordLine)
//!                                (snapshots)               (to disk, Plan 03)
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::deribit::book::{InstrumentBook, SequenceError};
use crate::feed::deribit::channels::{self, ChannelKind};
use crate::feed::deribit::messages::{
    BookData, DeribitMessage, PriceIndexData, TickerData, TradeData,
};
use crate::feed::traits::{RawMessage, RecordLine};
use crate::types::{
    DualTimestamp, InstrumentId, MarketSnapshot, Notional, Price, SnapshotGreeks,
    TraceId, Venue,
};

/// Buffer size for the downstream snapshot channel.
const SNAPSHOT_BUFFER: usize = 256;

/// Default staleness threshold in milliseconds (5 seconds).
///
/// Exchange data older than this is marked `is_stale = true` on the
/// resulting `MarketSnapshot` (RELY-03). Production callers pass the
/// configured value from `DeribitConfig.staleness_threshold_ms`.
#[cfg(test)]
const DEFAULT_STALENESS_THRESHOLD_MS: u64 = 5000;

/// Cached ticker state per instrument (latest mark/index prices, greeks, etc.).
#[derive(Debug, Clone, Default)]
pub struct TickerState {
    pub last_price: Option<f64>,
    pub mark_price: Option<f64>,
    pub index_price: Option<f64>,
    pub mark_iv: Option<f64>,
    pub open_interest: Option<f64>,
    pub volume_24h: Option<f64>,
    pub greeks: Option<GreeksState>,
    pub exchange_timestamp: Option<i64>,
}

/// Cached greeks from the most recent ticker update.
#[derive(Debug, Clone)]
pub struct GreeksState {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}

/// Deribit message processor.
///
/// Consumes `RawMessage` from the WS reader, maintains per-instrument book
/// and ticker state, and publishes `MarketSnapshot` events.
pub struct DeribitProcessor {
    raw_rx: mpsc::Receiver<RawMessage>,
    snapshot_tx: mpsc::Sender<MarketSnapshot>,
    record_tx: Option<mpsc::Sender<RecordLine>>,
    cancel: CancellationToken,
    books: HashMap<InstrumentId, InstrumentBook>,
    tickers: HashMap<InstrumentId, TickerState>,
    sequence: AtomicU64,
    /// Staleness threshold in milliseconds. Exchange data older than this is
    /// marked `is_stale = true` on the MarketSnapshot (RELY-03).
    staleness_threshold_ms: u64,
}

impl DeribitProcessor {
    /// Create a new processor.
    ///
    /// Returns `(DeribitProcessor, Receiver<MarketSnapshot>)`. The caller
    /// consumes the receiver; the processor owns the sender.
    pub fn new(
        raw_rx: mpsc::Receiver<RawMessage>,
        record_tx: Option<mpsc::Sender<RecordLine>>,
        cancel: CancellationToken,
        staleness_threshold_ms: u64,
    ) -> (Self, mpsc::Receiver<MarketSnapshot>) {
        let (snapshot_tx, snapshot_rx) = mpsc::channel(SNAPSHOT_BUFFER);
        let processor = Self {
            raw_rx,
            snapshot_tx,
            record_tx,
            cancel,
            books: HashMap::new(),
            tickers: HashMap::new(),
            sequence: AtomicU64::new(1),
            staleness_threshold_ms,
        };
        (processor, snapshot_rx)
    }

    /// Run the processing loop until cancelled or the input channel closes.
    pub async fn run(mut self) {
        tracing::info!("DeribitProcessor starting");

        loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    tracing::info!("DeribitProcessor cancelled");
                    break;
                }

                msg = self.raw_rx.recv() => {
                    match msg {
                        Some(raw) => self.handle_raw_message(raw).await,
                        None => {
                            tracing::info!("DeribitProcessor input channel closed");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("DeribitProcessor exiting");
    }

    /// Process a single raw message.
    async fn handle_raw_message(&mut self, raw: RawMessage) {
        // Fan-out to recording if configured
        if let Some(ref record_tx) = self.record_tx {
            // Best-effort send; drop on overflow
            let _ = record_tx.try_send(RecordLine {
                raw: raw.text.clone(),
                local_ts: raw.received_at.wall(),
                venue: Venue::Deribit,
                channel: String::new(), // filled after parse if needed
                instrument: None,
            });
        }

        // Parse the raw text as a DeribitMessage
        let message: DeribitMessage = match serde_json::from_str(&raw.text) {
            Ok(m) => m,
            Err(e) => {
                let truncated = if raw.text.len() > 200 {
                    format!("{}...", &raw.text[..200])
                } else {
                    raw.text.clone()
                };
                tracing::warn!(
                    error = %e,
                    raw = %truncated,
                    "failed to parse Deribit message"
                );
                return;
            }
        };

        match message {
            DeribitMessage::Response(resp) => {
                if let Some(err) = &resp.error {
                    tracing::error!(
                        id = resp.id,
                        code = err.code,
                        message = %err.message,
                        "Deribit RPC error response"
                    );
                } else {
                    tracing::debug!(
                        id = resp.id,
                        "Deribit RPC response (subscribe confirmation or other)"
                    );
                }
            }
            DeribitMessage::Heartbeat(hb) => {
                // Heartbeat messages are connection-level protocol messages.
                // They are handled by the DeribitClient WS loop (responds to
                // test_request with public/test). If they reach the processor,
                // just log and ignore -- they should NOT produce snapshots.
                tracing::debug!(
                    heartbeat_type = %hb.params.heartbeat_type,
                    "heartbeat message reached processor (should be handled by client)"
                );
            }
            DeribitMessage::Notification(notif) => {
                let channel = &notif.params.channel;
                let kind = ChannelKind::parse(channel);
                let instrument_name = channels::extract_instrument(channel);

                match kind {
                    ChannelKind::Book => {
                        self.handle_book(notif.params.data, &instrument_name, raw.received_at)
                            .await;
                    }
                    ChannelKind::Ticker => {
                        self.handle_ticker(notif.params.data, &instrument_name, raw.received_at)
                            .await;
                    }
                    ChannelKind::Trades => {
                        self.handle_trades(notif.params.data, &instrument_name);
                    }
                    ChannelKind::PriceIndex => {
                        self.handle_price_index(notif.params.data);
                    }
                    ChannelKind::Unknown(ch) => {
                        tracing::debug!(channel = %ch, "ignoring unknown Deribit channel");
                    }
                }
            }
        }
    }

    /// Handle a book channel message.
    async fn handle_book(
        &mut self,
        data: serde_json::Value,
        _instrument_name: &Option<String>,
        received_at: DualTimestamp,
    ) {
        let book_data: BookData = match serde_json::from_value(data) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize BookData");
                return;
            }
        };

        let inst_id = InstrumentId::new(&book_data.instrument_name);
        let exchange_ts = book_data.timestamp;

        // Get the sequence number before any mutable borrows
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);

        // Get or create the instrument book
        let book = self
            .books
            .entry(inst_id.clone())
            .or_insert_with(|| InstrumentBook::new(inst_id.clone()));

        match book.apply_snapshot(&book_data, received_at) {
            Ok(()) => {
                tracing::debug!(
                    instrument = %book_data.instrument_name,
                    change_id = book_data.change_id,
                    bids = book.bids.len(),
                    asks = book.asks.len(),
                    "book snapshot applied"
                );
            }
            Err(SequenceError::Gap { expected, got }) => {
                tracing::error!(
                    instrument = %book_data.instrument_name,
                    expected = expected,
                    got = got,
                    "book change_id sequence gap -- marking stale"
                );
                // In Phase 2 we just log and mark stale. Re-subscribe comes in Phase 3.
            }
        }

        // Build and send snapshot (even if stale, so downstream sees the flag)
        let ticker = self.tickers.get(&inst_id);
        let snapshot = build_snapshot(
            &inst_id,
            book,
            ticker,
            seq,
            received_at,
            Some(exchange_ts),
            self.staleness_threshold_ms,
        );

        if self.snapshot_tx.send(snapshot).await.is_err() {
            tracing::warn!("snapshot receiver dropped");
        }
    }

    /// Handle a ticker channel message.
    async fn handle_ticker(
        &mut self,
        data: serde_json::Value,
        _instrument_name: &Option<String>,
        received_at: DualTimestamp,
    ) {
        let ticker_data: TickerData = match serde_json::from_value(data) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize TickerData");
                return;
            }
        };

        let inst_id = InstrumentId::new(&ticker_data.instrument_name);
        let exchange_ts = ticker_data.timestamp;

        // Update cached ticker state
        {
            let state = self.tickers.entry(inst_id.clone()).or_default();
            state.last_price = ticker_data.last_price;
            state.mark_price = Some(ticker_data.mark_price);
            state.index_price = Some(ticker_data.index_price);
            state.mark_iv = ticker_data.mark_iv;
            state.open_interest = Some(ticker_data.open_interest);
            state.volume_24h = ticker_data.stats.as_ref().and_then(|s| s.volume);
            state.greeks = ticker_data.greeks.as_ref().map(|g| GreeksState {
                delta: g.delta,
                gamma: g.gamma,
                vega: g.vega,
                theta: g.theta,
                rho: g.rho,
            });
            state.exchange_timestamp = Some(exchange_ts);
        }

        tracing::debug!(
            instrument = %ticker_data.instrument_name,
            mark_price = ticker_data.mark_price,
            index_price = ticker_data.index_price,
            "ticker update"
        );

        // Get sequence before building snapshot
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);

        // Build and send snapshot merging book + ticker
        let empty_book;
        let book_ref = match self.books.get(&inst_id) {
            Some(b) => b,
            None => {
                empty_book = InstrumentBook::new(inst_id.clone());
                &empty_book
            }
        };
        let ticker_ref = self.tickers.get(&inst_id);

        let snapshot = build_snapshot(
            &inst_id,
            book_ref,
            ticker_ref,
            seq,
            received_at,
            Some(exchange_ts),
            self.staleness_threshold_ms,
        );

        if self.snapshot_tx.send(snapshot).await.is_err() {
            tracing::warn!("snapshot receiver dropped");
        }
    }

    /// Handle a trades channel message.
    fn handle_trades(&self, data: serde_json::Value, _instrument_name: &Option<String>) {
        let trades: Vec<TradeData> = match serde_json::from_value(data) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize TradeData array");
                return;
            }
        };

        for trade in &trades {
            tracing::debug!(
                instrument = %trade.instrument_name,
                price = trade.price,
                amount = trade.amount,
                direction = %trade.direction,
                trade_id = %trade.trade_id,
                "trade"
            );
        }

        // Trades do NOT produce a MarketSnapshot in Phase 2.
        // They are recorded via the recording fan-out above.
    }

    /// Handle a price index channel message.
    fn handle_price_index(&self, data: serde_json::Value) {
        let index: PriceIndexData = match serde_json::from_value(data) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize PriceIndexData");
                return;
            }
        };

        tracing::debug!(
            index_name = %index.index_name,
            price = index.price,
            "price index update"
        );

        // Price index does NOT produce a MarketSnapshot in Phase 2.
        // It is reference data used in Phase 7 for Black-76 forward price.
    }

}

/// Check if exchange data is stale based on exchange-reported timestamp.
/// Uses exchange_timestamp (from Deribit's clock), NOT received_at.
fn is_exchange_data_stale(exchange_ts_ms: i64, threshold_ms: u64) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let age_ms = (now_ms - exchange_ts_ms).max(0) as u64;
    age_ms > threshold_ms
}

/// Build a `MarketSnapshot` from the current book and ticker state.
pub fn build_snapshot(
    instrument: &InstrumentId,
    book: &InstrumentBook,
    ticker: Option<&TickerState>,
    sequence: u64,
    received_at: DualTimestamp,
    exchange_timestamp: Option<i64>,
    staleness_threshold_ms: u64,
) -> MarketSnapshot {
    let (bid, bid_size) = match book.best_bid() {
        Some((p, s)) => (Some(p), Some(s)),
        None => (None, None),
    };

    let (ask, ask_size) = match book.best_ask() {
        Some((p, s)) => (Some(p), Some(s)),
        None => (None, None),
    };

    let greeks = ticker
        .and_then(|t| t.greeks.as_ref())
        .map(|g| SnapshotGreeks {
            delta: g.delta,
            gamma: g.gamma,
            vega: g.vega,
            theta: g.theta,
            rho: g.rho,
        });

    let last_price = ticker
        .and_then(|t| t.last_price)
        .map(|v| Price::new(Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)));
    let mark_price = ticker
        .and_then(|t| t.mark_price)
        .map(|v| Price::new(Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)));
    let index_price = ticker
        .and_then(|t| t.index_price)
        .map(|v| Price::new(Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)));
    let mark_iv = ticker.and_then(|t| t.mark_iv);
    let open_interest = ticker
        .and_then(|t| t.open_interest)
        .map(|v| Notional::new(Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)));
    let volume_24h = ticker
        .and_then(|t| t.volume_24h)
        .map(|v| Notional::new(Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)));

    let exchange_ts = exchange_timestamp.or_else(|| {
        ticker.and_then(|t| t.exchange_timestamp)
    });

    // Staleness gate: OR book staleness with exchange-timestamp age check (RELY-03).
    // Only check exchange-timestamp staleness if we have an exchange timestamp.
    let exchange_data_stale = exchange_ts
        .map(|ts| is_exchange_data_stale(ts, staleness_threshold_ms))
        .unwrap_or(false);

    if exchange_data_stale {
        let age_ms = exchange_ts
            .map(|ts| (chrono::Utc::now().timestamp_millis() - ts).max(0) as u64)
            .unwrap_or(0);
        tracing::warn!(
            instrument = %instrument,
            age_ms = age_ms,
            threshold_ms = staleness_threshold_ms,
            "exchange data stale -- marking snapshot is_stale=true"
        );
    }

    let is_stale = book.is_stale || exchange_data_stale;

    // Latency metrics: record feed latency on every snapshot with exchange timestamp.
    // The metrics crate macros are zero-cost no-ops when no recorder is installed.
    // No recorder is installed in Phase 3 -- the Prometheus exporter comes in Phase 6.
    if let Some(exchange_ts_ms) = exchange_ts {
        let local_ms = received_at.wall().timestamp_millis();
        let latency_ms = (local_ms - exchange_ts_ms) as f64;
        metrics::histogram!("feed_latency_ms", "venue" => "deribit").record(latency_ms);
        metrics::gauge!("feed_last_latency_ms", "venue" => "deribit").set(latency_ms);
        metrics::counter!("feed_messages_total", "venue" => "deribit").increment(1);
    }

    MarketSnapshot {
        venue: Venue::Deribit,
        instrument_id: instrument.clone(),
        event_id: None,
        bid,
        ask,
        bid_size,
        ask_size,
        depth_bids: book.bids.clone(),
        depth_asks: book.asks.clone(),
        bid_probability: None,
        ask_probability: None,
        last_price,
        mark_price,
        index_price,
        mark_iv,
        open_interest,
        volume_24h,
        greeks,
        exchange_timestamp: exchange_ts,
        timestamp: received_at,
        sequence,
        trace_id: TraceId::new(),
        is_stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn ts() -> DualTimestamp {
        DualTimestamp::now()
    }

    /// Return a "fresh" exchange timestamp (now minus a small offset).
    fn fresh_exchange_ts() -> i64 {
        chrono::Utc::now().timestamp_millis() - 100 // 100ms ago
    }

    #[test]
    fn build_snapshot_from_book_only() {
        let now_ts = fresh_exchange_ts();
        let mut book = InstrumentBook::new(InstrumentId::new("BTC-27JUN25-100000-C"));
        let data = crate::feed::deribit::messages::BookData {
            timestamp: now_ts,
            instrument_name: "BTC-27JUN25-100000-C".to_string(),
            change_id: 100,
            prev_change_id: None,
            update_type: Some("snapshot".to_string()),
            bids: vec![[0.0055, 10.0], [0.0050, 25.0]],
            asks: vec![[0.0060, 8.0], [0.0065, 12.0]],
        };
        book.apply_snapshot(&data, ts()).unwrap();

        let snap = build_snapshot(
            &InstrumentId::new("BTC-27JUN25-100000-C"),
            &book,
            None,
            1,
            ts(),
            Some(now_ts),
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert_eq!(snap.venue, Venue::Deribit);
        assert_eq!(snap.instrument_id, InstrumentId::new("BTC-27JUN25-100000-C"));
        assert!(snap.bid.is_some());
        assert!(snap.ask.is_some());
        assert_eq!(snap.depth_bids.len(), 2);
        assert_eq!(snap.depth_asks.len(), 2);
        assert!(snap.greeks.is_none());
        assert!(snap.mark_price.is_none());
        assert!(!snap.is_stale);
        assert_eq!(snap.sequence, 1);
        assert_eq!(snap.exchange_timestamp, Some(now_ts));
    }

    #[test]
    fn build_snapshot_with_ticker_state() {
        let now_ts = fresh_exchange_ts();
        let book = InstrumentBook::new(InstrumentId::new("BTC-27JUN25-100000-C"));
        let ticker = TickerState {
            last_price: Some(0.0055),
            mark_price: Some(0.0057),
            index_price: Some(43500.0),
            mark_iv: Some(65.5),
            open_interest: Some(500.0),
            volume_24h: Some(100.0),
            greeks: Some(GreeksState {
                delta: 0.05,
                gamma: 0.00001,
                vega: 5.5,
                theta: -0.5,
                rho: 0.001,
            }),
            exchange_timestamp: Some(now_ts),
        };

        let snap = build_snapshot(
            &InstrumentId::new("BTC-27JUN25-100000-C"),
            &book,
            Some(&ticker),
            2,
            ts(),
            None,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        // Ticker data should be present
        assert!(snap.mark_price.is_some());
        let mark = snap.mark_price.unwrap().into_inner();
        assert_eq!(mark, Decimal::from_f64_retain(0.0057).unwrap());

        assert!(snap.index_price.is_some());
        assert_eq!(snap.mark_iv, Some(65.5));

        // Greeks should be present
        let g = snap.greeks.unwrap();
        assert!((g.delta - 0.05).abs() < f64::EPSILON);
        assert!((g.vega - 5.5).abs() < f64::EPSILON);

        // Open interest / volume
        assert!(snap.open_interest.is_some());
        assert!(snap.volume_24h.is_some());

        // Exchange timestamp from ticker (since we passed None explicitly)
        assert_eq!(snap.exchange_timestamp, Some(now_ts));

        assert_eq!(snap.sequence, 2);
    }

    #[test]
    fn build_snapshot_stale_flag_propagates() {
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));
        book.mark_stale();

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            None,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(snap.is_stale);
    }

    #[test]
    fn build_snapshot_empty_book_no_bid_ask() {
        let book = InstrumentBook::new(InstrumentId::new("TEST"));

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            None,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(snap.bid.is_none());
        assert!(snap.ask.is_none());
        assert!(snap.bid_size.is_none());
        assert!(snap.ask_size.is_none());
        assert!(snap.depth_bids.is_empty());
        assert!(snap.depth_asks.is_empty());
    }

    // --- Staleness gate tests ---

    #[test]
    fn test_staleness_gate_marks_old_data() {
        let book = InstrumentBook::new(InstrumentId::new("TEST"));
        // Exchange timestamp from 10 seconds ago (threshold 5000ms)
        let old_ts = chrono::Utc::now().timestamp_millis() - 10_000;

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            Some(old_ts),
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(snap.is_stale, "exchange data 10s old should be stale (threshold 5s)");
    }

    #[test]
    fn test_staleness_gate_fresh_data() {
        let book = InstrumentBook::new(InstrumentId::new("TEST"));
        // Exchange timestamp from 1 second ago (well within threshold)
        let fresh_ts = chrono::Utc::now().timestamp_millis() - 1_000;

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            Some(fresh_ts),
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(!snap.is_stale, "exchange data 1s old should not be stale (threshold 5s)");
    }

    #[test]
    fn test_staleness_gate_no_exchange_ts() {
        // Without exchange timestamp, staleness comes only from book.is_stale
        let book = InstrumentBook::new(InstrumentId::new("TEST"));

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            None,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(!snap.is_stale, "no exchange ts + fresh book = not stale");
    }

    #[test]
    fn test_staleness_or_with_book_stale() {
        // Book is stale but exchange_ts is fresh -- should still be stale (OR logic)
        let mut book = InstrumentBook::new(InstrumentId::new("TEST"));
        book.mark_stale();
        let fresh_ts = chrono::Utc::now().timestamp_millis() - 100;

        let snap = build_snapshot(
            &InstrumentId::new("TEST"),
            &book,
            None,
            1,
            ts(),
            Some(fresh_ts),
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(snap.is_stale, "stale book OR'd with fresh exchange_ts = stale");
    }

    #[test]
    fn test_is_exchange_data_stale_function() {
        let now = chrono::Utc::now().timestamp_millis();
        assert!(is_exchange_data_stale(now - 10_000, 5000));
        assert!(!is_exchange_data_stale(now - 1_000, 5000));
        assert!(!is_exchange_data_stale(now, 5000));
        // Future timestamp should not be stale
        assert!(!is_exchange_data_stale(now + 1_000, 5000));
    }

    #[test]
    fn sequence_numbers_increment() {
        let proc_seq = AtomicU64::new(1);
        let s1 = proc_seq.fetch_add(1, Ordering::Relaxed);
        let s2 = proc_seq.fetch_add(1, Ordering::Relaxed);
        let s3 = proc_seq.fetch_add(1, Ordering::Relaxed);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[tokio::test]
    async fn processor_handles_book_message() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) = DeribitProcessor::new(raw_rx, None, cancel.clone(), DEFAULT_STALENESS_THRESHOLD_MS);

        // Spawn processor
        let handle = tokio::spawn(processor.run());

        // Send a book notification
        let book_json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.BTC-27JUN25-100000-C.none.20.100ms",
                "data": {
                    "timestamp": 1703001600000,
                    "instrument_name": "BTC-27JUN25-100000-C",
                    "change_id": 12345678,
                    "type": "snapshot",
                    "bids": [[0.0055, 10.0], [0.0050, 25.0]],
                    "asks": [[0.0060, 8.0], [0.0065, 12.0]]
                }
            }
        }"#;

        raw_tx
            .send(RawMessage {
                text: book_json.to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // Should receive a snapshot
        let snap = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            snapshot_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(snap.venue, Venue::Deribit);
        assert_eq!(snap.instrument_id, InstrumentId::new("BTC-27JUN25-100000-C"));
        assert_eq!(snap.depth_bids.len(), 2);
        assert_eq!(snap.depth_asks.len(), 2);
        assert!(snap.bid.is_some());
        assert!(snap.ask.is_some());
        // The test data has a 2023 exchange timestamp, which the staleness
        // gate correctly flags as stale (RELY-03).
        assert!(snap.is_stale);

        // Clean up
        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_ticker_message() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) = DeribitProcessor::new(raw_rx, None, cancel.clone(), DEFAULT_STALENESS_THRESHOLD_MS);

        let handle = tokio::spawn(processor.run());

        let ticker_json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "ticker.BTC-27JUN25-100000-C.raw",
                "data": {
                    "timestamp": 1703001600000,
                    "instrument_name": "BTC-27JUN25-100000-C",
                    "state": "open",
                    "last_price": 0.0055,
                    "mark_price": 0.0057,
                    "index_price": 43500.0,
                    "open_interest": 500.0,
                    "min_price": 0.0001,
                    "max_price": 0.5,
                    "mark_iv": 65.5,
                    "greeks": {
                        "delta": 0.05,
                        "gamma": 0.00001,
                        "vega": 5.5,
                        "theta": -0.5,
                        "rho": 0.001
                    }
                }
            }
        }"#;

        raw_tx
            .send(RawMessage {
                text: ticker_json.to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        let snap = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            snapshot_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(snap.venue, Venue::Deribit);
        assert!(snap.mark_price.is_some());
        assert!(snap.greeks.is_some());
        assert_eq!(snap.mark_iv, Some(65.5));

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_trades_without_snapshot() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) = DeribitProcessor::new(raw_rx, None, cancel.clone(), DEFAULT_STALENESS_THRESHOLD_MS);

        let handle = tokio::spawn(processor.run());

        let trades_json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "trades.BTC-27JUN25-100000-C.raw",
                "data": [
                    {
                        "trade_id": "123456",
                        "instrument_name": "BTC-27JUN25-100000-C",
                        "timestamp": 1703001600000,
                        "direction": "buy",
                        "price": 0.0055,
                        "amount": 5.0,
                        "trade_seq": 100
                    }
                ]
            }
        }"#;

        raw_tx
            .send(RawMessage {
                text: trades_json.to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // Trades should NOT produce a snapshot
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            snapshot_rx.recv(),
        )
        .await;

        assert!(result.is_err(), "trades should not produce a snapshot");

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_parse_error_gracefully() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) = DeribitProcessor::new(raw_rx, None, cancel.clone(), DEFAULT_STALENESS_THRESHOLD_MS);

        let handle = tokio::spawn(processor.run());

        // Send invalid JSON
        raw_tx
            .send(RawMessage {
                text: "not valid json at all".to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // Send a valid book message after the invalid one
        let book_json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "book.TEST.none.20.100ms",
                "data": {
                    "timestamp": 1703001600000,
                    "instrument_name": "TEST",
                    "change_id": 1,
                    "bids": [[100.0, 1.0]],
                    "asks": [[101.0, 1.0]]
                }
            }
        }"#;

        raw_tx
            .send(RawMessage {
                text: book_json.to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // Should still receive the valid snapshot (parse error logged, not fatal)
        let snap = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            snapshot_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(snap.instrument_id, InstrumentId::new("TEST"));

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_rpc_response() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) = DeribitProcessor::new(raw_rx, None, cancel.clone(), DEFAULT_STALENESS_THRESHOLD_MS);

        let handle = tokio::spawn(processor.run());

        // Send an RPC response (subscribe confirmation)
        let response_json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": ["book.BTC-27JUN25-100000-C.none.20.100ms"]
        }"#;

        raw_tx
            .send(RawMessage {
                text: response_json.to_string(),
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // RPC responses should NOT produce snapshots
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            snapshot_rx.recv(),
        )
        .await;

        assert!(result.is_err(), "RPC responses should not produce snapshots");

        cancel.cancel();
        handle.await.unwrap();
    }
}
