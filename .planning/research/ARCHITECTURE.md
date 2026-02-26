# Architecture Research: Automated Event Discovery Integration

**Domain:** Event discovery and cross-venue matching for existing arbitrage pipeline
**Researched:** 2026-02-26
**Confidence:** HIGH (based on direct codebase analysis of 32K+ LOC existing system)

## Executive Summary

The v1.2 Automated Event Management milestone adds event discovery, cross-venue matching, TOML proposal writing, expired event retirement, and live subscription management to an existing production-grade arbitrage signal generator. The critical architectural constraint is: **the hot path (Feeds -> SpreadEngine -> SignalEngine) must never be disrupted** by discovery activity.

Analysis of the existing codebase reveals that the foundation for this milestone is already substantially built. The `ContractLifecycleManager` (lifecycle.rs, 593 lines), `discovery.rs` (981 lines), and `toml_writer.rs` (303 lines) already implement the core discovery loop, REST API polling, cross-venue matching, TOML append/expire, and registry refresh. What remains is the **feed subscription management gap**: when a user approves a newly discovered mapping (sets `approved = true` in events.toml), the system must detect this config change and subscribe to the new instruments on the appropriate venue feeds -- without restarting.

## Existing Architecture (Current State)

```
                        +-----------------------+
                        |   ConfigReloader      |
                        |  (OS thread, notify)  |
                        |  watches config/*.toml|
                        +----------+------------+
                                   |
                              watch::channel
                                   |
                                   v
+-------------------+    +-------------------+    +-------------------+
| ContractLifecycle |    |   EventRegistry   |    |  Config Hot-Reload|
| Manager           |--->|  Arc<RwLock<...>> |<---| Subscriber Task   |
| (tokio task)      |    |  dual-index lookup|    | (in main.rs)      |
+-------------------+    +-------------------+    +-------------------+
  | REST polls venues          ^    |
  | appends to events.toml     |    | lookup_by_instrument()
  | marks expired              |    v
  | refreshes registry    +----+--------+
  v                       | forward_    |
+-------------------+     | snapshots() |
| events.toml       |     | annotates   |    Hot Path (never block)
| (atomic write)    |     | event_id    |    ========================
+-------------------+     +------+------+
                                 |           [DeribitSupervisor]  --->  [DeribitProcessor]  --+
                                 |           [PolySupervisor]     --->  [PolyProcessor]    --+--> fan-in
                                 |           [KalshiSupervisor]  --->  [KalshiProcessor]  --+     |
                                 |                                                                v
                                 +------>  [SnapshotFanOut] --> SpreadEngine --> SignalEngine
                                                            --> PricingEngine --> CrossAssetEngine
                                                            --> PaperTradeTracker
```

### Component Responsibilities (Existing)

| Component | Responsibility | Hot Path? |
|-----------|---------------|-----------|
| `ContractLifecycleManager` | Periodic REST polling, discovery, matching, TOML writes, registry refresh, basis risk cache | No |
| `EventRegistry` | O(1) instrument-to-event lookup, shared via `Arc<RwLock<>>` | Read-only in hot path |
| `ConfigReloader` | OS-thread file watcher, parses TOML, distributes via `watch::channel` | No |
| `Config Hot-Reload Subscriber` | Receives new `AppConfig`, calls `registry.refresh()` | No |
| `forward_snapshots()` | Annotates `event_id` on each snapshot via registry read lock | Yes (read lock) |
| `DeribitSupervisor` | Manages WS connection, exponential backoff reconnect | Yes |
| `SpreadEngine` | Spread calculation, signal generation, blocking send on primary channel | Yes (primary) |

## What Already Exists (Built in Prior Milestones)

### 1. Discovery (Complete)

