//! Kalshi normalization processor.
//!
//! Receives raw WebSocket frames, parses Kalshi messages, maintains
//! per-market incremental order book state, and produces MarketSnapshot
//! events with cents-to-probability conversion.
//!
//! Key normalization: Kalshi prices in cents (1-99) -> probability (0.01-0.99).
//! Asks are derived from the complementary side (Pitfall 2).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::kalshi::book::KalshiBook;
use crate::feed::kalshi::messages::KalshiMessage;
use crate::feed::traits::{RawMessage, RecordLine};
use crate::subscription::CleanupEvent;
use crate::types::{
    DualTimestamp, InstrumentId, MarketSnapshot, Notional, Price, Probability, TraceId, Venue,
};

/// Buffer size for the downstream snapshot channel.
const SNAPSHOT_BUFFER: usize = 256;

/// Convert Kalshi cents to probability.
///
/// 42 cents -> 0.42, 99 cents -> 0.99.
pub fn cents_to_probability(cents: i64) -> Decimal {
    Decimal::new(cents, 2)
}

/// Kalshi message processor.
///
/// Maintains per-market book state and produces MarketSnapshot events
/// from the YES contract perspective.
pub struct KalshiProcessor {
    raw_rx: mpsc::Receiver<RawMessage>,
    snapshot_tx: mpsc::Sender<MarketSnapshot>,
    record_tx: Option<mpsc::Sender<RecordLine>>,
    cancel: CancellationToken,
    staleness_threshold_ms: u64,
    books: HashMap<String, KalshiBook>,
    sequence: AtomicU64,
    /// Last exchange timestamp (ISO 8601) seen from orderbook_delta messages, per market.
    /// Used for best-effort exchange_timestamp on MarketSnapshot.
    last_exchange_ts: HashMap<String, String>,
    /// Cleanup channel: receives events when instruments are unsubscribed,
    /// triggering eviction of stale books and last_exchange_ts.
    cleanup_rx: mpsc::Receiver<CleanupEvent>,
}

impl KalshiProcessor {
    /// Create a new processor.
    ///
    /// Returns `(KalshiProcessor, Receiver<MarketSnapshot>)`.
    pub fn new(
        raw_rx: mpsc::Receiver<RawMessage>,
        record_tx: Option<mpsc::Sender<RecordLine>>,
        cancel: CancellationToken,
        staleness_threshold_ms: u64,
        cleanup_rx: mpsc::Receiver<CleanupEvent>,
    ) -> (Self, mpsc::Receiver<MarketSnapshot>) {
        let (snapshot_tx, snapshot_rx) = mpsc::channel(SNAPSHOT_BUFFER);
        let processor = Self {
            raw_rx,
            snapshot_tx,
            record_tx,
            cancel,
            staleness_threshold_ms,
            books: HashMap::new(),
            sequence: AtomicU64::new(1),
            last_exchange_ts: HashMap::new(),
            cleanup_rx,
        };
        (processor, snapshot_rx)
    }

