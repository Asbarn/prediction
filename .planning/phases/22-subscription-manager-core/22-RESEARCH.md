# Phase 22: Subscription Manager Core - Research

**Researched:** 2026-02-27
**Domain:** Async subscription reconciliation, tokio channel orchestration, config-driven instrument management
**Confidence:** HIGH

## Summary

Phase 22 introduces a `SubscriptionManager` component that bridges config hot-reload to feed supervisors. The core problem is: when `events.toml` changes (operator approves a mapping, discovery adds a candidate, lifecycle archives an event), the system must compute which instruments to add and remove per venue, then push the updated instrument lists to supervisors so they reconnect with the correct subscriptions.

The existing infrastructure already provides the key building blocks: `ConfigReloader` watches `events.toml` and publishes `AppConfig` via `tokio::sync::watch`, `EventRegistry` can `refresh()` from new config and filter via `active_approved()`, and all three venue supervisors accept instrument lists at construction time. The gap is that supervisors currently receive static instrument lists at startup and never update them. The solution uses `tokio::sync::watch` channels to push per-venue instrument lists from the SubscriptionManager to supervisors, and `tokio::sync::Notify` to guarantee registry refresh completes before subscription reconciliation reads registry state.

**Primary recommendation:** Create a `SubscriptionManager` struct in a new `src/subscription/` module that owns the reconciliation logic, watch channel senders, and diff computation. Wire it into `main.rs` between the config reload subscriber and the pipeline setup. Supervisors receive `watch::Receiver` handles at construction and check for updates at the top of their reconnect loop.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SUB-03 | Config change to events.toml triggers automatic reconciliation that computes per-venue instrument diffs and issues minimal subscribe/unsubscribe actions | SubscriptionManager reconcile() method computes set difference between current and desired instruments per venue, logs diff, sends updated list via watch channels. Reconnect-based approach means "subscribe/unsubscribe" = reconnect with new list. |
| SUB-04 | Registry refresh completes before subscription reconciliation reads registry state (ordering guarantee) | tokio::sync::Notify used in main.rs config reload subscriber: registry.refresh() completes, then notify.notify_one() wakes SubscriptionManager which reads the fresh registry state. See Architecture Pattern 1. |
| SUB-06 | Reconnect-based subscription uses latest instrument list from registry, not static startup config | Supervisors hold watch::Receiver<Vec<String>> (or equivalent per-venue type). At the top of their reconnect loop, they call receiver.borrow().clone() to get the latest list. See Architecture Pattern 3. |
| OBS-03 | Structured tracing logs emit subscription diffs on each reconciliation (instruments added/removed per venue) | reconcile() method emits tracing::info! with structured fields: venue, added (list), removed (list), total_active (count). See Code Example 3. |
| OPS-02 | Only instruments from active_approved() event mappings are subscribed (safety gate preserved) | SubscriptionManager::compute_desired_instruments() calls registry.active_approved() and extracts per-venue instrument IDs. Unapproved and non-Active mappings are never included. See Code Example 1. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio::sync::watch | tokio 1.x (already in Cargo.toml) | Push latest instrument list to supervisors | Latest-value semantics (receivers always see most recent value); project decision from v1.3 roadmap |
| tokio::sync::Notify | tokio 1.x (already in Cargo.toml) | Registry-before-reconciliation ordering | Lightweight wakeup signal; project decision from v1.3 roadmap |
| tracing | 0.1 (already in Cargo.toml) | Structured diff logging | Project standard for all observability |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::collections::HashSet | stdlib | Set difference for instrument diff computation | Computing added/removed instruments per venue |
| std::collections::HashMap | stdlib | Per-venue instrument tracking | Grouping instruments by Venue enum |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio::sync::watch | tokio::sync::broadcast | broadcast requires receivers to keep up; watch overwrites with latest value which is exactly what we want (supervisors only need the most recent instrument list) |
| tokio::sync::Notify | Sequencing via async mutex | Notify is zero-allocation for single-waiter pattern; mutex would hold the lock across async boundaries unnecessarily |
| Per-venue watch channels | Single watch with HashMap<Venue, Vec<String>> | Per-venue channels are cleaner: each supervisor only watches its own channel, no filtering needed. But a single channel is simpler to wire. Recommend per-venue for type clarity. |

