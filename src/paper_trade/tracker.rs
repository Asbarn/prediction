//! Paper trade tracking engine.
//!
//! Consumes spread signals from the SpreadEngine, enters hypothetical positions
//! at next-tick-after-signal prices (capturing adverse selection), tracks
//! mark-to-market values over position lifetime, and produces daily P&L rollups.
//!
//! Key design: signals queue as Pending positions. On the NEXT MarketSnapshot
//! for the same event, the position is filled at that tick's prices. This models
//! realistic adverse selection -- you can't trade at the price that triggered
//! the signal.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::PaperTradeConfig;
use crate::spread::patterns::SpreadResult;
use crate::types::MarketSnapshot;

use super::aggregator::DailyAggregator;
use super::position::{PaperPosition, PositionStatus};

/// Paper trade tracking engine.
///
/// Manages the lifecycle of hypothetical positions:
/// 1. Signal received -> create Pending position
/// 2. Next tick for same event -> fill at that tick's prices (Open)
/// 3. Track MTM over lifetime
/// 4. Daily P&L rollups
pub struct PaperTradeTracker {
    /// Pending positions awaiting next-tick fill, keyed by event_id.
    pending: HashMap<String, Vec<PaperPosition>>,
    /// Active positions being tracked.
    open: Vec<PaperPosition>,
    /// Configuration.
    config: PaperTradeConfig,
    /// Daily P&L aggregator.
    aggregator: DailyAggregator,
    /// JSONL writer for individual trade events.
    trade_logger: TradeLogger,
    /// Running count of total trades entered.
    total_trades: u64,
}

/// JSONL trade event logger with daily file rotation.
struct TradeLogger {
    log_dir: PathBuf,
    writer: Option<BufWriter<File>>,
    current_date: Option<NaiveDate>,
    writes_since_flush: u64,
}

impl TradeLogger {
    fn new(log_dir: &str) -> Self {
        Self {
            log_dir: PathBuf::from(log_dir),
            writer: None,
            current_date: None,
            writes_since_flush: 0,
        }
    }

    fn log_event(&mut self, event: &TradeEvent) -> anyhow::Result<()> {
        let today = Utc::now().date_naive();

        if self.current_date != Some(today) {
            self.rotate_file(today)?;
        }

        let line = serde_json::to_string(event)?;
        if let Some(ref mut writer) = self.writer {
            writeln!(writer, "{}", line)?;
            self.writes_since_flush += 1;

            if self.writes_since_flush >= 100 {
                writer.flush()?;
                self.writes_since_flush = 0;
            }
        }

        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.flush()?;
            self.writes_since_flush = 0;
        }
        Ok(())
    }

    fn rotate_file(&mut self, date: NaiveDate) -> anyhow::Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.flush()?;
        }

        fs::create_dir_all(&self.log_dir)?;

        let filename = format!("trades-{}.jsonl", date.format("%Y-%m-%d"));
        let path = self.log_dir.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        tracing::info!(path = %path.display(), "opened paper trade log file");

        self.writer = Some(BufWriter::new(file));
        self.current_date = Some(date);
        self.writes_since_flush = 0;

        Ok(())
    }
}

/// Trade event types logged to JSONL.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum TradeEvent {
    /// Signal received, position pending.
    #[serde(rename = "signal")]
    Signal {
        trade_id: String,
        event_id: String,
        pattern: String,
        signal_spread: String,
        notional: String,
        timestamp_ms: i64,
    },
    /// Position filled at next-tick prices.
    #[serde(rename = "entry")]
    Entry {
        trade_id: String,
        event_id: String,
        entry_price_buy: String,
        entry_price_sell: String,
        adverse_selection: String,
        timestamp_ms: i64,
    },
    /// Mark-to-market update.
    #[serde(rename = "mtm")]
    Mtm {
        trade_id: String,
        event_id: String,
        current_spread: String,
        unrealized_pnl: String,
        timestamp_ms: i64,
    },
    /// Position settled.
    #[serde(rename = "settlement")]
    Settlement {
        trade_id: String,
        event_id: String,
        settlement_pnl: String,
        timestamp_ms: i64,
    },
}

