# Architecture Patterns

**Domain:** Paper trading validation features for cross-venue prediction market arbitrage system
**Researched:** 2026-02-24
**Confidence:** HIGH (based on direct source code analysis of 22,751 LOC existing codebase)

## Existing Architecture Summary

The system is a single-binary async Rust service with this pipeline:

```
[Venue Supervisors] --RawMessage--> [Processors] --MarketSnapshot--> [Fan-in channel]
                                                                           |
                                                              [3-way Fan-out task]
                                                             /       |          \
                                                            v        v           v
                                                   [SpreadEngine] [PricingEngine] [CrossAssetEngine]
                                                        |               |                |
                                                  SpreadResult    ImpliedProbability  ArbSignal
                                                        |               |                |
                                                        v               +------->--------+
                                                 [PaperTradeTracker]                     |
                                                                               [ArbSignal consumer (log only)]
```

Key architectural patterns already established:
- **Tokio tasks** with `CancellationToken` per-task for graceful shutdown
- **mpsc channels** (bounded, 1024) for inter-component data flow
- **`tokio::select! biased`** in every run loop (cancel highest priority)
- **JSONL file logging** with daily rotation and buffered writers (BufWriter, flush every 100 writes)
- **Config structs** with `#[serde(default)]` for backward-compatible TOML loading
- **`Arc<RwLock<EventRegistry>>`** for shared event mapping state
- **`BasisRiskCache`** (`Arc<RwLock<HashMap>>`) with `try_read` for non-blocking hot path access
- **`metrics::counter!/gauge!/histogram!`** for Prometheus instrumentation
- **`tracing::info!/warn!`** with structured fields for JSON log output

## Recommended Architecture for v1.1 Features

### Design Principle: Extend, Don't Restructure

All four v1.1 features integrate as **new consumers of existing channel data** or **new periodic tasks**. No changes to the hot path (fan-out, SpreadEngine, PricingEngine, CrossAssetEngine). The pipeline topology stays the same.

### Updated Pipeline Diagram

```
[Existing Pipeline -- UNCHANGED]
       |                    |                     |
  SpreadResult         ArbSignal          MarketSnapshot
       |                    |              (from fan-out)
       v                    v                     |
[PaperTradeTracker]  [ArbSignal consumer]         |
       |                    |                     |
       +----+----+----------+                     |
            |    |                                |
            v    v                                v
    [SignalAnalyzer]  [SettlementTracker]   [AlertMonitor]
            |                |                    |
            v                v                    v
    [StatePersistence] <-----+--------------------+
         (JSONL files)
```

### Component Boundaries

| Component | Responsibility | Reads From | Writes To | New/Modified |
|-----------|---------------|------------|-----------|--------------|
| `SettlementTracker` | Poll venues for settlement outcomes, match against historical signals | EventRegistry, venue REST APIs | settlement_outcomes JSONL, Prometheus metrics | **NEW module** |
| `SignalAnalyzer` | Compute hit rate, edge accuracy, false positive rate, time-to-convergence | signal_logs JSONL, settlement outcomes | analysis_reports JSONL, Prometheus metrics | **NEW module** |
| `AlertMonitor` | Detect degraded states (stale feeds, partial feeds, silent failures) | VenueHealth, channel lag, metrics state | tracing::warn, Prometheus alerts, alert_log JSONL | **NEW module** |
| `StatePersistence` | Save/load paper P&L and signal history across restarts | PaperTradeTracker state, SignalAnalyzer state | state/ directory (JSONL snapshots) | **NEW module** |
| `PaperTradeTracker` | Existing paper trade lifecycle | SpreadResult, MarketSnapshot | trade JSONL (existing) | **MODIFIED** -- add settlement integration |
| `HealthState` | Existing health endpoint | VenueHealth | JSON response | **MODIFIED** -- add alert summary |

## Detailed Component Architecture

### 1. Settlement Outcome Tracker (`src/settlement/`)

**Purpose:** After events expire, determine actual outcomes and match against signals/positions.

