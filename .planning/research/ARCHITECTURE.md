# Architecture Research: Dynamic Subscription Management

**Domain:** Live feed subscription/unsubscription for prediction market arbitrage system
**Researched:** 2026-02-27
**Confidence:** HIGH (based on direct analysis of 34,753 LOC codebase + prior v1.2 architecture research)

## Executive Summary

The v1.3 milestone bridges the gap between the existing automated event discovery pipeline (v1.2) and the live feed supervisors (v1.0). Today, when a newly discovered mapping is approved in `events.toml`, the EventRegistry correctly picks it up via config hot-reload -- but no venue supervisor subscribes to the new instrument's WebSocket channel. Data for the new instrument never arrives. The system must be restarted to activate new subscriptions.

This research documents the exact current architecture, identifies the precise integration seams, defines the new SubscriptionManager component, specifies per-venue subscription mechanics, and recommends a build order that respects dependency chains and preserves hot-path integrity.

**Key architectural principle: the hot path (feeds -> SpreadEngine -> SignalEngine) must never be blocked or disrupted by subscription management activity.** All subscription changes happen in background tasks, communicating via `tokio::sync::watch` channels to supervisors that already handle reconnection.

## Current Architecture (Verified Against Source)

### System Topology

```
                    +----------------------------+
                    |     ConfigReloader          |
                    |  (OS thread, notify crate)  |
                    |  watches config/*.toml      |
                    +-------+--------------------+
                            |
                       watch::channel<AppConfig>
                            |
                            v
              +---------------------------+
              | Config Hot-Reload Sub     |
              | (tokio task in main.rs)   |
              | - registry.write().await  |
              | - registry.refresh()      |
              +---------------------------+
                            |
                    Arc<RwLock<EventRegistry>>
                            |
      +---------------------+---------------------+
      |                     |                     |
      v                     v                     v
[DeribitSupervisor]  [PolySupervisor]    [KalshiSupervisor]
 instruments:         config.assets:      config.market_tickers:
 Vec<String>          Vec<PolyAsset>      Vec<String>
 (fixed at init)      (fixed at init)     (fixed at init)
      |                     |                     |
      v                     v                     v
[DeribitClient]      [PolyClient]         [KalshiClient]
  WS subscribe         WS subscribe        WS subscribe
  batch channels       assets_ids          per-ticker
      |                     |                     |
      v                     v                     v
[DeribitProcessor]   [PolyProcessor]      [KalshiProcessor]
      |                     |                     |
      +----------+----------+----------+----------+
                 |                     |
            forward_snapshots()   forward_snapshots()
            (event_id annotation) (event_id annotation)
                 |                     |
                 +----------+----------+
                            |
                       fan-in mpsc
                            |
                     [SnapshotFanOut]
                    /       |        \
            SpreadEngine PricingEngine CrossAssetEngine
                |                          |
          SignalEngine              PaperTradeTracker
```

### The Gap: Subscription List is Static

Each supervisor receives its instrument list at construction time and never updates it:

| Venue | Supervisor Field | How Instruments Arrive | Client Usage |
|-------|-----------------|----------------------|--------------|
| Deribit | `instruments: Vec<String>` (constructor param) | `config.deribit.instruments.clone()` from `venues.toml` | `DeribitClient::new(config, instruments, ...)` builds channel list |
| Polymarket | `config: PolymarketConfig` (contains `assets`) | `config.polymarket.clone()` from `venues.toml` | `PolymarketClient` reads `self.config.assets` for token IDs |
| Kalshi | `config: KalshiConfig` (contains `market_tickers`) | `config.kalshi.clone()` from `venues.toml` | `KalshiClient` iterates `self.config.market_tickers` |

**Critical observation:** Deribit takes instruments as a separate parameter; Polymarket and Kalshi embed them in their config structs. The subscription manager must account for these structural differences.

### What Already Works for Dynamic Updates