`src/events/discovery.rs` provides:
- `discover_deribit()` -- polls `GET /api/v2/public/get_instruments?currency=BTC&kind=option`
- `discover_kalshi()` -- polls `GET /trade-api/v2/markets?series_ticker=KXBTC&status=open` with RSA-PSS auth
- `discover_polymarket()` -- polls Gamma API for deactivation monitoring only (no structured field extraction)
- `DiscoveredInstrument` -- normalized struct with venue, instrument_id, asset, strike, expiry, direction
- `MatchKey` -- four-field exact matching (asset + strike + expiry + direction)
- `find_cross_venue_candidates()` -- groups by MatchKey, returns groups with 2+ venues
- `filter_new_candidates()` -- excludes already-registered mappings
- `flag_novel_instruments()` -- identifies single-venue unmatched instruments

### 2. Cross-Venue Matching (Complete)

The matching logic uses exact four-field matching after normalization (not fuzzy text). This is a deliberate design decision per the codebase: Deribit provides structured fields (strike, option_type, expiry), Kalshi provides floor_strike/cap_strike, so no NLP is needed.

### 3. TOML Writing (Complete)

`src/events/toml_writer.rs` provides:
- `append_candidate_to_toml()` -- preserves formatting/comments via `toml_edit`, adds `approved = false`
- `mark_expired_in_toml()` -- sets `status = "expired"` by event_id

### 4. Lifecycle Manager (Complete)

`src/events/lifecycle.rs` orchestrates the full cycle:
1. Poll each venue on independent intervals
2. Find cross-venue candidates
3. Append new candidates to events.toml (approved = false)
4. Flag novel unmatched instruments
5. Detect expired instruments
6. Handle Deribit expiry rolls (create new candidate for next expiry)
7. Apply near-expiry warnings
8. Populate BasisRiskCache
9. Refresh EventRegistry from updated TOML

### 5. EventRegistry.refresh() (Handles New Entries)

Key finding: `EventRegistry::refresh()` performs a full rebuild -- clears all mappings and indexes, replaces with new config. This means it **already handles new entries**, not just parameter changes. When a new mapping is added to events.toml and the registry is refreshed, the new mapping appears in `active_approved()` if `approved = true` and `status = active`.

## The Gap: Feed Subscription Management

### Problem Statement

When the user approves a newly discovered mapping (edits events.toml, sets `approved = true`), the following happens today:
1. ConfigReloader detects the file change
2. Config hot-reload subscriber calls `registry.refresh()`
3. EventRegistry now contains the new active+approved mapping
4. `forward_snapshots()` can annotate snapshots with the new event_id

**But**: No venue supervisor subscribes to the new instrument's WebSocket channel. Snapshots for the new instrument never arrive. The mapping is registered but produces no data.

### Current Subscription Model

Each venue supervisor receives its instrument list at construction time:

```rust
// DeribitSupervisor::new() -- instruments are fixed at creation
pub fn new(
    config: DeribitConfig,
    instruments: Vec<String>,  // <-- fixed list
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,
    health: Arc<VenueHealth>,
) -> Self { ... }
```

The `DeribitConfig.instruments` list comes from `venues.toml` at startup. There is no mechanism to add instruments to a running supervisor.

Similarly for Kalshi and Polymarket, the market lists are either configured at startup or determined by the supervisor's own logic.

## Recommended Architecture

### Design Principle: Sidecar Subscription, Not Inline Mutation

Do NOT modify the existing supervisors to accept dynamic instrument lists via mpsc commands. Instead, add a **SubscriptionManager** that runs as a sibling background task and coordinates new subscriptions through the existing reconnection mechanism.

### Approach: Subscription via Targeted Reconnect

When a new instrument is approved, the simplest and safest approach is:
1. Detect the new approved instrument via registry diff
2. Update the supervisor's instrument list (via a `watch::channel`)
3. Trigger a graceful reconnect of the affected venue supervisor

This works because:
- Supervisors already handle reconnection with backoff
- On reconnect, they re-subscribe to all instruments
- Adding one instrument to the list and reconnecting gets the new subscription
- The reconnect gap is seconds, acceptable for minute-to-hour arb windows
- No changes to the hot path

### System Overview with v1.2 Additions