**Integration points with existing code:**
- Reads `EventRegistry` (`Arc<RwLock<EventRegistry>>`) to find expired events with settlement metadata
- Reads `SettlementMetadata` from `EventMapping.settlement` for venue-specific resolution sources
- Reuses `ContractLifecycleManager`'s expiry detection pattern (periodic polling, REST API calls)
- Uses existing `reqwest::Client` pattern for venue REST API calls

**Data flow:**
```
[ContractLifecycleManager marks event expired]
       |
       v
[SettlementTracker detects expired+unresolved events]
       |
       +--> Poll Deribit REST: GET /public/get_delivery_prices?index_name=btc_usd
       +--> Poll Kalshi REST: GET /trade-api/v2/events/{event_ticker}
       +--> Poll Polymarket REST: Check condition resolution via Gamma API
       |
       v
[SettlementOutcome { event_id, venue_outcomes: HashMap<Venue, VenueOutcome>, resolved_at }]
       |
       v
[Write to settlement_outcomes/ JSONL + notify SignalAnalyzer + notify PaperTradeTracker]
```

**Structural pattern:**
```rust
pub struct SettlementTracker {
    registry: Arc<RwLock<EventRegistry>>,
    http_client: reqwest::Client,
    venues_config: VenuesConfig,
    credentials: Credentials,
    cancel: CancellationToken,
    outcome_tx: mpsc::Sender<SettlementOutcome>,
    log_dir: PathBuf,
    /// Track which events we have already resolved to avoid re-polling.
    resolved: HashSet<String>,
    /// Poll interval (e.g., every 5 minutes).
    poll_interval_secs: u64,
}

pub struct SettlementOutcome {
    pub event_id: String,
    pub asset: String,
    pub strike: String,
    pub direction: Direction,
    pub actual_outcome: OutcomeResult, // Yes/No/Unknown
    pub settlement_price: Option<Decimal>,
    pub venue_outcomes: HashMap<Venue, VenueOutcome>,
    pub resolved_at: DateTime<Utc>,
}

pub enum OutcomeResult {
    Yes,     // Event occurred (e.g., BTC > 100K)
    No,      // Event did not occur
    Unknown, // Could not determine (API error, ambiguous)
}

pub struct VenueOutcome {
    pub venue: Venue,
    pub settlement_value: Option<Decimal>, // 1.0 = Yes, 0.0 = No
    pub source: String, // "deribit_index", "oracle", etc.
    pub fetched_at: DateTime<Utc>,
}
```

**Why this design:**
- Separate from `ContractLifecycleManager` because lifecycle handles discovery/expiry, settlement handles post-expiry resolution. Different polling cadences and different API endpoints.
- `outcome_tx` channel allows `SignalAnalyzer` to react to new outcomes without polling files.
- `resolved` set prevents redundant API calls for already-settled events.
- Follows the `ContractLifecycleManager` pattern: periodic `tokio::time::interval` in a `tokio::select! biased` loop with cancellation.

**Key integration detail:** The `EventMapping.status` field transitions from `Active` to `Expired` in lifecycle. Settlement tracker watches for `Expired` status and attempts resolution. A new status `Settled` could be added, but it is simpler to track resolution state internally in the `resolved` HashSet and in the outcome JSONL files, avoiding changes to the shared config format.

### 2. Signal Analyzer (`src/analysis/`)

**Purpose:** Backtest signal quality by comparing generated signals against settlement outcomes.

**Integration points with existing code:**
- Reads signal JSONL files from `signal_logs/` (written by `SignalLogger` in `CrossAssetEngine`)
- Reads spread JSONL files from `spreads/` (written by `SpreadLogger` in `SpreadEngine`)
- Reads trade JSONL from `paper_trades/` (written by `TradeLogger` in `PaperTradeTracker`)
- Receives `SettlementOutcome` via mpsc channel from `SettlementTracker`
- Exposes analysis metrics via existing `metrics::gauge!` pattern

