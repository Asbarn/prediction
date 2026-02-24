# Phase 8: Cross-Asset Signal Generation - Research

**Researched:** 2026-02-23
**Domain:** Cross-asset arbitrage signal pipeline (options-implied probability vs. prediction market prices)
**Confidence:** HIGH

## Summary

Phase 8 bridges the two independent pipeline branches -- PricingEngine (Phase 7, Deribit options -> ImpliedProbability) and SpreadEngine (Phase 6, Polymarket/Kalshi prediction markets) -- into a unified cross-asset signal generator. The core work is: (1) a new `CrossAssetEngine` that consumes both ImpliedProbability events and prediction market MarketSnapshots, pairs them by event ID, computes directional spreads with the full cost model, and emits `ArbSignal` structs; (2) extending the existing dynamic threshold infrastructure (already proven in SpreadEngine) to support the cross-asset spread domain; and (3) wiring JSONL logging, Prometheus metrics, and tokio mpsc channels following established patterns.

The codebase already provides nearly all building blocks: `walk_the_book` for realistic fills, the fee/carry cost model, `RollingStats` for dynamic thresholds, `SpreadLogger` for JSONL output, Prometheus metrics via the `metrics` facade, and the `EventRegistry` for cross-venue mapping. The main new work is the pairing logic (instrument_id from ImpliedProbability -> event_id -> prediction market data), the ArbSignal struct definition, the liquidity factor computation from existing WalkResult data, and the pipeline wiring in main.rs to consume the currently-unused `_probability_rx` channel.

**Primary recommendation:** Build the CrossAssetEngine as a new module (`src/signal/`) following the SpreadEngine's architectural pattern (struct with `run()` async method, HashMap-based latest-data cache, biased `tokio::select!` loop). Reuse the existing cost model, threshold, rolling stats, and logger patterns directly -- do not reinvent any of them.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Spread Computation:**
- Staleness gate: only compute spreads when both sides (options-implied prob and prediction market price) have data within a configurable freshness window
- Missing pairs: log at debug level and skip -- no signal for unpaired instruments
- Directional with costs: compute both directions (buy prediction + sell options-implied, sell prediction + buy options-implied), subtract costs from each, report the profitable direction
- Confidence pass-through: compute spreads for ALL options-implied probabilities regardless of confidence score. Carry confidence into ArbSignal metadata. Let the threshold engine use confidence as one factor -- do not gate on input

**Signal Output & Metadata:**
- Rich metadata: ArbSignal carries pricing method used, vol surface quality, solver convergence info, prediction market venue, book depth, IV spread -- beyond the minimum required fields
- Channel + JSONL: emit on tokio mpsc channel for real-time consumers AND log to JSONL file for offline analysis (follows existing spread/trade logging pattern)
- Fixed configurable TTL: all signals get the same TTL from config (e.g., 30 seconds). Not dynamic for v1
- Full Prometheus metrics: counters for signals generated/filtered, histograms for edge size and confidence, gauge for active signal count

**Dynamic Thresholds:**
- Static floor + dynamic component: `max(static_floor, rolling_mean + k * rolling_stddev)` -- already decided in project decisions doc
- Config: `min_edge_bps` (static floor, e.g., 100bps), `threshold_k` (multiplier), `rolling_window_seconds` (default 14400 = 4 hours)
- Liquidity penalty reduces effective edge (not threshold): `net_edge * liquidity_factor` where factor maps from book walker fill price vs top-of-book. Lives in cost_breakdown alongside fees and slippage -- it's a measured quantity, not a tuning parameter
- Static floor during warmup: use only static floor until rolling window has sufficient history, then dynamic component kicks in
- No hysteresis: each cycle is independent. Signal emitted if edge > threshold at that moment. No state tracking of "active" signals

**Signal Lifecycle:**
- Emit new each time: every spread computation that passes threshold emits a fresh ArbSignal. Downstream handles dedup if needed
- Event-driven: recompute spread whenever either side updates (options prob or prediction market price). Immediate signal response, not periodic tick
- Same JSONL file with flag: all signals in one file with `threshold_status` field: "passed_both", "passed_static_only", "filtered". Simpler for analysis
- Periodic summary: log aggregate stats every N minutes (configurable) at info level -- event coverage, signal rate, filter rate, mean edge

