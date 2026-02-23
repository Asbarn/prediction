# Phase 6: Prediction Market Spreads - Research

**Researched:** 2026-02-23
**Domain:** Cross-platform prediction market arbitrage detection, fee-adjusted spread computation, Prometheus metrics export, paper trade P&L tracking
**Confidence:** HIGH

## Summary

Phase 6 builds the first actionable trading signal layer on top of the multi-venue data pipeline (Phases 1-5). The core challenge is consuming `MarketSnapshot` events from the fan-in channel, pairing snapshots from Polymarket and Kalshi by event ID using the `EventRegistry`, computing fee-adjusted net spreads across all 4 directional patterns, and logging every computation for distribution analysis. The secondary deliverables are a Prometheus metrics exporter (replacing the current zero-cost no-op `metrics` facade with a real recorder), rolling aggregate statistics, and a paper trade P&L tracker.

The codebase is well-positioned for this phase. The `metrics` crate v0.24 is already a dependency with macros emitting counter/gauge/histogram calls throughout the feed layer. Installing `metrics-exporter-prometheus` v0.16 (compatible with `metrics ^0.24`) as a recorder will activate all existing metric instrumentation with zero code changes to the feed layer. The `EventRegistry` already provides O(1) lookups by `(Venue, instrument_id)` and indexes by `event_id`, and `MarketSnapshot` carries `depth_bids`/`depth_asks` with `(Price, Notional)` tuples -- exactly the depth data needed for walk-the-book cost computation. The `BasisRiskScore` from Phase 5 is already available as annotation metadata per event mapping.

**Primary recommendation:** Implement a `SpreadEngine` that subscribes to the fan-in `MarketSnapshot` channel, maintains latest-snapshot-per-instrument state, and on each update computes all 4 spread patterns for every matched event pair. Use `metrics-exporter-prometheus` 0.16 with `http-listener` feature for Prometheus scraping. Log every spread computation as JSONL for offline analysis. Implement rolling statistics with Welford's online algorithm for numerical stability. Paper trade tracker records entries at next-tick-after-signal and tracks both settlement and mark-to-market outcomes.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **Signal thresholds:** Static floor + dynamic component: `max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty`
- Spread distribution is the primary dynamic signal (statistical unusualness)
- Liquidity depth acts as a cost adjustment -- thin books raise the threshold via an inverse-depth penalty
- All parameters (static_floor, k, penalty scaling) configurable in TOML
- Log all threshold components (static floor, rolling mean, rolling stddev, k*sigma, liquidity penalty, final threshold) for post-hoc evaluation of which factor drives useful signals
- Rolling window: configurable, default 4 hours -- short enough for regime adaptation (FOMC, ETF news, weekend/weekday shifts), long enough for meaningful sample size
- Start with single window; design allows adding multiple windows (1h/4h/24h) later
- No cooldown or deduplication -- fire every threshold crossing, deduplication is a downstream concern

- **Cost model approach:** Walk the book for a configurable fixed notional size (e.g., $500) to compute average fill price -- not top-of-book + flat penalty
- Polymarket fees: implement exact dynamic fee formula from their docs + TOML override to swap in flat rate for comparison
- Kalshi fees: 7% profit fee (from their current structure)
- Include carry cost: configurable annualized rate prorated by expected holding period, penalizing longer-dated positions
- Basis risk is a SEPARATE concern -- not folded into the cost model. BasisRiskScore from Phase 5 is metadata/filter on the signal, not a cost component
- Both legs must pass staleness gate before spread computation proceeds

- **Paper trade rules:** Configurable fixed notional per trade (TOML), leave room for Kelly/edge-proportional sizing later
- Entry: fill at next tick after signal fires (not at signal-time quote) -- captures some adverse selection
- Track both hold-to-settlement AND mark-to-market over time, so both settlement P&L and early-exit (spread reversion) strategies can be analyzed post-hoc
- P&L aggregation: per-signal individual trade P&L + daily rollups. Weekly can be derived offline
- Log mark-to-market values over position lifetime for later strategy comparison