```
                    +----------------------------+
                    |     ConfigReloader          |
                    |  (watches config/*.toml)    |
                    +-------+--------------------+
                            |
                       watch::channel (AppConfig)
                            |
              +-------------+-------------+
              |                           |
              v                           v
+---------------------------+   +-----------------------+
| Config Hot-Reload Sub     |   | SubscriptionManager   |  <--- NEW
| (existing task)           |   | (new tokio task)      |
| - refreshes EventRegistry|   | - diffs registry      |
+---------------------------+   | - updates instrument  |
                                |   lists per venue     |
                                | - triggers reconnect  |
                                |   via venue channels  |
                                +-----------+-----------+
                                            |
                          +-----------------+-----------------+
                          |                 |                 |
                          v                 v                 v
                 [DeribitSupervisor] [PolySupervisor] [KalshiSupervisor]
                  instruments:        markets:          tickers:
                  watch::Receiver     watch::Receiver   watch::Receiver
                          |                 |                 |
                          v                 v                 v
                 [DeribitProcessor] [PolyProcessor]  [KalshiProcessor]
                          |                 |                 |
                          +---------+-------+---------+------+
                                    |
                               fan-in mpsc
                                    |
                                    v
                            [SnapshotFanOut]
                                    |
                        +-----------+-----------+
                        |           |           |
                   SpreadEngine PricingEngine CrossAssetEngine
```

### Component Boundaries

| Component | Responsibility | New/Modified | Communicates With |
|-----------|---------------|-------------|-------------------|
| `SubscriptionManager` | Watches registry for newly approved mappings, computes instrument diffs, pushes updated lists to supervisors | **NEW** | EventRegistry (read), Venue Supervisors (instrument update channel) |
| `DeribitSupervisor` | Accept dynamic instrument list via `watch::Receiver<Vec<String>>`, reconnect when list changes | **MODIFIED** (minor) | SubscriptionManager, DeribitClient |
| `PolymarketSupervisor` | Accept dynamic market list, reconnect when list changes | **MODIFIED** (minor) | SubscriptionManager, PolymarketClient |
| `KalshiSupervisor` | Accept dynamic ticker list, reconnect when list changes | **MODIFIED** (minor) | SubscriptionManager, KalshiClient |
| `ContractLifecycleManager` | Existing discovery/matching/TOML-writing cycle (no changes needed) | **UNCHANGED** | EventRegistry, events.toml |
| `EventRegistry` | Existing dual-index registry with refresh() | **UNCHANGED** | All readers |
| `ConfigReloader` | Existing file watcher | **UNCHANGED** | Config subscribers |
| `forward_snapshots()` | Existing event_id annotation | **UNCHANGED** | EventRegistry (read lock) |
| `SpreadEngine` | Existing spread calculation | **UNCHANGED** | Snapshot fan-in |

## Data Flow: Discovery to Subscription

### Full Lifecycle of a New Event

