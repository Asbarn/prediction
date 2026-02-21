# Architecture Research

**Domain:** Cross-venue crypto prediction market / options arbitrage system
**Researched:** 2026-02-21
**Confidence:** HIGH

## Standard Architecture

### System Overview

```
                          EXTERNAL VENUES
 ===================================================================
  Deribit WS         Polymarket WS/REST       Kalshi WS/REST
  (Options)          (Prediction Market)      (Prediction Market)
 ===================================================================
       |                     |                       |
       v                     v                       v
 +-----------+        +-----------+           +-----------+
 | Deribit   |        | Polymarket|           | Kalshi    |
 | Feed      |        | Feed      |           | Feed      |
 | Actor     |        | Actor     |           | Actor     |
 +-----------+        +-----------+           +-----------+
       |                     |                       |
       | MarketSnapshot      | MarketSnapshot        | MarketSnapshot
       | (bounded mpsc)      | (bounded mpsc)        | (bounded mpsc)
       |                     |                       |
 ======|=====================|=======================|===============
       v                     v                       v
 +-------------------------------------------------------------------+
 |                    NORMALIZATION BUS                               |
 |  Fan-in: tokio::select! over all feed channels                    |
 |  Attaches: receive timestamp, sequence number, venue tag          |
 |  Publishes: NormalizedSnapshot to broadcast channel               |
 +-------------------------------------------------------------------+
       |
       | NormalizedSnapshot (tokio::broadcast)
       |
       +---------------------------+---------------------------+
       |                           |                           |
       v                           v                           v
 +-----------+             +--------------+            +-------------+
 | Event     |             | Pricing      |            | Telemetry   |
 | Mapping & |             | Engine       |            | Collector   |
 | Reconcil. |             | (pure sync)  |            |             |
 +-----------+             +--------------+            +-------------+
       |                           |
       | MappedPair                | PricedOutcome
       | (bounded mpsc)            | (bounded mpsc)
       |                           |
       +-------------+-------------+
                     |
                     v
            +-----------------+
            | Signal          |
            | Generator       |
            | (spread calc,   |
            |  cost adjust,   |
            |  staleness)     |
            +-----------------+
                     |
                     | Signal (bounded mpsc)
                     v
          +---------------------+
          | Signal Router       |
          | - Log/Record        |
          | - Prometheus metric |
          | - (v2: Execution)   |
          +---------------------+
                     |
                     | (v2: OrderRequest)
                     v
          +---------------------+        +------------------+
          | Execution Engine    | -----> | Risk Manager     |
          | (v2: trait impl)    |        | (v2: position    |
          +---------------------+        |  limits, Greeks) |
                                         +------------------+

 ===================================================================
                        CROSS-CUTTING CONCERNS
 ===================================================================
  +------------------+   +------------------+   +------------------+
  | Config (TOML)    |   | Telemetry        |   | Clock / Time     |
  | hot-reload via   |   | tracing spans    |   | Instant-based    |
  | watch channel    |   | Prometheus /     |   | monotonic for    |
  |                  |   | metrics          |   | latency; wall    |
  |                  |   | feed recording   |   | clock for stamps |
  +------------------+   +------------------+   +------------------+
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| **Feed Actors** (src/feeds/) | Maintain WebSocket/REST connections to each venue, parse venue-specific wire formats, produce normalized `MarketSnapshot` events | One tokio task per venue; owns reconnection logic, heartbeat, auth; uses `tokio-tungstenite` for WS |
| **Normalization Bus** | Fan-in from all feeds, attach metadata (receive timestamp, sequence), fan-out to downstream consumers | Single task with `tokio::select!` over feed receivers; publishes on `tokio::broadcast` channel |
| **Event Mapping** (src/events/) | Map equivalent instruments across venues (e.g., Polymarket "BTC > $100k" = Deribit BTC-100000-C), maintain instrument registry | Registry data structure keyed by canonical event ID; settlement basis analyzer compares contract terms |
| **Pricing Engine** (src/pricing/) | IV solving (Newton-Raphson/Brent), Black-76 pricing, probability extraction (N(d2), call spread, smile interpolation), Greeks computation | Pure synchronous functions, no async; operates on `rust_decimal`; called inline by signal generator |
| **Signal Generator** (src/signals/) | Calculate cross-venue spreads, apply cost adjustments (fees, slippage), detect stale data, fire signals when thresholds breached | Consumes `MappedPair` + `PricedOutcome`; uses configurable thresholds from TOML config |
| **Signal Router** | Route signals to sinks: logging, metrics, recording, and (v2) execution | Fan-out via broadcast or direct calls to observer trait objects |
| **Execution Engine** (src/execution/) | v2 -- submit orders, manage fills, handle venue-specific order types | Trait interface designed in v1; concrete implementations in v2 |
| **Risk Manager** (src/risk/) | v2 -- position limits, Greeks exposure limits, P&L tracking | Trait interface designed in v1; concrete implementations in v2 |
| **Telemetry** (src/telemetry/) | Structured logging, Prometheus metrics, span-based tracing, feed recording for replay | `tracing` crate + `tracing-subscriber`; `metrics` crate with Prometheus exporter |
| **Config** (src/config/) | Load and distribute TOML configuration; support hot-reload for thresholds | `tokio::sync::watch` channel for runtime config updates without restart |

## Recommended Project Structure

```
src/
├── main.rs                    # Runtime bootstrap, task spawning, graceful shutdown
├── config/
│   ├── mod.rs                 # Config module root
│   ├── types.rs               # AppConfig, FeedConfig, PricingConfig, SignalConfig structs
│   └── loader.rs              # TOML parsing, validation, watch-based hot reload
├── feeds/
│   ├── mod.rs                 # Feed trait, MarketSnapshot type, common types
│   ├── common.rs              # Shared WS utilities: reconnect, heartbeat, auth
│   ├── deribit.rs             # Deribit WS feed actor
│   ├── polymarket.rs          # Polymarket WS/REST feed actor
│   ├── kalshi.rs              # Kalshi WS/REST feed actor
│   └── normalizer.rs          # Fan-in bus, timestamp attachment, broadcast publisher
├── events/
│   ├── mod.rs                 # Event mapping module root
│   ├── registry.rs            # Canonical event ID registry, cross-venue instrument map
│   ├── settlement.rs          # Settlement basis comparison (binary vs option payoff)
│   └── types.rs               # MappedPair, CanonicalEvent, VenueInstrument
├── pricing/
│   ├── mod.rs                 # Pricing module root
│   ├── black76.rs             # Black-76 model implementation
│   ├── iv_solver.rs           # Newton-Raphson + Brent method IV solver
│   ├── probability.rs         # N(d2), call spread replication, smile interpolation
│   ├── greeks.rs              # Delta, gamma, theta, vega computations
│   └── types.rs               # PricedOutcome, ImpliedVol, Greeks structs
├── signals/
│   ├── mod.rs                 # Signal generation module root
│   ├── spread.rs              # Cross-venue spread calculator
│   ├── cost.rs                # Fee, slippage, and funding cost adjustments
│   ├── staleness.rs           # Data freshness detection and circuit breaking
│   ├── threshold.rs           # Configurable threshold engine
│   └── types.rs               # Signal, SpreadResult, CostAdjustment
├── execution/
│   ├── mod.rs                 # Execution module root (v1: trait only)
│   └── traits.rs              # ExecutionEngine trait, OrderRequest, OrderResponse
├── risk/
│   ├── mod.rs                 # Risk module root (v1: trait only)
│   └── traits.rs              # RiskManager trait, PositionLimit, ExposureLimit
├── telemetry/
│   ├── mod.rs                 # Telemetry module root
│   ├── metrics.rs             # Prometheus counters, histograms, gauges
│   ├── tracing.rs             # Structured tracing setup, span management
│   └── recorder.rs            # Feed recording for replay/debugging
├── types/
│   ├── mod.rs                 # Shared domain types
│   ├── decimal.rs             # rust_decimal wrappers, arithmetic helpers
│   ├── venue.rs               # Venue enum, VenueId
│   └── timestamp.rs           # Timestamp types, monotonic vs wall-clock
└── error.rs                   # Unified error types across modules
```

### Structure Rationale

- **feeds/**: Each venue is its own file because venue APIs are wildly different (Deribit WS subscriptions vs Polymarket CLOB WS vs Kalshi REST+WS). The `common.rs` extracts shared reconnection/heartbeat logic. The `normalizer.rs` is the fan-in point that decouples venue-specific code from downstream consumers.
- **events/**: Separated from feeds because mapping is a domain concern (which instruments are equivalent), not an ingestion concern. The registry is the single source of truth for cross-venue relationships.
- **pricing/**: Pure computational module with zero async. Deliberately isolated so it can be unit-tested with deterministic inputs without needing a runtime. All functions take `rust_decimal` values and return `rust_decimal` results.
- **signals/**: Orchestrates the combination of mapped pairs + pricing + costs into actionable signals. Staleness detection lives here because it is a signal-quality concern, not a feed concern.
- **types/**: Shared types used across module boundaries. Prevents circular dependencies by being a leaf module that other modules depend on but that depends on nothing internal.

## Architectural Patterns

### Pattern 1: Pipeline of Async Actors Connected by Bounded Channels

**What:** Each major system component runs as an independent tokio task (actor). Components communicate exclusively through bounded `tokio::sync::mpsc` channels. Each actor has a simple loop: receive from input channel, process, send to output channel.

**When to use:** Whenever two components have a producer-consumer relationship with different processing rates.

**Trade-offs:**
- PRO: Natural backpressure -- if the pricing engine is slow, the channel fills and feeds slow their publish rate
- PRO: Component isolation -- each actor can crash and be restarted independently
- PRO: Testability -- inject a channel sender/receiver to test each stage in isolation
- CON: Latency overhead of channel send/recv (~50-200ns per hop)
- CON: Debugging data flow requires correlating across task boundaries

**Example:**
```rust
use tokio::sync::mpsc;