impl PaperTradeTracker {
    /// Create a new PaperTradeTracker with the given configuration.
    pub fn new(config: PaperTradeConfig) -> Self {
        let trade_logger = TradeLogger::new(&config.log_dir);
        Self {
            pending: HashMap::new(),
            open: Vec::new(),
            config,
            aggregator: DailyAggregator::new(),
            trade_logger,
            total_trades: 0,
        }
    }

    /// Main event loop: consume signals and snapshots, manage position lifecycle.
    ///
    /// Uses `tokio::select!` with biased selection:
    /// 1. Cancellation token (highest priority)
    /// 2. Daily tick for rollup emission
    /// 3. Signal reception (create Pending positions)
    /// 4. Snapshot reception (fill pending, update MTM)
    pub async fn run(
        mut self,
        mut signal_rx: mpsc::Receiver<SpreadResult>,
        mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
        cancel: CancellationToken,
    ) {
        // Daily tick for rollup emission
        let mut daily_tick = tokio::time::interval(Duration::from_secs(60));
        daily_tick.tick().await; // skip first immediate tick
        let mut last_date = Utc::now().format("%Y-%m-%d").to_string();

        tracing::info!(
            notional = %self.config.notional_per_trade,
            log_mtm = self.config.log_mtm,
            log_dir = %self.config.log_dir,
            "PaperTradeTracker started"
        );

        loop {
            tokio::select! {
                biased;

                _ = cancel.cancelled() => {
                    tracing::info!(
                        total_trades = self.total_trades,
                        open_positions = self.open.len(),
                        "PaperTradeTracker shutting down"
                    );
                    // Emit final daily summary
                    let today = Utc::now().format("%Y-%m-%d").to_string();
                    self.aggregator.emit_daily_summary(&today);
                    let _ = self.trade_logger.flush();
                    break;
                }

                _ = daily_tick.tick() => {
                    let today = Utc::now().format("%Y-%m-%d").to_string();
                    if today != last_date {
                        // Day boundary crossed -- emit yesterday's summary
                        self.aggregator.emit_daily_summary(&last_date);
                        last_date = today;
                    }
                }

                signal = signal_rx.recv() => {
                    match signal {
                        Some(result) => {
                            self.handle_signal(result);
                        }
                        None => {
                            tracing::info!("signal channel closed, PaperTradeTracker stopping");
                            break;
                        }
                    }
                }

                snapshot = snapshot_rx.recv() => {
                    match snapshot {
                        Some(snap) => {
                            self.handle_snapshot(snap);
                        }
                        None => {
                            tracing::info!("snapshot channel closed, PaperTradeTracker stopping");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Handle a new spread signal: create a Pending position.
    fn handle_signal(&mut self, signal: SpreadResult) {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.aggregator.record_signal(&today);

        let position = PaperPosition::new_pending(&signal, self.config.notional_per_trade);
        let trade_id = position.id.clone();
        let event_id = signal.event_id.clone();

        // Log signal event
        if let Err(e) = self.trade_logger.log_event(&TradeEvent::Signal {
            trade_id: trade_id.clone(),
            event_id: event_id.clone(),
            pattern: format!("{:?}", signal.pattern),
            signal_spread: signal.net_spread.to_string(),
            notional: self.config.notional_per_trade.to_string(),
            timestamp_ms: signal.timestamp_ms,
        }) {
            tracing::warn!(error = %e, "failed to log signal event");
        }

        tracing::debug!(
            trade_id = trade_id.as_str(),
            event_id = event_id.as_str(),
            pattern = ?signal.pattern,
            signal_spread = %signal.net_spread,
            "new pending paper trade"
        );

        self.pending
            .entry(event_id)
            .or_default()
            .push(position);

        metrics::counter!("paper_trade_signals_total").increment(1);
    }

    /// Handle a new market snapshot: fill pending positions, update MTM on open positions.
    fn handle_snapshot(&mut self, snap: MarketSnapshot) {
        let event_id = match &snap.event_id {
            Some(eid) => eid.to_string(),
            None => return, // No event mapping, skip
        };

        let now_ms = chrono::Utc::now().timestamp_millis();

        // 1. Check pending positions for this event: fill at this tick's prices
        if let Some(mut pending_positions) = self.pending.remove(&event_id) {
            for pos in &mut pending_positions {
                if pos.status != PositionStatus::Pending {
                    continue;
                }

                // Walk the book on this snapshot for fill prices.
                // For the buy side, walk asks; for sell side, walk bids.
                // Since we're doing a simplified fill using the snapshot's
                // top-of-book probabilities as a proxy for fill price.
                let buy_price = snap
                    .ask_probability
                    .map(|p| p.into_inner())
                    .or_else(|| snap.ask.map(|p| p.into_inner()))
                    .unwrap_or(Decimal::ZERO);

                let sell_price = snap
                    .bid_probability
                    .map(|p| p.into_inner())
                    .or_else(|| snap.bid.map(|p| p.into_inner()))
                    .unwrap_or(Decimal::ZERO);

                if buy_price.is_zero() || sell_price.is_zero() {
                    // Not enough price data to fill
                    continue;
                }

                pos.fill(buy_price, sell_price, now_ms);
                self.total_trades += 1;

                let adverse = pos
                    .adverse_selection
                    .unwrap_or(Decimal::ZERO);

                // Log entry event
                if let Err(e) = self.trade_logger.log_event(&TradeEvent::Entry {
                    trade_id: pos.id.clone(),
                    event_id: event_id.clone(),
                    entry_price_buy: buy_price.to_string(),
                    entry_price_sell: sell_price.to_string(),
                    adverse_selection: adverse.to_string(),
                    timestamp_ms: now_ms,
                }) {
                    tracing::warn!(error = %e, "failed to log entry event");
                }

                tracing::info!(
                    trade_id = pos.id.as_str(),
                    event_id = event_id.as_str(),
                    buy = %buy_price,
                    sell = %sell_price,
                    adverse_selection = %adverse,
                    "paper trade filled at next tick"
                );

                metrics::counter!("paper_trades_total", "event" => event_id.clone())
                    .increment(1);
            }

            // Move filled positions to open, keep unfilled in pending
            let mut still_pending = Vec::new();
            for pos in pending_positions {
                match pos.status {
                    PositionStatus::Open => self.open.push(pos),
                    PositionStatus::Pending => still_pending.push(pos),
                    _ => {}
                }
            }
            if !still_pending.is_empty() {
                self.pending.insert(event_id.clone(), still_pending);
            }
        }

        // 2. Update MTM on open positions matching this event
        let current_spread = match (snap.bid_probability, snap.ask_probability) {
            (Some(bid), Some(ask)) => {
                // Spread proxy: bid - ask of probabilities (simplified)
                bid.into_inner() - ask.into_inner()
            }
            _ => return, // Can't compute MTM without probabilities
        };

        for pos in &mut self.open {
            if pos.event_id == event_id && pos.status == PositionStatus::Open {
                pos.update_mtm(current_spread, now_ms);

                if self.config.log_mtm {
                    if let Some(mtm) = pos.mtm_history.last() {
                        if let Err(e) = self.trade_logger.log_event(&TradeEvent::Mtm {
                            trade_id: pos.id.clone(),
                            event_id: event_id.clone(),
                            current_spread: mtm.current_spread.to_string(),
                            unrealized_pnl: mtm.unrealized_pnl.to_string(),
                            timestamp_ms: now_ms,
                        }) {
                            tracing::warn!(error = %e, "failed to log MTM event");
                        }
                    }
                }
            }
        }

        // Update open positions gauge
        metrics::gauge!("paper_trades_open").set(self.open.len() as f64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spread::patterns::SpreadPattern;
    use crate::types::{
        DualTimestamp, EventId, InstrumentId, Probability, TraceId, Venue,
    };
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn prob(s: &str) -> Probability {
        Probability::new(dec(s)).unwrap()
    }

    fn make_config() -> PaperTradeConfig {
        PaperTradeConfig {
            notional_per_trade: dec("500"),
            log_mtm: true,
            log_dir: std::env::temp_dir()
                .join("paper_trade_test")
                .to_str()
                .unwrap()
                .to_string(),
        }
    }

    fn make_signal(event_id: &str, net_spread: &str) -> SpreadResult {
        SpreadResult {
            event_id: event_id.to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            gross_spread: dec("0.05"),
            net_spread: dec(net_spread),
            buy_fill_price: dec("0.45"),
            sell_fill_price: dec("0.50"),
            buy_fee: dec("0.005"),
            sell_fee: dec("0.007"),
            carry_cost: dec("0.002"),
            total_cost: dec("0.014"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("0.95"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: None,
            kalshi_exchange_ts: None,
            threshold: None,
            threshold_components: None,
        }
    }

    fn make_snapshot(event_id: &str, bid_prob: &str, ask_prob: &str) -> MarketSnapshot {
        MarketSnapshot {
            venue: Venue::Polymarket,
            instrument_id: InstrumentId::new("TEST-INST"),
            event_id: Some(EventId::new(event_id)),
            bid: None,
            ask: None,
            bid_size: None,
            ask_size: None,
            depth_bids: vec![],
            depth_asks: vec![],
            bid_probability: Some(prob(bid_prob)),
            ask_probability: Some(prob(ask_prob)),
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
            exchange_timestamp: Some(chrono::Utc::now().timestamp_millis()),
            timestamp: DualTimestamp::now(),
            sequence: 1,
            trace_id: TraceId::new(),
            is_stale: false,
        }
    }

    #[test]
    fn pending_fills_on_next_tick() {
        let config = make_config();
        let mut tracker = PaperTradeTracker::new(config);

        // Signal arrives
        let signal = make_signal("evt-001", "0.03");
        tracker.handle_signal(signal);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.open.is_empty());

        // Next tick for same event
        let snap = make_snapshot("evt-001", "0.48", "0.52");
        tracker.handle_snapshot(snap);

        // Position should now be open
        assert!(tracker.pending.is_empty() || tracker.pending.get("evt-001").is_none());
        assert_eq!(tracker.open.len(), 1);
        assert_eq!(tracker.open[0].status, PositionStatus::Open);
        // Buy price = ask_probability = 0.52, sell price = bid_probability = 0.48
        assert_eq!(tracker.open[0].entry_price_buy, Some(dec("0.52")));
        assert_eq!(tracker.open[0].entry_price_sell, Some(dec("0.48")));
    }

    #[test]
    fn mtm_updates_on_subsequent_snapshots() {
        let config = make_config();
        let mut tracker = PaperTradeTracker::new(config);

        // Signal + fill (fill snapshot also generates 1 MTM data point)
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));
        assert_eq!(tracker.open.len(), 1);
        assert_eq!(tracker.open[0].mtm_history.len(), 1); // fill snapshot MTM

        // Subsequent MTM updates accumulate
        tracker.handle_snapshot(make_snapshot("evt-001", "0.50", "0.54"));
        assert_eq!(tracker.open[0].mtm_history.len(), 2);

        tracker.handle_snapshot(make_snapshot("evt-001", "0.46", "0.50"));
        assert_eq!(tracker.open[0].mtm_history.len(), 3);
    }

    #[test]
    fn unrelated_event_snapshots_ignored() {
        let config = make_config();
        let mut tracker = PaperTradeTracker::new(config);

        tracker.handle_signal(make_signal("evt-001", "0.03"));

        // Snapshot for different event
        let snap = make_snapshot("evt-999", "0.50", "0.54");
        tracker.handle_snapshot(snap);

        // Pending should still be there, unfilled
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.open.is_empty());
    }

    #[test]
    fn multiple_signals_same_event_all_fill() {
        let config = make_config();
        let mut tracker = PaperTradeTracker::new(config);

        // Two signals for same event
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_signal(make_signal("evt-001", "0.04"));

        assert_eq!(
            tracker.pending.get("evt-001").map(|v| v.len()),
            Some(2)
        );

        // Single tick fills both
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));
        assert_eq!(tracker.open.len(), 2);
    }

    // Cleanup temp dir after tests
    fn cleanup_test_dir() {
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join("paper_trade_test"),
        );
    }

    #[test]
    fn cleanup() {
        cleanup_test_dir();
    }
}
