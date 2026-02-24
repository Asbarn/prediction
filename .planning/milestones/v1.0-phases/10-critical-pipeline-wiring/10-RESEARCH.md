# Phase 10: Critical Pipeline Wiring - Research

**Researched:** 2026-02-24
**Domain:** Async channel wiring, cross-phase data flow, tokio::sync::watch propagation
**Confidence:** HIGH

## Summary

Phase 10 addresses three broken end-to-end flows discovered by the v1.0 milestone audit. All three are **wiring bugs** -- the components exist and work correctly in isolation, but the channels connecting them are either orphaned (receiver dropped) or the data they carry is missing a critical field populated at the wrong layer. No new crates, algorithms, or architectural changes are needed. This is purely a plumbing phase.

The three breaks are: (1) `MarketSnapshot.event_id` is never populated by any venue normalizer, causing PaperTradeTracker to discard every snapshot; (2) `_arb_signal_rx` in main.rs is bound with underscore and never read, so ArbSignal outputs from CrossAssetEngine silently fill the buffer and get dropped; (3) `_config_rx` in main.rs is discarded, so config hot-reload file changes are detected but never propagated to any engine.

All three fixes require changes to existing files only. No new dependencies. The codebase already has the infrastructure (EventRegistry lookup, watch channel, ArbSignal type) -- the wires just need connecting.

**Primary recommendation:** Wire the three orphaned channels/fields by annotating snapshots with event_id in `forward_snapshots`, consuming `arb_signal_rx` with a logging task, and plumbing `config_rx` into the EventRegistry refresh loop.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| OBSV-04 | Paper trade P&L tracking: hypothetical entry/exit at signal time, per-signal P&L assuming fill at quoted price, daily/weekly aggregates | Fix event_id population so PaperTradeTracker.handle_snapshot() stops discarding every snapshot. The tracker logic is complete and tested -- it just never receives snapshots with event_id populated. |
| SGNL-05 | Signal generation produces ArbSignal with: event ID, direction, raw spread, net edge after costs, confidence, constituent legs, timestamp, and TTL | ArbSignal struct is complete and logged to JSONL. The fix is adding a consumer for `arb_signal_rx` so signals are not silently dropped. Logging to tracing + metrics satisfies v1 "produces ArbSignal" since execution is v2. |
| OBSV-01 | All parameters configurable via TOML: strike filters, staleness thresholds, fee assumptions, signal thresholds, log rotation, venue credentials | Config loads at startup (satisfied). Hot-reload via ConfigReloader detects file changes (satisfied). The fix is wiring `config_rx` to at least the EventRegistry so runtime config changes propagate. Full engine hot-reload is out of scope for v1 -- EventRegistry refresh is the minimum viable fix. |
</phase_requirements>

## Standard Stack

### Core

No new libraries needed. This phase uses only existing project dependencies:

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio::sync::mpsc | tokio 1.x | Bounded async channel for ArbSignal consumption | Already used throughout for all pipeline channels |
| tokio::sync::watch | tokio 1.x | Config hot-reload broadcast | Already used by ConfigReloader, just need to add subscriber |
| tokio::sync::RwLock | tokio 1.x | Shared access to EventRegistry | Already used in main.rs and all engines |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 | Structured logging for ArbSignal consumer | Already imported in main.rs |
| metrics | 0.24 | Prometheus counters for signal consumption tracking | Already used throughout |

### Alternatives Considered

None. This phase requires zero new dependencies. All components and channel types already exist.

## Architecture Patterns

### Pattern 1: Event ID Annotation in `forward_snapshots`

**What:** The `forward_snapshots()` function in `feed/pipeline.rs` is the fan-in point where per-venue processor snapshots merge into the shared channel. This is the natural annotation point -- it already receives each snapshot and can look up the EventRegistry to populate `event_id`.

**When to use:** Every snapshot that passes through the pipeline needs event_id annotation before reaching downstream engines (SpreadEngine, PricingEngine, CrossAssetEngine, PaperTradeTracker).