pub struct FeedActor {
    output: mpsc::Sender<MarketSnapshot>,
    config: FeedConfig,
}

impl FeedActor {
    pub async fn run(self, shutdown: CancellationToken) -> Result<()> {
        let mut ws = self.connect().await?;
        loop {
            tokio::select! {
                biased;  // Check shutdown first
                _ = shutdown.cancelled() => {
                    tracing::info!("feed shutting down gracefully");
                    return Ok(());
                }
                msg = ws.next() => {
                    match msg {
                        Some(Ok(frame)) => {
                            let snapshot = self.parse(frame)?;
                            // Bounded send -- applies backpressure if downstream is slow
                            if self.output.send(snapshot).await.is_err() {
                                tracing::warn!("downstream dropped, shutting down");
                                return Ok(());
                            }
                        }
                        Some(Err(e)) => {
                            tracing::error!(?e, "ws error, reconnecting");
                            ws = self.reconnect_with_backoff().await?;
                        }
                        None => {
                            tracing::warn!("ws closed, reconnecting");
                            ws = self.reconnect_with_backoff().await?;
                        }
                    }
                }
            }
        }
    }
}
```

### Pattern 2: Broadcast Fan-Out for Multi-Consumer Data

**What:** Use `tokio::sync::broadcast` when multiple independent consumers all need every message (e.g., telemetry, signal generator, and event mapper all need every normalized snapshot). Unlike mpsc which is point-to-point, broadcast lets any number of receivers subscribe.

**When to use:** When the normalization bus publishes snapshots that multiple downstream components need simultaneously.

**Trade-offs:**
- PRO: Decouples publisher from subscriber count -- add new consumers without modifying publisher
- PRO: Each receiver gets its own independent read pointer
- CON: No backpressure -- slow receivers get `RecvError::Lagged` and miss messages
- CON: All messages are cloned for each receiver (use `Arc<T>` to avoid deep clones)

**Example:**
```rust
use tokio::sync::broadcast;
use std::sync::Arc;