**Metrics computed:**
```rust
pub struct SignalAnalysisReport {
    pub period: AnalysisPeriod,       // Daily/Weekly/AllTime
    pub total_signals: u64,
    pub settled_signals: u64,
    pub hit_rate: f64,                // % of signals where direction was correct
    pub avg_edge_accuracy: f64,       // avg(actual_pnl / predicted_edge)
    pub false_positive_rate: f64,     // % of PassedBoth signals that lost money
    pub avg_time_to_convergence_ms: Option<i64>, // avg time from signal to price convergence
    pub edge_distribution: EdgeStats, // mean, median, stddev, p5, p95
    pub per_event_stats: HashMap<String, EventAnalysis>,
    pub per_direction_stats: HashMap<String, DirectionAnalysis>,
    pub generated_at: DateTime<Utc>,
}
```

**Architecture decision -- online vs batch:**

Use **hybrid approach**: online accumulation with periodic batch reconciliation.

- **Online:** As each `SettlementOutcome` arrives via channel, immediately score any matching signals from the in-memory signal index. Update running counters.
- **Batch:** On a configurable interval (e.g., hourly), scan JSONL files for any signals that were missed (e.g., if the system restarted between signal emission and settlement). This handles the cold-start problem.

**Structural pattern:**
```rust
pub struct SignalAnalyzer {
    /// In-memory index of recent signals awaiting settlement.
    pending_signals: HashMap<String, Vec<IndexedSignal>>, // event_id -> signals
    /// Running statistics accumulator.
    stats: AnalysisAccumulator,
    /// Receives settlement outcomes from SettlementTracker.
    outcome_rx: mpsc::Receiver<SettlementOutcome>,
    /// Receives new arb signals for indexing (tapped from existing arb_signal channel).
    signal_rx: mpsc::Receiver<ArbSignal>,
    /// Configuration.
    config: AnalysisConfig,
    /// JSONL writer for analysis reports.
    report_logger: AnalysisLogger,
    cancel: CancellationToken,
}
```

**Key integration detail:** The existing `ArbSignal` consumer in `main.rs` (lines 409-442) currently just logs and meters. To feed `SignalAnalyzer`, add a second consumer by cloning the `arb_signal_tx` sender or adding a new fan-out. The cheapest approach: create `arb_signal_tx` with `mpsc::channel`, then create a small fan-out task that clones each `ArbSignal` to both the existing log consumer and the `SignalAnalyzer`. Since `ArbSignal` derives `Clone`, the fan-out cost is minimal.

### 3. Alert Monitor (`src/alert/`)

**Purpose:** Detect operational degradation beyond what reconnection supervisors handle. Reconnection supervisors handle connection drops. Alert monitor handles subtle degradation: stale data that technically arrives but is outdated, partial feed coverage, silent processing failures, channel backpressure.

**Integration points with existing code:**
- Reads `VenueHealth` (`Arc<VenueHealth>`) -- already tracked per venue with atomics
- Reads `VenueHealth.last_message_at()` for silence detection (message gap analysis)
- Can read Prometheus metrics state (counters like `arb_staleness_rejections`, `arb_computations_total`)
- Adds to existing `/health` endpoint response for alert summary

**Alert types:**
```rust
pub enum AlertSeverity {
    Warning,  // Degraded but operational
    Critical, // Requires attention, may affect signal quality
}

pub enum AlertType {
    /// No messages from venue for > threshold seconds despite connection.
    SilentFeed { venue: Venue, gap_secs: u64 },
    /// Staleness rejection rate exceeds threshold.
    HighStalenessRate { venue: Venue, rate: f64 },
    /// Fewer than expected venues contributing to spread computation.
    PartialCoverage { active_venues: usize, expected_venues: usize },
    /// Channel backpressure detected (try_send failures).
    ChannelBackpressure { component: String, drop_rate: f64 },
    /// No spread computations for > threshold seconds despite data flowing.
    NoComputations { gap_secs: u64 },
    /// Paper trade tracker has stale pending positions (signal but no fill).
    StalePendingTrades { count: usize, oldest_age_secs: u64 },
}
```