**Why here, not in normalizers:** The normalizers (DeribitProcessor, PolymarketProcessor, KalshiProcessor) do not have access to the EventRegistry. They produce raw snapshots with `event_id: None`. The `forward_snapshots` function is the first point in the pipeline where the shared EventRegistry is accessible.

**Implementation approach:**
- Pass `Arc<RwLock<EventRegistry>>` to `forward_snapshots`
- Before sending each snapshot, do a non-blocking `registry.read().await` lookup
- If found, set `snap.event_id = Some(EventId::new(mapping.id.clone()))`
- If not found, leave as None (unmapped instruments are normal)

**Key detail:** The `_event_registry` parameter on `run_multi_venue_pipeline()` is already present but unused (passed as `_event_registry`). Remove the underscore and thread it through to `forward_snapshots` calls.

```rust
// In forward_snapshots (sketch):
pub async fn forward_snapshots(
    mut venue_rx: mpsc::Receiver<MarketSnapshot>,
    fan_in_tx: mpsc::Sender<MarketSnapshot>,
    venue: Venue,
    cancel: CancellationToken,
    health: Option<Arc<VenueHealth>>,
    registry: Option<Arc<RwLock<EventRegistry>>>,  // NEW parameter
) {
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => { break; }
            snapshot = venue_rx.recv() => {
                match snapshot {
                    Some(mut snap) => {
                        // Annotate event_id from registry
                        if let Some(ref reg) = registry {
                            let r = reg.read().await;
                            if let Some(mapping) = r.lookup_by_instrument(snap.venue, &snap.instrument_id.to_string()) {
                                snap.event_id = Some(EventId::new(&mapping.id));
                            }
                        }
                        if let Some(h) = &health {
                            h.record_message();
                        }
                        if fan_in_tx.send(snap).await.is_err() { break; }
                    }
                    None => { break; }
                }
            }
        }
    }
}
```

**Source:** Direct codebase analysis of `src/feed/pipeline.rs`, `src/events/registry.rs`, `src/types/snapshot.rs`

### Pattern 2: ArbSignal Consumer Task

**What:** Spawn a tokio task that reads from `arb_signal_rx`, logs each signal at INFO level with structured fields, and increments Prometheus counters.

**When to use:** v1 has no execution engine. The consumer's job is to make signals visible and measurable. This satisfies SGNL-05 "produces ArbSignal" by ensuring they are not silently dropped.

**Implementation approach:**
- Remove underscore from `_arb_signal_rx` in main.rs (line 310)
- Spawn a consumer task with its own CancellationToken
- Each received ArbSignal: log event_id, direction, net_edge, confidence at INFO level
- Increment `arb_signals_consumed_total` counter with direction/event labels

```rust
// In main.rs after CrossAssetEngine spawn:
let arb_cancel = shutdown_token.child_token();
tokio::spawn(async move {
    let mut arb_signal_rx = arb_signal_rx;
    loop {
        tokio::select! {
            biased;
            _ = arb_cancel.cancelled() => { break; }
            signal = arb_signal_rx.recv() => {
                match signal {
                    Some(sig) => {
                        tracing::info!(
                            event_id = %sig.event_id,
                            direction = ?sig.direction,
                            net_edge = %sig.net_edge,
                            confidence = %sig.confidence,
                            "ArbSignal received"
                        );
                        metrics::counter!("arb_signals_consumed_total",
                            "direction" => format!("{:?}", sig.direction)
                        ).increment(1);
                    }
                    None => { break; }
                }
            }
        }
    }
});
```

**Source:** Direct analysis of main.rs lines 309-323, `src/signal/types.rs`

### Pattern 3: Config Watch Subscription for EventRegistry

**What:** Wire the `config_rx` watch::Receiver to a task that refreshes the EventRegistry when config changes.

**When to use:** When `events.toml` is modified (e.g., new event mappings added, status changes). The ConfigReloader already detects TOML file changes and broadcasts the new AppConfig -- we just need a subscriber.

**Implementation approach:**
- Remove underscore from `_config_rx` in main.rs (line 110)
- Spawn a task that calls `config_rx.changed().await` in a loop
- On each change, extract `new_config.events` and call `event_registry.write().await.refresh(&new_config.events)`
- This is the minimum viable config reload: EventRegistry refresh. Full engine reconfiguration (staleness thresholds, fee models) is deferred to v2.

