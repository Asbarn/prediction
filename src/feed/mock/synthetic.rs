//! Synthetic data source that generates realistic Deribit-format messages.
//!
//! Produces valid JSON-RPC notification messages (book, ticker, trades) for
//! development and testing without a live Deribit connection or recorded data.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::traits::RawMessage;
use crate::types::DualTimestamp;

/// Buffer size for synthetic message channel.
const SYNTHETIC_BUFFER: usize = 1024;

/// Generates realistic Deribit-format JSON-RPC messages.
///
/// Does NOT need to be perfectly realistic -- the goal is producing valid JSON
/// that the parser, book manager, and processor can handle without errors.
pub struct SyntheticDataSource {
    instruments: Vec<String>,
    interval_ms: u64,
    base_price: f64,
    spread_bps: f64,
    depth_levels: usize,
    cancel: CancellationToken,
}

impl SyntheticDataSource {
    /// Create a new synthetic data source with sensible defaults.
    ///
    /// - `instruments`: List of instrument names to generate data for
    /// - `cancel`: Cancellation token for graceful shutdown
    pub fn new(instruments: Vec<String>, cancel: CancellationToken) -> Self {
        Self {
            instruments,
            interval_ms: 100,
            base_price: 0.05, // Options price, not BTC spot
            spread_bps: 10.0,
            depth_levels: 20,
            cancel,
        }
    }

    /// Set the interval between generated snapshots in milliseconds.
    pub fn with_interval(mut self, ms: u64) -> Self {
        self.interval_ms = ms;
        self
    }

    /// Set the base price for generated order books.
    pub fn with_base_price(mut self, price: f64) -> Self {
        self.base_price = price;
        self
    }
}

impl crate::feed::traits::RawDataSource for SyntheticDataSource {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let instruments = self.instruments.clone();
        let interval_ms = self.interval_ms;
        let base_price = self.base_price;
        let spread_bps = self.spread_bps;
        let depth_levels = self.depth_levels;
        let cancel = self.cancel.clone();

        tracing::info!(
            instruments = ?instruments,
            interval_ms = interval_ms,
            "starting synthetic data generation"
        );

        let (tx, rx) = mpsc::channel::<RawMessage>(SYNTHETIC_BUFFER);

        tokio::spawn(async move {
            let mut rng = StdRng::from_entropy();
            let mut change_ids: std::collections::HashMap<String, i64> =
                instruments.iter().map(|i| (i.clone(), 0)).collect();
            let mut current_prices: std::collections::HashMap<String, f64> =
                instruments.iter().map(|i| (i.clone(), base_price)).collect();
            let mut iteration: u64 = 0;

            loop {
                for instrument in &instruments {
                    let change_id = change_ids.get_mut(instrument).unwrap();
                    let prev_change_id = if *change_id > 0 {
                        Some(*change_id)
                    } else {
                        None
                    };
                    *change_id += 1;
                    let current_id = *change_id;

                    // Random walk the price
                    let price = current_prices.get_mut(instrument).unwrap();
                    let walk = rng.gen_range(-0.001..=0.001);
                    *price = (*price + walk).max(0.0001);
                    let mid = *price;

                    // Generate book snapshot
                    let spread = mid * spread_bps / 10000.0;
                    let best_bid = mid - spread / 2.0;
                    let best_ask = mid + spread / 2.0;

                    let bids: Vec<[f64; 2]> = (0..depth_levels)
                        .map(|level| {
                            let price_offset = level as f64 * spread * 0.5;
                            let level_price = (best_bid - price_offset).max(0.0001);
                            let size = rng.gen_range(1.0..50.0);
                            [level_price, size]
                        })
                        .collect();

                    let asks: Vec<[f64; 2]> = (0..depth_levels)
                        .map(|level| {
                            let price_offset = level as f64 * spread * 0.5;
                            let level_price = best_ask + price_offset;
                            let size = rng.gen_range(1.0..50.0);
                            [level_price, size]
                        })
                        .collect();

                    let now_ms = chrono::Utc::now().timestamp_millis();

                    let book_json = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "subscription",
                        "params": {
                            "channel": format!("book.{}.none.20.100ms", instrument),
                            "data": {
                                "timestamp": now_ms,
                                "instrument_name": instrument,
                                "change_id": current_id,
                                "prev_change_id": prev_change_id,
                                "type": "snapshot",
                                "bids": bids,
                                "asks": asks,
                            }
                        }
                    });

                    let raw = RawMessage {
                        text: book_json.to_string(),
                        received_at: DualTimestamp::now(),
                    };

                    tokio::select! {
                        _ = cancel.cancelled() => {
                            tracing::info!("synthetic data generation cancelled");
                            return;
                        }
                        result = tx.send(raw) => {
                            if result.is_err() {
                                tracing::warn!("synthetic receiver dropped, stopping");
                                return;
                            }
                        }
                    }

                    // Generate ticker
                    let mark_price = mid + rng.gen_range(-0.0005..0.0005);
                    let index_price = 95000.0 + rng.gen_range(-500.0..500.0);

                    let ticker_json = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "subscription",
                        "params": {
                            "channel": format!("ticker.{}.raw", instrument),
                            "data": {
                                "timestamp": now_ms,
                                "instrument_name": instrument,
                                "state": "open",
                                "last_price": mid,
                                "mark_price": mark_price,
                                "index_price": index_price,
                                "best_bid_price": best_bid,
                                "best_bid_amount": bids[0][1],
                                "best_ask_price": best_ask,
                                "best_ask_amount": asks[0][1],
                                "open_interest": rng.gen_range(100.0..5000.0),
                                "min_price": 0.0001,
                                "max_price": 0.5,
                                "mark_iv": rng.gen_range(40.0..90.0),
                                "greeks": {
                                    "delta": rng.gen_range(0.01..0.99),
                                    "gamma": rng.gen_range(0.00001..0.001),
                                    "vega": rng.gen_range(1.0..20.0),
                                    "theta": rng.gen_range(-5.0..-0.01),
                                    "rho": rng.gen_range(0.0001..0.01),
                                },
                                "stats": {
                                    "volume": rng.gen_range(10.0..500.0),
                                    "volume_usd": rng.gen_range(1000.0..50000.0),
                                }
                            }
                        }
                    });