### Claude's Discretion
- Logging and metrics design (what goes to file vs Prometheus vs stdout)
- Prometheus metric naming and label conventions
- Exact data structures for spread computation pipeline
- Aggregate statistics implementation (mean, stddev, percentiles)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SGNL-02 | Spread calculation adjusts for: transaction fees (Polymarket dynamic fees up to ~1.56% at 50/50, Kalshi 7% profit fee), slippage estimate from available depth, funding/carry cost, settlement basis risk premium | Fee formula research (Polymarket: `fee = C * feeRate * (p * (1-p))^exponent`; Kalshi: `0.07 * C * P * (1-P)` taker), walk-the-book depth traversal pattern, carry cost annualization pattern |
| SGNL-03 | Every spread calculation validates both sides are fresh (staleness gate) and rejects with logging if either side exceeds threshold | Existing `is_stale` flag on MarketSnapshot + exchange_timestamp; staleness gate pattern already proven in Deribit/Polymarket/Kalshi processors |
| SGNL-04 | Cross-platform prediction market spread detection (Polymarket vs Kalshi) for 4 patterns: Poly YES + Kalshi NO, inverse, and each direction | Pattern enumeration in Architecture section; EventRegistry lookup_by_event_id provides the mapping; bid_probability/ask_probability already populated on snapshots |
| SGNL-07 | Every spread computation logged to file (not just signals above threshold) for distribution analysis, regime detection, and threshold tuning | JSONL writer pattern from feed/recording/writer.rs; SpreadRecord struct with all computation components |
| SGNL-08 | Periodic aggregate spread statistics (mean, stddev, percentiles) emitted to metrics and stdout | Rolling statistics with Welford's algorithm; Prometheus histogram for distribution; periodic tick-based emission |
| OBSV-03 | Prometheus metrics exporter with key metrics: spread by event (histogram), signal count, fill rate proxy, feed-to-signal latency, feed health | `metrics-exporter-prometheus` 0.16 with PrometheusBuilder, http-listener on configurable port; metric naming conventions documented |
| OBSV-04 | Paper trade P&L tracking: hypothetical entry/exit at signal time, per-signal P&L assuming fill at quoted price, daily/weekly aggregates | PaperTradeTracker with next-tick entry, dual settlement/MTM tracking, daily rollup pattern |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| metrics | 0.24 | Metrics facade (counters, gauges, histograms) | Already in Cargo.toml; zero-cost no-ops until recorder installed |
| metrics-exporter-prometheus | 0.16 | Prometheus recorder + HTTP scrape endpoint | Compatible with `metrics ^0.24` (already in project); provides PrometheusBuilder with HTTP listener, histogram buckets, recommended naming |
| metrics-util | (transitive) | Metric kind masks for idle timeout config | Pulled in by metrics-exporter-prometheus; used for `MetricKindMask` in builder config |
| rust_decimal | 1.40 | Precise decimal arithmetic for fee/spread computation | Already in Cargo.toml; Price/Probability/Notional types are Decimal wrappers |
| serde / serde_json | 1.0 | JSONL serialization for spread logs | Already in Cargo.toml |
| chrono | 0.4 | Timestamps for paper trade lifecycle | Already in Cargo.toml |
| tokio | 1 | Async runtime, channels, timers | Already in Cargo.toml |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 | Structured logging for spread computations | Already in Cargo.toml; used for stdout emission of aggregate stats |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| metrics-exporter-prometheus 0.16 | 0.18 (latest) | 0.18 requires `metrics ^0.24` -- same. But 0.18 requires `hyper 1.8`; the project uses `reqwest 0.12` which brings its own hyper. Use 0.16 to avoid hyper version conflicts with existing reqwest dependency. If 0.16 has compatibility issues, fallback to 0.18 with feature-flag isolation. |
| Hand-rolled rolling stats | ta-statistics crate | ta-statistics adds unnecessary complexity for mean/stddev/percentile; Welford's algorithm is <50 lines and avoids another dependency |
| VecDeque-based rolling window | Circular buffer crate | VecDeque is idiomatic Rust, no external dep needed, O(1) push/pop |

**IMPORTANT VERSION NOTE:** The project already has `metrics = "0.24"` in Cargo.toml. The `metrics-exporter-prometheus` version MUST be compatible with `metrics ^0.24`. Version 0.16 targets `metrics ^0.23`, and version 0.18 targets `metrics ^0.24`. **Use version 0.18** (not 0.16) to match the existing `metrics = "0.24"` dependency. The version 0.16 suggestion was based on initial research but verified docs.rs shows 0.18 is the correct match. If hyper version conflicts arise with reqwest, use `default-features = false, features = ["http-listener"]` to minimize transitive dependencies.