| Component | Capability | Verified |
|-----------|-----------|----------|
| `ConfigReloader` | Detects `events.toml` changes, re-parses, distributes `AppConfig` via `watch::channel` | Yes -- `reload.rs:62-117` |
| Config hot-reload subscriber | Receives `AppConfig`, calls `registry.write().refresh()` | Yes -- `main.rs:286-319` |
| `EventRegistry::refresh()` | Full rebuild: clears indexes, replaces mappings, rebuilds dual-index | Yes -- `registry.rs:75-80` |
| `EventRegistry::active_approved()` | Iterates only `approved == true && status == Active` | Yes -- `registry.rs:60-64` |
| `forward_snapshots()` | Annotates event_id via `registry.read().lookup_by_instrument()` | Yes -- `pipeline.rs:338-387` |
| Supervisor reconnect loop | Each supervisor reconnects with exponential backoff on connection loss | Yes -- all three supervisors |

**What is missing:** Nothing tells the supervisors to reconnect with an updated instrument list when the registry changes.

### Subscription Semantics Per Venue

| Venue | WS Subscribe Format | Supports In-Connection Subscribe/Unsubscribe? | Reconnect Cost |
|-------|--------------------|--------------------------------------------|----------------|
| Deribit | Batch `public/subscribe` with channel array | **YES** -- `public/subscribe` and `public/unsubscribe` JSON-RPC | ~2-5s (connect + subscribe + first heartbeat) |
| Polymarket | Single JSON `{"assets_ids": [...], "type": "market"}` | **Unknown** -- not documented; likely requires reconnect | ~2-3s (connect + subscribe) |
| Kalshi | Per-ticker `{"cmd": "subscribe", "params": {"channels": ["orderbook_delta"], "market_ticker": ticker}}` | **Likely YES** -- subscribe command is per-ticker | ~2-3s (connect + auth sign + subscribe) |

**Architectural decision:** Use reconnect-based subscription management for all venues. Deribit supports in-connection subscribe/unsubscribe, but using reconnect for all three keeps the code paths uniform and avoids venue-specific dynamic subscription complexity. The 2-5 second reconnect gap is irrelevant for minute-to-hour arbitrage windows.

**Future optimization (not v1.3):** For Deribit, send incremental `public/subscribe`/`public/unsubscribe` on the existing connection to avoid any data gap. This can be added later if the reconnect gap proves problematic.

## Recommended Architecture

### New Component: SubscriptionManager

A single new tokio background task that bridges config changes to supervisor instrument lists.

```
                    +----------------------------+
                    |     ConfigReloader          |
                    |  (OS thread, notify crate)  |
                    +-------+--------------------+
                            |
                       watch::channel<AppConfig>
                            |
              +-------------+------------------+
              |                                |
              v                                v
+---------------------------+   +----------------------------+
| Config Hot-Reload Sub     |   | SubscriptionManager        |  <-- NEW
| (existing task)           |   | (new tokio task)           |
| - registry.refresh()     |   | - reads registry after     |
+---------------------------+   |   refresh notification     |
                                | - diffs per-venue sets     |
                                | - pushes via watch channels|
                                +------+-----+-----+---------+
                                       |     |     |
                          watch<Vec>   |     |     |  watch<Vec>
                          (Deribit)    |     |     |  (Kalshi)
                                       |     |     |
                                       v     |     v
                       [DeribitSupervisor]    |  [KalshiSupervisor]
                         +instruments_rx     |    +tickers_rx
                         select! branch:     |    select! branch:
                         changed() -> break  |    changed() -> break
                         -> reconnect with   |    -> reconnect with
                            new list         |       new list
                                             |
                                   watch<Vec>|
                                   (Polymarket)
                                             |
                                             v
                                  [PolymarketSupervisor]
                                    +assets_rx
                                    select! branch:
                                    changed() -> break
                                    -> reconnect with
                                       new list
```

### Component Boundaries