// Wrap in Arc to avoid cloning large snapshots
let (tx, _) = broadcast::channel::<Arc<NormalizedSnapshot>>(256);

// Publisher (normalizer bus)
let snapshot = Arc::new(normalized);
let _ = tx.send(snapshot);  // Returns Err only if zero receivers

// Consumer 1: Signal generator
let mut rx_signals = tx.subscribe();
tokio::spawn(async move {
    loop {
        match rx_signals.recv().await {
            Ok(snap) => process_for_signals(&snap).await,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "signal gen lagged, skipping stale data");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
});

// Consumer 2: Telemetry recorder
let mut rx_telemetry = tx.subscribe();
tokio::spawn(async move {
    while let Ok(snap) = rx_telemetry.recv().await {
        record_snapshot(&snap).await;
    }
});
```

### Pattern 3: Watch Channel for Latest-Value State (Config, Snapshots)

**What:** Use `tokio::sync::watch` when consumers only care about the most recent value, not every intermediate update. The watch channel retains only the last sent value; receivers see the latest state when they check.

**When to use:** Configuration hot-reload (only the current config matters), latest market state snapshot for on-demand queries, current system health status.

**Trade-offs:**
- PRO: Zero message accumulation -- no backpressure needed because intermediates are irrelevant
- PRO: Readers never block the writer
- CON: Not suitable when every message must be processed (use mpsc or broadcast instead)

**Example:**
```rust
use tokio::sync::watch;

// Config hot-reload
let (config_tx, config_rx) = watch::channel(initial_config);

// Config reloader task
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        if let Ok(new_config) = reload_config_from_disk().await {
            let _ = config_tx.send(new_config);
        }
    }
});

