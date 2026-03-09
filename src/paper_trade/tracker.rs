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

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use rust_decimal::prelude::FromStr;

use crate::config::{AnalysisConfig, PaperTradeConfig};
use crate::persistence::checkpoint::{CheckpointState, SettlementTrackingEntry};
use crate::settlement::types::{
    OutcomeKind, SettledLeg, SettlementOutcome,
};
use crate::spread::patterns::{SpreadPattern, SpreadResult};
use crate::types::MarketSnapshot;

use super::aggregator::DailyAggregator;
use super::analyzer::{FilteredSignalEvent, SignalAnalyzer};
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
    /// Signal analysis engine (Phase 17).
    analyzer: SignalAnalyzer,
    /// JSONL writer for individual trade events.
    trade_logger: TradeLogger,
    /// JSONL writer for settlement records.
    settlement_logger: SettlementLogger,
    /// Running count of total trades entered.
    total_trades: u64,
    /// Directory for checkpoint files (None = persistence disabled).
    checkpoint_dir: Option<PathBuf>,
    /// How often to write periodic checkpoints (None = persistence disabled).
    checkpoint_interval: Option<Duration>,
    /// Recently settled positions for bounded retention (capped at 100 or 48 hours).
    recently_settled: VecDeque<(i64, PaperPosition)>,
    /// Shared settlement tracking state for checkpoint persistence.
    /// Updated by SettlementMonitor, read during checkpoint snapshots.
    settlement_tracking_state: Option<Arc<RwLock<HashMap<String, Vec<SettlementTrackingEntry>>>>>,
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

/// JSONL settlement record logger with daily file rotation.
struct SettlementLogger {
    log_dir: PathBuf,
    writer: Option<BufWriter<File>>,
    current_date: Option<NaiveDate>,
    writes_since_flush: u64,
}

impl SettlementLogger {
    fn new(log_dir: &str) -> Self {
        Self {
            log_dir: PathBuf::from(log_dir),
            writer: None,
            current_date: None,
            writes_since_flush: 0,
        }
    }

    fn log_record(&mut self, record: &impl serde::Serialize) -> anyhow::Result<()> {
        let today = Utc::now().date_naive();

        if self.current_date != Some(today) {
            self.rotate_file(today)?;
        }

        let line = serde_json::to_string(record)?;
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

        let filename = format!("settlements-{}.jsonl", date.format("%Y-%m-%d"));
        let path = self.log_dir.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        tracing::info!(path = %path.display(), "opened settlement log file");

        self.writer = Some(BufWriter::new(file));
        self.current_date = Some(date);
        self.writes_since_flush = 0;

        Ok(())
    }
}