| Component | Responsibility | New/Modified | Communicates With |
|-----------|---------------|-------------|-------------------|
| `SubscriptionManager` | Watches config changes, reads registry, diffs per-venue instrument sets, pushes updated lists to supervisors, emits metrics and structured logs | **NEW** | ConfigReloader (via `watch::Receiver<AppConfig>`), EventRegistry (`Arc<RwLock<>>` read), venue supervisors (via `watch::Sender<Vec<_>>`) |
| `DeribitSupervisor` | Accept `watch::Receiver<Vec<String>>` for instruments, add `changed()` branch to inner `select!`, break to reconnect loop on change | **MODIFIED** (minor) | SubscriptionManager (watch channel), DeribitClient |
| `PolymarketSupervisor` | Accept `watch::Receiver<Vec<PolymarketAsset>>` for assets, add `changed()` branch | **MODIFIED** (minor) | SubscriptionManager (watch channel), PolymarketClient |
| `KalshiSupervisor` | Accept `watch::Receiver<Vec<String>>` for tickers, add `changed()` branch | **MODIFIED** (minor) | SubscriptionManager (watch channel), KalshiClient |
| `pipeline.rs` (`run_live_multi_venue`) | Accept watch receivers from main, pass to supervisors; derive initial lists from registry instead of config | **MODIFIED** (moderate) | main.rs, supervisors |
| `main.rs` | Create SubscriptionManager, watch channels, wire between config subscriber and pipeline | **MODIFIED** (moderate) | SubscriptionManager, pipeline |
| `EventRegistry` | No changes needed | **UNCHANGED** | All readers |
| `ContractLifecycleManager` | No changes needed (already writes to events.toml and refreshes registry) | **UNCHANGED** | EventRegistry, events.toml |
| `ConfigReloader` | No changes needed | **UNCHANGED** | Config subscribers |
| `forward_snapshots()` | No changes needed (already annotates event_id from registry) | **UNCHANGED** | EventRegistry (read lock) |
| Hot-path engines | No changes needed | **UNCHANGED** | Snapshot fan-in |

### Data Flow: Full Lifecycle of a New Event (with v1.3)

```
Phase 1: Discovery (automated -- unchanged from v1.2)
================================================================
ContractLifecycleManager.poll_cycle()
  --> discover_deribit() / discover_kalshi() / discover_polymarket_structured()
  --> find_cross_venue_candidates_fuzzy()
  --> filter_new_candidates_fuzzy()
  --> append_candidates_to_doc() --> events.toml (approved = false)
  --> refresh_registry()
  --> log "discovered new candidate mapping"
  --> metric: lifecycle_candidates_discovered++


Phase 2: Operator Review (manual -- unchanged)
================================================================
Operator sees log, reviews events.toml
  --> Sets approved = true
  --> Saves file


Phase 3: Config Reload (automated -- existing mechanism)
================================================================
ConfigReloader detects events.toml change (notify debounce 500ms)
  --> Parses TOML --> new AppConfig
  --> watch::channel.send(new_config)


Phase 4: Registry Refresh (existing mechanism)
================================================================
Config Hot-Reload Subscriber receives new AppConfig
  --> registry.write().await
  --> registry.refresh(&new_config.events)
  --> log "EventRegistry refreshed, N mappings"


Phase 5: Subscription Reconciliation (NEW -- v1.3)
================================================================
SubscriptionManager receives same AppConfig notification
  --> tokio::task::yield_now() (ensure registry refresh completed)
  --> registry.read().await
  --> Collect per-venue instrument sets from active_approved()
  --> Diff against last-known sets:
        added_deribit = new_deribit - last_deribit
        removed_deribit = last_deribit - new_deribit
        (same for kalshi, polymarket)
  --> For each venue with changes:
        watch::Sender.send(new_instrument_list)
        log "subscription change: {venue} +{added} -{removed}"
        metric: subscription_activations / subscription_removals
  --> Update last-known sets


Phase 6: Supervisor Reconnect (modified supervisors)
================================================================
DeribitSupervisor detects instruments_rx.changed()
  --> break inner message loop
  --> borrow_and_update() to get new instruments
  --> Back to outer loop: create fresh DeribitClient with updated list
  --> Connect + subscribe + forward messages
  --> log "instrument list updated, reconnecting with N instruments"
  --> metric: feed_subscription_reconnects++


Phase 7: Data Flows (unchanged hot path)
================================================================
DeribitClient receives data for new instrument
  --> DeribitProcessor normalizes to MarketSnapshot
  --> forward_snapshots() annotates event_id (registry already has mapping)
  --> Snapshot enters fan-in channel
  --> SpreadEngine calculates spreads for new event
  --> Signal generation begins


Phase 8: Retirement (triggered by lifecycle manager, enhanced by v1.3)
================================================================
ContractLifecycleManager detects instrument absent N consecutive polls
  --> mark_expired_batch_in_doc() --> events.toml status = "expired"
  --> archive_and_cleanup() --> events_archive.toml
  --> refresh_registry() --> expired mapping removed from active set
  --> ConfigReloader also detects file change --> triggers Phase 3-6
  --> SubscriptionManager sees instrument removed from active set
  --> Pushes updated list WITHOUT the expired instrument
  --> Supervisor reconnects with pruned list
  --> No more data for retired instrument
```