**Installation:**
No new dependencies. All primitives come from `tokio = { version = "1", features = ["full"] }` already in Cargo.toml.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── subscription/
│   ├── mod.rs           # pub mod manager; pub use manager::SubscriptionManager;
│   └── manager.rs       # SubscriptionManager struct, reconcile(), compute_desired_instruments()
```

### Pattern 1: Notify-Based Ordering (Registry-Before-Reconciliation)
**What:** Use `tokio::sync::Notify` to ensure the config reload subscriber finishes `registry.refresh()` before the SubscriptionManager reads registry state for reconciliation.
**When to use:** Whenever two async tasks must execute in a guaranteed order on the same trigger event.
**How it works:**

The existing config reload subscriber in `main.rs` (lines 286-318) watches `config_rx` and calls `registry.refresh()`. After refresh completes, it calls `notify.notify_one()`. The SubscriptionManager awaits `notify.notified()`, then reads the registry and reconciles.

```rust
// In main.rs config reload subscriber (modify existing):
result = config_rx.changed() => {
    match result {
        Ok(()) => {
            let new_config = config_rx.borrow_and_update().clone();
            let mut reg = config_registry.write().await;
            reg.refresh(&new_config.events);
            tracing::info!(mappings = reg.mapping_count(), "EventRegistry refreshed");
            drop(reg); // Release write lock before notifying
            registry_notify.notify_one(); // Wake SubscriptionManager
        }
        // ...
    }
}
```

```rust
// In SubscriptionManager::run():
loop {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => break,
        _ = registry_notify.notified() => {
            self.reconcile().await;
        }
    }
}
```

**Key insight:** The `Notify` creates a happens-before relationship: `registry.refresh()` -> `notify_one()` -> `notified()` returns -> `reconcile()` reads registry. The RwLock on EventRegistry provides the memory ordering guarantee for the data itself.

### Pattern 2: Set-Difference Reconciliation
**What:** Compare current subscribed instruments against desired instruments (from registry) to compute add/remove diffs per venue.
**When to use:** Every time reconciliation is triggered (config change).
**How it works:**

```rust
struct VenueDiff {
    venue: Venue,
    added: Vec<String>,
    removed: Vec<String>,
    total: usize,
}