// Consumer: Signal generator reads latest config
async fn check_threshold(config_rx: &watch::Receiver<AppConfig>, spread: Decimal) -> bool {
    let config = config_rx.borrow();
    spread >= config.signals.min_spread_threshold
}
```

### Pattern 4: Graceful Shutdown via CancellationToken Tree

**What:** Use `tokio_util::sync::CancellationToken` to coordinate shutdown across all tasks. Create a root token and derive child tokens for subsystems. Cancelling the root cascades to all children. Each task uses `tokio::select!` to race its work against the cancellation signal.

**When to use:** Always. Every long-running task must respect shutdown signals.

**Trade-offs:**
- PRO: Hierarchical -- cancel the "feeds" subtree without stopping signal generation
- PRO: Composable with `tokio::select!`
- PRO: No channel overhead -- just an atomic flag check

**Example:**
```rust
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let root_token = CancellationToken::new();

    // Subsystem tokens
    let feeds_token = root_token.child_token();
    let pricing_token = root_token.child_token();
    let signals_token = root_token.child_token();

    // Signal handler
    let shutdown_token = root_token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("shutdown signal received");
        shutdown_token.cancel();
    });

    // Spawn subsystems with their tokens
    let feeds_handle = tokio::spawn(run_feeds(feeds_token));
    let signals_handle = tokio::spawn(run_signals(signals_token));

    // Wait for all to complete
    let _ = tokio::try_join!(feeds_handle, signals_handle);
    tracing::info!("all subsystems shut down");
    Ok(())
}
```

### Pattern 5: Trait-Based Abstraction for Deferred Components

**What:** Define trait interfaces for components that will be implemented later (Execution, Risk). This locks in the contract between components early while deferring implementation complexity.

**When to use:** v2 components (Execution Engine, Risk Manager) that need their interfaces designed in v1 so that v1 components (Signal Generator) can be written against them.

**Trade-offs:**
- PRO: v1 code compiles and tests against the trait, not a concrete type
- PRO: Can provide a `NoOpExecution` and `NoOpRisk` for v1 that log signals without acting
- CON: Trait design might need revision when implementing; accept this cost

**Example:**
```rust
#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    async fn submit_order(&self, request: OrderRequest) -> Result<OrderResponse>;
    async fn cancel_order(&self, order_id: OrderId) -> Result<()>;
    async fn get_positions(&self) -> Result<Vec<Position>>;
}

// v1: No-op implementation that logs
pub struct LogOnlyExecution;

#[async_trait]
impl ExecutionEngine for LogOnlyExecution {
    async fn submit_order(&self, request: OrderRequest) -> Result<OrderResponse> {
        tracing::info!(?request, "SIGNAL: would submit order");
        Ok(OrderResponse::simulated(request))
    }
    // ...
}
```

## Data Flow

### Primary Data Flow: Market Data to Signal

```
Deribit WS ──────┐
                  │  Raw venue-specific messages
