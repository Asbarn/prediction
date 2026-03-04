//! Derive message processor and normalization pipeline.
//!
//! Receives raw WebSocket frames via an mpsc channel, parses them as
//! `DeriveMessage`, routes by channel type, updates per-instrument book
//! and ticker state, and publishes `MarketSnapshot` events downstream.
//!
//! Key differences from Deribit processor:
//! - Snapshot-only book model (no delta reconciliation)
//! - No heartbeat variant (WS-level PING/PONG)
//! - Prices are strings parsed to Decimal (USDC denomination, no conversion)
//! - Ticker uses abbreviated single-letter keys (`ticker_slim`)

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::DeriveConfig;
use crate::feed::derive::book::DeriveBook;
use crate::feed::derive::channels::{self, DeriveChannelKind};
use crate::feed::derive::messages::{
    DeriveBookData, DeriveMessage, DeriveTickerSlimData, DeriveTickerSlimWrapper,
};
use crate::feed::traits::{RawMessage, RecordLine};
use crate::subscription::CleanupEvent;
use crate::types::{
    DualTimestamp, InstrumentId, MarketSnapshot, Notional, Price, SnapshotGreeks, TraceId, Venue,
};

/// Buffer size for the downstream snapshot channel.
const SNAPSHOT_BUFFER: usize = 256;

/// Default staleness threshold in milliseconds (5 seconds).
#[cfg(test)]
const DEFAULT_STALENESS_THRESHOLD_MS: u64 = 5000;

/// Derive message processor.
///
/// Consumes `RawMessage` from the WS reader, maintains per-instrument book
/// and ticker state, and publishes `MarketSnapshot` events. Unlike Deribit,
/// books are snapshot-only (no delta reconciliation) and prices are USDC-denominated
/// strings parsed to `Decimal`.
pub struct DeriveProcessor {
    raw_rx: mpsc::Receiver<RawMessage>,
    snapshot_tx: mpsc::Sender<MarketSnapshot>,
    record_tx: Option<mpsc::Sender<RecordLine>>,
    cancel: CancellationToken,
    books: HashMap<String, DeriveBook>,
    ticker_data: HashMap<String, DeriveTickerSlimData>,
    sequence: AtomicU64,
    staleness_threshold_ms: u64,
    cleanup_rx: mpsc::Receiver<CleanupEvent>,
}

impl DeriveProcessor {
    /// Create a new processor.
    ///
    /// Returns `(DeriveProcessor, Receiver<MarketSnapshot>)`. The caller
    /// consumes the receiver; the processor owns the sender.
    pub fn new(
        raw_rx: mpsc::Receiver<RawMessage>,
        record_tx: Option<mpsc::Sender<RecordLine>>,
        cancel: CancellationToken,
        config: &DeriveConfig,
        cleanup_rx: mpsc::Receiver<CleanupEvent>,
    ) -> (Self, mpsc::Receiver<MarketSnapshot>) {
        let (snapshot_tx, snapshot_rx) = mpsc::channel(SNAPSHOT_BUFFER);
        let processor = Self {
            raw_rx,
            snapshot_tx,
            record_tx,
            cancel,
            books: HashMap::new(),
            ticker_data: HashMap::new(),
            sequence: AtomicU64::new(1),
            staleness_threshold_ms: config.staleness_threshold_ms,
            cleanup_rx,
        };
        (processor, snapshot_rx)
    }

    /// Run the processing loop until cancelled or the input channel closes.
    pub async fn run(mut self) {
        tracing::info!("DeriveProcessor starting");

        loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    tracing::info!("DeriveProcessor cancelled");
                    break;
                }

                Some(cleanup) = self.cleanup_rx.recv() => {
                    let before_books = self.books.len();
                    let before_tickers = self.ticker_data.len();
                    for inst in &cleanup.derive_instruments {
                        self.books.remove(inst);
                        self.ticker_data.remove(inst);
                    }
                    tracing::info!(
                        books_removed = before_books - self.books.len(),
                        tickers_removed = before_tickers - self.ticker_data.len(),
                        "DeriveProcessor: cleaned up stale entries"
                    );
                }