fn compute_diff(current: &HashSet<String>, desired: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let added: Vec<String> = desired.difference(current).cloned().collect();
    let removed: Vec<String> = current.difference(desired).cloned().collect();
    (added, removed)
}
```

The SubscriptionManager keeps `HashMap<Venue, HashSet<String>>` as its "current" state. On each reconcile, it computes "desired" from the registry, diffs, logs, and pushes the new desired set via watch channels.

### Pattern 3: Watch Channel for Supervisor Instrument Updates
**What:** Each supervisor receives a `watch::Receiver` that carries the latest instrument list for that venue. The supervisor checks for updates at the top of its reconnect loop.
**When to use:** Pushing latest-value state from a manager to long-lived consumer tasks.

Currently, supervisors store instruments as a field set at construction:
```rust
// Current (DeribitSupervisor):
pub struct DeribitSupervisor {
    instruments: Vec<String>,  // Static, set at construction
    // ...
}
```

After Phase 22, supervisors will accept a watch::Receiver:
```rust
// After Phase 22 (DeribitSupervisor):
pub struct DeribitSupervisor {
    instruments_rx: watch::Receiver<Vec<String>>,  // Dynamic, updated by SubscriptionManager
    // ...
}
```

At each reconnection attempt, the supervisor reads the latest value:
```rust
// In supervisor.run(), at top of reconnect loop:
let instruments = self.instruments_rx.borrow().clone();
let client = DeribitClient::new(self.config.clone(), instruments, self.cancel.clone());
```

**Critical:** This pattern means Phase 22 must modify supervisor constructors to accept `watch::Receiver` instead of `Vec<String>`. The watch channel sender lives in SubscriptionManager. Phase 23 will wire this fully, but Phase 22 must define the channel types and create them.

### Pattern 4: SubscriptionManager Owns the Watch Senders
**What:** The SubscriptionManager creates the per-venue watch channels and holds the senders. Receivers are passed to supervisors during pipeline construction.
**When to use:** Central ownership pattern for channel lifecycle.

```rust
pub struct SubscriptionManager {
    registry: Arc<RwLock<EventRegistry>>,
    registry_notify: Arc<Notify>,
    cancel: CancellationToken,
    // Per-venue watch channel senders
    deribit_tx: watch::Sender<Vec<String>>,
    polymarket_tx: watch::Sender<Vec<PolymarketSubscription>>,
    kalshi_tx: watch::Sender<Vec<String>>,
    // Current state for diff computation
    current_subscriptions: HashMap<Venue, HashSet<String>>,
}
```

### Anti-Patterns to Avoid
- **Polling the registry on a timer:** Creates unnecessary load and non-deterministic delays. Use Notify for event-driven wakeup.
- **Sending diffs instead of full instrument lists:** Supervisors reconnect with a full channel list; they don't incrementally subscribe. Sending diffs would require supervisors to maintain their own state -- unnecessary complexity.
- **Holding the registry RwLock across the reconcile + watch send:** Acquire the read lock, extract what you need, drop the lock, then compute diffs and send. Holding the lock blocks the config reload subscriber and the pipeline's forward_snapshots.
- **Modifying supervisor reconnect logic in Phase 22:** Phase 22 focuses on the SubscriptionManager and diff computation. Phase 23 wires it to supervisors. Phase 22 should create the watch channels and prove the diff logic works, but supervisor modifications happen in Phase 23.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Set difference computation | Custom iteration with removal tracking | `HashSet::difference()` | Standard library, O(n), handles all edge cases |
| Latest-value push channel | Custom Arc<Mutex<T>> + Condvar | `tokio::sync::watch` | Designed for exactly this pattern; handles multiple receivers, no allocation per send |
| Ordered wakeup signal | Custom channel with dummy messages | `tokio::sync::Notify` | Zero-allocation for single-waiter; designed for this coordination pattern |
| Per-venue instrument extraction | Separate functions per venue | Single generic extraction loop over `active_approved()` with match on venue mapping fields | DRY; the registry already provides the iteration |

**Key insight:** tokio's sync primitives are purpose-built for these coordination patterns. The project already uses `watch` (ConfigReloader), `mpsc` (pipeline), `RwLock` (EventRegistry), and `CancellationToken` (shutdown). Adding `Notify` and another `watch` channel follows the existing pattern language perfectly.

## Common Pitfalls

### Pitfall 1: Registry Read Lock Held During Watch Send
**What goes wrong:** If `reconcile()` holds the `RwLock<EventRegistry>` read lock while calling `watch_tx.send()`, and a config reload arrives simultaneously, the config reload subscriber blocks on acquiring the write lock, creating a priority inversion.
**Why it happens:** Natural to write `let reg = registry.read().await; /* compute diffs */ watch_tx.send(new_list);` without dropping `reg` first.
**How to avoid:** Extract instrument lists into local variables, drop the read lock, then compute diffs and send.
**Warning signs:** Config reload latency spikes; "EventRegistry refreshed" log appears with multi-second gaps.

### Pitfall 2: Watch Channel Initial Value Mismatch
**What goes wrong:** Watch channels are created with an initial value. If the SubscriptionManager creates the channel with an empty `Vec<String>`, but supervisors start before the first reconciliation runs, supervisors connect with zero instruments.
**Why it happens:** SubscriptionManager.run() hasn't executed its first reconcile yet.
**How to avoid:** Seed the watch channels with the initial instrument lists derived from the startup config (same lists currently passed to supervisors). The SubscriptionManager constructor should compute initial instruments from the registry and use those as the initial watch channel values.
**Warning signs:** First connection attempt subscribes to zero channels; immediate reconnect.

### Pitfall 3: Notify Wake Before Listener Ready
**What goes wrong:** If `notify_one()` fires before the SubscriptionManager has called `notified()`, the notification is lost (Notify permits are consumed).
**Why it happens:** Race between task startup ordering. Config reload subscriber starts, a config change arrives, `notify_one()` fires, but SubscriptionManager hasn't started its select loop yet.
**How to avoid:** `tokio::sync::Notify` permits are stored and consumed by the next `notified()` call. A single `notify_one()` before `notified()` IS captured -- Notify stores one permit. Multiple `notify_one()` calls before `notified()` coalesce to one wakeup, which is correct behavior (we only need "something changed", not "changed N times").
**Warning signs:** None for single-permit case. If multiple rapid config changes are expected, the coalescing behavior is actually desirable (reconcile once with latest state, not once per change).

### Pitfall 4: Polymarket Subscription Format Difference
**What goes wrong:** Deribit and Kalshi use simple string instrument IDs, but Polymarket subscriptions require both `condition_id` and `token_id`. If the watch channel only carries `Vec<String>` (token_ids), the supervisor loses the condition_id context.
**Why it happens:** Venue subscription formats differ.
**How to avoid:** Use venue-specific types for the watch channels. Deribit: `watch::Sender<Vec<String>>` (instrument names). Polymarket: `watch::Sender<Vec<PolymarketSubscription>>` where `PolymarketSubscription { condition_id: String, token_id: String }` (or reuse the existing `PolymarketAsset` config type). Kalshi: `watch::Sender<Vec<String>>` (market tickers).
**Warning signs:** Polymarket supervisor can't build subscription message because it only has token_ids.

### Pitfall 5: Windows Atomic Rename Produces Multiple File Events
**What goes wrong:** On Windows, atomic rename (write tmp + rename) generates DELETE + RENAME events that may trigger two config reloads within the debounce window.
**Why it happens:** Windows file system semantics differ from Linux. The existing `notify_debouncer_mini` with 500ms debounce handles this, but it's worth noting.
**How to avoid:** The existing debouncer handles this. The SubscriptionManager's Notify-based pattern naturally coalesces multiple rapid reconciliation triggers. No additional mitigation needed.
**Warning signs:** Two "EventRegistry refreshed" log lines in quick succession. Harmless but noisy.

### Pitfall 6: Empty Diff Still Sends Watch Update
**What goes wrong:** If events.toml changes in a way that doesn't affect active_approved() instruments (e.g., discovery adds a new unapproved candidate), the SubscriptionManager still sends the same instrument list via watch channels, causing supervisors to log "instrument list updated" even though nothing changed.
**Why it happens:** Reconcile runs on every registry refresh, not just on subscription-affecting changes.
**How to avoid:** Compare desired instruments with current state before sending. Only send if there's an actual diff. Log "no subscription changes" at debug level for uneventful reconciliations.
**Warning signs:** Excessive "instrument list updated" logs during discovery poll cycles.

## Code Examples

Verified patterns from the existing codebase and tokio documentation:

### Example 1: Compute Desired Instruments from Registry
```rust
use std::collections::{HashMap, HashSet};
use crate::config::PolymarketAsset;
use crate::events::registry::EventRegistry;
use crate::types::Venue;