Polymarket WS ───┤
                  │
Kalshi WS/REST ──┘
                  │
                  v
          [Feed Actors]
          Parse venue wire format into MarketSnapshot {
              venue: Venue,
              instrument: VenueInstrument,
              bid/ask/last: Decimal,
              timestamp: Instant,
              raw_ts: SystemTime,
          }
                  │
                  │ bounded mpsc (per feed, capacity ~64)
                  v
          [Normalization Bus]
          Attach receive_ts (Instant::now()), sequence_id
          Emit NormalizedSnapshot
                  │
                  │ broadcast (capacity ~256)
                  │
         +--------+--------+
         |                  |
         v                  v
  [Event Mapper]      [Telemetry]
  Lookup canonical     Record raw
  event, pair with     snapshots
  cross-venue match    for replay
         |
         │ bounded mpsc (capacity ~32)
         v
  [Pricing Engine]  <-- called synchronously by Signal Generator
  Compute IV, probability, Greeks
         |
         │ (inline return, not a channel -- pure function)
         v
  [Signal Generator]
  Compare probabilities across venues
  Subtract costs (fees, slippage estimates)
  Check staleness (reject if data age > threshold)
  Fire Signal if spread > threshold
         |
         │ bounded mpsc (capacity ~16)
         v
  [Signal Router]
  Log structured signal
  Increment Prometheus counters
  (v2: forward to Execution Engine)
```

### Timing and Staleness Flow

```
Feed receives msg at T0 (Instant::now())
    |
    v
Normalizer attaches receive_ts = T0, publishes at T1
    |
    v
Signal Generator receives at T2
    |
    v
Staleness check: (T2 - T0) > max_age_ms?
    YES --> discard, increment stale_data counter
    NO  --> proceed to spread calculation
    |
    v
Cross-venue staleness: |venue_A.receive_ts - venue_B.receive_ts| > max_skew_ms?
    YES --> flag as uncertain, widen required spread threshold
    NO  --> proceed with normal threshold
```

### Key Data Types Moving Between Components

```
MarketSnapshot          Feed Actor --> Normalizer
  .venue: Venue
  .instrument_id: String
  .bid: Decimal
  .ask: Decimal
  .last: Option<Decimal>
  .volume: Option<Decimal>
  .feed_ts: Instant           // when we received from wire

NormalizedSnapshot      Normalizer --> broadcast subscribers
  .snapshot: MarketSnapshot
  .receive_ts: Instant         // normalizer receive time
  .seq: u64                    // monotonic sequence number

MappedPair              Event Mapper --> Signal Generator
  .canonical_event: CanonicalEventId
  .venue_a: NormalizedSnapshot  // e.g., Polymarket
  .venue_b: NormalizedSnapshot  // e.g., Deribit
  .settlement_match: SettlementBasis

PricedOutcome           Pricing Engine (inline) --> Signal Generator
  .probability_a: Decimal      // implied probability from venue A
  .probability_b: Decimal      // implied probability from venue B
  .iv: Option<Decimal>         // implied vol (options venue only)
  .greeks: Option<Greeks>

Signal                  Signal Generator --> Signal Router
  .canonical_event: CanonicalEventId
  .raw_spread: Decimal         // probability difference
  .net_spread: Decimal         // after costs
  .direction: Direction        // BuyA_SellB or BuyB_SellA
  .confidence: SignalConfidence
  .timestamp: Instant
  .staleness_flag: bool
