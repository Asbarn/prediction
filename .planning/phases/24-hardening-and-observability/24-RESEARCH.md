# Phase 24: Hardening and Observability - Research

**Researched:** 2026-02-27
**Domain:** Subscription lifecycle observability, stale state cleanup, dry-run reconciliation
**Confidence:** HIGH

## Summary

Phase 24 closes the remaining gaps in the v1.3 subscription management milestone. Four requirements span three concerns: (1) cleaning up stale internal state after instruments are unsubscribed so no phantom signals leak from stale data paired with live data, (2) exposing subscription lifecycle metrics via Prometheus gauges and counters, and (3) adding a dry-run mode to the reconciliation engine for safe operational testing.

The project already uses the `metrics` 0.24 crate with `metrics-exporter-prometheus` 0.18 and has extensive precedent for `metrics::gauge!()`, `metrics::counter!()`, and `metrics::histogram!()` macros with label pairs (venue labels, event labels, etc.). No new crate dependencies are needed. The `SubscriptionManager` already computes per-venue diffs (added/removed sets) -- metrics and dry-run logic plug directly into the existing `reconcile()` method. Stale state cleanup requires introducing a notification mechanism (e.g., a broadcast of removed instrument IDs) that downstream engines can consume to evict HashMap entries.

**Primary recommendation:** Emit metrics and implement dry-run mode directly in `SubscriptionManager::reconcile()`. For stale state cleanup, add a `cleanup_instruments` method to each stateful engine (`SpreadEngine`, `CrossAssetEngine`, `PricingEngine`) and drive cleanup from the `SubscriptionManager` via a new channel that carries removed instrument/event IDs.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SUB-05 | Stale internal state (order books, snapshots, rolling stats) is cleaned up after instruments are unsubscribed | Architecture Pattern 1 (cleanup channel) covers SpreadEngine.latest, SpreadEngine.stats, CrossAssetEngine.latest_prob/latest_pred/stats, DeribitProcessor.books/tickers, KalshiProcessor.books/last_exchange_ts, PricingEngine.smiles/iv_cache/smile_points |
| OBS-01 | Prometheus gauges show per-venue active subscription count | Architecture Pattern 2 (gauge per venue, set in reconcile after updating current state) |
| OBS-02 | Prometheus counters track subscription activations and removals per venue | Architecture Pattern 2 (counter per venue, incremented by added.len()/removed.len() in reconcile) |
| OPS-01 | Dry-run reconciliation mode (config flag) logs what actions would be taken without sending subscribe/unsubscribe commands | Architecture Pattern 3 (dry_run flag on SubscriptionManager, skip watch send and state update when true) |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `metrics` | 0.24 | Metrics facade for gauge/counter/histogram macros | Already in Cargo.toml; zero-cost no-op when no recorder |
| `metrics-exporter-prometheus` | 0.18 | Prometheus HTTP scrape endpoint | Already in Cargo.toml; HTTP listener installed in main.rs |
| `tokio::sync::mpsc` | tokio 1.x | Channel for cleanup event notifications | Already pervasive in the codebase for inter-task communication |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio::sync::watch` | tokio 1.x | Existing subscription push channels | Already used by SubscriptionManager; no change needed |
| `tracing` | 0.1 | Structured logging for dry-run output | Already used everywhere; dry-run log messages use tracing::info! |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| mpsc cleanup channel | watch channel with Vec of removed IDs | mpsc is better because cleanup is event-driven (fire-and-forget), not latest-value semantics |
| Per-engine cleanup method | Periodic sweep that checks registry | Periodic sweep introduces latency between unsubscribe and cleanup; event-driven is immediate |
| Shared cleanup set behind RwLock | mpsc channel | RwLock adds contention; mpsc is lock-free and matches existing patterns |

**No new dependencies needed.** This phase uses only existing crates.

## Architecture Patterns

### Recommended Scope of Changes

```
src/
├── subscription/
│   └── manager.rs       # Add metrics, dry-run, cleanup channel
├── spread/
│   └── engine.rs         # Add cleanup_instruments() method
├── signal/
│   └── engine.rs         # Add cleanup_instruments() method
├── pricing/
│   └── engine.rs         # Add cleanup_instruments() method
├── feed/
│   ├── deribit/
│   │   └── normalize.rs  # Add cleanup method (books, tickers)
│   └── kalshi/
│       └── normalize.rs  # Add cleanup method (books, last_exchange_ts)
├── config/
│   └── system.rs         # Add SubscriptionConfig with dry_run field
└── main.rs               # Wire cleanup channels, pass dry_run config
```

### Pattern 1: Cleanup Channel for Stale State Removal (SUB-05)

**What:** SubscriptionManager sends removed instrument/event identifiers through an mpsc channel after reconciliation. Downstream engines receive these IDs and evict corresponding HashMap entries.

**When to use:** Whenever an instrument is removed from the desired set during reconciliation.

**Rationale:** The stateful components that accumulate per-instrument data are:

1. **SpreadEngine** (`src/spread/engine.rs`):
   - `latest: HashMap<(String, Venue), MarketSnapshot>` -- keyed by (event_id, venue)
   - `stats: HashMap<String, RollingStats>` -- keyed by event_id
   - Cleanup: remove entries matching event IDs whose instruments were unsubscribed

2. **CrossAssetEngine** (`src/signal/engine.rs`):
   - `latest_prob: HashMap<String, ImpliedProbability>` -- keyed by event_id
   - `latest_pred: HashMap<(String, Venue), MarketSnapshot>` -- keyed by (event_id, venue)
   - `stats: HashMap<String, RollingStats>` -- keyed by event_id
   - Cleanup: same pattern as SpreadEngine

3. **DeribitProcessor** (`src/feed/deribit/normalize.rs`):
   - `books: HashMap<InstrumentId, InstrumentBook>` -- keyed by instrument
   - `tickers: HashMap<InstrumentId, TickerState>` -- keyed by instrument
   - Cleanup: remove entries for unsubscribed Deribit instruments

4. **KalshiProcessor** (`src/feed/kalshi/normalize.rs`):
   - `books: HashMap<String, KalshiBook>` -- keyed by market_ticker
   - `last_exchange_ts: HashMap<String, String>` -- keyed by market_ticker
   - Cleanup: remove entries for unsubscribed Kalshi tickers

5. **PricingEngine** (`src/pricing/engine.rs`):
   - `smiles: HashMap<NaiveDate, VolSmile>` -- keyed by expiry date
   - `iv_cache: HashMap<InstrumentId, IvCacheEntry>` -- keyed by instrument
   - `smile_points: HashMap<NaiveDate, HashMap<u64, SmilePoint>>` -- keyed by expiry
   - Cleanup: remove iv_cache entries for unsubscribed Deribit instruments; smile/smile_points cleanup is complex (multiple instruments share an expiry) but iv_cache is the primary concern

**Design choice -- What IDs to send:**

The key challenge is that processors are keyed by instrument IDs, while SpreadEngine/CrossAssetEngine are keyed by event IDs. The SubscriptionManager already computes per-venue removed instrument sets. Two approaches:

- **Option A (simpler):** Send removed instrument IDs per venue. Processors use directly. Engines look up event IDs from EventRegistry before cleanup. Problem: removed instruments may no longer be in the registry after refresh.
- **Option B (recommended):** SubscriptionManager looks up event IDs from the registry BEFORE computing diffs (it already reads the registry during reconcile). Send a `CleanupEvent` struct containing both removed instrument IDs (per-venue) and the corresponding event IDs. This way all receivers get what they need without additional registry lookups.

**Implementation approach:**

```rust
/// Instruments and event IDs to clean up after unsubscribe.
#[derive(Debug, Clone)]
pub struct CleanupEvent {
    pub deribit_instruments: Vec<String>,
    pub kalshi_tickers: Vec<String>,
    pub polymarket_token_ids: Vec<String>,
    pub event_ids: Vec<String>,
}
```

Use `tokio::sync::broadcast` or multiple `mpsc::Sender` clones to fan out to all consumers. Since the number of receivers is fixed and known at compile time, multiple mpsc channels (one per consumer) is cleaner than broadcast.

**Critical insight:** The SubscriptionManager must compute the event IDs for removed instruments BEFORE updating `current_*` sets and BEFORE the registry is refreshed away. The current flow already reads the registry and computes diffs before updating state -- the event ID lookup fits naturally into this window.

**Event ID resolution approach:**

For `removed_deribit` instruments, SubscriptionManager can call `registry.lookup_by_instrument(Venue::Deribit, &inst)` to get the event_id. But there is a subtlety: if an instrument was removed because its mapping was archived, the registry may have already been refreshed (the Notify fires AFTER registry refresh). This means the mapping may already be gone from the registry.

**Solution:** Maintain a reverse map `HashMap<(Venue, String), String>` (instrument -> event_id) inside SubscriptionManager that is populated when instruments are first seen. When instruments are removed, look up from this map instead of the registry. Update the map on each reconciliation.

Alternatively (simpler): engines that need event IDs for cleanup (`SpreadEngine`, `CrossAssetEngine`) can use `.retain()` to keep only entries whose event_id is still present in the registry. This is a periodic/on-demand check rather than event-driven, but avoids the reverse-map complexity.

**Recommended approach:** Use `.retain()` in engines keyed by event_id. The cleanup trigger is still event-driven (receives the cleanup notification), but the engine queries the registry to determine which event_ids to keep vs remove. For processors keyed by instrument_id, the per-venue instrument lists from the cleanup event work directly.

### Pattern 2: Subscription Metrics (OBS-01, OBS-02)

**What:** Emit Prometheus gauge and counter metrics from `SubscriptionManager::reconcile()` after computing diffs and updating state.

**When to use:** Every reconciliation pass.

**Metrics to emit:**

```rust
// OBS-01: Active subscription count per venue (gauge)
metrics::gauge!("subscription_active", "venue" => "deribit")
    .set(self.current_deribit.len() as f64);