### Ordering Constraint: Registry Refresh Before Subscription Diff

The SubscriptionManager and the config hot-reload subscriber both subscribe to the same `watch::channel<AppConfig>`. The refresh must complete before the diff reads the registry.

**Problem:** `tokio::sync::watch` delivers the latest value, but does not guarantee ordering between two subscribers.

**Solution options (in order of recommendation):**

1. **Explicit Notify signal (recommended):** The config hot-reload subscriber sends a `tokio::sync::Notify::notify_one()` after `registry.refresh()` completes. The SubscriptionManager awaits `notify.notified()` instead of watching the config channel directly. This guarantees the registry is refreshed before the diff runs.

2. **Small delay:** SubscriptionManager adds `tokio::time::sleep(Duration::from_millis(100))` after receiving config change before reading registry. Simple but brittle.

3. **Single combined task:** Merge the config subscriber and SubscriptionManager into one task that first refreshes the registry, then computes the diff. Simplest, but conflates two concerns.

**Recommendation:** Option 1 (Notify). It is explicit, zero-latency, and separates concerns cleanly.

```rust
// In main.rs setup:
let registry_refreshed = Arc::new(tokio::sync::Notify::new());

// Config hot-reload subscriber:
reg.refresh(&new_config.events);
registry_refreshed.notify_one();  // Signal subscription manager

// SubscriptionManager:
loop {
    registry_refreshed.notified().await;
    // Registry is guaranteed to be refreshed at this point
    let reg = self.registry.read().await;
    self.reconcile_subscriptions(&reg);
}
```

## SubscriptionManager Internal Design

### State

```rust
pub struct SubscriptionManager {
    /// Shared event registry (read-only access)
    registry: Arc<RwLock<EventRegistry>>,

    /// Notification that registry has been refreshed
    registry_refreshed: Arc<Notify>,

    /// Per-venue instrument list senders
    deribit_tx: watch::Sender<Vec<String>>,
    kalshi_tx: watch::Sender<Vec<String>>,
    polymarket_tx: watch::Sender<Vec<PolymarketSubscription>>,

    /// Last-known instrument sets for diffing
    last_deribit: HashSet<String>,
    last_kalshi: HashSet<String>,
    last_polymarket: HashSet<String>,

    /// Cancellation token
    cancel: CancellationToken,
}

/// Polymarket needs both condition_id and token_id for subscription
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PolymarketSubscription {
    pub condition_id: String,
    pub token_id: String,
}
```

### Core Loop

```rust
impl SubscriptionManager {
    pub async fn run(mut self) {
        // Initial reconciliation on startup
        self.reconcile().await;

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    tracing::info!("SubscriptionManager shutting down");
                    break;
                }
                _ = self.registry_refreshed.notified() => {
                    self.reconcile().await;
                }
            }
        }
    }

    async fn reconcile(&mut self) {
        let reg = self.registry.read().await;

        let mut new_deribit = HashSet::new();
        let mut new_kalshi = HashSet::new();
        let mut new_polymarket = HashSet::new();

        for mapping in reg.active_approved() {
            if let Some(ref d) = mapping.venues.deribit {
                new_deribit.insert(d.instrument.clone());
            }
            if let Some(ref k) = mapping.venues.kalshi {
                new_kalshi.insert(k.ticker.clone());
            }
            if let Some(ref p) = mapping.venues.polymarket {
                new_polymarket.insert(p.token_id.clone());
            }
        }
        drop(reg);

        // Deribit diff
        if new_deribit != self.last_deribit {
            let added: Vec<_> = new_deribit.difference(&self.last_deribit).collect();
            let removed: Vec<_> = self.last_deribit.difference(&new_deribit).collect();
            tracing::info!(?added, ?removed, "Deribit subscription change");
            metrics::counter!("subscription_activations", "venue" => "deribit")
                .increment(added.len() as u64);
            metrics::counter!("subscription_removals", "venue" => "deribit")
                .increment(removed.len() as u64);
            let _ = self.deribit_tx.send(new_deribit.iter().cloned().collect());
            self.last_deribit = new_deribit;
        }
        // ... same pattern for kalshi, polymarket
    }
}
```

### Startup Behavior

