//! Polymarket message processor and normalization pipeline.
//!
//! Receives raw WebSocket frames via an mpsc channel, parses them as
//! Polymarket events, and converts book snapshots into `MarketSnapshot`
//! events with probability fields populated.
//!
//! Polymarket prices ARE probabilities (Pattern 3 from research):
//! `bid_probability = bid price`, `ask_probability = ask price`.

use std::sync::atomic::{AtomicU64, Ordering};

use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::polymarket::messages::{self, PolymarketBookEvent, PolymarketEvent};
use crate::feed::traits::{RawMessage, RecordLine};
use crate::types::{
    DualTimestamp, InstrumentId, MarketSnapshot, Notional, Price, Probability, TraceId, Venue,
};

/// Buffer size for the downstream snapshot channel.
const SNAPSHOT_BUFFER: usize = 256;

/// Polymarket message processor.
///
/// Consumes `RawMessage` from the WS reader, parses Polymarket events,
/// and produces `MarketSnapshot` events with probability fields populated.
pub struct PolymarketProcessor {
    raw_rx: mpsc::Receiver<RawMessage>,
    snapshot_tx: mpsc::Sender<MarketSnapshot>,
    record_tx: Option<mpsc::Sender<RecordLine>>,
    cancel: CancellationToken,
    sequence: AtomicU64,
    staleness_threshold_ms: u64,
}

impl PolymarketProcessor {
    /// Create a new processor.
    ///
    /// Returns `(PolymarketProcessor, Receiver<MarketSnapshot>)`.
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
            sequence: AtomicU64::new(1),
            staleness_threshold_ms,
        };
        (processor, snapshot_rx)
    }

    /// Run the processing loop until cancelled or the input channel closes.
    pub async fn run(mut self) {
        tracing::info!("PolymarketProcessor starting");

        loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    tracing::info!("PolymarketProcessor cancelled");
                    break;
                }

                msg = self.raw_rx.recv() => {
                    match msg {
                        Some(raw) => self.handle_raw_message(raw).await,
                        None => {
                            tracing::info!("PolymarketProcessor input channel closed");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("PolymarketProcessor exiting");
    }

    /// Process a single raw message.
    async fn handle_raw_message(&mut self, raw: RawMessage) {
        // Record raw message (best-effort)
        if let Some(ref record_tx) = self.record_tx {
            let _ = record_tx.try_send(RecordLine {
                raw: raw.text.clone(),
                local_ts: raw.received_at.wall(),
                venue: Venue::Polymarket,
                channel: String::new(),
                instrument: None,
            });
        }

        // Parse events (handles both array and single object -- Pitfall 5)
        let events = messages::parse_events(&raw.text);

        for event in events {
            match event {
                PolymarketEvent::Book(book) => {
                    self.handle_book_event(&book, raw.received_at).await;
                }
                PolymarketEvent::PriceChange(pc) => {
                    tracing::debug!(
                        market = %pc.market,
                        changes = pc.price_changes.len(),
                        "Polymarket price_change (Phase 4: log only, using full book snapshots)"
                    );
                }
                PolymarketEvent::TickSizeChange(_) => {
                    tracing::debug!("Polymarket tick_size_change (ignored)");
                }
                PolymarketEvent::Unknown => {
                    tracing::debug!("Polymarket unknown event type (ignored)");
                }
            }
        }
    }

    /// Handle a Polymarket book event and produce a MarketSnapshot.
    async fn handle_book_event(&mut self, book: &PolymarketBookEvent, received_at: DualTimestamp) {
        let inst_id = InstrumentId::new(&book.asset_id);
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);

        // Parse exchange timestamp from string (milliseconds)
        let exchange_ts: Option<i64> = book.timestamp.parse().ok();

        // Staleness gate
        let exchange_data_stale = exchange_ts
            .map(|ts| is_exchange_data_stale(ts, self.staleness_threshold_ms))
            .unwrap_or(false);

        let is_stale = exchange_data_stale;

        if exchange_data_stale {
            tracing::warn!(
                asset_id = %book.asset_id,
                "Polymarket exchange data stale -- marking is_stale=true"
            );
        }

        // Parse bid levels: Polymarket sends bids sorted descending (best first)
        let depth_bids: Vec<(Price, Notional)> = book
            .bids
            .iter()
            .filter_map(|level| parse_price_level(level))
            .collect();

        // Parse ask levels: Polymarket sends asks sorted ascending (best first)
        let depth_asks: Vec<(Price, Notional)> = book
            .asks
            .iter()
            .filter_map(|level| parse_price_level(level))
            .collect();

        let best_bid = depth_bids.first().copied();
        let best_ask = depth_asks.first().copied();

        // Polymarket prices ARE probabilities (Pattern 3)
        let bid_probability = best_bid
            .and_then(|(p, _)| Probability::new(p.into_inner()).ok());
        let ask_probability = best_ask
            .and_then(|(p, _)| Probability::new(p.into_inner()).ok());

        // Latency metrics
        if let Some(exchange_ts_ms) = exchange_ts {
            let local_ms = received_at.wall().timestamp_millis();
            let latency_ms = (local_ms - exchange_ts_ms) as f64;
            metrics::histogram!("feed_latency_ms", "venue" => "polymarket").record(latency_ms);
            metrics::gauge!("feed_last_latency_ms", "venue" => "polymarket").set(latency_ms);
        }
        metrics::counter!("feed_messages_total", "venue" => "polymarket").increment(1);

        let snapshot = MarketSnapshot {
            venue: Venue::Polymarket,
            instrument_id: inst_id,
            event_id: None,
            bid: best_bid.map(|(p, _)| p),
            ask: best_ask.map(|(p, _)| p),
            bid_size: best_bid.map(|(_, s)| s),
            ask_size: best_ask.map(|(_, s)| s),
            depth_bids,
            depth_asks,
            bid_probability,
            ask_probability,
            last_price: None,
            mark_price: None,
            index_price: None,
            mark_iv: None,
            open_interest: None,
            volume_24h: None,
            greeks: None,
            exchange_timestamp: exchange_ts,
            timestamp: received_at,
            sequence: seq,
            trace_id: TraceId::new(),
            is_stale,
        };

        if self.snapshot_tx.send(snapshot).await.is_err() {
            tracing::warn!("Polymarket snapshot receiver dropped");
        }
    }
}