**Key detail:** `tokio::sync::watch::Receiver::changed()` returns `Ok(())` when a new value is available. The receiver can then call `.borrow()` to get the current value. The watch channel is multi-consumer -- calling `.clone()` on the receiver creates additional subscribers if needed later.

```rust
// In main.rs after EventRegistry creation:
let config_cancel = shutdown_token.child_token();
let config_registry = event_registry.clone();
tokio::spawn(async move {
    let mut config_rx = config_rx;
    loop {
        tokio::select! {
            biased;
            _ = config_cancel.cancelled() => { break; }
            result = config_rx.changed() => {
                match result {
                    Ok(()) => {
                        let new_config = config_rx.borrow().clone();
                        let mut reg = config_registry.write().await;
                        reg.refresh(&new_config.events);
                        tracing::info!(
                            mappings = reg.mapping_count(),
                            "EventRegistry refreshed from config hot-reload"
                        );
                    }
                    Err(_) => {
                        tracing::debug!("config watch channel closed");
                        break;
                    }
                }
            }
        }
    }
});
```

**Source:** Direct analysis of `src/config/reload.rs`, tokio::sync::watch API

### Anti-Patterns to Avoid

- **Annotating event_id inside normalizers:** The normalizers (DeribitProcessor, PolymarketProcessor, KalshiProcessor) do not have access to EventRegistry and should remain venue-specific. Adding registry access to every normalizer would create tight coupling and duplicate lookup logic 3x.

- **Blocking read lock on EventRegistry in hot path:** The `forward_snapshots` annotation uses `registry.read().await` which is non-blocking for concurrent readers. Never use `write()` in the forwarding hot path.

- **Full engine reconfiguration on config reload:** Engines (SpreadEngine, PricingEngine, CrossAssetEngine) have complex internal state (rolling stats, latest snapshots, caches). Hot-swapping their config mid-run risks inconsistency. For v1, only EventRegistry refresh is in scope. Engine restart for config changes is acceptable.

- **Moving annotation to the fan-out task in main.rs:** The fan-out task (lines 232-273 in main.rs) could annotate, but this is the wrong layer -- it would only work for the main pipeline, not for replay. Annotation in `forward_snapshots` covers both Live and Replay modes.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Config change notification | Custom file polling | `tokio::sync::watch::Receiver::changed()` | Already built by ConfigReloader in Phase 1 |
| Event ID lookup | Linear scan of mappings | `EventRegistry::lookup_by_instrument()` | O(1) HashMap lookup, already implemented and tested |
| ArbSignal consumer | Custom signal aggregator | Simple logging task with tracing + metrics | v1 is paper trading; execution (the real consumer) is v2 |

**Key insight:** Every component needed already exists. This phase is pure wiring -- connecting existing pieces that were built in isolation across phases 1-9.

## Common Pitfalls

### Pitfall 1: Replay Mode Missing Event ID Annotation

**What goes wrong:** The `run_replay_pipeline()` in `replay/mod.rs` also calls `forward_snapshots` but does not pass the EventRegistry. If annotation is only added to the live pipeline, replay mode will still produce snapshots with `event_id: None`.

**Why it happens:** The replay pipeline was built to mirror the live pipeline structure, but the live pipeline never used `_event_registry` either. Both paths need the fix.

**How to avoid:** Thread `event_registry` into `run_replay_pipeline` and pass it to all `forward_snapshots` calls. The `run_multi_venue_pipeline` already receives it -- just pass it through. For replay, the `run_replay_pipeline` signature needs an optional `Arc<RwLock<EventRegistry>>` parameter.

**Warning signs:** Replay mode produces zero paper trades despite having signal activity.

### Pitfall 2: Watch Channel Borrow vs Clone

**What goes wrong:** Calling `config_rx.borrow()` returns a `Ref<AppConfig>` that holds a read lock on the watch channel. If this borrow is held across an `.await` point, it blocks the sender from updating.

**Why it happens:** `watch::Ref` implements `Deref` but not `Send`, and holding it across await points is a common mistake.