### Claude's Discretion
- Exact channel buffer sizes
- JSONL rotation/file naming conventions (follow existing patterns)
- Internal caching strategy for latest prices from each side
- Warmup threshold (how many data points before dynamic component activates)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SGNL-01 | Spread calculator computes spread between prediction market price and options-implied probability for each mapped event | CrossAssetEngine pairs ImpliedProbability (by instrument_id -> event_id via EventRegistry) with latest prediction market MarketSnapshot (by event_id + venue), computes directional spread in probability space, applies cost model |
| SGNL-05 | Signal generation produces ArbSignal with: event ID, direction, raw spread, net edge after costs, confidence, constituent legs, timestamp, and TTL | New ArbSignal struct carries all required fields plus rich metadata (pricing method, vol surface quality, solver convergence, venue, book depth, IV spread). TTL from config |
| SGNL-06 | Configurable minimum edge threshold after all costs, with dynamic thresholds based on volatility regime and available liquidity | Reuses existing `compute_threshold()` + `RollingStats` pattern from SpreadEngine. Liquidity penalty applied to effective edge (not threshold) per user decision. Static floor during warmup |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x | Async runtime, mpsc channels, select!, CancellationToken | Already used throughout project |
| rust_decimal | 1.40 | Precise arithmetic for spreads, costs, thresholds | Project standard for financial math |
| serde / serde_json | 1.0 | ArbSignal serialization for JSONL logging | Project standard |
| chrono | 0.4 | Timestamps, daily file rotation | Project standard |
| metrics | 0.24 | Prometheus metrics facade | Project standard (Phase 6) |
| tracing | 0.1 | Structured logging | Project standard |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio_util | 0.7 | CancellationToken for graceful shutdown | Task spawning in main.rs |
| uuid | 1.x (v7) | Signal ID generation | Signal dedup downstream |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| HashMap cache for latest data | dashmap | Unnecessary -- single-task access pattern, no concurrent reads |
| New module `src/signal/` | Extend `src/spread/engine.rs` | Separate module is cleaner -- cross-asset spreads are fundamentally different from prediction-market-vs-prediction-market spreads |

**Installation:** No new crate dependencies needed. All required libraries are already in Cargo.toml.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── signal/
│   ├── mod.rs           # pub mod declarations, re-exports
│   ├── engine.rs        # CrossAssetEngine: main async pipeline stage
│   ├── types.rs         # ArbSignal, ArbDirection, CostBreakdown, LegInfo, ThresholdStatus
│   ├── config.rs        # SignalGenerationConfig (TOML-driven, serde(default))
│   └── logger.rs        # SignalLogger (JSONL with daily rotation, follows SpreadLogger pattern)
├── spread/              # Existing (Phase 6) -- prediction market vs prediction market
├── pricing/             # Existing (Phase 7) -- options pricing engine
└── ...
```

### Pattern 1: Dual-Input Event Loop (CrossAssetEngine::run)
**What:** A `tokio::select!` loop that consumes from two mpsc channels -- one for ImpliedProbability events (from PricingEngine) and one for prediction market MarketSnapshots. Each incoming event updates the latest-data cache, then attempts spread computation against the paired data.

**When to use:** This is the core engine pattern for Phase 8.

**Why this pattern:** The SpreadEngine already uses a single-input select! loop. Phase 8 needs dual input because the two data sources arrive on separate channels at different rates. The biased select! ensures shutdown is always highest priority.

**Example:**
```rust
pub struct CrossAssetEngine {
    // Latest options-implied probability per event_id
    latest_prob: HashMap<String, ImpliedProbability>,
    // Latest prediction market snapshot per (event_id, venue)
    latest_pred: HashMap<(String, Venue), MarketSnapshot>,
    // Rolling statistics per event_id (for dynamic threshold)
    stats: HashMap<String, RollingStats>,
    config: SignalGenerationConfig,
    logger: SignalLogger,
    signal_count: u64,
    filtered_count: u64,
}