```
Phase 1: Discovery (automated, runs continuously)
================================================================
ContractLifecycleManager.poll_cycle()
    |
    +---> discover_deribit() --> DiscoveredInstrument[]
    +---> discover_kalshi()  --> DiscoveredInstrument[]
    |
    +---> find_cross_venue_candidates() --> MatchKey groups
    +---> filter_new_candidates() --> CandidateMapping[]
    |
    +---> append_candidate_to_toml() --> events.toml updated
    |        (approved = false, discovered_at = now)
    |
    +---> refresh_registry() --> EventRegistry now has unapproved entry
    |
    +---> tracing::info!("discovered new candidate mapping")
    +---> metrics: lifecycle_candidates_discovered++


Phase 2: Operator Review (manual, human-in-the-loop)
================================================================
Operator sees structured log: "discovered new candidate mapping"
    |
    +---> Reviews events.toml
    +---> Optionally fills in Polymarket condition_id/token_id
    +---> Sets approved = true
    +---> Saves file


Phase 3: Activation (automated, triggered by file save)
================================================================
ConfigReloader detects events.toml change
    |
    +---> Parses all config --> new AppConfig
    +---> watch::channel.send(new_config)
    |
    +---> Config Hot-Reload Subscriber:
    |        registry.refresh(new_config.events)
    |        Log: "EventRegistry refreshed, N mappings, M active"
    |
    +---> SubscriptionManager (NEW):
             Reads registry.active_approved()
             Compares with current subscription set
             For each venue with new instruments:
                 Push updated instrument list via watch::channel
                 Log: "new subscription: {venue} {instrument}"
                 Metric: subscription_activations++


Phase 4: Feed Subscription (automated, triggered by instrument list change)
================================================================
DeribitSupervisor detects instrument list change (watch::changed())
    |
    +---> Graceful disconnect of current WS
    +---> Reconnect with updated instrument list
    +---> Subscribe to all instruments (including new one)
    +---> First message arrives, backoff resets
    |
    +---> DeribitProcessor normalizes messages
    +---> forward_snapshots() annotates event_id from registry
    +---> Snapshot enters fan-in channel
    +---> SpreadEngine begins calculating spreads for new event


Phase 5: Retirement (automated, detected by lifecycle manager)
================================================================
ContractLifecycleManager detects instrument no longer in venue API
    |
    +---> mark_expired_in_toml() --> status = "expired"
    +---> Optionally: handle_deribit_roll() for expiry rolls
    +---> refresh_registry()
    |
    +---> SubscriptionManager detects removed instrument
    +---> Push updated instrument list (without expired)
    +---> Supervisor reconnects with pruned list
```

## Architectural Patterns

### Pattern 1: Watch Channel for Dynamic Instrument Lists

**What:** Use `tokio::sync::watch` channels to push updated instrument lists from SubscriptionManager to each venue supervisor. The supervisor polls `changed()` in its reconnection loop.

**When to use:** When a background task needs to dynamically update a long-lived connection manager without disrupting its current operation.

**Trade-offs:**
- Pro: Lock-free reads, single-producer simplicity, latest-value semantics (no queue buildup)
- Pro: Supervisor can check for changes at natural reconnection boundaries
- Con: If supervisor is mid-connection, it must wait for next reconnection cycle (unless we add a periodic check)

**Example:**
```rust
// SubscriptionManager holds the sender
let (deribit_instruments_tx, deribit_instruments_rx) =
    watch::channel(initial_deribit_instruments);

// DeribitSupervisor receives the receiver
impl DeribitSupervisor {
    pub async fn run(self, tx: mpsc::Sender<RawMessage>) {
        loop {
            // Check for instrument list changes before each connection
            let instruments = self.instruments_rx.borrow().clone();

            let client = DeribitClient::new(
                self.config.clone(),
                instruments,  // Use latest list
                self.cancel.clone(),
            );

            match client.start().await {
                Ok(mut raw_rx) => {
                    // Forward messages, but also watch for list changes
                    loop {
                        tokio::select! {
                            biased;
                            _ = self.cancel.cancelled() => return,
                            _ = self.instruments_rx.changed() => {
                                // New instruments -- trigger graceful reconnect
                                tracing::info!("instrument list updated, reconnecting");
                                break;
                            }
                            msg = raw_rx.recv() => {
                                match msg {
                                    Some(raw) => { /* forward */ }
                                    None => break, // connection lost
                                }
                            }
                        }
                    }
                }
                Err(e) => { /* backoff */ }
            }
        }
    }
}
```

### Pattern 2: Registry Diff for Subscription Changes

**What:** SubscriptionManager maintains a snapshot of the last-known active instrument set per venue. On each registry refresh notification, it computes the diff and only triggers reconnects for venues with actual changes.

**When to use:** To avoid unnecessary reconnections when non-subscription config changes occur (e.g., risk weight tuning, threshold changes).

**Trade-offs:**
- Pro: Minimizes feed disruption from unrelated config changes
- Pro: Clear audit trail of what changed and when
- Con: Small additional memory for last-known set (negligible)