/// Instrument subscription info per venue.
/// Polymarket needs both condition_id and token_id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolymarketSubscription {
    pub condition_id: String,
    pub token_id: String,
}

/// Extract per-venue instrument sets from active_approved() mappings.
fn compute_desired_instruments(
    registry: &EventRegistry,
) -> (HashSet<String>, HashSet<PolymarketSubscription>, HashSet<String>) {
    let mut deribit = HashSet::new();
    let mut polymarket = HashSet::new();
    let mut kalshi = HashSet::new();

    for mapping in registry.active_approved() {
        if let Some(ref d) = mapping.venues.deribit {
            deribit.insert(d.instrument.clone());
        }
        if let Some(ref p) = mapping.venues.polymarket {
            polymarket.insert(PolymarketSubscription {
                condition_id: p.condition_id.clone(),
                token_id: p.token_id.clone(),
            });
        }
        if let Some(ref k) = mapping.venues.kalshi {
            kalshi.insert(k.ticker.clone());
        }
    }

    (deribit, polymarket, kalshi)
}
```

### Example 2: Set Difference Computation
```rust
fn compute_venue_diff<T: Clone + Eq + std::hash::Hash + std::fmt::Debug>(
    current: &HashSet<T>,
    desired: &HashSet<T>,
) -> (Vec<T>, Vec<T>) {
    let added: Vec<T> = desired.difference(current).cloned().collect();
    let removed: Vec<T> = current.difference(desired).cloned().collect();
    (added, removed)
}
```

### Example 3: Structured Diff Logging (OBS-03)
```rust
fn log_venue_diff(venue: Venue, added: &[String], removed: &[String], total: usize) {
    if added.is_empty() && removed.is_empty() {
        tracing::debug!(
            venue = %venue,
            total = total,
            "subscription reconciliation: no changes"
        );
    } else {
        tracing::info!(
            venue = %venue,
            added_count = added.len(),
            removed_count = removed.len(),
            total = total,
            added = ?added,
            removed = ?removed,
            "subscription reconciliation: diff computed"
        );
    }
}
```

### Example 4: Watch Channel Creation with Initial Values
```rust
use tokio::sync::watch;

// Compute initial instrument lists from startup registry state
let registry_guard = event_registry.read().await;
let (initial_deribit, initial_polymarket, initial_kalshi) =
    compute_desired_instruments(&registry_guard);
drop(registry_guard);