                    let raw = RawMessage {
                        text: ticker_json.to_string(),
                        received_at: DualTimestamp::now(),
                    };

                    tokio::select! {
                        _ = cancel.cancelled() => {
                            tracing::info!("synthetic data generation cancelled");
                            return;
                        }
                        result = tx.send(raw) => {
                            if result.is_err() {
                                tracing::warn!("synthetic receiver dropped, stopping");
                                return;
                            }
                        }
                    }

                    // Occasionally generate trades (~every 10 iterations)
                    if iteration % 10 == 0 {
                        let trade_json = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "subscription",
                            "params": {
                                "channel": format!("trades.{}.raw", instrument),
                                "data": [{
                                    "trade_id": format!("SYNTH_{}", iteration),
                                    "instrument_name": instrument,
                                    "timestamp": now_ms,
                                    "direction": if rng.gen_bool(0.5) { "buy" } else { "sell" },
                                    "price": mid,
                                    "amount": rng.gen_range(0.1..10.0),
                                    "trade_seq": iteration as i64,
                                }]
                            }
                        });

                        let raw = RawMessage {
                            text: trade_json.to_string(),
                            received_at: DualTimestamp::now(),
                        };

                        tokio::select! {
                            _ = cancel.cancelled() => {
                                tracing::info!("synthetic data generation cancelled");
                                return;
                            }
                            result = tx.send(raw) => {
                                if result.is_err() {
                                    tracing::warn!("synthetic receiver dropped, stopping");
                                    return;
                                }
                            }
                        }
                    }
                }

                iteration += 1;

                // Sleep between rounds
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("synthetic data generation cancelled");
                        return;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(interval_ms)) => {}
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::deribit::messages::DeribitMessage;
    use crate::feed::traits::RawDataSource;

    #[tokio::test]
    async fn synthetic_generates_valid_deribit_messages() {
        let cancel = CancellationToken::new();
        let source = SyntheticDataSource::new(
            vec!["BTC-27JUN25-100000-C".to_string()],
            cancel.clone(),
        )
        .with_interval(10); // Fast for testing

        let mut rx = source.start().await.expect("start should succeed");

        // Receive at least 3 messages and verify they parse as valid DeribitMessage
        let mut parsed_count = 0;
        for _ in 0..10 {
            if let Some(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                rx.recv(),
            )
            .await
            .ok()
            .flatten()
            {
                let parsed: Result<DeribitMessage, _> = serde_json::from_str(&msg.text);
                assert!(
                    parsed.is_ok(),
                    "synthetic message should parse as DeribitMessage: {}",
                    msg.text
                );

                // Verify it's a notification (not a response)
                match parsed.unwrap() {
                    DeribitMessage::Notification(notif) => {
                        assert_eq!(notif.method, "subscription");
                        assert!(!notif.params.channel.is_empty());
                    }
                    _ => panic!("synthetic messages should be notifications"),
                }

                parsed_count += 1;
                if parsed_count >= 3 {
                    break;
                }
            }
        }

        assert!(
            parsed_count >= 3,
            "should have received at least 3 valid messages, got {parsed_count}"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn synthetic_produces_book_and_ticker_messages() {
        let cancel = CancellationToken::new();
        let source = SyntheticDataSource::new(
            vec!["TEST-INST".to_string()],
            cancel.clone(),
        )
        .with_interval(10);

        let mut rx = source.start().await.expect("start should succeed");

        let mut has_book = false;
        let mut has_ticker = false;

        for _ in 0..20 {
            if let Some(msg) = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                rx.recv(),
            )
            .await
            .ok()
            .flatten()
            {
                if msg.text.contains("book.TEST-INST.none.20.100ms") {
                    has_book = true;
                }
                if msg.text.contains("ticker.TEST-INST.raw") {
                    has_ticker = true;
                }
                if has_book && has_ticker {
                    break;
                }
            }
        }

        assert!(has_book, "should produce book messages");
        assert!(has_ticker, "should produce ticker messages");

        cancel.cancel();
    }
}