On startup, the SubscriptionManager performs its first `reconcile()` call to populate all venue channels with the initial instrument lists derived from `EventRegistry::active_approved()`. This replaces the current pattern where instrument lists come from `venues.toml` static config.

**Migration path:** The `DeribitConfig.instruments`, `PolymarketConfig.assets`, and `KalshiConfig.market_tickers` fields in `venues.toml` become fallback/override only. The primary instrument list source shifts to `events.toml` via the registry.

## Supervisor Modifications (Per Venue)

### DeribitSupervisor Changes

**Current signature:**
```rust
pub fn new(config, instruments: Vec<String>, cancel, rate_limiter, health) -> Self
```

**New signature:**
```rust
pub fn new(config, instruments_rx: watch::Receiver<Vec<String>>, cancel, rate_limiter, health) -> Self
```

**Inner loop change:**
```rust
// Current: only select on cancel + message recv
// New: add instruments_rx.changed() branch

loop {
    tokio::select! {
        biased;
        _ = self.cancel.cancelled() => return,
        Ok(()) = self.instruments_rx.changed() => {
            tracing::info!("DeribitSupervisor: instrument list updated, reconnecting");
            metrics::counter!("feed_subscription_reconnects", "venue" => "deribit")
                .increment(1);
            break; // Break inner loop -> outer loop creates fresh client
        }
        msg = raw_rx.recv() => { /* existing forwarding logic */ }
    }
}
// After breaking inner loop, outer loop re-enters:
// let instruments = self.instruments_rx.borrow().clone();
// let client = DeribitClient::new(config, instruments, ...);
```

### PolymarketSupervisor Changes

**Structural difference:** Polymarket's subscription uses `config.assets` (a `Vec<PolymarketAsset>`). The watch channel carries `Vec<PolymarketSubscription>` which the supervisor converts to the config-compatible format before creating a fresh client.

**New field:**
```rust
pub struct PolymarketSupervisor {
    config: PolymarketConfig,
    assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,  // NEW
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
}
```

**Client creation change:** Before creating `PolymarketClient`, update `config.assets` with the latest from the watch channel:
```rust
let assets = self.assets_rx.borrow().clone();
let mut config = self.config.clone();
config.assets = assets.into_iter().map(|s| PolymarketAsset {
    condition_id: s.condition_id,
    token_id: s.token_id,
}).collect();
let client = PolymarketClient::new(config, self.cancel.clone());
```

### KalshiSupervisor Changes

**Structurally identical to Deribit:** Kalshi uses `config.market_tickers: Vec<String>`. The watch channel carries `Vec<String>`.

**Client creation change:** Before creating `KalshiClient`, update `config.market_tickers`:
```rust
let tickers = self.tickers_rx.borrow().clone();
let mut config = self.config.clone();
config.market_tickers = tickers;
let client = KalshiClient::new(config, ...);
```

## Pipeline.rs Changes

`run_live_multi_venue()` currently creates supervisors with config-derived instrument lists. It needs to accept watch receivers and pass them to supervisors.

**New parameter:**
```rust
pub struct SubscriptionChannels {
    pub deribit_rx: watch::Receiver<Vec<String>>,
    pub polymarket_rx: watch::Receiver<Vec<PolymarketSubscription>>,
    pub kalshi_rx: watch::Receiver<Vec<String>>,
}

async fn run_live_multi_venue(
    config: &VenuesConfig,
    credentials: &Credentials,
    recording_dir: PathBuf,
    cancel: CancellationToken,
    event_registry: Option<Arc<RwLock<EventRegistry>>>,
    subscription_channels: Option<SubscriptionChannels>,  // NEW
) -> anyhow::Result<PipelineHandles>
```

The `Option` wrapper maintains backward compatibility for Mock/Replay modes (which do not use dynamic subscriptions).

## Main.rs Wiring Changes