**Structural pattern:**
```rust
pub struct AlertMonitor {
    venue_health: Vec<Arc<VenueHealth>>,
    config: AlertConfig,
    active_alerts: HashMap<String, ActiveAlert>, // dedup key -> alert
    cancel: CancellationToken,
    alert_logger: AlertLogger, // JSONL writer
}
```

**Run pattern:** Periodic evaluation (e.g., every 30 seconds) using `tokio::time::interval`. Reads atomic state from `VenueHealth` (never blocks pipeline). Emits `tracing::warn!` for new alerts, `tracing::info!` for resolved alerts. Updates Prometheus gauges.

**Key integration detail:** `VenueHealth` already provides `is_available()`, `last_message_at()`, `last_error()`, `connection_count()` -- all via atomics/mutex, all non-blocking. The alert monitor just needs periodic reads. No changes to existing feed infrastructure needed.

**Health endpoint extension:** Add an `alerts: Vec<AlertSummary>` field to `HealthResponse`. The `AlertMonitor` shares its `active_alerts` via `Arc<RwLock<HashMap>>` (same pattern as `BasisRiskCache`). The health handler reads it with `try_read`.

### 4. State Persistence (`src/persistence/`)

**Purpose:** Save paper P&L state and signal history so the system can resume after restart without losing accumulated data.

**Integration points with existing code:**
- Serializes `PaperTradeTracker` state: open positions, pending positions, daily aggregator
- Serializes `SignalAnalyzer` running statistics
- Already-existing JSONL trade logs, signal logs, and spread logs provide the historical record. Persistence focuses on **in-memory state** that would be lost on restart.

**Design: Checkpoint-based, not WAL:**

The system already writes every event to JSONL (signals, trades, spreads). What is lost on restart is computed in-memory state (running averages, open positions, pending fills). Two approaches:

1. **WAL (Write-Ahead Log):** Complex, overkill for paper trading
2. **Periodic checkpoint:** Serialize in-memory state to a snapshot file every N minutes. On restart, load latest checkpoint, then replay any JSONL events after the checkpoint timestamp.

**Use checkpoint approach** because:
- Paper trading is not real money -- losing a few minutes of state is acceptable
- JSONL files already provide complete event history for replay
- Checkpoint files are simple to debug (human-readable JSON)
- Matches the existing file I/O patterns (BufWriter, daily rotation)

**Structural pattern:**
```rust
pub struct StateCheckpoint {
    pub version: u32,
    pub timestamp: DateTime<Utc>,
    pub paper_trade_state: PaperTradeState,
    pub analysis_state: Option<AnalysisState>,
}

pub struct PaperTradeState {
    pub pending: HashMap<String, Vec<PaperPosition>>,
    pub open: Vec<PaperPosition>,
    pub aggregator_daily: HashMap<String, DailyRollup>,
    pub total_trades: u64,
}

pub struct StatePersistence {
    state_dir: PathBuf,
    checkpoint_interval_secs: u64,
    cancel: CancellationToken,
}
```

**Checkpoint flow:**
```
Every N minutes:
  1. PaperTradeTracker.snapshot() -> PaperTradeState (via method on tracker, not direct field access)
  2. SignalAnalyzer.snapshot() -> AnalysisState
  3. Serialize StateCheckpoint to JSON
  4. Atomic write: write to state/checkpoint.json.tmp, rename to state/checkpoint.json
```

**Restart flow:**
```
On startup:
  1. If state/checkpoint.json exists, load it
  2. Reconstruct PaperTradeTracker with loaded state
  3. Reconstruct SignalAnalyzer with loaded state
  4. Find JSONL events after checkpoint timestamp, replay them
  5. Resume normal operation
```

**Key integration detail:** `PaperPosition` already derives `Serialize, Deserialize`. `DailyRollup` already derives `Serialize` (needs `Deserialize` added). The types are mostly ready for checkpoint serialization. The main change is adding `snapshot()` and `restore()` methods to `PaperTradeTracker` and (later) `SignalAnalyzer`.

**Atomic write pattern:** Already used by `ContractLifecycleManager.atomic_write()` for events.toml updates. Reuse the same pattern: write to `.tmp`, then `tokio::fs::rename`.