impl CrossAssetEngine {
    pub async fn run(
        mut self,
        mut prob_rx: mpsc::Receiver<ImpliedProbability>,
        mut pred_snap_rx: mpsc::Receiver<MarketSnapshot>,
        registry: Arc<RwLock<EventRegistry>>,
        cancel: CancellationToken,
        signal_tx: mpsc::Sender<ArbSignal>,
    ) {
        let mut stats_interval = tokio::time::interval(/* config */);
        stats_interval.tick().await;

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => { break; }
                _ = stats_interval.tick() => { self.emit_summary(); }

                prob = prob_rx.recv() => {
                    if let Some(prob) = prob {
                        self.handle_probability(prob, &registry, &signal_tx).await;
                    } else { break; }
                }

                snap = pred_snap_rx.recv() => {
                    if let Some(snap) = snap {
                        self.handle_prediction_snapshot(snap, &registry, &signal_tx).await;
                    } else { break; }
                }
            }
        }
    }
}
```

### Pattern 2: Event ID Pairing via EventRegistry
**What:** ImpliedProbability arrives keyed by `instrument_id` (e.g., "BTC-27JUN25-100000-C"). The EventRegistry maps `(Venue::Deribit, instrument_id)` -> `EventMapping`, which provides the canonical `event_id`. Prediction market snapshots are already annotated with `event_id` (or looked up via `(Venue::Polymarket, token_id)`). The engine caches data keyed by event_id for pairing.

**When to use:** Every time an ImpliedProbability or prediction market snapshot arrives.

**Key detail:** A single Deribit option (e.g., BTC-27JUN25-100000-C) maps to one event. But the event may have both Polymarket AND Kalshi legs. The engine should compute spreads against each available prediction market venue independently.

**Example:**
```rust
fn handle_probability(&mut self, prob: ImpliedProbability, registry: &EventRegistry) {
    let reg = registry.read().await;
    let mapping = match reg.lookup_by_instrument(Venue::Deribit, &prob.instrument_id.to_string()) {
        Some(m) => m.clone(),
        None => return, // unmapped option instrument
    };
    drop(reg);

    let event_id = mapping.id.clone();
    self.latest_prob.insert(event_id.clone(), prob);

    // Try spread computation against each prediction market venue
    for venue in [Venue::Polymarket, Venue::Kalshi] {
        if let Some(pred_snap) = self.latest_pred.get(&(event_id.clone(), venue)) {
            self.compute_and_emit_signal(&event_id, venue, pred_snap, &self.latest_prob[&event_id]);
        }
    }
}
```

### Pattern 3: Liquidity Factor as Cost Component
**What:** Per user decision, liquidity penalty reduces effective edge rather than inflating the threshold. The liquidity factor is computed from the book walker's fill price vs top-of-book: `(walked_fill_price - top_of_book) / top_of_book`. This is placed in the cost breakdown alongside fees and slippage.

**When to use:** During every spread computation.

**Key detail:** For the options side, there is no walk-the-book equivalent (Phase 7 produces a probability, not a depth array). The liquidity factor for the options leg should use the bid-ask spread from ImpliedProbability (`prob_bid`, `prob_ask`) as a proxy. For the prediction market side, use the existing `walk_the_book` function from `spread::book_walker`.

**Example:**
```rust
fn compute_liquidity_factor(walk: &WalkResult) -> Decimal {
    if walk.target_notional.is_zero() || walk.avg_fill_price.is_zero() {
        return Decimal::ONE;
    }
    // fill_ratio already captures depth adequacy
    // Additional slippage from walking deeper into the book
    walk.fill_ratio()
}