metrics::gauge!("subscription_active", "venue" => "polymarket")
    .set(self.current_polymarket.len() as f64);
metrics::gauge!("subscription_active", "venue" => "kalshi")
    .set(self.current_kalshi.len() as f64);

// OBS-02: Cumulative activations and removals per venue (counters)
if !added_d.is_empty() {
    metrics::counter!("subscription_activations_total", "venue" => "deribit")
        .increment(added_d.len() as u64);
}
if !removed_d.is_empty() {
    metrics::counter!("subscription_removals_total", "venue" => "deribit")
        .increment(removed_d.len() as u64);
}
// ... same for polymarket and kalshi
```

**Placement:** After `self.current_deribit = desired_d;` (line 245 in manager.rs) -- metrics reflect the new state. The gauge is set to the size of the current set; counters are incremented by the diff sizes.

**Metric naming:** Matches the success criteria exactly: `subscription_active{venue="deribit"}`, `subscription_activations_total{venue="deribit"}`, `subscription_removals_total{venue="deribit"}`.

### Pattern 3: Dry-Run Reconciliation (OPS-01)

**What:** When `dry_run = true`, `SubscriptionManager::reconcile()` logs what subscribe/unsubscribe actions would be taken but does NOT send updated instrument lists via watch channels and does NOT update internal `current_*` state.

**When to use:** Operational testing -- verify reconciliation logic without triggering reconnections.

**Implementation:**

```rust
// In reconcile(), after computing diffs and logging:
if self.dry_run {
    tracing::info!(
        deribit_add = added_d.len(),
        deribit_remove = removed_d.len(),
        polymarket_add = added_p.len(),
        polymarket_remove = removed_p.len(),
        kalshi_add = added_k.len(),
        kalshi_remove = removed_k.len(),
        "DRY RUN: reconciliation would apply these changes"
    );
    // Do NOT send watch channel updates
    // Do NOT update current_* sets
    // Do NOT send cleanup events
    // Do NOT update metrics (counters/gauges reflect actual state, not hypothetical)
    return;
}
```

**Config location:** Add to `SystemConfig` as a new `SubscriptionConfig` section:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SubscriptionConfig {
    pub dry_run: bool,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self { dry_run: false }
    }
}
```