**Example:**
```rust
struct SubscriptionManager {
    registry: Arc<RwLock<EventRegistry>>,
    config_rx: watch::Receiver<AppConfig>,
    // Per-venue instrument senders
    deribit_tx: watch::Sender<Vec<String>>,
    kalshi_tx: watch::Sender<Vec<String>>,
    polymarket_tx: watch::Sender<Vec<String>>,
    // Last-known instrument sets for diffing
    last_deribit: HashSet<String>,
    last_kalshi: HashSet<String>,
    last_polymarket: HashSet<String>,
}

impl SubscriptionManager {
    async fn on_registry_change(&mut self) {
        let registry = self.registry.read().await;
        let mut new_deribit = HashSet::new();
        let mut new_kalshi = HashSet::new();
        let mut new_polymarket = HashSet::new();

        for mapping in registry.active_approved() {
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
        drop(registry);

        if new_deribit != self.last_deribit {
            let added: Vec<_> = new_deribit.difference(&self.last_deribit).collect();
            let removed: Vec<_> = self.last_deribit.difference(&new_deribit).collect();
            tracing::info!(?added, ?removed, "Deribit subscriptions changed");
            let _ = self.deribit_tx.send(new_deribit.iter().cloned().collect());
            self.last_deribit = new_deribit;
        }
        // Same for kalshi, polymarket...
    }
}
```

### Pattern 3: Non-Blocking Registry Access in Hot Path

**What:** The existing pattern of `RwLock` read access in `forward_snapshots()` is correct and must be preserved. Discovery writes (via `ContractLifecycleManager`) and config reloads (via subscriber task) take write locks, but these are infrequent (every 5-10 minutes) and complete quickly (Vec swap + index rebuild). The hot path only takes read locks.

**When to use:** Already in use. Document to prevent regression.

**Trade-offs:**
- Pro: Multiple concurrent readers, no hot-path contention
- Pro: Write locks are rare (every poll cycle, ~5 min) and fast (~microseconds for index rebuild)
- Con: Write lock during refresh blocks all reads momentarily (acceptable given sub-ms duration)

## Recommended Project Structure (New/Modified Files Only)

```
src/
├── events/
│   ├── mod.rs              # Add: pub mod subscription;
│   ├── discovery.rs        # UNCHANGED (already complete)
│   ├── lifecycle.rs        # UNCHANGED (already complete)
│   ├── registry.rs         # UNCHANGED (already complete)
│   ├── risk.rs             # UNCHANGED
│   ├── toml_writer.rs      # UNCHANGED (already complete)
│   └── subscription.rs     # NEW: SubscriptionManager
├── feed/
│   ├── pipeline.rs         # MODIFIED: pass instrument watch channels to supervisors
│   ├── deribit/
│   │   └── supervisor.rs   # MODIFIED: accept watch::Receiver<Vec<String>>
│   ├── polymarket/
│   │   └── supervisor.rs   # MODIFIED: accept watch::Receiver for market list
│   └── kalshi/
│       └── supervisor.rs   # MODIFIED: accept watch::Receiver for ticker list
├── config/
│   ├── events.rs           # UNCHANGED (EventMapping, DiscoveryConfig already defined)
│   └── venues.rs           # May add: initial_instruments computed from events.toml
└── main.rs                 # MODIFIED: wire SubscriptionManager, pass watch channels
```

### Structure Rationale

- **events/subscription.rs:** New module colocated with registry and discovery because it bridges the two -- reads registry state, decides what subscriptions should exist.
- **Supervisor modifications are minimal:** Add a `watch::Receiver` field and a `select!` branch. The reconnection logic already exists; we just add a new trigger for it.
- **pipeline.rs changes:** The `run_live_multi_venue()` function creates supervisors. It needs to accept and pass the watch receivers from SubscriptionManager.

## Integration Points

### External Services

| Service | Integration Pattern | Impact of v1.2 Changes |
|---------|---------------------|------------------------|
| Deribit WS | Supervisor reconnects with new instrument list | Reconnect gap ~2-5s (acceptable) |
| Kalshi WS | Supervisor reconnects with new ticker list | Same as Deribit |
| Polymarket WS | Supervisor reconnects with new market list | Same as Deribit |
| Deribit REST | Discovery polling (already implemented) | No change |
| Kalshi REST | Discovery polling (already implemented) | No change |
| Polymarket Gamma | Discovery polling (already implemented) | No change |