    /// Run the processing loop until cancelled or the input channel closes.
    pub async fn run(mut self) {
        tracing::info!("KalshiProcessor starting");

        loop {
            tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    tracing::info!("KalshiProcessor cancelled");
                    break;
                }

                Some(cleanup) = self.cleanup_rx.recv() => {
                    let to_remove: std::collections::HashSet<&str> =
                        cleanup.kalshi_tickers.iter().map(|s| s.as_str()).collect();
                    let before_books = self.books.len();
                    self.books.retain(|k, _| !to_remove.contains(k.as_str()));
                    let before_ts = self.last_exchange_ts.len();
                    self.last_exchange_ts.retain(|k, _| !to_remove.contains(k.as_str()));
                    tracing::info!(
                        books_removed = before_books - self.books.len(),
                        ts_removed = before_ts - self.last_exchange_ts.len(),
                        "KalshiProcessor: cleaned up stale entries"
                    );
                }

                msg = self.raw_rx.recv() => {
                    match msg {
                        Some(raw) => self.handle_raw_message(raw).await,
                        None => {
                            tracing::info!("KalshiProcessor input channel closed");
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("KalshiProcessor exiting");
    }

    /// Process a single raw message.
    async fn handle_raw_message(&mut self, raw: RawMessage) {
        // Record raw message (best-effort)
        if let Some(ref record_tx) = self.record_tx {
            let _ = record_tx.try_send(RecordLine {
                raw: raw.text.clone(),
                local_ts: raw.received_at.wall(),
                venue: Venue::Kalshi,
                channel: String::new(),
                instrument: None,
            });
        }

        let message = KalshiMessage::parse(&raw.text);

        match message {
            KalshiMessage::OrderbookSnapshot(data) => {
                let book = self.books.entry(data.market_ticker.clone()).or_default();
                book.apply_snapshot(&data.yes, &data.no);

                tracing::debug!(
                    market = %data.market_ticker,
                    yes_levels = book.yes_bids.len(),
                    no_levels = book.no_bids.len(),
                    "Kalshi orderbook snapshot applied"
                );

                self.produce_snapshot(&data.market_ticker, raw.received_at)
                    .await;
            }
            KalshiMessage::OrderbookDelta(data) => {
                let book = self.books.entry(data.market_ticker.clone()).or_default();
                book.apply_delta(&data.side, data.price, data.delta);

                // Track last exchange timestamp from delta (best-effort)
                if let Some(ref ts) = data.ts {
                    self.last_exchange_ts
                        .insert(data.market_ticker.clone(), ts.clone());
                }

                tracing::debug!(
                    market = %data.market_ticker,
                    side = %data.side,
                    price = data.price,
                    delta = data.delta,
                    "Kalshi orderbook delta applied"
                );

                self.produce_snapshot(&data.market_ticker, raw.received_at)
                    .await;
            }
            KalshiMessage::Subscribed(data) => {
                tracing::info!(
                    id = data.id,
                    msg = %data.msg,
                    "Kalshi subscription acknowledged"
                );
            }
            KalshiMessage::Error(data) => {
                tracing::error!(
                    code = data.code,
                    msg = %data.msg,
                    "Kalshi error message"
                );
            }
            KalshiMessage::Unknown(raw_text) => {
                let truncated = if raw_text.len() > 200 {
                    format!("{}...", &raw_text[..200])
                } else {
                    raw_text
                };
                tracing::debug!(raw = %truncated, "Kalshi unknown message");
            }
        }
    }

    /// Build and send a MarketSnapshot from current book state.
    ///
    /// Uses the YES contract perspective: bid = best YES bid,
    /// ask = derived from best NO bid (100 - NO bid cents).
    async fn produce_snapshot(&mut self, market_ticker: &str, received_at: DualTimestamp) {
        let book = match self.books.get(market_ticker) {
            Some(b) => b,
            None => return,
        };

        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let inst_id = InstrumentId::new(market_ticker);

        // YES perspective: bid from YES bids, ask derived from NO bids
        let best_yes_bid = book.best_yes_bid();
        let derived_ask_cents = book.best_yes_ask_from_no();

        let bid = best_yes_bid.map(|(cents, _)| Price::new(cents_to_probability(cents)));
        let ask = derived_ask_cents.map(|cents| Price::new(cents_to_probability(cents)));

        let bid_size = best_yes_bid.map(|(_, qty)| Notional::new(Decimal::from(qty)));
        // Ask size comes from the NO bid that generates the derived ask
        let ask_size = book.best_no_bid().map(|(_, qty)| Notional::new(Decimal::from(qty)));

        let bid_probability = best_yes_bid
            .and_then(|(cents, _)| Probability::new(cents_to_probability(cents)).ok());
        let ask_probability = derived_ask_cents
            .and_then(|cents| Probability::new(cents_to_probability(cents)).ok());

        // Depth: YES bids descending, derived asks from NO bids
        let depth_bids: Vec<(Price, Notional)> = book
            .yes_depth_descending()
            .iter()
            .map(|&(cents, qty)| {
                (
                    Price::new(cents_to_probability(cents)),
                    Notional::new(Decimal::from(qty)),
                )
            })
            .collect();

        let depth_asks: Vec<(Price, Notional)> = book
            .no_depth_descending()
            .iter()
            .map(|&(cents, qty)| {
                // Derived ask: 100 - NO bid price
                (
                    Price::new(cents_to_probability(100 - cents)),
                    Notional::new(Decimal::from(qty)),
                )
            })
            .collect();

        // Best-effort exchange timestamp from Kalshi orderbook_delta `ts` field.
        // Protocol limitation: second-precision ISO 8601, not all message types include ts.
        // Snapshots after initial connect may have None until first delta arrives.
        let exchange_ts_ms: Option<i64> =
            self.last_exchange_ts
                .get(market_ticker)
                .and_then(|ts_str| {
                    chrono::DateTime::parse_from_rfc3339(ts_str)
                        .map(|dt| dt.timestamp_millis())
                        .map_err(|e| {
                            tracing::warn!(
                                ts = %ts_str,
                                error = %e,
                                "failed to parse Kalshi exchange timestamp"
                            );
                            e
                        })
                        .ok()
                });

        let is_stale = false;

        // Latency metrics -- only when exchange timestamp available.
        // Note: Kalshi ts is second-precision ISO 8601; latency values have up to ~999ms jitter.
        if let Some(exchange_ts) = exchange_ts_ms {
            let local_ms = received_at.wall().timestamp_millis();
            let latency_ms = (local_ms - exchange_ts) as f64;
            metrics::histogram!("feed_latency_ms", "venue" => "kalshi").record(latency_ms);
            metrics::gauge!("feed_last_latency_ms", "venue" => "kalshi").set(latency_ms);
        }
        metrics::counter!("feed_messages_total", "venue" => "kalshi").increment(1);

        let snapshot = MarketSnapshot {
            venue: Venue::Kalshi,
            instrument_id: inst_id,
            event_id: None,
            bid,
            ask,
            bid_size,
            ask_size,
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
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            underlying_index: None,
            exchange_timestamp: exchange_ts_ms,
            timestamp: received_at,
            sequence: seq,
            trace_id: TraceId::new(),
            is_stale,
        };

        if self.snapshot_tx.send(snapshot).await.is_err() {
            tracing::warn!("Kalshi snapshot receiver dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DualTimestamp {
        DualTimestamp::now()
    }

    #[test]
    fn cents_to_probability_accuracy() {
        assert_eq!(cents_to_probability(42), Decimal::new(42, 2)); // 0.42
        assert_eq!(cents_to_probability(99), Decimal::new(99, 2)); // 0.99
        assert_eq!(cents_to_probability(1), Decimal::new(1, 2)); // 0.01
        assert_eq!(cents_to_probability(50), Decimal::new(50, 2)); // 0.50
    }

    #[tokio::test]
    async fn processor_handles_snapshot() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            KalshiProcessor::new(raw_rx, None, cancel.clone(), 5000, {
                let (_tx, rx) = mpsc::channel::<CleanupEvent>(1);
                rx
            });

        let handle = tokio::spawn(processor.run());

        let snapshot_json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXBTC-26FEB22-T100000",
            "yes": [[42, 100], [45, 200]],
            "no": [[55, 150], [58, 300]]
        }"#;

        raw_tx
            .send(RawMessage {
                text: snapshot_json.to_string(),
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

        assert_eq!(snap.venue, Venue::Kalshi);
        assert_eq!(
            snap.instrument_id,
            InstrumentId::new("KXBTC-26FEB22-T100000")
        );

        // Best YES bid = 45 cents = 0.45 probability
        let bid_prob = snap.bid_probability.unwrap();
        assert_eq!(bid_prob.into_inner(), Decimal::new(45, 2));

        // Derived YES ask from best NO bid: 100 - 58 = 42 cents = 0.42
        let ask_prob = snap.ask_probability.unwrap();
        assert_eq!(ask_prob.into_inner(), Decimal::new(42, 2));

        assert!(snap.greeks.is_none());
        assert!(snap.mark_price.is_none());

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_delta() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            KalshiProcessor::new(raw_rx, None, cancel.clone(), 5000, {
                let (_tx, rx) = mpsc::channel::<CleanupEvent>(1);
                rx
            });

        let handle = tokio::spawn(processor.run());

        // First send a snapshot
        let snapshot_json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXTEST",
            "yes": [[42, 100]],
            "no": [[55, 150]]
        }"#;

        raw_tx
            .send(RawMessage {
                text: snapshot_json.to_string(),
                received_at: ts(),
            })
            .await
            .unwrap();

        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            snapshot_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();

        // Apply a delta that adds a better YES bid
        let delta_json = r#"{
            "type": "orderbook_delta",
            "market_ticker": "KXTEST",
            "price": 48,
            "delta": 300,
            "side": "yes"
        }"#;

        raw_tx
            .send(RawMessage {
                text: delta_json.to_string(),
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

        // Best YES bid should now be 48 cents = 0.48
        let bid_prob = snap.bid_probability.unwrap();
        assert_eq!(bid_prob.into_inner(), Decimal::new(48, 2));

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_handles_subscribed_without_crash() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            KalshiProcessor::new(raw_rx, None, cancel.clone(), 5000, {
                let (_tx, rx) = mpsc::channel::<CleanupEvent>(1);
                rx
            });

        let handle = tokio::spawn(processor.run());

        let sub_json = r#"{"id": 1, "msg": "subscribed"}"#;

        raw_tx
            .send(RawMessage {
                text: sub_json.to_string(),
                received_at: ts(),
            })
            .await
            .unwrap();

        // Subscribed should NOT produce a snapshot
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            snapshot_rx.recv(),
        )
        .await;

        assert!(result.is_err(), "subscribed should not produce a snapshot");

        cancel.cancel();
        handle.await.unwrap();
    }

    #[test]
    fn ask_derivation_from_complementary_side() {
        // YES ask = 100 - best NO bid
        // If best NO bid = 58, YES ask = 42 cents = 0.42
        let ask_cents = 100 - 58;
        assert_eq!(cents_to_probability(ask_cents), Decimal::new(42, 2));
    }

    #[tokio::test]
    async fn processor_propagates_exchange_timestamp() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            KalshiProcessor::new(raw_rx, None, cancel.clone(), 5000, {
                let (_tx, rx) = mpsc::channel::<CleanupEvent>(1);
                rx
            });

        let handle = tokio::spawn(processor.run());

        // Send a snapshot (no ts field on snapshots)
        let snapshot_json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXTEST-TS",
            "yes": [[42, 100]],
            "no": [[55, 150]]
        }"#;

        raw_tx
            .send(RawMessage {
                text: snapshot_json.to_string(),
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

        // Snapshot before any delta: exchange_timestamp should be None
        assert_eq!(snap.exchange_timestamp, None);

        // Send a delta with ts field
        let delta_json = r#"{
            "type": "orderbook_delta",
            "market_ticker": "KXTEST-TS",
            "price": 48,
            "delta": 300,
            "side": "yes",
            "ts": "2024-01-15T10:30:00Z"
        }"#;

        raw_tx
            .send(RawMessage {
                text: delta_json.to_string(),
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

        // Delta with ts: exchange_timestamp should be populated
        // "2024-01-15T10:30:00Z" = 1705314600000 millis
        assert_eq!(snap.exchange_timestamp, Some(1705314600000));

        cancel.cancel();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn processor_no_exchange_ts_on_snapshot_only() {
        let (raw_tx, raw_rx) = mpsc::channel::<RawMessage>(16);
        let cancel = CancellationToken::new();
        let (processor, mut snapshot_rx) =
            KalshiProcessor::new(raw_rx, None, cancel.clone(), 5000, {
                let (_tx, rx) = mpsc::channel::<CleanupEvent>(1);
                rx
            });

        let handle = tokio::spawn(processor.run());

        // Send only a snapshot (no prior delta with ts)
        let snapshot_json = r#"{
            "type": "orderbook_snapshot",
            "market_ticker": "KXTEST-NOTS",
            "yes": [[50, 200]],
            "no": [[60, 100]]
        }"#;

        raw_tx
            .send(RawMessage {
                text: snapshot_json.to_string(),
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

        assert_eq!(snap.exchange_timestamp, None);

        cancel.cancel();
        handle.await.unwrap();
    }
}
