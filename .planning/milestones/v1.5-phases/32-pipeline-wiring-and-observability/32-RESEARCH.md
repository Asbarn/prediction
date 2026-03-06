# Phase 32: Pipeline Wiring and Observability - Research

**Researched:** 2026-03-05
**Domain:** Tokio channel wiring, subscription reconciliation, Prometheus metrics, multi-venue pipeline integration
**Confidence:** HIGH

## Summary

Phase 32 wires the standalone Derive feed (built in Phase 31) into the existing multi-venue pipeline so that Derive `MarketSnapshot` events flow through `run_live_multi_venue()` and reach SpreadEngine, SignalEngine (CrossAssetEngine), PricingEngine, and PaperTradeTracker automatically. This requires three coordinated changes: (1) extending `SubscriptionManager` with a fourth venue (Derive) following the exact Deribit/Polymarket/Kalshi pattern of HashSet diff, watch channel, and Notify ordering; (2) adding a Derive pipeline block inside `run_live_multi_venue()` that spawns DeriveSupervisor, DeriveProcessor, RecordingService, and a `forward_snapshots` task; and (3) adding Prometheus metrics for Derive feed state (connection status, message rate, subscription count, reconnection events).

The implementation is highly mechanical. Every pattern already exists for 3 venues -- Phase 32 adds a 4th by replicating the exact same wiring. The `SubscriptionManager` already has `derive_instruments` on `CleanupEvent` (added in Phase 31, currently hardcoded to `Vec::new()`). The `SubscriptionSenders`/`SubscriptionReceivers` structs need a `derive` field. The `compute_desired_instruments` function needs to extract Derive instruments from the registry. The `run_live_multi_venue` function needs a `// --- Derive pipeline ---` block identical in structure to Deribit's. No downstream engine changes are needed -- all engines consume `MarketSnapshot` regardless of `venue` field.

All changes are within existing files. No new files are created. No new dependencies are needed.

**Primary recommendation:** Follow the existing 3-venue pattern exactly -- add `derive` fields to `SubscriptionSenders`/`SubscriptionReceivers`, add Derive HashSet/watch/diff to `SubscriptionManager`, add Derive pipeline block to `run_live_multi_venue()`, and add Derive-specific Prometheus metrics. Zero architectural invention required.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PIPE-03 | SubscriptionManager extended with Derive venue support (HashSet diff, watch channel, Notify ordering) | `SubscriptionManager` in `src/subscription/manager.rs` already manages 3 venues with identical pattern: `current_X: HashSet`, `X_tx: watch::Sender`, `compute_desired_instruments()` extraction, `compute_diff()`, sorted `send_replace()`. Derive follows identically -- `DeriveMapping.instrument` is a `String` like Deribit, so the exact same `HashSet<String>` + `watch::Sender<Vec<String>>` pattern applies. `CleanupEvent.derive_instruments` already exists (Phase 31). |
| PIPE-04 | Derive wired into `run_live_multi_venue()` pipeline -- SpreadEngine, SignalEngine, PaperTradeTracker receive Derive snapshots automatically | `run_live_multi_venue()` in `src/feed/pipeline.rs` has 3 pipeline blocks (Deribit, Polymarket, Kalshi). Adding a 4th block for Derive follows Deribit's exact pattern: `VenueHealth::new(Venue::Derive)`, child `CancellationToken`, `RecordingService::start()`, `DeriveSupervisor::new()`, `DeriveProcessor::new()`, `forward_snapshots()`. Derive requires no auth (unlike Kalshi), so no credential guard needed. The fan-in `snapshot_tx.clone()` pattern ensures Derive snapshots reach the same shared channel consumed by downstream engines. |
| PIPE-05 | Prometheus metrics for Derive feed (connection state, message rate, subscription count) | Three metric types already exist and emit with `venue` label: (1) `feed_available` gauge in `VenueHealth::mark_available/mark_unavailable` -- automatic once `VenueHealth::new(Venue::Derive)` is created; (2) `feed_latency_ms` histogram, `feed_last_latency_ms` gauge, and `feed_messages_total` counter -- already emitted in `DeriveProcessor::build_snapshot()` (Phase 31); (3) `subscription_active` gauge and `subscription_activations_total`/`subscription_removals_total` counters -- will emit automatically once Derive is added to `SubscriptionManager::reconcile()`. No new metric registration needed. |
</phase_requirements>