**How to avoid:** Always `.borrow().clone()` immediately to get an owned `AppConfig`, then drop the `Ref`. The `AppConfig` derives `Clone` so this is safe.

**Warning signs:** Compiler error "future is not Send" or config reload hangs.

### Pitfall 3: PaperTradeTracker Receives Snapshots Without event_id

**What goes wrong:** The SpreadEngine forwards snapshots to PaperTradeTracker via `ptrade_snap_tx` (line 116 in spread/engine.rs). These are clones of the same snapshots received from the fan-out. If event_id is populated before the fan-out (in `forward_snapshots`), the ptrade snapshots will carry the event_id. If event_id is only populated after the fan-out (in SpreadEngine itself), the ptrade forwarded clone will still have `event_id: None`.

**Why it happens:** The fan-out task in main.rs clones snapshots before sending to SpreadEngine. If event_id is annotated in `forward_snapshots` (before fan-out), the clones carry the annotation correctly. This is the correct fix location.

**How to avoid:** Annotate event_id in `forward_snapshots` (before fan-out in main.rs), not in SpreadEngine's `process_snapshot`. This ensures all downstream consumers (SpreadEngine, PricingEngine, CrossAssetEngine, and through SpreadEngine -> PaperTradeTracker) see the populated event_id.

**Warning signs:** SpreadEngine processes snapshots correctly (it does its own registry lookup) but PaperTradeTracker still gets `event_id: None`.

### Pitfall 4: ArbSignal Consumer Backpressure

**What goes wrong:** If the ArbSignal consumer task is slow (e.g., synchronous I/O in the loop), the bounded channel (1024) fills up and CrossAssetEngine's `try_send` starts dropping signals.

**Why it happens:** CrossAssetEngine uses `try_send` (non-blocking) to avoid blocking on slow downstream consumers.

**How to avoid:** The consumer should only do cheap operations: structured tracing log + metrics counter increment. No disk I/O, no network calls. The CrossAssetEngine already logs signals to JSONL, so the consumer does not need to duplicate that.

**Warning signs:** `arb_signals_consumed_total` metric diverges from `arb_signals_emitted_total` (if such a counter exists in CrossAssetEngine).

### Pitfall 5: forward_snapshots Signature Change Breaks Callers

**What goes wrong:** Adding the `registry` parameter to `forward_snapshots` breaks all existing call sites: live pipeline (3 calls), replay pipeline (3 calls), and potentially any test code.

**Why it happens:** Rust function signatures are not optional-parameter-friendly.

**How to avoid:** Make the parameter `Option<Arc<RwLock<EventRegistry>>>` with `None` as the default for contexts that don't have a registry (backward compatible). Update all call sites in both `pipeline.rs` and `replay/mod.rs`.

**Warning signs:** Compiler errors listing all call sites of `forward_snapshots`.

## Code Examples

### Event ID Annotation Pattern

The EventRegistry already has the exact lookup method needed:

```rust
// src/events/registry.rs (existing code)
pub fn lookup_by_instrument(&self, venue: Venue, instrument_id: &str) -> Option<&EventMapping> {
    self.instrument_index
        .get(&(venue, instrument_id.to_string()))
        .map(|&idx| &self.mappings[idx])
}
```

The annotation in `forward_snapshots` would be:
```rust
if let Some(ref reg) = registry {
    let r = reg.read().await;
    if let Some(mapping) = r.lookup_by_instrument(snap.venue, &snap.instrument_id.to_string()) {
        snap.event_id = Some(EventId::new(&mapping.id));
    }
}
```

**Source:** `src/events/registry.rs:41-45`, `src/types/ids.rs:12-16`

### Watch Channel Subscription Pattern

```rust
// tokio::sync::watch subscriber pattern
let mut config_rx: watch::Receiver<AppConfig> = /* from ConfigReloader::start */;

loop {
    match config_rx.changed().await {
        Ok(()) => {
            let new_config = config_rx.borrow().clone(); // borrow + clone immediately
            // ... use new_config ...
        }
        Err(_) => break, // sender dropped
    }
}
```

**Source:** tokio::sync::watch API (verified against tokio 1.x docs)