                msg = self.raw_rx.recv() => {
                    match msg {
                        Some(raw) => self.handle_raw_message(raw).await,
                        None => {
                            tracing::info!("DeriveProcessor input channel closed");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("DeriveProcessor exiting");
    }

    /// Process a single raw message.
    async fn handle_raw_message(&mut self, raw: RawMessage) {
        // Parse the raw text as a DeriveMessage
        let message: DeriveMessage = match serde_json::from_str(&raw.text) {
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
                    "failed to parse Derive message"
                );
                return;
            }
        };

        match message {
            DeriveMessage::Response(resp) => {
                if let Some(err) = &resp.error {
                    tracing::error!(
                        id = resp.id,
                        code = err.code,
                        message = %err.message,
                        "Derive RPC error response"
                    );
                } else {
                    tracing::info!(
                        id = resp.id,
                        "Derive subscribe confirmation"
                    );
                }
            }
            DeriveMessage::Notification(notif) => {
                let channel = notif.params.channel.clone();
                let kind = DeriveChannelKind::parse(&channel);
                let instrument = channels::extract_instrument(&channel);

                // Fan-out to recording if configured
                if let Some(ref record_tx) = self.record_tx {
                    let _ = record_tx.try_send(RecordLine {
                        raw: raw.text.clone(),
                        local_ts: raw.received_at.wall(),
                        venue: Venue::Derive,
                        channel: channel.clone(),
                        instrument: instrument.clone(),
                    });
                }

                match kind {
                    DeriveChannelKind::Orderbook => {
                        self.process_orderbook(&channel, notif.params.data, raw.received_at)
                            .await;
                    }
                    DeriveChannelKind::TickerSlim => {
                        self.process_ticker_slim(&channel, notif.params.data, raw.received_at)
                            .await;
                    }
                    DeriveChannelKind::Unknown(ch) => {
                        tracing::debug!(channel = %ch, "ignoring unknown Derive channel");
                    }
                }
            }
        }
    }

    /// Handle an orderbook channel message.
    async fn process_orderbook(
        &mut self,
        channel: &str,
        data: serde_json::Value,
        received_at: DualTimestamp,
    ) {
        let book_data: DeriveBookData = match serde_json::from_value(data) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize DeriveBookData");
                return;
            }
        };

        let instrument_name = match channels::extract_instrument(channel) {
            Some(name) => name,
            None => {
                tracing::warn!(channel = %channel, "could not extract instrument from channel");
                return;
            }
        };

        // Get or create book
        let book = self
            .books
            .entry(instrument_name.clone())
            .or_insert_with(|| DeriveBook::new(instrument_name.clone()));

        book.apply_snapshot(&book_data);

        tracing::debug!(
            instrument = %instrument_name,
            publish_id = book_data.publish_id,
            bids = book.bids.len(),
            asks = book.asks.len(),
            "derive book snapshot applied"
        );

        // Try to build and emit snapshot
        self.try_emit_snapshot(&instrument_name, received_at).await;
    }

    /// Handle a ticker_slim channel message.
    async fn process_ticker_slim(
        &mut self,
        channel: &str,
        data: serde_json::Value,
        received_at: DualTimestamp,
    ) {
        let wrapper: DeriveTickerSlimWrapper = match serde_json::from_value(data) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize DeriveTickerSlimWrapper");
                return;
            }
        };

        let instrument_name = match channels::extract_instrument(channel) {
            Some(name) => name,
            None => {
                tracing::warn!(channel = %channel, "could not extract instrument from channel");
                return;
            }
        };

        tracing::debug!(
            instrument = %instrument_name,
            mark_price = ?wrapper.instrument_ticker.mark_price,
            "derive ticker_slim update"
        );

        // Store the ticker data
        self.ticker_data
            .insert(instrument_name.clone(), wrapper.instrument_ticker);

        // Try to build and emit snapshot
        self.try_emit_snapshot(&instrument_name, received_at).await;
    }

    /// Attempt to build a MarketSnapshot and send it downstream.
    ///
    /// Requires both book AND ticker data for the instrument.
    async fn try_emit_snapshot(&mut self, instrument_name: &str, received_at: DualTimestamp) {
        let book = match self.books.get_mut(instrument_name) {
            Some(b) => b,
            None => return,
        };
        let ticker = match self.ticker_data.get(instrument_name) {
            Some(t) => t,
            None => return,
        };

        // Determine exchange timestamp (most recent of book or ticker)
        let book_ts = book.last_timestamp.unwrap_or(0);
        let ticker_ts = ticker.timestamp.unwrap_or(0);
        let exchange_ts = book_ts.max(ticker_ts);

        // Staleness check
        if is_exchange_data_stale(exchange_ts, self.staleness_threshold_ms) {
            let age_ms =
                (chrono::Utc::now().timestamp_millis() - exchange_ts).max(0) as u64;
            tracing::warn!(
                instrument = %instrument_name,
                age_ms = age_ms,
                threshold_ms = self.staleness_threshold_ms,
                "derive exchange data stale -- skipping snapshot"
            );
            book.mark_stale();
            return;
        }

        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let snapshot = build_snapshot(
            instrument_name,
            book,
            ticker,
            seq,
            received_at,
            exchange_ts,
            self.staleness_threshold_ms,
        );

        if self.snapshot_tx.send(snapshot).await.is_err() {
            tracing::warn!("derive snapshot receiver dropped");
        }
    }
}