## Patterns to Follow

### Pattern 1: Tokio Task with Cancellation

Every new component follows this pattern (used by SpreadEngine, PricingEngine, CrossAssetEngine, PaperTradeTracker, ContractLifecycleManager):

```rust
pub async fn run(self, /* channels */, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(self.config.poll_interval));
    interval.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::info!("ComponentName shutting down");
                // flush any buffered state
                break;
            }

            _ = interval.tick() => {
                self.do_periodic_work().await;
            }

            msg = channel_rx.recv() => {
                match msg {
                    Some(m) => self.handle(m).await,
                    None => {
                        tracing::info!("channel closed, stopping");
                        break;
                    }
                }
            }
        }
    }
}
```

### Pattern 2: JSONL Logger with Daily Rotation

Every logging component follows this pattern (used by SignalLogger, SpreadLogger, TradeLogger):

```rust
struct ComponentLogger {
    log_dir: PathBuf,
    writer: Option<BufWriter<File>>,
    current_date: Option<NaiveDate>,
    writes_since_flush: u64,
}

impl ComponentLogger {
    fn log_event(&mut self, event: &impl Serialize) -> anyhow::Result<()> {
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
}
```

### Pattern 3: Config with Serde Defaults

Every config struct follows this pattern for backward-compatible TOML:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NewFeatureConfig {
    pub some_threshold: u64,
    pub log_dir: String,
    pub enabled: bool,
}

impl Default for NewFeatureConfig {
    fn default() -> Self { /* sensible defaults */ }
}
```

Then add to `SystemConfig` in `src/config/system.rs`:
```rust
#[serde(default)]
pub new_feature: NewFeatureConfig,
```

### Pattern 4: Shared State via Arc<RwLock> with try_read

For state that must be readable from the hot path without blocking:

```rust
pub type SharedAlertState = Arc<RwLock<HashMap<String, ActiveAlert>>>;

// Writer (AlertMonitor): .write().await -- ok, runs on its own interval
// Reader (HealthEndpoint): .try_read() -- never blocks the HTTP handler
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Modifying the Fan-out Hot Path

**What:** Adding new channel sends to the existing 3-way fan-out task in main.rs for v1.1 features.
**Why bad:** The fan-out task is on the critical data path. Every additional `try_send` adds latency and backpressure risk. The fan-out already drops snapshots for pricing and signal engines when channels are full.
**Instead:** New components that need MarketSnapshot data should either: (a) subscribe to a broadcast channel added alongside the fan-out, or (b) read from their upstream component (e.g., AlertMonitor reads VenueHealth, not raw snapshots).

### Anti-Pattern 2: Direct Field Access for Checkpointing

**What:** Having `StatePersistence` directly read `PaperTradeTracker.open` and `.pending` fields.
**Why bad:** Creates tight coupling. If PaperTradeTracker's internal representation changes, persistence breaks. Also requires making fields `pub` or using unsafe access patterns.
**Instead:** Add `snapshot() -> PaperTradeState` and `restore(state: PaperTradeState)` methods to PaperTradeTracker. The tracker owns its state representation; persistence only deals with the serializable snapshot type.

### Anti-Pattern 3: Settlement Polling in ContractLifecycleManager

**What:** Adding settlement outcome fetching to the existing ContractLifecycleManager poll cycle.
**Why bad:** Lifecycle manager already has complex multi-venue polling with independent intervals. Adding settlement polling (different API endpoints, different cadence, different error handling) makes it harder to test and debug. Single responsibility violation.
**Instead:** SettlementTracker as a separate tokio task. It can read from EventRegistry (shared) to find expired events, then poll its own endpoints independently.

### Anti-Pattern 4: Database for State Persistence

**What:** Adding SQLite or another database for paper trade state.
**Why bad:** Adds a dependency, deployment complexity (single binary constraint), and migration burden for what is essentially serializing a few structs. The system already writes JSONL everywhere -- adding a database creates two persistence patterns to maintain.
**Instead:** JSON checkpoint files with atomic writes. Consistent with existing patterns. Human-readable for debugging. Zero new dependencies.