**Installation:**
```toml
# Add to Cargo.toml [dependencies]
metrics-exporter-prometheus = "0.18"
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── spread/                     # NEW: Phase 6 spread computation module
│   ├── mod.rs                  # Module exports
│   ├── engine.rs               # SpreadEngine: snapshot pairing + spread computation loop
│   ├── cost_model.rs           # Fee calculations: Polymarket dynamic, Kalshi profit, carry cost
│   ├── book_walker.rs          # Walk-the-book average fill price for configurable notional
│   ├── threshold.rs            # Dynamic threshold: max(floor, mean + k*sigma) + liquidity_penalty
│   ├── rolling_stats.rs        # Welford's online algorithm for rolling mean/stddev/percentiles
│   ├── patterns.rs             # 4-pattern spread detection (PolyYES+KalshiNO etc.)
│   ├── logger.rs               # JSONL spread computation logger
│   └── config.rs               # SpreadConfig TOML structs
├── paper_trade/                # NEW: Phase 6 paper trade module
│   ├── mod.rs                  # Module exports
│   ├── tracker.rs              # PaperTradeTracker: entry/exit, MTM, settlement tracking
│   ├── position.rs             # PaperPosition: individual trade lifecycle
│   └── aggregator.rs           # Daily P&L rollups
├── metrics_export/             # NEW: Phase 6 Prometheus setup
│   └── mod.rs                  # PrometheusBuilder setup, metric descriptions
├── feed/                       # Existing: no changes needed
├── events/                     # Existing: EventRegistry consumed read-only
├── types/                      # Existing: may add spread-specific types
└── ...
```

### Pattern 1: Snapshot Pairing via Latest-State Map
**What:** Maintain a `HashMap<(EventId, Venue), MarketSnapshot>` that stores the most recent non-stale snapshot per event per venue. On each incoming snapshot, update the map and trigger spread computation for all venue pairs sharing that event ID.
**When to use:** Always -- this is the core data flow pattern.
**Example:**
```rust
use std::collections::HashMap;
use crate::types::{MarketSnapshot, Venue};

struct SnapshotState {
    /// Latest snapshot per (event_id, venue)
    latest: HashMap<(String, Venue), MarketSnapshot>,
}

impl SnapshotState {
    fn update(&mut self, event_id: &str, snap: MarketSnapshot) {
        let key = (event_id.to_string(), snap.venue);
        self.latest.insert(key, snap);
    }

    fn get_pair(&self, event_id: &str, venue_a: Venue, venue_b: Venue)
        -> Option<(&MarketSnapshot, &MarketSnapshot)>
    {
        let a = self.latest.get(&(event_id.to_string(), venue_a))?;
        let b = self.latest.get(&(event_id.to_string(), venue_b))?;
        Some((a, b))
    }
}
```