### Internal Boundaries

| Boundary | Communication | Direction | Notes |
|----------|---------------|-----------|-------|
| SubscriptionManager -> Supervisors | `watch::channel<Vec<String>>` | Push | One channel per venue |
| ConfigReloader -> SubscriptionManager | `watch::channel<AppConfig>` | Push | Same channel as existing config subscriber |
| SubscriptionManager -> EventRegistry | `Arc<RwLock<EventRegistry>>` read | Pull | Read-only, same pattern as forward_snapshots |
| ContractLifecycleManager -> events.toml | Atomic file write | Write | Triggers ConfigReloader |
| ConfigReloader -> EventRegistry | `watch::channel` -> subscriber -> `registry.refresh()` | Push | Existing mechanism, unchanged |

### Critical Ordering Constraint

The activation flow must be:
1. ConfigReloader detects change
2. Config subscriber refreshes EventRegistry (write lock)
3. SubscriptionManager reads refreshed registry (read lock)
4. SubscriptionManager pushes new instrument lists
5. Supervisors reconnect with new lists

Steps 2 and 3 must be sequential. The simplest approach: SubscriptionManager subscribes to the same `watch::channel<AppConfig>` and reads the registry after the config subscriber has refreshed it. Use a small delay (100ms) or have the config subscriber explicitly notify the SubscriptionManager via a separate channel after refresh completes.

**Recommended:** Have the SubscriptionManager subscribe to `watch::channel<AppConfig>` directly and call its own `registry.read()` after receiving notification. Since `watch` delivers the latest value and the config subscriber processes it first (spawned earlier), a brief `tokio::task::yield_now()` or `tokio::time::sleep(Duration::from_millis(50))` ensures ordering. Alternatively, use a dedicated `tokio::sync::Notify` triggered after registry refresh.

## Anti-Patterns

### Anti-Pattern 1: Modifying Supervisors to Accept mpsc Commands

**What people do:** Add an mpsc channel to supervisors for "add instrument" / "remove instrument" commands, requiring the supervisor to manage incremental subscription changes mid-connection.
**Why it is wrong:** Each venue's WebSocket protocol has different subscription semantics. Deribit supports dynamic subscribe/unsubscribe, but Polymarket and Kalshi may not. Incremental subscription management adds complexity to each supervisor and creates divergent code paths.
**Do this instead:** Push the full instrument list and trigger a reconnect. Reconnection is already battle-tested. The subscription gap of 2-5 seconds is irrelevant for minute-to-hour arbitrage windows.

### Anti-Pattern 2: Discovery Writing Directly to Registry

**What people do:** Have ContractLifecycleManager write directly to the in-memory EventRegistry without going through events.toml.
**Why it is wrong:** The TOML file is the source of truth. If the process restarts, in-memory-only changes are lost. The approval workflow requires human review of the file. Bypass creates split-brain between file and memory state.
**Do this instead:** Always write to events.toml first, then refresh from file. This is already how the system works -- preserve it.

### Anti-Pattern 3: Hot-Path Subscription Checks

**What people do:** Check "should I subscribe to this instrument?" inside `forward_snapshots()` or the fan-out task on every snapshot.
**Why it is wrong:** Subscription decisions are infrequent (new events appear every few days). Checking on every snapshot (thousands per second) wastes CPU and adds latency to the hot path.
**Do this instead:** Subscription decisions happen in the SubscriptionManager background task, which runs only on config change events.

### Anti-Pattern 4: Automatic Approval

**What people do:** Auto-approve discovered mappings to reduce operator intervention.
**Why it is wrong:** Cross-venue matching can produce false positives (similar but non-equivalent instruments). Auto-approval would subscribe to incorrect instruments and generate false signals. The system explicitly uses `approved = false` as a human gate.
**Do this instead:** Keep the manual approval gate. Reduce friction by providing clear structured logs with all mapping details so the operator can quickly verify and approve.