### Anti-Pattern 5: Blocking File I/O on the Main Pipeline

**What:** Performing synchronous file reads in components that receive channel messages.
**Why bad:** Even brief file I/O stalls block the tokio task, creating backpressure on upstream channels. The system is designed for sub-millisecond channel processing.
**Instead:** File I/O (like checkpoint loading) happens at startup before the run loop starts. Checkpoint writes happen on a separate interval tick, never in the message processing path. Use `tokio::fs` for async operations where needed.

## Scalability Considerations

| Concern | Current (paper trading) | If Signal Volume 10x | If Adding Real Execution |
|---------|------------------------|----------------------|--------------------------|
| Signal log size | ~MB/day, daily rotation handles it | Add JSONL compression or retention policy | Same -- execution layer reads from channels, not files |
| Settlement polling | Once per event expiry (handful/month) | Same -- number of events stays small | Same |
| Checkpoint size | KB (few open positions) | Still KB-MB | Add incremental checkpoints, consider WAL |
| Alert monitoring | 30s poll, reads atomics | Same -- O(1) per venue | Add execution-specific alerts |
| Analysis computation | On-demand when settlements arrive | Add background batch job | Same |
| Channel buffer sizing | 1024 per channel, sufficient | Monitor `try_send` failure rate, increase if needed | Separate execution channel with its own buffer |

## Data Flow Changes Summary

```
EXISTING DATA FLOWS (unchanged):
  Snapshots -> Fan-out -> SpreadEngine/PricingEngine/CrossAssetEngine
  SpreadResult -> PaperTradeTracker
  ImpliedProbability -> CrossAssetEngine
  ArbSignal -> (log consumer)

NEW DATA FLOWS:
  ArbSignal -> SignalAnalyzer (new fan-out from arb_signal channel)
  SettlementOutcome -> SignalAnalyzer (new mpsc channel)
  SettlementOutcome -> PaperTradeTracker (for settling open positions)
  EventRegistry -> SettlementTracker (reads expired events)
  VenueHealth -> AlertMonitor (reads atomic state, no channel needed)
  AlertMonitor shared state -> HealthEndpoint (Arc<RwLock>, try_read)
  PaperTradeTracker -> StatePersistence (snapshot method, periodic)
  SignalAnalyzer -> StatePersistence (snapshot method, periodic)
```

## Module Organization

```
src/
  settlement/          <-- NEW
    mod.rs
    tracker.rs         # SettlementTracker task
    types.rs           # SettlementOutcome, VenueOutcome, OutcomeResult
    fetcher.rs         # Per-venue REST API settlement fetching
  analysis/            <-- NEW
    mod.rs
    analyzer.rs        # SignalAnalyzer task
    types.rs           # SignalAnalysisReport, EdgeStats, AnalysisPeriod
    accumulator.rs     # Running statistics (online accumulation)
    config.rs          # AnalysisConfig
  alert/               <-- NEW
    mod.rs
    monitor.rs         # AlertMonitor task
    types.rs           # AlertType, AlertSeverity, ActiveAlert
    config.rs          # AlertConfig (thresholds, intervals)
  persistence/         <-- NEW
    mod.rs
    checkpoint.rs      # StateCheckpoint, atomic write/read
    restore.rs         # Startup restoration from checkpoint + JSONL replay
    config.rs          # PersistenceConfig
  paper_trade/         <-- MODIFIED
    tracker.rs         # Add snapshot()/restore(), settlement integration
    position.rs        # (unchanged, already has Serialize/Deserialize)
    aggregator.rs      # Add Deserialize to DailyRollup
  health/              <-- MODIFIED
    mod.rs             # Add alerts field to HealthResponse
  config/              <-- MODIFIED
    system.rs          # Add SettlementConfig, AnalysisConfig, AlertConfig, PersistenceConfig
```

## Build Order (Dependency-Driven)

The components have these dependencies:

```
StatePersistence depends on: PaperTradeTracker (snapshot API), SignalAnalyzer (snapshot API)
SignalAnalyzer depends on: SettlementTracker (outcome channel), ArbSignal channel
SettlementTracker depends on: EventRegistry (already exists), venue REST APIs (existing patterns)
AlertMonitor depends on: VenueHealth (already exists)
```

**Recommended build order:**

1. **AlertMonitor** -- No dependencies on other new components. Reads existing VenueHealth atomics. Immediate operational value for unattended paper trading. Simplest of the four.

2. **SettlementTracker** -- Depends only on existing EventRegistry. Requires REST API integration (follows ContractLifecycleManager patterns). Must be built before SignalAnalyzer.

3. **SignalAnalyzer** -- Depends on SettlementTracker (outcome channel) and ArbSignal tap. This is the core analytical value of v1.1.

4. **StatePersistence** -- Depends on PaperTradeTracker and SignalAnalyzer having snapshot APIs. Build last because it needs stable interfaces from the other components. Also least critical -- restarting during paper trading is acceptable.

## Wiring in main.rs

Approximate addition to `main.rs` (after existing pipeline wiring, around line 408):

```rust
// -- v1.1: Alert Monitor --
let alert_state: SharedAlertState = Arc::new(RwLock::new(HashMap::new()));
let alert_config = config.system.alert.clone();
let alert_monitor = AlertMonitor::new(
    pipeline_handles.venue_health.clone(),
    alert_config,
    alert_state.clone(),
    shutdown_token.child_token(),
);
tokio::spawn(alert_monitor.run());

// -- v1.1: Settlement Tracker --
let (outcome_tx, outcome_rx) = mpsc::channel::<SettlementOutcome>(64);
let outcome_tx_for_tracker = outcome_tx.clone(); // for PaperTradeTracker
let settlement_tracker = SettlementTracker::new(
    event_registry.clone(),
    config.system.settlement.clone(),
    config.venues.clone(),
    config.credentials.clone(),
    outcome_tx,
    shutdown_token.child_token(),
);
if is_live {
    tokio::spawn(settlement_tracker.run());
}

// -- v1.1: ArbSignal fan-out to SignalAnalyzer --
// Replace single arb_signal consumer with fan-out to both log consumer + analyzer
let (analysis_signal_tx, analysis_signal_rx) = mpsc::channel::<ArbSignal>(1024);
// Modify arb consumer loop to also forward to analysis_signal_tx

// -- v1.1: Signal Analyzer --
let signal_analyzer = SignalAnalyzer::new(
    analysis_signal_rx,
    outcome_rx,
    config.system.analysis.clone(),
    shutdown_token.child_token(),
);
tokio::spawn(signal_analyzer.run());

// -- v1.1: State Persistence --
// Wired after all components are constructed, periodic checkpoint on interval
```

## Sources

- Direct analysis of existing source code at `D:\Programming\Rust\prediction\src\` (22,751 LOC)
- `src/main.rs` -- pipeline wiring and task spawning patterns (lines 1-459)
- `src/signal/engine.rs` -- CrossAssetEngine run loop pattern
- `src/signal/types.rs` -- ArbSignal struct (derives Clone, Serialize, Deserialize)
- `src/paper_trade/tracker.rs` -- PaperTradeTracker lifecycle and JSONL logging
- `src/paper_trade/position.rs` -- PaperPosition (derives Serialize, Deserialize)
- `src/paper_trade/aggregator.rs` -- DailyRollup (derives Serialize, needs Deserialize)
- `src/events/lifecycle.rs` -- ContractLifecycleManager polling and atomic write patterns
- `src/events/risk.rs` -- BasisRiskCache (Arc<RwLock<HashMap>>) pattern
- `src/feed/health.rs` -- VenueHealth atomic state pattern
- `src/health/mod.rs` -- HealthState and endpoint structure
- `src/feed/pipeline.rs` -- Multi-venue pipeline assembly and fan-out
- `src/feed/recording/mod.rs` -- RecordingService non-blocking I/O pattern
- `src/config/system.rs` -- SystemConfig struct with #[serde(default)] fields
- `src/config/events.rs` -- EventMapping, SettlementMetadata structures