fn options_liquidity_proxy(prob: &ImpliedProbability) -> Decimal {
    // Use bid-ask spread width as proxy for options liquidity
    match (prob.prob_bid, prob.prob_ask) {
        (Some(bid), Some(ask)) => {
            let spread = (ask.into_inner() - bid.into_inner()).abs();
            // Wider spread = lower liquidity factor
            // 0.01 spread -> factor ~1.0, 0.10 spread -> factor ~0.5
            let factor = Decimal::ONE - spread * Decimal::new(5, 0);
            factor.max(Decimal::new(1, 1)) // floor at 0.1
        }
        _ => Decimal::new(5, 1), // 0.5 conservative default
    }
}
```

### Pattern 4: Three-Tier Threshold Status Logging
**What:** Every spread computation is logged to JSONL with a `threshold_status` field: `"passed_both"` (above both static and dynamic threshold), `"passed_static_only"` (above static floor but below dynamic), or `"filtered"` (below static floor). Per user decision, this is a single file, not separate files.

**When to use:** Every signal evaluation, regardless of outcome.

**Key detail:** Signals that pass static floor but fail dynamic threshold are still interesting for Phase 9 threshold effectiveness analysis.

### Anti-Patterns to Avoid
- **Gating on confidence score:** User explicitly decided that ALL options-implied probabilities flow through, regardless of confidence. Confidence is metadata for downstream, not a filter.
- **Dynamic TTL:** User decided fixed configurable TTL for v1. Do not build dynamic TTL based on volatility.
- **Hysteresis / active signal tracking:** Each cycle is independent. No state machine for "signal active" / "signal expired". Just emit fresh each time.
- **Extending SpreadEngine:** The cross-asset engine is fundamentally different (different input types, different cost model, different directionality). Build a new module.
- **Blocking sends on signal channel:** Use `try_send` for downstream signal delivery (same as SpreadEngine and PricingEngine patterns). Never block the engine.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rolling window statistics | Custom windowed stats | `spread::rolling_stats::RollingStats` | Already tested and used in SpreadEngine |
| Dynamic threshold computation | Custom threshold logic | `spread::threshold::compute_threshold()` | Already implements the `max(floor, mean + k*stddev) + liquidity_penalty` formula |
| JSONL logging with daily rotation | Custom file writer | Follow `spread::logger::SpreadLogger` pattern | Proven pattern with flush intervals, append mode, date rotation |
| Order book walking | Custom fill simulation | `spread::book_walker::walk_the_book()` | Already handles partial fills, weighted averages |
| Fee computation | Custom fee math | `spread::cost_model::{polymarket_fee, kalshi_taker_fee, carry_cost}` | Already tested with edge cases |
| Event ID resolution | Custom mapping | `events::registry::EventRegistry::lookup_by_instrument()` | Already indexes by (Venue, instrument_id) |
| Prometheus metrics | Custom metrics | `metrics::counter!`, `metrics::histogram!`, `metrics::gauge!` macros | Zero-cost no-ops without recorder, already wired |

**Key insight:** Phase 8 is primarily an integration phase. The mathematical and infrastructure building blocks already exist. The new work is wiring them together in a new topology (options + prediction markets) and defining the ArbSignal output type.

## Common Pitfalls

### Pitfall 1: Instrument-to-Event Mapping Gap
**What goes wrong:** ImpliedProbability contains `instrument_id` (e.g., "BTC-27JUN25-100000-C") but the cross-asset engine needs `event_id` to pair with prediction market data. If the EventRegistry mapping doesn't include a Deribit instrument entry for a given option, the probability is silently dropped.
**Why it happens:** Not all Deribit instruments have corresponding event mappings. Many options (different strikes, different expiries) won't match any prediction market contract.
**How to avoid:** Log unmapped instruments at debug level (not warn -- this is expected for most options). Track a metric `signal_unmapped_instruments_total` to monitor coverage.
**Warning signs:** Zero signals despite active PricingEngine output. Check that events.toml has Deribit venue entries.

### Pitfall 2: Direction Confusion in Cross-Asset Spreads
**What goes wrong:** Options-implied probability is P(S > K) -- the probability the underlying exceeds the strike. Prediction market prices are YES/NO probabilities. The direction mapping must account for event direction (above/below) from EventMapping.
**Why it happens:** A "BTC above 100K" prediction market YES price should be compared with P(S > 100K). But a "BTC above 100K" Deribit call's implied probability is also P(S > 100K). The spread is `prediction_price - options_implied_prob` for the "buy options, sell prediction" direction. Getting the signs wrong produces phantom arbitrage.
**How to avoid:** Define `ArbDirection` as an enum with clear semantics. For each computation, log both the raw probabilities and the computed spread direction. Unit tests with known values are critical.
**Warning signs:** All computed spreads are positive (or all negative) -- indicates systematic direction error.

### Pitfall 3: Mismatched Probability Spaces
**What goes wrong:** Prediction market prices are mid-market (or bid/ask) in [0, 1] probability space. Options-implied probability is a model output also in [0, 1]. But prediction market bid/ask spreads represent transaction cost, while options bid/ask IV spread represents model uncertainty. Comparing midpoints directly overstates theoretical edge because the "realizable" probability on each side is different.
**Why it happens:** Naive implementation uses `prediction_mid - options_implied_mid` as the spread. This ignores that you'd buy prediction at the ask (higher) and sell options-implied at whatever the actual bid represents.
**How to avoid:** For the prediction market leg, use `walk_the_book` with the appropriate side (asks for buying, bids for selling). For the options leg, use `prob_bid` or `prob_ask` from ImpliedProbability depending on direction. The raw spread uses midpoints; the net edge uses executable prices.
**Warning signs:** Signals that look profitable in raw spread but always fail after costs. The cost breakdown should show significant slippage.

### Pitfall 4: Stale Pairing Across Timescales
**What goes wrong:** Deribit options update relatively infrequently (tickers on every trade/mark change, not every millisecond). Prediction markets (especially Polymarket WebSocket) update more frequently. A stale options probability paired with a fresh prediction market price produces misleading spreads.
**Why it happens:** The engine caches "latest" data from each side. If the options side hasn't updated in 60 seconds but the prediction market moved, the computed spread reflects an arbitrage that no longer exists.
**How to avoid:** Configurable staleness gate on BOTH sides (per user decision). The ImpliedProbability carries a `timestamp` field -- compare against current time. Use separate staleness thresholds for the options side (more lenient, e.g., 30-60s) vs prediction market side (tighter, e.g., 5-15s per existing config).
**Warning signs:** High signal rate that doesn't correlate with actual market dislocations. Signals cluster right after options updates then decay.

### Pitfall 5: Overwhelming Signal Volume During Warmup
**What goes wrong:** Before the rolling window accumulates enough samples, the cold-start multiplied static floor is the only threshold. If it's set too low, a flood of signals during the first few hours overwhelms downstream.
**Why it happens:** The dynamic threshold needs history to be meaningful. During warmup, the threshold is `static_floor * cold_start_multiplier` (default: 0.01 * 2.0 = 0.02 = 2%). Cross-asset spreads that appear to be 2%+ before costs are common noise.
**How to avoid:** Set the static floor for cross-asset signals conservatively (suggest 100bps = 0.01, but configurable). The warmup threshold (min_samples) defaults to 30 from the existing ThresholdConfig. Monitor `signal_filtered_total` metrics during the first rolling window period.
**Warning signs:** Signal volume drops sharply after 4 hours (rolling window fills up and dynamic threshold kicks in).

### Pitfall 6: Deribit Options Cost Model Differs from Prediction Markets
**What goes wrong:** The existing cost model (Phase 6) covers Polymarket and Kalshi fees. Deribit options have a different fee structure: maker 0.00%, taker 0.03% of underlying (with caps), settled in BTC/ETH.
**Why it happens:** Phase 6 was prediction-market-only. Cross-asset spreads need the options leg cost too.
**How to avoid:** Add a Deribit options fee function to the cost model. For v1 paper trading, a simple `taker_rate * underlying_price * option_delta` approximation is sufficient. The exact fee is less critical than including *some* fee to avoid zero-cost-illusion signals.
**Warning signs:** Net edge is suspiciously close to gross edge on the options leg.

## Code Examples

### ArbSignal Struct Definition
```rust
// src/signal/types.rs
use rust_decimal::Decimal;
use serde::Serialize;
use crate::pricing::types::{PricingMethod, ConfidenceComponents, SolverResult};
use crate::types::{DualTimestamp, InstrumentId, Venue};