## Scaling Considerations

| Concern | Current (5-10 events) | At 50 events | At 500 events |
|---------|----------------------|--------------|---------------|
| Registry refresh time | Sub-ms | ~1ms | ~10ms (HashMap rebuild) |
| Registry read lock contention | None (rare writes) | None | Possible brief pauses during refresh |
| Supervisor reconnect time | ~3s per venue | Same (~3s) | Same (one reconnect per venue) |
| Discovery API calls | 3 venues * 1 request each | Same (paginated Kalshi) | Pagination increases Kalshi calls |
| TOML file size | ~2KB | ~20KB | ~200KB (toml_edit parsing may slow) |

### Scaling Priorities

1. **First bottleneck (200+ events):** TOML file parsing with `toml_edit` preserving formatting. If events.toml grows large, the `append_candidate_to_toml()` function parses the entire file for each append. Mitigation: batch appends per poll cycle (already done -- lifecycle manager appends all candidates, then refreshes once).

2. **Second bottleneck (1000+ instruments per venue):** WebSocket subscription message size. Deribit subscribe messages with 1000+ channels may exceed message size limits or take significant time to process. Mitigation: not a concern for BTC-only (typically <100 active options).

## Build Order (Dependency-Aware)

### Phase 1: SubscriptionManager Core (no supervisor changes yet)

Build the SubscriptionManager as a read-only observer first:
1. Create `events/subscription.rs`
2. Subscribe to config watch channel
3. Read EventRegistry on each change
4. Compute per-venue instrument diffs
5. Log additions and removals
6. Emit metrics (subscription_activations, subscription_removals)

**Verifiable:** Run the system, approve a mapping in events.toml, observe structured logs showing the diff. No actual subscription change yet -- just detection.

### Phase 2: Supervisor Dynamic Instrument Lists

Modify supervisors to accept `watch::Receiver<Vec<String>>`:
1. Add `instruments_rx: watch::Receiver<Vec<String>>` to `DeribitSupervisor`
2. Add `instruments_rx.changed()` branch to the `select!` in the forwarding loop
3. On change, break the inner loop to trigger reconnect with new list
4. Update `pipeline.rs` to create and pass watch channels
5. Repeat for Kalshi and Polymarket supervisors

**Verifiable:** Approve a mapping, observe supervisor reconnect log, then observe snapshots arriving for the new instrument.

### Phase 3: Wire End-to-End in main.rs

1. Create watch channels for each venue in `main.rs`
2. Pass senders to SubscriptionManager
3. Pass receivers to `run_live_multi_venue()` / supervisors
4. Ensure SubscriptionManager starts after config subscriber
5. Handle startup: initial instrument list comes from registry's active_approved

**Verifiable:** Full end-to-end: discovery proposes, operator approves, supervisor reconnects, spreads appear for new event.

### Phase 4: Expired Event Unsubscription

1. When SubscriptionManager detects removed instruments (expired/retired), push updated list
2. Supervisor reconnects without the expired instrument
3. Spread/signal engines naturally stop receiving snapshots for that instrument

**Verifiable:** Mark an event as expired, observe supervisor reconnect, confirm no more snapshots for that instrument.

## Sources

- Direct codebase analysis of `src/events/lifecycle.rs` (593 lines)
- Direct codebase analysis of `src/events/discovery.rs` (981 lines)
- Direct codebase analysis of `src/events/toml_writer.rs` (303 lines)
- Direct codebase analysis of `src/events/registry.rs` (386 lines)
- Direct codebase analysis of `src/feed/pipeline.rs` (474 lines)
- Direct codebase analysis of `src/feed/deribit/supervisor.rs` (182 lines)
- Direct codebase analysis of `src/config/reload.rs` (118 lines)
- Direct codebase analysis of `src/main.rs` (791 lines)
- Direct codebase analysis of `config/events.toml` (active configuration)

---
*Architecture research for: Automated Event Discovery Integration (v1.2)*
*Researched: 2026-02-26*