## Standard Stack

### Core

No new Cargo dependencies required. All libraries are already in the workspace.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x (existing) | Async channels (mpsc, watch), CancellationToken, task spawning | Core async infrastructure, same as all other venues |
| metrics | 0.23 (existing) | Prometheus gauge/counter/histogram recording | All venue metrics use `metrics::gauge!`/`counter!` macros |
| metrics-exporter-prometheus | 0.15 (existing) | Prometheus HTTP scrape endpoint | Already installed in `main.rs` before pipeline spawning |
| tracing | 0.1 (existing) | Structured logging with venue labels | All pipeline components use `tracing::info!`/`warn!` |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-util | existing | `CancellationToken` for crash isolation | Each venue pipeline gets a child token |

### What NOT to Add

- No new crates needed -- this phase is pure wiring within existing infrastructure
- No custom metric types -- `metrics::gauge!`/`counter!` macros with `venue => "derive"` label suffice
- No new trait implementations -- `DeriveSupervisor`, `DeriveProcessor`, `VenueHealth` already implement the required interfaces

## Architecture Patterns

### Pattern 1: Per-Venue Pipeline Block (from existing Deribit/Kalshi/Polymarket blocks)

**What:** Each venue in `run_live_multi_venue()` follows an identical 7-step pattern inside a scoped block.
**When to use:** Adding any new venue to the live pipeline.
**Structure:**

```rust
// --- Derive pipeline ---
{
    // 1. Create VenueHealth tracker
    let health = VenueHealth::new(Venue::Derive);
    venue_health_handles.push(health.clone());

    // 2. Create child CancellationToken for crash isolation
    let venue_cancel = cancel.child_token();

    // 3. Start RecordingService for JSONL raw feed recording
    let derive_recording = RecordingService::start(
        recording_dir.join("derive"),
        Venue::Derive,
        venue_cancel.clone(),
    );

    // 4. Create rate limiter and store for lifecycle sharing
    let rate_limiter = VenueRateLimiter::new("derive", config.derive.rate_limit_per_second);
    rate_limiters.insert(Venue::Derive, rate_limiter.clone());

    // 5. Create supervisor with watch channel for dynamic instruments
    let (supervisor_tx, supervisor_rx) = mpsc::channel::<RawMessage>(1024);
    let instruments_rx = match derive_rx {
        Some(rx) => rx,
        None => {
            let (_tx, rx) = watch::channel(config.derive.instruments.clone());
            rx
        }
    };
    let supervisor = DeriveSupervisor::new(
        config.derive.clone(),
        instruments_rx,
        venue_cancel.clone(),
        rate_limiter,
        health.clone(),
    );
    tokio::spawn(supervisor.run(supervisor_tx));

    // 6. Create processor with recording + cleanup channels
    let (processor, venue_snapshot_rx) = DeriveProcessor::new(
        supervisor_rx,
        Some(derive_recording.sender()),
        venue_cancel.clone(),
        &config.derive,
        derive_cleanup_rx,
    );
    tokio::spawn(processor.run());

    // 7. Forward snapshots to shared fan-in channel
    let fan_in_tx = snapshot_tx.clone();
    tokio::spawn(forward_snapshots(
        venue_snapshot_rx,
        fan_in_tx,
        Venue::Derive,
        venue_cancel,
        Some(health.clone()),
        event_registry.clone(),
    ));
}
```

### Pattern 2: SubscriptionManager Venue Extension

**What:** Adding a new venue to `SubscriptionManager` requires exactly 6 coordinated changes.
**Changes required:**