In config.toml: `[subscription]` section with `dry_run = true/false`.

**Pass to SubscriptionManager:** Add `dry_run: bool` field to `SubscriptionManager::new()`.

### Anti-Patterns to Avoid

- **Cleanup via reconnect-only:** Don't rely on supervisor reconnect to clear processor state. Reconnect creates a fresh client, but the processor is created once and runs for the lifetime of the application. The processor's HashMaps persist across reconnects.
- **Polling-based cleanup:** Don't use a periodic timer to scan for stale entries. This introduces a window where stale data can produce phantom signals. Event-driven cleanup from reconciliation is immediate.
- **Metrics in dry-run mode:** Don't emit subscription_active/activations/removals metrics when dry_run is true. These metrics should reflect actual state, not hypothetical state.
- **Blocking cleanup in reconcile():** Don't perform cleanup synchronously in reconcile(). Send cleanup events asynchronously via mpsc. Engines process cleanup in their own event loops.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Prometheus metrics emission | Custom metrics collection/export | `metrics::gauge!()` / `metrics::counter!()` macros | Already installed; zero-cost; matches 40+ existing call sites |
| Metric label management | Custom label struct/registry | Inline string labels in macros | The `metrics` crate handles deduplication and storage |
| Channel fan-out for cleanup | Custom broadcast mechanism | Multiple mpsc::Sender clones (one per receiver) | Known receiver count; simpler than broadcast; matches project patterns |