```rust
// After EventRegistry creation, before pipeline start:

// Create subscription watch channels
let initial_deribit: Vec<String> = { /* from registry active_approved */ };
let initial_poly: Vec<PolymarketSubscription> = { /* from registry */ };
let initial_kalshi: Vec<String> = { /* from registry */ };

let (deribit_sub_tx, deribit_sub_rx) = watch::channel(initial_deribit);
let (poly_sub_tx, poly_sub_rx) = watch::channel(initial_poly);
let (kalshi_sub_tx, kalshi_sub_rx) = watch::channel(initial_kalshi);

// Create Notify for refresh synchronization
let registry_refreshed = Arc::new(tokio::sync::Notify::new());

// Pass subscription channels to pipeline
let pipeline_handles = pipeline::run_multi_venue_pipeline(
    mode, &config.venues, &config.credentials, recording_dir,
    shutdown_token.clone(), Some(event_registry.clone()),
    Some(SubscriptionChannels { deribit_rx, poly_rx, kalshi_rx }),
).await?;

// Modify config hot-reload subscriber to notify after refresh:
// reg.refresh(&new_config.events);
// registry_refreshed.notify_one();  // <-- add this line

// Start SubscriptionManager (live mode only)
if is_live {
    let sub_manager = SubscriptionManager::new(
        event_registry.clone(),
        registry_refreshed.clone(),
        deribit_sub_tx, kalshi_sub_tx, poly_sub_tx,
        shutdown_token.child_token(),
    );
    tokio::spawn(sub_manager.run());
    tracing::info!("SubscriptionManager started");
}
```

## Patterns to Follow

### Pattern 1: Watch Channel for Dynamic Instrument Lists

**What:** `tokio::sync::watch` channels carry the latest instrument list from SubscriptionManager to each supervisor. Latest-value semantics means no queue buildup.

**Why watch, not mpsc:** Supervisors only need the latest full list, not a history of additions/removals. Watch provides exactly this -- the latest value, atomically readable.

**Implementation:**
```rust
let (tx, rx) = watch::channel(initial_instruments);
// SubscriptionManager: tx.send(new_list)
// Supervisor: rx.changed().await, then rx.borrow().clone()
```

### Pattern 2: Registry Diff for Minimal Reconnects

**What:** SubscriptionManager maintains `HashSet<String>` per venue of last-known instruments. On each registry refresh, computes symmetric difference and only triggers reconnects for venues with actual changes.

**Why:** Prevents unnecessary reconnections when non-subscription config changes occur (threshold tuning, risk weight changes, etc.).

### Pattern 3: Notify for Ordered Updates

**What:** `tokio::sync::Notify` ensures the SubscriptionManager reads the registry only after the config subscriber has refreshed it.

**Why:** Two independent subscribers to the same `watch::channel` have no ordering guarantee. Without Notify, the SubscriptionManager could read stale registry state.

### Pattern 4: Initial List from Registry, Not Config

**What:** On startup, instrument lists are derived from `EventRegistry::active_approved()`, not from `venues.toml` static config.

**Why:** Ensures consistency -- the registry is the single source of truth for what instruments should be subscribed. The static config becomes a fallback only.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Incremental Subscribe/Unsubscribe Commands to Supervisors

**What:** Adding mpsc channels to supervisors for "add instrument X" / "remove instrument Y" commands.
**Why bad:** Each venue has different WS subscription semantics (Deribit supports incremental, Polymarket likely does not, Kalshi unclear). Incremental management creates venue-divergent code paths, partial state, and edge cases around concurrent subscribe/reconnect.
**Do instead:** Push the full instrument list and trigger reconnect. Battle-tested reconnection handles the rest.

### Anti-Pattern 2: Reading Config Instead of Registry for Instrument Lists

**What:** SubscriptionManager reading `AppConfig.venues.deribit.instruments` to determine what to subscribe.
**Why bad:** The config's instrument list is manually maintained and may not reflect approved event mappings. The EventRegistry, built from events.toml, is the authoritative source for what instruments are active and approved.
**Do instead:** Always derive instrument lists from `registry.active_approved()`.

### Anti-Pattern 3: Hot-Path Subscription Checks

**What:** Checking subscription state inside `forward_snapshots()` on every snapshot.
**Why bad:** Subscription changes are rare (days between new events). Checking thousands of times per second wastes CPU.
**Do instead:** Subscription decisions happen only in the SubscriptionManager background task, triggered by config changes.

### Anti-Pattern 4: Modifying the Lifecycle Manager to Push Subscriptions