/// Parse an `Option<String>` to `Option<f64>` via `Decimal`.
///
/// Returns None on parse failure or None input. Uses Decimal as intermediate
/// to avoid floating-point precision loss on the parse path.
fn parse_decimal_option(s: &Option<String>) -> Option<f64> {
    s.as_ref()
        .and_then(|v| Decimal::from_str(v).ok())
        .and_then(|d| d.to_string().parse::<f64>().ok())
}

/// Check if exchange data is stale based on exchange-reported timestamp.
fn is_exchange_data_stale(exchange_ts_ms: i64, threshold_ms: u64) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let age_ms = (now_ms - exchange_ts_ms).max(0) as u64;
    age_ms > threshold_ms
}

/// Build a `MarketSnapshot` from Derive book and ticker state.
///
/// Prices are USDC-denominated -- no conversion needed (unlike Deribit's
/// BTC-inverse pricing).
fn build_snapshot(
    instrument_name: &str,
    book: &DeriveBook,
    ticker: &DeriveTickerSlimData,
    sequence: u64,
    received_at: DualTimestamp,
    exchange_ts: i64,
    staleness_threshold_ms: u64,
) -> MarketSnapshot {
    let inst_id = InstrumentId::new(instrument_name);

    // Best bid/ask from book (Decimal -> Price)
    let (bid, bid_size) = match book.best_bid() {
        Some((p, s)) => (Some(Price::new(p)), Some(Notional::new(s))),
        None => (None, None),
    };
    let (ask, ask_size) = match book.best_ask() {
        Some((p, s)) => (Some(Price::new(p)), Some(Notional::new(s))),
        None => (None, None),
    };

    // Depth levels
    let depth_bids: Vec<(Price, Notional)> = book
        .bids
        .iter()
        .map(|(p, s)| (Price::new(*p), Notional::new(*s)))
        .collect();
    let depth_asks: Vec<(Price, Notional)> = book
        .asks
        .iter()
        .map(|(p, s)| (Price::new(*p), Notional::new(*s)))
        .collect();

    // Mark price from ticker `M` field
    let mark_price = parse_decimal_option(&ticker.mark_price)
        .map(|v| Price::new(Decimal::from_str(&v.to_string()).unwrap_or(Decimal::ZERO)));

    // Index price from ticker `I` field
    let index_price = parse_decimal_option(&ticker.index_price)
        .map(|v| Price::new(Decimal::from_str(&v.to_string()).unwrap_or(Decimal::ZERO)));

    // Mark IV from ticker `option_pricing.i`
    let mark_iv = ticker
        .option_pricing
        .as_ref()
        .and_then(|op| parse_decimal_option(&op.iv));

    // Bid/Ask IV
    let bid_iv = ticker
        .option_pricing
        .as_ref()
        .and_then(|op| parse_decimal_option(&op.bid_iv));
    let ask_iv = ticker
        .option_pricing
        .as_ref()
        .and_then(|op| parse_decimal_option(&op.ask_iv));

    // Underlying/forward price from option_pricing.f (NOT top-level f)
    let underlying_price = ticker
        .option_pricing
        .as_ref()
        .and_then(|op| parse_decimal_option(&op.forward));

    // Greeks from option_pricing
    let greeks = ticker.option_pricing.as_ref().and_then(|op| {
        let delta = parse_decimal_option(&op.delta)?;
        let gamma = parse_decimal_option(&op.gamma)?;
        let vega = parse_decimal_option(&op.vega)?;
        let theta = parse_decimal_option(&op.theta)?;
        Some(SnapshotGreeks {
            delta,
            gamma,
            vega,
            theta,
            rho: 0.0, // Derive does not provide rho
        })
    });

    // Staleness: OR book staleness with exchange-timestamp age check
    let exchange_data_stale = is_exchange_data_stale(exchange_ts, staleness_threshold_ms);
    let is_stale = book.is_stale || exchange_data_stale;

    // Latency metrics
    let local_ms = received_at.wall().timestamp_millis();
    let latency_ms = (local_ms - exchange_ts) as f64;
    metrics::histogram!("feed_latency_ms", "venue" => "derive").record(latency_ms);
    metrics::gauge!("feed_last_latency_ms", "venue" => "derive").set(latency_ms);
    metrics::counter!("feed_messages_total", "venue" => "derive").increment(1);

    MarketSnapshot {
        venue: Venue::Derive,
        instrument_id: inst_id,
        event_id: None,
        bid,
        ask,
        bid_size,
        ask_size,
        depth_bids,
        depth_asks,
        bid_probability: None,
        ask_probability: None,
        last_price: None, // Derive ticker_slim does not provide last_price
        mark_price,
        index_price,
        mark_iv,
        open_interest: None, // Not available in ticker_slim
        volume_24h: None,    // Not available in ticker_slim core fields
        greeks,
        bid_iv,
        ask_iv,
        underlying_price,
        underlying_index: None, // Derive does not use underlying_index
        exchange_timestamp: Some(exchange_ts),
        timestamp: received_at,
        sequence,
        trace_id: TraceId::new(),
        is_stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::derive::messages::{DeriveOptionPricing, DeriveTickerSlimData};

    fn ts() -> DualTimestamp {
        DualTimestamp::now()
    }

    fn fresh_exchange_ts() -> i64 {
        chrono::Utc::now().timestamp_millis() - 100
    }

    fn make_ticker_data() -> DeriveTickerSlimData {
        DeriveTickerSlimData {
            timestamp: Some(fresh_exchange_ts()),
            best_ask_amount: Some("0.4".to_string()),
            best_ask_price: Some("414".to_string()),
            best_bid_amount: Some("0.4".to_string()),
            best_bid_price: Some("341".to_string()),
            index_price: Some("71078".to_string()),
            mark_price: Some("364".to_string()),
            forward_price: None,
            option_pricing: Some(DeriveOptionPricing {
                delta: Some("-0.24967".to_string()),
                theta: Some("-453.85103".to_string()),
                gamma: Some("0.00013192".to_string()),
                vega: Some("10.84014".to_string()),
                iv: Some("0.70513".to_string()),
                rate: Some("0.84114".to_string()),
                forward: Some("71067".to_string()),
                mark_price: Some("364".to_string()),
                discount_factor: Some("1".to_string()),
                bid_iv: Some("0.68323".to_string()),
                ask_iv: Some("0.75013".to_string()),
            }),
        }
    }

    fn make_book_with_data() -> DeriveBook {
        let mut book = DeriveBook::new("BTC-20260305-69500-P".to_string());
        let data = crate::feed::derive::messages::DeriveBookData {
            timestamp: fresh_exchange_ts(),
            instrument_name: "BTC-20260305-69500-P".to_string(),
            publish_id: 56593,
            bids: vec![
                ["340".to_string(), "0.4".to_string()],
                ["320".to_string(), "1".to_string()],
            ],
            asks: vec![
                ["420".to_string(), "0.4".to_string()],
                ["520".to_string(), "0.7".to_string()],
            ],
        };
        book.apply_snapshot(&data);
        book
    }

    #[test]
    fn build_snapshot_with_both_book_and_ticker() {
        let book = make_book_with_data();
        let ticker = make_ticker_data();
        let now = fresh_exchange_ts();

        let snap = build_snapshot(
            "BTC-20260305-69500-P",
            &book,
            &ticker,
            1,
            ts(),
            now,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert_eq!(snap.venue, Venue::Derive);
        assert_eq!(
            snap.instrument_id,
            InstrumentId::new("BTC-20260305-69500-P")
        );

        // Bid/ask from book
        assert!(snap.bid.is_some());
        assert!(snap.ask.is_some());
        assert_eq!(snap.depth_bids.len(), 2);
        assert_eq!(snap.depth_asks.len(), 2);

        // Mark price from ticker
        assert!(snap.mark_price.is_some());
        let mark = snap.mark_price.unwrap().into_inner();
        assert_eq!(mark, Decimal::from_str("364").unwrap());

        // Greeks from ticker
        let g = snap.greeks.unwrap();
        assert!((g.delta - (-0.24967)).abs() < 1e-6);
        assert!((g.gamma - 0.00013192).abs() < 1e-9);
        assert!((g.vega - 10.84014).abs() < 1e-6);
        assert!((g.theta - (-453.85103)).abs() < 1e-4);
        assert_eq!(g.rho, 0.0); // Derive doesn't provide rho

        // IV values
        assert!((snap.mark_iv.unwrap() - 0.70513).abs() < 1e-6);
        assert!((snap.bid_iv.unwrap() - 0.68323).abs() < 1e-6);
        assert!((snap.ask_iv.unwrap() - 0.75013).abs() < 1e-6);

        // Underlying from option_pricing.f
        assert!((snap.underlying_price.unwrap() - 71067.0).abs() < 1e-1);

        // Not stale (fresh timestamp)
        assert!(!snap.is_stale);
        assert_eq!(snap.sequence, 1);
    }

    #[test]
    fn build_snapshot_stale_data_detected() {
        let book = make_book_with_data();
        let ticker = make_ticker_data();
        // Exchange timestamp from 10 seconds ago
        let old_ts = chrono::Utc::now().timestamp_millis() - 10_000;

        let snap = build_snapshot(
            "BTC-20260305-69500-P",
            &book,
            &ticker,
            1,
            ts(),
            old_ts,
            DEFAULT_STALENESS_THRESHOLD_MS,
        );

        assert!(snap.is_stale, "exchange data 10s old should be stale (threshold 5s)");
    }

    #[test]
    fn parse_decimal_option_works() {
        assert_eq!(parse_decimal_option(&Some("0.70513".to_string())), Some(0.70513));
        assert_eq!(parse_decimal_option(&Some("364".to_string())), Some(364.0));
        assert_eq!(parse_decimal_option(&None), None);
        assert_eq!(parse_decimal_option(&Some("not_a_number".to_string())), None);
    }

    #[test]
    fn is_exchange_data_stale_function() {
        let now = chrono::Utc::now().timestamp_millis();
        assert!(is_exchange_data_stale(now - 10_000, 5000));
        assert!(!is_exchange_data_stale(now - 1_000, 5000));
        assert!(!is_exchange_data_stale(now, 5000));
        // Future timestamp should not be stale
        assert!(!is_exchange_data_stale(now + 1_000, 5000));
    }

    #[tokio::test]
    async fn processor_routes_orderbook_message() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (_cleanup_tx, cleanup_rx) = mpsc::channel::<CleanupEvent>(1);
        let config = DeriveConfig {
            ws_url: "wss://test".to_string(),
            rate_limit_per_second: 10,
            book_depth_levels: 10,
            staleness_threshold_ms: DEFAULT_STALENESS_THRESHOLD_MS,
            reconnect: Default::default(),
            instruments: vec![],
        };
        let (processor, mut snapshot_rx) =
            DeriveProcessor::new(raw_rx, None, cancel.clone(), &config, cleanup_rx);

        let handle = tokio::spawn(processor.run());

        let now_ts = fresh_exchange_ts();

        // Send a book notification first
        let book_json = format!(
            r#"{{
                "method": "subscription",
                "params": {{
                    "channel": "orderbook.BTC-20260305-69500-P.10.10",
                    "data": {{
                        "timestamp": {now_ts},
                        "instrument_name": "BTC-20260305-69500-P",
                        "publish_id": 56593,
                        "bids": [["340", "0.4"]],
                        "asks": [["420", "0.4"]]
                    }}
                }}
            }}"#
        );

        raw_tx
            .send(RawMessage {
                text: book_json,
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // No snapshot yet (no ticker data)
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            snapshot_rx.recv(),
        )
        .await;
        assert!(
            result.is_err(),
            "should not produce snapshot without ticker data"
        );

        // Now send ticker data
        let ticker_json = format!(
            r#"{{
                "method": "subscription",
                "params": {{
                    "channel": "ticker_slim.BTC-20260305-69500-P.100",
                    "data": {{
                        "timestamp": {now_ts},
                        "instrument_ticker": {{
                            "t": {now_ts},
                            "A": "0.4",
                            "a": "414",
                            "B": "0.4",
                            "b": "341",
                            "f": null,
                            "option_pricing": {{
                                "d": "-0.24967",
                                "t": "-453.85103",
                                "g": "0.00013192",
                                "v": "10.84014",
                                "i": "0.70513",
                                "r": "0.84114",
                                "f": "71067",
                                "m": "364",
                                "df": "1",
                                "bi": "0.68323",
                                "ai": "0.75013"
                            }},
                            "I": "71078",
                            "M": "364"
                        }}
                    }}
                }}
            }}"#
        );

        raw_tx
            .send(RawMessage {
                text: ticker_json,
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        // Now should receive a snapshot (both book and ticker present)
        let snap = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            snapshot_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(snap.venue, Venue::Derive);
        assert_eq!(
            snap.instrument_id,
            InstrumentId::new("BTC-20260305-69500-P")
        );
        assert!(snap.bid.is_some());
        assert!(snap.ask.is_some());
        assert!(snap.mark_price.is_some());
        assert!(snap.greeks.is_some());
        assert!(!snap.is_stale);

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_rpc_response() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (_cleanup_tx, cleanup_rx) = mpsc::channel::<CleanupEvent>(1);
        let config = DeriveConfig {
            ws_url: "wss://test".to_string(),
            rate_limit_per_second: 10,
            book_depth_levels: 10,
            staleness_threshold_ms: DEFAULT_STALENESS_THRESHOLD_MS,
            reconnect: Default::default(),
            instruments: vec![],
        };
        let (processor, mut snapshot_rx) =
            DeriveProcessor::new(raw_rx, None, cancel.clone(), &config, cleanup_rx);

        let handle = tokio::spawn(processor.run());

        let response_json = r#"{
            "id": 1,
            "result": {
                "status": {
                    "orderbook.BTC-20260305-69500-P.10.10": "ok"
                },
                "current_subscriptions": [
                    "orderbook.BTC-20260305-69500-P.10.10"
                ]
            }
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

    #[tokio::test]
    async fn processor_records_messages() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let (record_tx, mut record_rx) = mpsc::channel::<RecordLine>(16);
        let cancel = CancellationToken::new();
        let (_cleanup_tx, cleanup_rx) = mpsc::channel::<CleanupEvent>(1);
        let config = DeriveConfig {
            ws_url: "wss://test".to_string(),
            rate_limit_per_second: 10,
            book_depth_levels: 10,
            staleness_threshold_ms: DEFAULT_STALENESS_THRESHOLD_MS,
            reconnect: Default::default(),
            instruments: vec![],
        };
        let (processor, _snapshot_rx) =
            DeriveProcessor::new(raw_rx, Some(record_tx), cancel.clone(), &config, cleanup_rx);

        let handle = tokio::spawn(processor.run());

        let now_ts = fresh_exchange_ts();
        let book_json = format!(
            r#"{{
                "method": "subscription",
                "params": {{
                    "channel": "orderbook.BTC-20260305-69500-P.10.10",
                    "data": {{
                        "timestamp": {now_ts},
                        "instrument_name": "BTC-20260305-69500-P",
                        "publish_id": 56593,
                        "bids": [["340", "0.4"]],
                        "asks": [["420", "0.4"]]
                    }}
                }}
            }}"#
        );

        raw_tx
            .send(RawMessage {
                text: book_json,
                received_at: DualTimestamp::now(),
            })
            .await
            .unwrap();

        let record = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            record_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(record.venue, Venue::Derive);
        assert_eq!(record.channel, "orderbook.BTC-20260305-69500-P.10.10");
        assert_eq!(
            record.instrument,
            Some("BTC-20260305-69500-P".to_string())
        );

        cancel.cancel();
        handle.await.unwrap();
    }
}