**Key insight:** Every piece of infrastructure needed for this phase already exists in the project. No new crates, no new patterns -- just extending existing components.

## Common Pitfalls

### Pitfall 1: Phantom Signals from Partially-Cleaned State

**What goes wrong:** Cleaning up one venue's state but not the other leaves stale data that can pair with live data to produce phantom spread signals. For example, if Polymarket token_id is removed but the SpreadEngine still has a cached Kalshi snapshot for the same event_id, a new Polymarket snapshot for a DIFFERENT event could match the stale Kalshi data (if event_id lookup changes).

**Why it happens:** SpreadEngine pairs snapshots by event_id. If stale snapshots remain, they can pair with incoming live data.

**How to avoid:** Clean up ALL entries for an event_id atomically -- remove both the `(event_id, Venue::Polymarket)` and `(event_id, Venue::Kalshi)` entries from `SpreadEngine.latest` in the same cleanup pass. Clean up `stats` for the same event_id simultaneously.

**Warning signs:** Spread computations appearing for event_ids that have no active instrument subscriptions.

### Pitfall 2: Race Between Registry Refresh and Event ID Lookup

**What goes wrong:** SubscriptionManager receives Notify, reads registry (which has already been refreshed with removed mappings), tries to look up event_ids for removed instruments -- but they are already gone from the registry.

**Why it happens:** The Notify fires AFTER `reg.refresh()` drops the write lock. By the time SubscriptionManager reads the registry, removed mappings are already absent.

**How to avoid:** Use the `.retain()` approach in engines: cleanup removes entries whose event_id is NOT in the current registry active set, rather than trying to resolve removed instrument -> event_id. Alternatively, maintain a reverse map in SubscriptionManager populated on first reconciliation.

**Warning signs:** Cleanup events with empty event_ids; state that should have been cleaned up persisting indefinitely.

### Pitfall 3: Dry-Run Mode Accumulating State Drift

**What goes wrong:** When dry_run = true, `current_*` sets are never updated. Over multiple reconciliations, the diff between current and desired grows, making logs increasingly confusing (every reconciliation shows cumulative changes, not just new ones).

**Why it happens:** The diff is computed against `current_*` which stays frozen at the initial state.

**How to avoid:** In dry-run mode, still update `current_*` sets (so diffs are meaningful) but skip the watch channel sends and cleanup events. The dry-run semantics are: "don't send commands to venues" not "don't track state changes."

**Revised approach:** Update internal state even in dry-run. Only skip watch channel sends (which trigger supervisor reconnects) and cleanup channel sends.

### Pitfall 4: Processor Cleanup After Reconnect Creates Fresh State

**What goes wrong:** After cleanup removes entries from DeribitProcessor, the supervisor reconnects and the processor starts receiving data for the same instruments again (if they are still subscribed on the venue side during reconnect lag). This re-populates the just-cleaned entries.

**Why it happens:** Reconnect-based subscription means there is a window where the old connection is still delivering data for instruments that are being removed.

**How to avoid:** This is acceptable behavior. The cleanup channel should be processed AFTER the reconnect completes with the new instrument list. The processor will only accumulate state for instruments that arrive in new messages -- which will be the updated set. Stale entries from the pre-reconnect state won't receive new data and will be benign (staleness gate will reject them). The cleanup is a memory optimization, not a correctness gate.

### Pitfall 5: PricingEngine Smile Cleanup Complexity

**What goes wrong:** Removing a single Deribit instrument from `iv_cache` is straightforward, but `smiles` and `smile_points` are keyed by expiry date, shared across multiple instruments with the same expiry.

**Why it happens:** Deribit option instruments sharing an expiry all contribute to the same vol smile.

**How to avoid:** Only clean up `iv_cache` entries for removed instruments. Don't remove smiles/smile_points unless ALL instruments for that expiry are removed. This is a minor leak that is acceptable -- vol smiles are small and expire naturally when their date passes.

## Code Examples

Verified patterns from the existing codebase:

### Gauge with Venue Label (existing pattern from feed/health.rs)