1. Add `derive: watch::Sender<Vec<String>>` to `SubscriptionSenders`
2. Add `derive: watch::Receiver<Vec<String>>` to `SubscriptionReceivers`
3. Add `derive_tx: watch::Sender<Vec<String>>` field to `SubscriptionManager` struct
4. Add `current_derive: HashSet<String>` field to `SubscriptionManager` struct
5. Extend `compute_desired_instruments()` to extract `mapping.venues.derive.instrument`
6. Add Derive diff/log/send/metrics block in `reconcile()` (identical to Deribit's block)
7. Add Derive to `create_channels()` with sorted initial list
8. Populate `CleanupEvent.derive_instruments` with removed instruments (currently `Vec::new()`)

### Pattern 3: Subscription Watch Channel Seeding

**What:** Watch channels must be seeded with initial instrument lists, not empty vecs.
**Why critical:** Empty initial value causes supervisors to connect with zero instruments, then reconnect when first reconciliation fires (Pitfall 2 from Phase 22 research). `create_channels()` reads the registry upfront to seed correct initial values.

```rust
// In create_channels():
let (desired_d, desired_p, desired_k, desired_derive) = Self::compute_desired_instruments(registry);
// ... sort and create watch channels seeded with desired values
let (derive_tx, derive_rx) = watch::channel(initial_derive);
```

### Pattern 4: Cleanup Channel Extension

**What:** `run_live_multi_venue()` creates cleanup channels for stateful engines. Derive processor needs one too.
**Current cleanup channels:** `[spread, signal, pricing, deribit, kalshi]` (5 total)
**Required change:** Add `derive_cleanup_tx/rx` pair. Sender goes to `PipelineHandles.cleanup_txs`, receiver goes to `DeriveProcessor::new()`.

### Anti-Patterns to Avoid

- **Modifying downstream engines:** SpreadEngine, SignalEngine, PricingEngine, PaperTradeTracker should NOT be touched. They already consume `MarketSnapshot` venue-agnostically. The only venue-aware code is in `PricingEngine` (price conversion gating, added in Phase 31).
- **Skipping the fan-in pattern:** Do NOT send Derive snapshots directly to engines. They MUST go through the shared `snapshot_tx` channel and the fan-out task in `main.rs`.
- **Creating Derive-specific metric names:** Use the existing `"venue" => "derive"` label pattern on existing metric names (`feed_available`, `feed_latency_ms`, `subscription_active`, etc.).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Connection health tracking | Custom Derive health struct | `VenueHealth::new(Venue::Derive)` | Already handles availability, message timestamps, connection counting, and metrics |
| Subscription reconciliation | Custom Derive subscription logic | Extend existing `SubscriptionManager` | HashSet diff, Notify ordering, and watch channel push are already correct |
| Raw feed recording | Custom Derive recording | `RecordingService::start(..., Venue::Derive, ...)` | Venue-generic JSONL writer already supports any `Venue` variant |
| Snapshot fan-in | Custom Derive-to-engine channel | Clone `snapshot_tx` and use `forward_snapshots()` | Existing function handles event_id annotation, health recording, and backpressure |
| Rate limiting | Custom Derive rate limiter | `VenueRateLimiter::new("derive", rate)` | Arc-wrapped governor already tested for Deribit/Polymarket/Kalshi |

**Key insight:** Phase 32 adds zero new abstractions. Every component it touches already handles N venues -- this phase changes N from 3 to 4.

## Common Pitfalls

### Pitfall 1: Forgetting to Destructure Derive from SubscriptionReceivers

**What goes wrong:** The `subscription_rx` Option is destructured into `(deribit_rx, polymarket_rx, kalshi_rx)` in `run_live_multi_venue()`. If Derive is added to `SubscriptionReceivers` but not destructured here, the Derive supervisor gets no watch channel and can't receive dynamic instrument updates.
**How to avoid:** Add `derive_rx` to the destructuring tuple. When `None`, create a one-shot watch channel seeded from `config.derive.instruments`.

### Pitfall 2: Missing `snapshot_tx.clone()` Before Drop

**What goes wrong:** `snapshot_tx` is dropped after all venue blocks to close the fan-in channel. If the Derive block is placed AFTER the `drop(snapshot_tx)`, the Derive `forward_snapshots` task gets a dead sender.
**How to avoid:** Place the Derive pipeline block BEFORE the `drop(snapshot_tx)` line. Clone `snapshot_tx` within the Derive block.

### Pitfall 3: Forgetting to Update `compute_desired_instruments` Return Type

**What goes wrong:** The function currently returns a 3-tuple `(HashSet<String>, HashSet<PolymarketSubscription>, HashSet<String>)`. Adding Derive requires a 4-tuple. All call sites must be updated.
**How to avoid:** Update both `compute_desired_instruments` (function body and signature) and all callers: `reconcile()` and `create_channels()`.

### Pitfall 4: DeriveProcessor Constructor Mismatch

**What goes wrong:** DeriveProcessor's `new()` takes `&DeriveConfig` as a parameter (not `staleness_threshold_ms: u64` like DeribitProcessor). Copying Deribit's pipeline wiring pattern verbatim will cause a compile error.
**How to avoid:** Use `DeriveProcessor::new(supervisor_rx, Some(recording.sender()), cancel, &config.derive, cleanup_rx)` -- pass the full config reference.

### Pitfall 5: Not Adding Derive to main.rs Venue Availability Log

**What goes wrong:** The startup log in `main.rs` currently logs availability for deribit, polymarket, and kalshi. Missing Derive makes debugging connection issues harder.
**How to avoid:** Add `derive = "available (public, no auth)"` to the venue availability tracing::info! call.

### Pitfall 6: Metrics Counter for Reconnection Events

**What goes wrong:** PIPE-05 requires "reconnection events" metric. `VenueHealth::increment_connections()` already tracks this via atomic counter, but it doesn't emit a metrics counter. The `connection_count` is only readable via the health endpoint, not Prometheus.
**How to avoid:** Add a `metrics::counter!("feed_reconnections_total", "venue" => venue)` call in `DeriveSupervisor` (or `VenueHealth::increment_connections()`). Check whether existing Deribit supervisor already emits this -- if not, adding it to `VenueHealth` benefits all venues.

## Code Examples

### SubscriptionManager: compute_desired_instruments with Derive

```rust
// Source: Existing pattern in src/subscription/manager.rs, extended for Derive
fn compute_desired_instruments(
    registry: &EventRegistry,
) -> (HashSet<String>, HashSet<PolymarketSubscription>, HashSet<String>, HashSet<String>) {
    let mut deribit = HashSet::new();
    let mut polymarket = HashSet::new();
    let mut kalshi = HashSet::new();
    let mut derive = HashSet::new();

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
        if let Some(ref dr) = mapping.venues.derive {
            derive.insert(dr.instrument.clone());
        }
    }

    (deribit, polymarket, kalshi, derive)
}
```

### Pipeline: Derive Block in run_live_multi_venue

```rust
// Source: Adapted from Deribit pipeline block in src/feed/pipeline.rs
// Key differences from Deribit: no credential guard, uses DeriveProcessor::new(&config.derive, ...)
// Key differences from Kalshi: no auth, always starts (like Polymarket)
```

### SubscriptionReceivers Extension

```rust
pub struct SubscriptionSenders {
    pub deribit: watch::Sender<Vec<String>>,
    pub polymarket: watch::Sender<Vec<PolymarketSubscription>>,
    pub kalshi: watch::Sender<Vec<String>>,
    pub derive: watch::Sender<Vec<String>>,  // NEW
}

pub struct SubscriptionReceivers {
    pub deribit: watch::Receiver<Vec<String>>,
    pub polymarket: watch::Receiver<Vec<PolymarketSubscription>>,
    pub kalshi: watch::Receiver<Vec<String>>,
    pub derive: watch::Receiver<Vec<String>>,  // NEW
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| 3-venue hardcoded pipeline | 4-venue pipeline with Derive | Phase 32 | Derive snapshots flow to all downstream engines |
| `CleanupEvent.derive_instruments: Vec::new()` | Populated from actual diff | Phase 32 | Removed Derive instruments trigger proper cleanup in DeriveProcessor |
| No Derive subscription management | Full HashSet diff + watch channel | Phase 32 | Dynamic instrument updates via hot-reload propagate to DeriveSupervisor |

## Metrics Inventory (PIPE-05)

Metrics that will be active for Derive after Phase 32:

| Metric | Type | Source | When Active |
|--------|------|--------|-------------|
| `feed_available{venue="derive"}` | Gauge (0/1) | `VenueHealth::mark_available/mark_unavailable` | Automatic from VenueHealth creation |
| `feed_latency_ms{venue="derive"}` | Histogram | `DeriveProcessor::build_snapshot()` | Already emitted (Phase 31) |
| `feed_last_latency_ms{venue="derive"}` | Gauge | `DeriveProcessor::build_snapshot()` | Already emitted (Phase 31) |
| `feed_messages_total{venue="derive"}` | Counter | `DeriveProcessor::build_snapshot()` | Already emitted (Phase 31) |
| `subscription_active{venue="derive"}` | Gauge | `SubscriptionManager::reconcile()` | Active after Phase 32 wiring |
| `subscription_activations_total{venue="derive"}` | Counter | `SubscriptionManager::reconcile()` | Active after Phase 32 wiring |
| `subscription_removals_total{venue="derive"}` | Counter | `SubscriptionManager::reconcile()` | Active after Phase 32 wiring |
| `feed_reconnections_total{venue="derive"}` | Counter | New: `VenueHealth::increment_connections()` | Needs addition in Phase 32 |

## Files to Modify

| File | Change Description |
|------|-------------------|
| `src/subscription/manager.rs` | Add `derive` to Senders/Receivers, add `current_derive`/`derive_tx` fields, extend `compute_desired_instruments` to 4-tuple, add Derive diff/log/send/metrics in `reconcile()`, add Derive to `create_channels()`, populate `CleanupEvent.derive_instruments` |
| `src/feed/pipeline.rs` | Add `use` for `DeriveProcessor`/`DeriveSupervisor`, add `derive_cleanup_tx/rx` channel pair, destructure `derive_rx` from `SubscriptionReceivers`, add `// --- Derive pipeline ---` block, add `derive_cleanup_tx` to `cleanup_txs` vec |
| `src/main.rs` | Add `derive = "available"` to venue availability log, destructure `derive_rx` from `SubscriptionReceivers` in channel creation |
| `src/feed/health.rs` | (Optional) Add `metrics::counter!("feed_reconnections_total", ...)` in `increment_connections()` for PIPE-05 reconnection metric |

## Open Questions

1. **Reconnection counter metric location**
   - What we know: `VenueHealth::increment_connections()` counts connection attempts but doesn't emit a Prometheus counter. PIPE-05 requires "reconnection events" metric.
   - What's unclear: Should the counter be added to `VenueHealth::increment_connections()` (benefits all venues) or only in `DeriveSupervisor`?
   - Recommendation: Add to `VenueHealth::increment_connections()` since it's already called by all supervisors and provides consistent cross-venue metrics. This is a one-line addition.

2. **Event_ids in CleanupEvent**
   - What we know: `CleanupEvent.event_ids` is currently `Vec::new()` in the cleanup construction. The comment says "Populated by Plan 02 when wiring is complete."
   - What's unclear: Whether this Phase should populate event_ids or leave it for a future phase.
   - Recommendation: Populate it in this phase -- we have access to the registry when computing diffs, and event_ids enable downstream engines to clean up event-level state.

## Sources

### Primary (HIGH confidence)
- `src/subscription/manager.rs` -- Current 3-venue SubscriptionManager implementation (410 lines, fully read)
- `src/feed/pipeline.rs` -- Current 3-venue pipeline wiring (560 lines, fully read)
- `src/feed/derive/supervisor.rs` -- DeriveSupervisor already built (Phase 31)
- `src/feed/derive/normalize.rs` -- DeriveProcessor already built (Phase 31)
- `src/feed/health.rs` -- VenueHealth implementation with metrics
- `src/main.rs` -- Pipeline startup, SubscriptionManager creation, fan-out wiring
- `src/metrics_export/mod.rs` -- Prometheus recorder setup
- `src/config/events.rs` -- `EventVenues.derive: Option<DeriveMapping>` already exists
- `src/config/venues.rs` -- `VenuesConfig.derive: DeriveConfig` already exists

### Secondary (MEDIUM confidence)
- `.planning/phases/31-derive-feed-and-normalization/31-RESEARCH.md` -- Phase 31 research confirming Derive implementation details

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all libraries already in use
- Architecture: HIGH -- exact same patterns replicated from 3 existing venues, code fully inspected
- Pitfalls: HIGH -- identified from reading actual code structure and spotting asymmetries (DeriveProcessor constructor, tuple destructuring)

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable internal architecture, no external API changes)