/// Direction of the cross-asset arbitrage signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArbDirection {
    /// Buy prediction market, sell options-implied (prediction price < options probability)
    BuyPredictionSellOptions,
    /// Sell prediction market, buy options-implied (prediction price > options probability)
    SellPredictionBuyOptions,
}

/// Threshold evaluation status for logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ThresholdStatus {
    /// Above both static floor and dynamic threshold
    PassedBoth,
    /// Above static floor only (dynamic threshold filtered it)
    PassedStaticOnly,
    /// Below even the static floor
    Filtered,
}

/// Cost breakdown for a single cross-asset spread computation.
#[derive(Debug, Clone, Serialize)]
pub struct CostBreakdown {
    pub prediction_fee: Decimal,
    pub options_fee_estimate: Decimal,
    pub carry_cost: Decimal,
    pub prediction_slippage: Decimal,
    pub options_spread_cost: Decimal,
    pub liquidity_factor: Decimal,
    pub total_cost: Decimal,
}

/// Information about one leg of the arbitrage.
#[derive(Debug, Clone, Serialize)]
pub struct LegInfo {
    pub venue: Venue,
    pub instrument_id: String,
    pub probability: Decimal,
    pub executable_price: Decimal,
    pub book_depth_levels: usize,
    pub fill_ratio: Decimal,
}