**What:** Having `ContractLifecycleManager` directly push subscription updates after its `refresh_registry()` call.
**Why bad:** Conflates discovery/expiry management with subscription management. The lifecycle manager writes to TOML; the subscription path goes through ConfigReloader -> Registry -> SubscriptionManager. Adding a direct push creates a second activation path and potential race conditions.
**Do instead:** Let the existing ConfigReloader -> Registry -> SubscriptionManager chain handle it. The lifecycle manager's `refresh_registry()` writes to events.toml, which triggers ConfigReloader, which triggers the full chain.

### Anti-Pattern 5: Reconnecting All Venues on Any Change

**What:** Reconnecting all three venue supervisors whenever any instrument list changes.
**Why bad:** If only a Deribit instrument is added, Polymarket and Kalshi should not reconnect.
**Do instead:** Per-venue diffing and per-venue watch channels. Only venues with actual changes reconnect.

## File Structure (New/Modified Only)

```
src/
+-- events/
|   +-- mod.rs              # Add: pub mod subscription;
|   +-- subscription.rs     # NEW: SubscriptionManager + PolymarketSubscription
|   +-- discovery.rs        # UNCHANGED
|   +-- lifecycle.rs        # UNCHANGED
|   +-- registry.rs         # UNCHANGED
|   +-- risk.rs             # UNCHANGED
|   +-- toml_writer.rs      # UNCHANGED
+-- feed/
|   +-- pipeline.rs         # MODIFIED: accept SubscriptionChannels, pass to supervisors
|   +-- deribit/
|   |   +-- supervisor.rs   # MODIFIED: accept watch::Receiver<Vec<String>>, add select! branch
|   |   +-- client.rs       # UNCHANGED (receives instruments via constructor, no change)
|   +-- polymarket/
|   |   +-- supervisor.rs   # MODIFIED: accept watch::Receiver<Vec<PolymarketSubscription>>
|   |   +-- client.rs       # UNCHANGED (reads config.assets, no change)
|   +-- kalshi/
|       +-- supervisor.rs   # MODIFIED: accept watch::Receiver<Vec<String>>
|       +-- client.rs       # UNCHANGED (reads config.market_tickers, no change)
+-- main.rs                 # MODIFIED: wire SubscriptionManager, create watch channels
```

### Lines of Code Estimate

| File | Change Type | Estimated LOC |
|------|-----------|--------------|
| `events/subscription.rs` | New | ~200 |
| `events/mod.rs` | Add module declaration | ~2 |
| `feed/deribit/supervisor.rs` | Add watch receiver + select branch | ~30 |
| `feed/polymarket/supervisor.rs` | Add watch receiver + select branch | ~35 |
| `feed/kalshi/supervisor.rs` | Add watch receiver + select branch | ~30 |
| `feed/pipeline.rs` | Accept and pass subscription channels | ~40 |
| `main.rs` | Wire SubscriptionManager, channels, Notify | ~50 |
| Tests (unit + integration) | New | ~200 |
| **Total** | | **~590** |

This is modest for the functionality delivered -- about 1.7% of the existing codebase.

## Build Order (Dependency-Aware)

### Phase A: SubscriptionManager Core (read-only observer)

**Build** `events/subscription.rs` as a read-only observer that logs diffs but does not push them.

1. Define `SubscriptionManager` struct and `PolymarketSubscription` type
2. Implement `reconcile()`: read registry, compute per-venue sets, diff against last-known
3. Implement `run()`: loop on `Notify`, call reconcile, log diffs
4. Wire in `main.rs`: create `Notify`, add `notify_one()` to config subscriber, spawn manager
5. Unit tests: verify diff detection for additions, removals, no-change cases

**Verifiable milestone:** Run the system, approve a mapping in events.toml, observe structured log "Deribit subscription change: +[BTC-27JUN25-120000-C] -[]". No actual subscription change yet.

**Dependencies:** None -- purely additive, does not modify any existing components.

### Phase B: Supervisor Dynamic Instrument Lists

**Modify** each supervisor to accept and react to watch channels.

1. Modify `DeribitSupervisor::new()` to accept `watch::Receiver<Vec<String>>`
2. Add `instruments_rx.changed()` branch to inner `select!` -- break to reconnect
3. On outer loop entry, `borrow()` latest instruments for client creation
4. Repeat for `PolymarketSupervisor` (with `PolymarketSubscription` type)
5. Repeat for `KalshiSupervisor`
6. Update `run_live_multi_venue()` in `pipeline.rs` to accept and pass channels

**Verifiable milestone:** Unit test that sends a new instrument list to a supervisor and verifies it attempts reconnection.