```rust
// Source: src/feed/health.rs line 49
metrics::gauge!("feed_available", "venue" => self.venue.to_string()).set(1.0);
```

### Counter with Venue Label (existing pattern from feed normalizers)

```rust
// Source: src/feed/deribit/normalize.rs line 504
metrics::counter!("feed_messages_total", "venue" => "deribit").increment(1);
```

### Retain Pattern for HashMap Cleanup

```rust
// Standard Rust HashMap::retain pattern
self.latest.retain(|&(ref event_id, _), _| {
    active_event_ids.contains(event_id)
});
self.stats.retain(|event_id, _| {
    active_event_ids.contains(event_id)
});
```

### Watch Channel Send (existing pattern from manager.rs)

```rust
// Source: src/subscription/manager.rs line 223
self.deribit_tx.send_replace(instruments);
```

### Config Section with serde(default) (existing pattern)

```rust
// Source: src/config/system.rs line 62
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PersistenceConfig {
    pub enabled: bool,
    pub checkpoint_dir: String,
    pub checkpoint_interval_secs: u64,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `register_counter!` + `increment_counter!` | `counter!("name").increment(1)` | metrics 0.22 (Dec 2023) | Project already uses new-style macros throughout |
| N/A | N/A | N/A | No state-of-the-art changes affect this phase |

**Deprecated/outdated:**
- None relevant. The `metrics` 0.24 API used in this project is current.

## Open Questions

1. **Should cleanup be best-effort or blocking?**
   - What we know: mpsc channels can be bounded; if a receiver is slow, the send would block or fail (try_send).
   - What's unclear: Whether cleanup backpressure should slow down reconciliation.
   - Recommendation: Use `try_send` (best-effort, non-blocking) -- consistent with all other channel sends in the project. Log a warning if the channel is full. Cleanup is a memory optimization, not a correctness requirement (staleness gates protect against phantom signals).

2. **Should DeribitProcessor and KalshiProcessor cleanup be implemented in this phase?**
   - What we know: These processors are created fresh on each supervisor run() call -- but they consume `self` (owned), so their HashMaps persist for the lifetime of the run() invocation, which spans multiple reconnects within the same connection supervisor.
   - What's unclear: On closer inspection, processors are created in the pipeline function and their `run()` consumes `self`. If the supervisor reconnects, it creates a new client but the PROCESSOR is not recreated -- it was already spawned. So processor state DOES accumulate.
   - Recommendation: Include processor cleanup. The processors' `run()` methods use `tokio::select!` -- add a cleanup channel branch. This ensures books/tickers are evicted when instruments are unsubscribed.

3. **Should metrics include an initial emission at startup?**
   - What we know: The first reconciliation in `SubscriptionManager::run()` fires when `registry_notify.notified()` is received. But `create_channels()` seeds the initial state -- the first reconcile will see those as "already present" (current_* starts empty, first reconcile detects all as "added").
   - What's unclear: Whether the initial "all added" reconciliation should count toward activation counters.
   - Recommendation: Yes, the initial reconciliation should emit metrics like any other. The first reconcile correctly detects all initial instruments as "added" and will increment activations_total. This is consistent behavior.

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/subscription/manager.rs`, `src/spread/engine.rs`, `src/signal/engine.rs`, `src/pricing/engine.rs`, `src/feed/deribit/normalize.rs`, `src/feed/kalshi/normalize.rs`, `src/feed/polymarket/normalize.rs`
- Codebase analysis: `src/config/system.rs`, `src/metrics_export/mod.rs`, `src/main.rs`
- Context7 `/metrics-rs/metrics` -- gauge!, counter!, describe_gauge!, describe_counter! macro usage with labels
- `.planning/REQUIREMENTS.md` -- SUB-05, OBS-01, OBS-02, OPS-01 requirement text
- `.planning/STATE.md` -- "Stale state after unsubscribe is the primary risk" documented as known blocker

### Secondary (MEDIUM confidence)
- Phase 22/23 SUMMARY.md files -- patterns established (watch channels, Notify ordering, pipeline threading)

### Tertiary (LOW confidence)
- None. All findings derived from direct codebase inspection and verified documentation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in use; no new dependencies
- Architecture: HIGH - Patterns derived directly from existing codebase; cleanup channel follows established mpsc patterns
- Pitfalls: HIGH - Identified through direct code analysis of HashMap accumulation patterns and race conditions in the existing reconciliation flow

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain, no external dependency changes expected)