### ArbSignal Fields Available for Logging

```rust
// From src/signal/types.rs (existing struct)
pub struct ArbSignal {
    pub event_id: String,
    pub direction: ArbDirection,
    pub raw_spread: Decimal,
    pub net_edge: Decimal,
    pub confidence: Decimal,
    // ... additional fields for legs, costs, timestamps
}
```

**Source:** `src/signal/types.rs:141+`

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| event_id never populated | Annotate in forward_snapshots | Phase 10 | PaperTradeTracker becomes functional |
| _arb_signal_rx dropped | Logging consumer task | Phase 10 | ArbSignals visible in logs + metrics |
| _config_rx dropped | Watch subscriber refreshes EventRegistry | Phase 10 | Config hot-reload propagates to mapping layer |

**Deprecated/outdated:**
- `_event_registry` parameter on `run_multi_venue_pipeline`: Currently unused (underscore prefix). Phase 10 removes the underscore and threads it through.

## Open Questions

1. **Should SpreadEngine/CrossAssetEngine use the snapshot's event_id instead of doing their own registry lookup?**
   - What we know: Both engines currently do their own `registry.read().await.lookup_by_instrument()` on every snapshot. After Phase 10, snapshots will already carry `event_id`.
   - What's unclear: Whether to refactor engines to use `snap.event_id` directly, eliminating redundant lookups.
   - Recommendation: Do NOT refactor engines in Phase 10. The redundant lookups are harmless (O(1) HashMap reads) and engines need the full `EventMapping` (not just event_id) for venue presence checks. Leave engine lookup logic unchanged. This is a potential cleanup for tech debt reduction but not a functional requirement.

2. **Should replay mode also get config hot-reload?**
   - What we know: Replay mode is for deterministic historical analysis. Config changes during replay would alter behavior non-deterministically.
   - What's unclear: Whether any replay use case benefits from runtime config changes.
   - Recommendation: No. Config hot-reload subscriber should be spawned only in Live mode (same guard as ContractLifecycleManager). The EventRegistry annotation in forward_snapshots uses the registry state at message-processing time, which is sufficient for replay with static config.

3. **What fields of AppConfig should be hot-reloadable beyond EventRegistry?**
   - What we know: Full engine reconfiguration is complex (staleness thresholds, fee models, pricing parameters). Engines have running state.
   - What's unclear: Which config fields are safe to change at runtime vs requiring restart.
   - Recommendation: For Phase 10, only `events` (EventRegistry refresh) is in scope. This satisfies OBSV-01 "configurable via TOML" + "hot-reload propagates runtime changes" at the mapping layer. Engine parameter hot-reload is a v2 concern.

## Sources

### Primary (HIGH confidence)
- `src/feed/pipeline.rs` - forward_snapshots function, _event_registry parameter, pipeline architecture
- `src/paper_trade/tracker.rs:307-310` - event_id gate that discards all snapshots
- `src/main.rs:110` - _config_rx dropped, `src/main.rs:310` - _arb_signal_rx dropped
- `src/feed/deribit/normalize.rs:516`, `src/feed/polymarket/normalize.rs:183`, `src/feed/kalshi/normalize.rs:232` - all normalizers set event_id: None
- `src/events/registry.rs` - EventRegistry lookup_by_instrument O(1) HashMap
- `src/config/reload.rs` - ConfigReloader watch channel implementation
- `src/signal/types.rs` - ArbSignal struct definition
- `src/replay/mod.rs` - Replay pipeline forward_snapshots calls
- `.planning/v1.0-MILESTONE-AUDIT.md` - Definitive gap analysis

### Secondary (MEDIUM confidence)
- tokio::sync::watch API documentation - changed()/borrow()/clone() patterns

### Tertiary (LOW confidence)
- None. All findings are from direct codebase analysis.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - No new dependencies; all existing crate APIs verified in codebase
- Architecture: HIGH - All three fixes are direct wiring of existing components; patterns verified by reading source
- Pitfalls: HIGH - Every pitfall identified by tracing data flow through actual code paths

**Research date:** 2026-02-24
**Valid until:** Indefinite (pure codebase wiring, no external dependency concerns)