**Dependencies:** Phase A (SubscriptionManager struct must exist to create the sender side of channels).

### Phase C: End-to-End Wiring

**Wire** everything together in `main.rs` and pipeline.

1. Create watch channels in `main.rs`, pass senders to SubscriptionManager, receivers to pipeline
2. Derive initial instrument lists from registry (not from venues.toml config)
3. SubscriptionManager pushes actual updates (not just logging)
4. Verify startup: system subscribes based on registry active_approved
5. Verify dynamic add: approve a mapping, supervisor reconnects, snapshots arrive
6. Verify dynamic remove: mark mapping expired, supervisor reconnects without it

**Verifiable milestone:** Full end-to-end: discovery proposes mapping -> operator approves -> supervisor reconnects -> spreads appear for new event (without restart).

**Dependencies:** Phase A + Phase B complete.

### Phase D: Metrics, Edge Cases, Hardening

1. Prometheus metrics: `subscription_activations_total`, `subscription_removals_total`, `feed_subscription_reconnects_total`
2. Edge case: what if registry has no active instruments for a venue? (Supervisor should still run but subscribe to nothing)
3. Edge case: rapid config changes (debounce already handled by ConfigReloader's 500ms debounce)
4. Edge case: SubscriptionManager starts before registry is populated (initial reconcile handles this)
5. Integration test: full lifecycle from discovery to subscription to retirement

**Dependencies:** Phase C complete.

## Scalability Considerations

| Concern | Current (5-10 events) | At 50 events | At 500 events |
|---------|----------------------|--------------|---------------|
| Registry read lock duration in SubscriptionManager | Sub-ms | ~1ms | ~5ms |
| Per-venue diff computation | Sub-ms | Sub-ms | ~1ms (HashSet diff) |
| Supervisor reconnect time | ~3s per venue | Same | Same (one reconnect per venue) |
| Watch channel memory | Negligible (Vec<String>) | Negligible | ~50KB per venue |
| Subscription change frequency | Rare (days) | Rare | Rare (human approval gate) |

**No scaling concerns at any realistic scale.** The human approval gate ensures subscription changes are infrequent regardless of discovery volume.

## Integration with Tech Debt Sweep

The v1.3 milestone also includes tech debt cleanup. The subscription management architecture has no conflicts with any of the 15 known tech debt items. However, two items are directly relevant:

1. **"Deribit instrument BTC-27JUN25-100000-C expired June 2025"** -- Once dynamic subscriptions are active, the instrument list in `venues.toml` becomes secondary. This tech debt resolves itself: the registry-derived list will contain only active instruments.

2. **"Kalshi market_tickers = []"** -- Same as above: the empty default in `venues.toml` becomes irrelevant when instrument lists come from the registry.

## Sources

- Direct codebase analysis: `src/feed/deribit/supervisor.rs` (182 lines)
- Direct codebase analysis: `src/feed/polymarket/supervisor.rs` (141 lines)
- Direct codebase analysis: `src/feed/kalshi/supervisor.rs` (163 lines)
- Direct codebase analysis: `src/feed/deribit/client.rs` (311 lines)
- Direct codebase analysis: `src/feed/polymarket/client.rs` (184 lines)
- Direct codebase analysis: `src/feed/kalshi/client.rs` (292 lines)
- Direct codebase analysis: `src/feed/pipeline.rs` (474 lines)
- Direct codebase analysis: `src/events/registry.rs` (429 lines)
- Direct codebase analysis: `src/events/lifecycle.rs` (1015+ lines)
- Direct codebase analysis: `src/config/reload.rs` (119 lines)
- Direct codebase analysis: `src/config/venues.rs` (172 lines)
- Direct codebase analysis: `src/config/events.rs` (348 lines)
- Direct codebase analysis: `src/main.rs` (795 lines)
- [tokio::sync::watch documentation](https://docs.rs/tokio/latest/tokio/sync/watch/index.html) -- watch channel semantics
- [Deribit API Documentation](https://docs.deribit.com/) -- confirms dynamic subscribe/unsubscribe support
- Prior v1.2 architecture research (`.planning/research/ARCHITECTURE.md`, dated 2026-02-26)

---
*Architecture research for: Dynamic Subscription Management (v1.3)*
*Researched: 2026-02-27*