// Create watch channels seeded with initial values
let (deribit_tx, deribit_rx) = watch::channel(
    initial_deribit.into_iter().collect::<Vec<_>>()
);
let (polymarket_tx, polymarket_rx) = watch::channel(
    initial_polymarket.into_iter().collect::<Vec<_>>()
);
let (kalshi_tx, kalshi_rx) = watch::channel(
    initial_kalshi.into_iter().collect::<Vec<_>>()
);
```

### Example 5: SubscriptionManager Run Loop
```rust
pub async fn run(mut self) {
    loop {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => {
                tracing::info!("SubscriptionManager shutting down");
                break;
            }
            _ = self.registry_notify.notified() => {
                self.reconcile().await;
            }
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Static instrument lists at supervisor startup | Dynamic watch channels for instrument list updates | Phase 22 (new) | Enables no-restart subscription changes |
| Config reload only refreshes EventRegistry | Config reload triggers EventRegistry refresh AND subscription reconciliation | Phase 22 (new) | Bridges config changes to feed subscriptions |
| Supervisors ignore config changes | Supervisors read latest instrument list on reconnect | Phase 22+23 (new) | Reconnect-based subscription management |

**Current state of the codebase relevant to Phase 22:**
- ConfigReloader: fully operational, publishes AppConfig via watch channel, 500ms debounce
- Config reload subscriber in main.rs (lines 286-318): refreshes EventRegistry on config change
- EventRegistry: has `refresh()` and `active_approved()` methods
- Supervisors: accept static instrument lists at construction; never update
- Pipeline: creates supervisors with config-derived instrument lists during startup
- All three venues use reconnect-based subscription (connect, subscribe all, read until drop)

## Open Questions

1. **Where should SubscriptionManager live in the module tree?**
   - Recommendation: New `src/subscription/` module (consistent with `src/settlement/`, `src/persistence/`, etc.)
   - Alternative: Inside `src/events/` since it's closely tied to EventRegistry
   - The new module is preferred because subscription management is a distinct concern from event registry management

2. **Should Phase 22 modify supervisor constructors or defer to Phase 23?**
   - Recommendation: Phase 22 creates the watch channels and SubscriptionManager, but does NOT modify supervisors yet. Phase 23 modifies supervisors to accept watch::Receiver and read from it.
   - Rationale: Phase 22 can be tested in isolation by verifying diff computation and watch channel sends without touching the live feed pipeline.
   - Trade-off: This means Phase 22 creates watch channels whose receivers are not yet consumed. The senders are held by SubscriptionManager; receivers can be passed through PipelineHandles for Phase 23 to wire.

3. **How should the SubscriptionManager handle Polymarket's dual-id subscription format?**
   - Recommendation: Use the existing `PolymarketAsset` type (from config::venues) or a similar struct for the Polymarket watch channel value type. This preserves both condition_id and token_id.
   - The PolymarketAsset type already exists and is `Clone + Debug`, suitable for watch channels. May need to derive `Eq + Hash` for set operations.

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/config/reload.rs` -- ConfigReloader with watch channel pattern
- Codebase analysis: `src/events/registry.rs` -- EventRegistry with active_approved() and refresh()
- Codebase analysis: `src/feed/deribit/supervisor.rs`, `src/feed/polymarket/supervisor.rs`, `src/feed/kalshi/supervisor.rs` -- current supervisor patterns
- Codebase analysis: `src/feed/pipeline.rs` -- pipeline construction and supervisor wiring
- Codebase analysis: `src/main.rs` -- config reload subscriber (lines 286-318), pipeline setup
- Codebase analysis: `src/config/events.rs` -- EventMapping, EventVenues, venue mapping types
- Codebase analysis: `Cargo.toml` -- tokio features = ["full"] confirms watch + Notify availability
- Project STATE.md decisions: watch for instrument lists, Notify for ordering, zero new dependencies

### Secondary (MEDIUM confidence)
- tokio::sync::watch semantics: latest-value overwrite, single producer multiple consumer
- tokio::sync::Notify semantics: permit storage for single-waiter, coalescing for multiple notify_one() calls
- HashSet::difference(): O(n) set difference from std

### Tertiary (LOW confidence)
- None. All findings are from direct codebase inspection and known tokio API behavior.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All primitives already in dependency tree; project decisions documented in STATE.md
- Architecture: HIGH - Builds directly on existing patterns (watch channels from ConfigReloader, RwLock on EventRegistry, supervisor reconnect loops)
- Pitfalls: HIGH - Identified from direct codebase analysis of existing race conditions and platform-specific behavior (Windows atomic rename already documented in STATE.md blockers)

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain; tokio sync primitives are mature)