### Pattern 2: Walk-the-Book Cost Model
**What:** For a configurable notional size, walk the depth levels accumulating fill quantity and weighted average price. This produces a realistic average fill price that accounts for depth, rather than top-of-book price.
**When to use:** Every spread computation (per the user's locked decision).
**Example:**
```rust
use rust_decimal::Decimal;
use crate::types::{Price, Notional};

/// Walk order book depth to compute average fill price for target notional.
/// Returns (average_fill_price, filled_notional) -- if depth is insufficient,
/// filled_notional < target_notional (signals a liquidity shortfall).
fn walk_the_book(
    depth: &[(Price, Notional)],
    target_notional: Decimal,
) -> (Decimal, Decimal) {
    let mut remaining = target_notional;
    let mut total_cost = Decimal::ZERO;
    let mut total_filled = Decimal::ZERO;

    for &(price, size) in depth {
        if remaining <= Decimal::ZERO {
            break;
        }
        let fill_at_level = remaining.min(size.into_inner());
        total_cost += fill_at_level * price.into_inner();
        total_filled += fill_at_level;
        remaining -= fill_at_level;
    }

    if total_filled > Decimal::ZERO {
        (total_cost / total_filled, total_filled)
    } else {
        (Decimal::ZERO, Decimal::ZERO)
    }
}
```

### Pattern 3: Four Spread Patterns
**What:** For each mapped event pair (Polymarket + Kalshi), detect all 4 directional patterns:
1. **Buy Poly YES, Sell Kalshi YES** (if Poly ask < Kalshi bid): spread = Kalshi_bid_prob - Poly_ask_prob
2. **Sell Poly YES, Buy Kalshi YES** (if Kalshi ask < Poly bid): spread = Poly_bid_prob - Kalshi_ask_prob
3. **Buy Poly NO, Sell Kalshi NO** (complement of pattern 2): spread = (1 - Poly_ask_prob) - (1 - Kalshi_bid_prob)
4. **Sell Poly NO, Buy Kalshi NO** (complement of pattern 1): spread = (1 - Kalshi_ask_prob) - (1 - Poly_bid_prob)

Note: Patterns 3 and 4 are the algebraic complements of 1 and 2 respectively, but when walked through the book at different depth levels, the effective fill prices (and thus net spreads) will differ. Both must be computed independently.

**When to use:** Every spread computation for every event pair.

### Pattern 4: Polymarket Dynamic Fee Formula
**What:** Polymarket fees use: `fee = C * feeRate * (p * (1 - p))^exponent`
- Sports markets: feeRate=0.0175, exponent=1 (max ~0.44% at p=0.50)
- Crypto 5-min/15-min: feeRate=0.25, exponent=2 (max ~1.56% at p=0.50)
- Most prediction markets (non-crypto-short-term, non-sports): currently 0% taker fee
- US Exchange (DCM): 0.10% taker fee (10 bps)

**Implementation:** Store feeRate and exponent per market type in TOML. Default to the crypto-15min params (highest fee) for conservative modeling. Allow TOML override to flat rate for comparison testing.

```rust
use rust_decimal::Decimal;

struct PolymarketFeeParams {
    fee_rate: Decimal,    // e.g., 0.25 for crypto markets
    exponent: u32,        // 1 for sports, 2 for crypto
}

impl PolymarketFeeParams {
    fn compute_fee(&self, shares: Decimal, price: Decimal) -> Decimal {
        let p_complement = Decimal::ONE - price;
        let base = price * p_complement;
        let scaled = if self.exponent == 1 {
            base
        } else {
            base * base // exponent=2
        };
        shares * self.fee_rate * scaled
    }
}
```

### Pattern 5: Kalshi Fee Formula
**What:** Kalshi fees: `fee = ceil(0.07 * C * P * (1-P))` for takers, `ceil(0.0175 * C * P * (1-P))` for makers.
- Max taker fee at 50c: 1.75c per contract
- The user's CONTEXT specifies "7% profit fee" -- this aligns with the taker coefficient of 0.07

**Implementation:** Per the user's locked decision, model Kalshi as `0.07 * P * (1-P)` per contract (taker assumption for conservative modeling).

```rust
fn kalshi_taker_fee_per_contract(price_probability: Decimal) -> Decimal {
    let coefficient = Decimal::new(7, 2); // 0.07
    coefficient * price_probability * (Decimal::ONE - price_probability)
}
```

### Pattern 6: Rolling Statistics with Welford's Algorithm
**What:** Maintain running mean and variance using Welford's online algorithm. Use a `VecDeque` as a circular buffer to support windowed computation (drop old samples as window slides).
**When to use:** For dynamic threshold computation and aggregate stats emission.
**Example:**
```rust
use std::collections::VecDeque;

struct RollingStats {
    window: VecDeque<(f64, i64)>,  // (value, timestamp_ms)
    window_duration_ms: i64,
    sum: f64,
    sum_sq: f64,
}

impl RollingStats {
    fn push(&mut self, value: f64, timestamp_ms: i64) {
        // Evict expired entries
        let cutoff = timestamp_ms - self.window_duration_ms;
        while let Some(&(old_val, old_ts)) = self.window.front() {
            if old_ts < cutoff {
                self.sum -= old_val;
                self.sum_sq -= old_val * old_val;
                self.window.pop_front();
            } else {
                break;
            }
        }
        self.window.push_back((value, timestamp_ms));
        self.sum += value;
        self.sum_sq += value * value;
    }

    fn mean(&self) -> f64 {
        let n = self.window.len() as f64;
        if n == 0.0 { return 0.0; }
        self.sum / n
    }

    fn stddev(&self) -> f64 {
        let n = self.window.len() as f64;
        if n < 2.0 { return 0.0; }
        let variance = (self.sum_sq - self.sum * self.sum / n) / (n - 1.0);
        variance.max(0.0).sqrt()
    }
}
```

### Pattern 7: Prometheus Metrics Setup
**What:** Install `metrics-exporter-prometheus` PrometheusBuilder as the global recorder early in `main()`. This activates ALL existing `metrics::counter!`, `metrics::gauge!`, `metrics::histogram!` calls throughout the feed layer with zero code changes.
**When to use:** Once at startup, before any metrics are emitted.
**Example:**
```rust
use metrics_exporter_prometheus::PrometheusBuilder;

fn setup_prometheus(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        .set_buckets(&[
            0.0001, 0.0005, 0.001, 0.005, 0.01, 0.02, 0.05, 0.10, 0.20,
        ])?  // spread buckets in probability space
        .install()?;
    Ok(())
}
```

### Anti-Patterns to Avoid
- **Top-of-book only pricing:** The user explicitly decided walk-the-book for a configurable notional. Never use best bid/ask alone as the fill price estimate.
- **Folding basis risk into cost model:** User decision: basis risk is SEPARATE -- it's metadata/filter, not a cost component. `BasisRiskScore` annotates the signal but does not adjust the spread.
- **Signal deduplication in this phase:** User decision: fire every threshold crossing. Deduplication is downstream.
- **Instant fill assumption for paper trades:** User decision: next-tick entry, not signal-time fill.
- **Weekly aggregation in-process:** User decision: daily rollups in-process, weekly derived offline.
- **Rounding Kalshi fees down:** Kalshi uses ceiling (round up) on fee computation. Always `ceil()` the fee.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Prometheus scrape endpoint | Custom HTTP server for metrics | `metrics-exporter-prometheus` PrometheusBuilder with http-listener | Handles text format, content negotiation, metric types, histograms, upkeep |
| Decimal arithmetic | f64 for fee/spread math | `rust_decimal::Decimal` | Already used everywhere in codebase; prevents floating-point drift in financial calcs |
| Metrics facade | Direct Prometheus client calls | `metrics` crate macros (counter!, gauge!, histogram!) | Already used in 15+ places in feed layer; consistent API; recorder-agnostic |
| Daily file rotation | Custom rotate logic | Adapt existing `JsonlWriter` pattern from `feed/recording/writer.rs` | Already proven pattern with date-based rotation, append mode, BufWriter |

**Key insight:** The existing codebase already has the metrics facade wired up throughout the feed layer. Installing a Prometheus recorder activates everything. The spread computation module should use the same `metrics::*` macros, keeping instrumentation consistent.

## Common Pitfalls

### Pitfall 1: Probability Direction Confusion (Poly vs Kalshi)
**What goes wrong:** Computing spreads without accounting for how each venue represents YES/NO probabilities. Polymarket bid/ask probabilities are directly available as `bid_probability`/`ask_probability` on `MarketSnapshot`. Kalshi snapshots represent YES perspective (bid = YES bid, ask = derived YES ask from NO side). Confusing which side is YES and which is NO leads to inverted spread signals.
**Why it happens:** Different venue conventions for representing binary outcomes.
**How to avoid:** Always use `bid_probability` and `ask_probability` fields from `MarketSnapshot` -- both venues already normalize to YES probability space in their processors. When computing NO probability, use `Probability::complement()` (1 - p).
**Warning signs:** Spread values that are consistently negative or impossibly large.

### Pitfall 2: Walk-the-Book with Insufficient Depth
**What goes wrong:** Target notional exceeds available depth -- the walk fills partially and the computed average price is misleading.
**Why it happens:** Thin markets, especially for Kalshi where book depth can be sparse.
**How to avoid:** `walk_the_book` must return both average fill price AND filled quantity. If filled < target, signal a liquidity shortfall. This shortfall feeds into the liquidity penalty component of the dynamic threshold.
**Warning signs:** `filled_notional / target_notional < 1.0` consistently for certain events.

### Pitfall 3: Polymarket Fee Formula Exponent Handling
**What goes wrong:** Using exponent=1 when the market is a crypto short-term market (should be exponent=2), or vice versa. The max fee difference is 4x (0.44% vs 1.56% at p=0.5).
**Why it happens:** Polymarket has different fee tiers per market type, and most prediction markets have 0% fees.
**How to avoid:** Default to highest fee tier (conservative) unless market type is explicitly classified. Provide TOML override for flat rate comparison. Log which fee tier was applied per computation.
**Warning signs:** Spreads that appear profitable but only because fees were underestimated.

### Pitfall 4: Stale Data Creating False Spreads
**What goes wrong:** One venue's snapshot is from 30 seconds ago while the other is current. The "spread" is an artifact of stale data, not a real arbitrage.
**Why it happens:** Different venues update at different frequencies. Kalshi REST polling can have inherent latency.
**How to avoid:** Both legs MUST pass the staleness gate before spread computation. Check both `is_stale` flag AND the age of `exchange_timestamp` (or `timestamp.wall` for Kalshi which lacks exchange timestamps). Log rejected computations with the staleness reason.
**Warning signs:** Spikes in spread that correlate with one venue's feed going quiet.

### Pitfall 5: Decimal vs f64 Boundary
**What goes wrong:** Converting Decimal to f64 for rolling statistics or Prometheus metrics loses precision. Converting back incorrectly.
**Why it happens:** `metrics` crate records f64 values. Rolling stats use f64 for Welford's algorithm.
**How to avoid:** Do all fee/spread arithmetic in `Decimal`. Convert to f64 only at the metrics/stats boundary (one-way). Never convert f64 back to Decimal for financial decisions.
**Warning signs:** Accumulated rounding errors in aggregate P&L.

### Pitfall 6: Prometheus Recorder Must Be Installed Before Any Metric Emission
**What goes wrong:** If `PrometheusBuilder::install()` is called after feed tasks have already started emitting metrics, those early emissions are lost (they went to the no-op recorder).
**Why it happens:** The metrics facade uses a global recorder. If not installed before tasks start, the default no-op handles everything.
**How to avoid:** Call `PrometheusBuilder::new().install()` in `main()` BEFORE spawning any feed or spread tasks. This is a one-line change in main.rs.
**Warning signs:** Missing feed metrics in Prometheus output despite feeds being active.

### Pitfall 7: Paper Trade Next-Tick Timing
**What goes wrong:** Recording entry at signal-time price rather than next-tick price. This overstates profitability by ignoring adverse selection.
**Why it happens:** Simpler to just use the price available at signal time.
**How to avoid:** Store the signal as "pending entry" and fill on the next MarketSnapshot update for that instrument. The difference between signal-time price and fill price IS the adverse selection metric.
**Warning signs:** Paper trade P&L that is consistently better than would be achievable in practice.

### Pitfall 8: Rolling Window Cold Start
**What goes wrong:** Dynamic threshold has no data in the rolling window at startup, so it falls back to only the static floor. This may generate many false signals during the first 4 hours.
**Why it happens:** Window needs `window_duration` of data before statistics are meaningful.
**How to avoid:** During cold start (window samples < configurable minimum, e.g., 30), use only the static floor with a configurable multiplier (e.g., 2x floor). Log that the system is in cold-start mode. Transition to dynamic threshold once sufficient samples accumulate.
**Warning signs:** Burst of signals immediately after startup that don't recur once the window fills.

## Code Examples

### SpreadEngine Main Loop (Core Pattern)
```rust
// Core event loop consuming from fan-in channel
async fn run_spread_engine(
    mut snapshot_rx: mpsc::Receiver<MarketSnapshot>,
    registry: Arc<RwLock<EventRegistry>>,
    config: SpreadConfig,
    cancel: CancellationToken,
) {
    let mut state = SnapshotState::new();
    let mut stats: HashMap<String, RollingStats> = HashMap::new();
    let mut spread_logger = SpreadLogger::new(config.log_dir.clone());
    let mut paper_tracker = PaperTradeTracker::new(config.paper_trade.clone());

    let stats_interval = tokio::time::interval(
        Duration::from_secs(config.stats_emission_interval_secs)
    );
    tokio::pin!(stats_interval);

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => break,

            _ = stats_interval.tick() => {
                emit_aggregate_stats(&stats);
            }

            snapshot = snapshot_rx.recv() => {
                let Some(snap) = snapshot else { break };

                // 1. Look up event_id for this snapshot
                let registry = registry.read().await;
                let event_id = match registry.lookup_by_instrument(
                    snap.venue,
                    &snap.instrument_id.to_string(),
                ) {
                    Some(mapping) => mapping.id.clone(),
                    None => continue, // unmapped instrument
                };

                // 2. Update latest state
                state.update(&event_id, snap);

                // 3. Compute spreads for all 4 patterns
                if let Some((poly, kalshi)) = state.get_pair(
                    &event_id, Venue::Polymarket, Venue::Kalshi
                ) {
                    // Staleness gate: both must be fresh
                    if poly.is_stale || kalshi.is_stale {
                        // Log rejection
                        continue;
                    }

                    for pattern in SpreadPattern::all() {
                        let result = compute_spread(
                            pattern, poly, kalshi, &config.cost_model,
                        );
                        spread_logger.log(&result).await;

                        // Update rolling stats
                        let per_event_stats = stats
                            .entry(event_id.clone())
                            .or_insert_with(|| RollingStats::new(config.rolling_window_ms));
                        per_event_stats.push(
                            result.net_spread.to_f64().unwrap_or(0.0),
                            result.timestamp_ms,
                        );

                        // Check threshold
                        let threshold = compute_threshold(
                            per_event_stats, &config.threshold, &result,
                        );
                        if result.net_spread > threshold {
                            paper_tracker.signal(result.clone());
                            metrics::counter!("spread_signals_total",
                                "event" => event_id.clone(),
                                "pattern" => pattern.label(),
                            ).increment(1);
                        }

                        // Always record to Prometheus histogram
                        metrics::histogram!("spread_net",
                            "event" => event_id.clone(),
                        ).record(result.net_spread.to_f64().unwrap_or(0.0));
                    }
                }
            }
        }
    }
}
```

### Dynamic Threshold Computation
```rust
/// Threshold = max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty
fn compute_threshold(
    stats: &RollingStats,
    config: &ThresholdConfig,
    result: &SpreadResult,
) -> Decimal {
    let static_floor = config.static_floor;

    let dynamic = if stats.count() >= config.min_samples {
        let mean = Decimal::from_f64_retain(stats.mean()).unwrap_or(Decimal::ZERO);
        let stddev = Decimal::from_f64_retain(stats.stddev()).unwrap_or(Decimal::ZERO);
        mean + config.k * stddev
    } else {
        // Cold start: use elevated static floor
        static_floor * config.cold_start_multiplier
    };

    let base = static_floor.max(dynamic);
    let liquidity_penalty = compute_liquidity_penalty(result, config);

    base + liquidity_penalty
}

/// Inverse-depth penalty: higher penalty when book is thin
fn compute_liquidity_penalty(
    result: &SpreadResult,
    config: &ThresholdConfig,
) -> Decimal {
    let fill_ratio = result.filled_notional / result.target_notional;
    if fill_ratio >= Decimal::ONE {
        Decimal::ZERO  // full fill, no penalty
    } else {
        // Linear penalty: penalty_scale * (1 - fill_ratio)
        config.liquidity_penalty_scale * (Decimal::ONE - fill_ratio)
    }
}
```

### Prometheus Setup in main.rs
```rust
// Add early in main(), BEFORE any task spawning
fn setup_prometheus(port: u16) -> anyhow::Result<()> {
    use metrics_exporter_prometheus::{PrometheusBuilder, Matcher};

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        // Spread histogram buckets (probability-space: 0.0001 to 0.20)
        .set_buckets_for_metric(
            Matcher::Prefix("spread_".to_string()),
            &[0.0001, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.10, 0.20],
        )?
        // Feed latency buckets (milliseconds: 1ms to 10s)
        .set_buckets_for_metric(
            Matcher::Full("feed_latency_ms".to_string()),
            &[1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0, 10000.0],
        )?
        .install()?;

    tracing::info!(port = port, "Prometheus metrics exporter started");
    Ok(())
}
```

### TOML Configuration Structure
```toml
# Add to config.toml or new spread.toml

[spread]
target_notional = "500.0"           # Walk-the-book notional size ($USD)
stats_emission_interval_secs = 60   # Aggregate stats to stdout/metrics
rolling_window_secs = 14400         # 4 hours default
rolling_min_samples = 30            # Min samples before dynamic threshold activates

[spread.threshold]
static_floor_bps = 100              # 1% minimum (100 basis points)
k = 2.0                             # Standard deviations above mean
liquidity_penalty_scale = "0.02"    # Max penalty for empty book
cold_start_multiplier = "2.0"       # Floor multiplier during cold start

[spread.polymarket_fees]
fee_rate = "0.25"                   # Default to crypto-tier (conservative)
exponent = 2                        # Crypto markets use squared
flat_rate_override = ""             # Empty = use dynamic formula; e.g. "0.01" for 1% flat

[spread.kalshi_fees]
taker_coefficient = "0.07"          # Kalshi taker fee coefficient
use_ceiling = true                  # Round up per Kalshi convention

[spread.carry]
annualized_rate = "0.05"            # 5% annualized carry cost
reference_holding_days = 30         # Expected holding period for carry computation

[paper_trade]
notional_per_trade = "500.0"        # Fixed notional per paper trade
log_mtm = true                      # Log mark-to-market over position lifetime
log_dir = "paper_trades"            # Output directory for paper trade JSONL

[prometheus]
port = 9000                         # Prometheus scrape port
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Top-of-book spread | Walk-the-book with notional | Always better for real execution modeling | Prevents false signals from thin books with wide spreads |
| Static threshold only | Static + dynamic (distribution-based) | Standard in quant since ~2010 | Adapts to volatility regimes, reduces false signals in quiet markets |
| Polymarket 0% fees assumption | Dynamic fee formula per market type | Feb 2026 -- Polymarket introduced taker fees on crypto markets | Must model fees to avoid systematic positive bias in spread estimates |
| Kalshi flat 7% profit fee | Per-contract taker fee: `0.07 * P * (1-P)` | Current Kalshi fee schedule | Fee varies by price -- near-50/50 events cost more |

**Deprecated/outdated:**
- Polymarket having zero fees for all markets is no longer accurate; crypto short-term markets now carry taker fees
- The user-specified "Kalshi 7% profit fee" is best interpreted as the 0.07 taker coefficient in the `0.07 * P * (1-P)` formula, not a flat 7% on profits

## Open Questions

1. **Polymarket market type classification**
   - What we know: Fee tiers differ by market type (sports, crypto-short, prediction). Most prediction markets currently have 0% taker fee.
   - What's unclear: How to automatically classify a Polymarket market into its fee tier. No API field identifies market type for fee purposes.
   - Recommendation: Default to highest fee tier (conservative). Allow per-event TOML override. LOW priority -- the TOML override handles this adequately for v1.

2. **metrics-exporter-prometheus version compatibility**
   - What we know: Project has `metrics = "0.24"`. Version 0.18 of the exporter targets `metrics ^0.24`.
   - What's unclear: Whether `hyper 1.8` (transitive from prometheus exporter 0.18) conflicts with `reqwest 0.12`'s transitive hyper dependency.
   - Recommendation: Try `metrics-exporter-prometheus = "0.18"` first. If Cargo resolver has conflicts, try with `default-features = false, features = ["http-listener"]` to reduce transitive surface. If still blocked, use `install_recorder()` instead of `install()` (which avoids the HTTP listener and lets you serve metrics through a custom endpoint).

3. **Kalshi lack of exchange timestamps**
   - What we know: Kalshi orderbook messages do not include exchange-reported timestamps. Staleness is currently `is_stale = false` always (set in `kalshi/normalize.rs` line 224).
   - What's unclear: Whether Kalshi REST polling latency is consistent enough that local receipt time is a reliable staleness proxy.
   - Recommendation: Use `timestamp.wall` age (local receipt time) for Kalshi staleness gating. Add a Kalshi-specific configurable staleness threshold that is more permissive (e.g., 15s instead of 5s) to account for REST polling inherent delay.

4. **Event ID annotation on snapshots**
   - What we know: `MarketSnapshot.event_id` is `Option<EventId>` and is currently `None` on all snapshots. The pipeline comment says "Mapped in Phase 5 (cross-venue event mapping)" but Phase 5 focused on registry/lifecycle, not snapshot annotation.
   - What's unclear: Whether to annotate snapshots at pipeline level (in the processor or forwarder) or at the spread engine level.
   - Recommendation: Annotate at the spread engine level via `EventRegistry::lookup_by_instrument()`. This avoids modifying existing pipeline code. The SpreadEngine already needs the registry for pairing, so the lookup is natural there.

## Sources

### Primary (HIGH confidence)
- `metrics-rs/metrics` Context7 library ID - PrometheusBuilder setup, histogram buckets, counter/gauge/histogram macros, recorder installation
- [docs.rs/metrics-exporter-prometheus](https://docs.rs/metrics-exporter-prometheus) - Version 0.18.1 confirmed compatible with metrics ^0.24, requires hyper ^1.8, features http-listener default
- [Polymarket Trading Fees docs](https://docs.polymarket.com/polymarket-learn/trading/fees) - Exact fee formula: `fee = C * feeRate * (p * (1-p))^exponent`, two tiers (sports: 0.0175/1, crypto: 0.25/2)

### Secondary (MEDIUM confidence)
- [Kalshi fee formula](https://whirligigbear.substack.com/p/makertaker-math-on-kalshi) - Taker: `0.07 * C * P * (1-P)`, Maker: `0.0175 * C * P * (1-P)`, verified against multiple sources
- [Kalshi Help Center fees](https://help.kalshi.com/trading/fees) - Confirms transaction fee on expected earnings
- [rolling-stats crate](https://crates.io/crates/rolling-stats) - Welford's online algorithm reference for rolling statistics

### Tertiary (LOW confidence)
- Polymarket US DCM 0.10% taker fee -- applies to US exchange only, unclear if relevant to CLOB API
- Polymarket maker rebate program -- funded by taker fees, may affect fee modeling but not directly

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - `metrics` 0.24 already in project, `metrics-exporter-prometheus` 0.18 confirmed compatible
- Architecture: HIGH - All data structures exist (`MarketSnapshot`, `EventRegistry`, depth data), patterns are straightforward pub/sub
- Fee formulas: MEDIUM - Polymarket formula verified from official docs; Kalshi formula verified from multiple sources but official PDF was inaccessible
- Pitfalls: HIGH - Based on direct codebase analysis (staleness flags, Decimal types, venue normalization differences)
- Paper trade design: MEDIUM - Standard approach but next-tick fill requires careful state management

**Research date:** 2026-02-23
**Valid until:** 2026-03-23 (fee structures may change; check venue docs before deploying)