/// Parse a Polymarket price level (string price, string size) to (Price, Notional).
fn parse_price_level(level: &crate::feed::polymarket::messages::PriceLevel) -> Option<(Price, Notional)> {
    let price: Decimal = level.price.parse().ok()?;
    let size: Decimal = level.size.parse().ok()?;
    Some((Price::new(price), Notional::new(size)))
}

/// Check if exchange data is stale based on exchange-reported timestamp.
fn is_exchange_data_stale(exchange_ts_ms: i64, threshold_ms: u64) -> bool {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let age_ms = (now_ms - exchange_ts_ms).max(0) as u64;
    age_ms > threshold_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DualTimestamp {
        DualTimestamp::now()
    }

    fn fresh_exchange_ts() -> String {
        (chrono::Utc::now().timestamp_millis() - 100).to_string()
    }

    #[tokio::test]
    async fn processor_normalizes_book_event() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            PolymarketProcessor::new(raw_rx, None, cancel.clone(), 5000);

        let handle = tokio::spawn(processor.run());

        let book_json = format!(
            r#"{{
            "event_type": "book",
            "asset_id": "token123",
            "market": "0xmarket",
            "hash": "abc",
            "bids": [
                {{"price": "0.55", "size": "100.0"}},
                {{"price": "0.54", "size": "200.0"}}
            ],
            "asks": [
                {{"price": "0.56", "size": "150.0"}},
                {{"price": "0.57", "size": "250.0"}}
            ],
            "timestamp": "{}"
        }}"#,
            fresh_exchange_ts()
        );

        raw_tx
            .send(RawMessage {
                text: book_json,
                received_at: ts(),
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

        assert_eq!(snap.venue, Venue::Polymarket);
        assert_eq!(snap.instrument_id, InstrumentId::new("token123"));
        assert_eq!(snap.depth_bids.len(), 2);
        assert_eq!(snap.depth_asks.len(), 2);

        // Bid probability = bid price (Pattern 3)
        let bid_prob = snap.bid_probability.unwrap();
        assert_eq!(bid_prob.into_inner(), Decimal::new(55, 2)); // 0.55

        let ask_prob = snap.ask_probability.unwrap();
        assert_eq!(ask_prob.into_inner(), Decimal::new(56, 2)); // 0.56

        assert!(!snap.is_stale);
        assert!(snap.greeks.is_none());
        assert!(snap.mark_price.is_none());

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_stale_data() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            PolymarketProcessor::new(raw_rx, None, cancel.clone(), 5000);

        let handle = tokio::spawn(processor.run());

        // Old timestamp (10 seconds ago, threshold is 5s)
        let old_ts = (chrono::Utc::now().timestamp_millis() - 10_000).to_string();
        let book_json = format!(
            r#"{{
            "event_type": "book",
            "asset_id": "token_stale",
            "market": "0xmarket",
            "bids": [{{"price": "0.50", "size": "10.0"}}],
            "asks": [{{"price": "0.51", "size": "20.0"}}],
            "timestamp": "{}"
        }}"#,
            old_ts
        );

        raw_tx
            .send(RawMessage {
                text: book_json,
                received_at: ts(),
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

        assert!(snap.is_stale, "10s old data should be stale (threshold 5s)");

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_price_change_without_crash() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            PolymarketProcessor::new(raw_rx, None, cancel.clone(), 5000);

        let handle = tokio::spawn(processor.run());

        let pc_json = r#"{
            "event_type": "price_change",
            "market": "0xmarket",
            "price_changes": [
                {"asset_id": "tok1", "price": "0.60", "size": "50.0", "side": "BUY", "best_bid": "0.59", "best_ask": "0.61"}
            ],
            "timestamp": "1703001600000"
        }"#;

        raw_tx
            .send(RawMessage {
                text: pc_json.to_string(),
                received_at: ts(),
            })
            .await
            .unwrap();

        // PriceChange should NOT produce a snapshot
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            snapshot_rx.recv(),
        )
        .await;

        assert!(result.is_err(), "price_change should not produce a snapshot");

        cancel.cancel();
        handle.await.unwrap();
    }

    #[test]
    fn parse_price_level_valid() {
        let level = crate::feed::polymarket::messages::PriceLevel {
            price: "0.55".to_string(),
            size: "100.0".to_string(),
        };
        let (price, notional) = parse_price_level(&level).unwrap();
        assert_eq!(price.into_inner(), Decimal::new(55, 2));
        assert_eq!(notional.into_inner(), Decimal::new(1000, 1));
    }

    #[test]
    fn parse_price_level_invalid_returns_none() {
        let level = crate::feed::polymarket::messages::PriceLevel {
            price: "not_a_number".to_string(),
            size: "100.0".to_string(),
        };
        assert!(parse_price_level(&level).is_none());
    }
}