/// Cross-asset arbitrage signal output.
///
/// Primary output of Phase 8. Carries all metadata needed for
/// downstream consumption (paper trading, future execution).
#[derive(Debug, Clone, Serialize)]
pub struct ArbSignal {
    // -- Required fields (SGNL-05) --
    pub signal_id: String,
    pub event_id: String,
    pub direction: ArbDirection,
    pub raw_spread: Decimal,
    pub net_edge: Decimal,
    pub confidence: f64,
    pub prediction_leg: LegInfo,
    pub options_leg: LegInfo,
    pub timestamp: DualTimestamp,
    pub ttl_secs: u64,

    // -- Rich metadata (user decision) --
    pub pricing_method: PricingMethod,
    pub confidence_components: ConfidenceComponents,
    pub solver_meta: Option<SolverResult>,
    pub iv_spread: f64,
    pub skew_adjustment: f64,
    pub cost_breakdown: CostBreakdown,
    pub prediction_venue: Venue,
    pub threshold_status: ThresholdStatus,
    pub threshold_value: Decimal,
    pub threshold_components: Option<crate::spread::patterns::ThresholdComponents>,
}
```

### Signal Generation Config
```rust
// src/signal/config.rs
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use crate::spread::config::ThresholdConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SignalGenerationConfig {
    /// Staleness threshold for options-implied probability (ms).
    /// More lenient than prediction markets due to lower update frequency.
    pub options_staleness_ms: u64,
    /// Staleness threshold for Polymarket prediction market snapshots (ms).
    pub polymarket_staleness_ms: u64,
    /// Staleness threshold for Kalshi prediction market snapshots (ms).
    pub kalshi_staleness_ms: u64,
    /// Signal TTL in seconds (fixed for v1).
    pub signal_ttl_secs: u64,
    /// Target notional for walk-the-book on prediction market leg.
    #[serde(with = "rust_decimal::serde::str")]
    pub target_notional: Decimal,
    /// Deribit options taker fee rate (fraction of underlying, e.g., 0.0003).
    #[serde(with = "rust_decimal::serde::str")]
    pub deribit_taker_fee_rate: Decimal,
    /// Threshold configuration (reuses existing ThresholdConfig).
    pub threshold: ThresholdConfig,
    /// Rolling window for spread statistics (seconds). Default 14400 = 4 hours.
    pub rolling_window_secs: u64,
    /// Directory for signal JSONL logs.
    pub log_dir: String,
    /// Interval for periodic summary emission (seconds).
    pub summary_interval_secs: u64,
    /// Carry cost config (reuse existing).
    pub carry: crate::spread::config::CarryConfig,
    /// Polymarket fee config (reuse existing).
    pub polymarket_fees: crate::spread::config::PolymarketFeeConfig,
    /// Kalshi fee config (reuse existing).
    pub kalshi_fees: crate::spread::config::KalshiFeeConfig,
}

impl Default for SignalGenerationConfig {
    fn default() -> Self {
        Self {
            options_staleness_ms: 30_000,  // 30 seconds
            polymarket_staleness_ms: 5_000,
            kalshi_staleness_ms: 15_000,
            signal_ttl_secs: 30,
            target_notional: Decimal::new(500, 0),
            deribit_taker_fee_rate: Decimal::new(3, 4), // 0.0003 = 0.03%
            threshold: ThresholdConfig::default(),
            rolling_window_secs: 14400,
            log_dir: "signal_logs".to_string(),
            summary_interval_secs: 300, // 5 minutes
            carry: crate::spread::config::CarryConfig::default(),
            polymarket_fees: crate::spread::config::PolymarketFeeConfig::default(),
            kalshi_fees: crate::spread::config::KalshiFeeConfig::default(),
        }
    }
}
```

### Pipeline Wiring in main.rs
```rust
// In main.rs, replace the current:
//   let (probability_tx, _probability_rx) = mpsc::channel::<ImpliedProbability>(1024);
// With:

// Probability channel: PricingEngine -> CrossAssetEngine
let (probability_tx, probability_rx) = mpsc::channel::<ImpliedProbability>(1024);

// Prediction market snapshot channel: fan-out -> CrossAssetEngine
// Fork from the existing fan-out task (add a third output)
let (signal_pred_snap_tx, signal_pred_snap_rx) = mpsc::channel::<MarketSnapshot>(1024);

// ArbSignal output channel: CrossAssetEngine -> downstream (future Phase 9)
let (arb_signal_tx, _arb_signal_rx) = mpsc::channel::<ArbSignal>(1024);

// Spawn CrossAssetEngine
let signal_config = config.system.signal_generation.clone();
let signal_engine = CrossAssetEngine::new(signal_config);
let signal_cancel = shutdown_token.child_token();
tokio::spawn(signal_engine.run(
    probability_rx,
    signal_pred_snap_rx,
    event_registry.clone(),
    signal_cancel,
    arb_signal_tx,
));
```

### Spread Computation Core Logic
```rust
fn compute_cross_asset_spread(
    &mut self,
    event_id: &str,
    pred_venue: Venue,
    pred_snap: &MarketSnapshot,
    implied_prob: &ImpliedProbability,
    signal_tx: &mpsc::Sender<ArbSignal>,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();

    // 1. Extract probabilities
    let options_prob = implied_prob.probability.into_inner();
    let pred_bid = match pred_snap.bid_probability {
        Some(p) => p.into_inner(),
        None => return,
    };
    let pred_ask = match pred_snap.ask_probability {
        Some(p) => p.into_inner(),
        None => return,
    };
    let pred_mid = (pred_bid + pred_ask) / Decimal::new(2, 0);

    // 2. Compute both directions
    // Direction A: buy prediction (at ask), profit if options prob > prediction ask
    let raw_spread_buy_pred = options_prob - pred_ask;
    // Direction B: sell prediction (at bid), profit if prediction bid > options prob
    let raw_spread_sell_pred = pred_bid - options_prob;

    // 3. For each direction, compute costs and net edge
    for (direction, raw_spread, pred_executable) in [
        (ArbDirection::BuyPredictionSellOptions, raw_spread_buy_pred, pred_ask),
        (ArbDirection::SellPredictionBuyOptions, raw_spread_sell_pred, pred_bid),
    ] {
        // Walk prediction market book for realistic fill
        let pred_depth = match direction {
            ArbDirection::BuyPredictionSellOptions => &pred_snap.depth_asks,
            ArbDirection::SellPredictionBuyOptions => &pred_snap.depth_bids,
        };
        let walk = walk_the_book(pred_depth, self.config.target_notional);

        // Compute costs
        let cost_breakdown = self.compute_costs(pred_venue, &walk, pred_executable, implied_prob);

        // Net edge = raw_spread - total_cost, adjusted by liquidity factor
        let net_edge = (raw_spread - cost_breakdown.total_cost) * cost_breakdown.liquidity_factor;

        // 4. Rolling stats update
        let rolling = self.stats
            .entry(event_id.to_string())
            .or_insert_with(|| RollingStats::new(self.config.rolling_window_secs));
        let net_edge_f64 = decimal_to_f64(net_edge);
        rolling.push(net_edge_f64, now_ms);

        // 5. Threshold evaluation
        let (threshold_value, components) = compute_threshold(
            rolling,
            &self.config.threshold,
            walk.fill_ratio(),
            Decimal::ONE, // options side has no walk equivalent
        );

        let threshold_status = if net_edge > threshold_value {
            ThresholdStatus::PassedBoth
        } else if net_edge > self.config.threshold.static_floor {
            ThresholdStatus::PassedStaticOnly
        } else {
            ThresholdStatus::Filtered
        };

        // 6. Build ArbSignal (always, for logging)
        let signal = ArbSignal { /* ... */ };

        // 7. Log to JSONL (all signals, regardless of status)
        self.logger.log(&signal).await;

        // 8. Emit on channel only if passed threshold
        if threshold_status == ThresholdStatus::PassedBoth {
            self.signal_count += 1;
            let _ = signal_tx.try_send(signal);
            metrics::counter!("arb_signals_emitted_total").increment(1);
        } else {
            self.filtered_count += 1;
            metrics::counter!("arb_signals_filtered_total").increment(1);
        }

        // 9. Metrics for every computation
        metrics::histogram!("arb_signal_net_edge_bps").record(net_edge_f64 * 10000.0);
        metrics::histogram!("arb_signal_confidence").record(implied_prob.confidence);
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single-pipeline spread engine (pred vs pred) | Dual-pipeline: pred-vs-pred (SpreadEngine) + options-vs-pred (CrossAssetEngine) | Phase 8 | Two independent signal streams, both feeding downstream |
| `_probability_rx` unused channel | `probability_rx` consumed by CrossAssetEngine | Phase 8 | PricingEngine output now has a consumer |

**Deprecated/outdated:**
- Nothing deprecated. Phase 8 adds new capability without removing existing.

## Open Questions

1. **Deribit Options Fee Precision**
   - What we know: Deribit taker fee is 0.03% of underlying for options. Maker is 0.00%. There are caps.
   - What's unclear: The exact cap structure (0.125% of option price for BTC) and how it interacts with delta-weighted positions. For paper trading, approximate fees may be sufficient.
   - Recommendation: Implement `deribit_taker_fee_rate * underlying_price * abs(delta)` as v1 estimate. The delta is available from ImpliedProbability's Greeks. Add a `TODO` for exact cap implementation if paper trading reveals fee sensitivity.

2. **Fan-out Topology: Two-Way or Three-Way?**
   - What we know: Currently there's a 2-way fan-out (SpreadEngine + PricingEngine). Phase 8 needs prediction market snapshots routed to the CrossAssetEngine too.
   - What's unclear: Whether to extend the existing fan-out to 3-way, or have the CrossAssetEngine receive snapshots from a different point.
   - Recommendation: Extend the existing fan-out to 3-way. The fan-out task is already simple (clone + send/try_send). Adding a third output is minimal change. Use try_send for the signal engine (same as PricingEngine -- best-effort, never block).

3. **Options Leg "Executable Price"**
   - What we know: ImpliedProbability provides `probability` (mid), `prob_bid`, and `prob_ask`. For a real trade, you'd execute on one side.
   - What's unclear: Whether to use mid, bid, or ask for the "executable" side of the options leg in spread computation.
   - Recommendation: Use `prob_bid` when "selling" the options-implied probability (buying prediction), and `prob_ask` when "buying" options-implied probability (selling prediction). This is conservative and avoids overstating edge. If either is None, fall back to mid with a confidence penalty note.

## Sources

### Primary (HIGH confidence)
- Existing codebase analysis: `src/spread/engine.rs`, `src/pricing/engine.rs`, `src/main.rs` -- direct reading of current implementation patterns
- Existing codebase analysis: `src/spread/threshold.rs`, `src/spread/cost_model.rs`, `src/spread/rolling_stats.rs` -- infrastructure to reuse
- Existing codebase analysis: `src/pricing/types.rs` -- ImpliedProbability struct definition
- Existing codebase analysis: `src/events/registry.rs` -- EventRegistry mapping API
- CONTEXT.md (user decisions) -- locked implementation decisions

### Secondary (MEDIUM confidence)
- Project STATE.md -- accumulated decisions from Phases 1-7, architecture patterns
- REQUIREMENTS.md -- SGNL-01, SGNL-05, SGNL-06 requirement definitions

### Tertiary (LOW confidence)
- Deribit fee structure: 0.03% taker fee for options is from training data. Recommend verifying against current Deribit docs before production use. For paper trading, the approximation is adequate.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in project, no new dependencies
- Architecture: HIGH -- pattern directly follows existing SpreadEngine/PricingEngine, codebase thoroughly analyzed
- Pitfalls: HIGH -- based on direct codebase analysis and understanding of the data flow
- Cost model: MEDIUM -- Deribit options fee approximation needs verification for production, adequate for paper trading

**Research date:** 2026-02-23
**Valid until:** 2026-03-23 (stable -- all infrastructure already exists, no external dependencies changing)