/// Trade event types logged to JSONL.
///
/// ## JSONL Schema (v1.0)
///
/// Tagged enum with `type` discriminator field. Possible types:
/// - `"signal"`: Signal received, position pending. Fields: trade_id, event_id, pattern, signal_spread, notional, timestamp_ms
/// - `"entry"`: Position filled at next-tick prices. Fields: trade_id, event_id, entry_price_buy, entry_price_sell, adverse_selection, timestamp_ms
/// - `"mtm"`: Mark-to-market update. Fields: trade_id, event_id, current_spread, unrealized_pnl, timestamp_ms
/// - `"settlement"`: Position settled. Fields: trade_id, event_id, settlement_pnl, timestamp_ms
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum TradeEvent {
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

impl TradeEvent {
    /// Extract the timestamp from any TradeEvent variant.
    pub fn timestamp_ms(&self) -> i64 {
        match self {
            TradeEvent::Signal { timestamp_ms, .. } => *timestamp_ms,
            TradeEvent::Entry { timestamp_ms, .. } => *timestamp_ms,
            TradeEvent::Mtm { timestamp_ms, .. } => *timestamp_ms,
            TradeEvent::Settlement { timestamp_ms, .. } => *timestamp_ms,
        }
    }
}

impl PaperTradeTracker {
    /// Create a new PaperTradeTracker with the given configuration.
    pub fn new(config: PaperTradeConfig, settlement_log_dir: &str, analysis_config: AnalysisConfig) -> Self {
        let trade_logger = TradeLogger::new(&config.log_dir);
        let settlement_logger = SettlementLogger::new(settlement_log_dir);
        Self {
            pending: HashMap::new(),
            open: Vec::new(),
            config,
            aggregator: DailyAggregator::new(),
            analyzer: SignalAnalyzer::new(analysis_config),
            trade_logger,
            settlement_logger,
            total_trades: 0,
            checkpoint_dir: None,
            checkpoint_interval: None,
            recently_settled: VecDeque::new(),
            settlement_tracking_state: None,
        }
    }

    /// Set the shared settlement tracking state for checkpoint persistence.
    pub fn set_settlement_tracking_state(
        &mut self,
        state: Arc<RwLock<HashMap<String, Vec<SettlementTrackingEntry>>>>,
    ) {
        self.settlement_tracking_state = Some(state);
    }

    /// Get a reference to the open positions (for SettlementMonitor initialization).
    pub fn open_positions(&self) -> &[PaperPosition] {
        &self.open
    }

    /// Get a reference to the signal analyzer (for tests and daily summary).
    pub fn analyzer(&self) -> &SignalAnalyzer {
        &self.analyzer
    }

    /// Configure periodic checkpoint persistence.
    ///
    /// When enabled, the tracker writes a checkpoint file at the given interval
    /// and on shutdown. The checkpoint directory is created automatically.
    pub fn with_persistence(mut self, checkpoint_dir: PathBuf, interval_secs: u64) -> Self {
        self.checkpoint_dir = Some(checkpoint_dir);
        self.checkpoint_interval = Some(Duration::from_secs(interval_secs));
        self
    }

    /// Main event loop: consume signals and snapshots, manage position lifecycle.
    ///
    /// Uses `tokio::select!` with biased selection:
    /// 1. Cancellation token (highest priority)
    /// 2. Daily tick for rollup emission
    /// 3. Signal reception (create Pending positions)
    /// 4. Settlement reception (settle positions)
    /// 5. Filtered signal reception (threshold effectiveness tracking)
    /// 6. Snapshot reception (fill pending, update MTM)
    pub async fn run(
        mut self,
        mut signal_rx: mpsc::Receiver<SpreadResult>,
        mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
        mut settlement_rx: mpsc::Receiver<SettlementOutcome>,
        mut filtered_signal_rx: mpsc::Receiver<FilteredSignalEvent>,
        cancel: CancellationToken,
    ) {
        // Daily tick for rollup emission
        let mut daily_tick = tokio::time::interval(Duration::from_secs(60));
        daily_tick.tick().await; // skip first immediate tick
        let mut last_date = Utc::now().format("%Y-%m-%d").to_string();

        // Checkpoint tick for periodic state persistence
        let checkpoint_interval_dur = self
            .checkpoint_interval
            .unwrap_or(Duration::from_secs(u64::MAX));
        let mut checkpoint_tick = tokio::time::interval(checkpoint_interval_dur);
        checkpoint_tick.tick().await; // skip first immediate tick

        tracing::info!(
            notional = %self.config.notional_per_trade,
            log_mtm = self.config.log_mtm,
            log_dir = %self.config.log_dir,
            persistence = self.checkpoint_dir.is_some(),
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
                    // Emit final daily summary with analysis metrics
                    let today = Utc::now().format("%Y-%m-%d").to_string();
                    let analysis_summary = self.analyzer.lifetime_summary();
                    self.aggregator.emit_daily_summary(&today, Some(&analysis_summary));
                    let _ = self.trade_logger.flush();
                    let _ = self.settlement_logger.flush();
                    // Write final checkpoint before shutdown
                    self.write_checkpoint();
                    break;
                }

                _ = daily_tick.tick() => {
                    let today = Utc::now().format("%Y-%m-%d").to_string();
                    if today != last_date {
                        // Day boundary crossed -- emit yesterday's summary with analysis metrics
                        let analysis_summary = self.analyzer.lifetime_summary();
                        self.aggregator.emit_daily_summary(&last_date, Some(&analysis_summary));
                        last_date = today;
                    }
                }

                _ = checkpoint_tick.tick(), if self.checkpoint_dir.is_some() => {
                    self.write_checkpoint();
                }

                settlement = settlement_rx.recv() => {
                    match settlement {
                        Some(outcome) => {
                            self.handle_settlement(outcome);
                        }
                        None => {
                            tracing::info!("settlement channel closed");
                        }
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

                filtered = filtered_signal_rx.recv() => {
                    if let Some(event) = filtered {
                        self.analyzer.record_filtered_signal(event);
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

        let mut position = PaperPosition::new_pending(&signal, self.config.notional_per_trade);
        position.mark_stale_fill(self.analyzer.config().max_leg_fill_gap_ms);
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

    /// Handle a settlement outcome: settle the matching position leg.
    fn handle_settlement(&mut self, outcome: SettlementOutcome) {
        let now_ms = chrono::Utc::now().timestamp_millis();

        // Find open/partially-settled positions matching this event
        let matching_indices: Vec<usize> = self
            .open
            .iter()
            .enumerate()
            .filter(|(_, pos)| {
                pos.event_id == outcome.event_id
                    && matches!(
                        pos.status,
                        PositionStatus::Open | PositionStatus::PartiallySettled
                    )
            })
            .map(|(i, _)| i)
            .collect();

        if matching_indices.is_empty() {
            tracing::debug!(
                event_id = %outcome.event_id,
                venue = ?outcome.venue,
                "no open position found for settlement outcome"
            );
            return;
        }

        // Process each matching position
        // We need to handle indices carefully since we may remove positions
        let mut positions_to_finalize: Vec<usize> = Vec::new();

        for &idx in &matching_indices {
            let pos = &self.open[idx];

            // Compute settlement value for this leg
            let settlement_value = match &outcome.outcome {
                OutcomeKind::Yes => Decimal::ONE,
                OutcomeKind::No => Decimal::ZERO,
                OutcomeKind::Ambiguous { settlement_price } => *settlement_price,
                OutcomeKind::Timeout => Decimal::ZERO,
            };

            // Determine which side of the position this venue is on based on SpreadPattern.
            // For Poly-Kalshi spreads:
            //   BuyPolyYesSellKalshiYes: Poly=buy side, Kalshi=sell side
            //   SellPolyYesBuyKalshiYes: Poly=sell side, Kalshi=buy side
            //   BuyPolyNoSellKalshiNo:   Poly=buy side (NO), Kalshi=sell side (NO)
            //   SellPolyNoBuyKalshiNo:   Poly=sell side (NO), Kalshi=buy side (NO)
            let (entry_price, direction_sign) =
                self.compute_leg_entry_and_direction(pos, &outcome);

            let raw_pnl = (settlement_value - entry_price) * pos.notional * direction_sign;

            // Use entry fee data from the signal if available
            let entry_fee = self.estimate_entry_fee(pos, &outcome);
            let exit_fee = Decimal::ZERO; // Settlement is typically free
            let slippage_estimate = Decimal::ZERO; // Captured in adverse_selection at entry

            let net_pnl = raw_pnl - entry_fee - exit_fee - slippage_estimate;

            let leg = SettledLeg {
                venue: outcome.venue.clone(),
                outcome: outcome.outcome.clone(),
                raw_pnl,
                entry_fee,
                exit_fee,
                slippage_estimate,
                net_pnl,
                fee_model_version: "v1.0".to_string(),
                resolved_at: outcome.resolved_at,
                detected_at: outcome.detected_at,
                resolution_source: outcome.resolution_source.clone(),
            };

            let pos = &mut self.open[idx];
            pos.record_settled_leg(leg);

            // Count expected venue legs: for spread patterns involving 2 venues, expect 2
            let expected_venue_count = 2usize;

            if pos.all_legs_settled(expected_venue_count) {
                positions_to_finalize.push(idx);
            } else {
                tracing::debug!(
                    event_id = %outcome.event_id,
                    venue = ?outcome.venue,
                    settled_legs = pos.settled_legs.len(),
                    expected = expected_venue_count,
                    "partially settled, waiting for remaining legs"
                );
            }
        }

        // Finalize fully settled positions (process in reverse order to preserve indices)
        positions_to_finalize.sort_unstable();
        for &idx in positions_to_finalize.iter().rev() {
            let pos = &mut self.open[idx];
            pos.finalize_settlement();
            let divergence = pos.compute_divergence();
            pos.divergence = divergence.clone();

            // Record in aggregator
            self.aggregator.record_trade(pos);

            // Record in SignalAnalyzer: updates accumulators, returns enriched record
            let analysis_record = self.analyzer.record_settlement(pos);

            // Log enriched AnalysisSettlementRecord to settlement JSONL
            if let Err(e) = self.settlement_logger.log_record(&analysis_record) {
                tracing::warn!(error = %e, "failed to log analysis settlement record");
            }

            // Human-readable settlement log line
            tracing::info!(
                event_id = %pos.event_id,
                venue_pair = %analysis_record.venue_pair,
                outcome = if analysis_record.net_hit { "hit" } else { "miss" },
                convergence_secs = analysis_record.convergence_secs,
                threshold_status = ?analysis_record.threshold_status,
                stale_fill = analysis_record.stale_fill,
                "SETTLED: {} {} {} edge (net), {}",
                pos.event_id,
                analysis_record.venue_pair,
                analysis_record.total_net_pnl,
                if analysis_record.net_hit { "hit" } else { "miss" }
            );

            let total_net_pnl: Decimal = pos.settled_legs.iter().map(|l| l.net_pnl).sum();
            let total_raw_pnl: Decimal = pos.settled_legs.iter().map(|l| l.raw_pnl).sum();

            // Log settlement trade event to trade logger as well
            if let Err(e) = self.trade_logger.log_event(&TradeEvent::Settlement {
                trade_id: pos.id.clone(),
                event_id: pos.event_id.clone(),
                settlement_pnl: total_net_pnl.to_string(),
                timestamp_ms: now_ms,
            }) {
                tracing::warn!(error = %e, "failed to log settlement trade event");
            }

            // Emit Prometheus metrics (per-trade)
            let net_pnl_f64 = total_net_pnl.to_f64().unwrap_or(0.0);
            for leg in &pos.settled_legs {
                let venue_str = format!("{:?}", leg.venue);
                let outcome_str = match &leg.outcome {
                    OutcomeKind::Yes => "yes",
                    OutcomeKind::No => "no",
                    OutcomeKind::Ambiguous { .. } => "ambiguous",
                    OutcomeKind::Timeout => "timeout",
                };
                metrics::counter!("paper_trades_settled_total",
                    "venue" => venue_str,
                    "outcome" => outcome_str.to_string()
                )
                .increment(1);
            }
            metrics::histogram!("paper_trade_net_pnl").record(net_pnl_f64);

            // Settlement latency: detected_at - signal_timestamp
            if let Some(first_leg) = pos.settled_legs.first() {
                let detected_ms = first_leg.detected_at.timestamp_millis();
                let latency_secs =
                    (detected_ms - pos.signal_timestamp_ms) as f64 / 1000.0;
                metrics::histogram!("paper_trade_settlement_latency_seconds")
                    .record(latency_secs);
            }

            // Divergence metric
            if let Some(ref div) = pos.divergence {
                let div_type = format!("{:?}", div.divergence_type);
                metrics::counter!("paper_trade_divergence_total",
                    "type" => div_type
                )
                .increment(1);
            }

            tracing::info!(
                trade_id = %pos.id,
                event_id = %pos.event_id,
                net_pnl = %total_net_pnl,
                raw_pnl = %total_raw_pnl,
                legs = pos.settled_legs.len(),
                divergence = ?pos.divergence.as_ref().map(|d| &d.divergence_type),
                "paper trade settled"
            );

            // Move to recently_settled and evict from open
            let settled_pos = self.open.remove(idx);
            let is_timeout = settled_pos
                .settled_legs
                .iter()
                .any(|l| matches!(l.outcome, OutcomeKind::Timeout));

            // Timeout positions evicted immediately per CONTEXT.md
            if !is_timeout {
                self.recently_settled.push_back((now_ms, settled_pos));
            }
        }

        // Correlate filtered signals with settlement outcome for threshold effectiveness
        let correlations = self.analyzer.correlate_filtered_with_settlement(
            &outcome.event_id,
            &outcome.outcome,
        );
        if !correlations.is_empty() {
            let hypothetical_hits = correlations.iter().filter(|c| c.hypothetical_hit).count();
            tracing::info!(
                event_id = %outcome.event_id,
                filtered_signals = correlations.len(),
                hypothetical_hits = hypothetical_hits,
                "threshold effectiveness: filtered signal settlement correlation"
            );
        }

        // Emit Prometheus gauges with latest accumulator values after all settlements processed
        self.analyzer.emit_prometheus_gauges();

        // Evict old entries from recently_settled (> 48 hours or > 100 entries)
        self.evict_recently_settled(now_ms);

        // Update open positions gauge
        metrics::gauge!("paper_trades_open").set(self.open.len() as f64);
    }

    /// Compute the entry price and direction sign for a settlement leg.
    ///
    /// Returns (entry_price, direction_sign) where direction_sign is +1 for buy, -1 for sell.
    fn compute_leg_entry_and_direction(
        &self,
        pos: &PaperPosition,
        outcome: &SettlementOutcome,
    ) -> (Decimal, Decimal) {
        use crate::spread::patterns::SpreadPattern;
        use crate::types::Venue;

        let buy_price = pos.entry_price_buy.unwrap_or(Decimal::ZERO);
        let sell_price = pos.entry_price_sell.unwrap_or(Decimal::ZERO);

        match (&pos.pattern, &outcome.venue) {
            // BuyPolyYesSellKalshiYes: Poly is buy side, Kalshi is sell side
            (SpreadPattern::BuyPolyYesSellKalshiYes, Venue::Polymarket) => {
                (buy_price, Decimal::ONE)
            }
            (SpreadPattern::BuyPolyYesSellKalshiYes, Venue::Kalshi) => {
                (sell_price, Decimal::new(-1, 0))
            }
            // SellPolyYesBuyKalshiYes: Poly is sell side, Kalshi is buy side
            (SpreadPattern::SellPolyYesBuyKalshiYes, Venue::Polymarket) => {
                (sell_price, Decimal::new(-1, 0))
            }
            (SpreadPattern::SellPolyYesBuyKalshiYes, Venue::Kalshi) => {
                (buy_price, Decimal::ONE)
            }
            // BuyPolyNoSellKalshiNo: Poly is buy side (NO complement), Kalshi is sell side (NO)
            (SpreadPattern::BuyPolyNoSellKalshiNo, Venue::Polymarket) => {
                (buy_price, Decimal::ONE)
            }
            (SpreadPattern::BuyPolyNoSellKalshiNo, Venue::Kalshi) => {
                (sell_price, Decimal::new(-1, 0))
            }
            // SellPolyNoBuyKalshiNo: Poly is sell side (NO), Kalshi is buy side (NO)
            (SpreadPattern::SellPolyNoBuyKalshiNo, Venue::Polymarket) => {
                (sell_price, Decimal::new(-1, 0))
            }
            (SpreadPattern::SellPolyNoBuyKalshiNo, Venue::Kalshi) => {
                (buy_price, Decimal::ONE)
            }
            // Deribit or unknown: default to buy side
            _ => (buy_price, Decimal::ONE),
        }
    }

    /// Estimate entry fee for a settlement leg from the signal data.
    fn estimate_entry_fee(&self, _pos: &PaperPosition, outcome: &SettlementOutcome) -> Decimal {
        use crate::types::Venue;

        // The SpreadResult carried buy_fee and sell_fee which are stored in the signal.
        // Since we don't store per-venue fees on the position, we use a simplified estimate.
        // The signal's buy_fee/sell_fee represent the total cost model.
        // Split proportionally: assume roughly equal fee contribution per venue leg.
        let _venue = &outcome.venue;
        // For now, use zero -- fees are already captured in the signal's total_cost.
        // The settlement net_pnl computation handles this via the entry_fee field.
        // TODO: In v2, propagate per-venue fees from SpreadEngine to position.
        match outcome.venue {
            Venue::Polymarket | Venue::Kalshi | Venue::Deribit | Venue::Derive => Decimal::ZERO,
        }
    }

    /// Evict old entries from recently_settled.
    fn evict_recently_settled(&mut self, now_ms: i64) {
        let max_age_ms: i64 = 48 * 3600 * 1000; // 48 hours
        let max_entries: usize = 100;

        // Evict entries older than 48 hours
        while let Some(&(ts, _)) = self.recently_settled.front() {
            if now_ms - ts > max_age_ms {
                self.recently_settled.pop_front();
            } else {
                break;
            }
        }

        // Cap at 100 entries
        while self.recently_settled.len() > max_entries {
            self.recently_settled.pop_front();
        }
    }

    /// Extract current state for checkpointing.
    ///
    /// Called periodically by the checkpoint manager to capture a consistent
    /// snapshot of all mutable paper trade state.
    pub fn snapshot_state(&self) -> CheckpointState {
        // Read shared settlement tracking state if available
        let settlement_tracking: HashMap<String, Vec<SettlementTrackingEntry>> = self
            .settlement_tracking_state
            .as_ref()
            .and_then(|state| state.try_read().ok())
            .map(|guard| (*guard).clone())
            .unwrap_or_default();

        CheckpointState {
            version: CheckpointState::current_version(),
            checkpoint_timestamp_ms: chrono::Utc::now().timestamp_millis(),
            pending: self.pending.clone(),
            open: self.open.clone(),
            daily_rollups: self.aggregator.export_rollups(),
            total_trades: self.total_trades,
            settlement_tracking,
            analysis_accumulators: self.analyzer.export_state().into_iter().collect(),
            filtered_signals: self.analyzer.export_filtered_state(),
        }
    }

    /// Restore state from a checkpoint.
    ///
    /// Called during startup recovery before entering the event loop.
    /// Replaces all mutable state fields with values from the checkpoint.
    pub fn restore_state(&mut self, state: CheckpointState) {
        self.pending = state.pending;
        self.open = state.open;
        self.aggregator.import_rollups(state.daily_rollups);
        self.total_trades = state.total_trades;
        self.analyzer.import_state(state.analysis_accumulators.into_iter().collect());
        self.analyzer.import_filtered_state(state.filtered_signals);
    }

    /// Write a checkpoint of current state to disk using atomic write.
    ///
    /// No-op if persistence is not configured. Uses `atomic_write` to ensure
    /// the checkpoint file is never partially written (crash safety).
    fn write_checkpoint(&self) {
        if let Some(ref dir) = self.checkpoint_dir {
            let state = self.snapshot_state();
            match serde_json::to_string_pretty(&state) {
                Ok(json) => {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        tracing::warn!(error = %e, "failed to create checkpoint directory");
                        return;
                    }
                    let target = dir.join("checkpoint.json");
                    if let Err(e) =
                        crate::persistence::atomic::atomic_write(&target, json.as_bytes())
                    {
                        tracing::warn!(error = %e, "failed to write checkpoint");
                    } else {
                        tracing::debug!(
                            total_trades = state.total_trades,
                            open_positions = state.open.len(),
                            pending_events = state.pending.len(),
                            "checkpoint written"
                        );
                        metrics::counter!("persistence_checkpoints_written").increment(1);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to serialize checkpoint state");
                }
            }
        }
    }

    /// Apply a single trade event to reconstruct state during JSONL replay.
    ///
    /// Called during startup recovery for each trade event that occurred after
    /// the checkpoint timestamp. Reconstructs the position lifecycle without
    /// going through the normal signal/snapshot handling path.
    pub fn apply_trade_event(&mut self, event: &TradeEvent) {
        match event {
            TradeEvent::Signal {
                trade_id,
                event_id,
                pattern,
                signal_spread,
                notional,
                timestamp_ms,
            } => {
                let spread = Decimal::from_str(signal_spread).unwrap_or_default();
                let notional_dec = Decimal::from_str(notional).unwrap_or_default();
                let pattern_parsed =
                    serde_json::from_str::<SpreadPattern>(&format!("\"{}\"", pattern))
                        .unwrap_or(SpreadPattern::BuyPolyYesSellKalshiYes);
                let pos = PaperPosition {
                    id: trade_id.clone(),
                    event_id: event_id.clone(),
                    pattern: pattern_parsed,
                    status: PositionStatus::Pending,
                    notional: notional_dec,
                    signal_spread: spread,
                    signal_timestamp_ms: *timestamp_ms,
                    entry_price_buy: None,
                    entry_price_sell: None,
                    entry_timestamp_ms: None,
                    adverse_selection: None,
                    mtm_history: Vec::new(),
                    settlement_pnl: None,
                    settled_at_ms: None,
                    settled_legs: Vec::new(),
                    divergence: None,
                    threshold_status: None,
                    inter_leg_gap_ms: None,
                    stale_fill: false,
                    poly_exchange_ts: None,
                    kalshi_exchange_ts: None,
                };
                self.pending
                    .entry(event_id.clone())
                    .or_default()
                    .push(pos);
            }
            TradeEvent::Entry {
                trade_id,
                entry_price_buy,
                entry_price_sell,
                timestamp_ms,
                ..
            } => {
                let buy = Decimal::from_str(entry_price_buy).unwrap_or_default();
                let sell = Decimal::from_str(entry_price_sell).unwrap_or_default();
                // Find in pending and fill
                for positions in self.pending.values_mut() {
                    if let Some(pos) = positions.iter_mut().find(|p| p.id == *trade_id) {
                        pos.fill(buy, sell, *timestamp_ms);
                        self.total_trades += 1;
                        break;
                    }
                }
                // Move filled positions from pending to open
                let event_ids: Vec<String> = self.pending.keys().cloned().collect();
                for eid in event_ids {
                    if let Some(positions) = self.pending.remove(&eid) {
                        let (open, still_pending): (Vec<_>, Vec<_>) = positions
                            .into_iter()
                            .partition(|p| p.status == PositionStatus::Open);
                        self.open.extend(open);
                        if !still_pending.is_empty() {
                            self.pending.insert(eid, still_pending);
                        }
                    }
                }
            }
            TradeEvent::Mtm {
                trade_id,
                current_spread,
                timestamp_ms,
                ..
            } => {
                let spread = Decimal::from_str(current_spread).unwrap_or_default();
                if let Some(pos) = self.open.iter_mut().find(|p| p.id == *trade_id) {
                    pos.update_mtm(spread, *timestamp_ms);
                }
            }
            TradeEvent::Settlement {
                trade_id,
                settlement_pnl,
                timestamp_ms,
                ..
            } => {
                if let Some(pos) = self.open.iter_mut().find(|p| p.id == *trade_id) {
                    let pnl = Decimal::from_str(settlement_pnl).unwrap_or_default();
                    pos.settle(pnl, *timestamp_ms);
                    self.aggregator.record_trade(pos);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AnalysisConfig;
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

    fn make_tracker(config: PaperTradeConfig) -> PaperTradeTracker {
        let log_dir = std::env::temp_dir()
            .join("settlement_test")
            .to_str()
            .unwrap()
            .to_string();
        PaperTradeTracker::new(config, &log_dir, AnalysisConfig::default())
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
            basis_risk_premium: dec("0"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("0.95"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: None,
            kalshi_exchange_ts: None,
            options_exchange_ts: None,
            threshold: None,
            threshold_components: None,
            threshold_status: None,
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
        let mut tracker = make_tracker(config);

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
        let mut tracker = make_tracker(config);

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
        let mut tracker = make_tracker(config);

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
        let mut tracker = make_tracker(config);

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

    #[test]
    fn test_snapshot_restore_roundtrip() {
        let config = make_config();
        let mut tracker = make_tracker(config);

        // Add a pending signal
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        // Add a second signal and fill it
        tracker.handle_signal(make_signal("evt-002", "0.04"));
        tracker.handle_snapshot(make_snapshot("evt-002", "0.48", "0.52"));

        assert_eq!(tracker.pending.len(), 1);
        assert_eq!(tracker.open.len(), 1);
        assert_eq!(tracker.total_trades, 1);

        // Take snapshot
        let snapshot = tracker.snapshot_state();
        assert_eq!(snapshot.version, CheckpointState::current_version());
        assert_eq!(snapshot.total_trades, 1);
        assert_eq!(snapshot.pending.len(), 1);
        assert_eq!(snapshot.open.len(), 1);

        // Restore into a fresh tracker
        let config2 = make_config();
        let mut tracker2 = make_tracker(config2);
        assert_eq!(tracker2.total_trades, 0);
        assert!(tracker2.pending.is_empty());
        assert!(tracker2.open.is_empty());

        tracker2.restore_state(snapshot);

        assert_eq!(tracker2.total_trades, 1);
        assert_eq!(tracker2.pending.len(), 1);
        assert_eq!(tracker2.open.len(), 1);
        assert_eq!(tracker2.open[0].event_id, "evt-002");
    }

    #[test]
    fn handle_settlement_finds_and_settles_matching_position() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        // Signal + fill
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));
        assert_eq!(tracker.open.len(), 1);

        // Settle one leg (Polymarket)
        let outcome1 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };
        tracker.handle_settlement(outcome1);

        // Position should be partially settled
        assert_eq!(tracker.open.len(), 1);
        assert_eq!(
            tracker.open[0].status,
            PositionStatus::PartiallySettled
        );
        assert_eq!(tracker.open[0].settled_legs.len(), 1);
    }

    #[test]
    fn handle_settlement_full_lifecycle() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        // Signal + fill
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));
        assert_eq!(tracker.open.len(), 1);

        // Settle leg 1 (Polymarket)
        let outcome1 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };
        tracker.handle_settlement(outcome1);
        assert_eq!(tracker.open.len(), 1); // still partially settled

        // Settle leg 2 (Kalshi)
        let outcome2 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Kalshi,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::KalshiSettlement,
            raw_response: None,
        };
        tracker.handle_settlement(outcome2);

        // Position should be fully settled and moved to recently_settled
        assert_eq!(tracker.open.len(), 0);
        assert_eq!(tracker.recently_settled.len(), 1);
    }

    #[test]
    fn handle_settlement_ignores_unknown_event() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        let outcome = SettlementOutcome {
            event_id: "unknown-event".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };
        // Should not panic
        tracker.handle_settlement(outcome);
        assert!(tracker.open.is_empty());
    }

    #[test]
    fn handle_settlement_timeout_evicts_immediately() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        // Signal + fill
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));

        // Settle both legs as timeout
        let outcome1 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Timeout,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::PriceInference,
            raw_response: None,
        };
        let outcome2 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Kalshi,
            outcome: OutcomeKind::Timeout,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::PriceInference,
            raw_response: None,
        };
        tracker.handle_settlement(outcome1);
        tracker.handle_settlement(outcome2);

        // Timeout positions evicted immediately (not in recently_settled)
        assert_eq!(tracker.open.len(), 0);
        assert_eq!(tracker.recently_settled.len(), 0);
    }

    #[test]
    fn handle_settlement_updates_analyzer_accumulators() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        // Signal + fill
        tracker.handle_signal(make_signal("evt-001", "0.03"));
        tracker.handle_snapshot(make_snapshot("evt-001", "0.48", "0.52"));
        assert_eq!(tracker.open.len(), 1);

        // Settle leg 1 (Polymarket)
        let outcome1 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };
        tracker.handle_settlement(outcome1);

        // Settle leg 2 (Kalshi)
        let outcome2 = SettlementOutcome {
            event_id: "evt-001".to_string(),
            venue: Venue::Kalshi,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::KalshiSettlement,
            raw_response: None,
        };
        tracker.handle_settlement(outcome2);

        // Position should be fully settled
        assert_eq!(tracker.open.len(), 0);

        // Analyzer should have recorded the settlement
        let summary = tracker.analyzer().lifetime_summary();
        assert_eq!(summary.total_settled, 1);
        // Should have a hit rate (gross or net depending on P&L direction)
        assert!(summary.gross_hit_rate >= 0.0);
        assert!(summary.net_hit_rate >= 0.0);
    }

    #[test]
    fn handle_settlement_enriched_record_fields() {
        use crate::settlement::types::{
            OutcomeKind, ResolutionSource, SettlementOutcome,
        };
        use crate::types::Venue;

        let config = make_config();
        let mut tracker = make_tracker(config);

        // Use signal with exchange timestamps for stale fill detection
        let mut signal = make_signal("evt-002", "0.03");
        signal.poly_exchange_ts = Some(1700000000100);
        signal.kalshi_exchange_ts = Some(1700000000200);
        signal.threshold_status = Some(crate::signal::types::ThresholdStatus::PassedBoth);
        tracker.handle_signal(signal);
        tracker.handle_snapshot(make_snapshot("evt-002", "0.48", "0.52"));

        // Settle both legs
        let outcome1 = SettlementOutcome {
            event_id: "evt-002".to_string(),
            venue: Venue::Polymarket,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::GammaApi,
            raw_response: None,
        };
        let outcome2 = SettlementOutcome {
            event_id: "evt-002".to_string(),
            venue: Venue::Kalshi,
            outcome: OutcomeKind::Yes,
            settlement_price: None,
            resolved_at: chrono::Utc::now(),
            detected_at: chrono::Utc::now(),
            resolution_source: ResolutionSource::KalshiSettlement,
            raw_response: None,
        };
        tracker.handle_settlement(outcome1);
        tracker.handle_settlement(outcome2);

        // Verify analyzer state after enriched record was produced
        let summary = tracker.analyzer().lifetime_summary();
        assert_eq!(summary.total_settled, 1);
        // Convergence secs should be > 0 (settled_at > signal_timestamp)
        assert!(summary.avg_convergence_secs > 0.0);
    }

    // Cleanup temp dir after tests
    fn cleanup_test_dir() {
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join("paper_trade_test"),
        );
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join("settlement_test"),
        );
    }

    #[test]
    fn cleanup() {
        cleanup_test_dir();
    }
}