```

## Scaling Considerations

| Concern | 3 Venues (v1) | 10 Venues | 50+ Instruments |
|---------|---------------|-----------|-----------------|
| **Feed connections** | 3 WS tasks, trivial | 10 WS tasks, still trivial on tokio | Same tasks, more subscriptions per connection |
| **Normalization throughput** | Single task easily handles ~10K msgs/sec | Single task still fine at ~100K msgs/sec | Consider sharding normalizer by venue or instrument group |
| **Event mapping lookups** | HashMap with ~50 entries, nanosecond lookup | HashMap with ~500 entries, still nanosecond | Consider grouping by asset class |
| **Pricing compute** | Inline sync calls, <100us per pricing | Same -- pricing is pure CPU, not I/O bound | If CPU-bound, spawn_blocking or dedicated thread pool |
| **Signal generation** | Single task, <1ms end-to-end | Same task, more pairs to compare | Parallelize across instrument groups if latency degrades |
| **Broadcast channel lag** | 256 buffer more than sufficient | May need 1024 buffer | Monitor `RecvError::Lagged` counter; increase buffer or add filtering |

### Scaling Priorities

1. **First bottleneck: WebSocket feed reliability.** Venue disconnections, rate limits, and malformed data will cause issues before any throughput concern. Invest heavily in reconnection logic with exponential backoff and circuit breakers.
2. **Second bottleneck: Event mapping staleness.** As more instruments are tracked, the probability of one side of a pair going stale increases. Staleness detection must be robust before scaling instrument count.
3. **Third bottleneck: Pricing compute under load.** If tracking 50+ options instruments with IV solving, Newton-Raphson iterations could consume meaningful CPU. Solution: `tokio::task::spawn_blocking` for pricing or a dedicated compute thread.

## Anti-Patterns

### Anti-Pattern 1: Shared Mutable State via Arc<Mutex<T>>

**What people do:** Share market data between tasks using `Arc<Mutex<HashMap<InstrumentId, Snapshot>>>`, with feeds writing and signal generators reading under the same lock.

**Why it's wrong:** Mutex contention under high message rates causes latency spikes. A slow consumer holding the read lock blocks feed updates. Priority inversion between time-critical feeds and less-critical telemetry.

**Do this instead:** Channel-based message passing. Each consumer gets its own copy of the data via broadcast or mpsc. The pricing engine, if it needs latest state, uses a `watch` channel that only retains the most recent value.

### Anti-Pattern 2: Unbounded Channels Everywhere

**What people do:** Use `tokio::sync::mpsc::unbounded_channel()` because it is simpler (no capacity to choose, no `.await` on send).

**Why it's wrong:** If any downstream consumer stalls (e.g., network hiccup in telemetry export), messages queue without bound. Memory grows until OOM kills the process. In a trading system, this is catastrophic -- you lose all state and open positions.

**Do this instead:** Always use bounded channels. Choose capacity based on expected burst size: `feeds -> normalizer` = 64 (short bursts during volatile markets), `normalizer -> broadcast` = 256 (multiple consumers at different speeds), `signals -> router` = 16 (signals are infrequent). Log warnings when channels are >75% full.

### Anti-Pattern 3: Async in the Pricing Engine

**What people do:** Make the IV solver and Black-76 functions `async fn` because "everything else is async."

**Why it's wrong:** Pricing is pure computation -- CPU-bound, not I/O-bound. Making it async adds the overhead of future state machines and poll cycles for zero benefit. Worse, a tight Newton-Raphson loop that does not yield will starve other tasks on the same executor thread.

**Do this instead:** Keep pricing as synchronous `fn` calls. If a single pricing call takes >1ms (e.g., complex smile interpolation), wrap in `tokio::task::spawn_blocking` to move it off the async executor.

### Anti-Pattern 4: Single Monolithic Event Enum

**What people do:** Create one giant `enum Event { MarketData(...), Signal(...), Config(...), Shutdown, ... }` and route everything through a single channel.

**Why it's wrong:** Every consumer must match against all variants, most of which are irrelevant. Type safety is lost -- the compiler cannot prevent sending a Signal to the feed actor. Adding a new variant requires touching every consumer's match arm.

**Do this instead:** Separate typed channels for separate concerns. `mpsc::Sender<MarketSnapshot>` for feeds, `mpsc::Sender<Signal>` for signals. The type system enforces correct routing at compile time.

### Anti-Pattern 5: Ignoring Lag in Broadcast Channels

**What people do:** Use `broadcast::Receiver::recv()` and silently ignore `RecvError::Lagged`, or worse, panic on it.

**Why it's wrong:** Lagged messages mean your signal generator missed market data. In arbitrage, missing data means potentially acting on stale information -- the exact thing the system should prevent.

**Do this instead:** Log lagged events with the count of missed messages. Increment a Prometheus counter. If lag exceeds a threshold, trigger a circuit breaker that pauses signal generation until data catches up. Consider increasing broadcast buffer capacity if lag is frequent.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| **Deribit** | WebSocket (wss://www.deribit.com/ws/api/v2) | Separate connections for market data vs trading (their recommendation). Subscribe to `ticker.{instrument}.100ms`, `book.{instrument}.100ms`. Auth via client_id/client_secret. Rate limit: respect API Usage Policy. Existing Rust crate: `deribit` (v0.3.3, somewhat stale) -- likely need custom implementation. |
| **Polymarket** | WebSocket (wss://ws-subscriptions-clob.polymarket.com/ws/market) + REST CLOB API | Official Rust client: `rs-clob-client`. WS for real-time orderbook updates, REST for market discovery and order submission. USDC settlement. CLOB heartbeat: if client disconnects, open orders cancelled. |
| **Kalshi** | WebSocket (wss://trading-api.kalshi.com/v1/ws) + REST API | Rust crate: `kalshi` on crates.io. REST for market discovery, WS for orderbook streaming. RSA-PSS signed authentication. CFTC-regulated -- stricter API terms. |
| **Prometheus** | HTTP scrape endpoint (:9090/metrics) | Use `metrics` crate with `metrics-exporter-prometheus`. Expose histograms for latency, counters for messages/signals, gauges for connection status. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| Feed Actor --> Normalizer | bounded mpsc (capacity 64) | One channel per feed. Backpressure on feed if normalizer is slow. |
| Normalizer --> All Consumers | broadcast (capacity 256) | `Arc<NormalizedSnapshot>` to avoid cloning. Consumers must handle `Lagged`. |
| Event Mapper --> Signal Generator | bounded mpsc (capacity 32) | Only mapped pairs flow here -- lower volume than raw snapshots. |
| Signal Generator --> Signal Router | bounded mpsc (capacity 16) | Signals are rare events -- small buffer is fine. |
| Config Loader --> All Consumers | watch channel | Latest-value semantics. Consumers `borrow()` current config on demand. |
| All Tasks --> Shutdown | CancellationToken tree | Root token with child tokens per subsystem. |

## Build Order (Dependencies Between Components)

The following build order respects data-flow dependencies -- each phase can be tested end-to-end before building the next.

### Phase 1: Foundation

Build first because everything depends on these:
- **types/** -- `Venue`, `Decimal` wrappers, `Timestamp`, `MarketSnapshot`, `NormalizedSnapshot`
- **config/** -- `AppConfig` struct, TOML loader, validation
- **error.rs** -- Unified error types
- **telemetry/** -- `tracing` setup, basic Prometheus metrics skeleton

**Why first:** Every other module imports types and uses tracing. Getting these right prevents cascading refactors.

### Phase 2: Market Data Ingestion

Build second because signal generation needs data to consume:
- **feeds/common.rs** -- WebSocket reconnection, heartbeat, backoff utilities
- **feeds/deribit.rs** -- Deribit feed actor (start here: best-documented API, existing Rust crate for reference)
- **feeds/normalizer.rs** -- Fan-in bus, broadcast publisher

**Why second:** The feed layer is the most complex integration (external APIs, reconnection, auth). Getting one feed working end-to-end proves the pipeline architecture.

### Phase 3: Event Mapping

Build third because pricing and signals need mapped pairs:
- **events/types.rs** -- `CanonicalEventId`, `MappedPair`, `SettlementBasis`
- **events/registry.rs** -- Cross-venue instrument map (initially config-driven, hardcoded mappings)
- **events/settlement.rs** -- Settlement basis analyzer

**Why third:** This is domain-specific logic with no external dependencies. Can be tested with synthetic data from Phase 2.

### Phase 4: Pricing Engine

Build fourth because signals need priced outcomes:
- **pricing/black76.rs** -- Black-76 forward pricing
- **pricing/iv_solver.rs** -- Newton-Raphson and Brent method
- **pricing/probability.rs** -- N(d2), call spread replication
- **pricing/greeks.rs** -- Delta, gamma, theta, vega

**Why fourth:** Pure math, no async, no external dependencies. Can be exhaustively unit-tested with known analytical solutions. Independent of everything except `rust_decimal`.

### Phase 5: Signal Generation + Remaining Feeds

Build fifth -- this is where the system becomes functional:
- **signals/** -- Spread calculator, cost adjustments, staleness detection, threshold engine
- **feeds/polymarket.rs** -- Second feed
- **feeds/kalshi.rs** -- Third feed
- **execution/traits.rs** -- Trait interface + `LogOnlyExecution` no-op
- **risk/traits.rs** -- Trait interface + `NoOpRisk`

**Why fifth:** Signal generation is the primary v1 deliverable. Adding remaining feeds here means the full pipeline can be tested. Trait stubs for execution/risk allow the signal router to be complete.

### Phase 6: Hardening

Build last:
- **telemetry/recorder.rs** -- Feed recording for replay
- Config hot-reload via watch channel
- Integration tests with recorded feed data
- Prometheus dashboard definitions

**Why last:** These are operational concerns that improve reliability but do not change core functionality.

## Sources

- [kucoin_arbitrage: Event-Driven Async Rust Arbitrage](https://github.com/kanekoshoyu/kucoin_arbitrage) -- reference architecture for event-driven crypto arbitrage in Rust with broadcast channels and JoinSet task management (HIGH confidence)
- [barter-rs: Rust Trading Framework](https://github.com/barter-rs/barter-rs) -- modular event-driven trading engine with separate crates for data, execution, and instruments (HIGH confidence)
- [Tokio Tutorial: Channels](https://tokio.rs/tokio/tutorial/channels) -- official documentation for mpsc, broadcast, watch, oneshot channel patterns (HIGH confidence)
- [Tokio Tutorial: Select](https://tokio.rs/tokio/tutorial/select) -- official documentation for tokio::select! fan-in pattern (HIGH confidence)
- [Tokio: Graceful Shutdown](https://tokio.rs/tokio/topics/shutdown) -- official CancellationToken and shutdown patterns (HIGH confidence)
- [Async Pipeline Pattern in Rust](https://github.com/alexpusch/rust-magic-patterns/blob/master/async-pipeline-pattern/Readme.md) -- bounded channel pipeline with backpressure (HIGH confidence)
- [Deribit API Documentation](https://docs.deribit.com/) -- WebSocket subscription channels, best practices for market data collection (HIGH confidence)
- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- WebSocket API for CLOB real-time data (HIGH confidence)
- [Polymarket rs-clob-client](https://github.com/Polymarket/rs-clob-client) -- Official Rust CLOB client (MEDIUM confidence -- AI-generated port, review before production use)
- [Kalshi API Documentation](https://docs.kalshi.com/welcome) -- REST + WebSocket API, RSA-PSS auth (HIGH confidence)
- [Deribit Market Data Best Practices](https://support.deribit.com/hc/en-us/articles/29592500256669-Market-Data-Collection-Best-Practices) -- separate connections for data vs trading (HIGH confidence)
- [tokio::sync::watch](https://docs.rs/tokio/latest/tokio/sync/watch/index.html) -- watch channel for latest-value state sharing (HIGH confidence)
- [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) -- WebSocket client for async Rust (HIGH confidence)
- [Rust Tokio Task Cancellation Patterns](https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/) -- CancellationToken patterns and pitfalls (MEDIUM confidence)
- [OpenTelemetry Rust](https://opentelemetry.io/docs/languages/rust/) -- tracing and metrics integration (MEDIUM confidence -- Rust support still in beta for traces)
- [Rust Forum: broadcast vs mpsc fan-out](https://users.rust-lang.org/t/switch-from-mpsc-to-broadcast-channel-or-use-arc-rwlock-to-share-data/114152/4) -- community discussion on channel selection (MEDIUM confidence)
- [Rust Forum: multiple channels + select vs single channel enum](https://users.rust-lang.org/t/pros-cons-of-multiple-channels-tokio-select-vs-single-channel-and-expanding-message-enum-variants/77758) -- tradeoffs of typed channels vs monolithic event enum (MEDIUM confidence)

---
*Architecture research for: Cross-venue crypto prediction market / options arbitrage system*
*Researched: 2026-02-21*
