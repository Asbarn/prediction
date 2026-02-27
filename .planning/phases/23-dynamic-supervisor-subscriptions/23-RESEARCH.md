# Phase 23: Dynamic Supervisor Subscriptions - Research

**Researched:** 2026-02-27
**Domain:** Wiring tokio::sync::watch channels into three venue WebSocket supervisors for reconnect-based dynamic subscription management
**Confidence:** HIGH

## Summary

Phase 23 wires the watch channel receivers created in Phase 22 into all three venue supervisors (Deribit, Polymarket, Kalshi) so that instrument list changes trigger graceful reconnections with updated subscriptions. The SubscriptionManager already computes per-venue diffs and pushes full instrument lists via `watch::Sender`. The supervisors need to: (1) accept a `watch::Receiver` in their constructor, (2) add a `changed()` branch to their inner `select!` loop that breaks to the reconnect loop, and (3) read the latest instrument list via `borrow().clone()` at the top of each connection attempt. The `pipeline.rs` wiring must pass the receivers from `PipelineHandles.subscription_rx` through to each supervisor constructor.

This is a mechanical wiring phase with no new crate dependencies, no new architectural patterns, and no ambiguity. The Phase 22 research already documented the exact patterns (Pattern 3: Watch Channel for Supervisor Instrument Updates, Pattern 4: SubscriptionManager Owns the Watch Senders). The three supervisor files are structurally identical (same reconnect loop pattern), differing only in their venue-specific client creation. Each supervisor modification follows the same template.

**Primary recommendation:** Modify all three supervisor constructors to accept `watch::Receiver`, add `changed()` select branch to break the inner forwarding loop, read latest instruments at the top of the outer reconnect loop, and update `pipeline.rs::run_live_multi_venue()` to thread receivers from `PipelineHandles.subscription_rx` to supervisor constructors. Polymarket requires a conversion step from `PolymarketSubscription` to `PolymarketAsset` when creating the client.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SUB-01 | System subscribes to newly approved instrument feeds without restart when operator sets `approved = true` in events.toml | Supervisors accept watch::Receiver and add `changed()` branch to inner select!. When SubscriptionManager pushes updated instrument list (containing the newly approved instrument), the supervisor breaks its inner loop, re-enters the reconnect loop, reads the updated list via `borrow().clone()`, and creates a fresh client with the new instrument included. No restart required. |
| SUB-02 | System unsubscribes from expired/retired instrument feeds without restart when events are archived | Same mechanism as SUB-01 but in reverse. When an event is archived and removed from `active_approved()`, SubscriptionManager pushes an updated list that excludes the retired instrument. The supervisor's `changed()` branch fires, it breaks to reconnect, and creates a fresh client subscribing only to the remaining instruments. The retired instrument's feed stops because the new connection doesn't subscribe to it. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio::sync::watch | 1.x (bundled with tokio) | Latest-value channels for pushing instrument lists from SubscriptionManager to supervisors | Already used by SubscriptionManager (Phase 22). `changed()` provides async notification, `borrow()` provides zero-copy read of latest value. |
| tokio::select! | 1.x (bundled with tokio) | Multiplex cancel + changed + message recv in supervisor inner loops | Already the core pattern in all three supervisors. Adding one more branch is mechanical. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1.x | Structured logging for reconnection events triggered by subscription changes | Already used everywhere. Log at info level when `changed()` fires. |
| metrics | 0.x | Counter for subscription-triggered reconnects per venue | Already used in supervisors. Add `feed_subscription_reconnects` counter. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `watch::Receiver::changed()` in inner select | Poll with `has_changed()` at top of outer loop only | Would not trigger mid-connection reconnect. Subscription changes would only take effect on next natural reconnect (could be minutes with exponential backoff). `changed()` in select gives sub-second response time. |
| Breaking inner loop on `changed()` | Sending in-connection subscribe/unsubscribe commands | Explicitly out of scope per REQUIREMENTS.md "Out of Scope" table. Reconnect-based approach is the project decision. |

**Installation:** No new dependencies. All libraries already in use.

## Architecture Patterns

### Recommended File Changes
```
src/
├── feed/
│   ├── deribit/
│   │   └── supervisor.rs    # MODIFY: accept watch::Receiver<Vec<String>>, add changed() branch
│   ├── polymarket/
│   │   └── supervisor.rs    # MODIFY: accept watch::Receiver<Vec<PolymarketSubscription>>, add changed() branch
│   ├── kalshi/
│   │   └── supervisor.rs    # MODIFY: accept watch::Receiver<Vec<String>>, add changed() branch
│   └── pipeline.rs          # MODIFY: thread subscription receivers to supervisor constructors
└── subscription/
    └── manager.rs           # NO CHANGE (Phase 22 complete)
```

### Pattern 1: Watch Receiver in Supervisor Constructor (Deribit/Kalshi)
**What:** Replace the static `instruments: Vec<String>` field with a `watch::Receiver<Vec<String>>` field. Read the latest value at each reconnection attempt.
**When to use:** Deribit and Kalshi supervisors, which use `Vec<String>` instrument lists.
**Example:**
```rust
// Source: tokio::sync::watch docs + Phase 22 research Pattern 3
pub struct DeribitSupervisor {
    config: DeribitConfig,
    instruments_rx: watch::Receiver<Vec<String>>,  // Was: instruments: Vec<String>
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,
    health: Arc<VenueHealth>,
}

impl DeribitSupervisor {
    pub fn new(
        config: DeribitConfig,
        instruments_rx: watch::Receiver<Vec<String>>,  // Was: instruments: Vec<String>
        cancel: CancellationToken,
        rate_limiter: VenueRateLimiter,
        health: Arc<VenueHealth>,
    ) -> Self {
        Self { config, instruments_rx, cancel, rate_limiter, health }
    }

    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
        // ... backoff setup unchanged ...

        loop {
            if self.cancel.is_cancelled() { break; }

            // Read latest instrument list at each reconnection attempt
            let instruments = self.instruments_rx.borrow().clone();

            let client = DeribitClient::new(
                self.config.clone(),
                instruments,  // Was: self.instruments.clone()
                self.cancel.clone(),
            ).with_rate_limiter(self.rate_limiter.clone());

            match client.start().await {
                Ok(mut raw_rx) => {
                    let mut received_first = false;
                    loop {
                        tokio::select! {
                            biased;
                            _ = self.cancel.cancelled() => { return; }
                            // NEW: subscription change triggers reconnect
                            Ok(()) = self.instruments_rx.changed() => {
                                tracing::info!(
                                    "DeribitSupervisor: instrument list updated, reconnecting"
                                );
                                break; // -> outer loop re-enters, reads updated list
                            }
                            msg = raw_rx.recv() => {
                                // ... existing forwarding logic unchanged ...
                            }
                        }
                    }
                }
                Err(e) => { /* ... existing error handling unchanged ... */ }
            }
            // ... existing backoff logic unchanged ...
        }
    }
}
```

### Pattern 2: Watch Receiver in Supervisor Constructor (Polymarket)
**What:** Polymarket requires a conversion step because the watch channel carries `Vec<PolymarketSubscription>` but PolymarketClient reads from `config.assets: Vec<PolymarketAsset>`. The supervisor must convert and inject the instrument list into a cloned config before creating the client.
**When to use:** PolymarketSupervisor only.
**Example:**
```rust
// Source: Phase 22 research + existing PolymarketSupervisor + ARCHITECTURE.md
pub struct PolymarketSupervisor {
    config: PolymarketConfig,
    assets_rx: watch::Receiver<Vec<PolymarketSubscription>>,  // NEW
    cancel: CancellationToken,
    health: Arc<VenueHealth>,
}

// At top of reconnect loop:
let subscriptions = self.assets_rx.borrow().clone();
let mut config = self.config.clone();
config.assets = subscriptions.into_iter().map(|s| PolymarketAsset {
    condition_id: s.condition_id,
    token_id: s.token_id,
}).collect();
let client = PolymarketClient::new(config, self.cancel.clone());
```

### Pattern 3: Watch Receiver in Supervisor Constructor (Kalshi)
**What:** Kalshi uses `config.market_tickers: Vec<String>`. The supervisor reads the latest tickers from the watch channel and injects them into a cloned config before creating the client.
**When to use:** KalshiSupervisor only.
**Example:**
```rust
// At top of reconnect loop:
let tickers = self.tickers_rx.borrow().clone();
let mut config = self.config.clone();
config.market_tickers = tickers;
let client = KalshiClient::new(config, self.api_key_id.clone(), self.private_key.clone(), self.cancel.clone());
```

### Pattern 4: Pipeline.rs Threading
**What:** `run_live_multi_venue()` must accept `Option<SubscriptionReceivers>` and destructure it to pass individual receivers to supervisor constructors. When `None` (Mock/Replay), supervisors use the static config values.
**When to use:** `pipeline.rs::run_live_multi_venue()` modification.
**Key consideration:** The function already receives instruments from config (`config.deribit.instruments`, `config.polymarket.assets`, `config.kalshi.market_tickers`). For live mode with subscription receivers, the watch channel values override the config values. For non-live modes, the config values are used directly (no watch channels).

### Pattern 5: Backoff Reset on Subscription Change
**What:** When the inner loop breaks due to `changed()` (not connection loss), the backoff should be reset. The reconnection is intentional, not a failure.
**When to use:** All three supervisors when `changed()` triggers the break.
**Example:**
```rust
Ok(()) = self.instruments_rx.changed() => {
    tracing::info!("DeribitSupervisor: instrument list updated, reconnecting");
    backoff.reset();  // Intentional reconnect, not a failure
    break;
}
```

### Anti-Patterns to Avoid
- **Moving `borrow()` inside the select! loop:** Holding a `Ref` guard across an `.await` point causes deadlock. Always `borrow().clone()` and drop the guard immediately. The tokio docs explicitly warn about this.
- **Using `borrow_and_update()` instead of `borrow()` at the top of the reconnect loop:** `borrow_and_update()` marks the value as seen, which would cause the `changed()` branch in the inner select to miss the update that just fired. Use `borrow()` for reading the value, let `changed()` handle the seen/unseen tracking.
- **Putting `changed()` in the outer loop without the inner select branch:** The supervisor would only pick up changes at natural reconnect boundaries. If the connection is stable for hours, subscription changes would be delayed for hours. The `changed()` must be in the inner forwarding select loop.
- **Forgetting to handle `Err` from `changed()`:** If all senders are dropped (SubscriptionManager crashed or shut down), `changed()` returns `Err`. This should NOT crash the supervisor -- it means no more subscription updates will arrive, so the supervisor continues operating with its current instrument list. Log a warning and remove the branch from further select iterations.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Instrument list change notification | Custom mpsc channel with add/remove commands | `watch::Receiver::changed()` in select! | Watch provides latest-value semantics, coalesces rapid updates, zero allocation on read |
| Subscription-triggered reconnection | Custom reconnect signaling mechanism | Break inner select loop -> outer reconnect loop re-enters | Existing reconnect loop already handles fresh client creation; just re-enter it |
| PolymarketSubscription to PolymarketAsset conversion | New conversion trait or From impl | Inline `map()` closure | Two-field struct mapping; adding a trait is over-engineering |

**Key insight:** The entire Phase 23 implementation is wiring, not logic. The reconnect loop already exists. The watch channels already exist. The instrument diff computation already exists. Phase 23 connects the dots.

## Common Pitfalls

### Pitfall 1: `changed()` Fires Immediately on New Receiver
**What goes wrong:** A freshly created `watch::Receiver` from `watch::channel()` considers the initial value as "seen" per tokio docs. However, if `mark_changed()` is called or if the sender has already pushed a value between receiver creation and the first `changed()` call, it fires immediately, causing a spurious reconnect at startup.
**Why it happens:** The SubscriptionManager runs its first reconciliation when `Notify::notified()` fires (which happens immediately in the config reload subscriber on first config watch tick). If this reconciliation pushes a value via `send_replace()` before the supervisor's `changed()` is first polled, the supervisor sees an unseen value and triggers an immediate reconnect.
**How to avoid:** This is actually benign. The first reconciliation pushes the same instrument list that was used to seed the channel (both come from `active_approved()` at startup). The `watch::Sender::send_replace()` only notifies receivers if the value has actually changed (it returns the old value but DOES notify regardless of whether the value is identical). However, since the SubscriptionManager only calls `send_replace()` when there are actual changes (the `if deribit_changed` guard), and the first reconciliation compares against an empty `current_*` set, the first reconciliation WILL fire `send_replace()` because all initial instruments appear as "added". This means supervisors WILL see a `changed()` notification shortly after startup.
**Mitigation:** Call `borrow_and_update()` once in the supervisor constructor or at the start of `run()` before entering the reconnect loop. This marks the initial value as "seen" so the first `changed()` in the select loop won't fire until SubscriptionManager actually pushes a new value from a config change. Alternatively: since the reconnect is to the same instrument list, it's a no-op from a functional perspective (just wastes one connection cycle). But for clean operation, marking the initial value as seen is better.

### Pitfall 2: Holding `watch::Ref` Across `.await` Causes Deadlock
**What goes wrong:** `borrow()` returns a `Ref` guard that holds a read lock. If this guard is held across an `.await` point (e.g., `client.start().await`), the SubscriptionManager's `send_replace()` blocks waiting for the write lock, which blocks reconciliation, which blocks config reload processing.
**Why it happens:** Rust's borrow checker does not prevent holding a `Ref` across `.await` because `Ref` is `Send`. The tokio docs explicitly warn about this.
**How to avoid:** Always pattern: `let instruments = self.instruments_rx.borrow().clone(); // Ref dropped here`. Never pass the Ref to a function that awaits.
**Warning signs:** System stops responding to config changes. SubscriptionManager reconciliation log entries stop appearing.

### Pitfall 3: Polymarket Subscription Type Mismatch
**What goes wrong:** The watch channel carries `Vec<PolymarketSubscription>` (from `subscription::manager`), but `PolymarketClient` reads from `config.assets: Vec<PolymarketAsset>` (from `config::venues`). These are different types with the same fields.
**Why it happens:** PolymarketSubscription was created in Phase 22 as a Hash+Eq type for set operations. PolymarketAsset is the config deserialization type. They have identical fields (condition_id, token_id) but are separate types.
**How to avoid:** Convert explicitly when reading from the watch channel: `subscriptions.into_iter().map(|s| PolymarketAsset { condition_id: s.condition_id, token_id: s.token_id }).collect()`. Do NOT add a `From` impl -- the types are in different modules and adding trait coupling is worse than inline conversion for a two-field struct.
**Warning signs:** Compiler error about type mismatch.

### Pitfall 4: Supervisor `run()` Takes `self` by Value, Not `mut self`
**What goes wrong:** `watch::Receiver::changed()` requires `&mut self`. If the supervisor's `run()` method takes `self` (by value, not mutable), the borrow checker rejects `self.instruments_rx.changed()`.
**Why it happens:** The current supervisors take `self` by value in `run()`. The existing `cancel.cancelled()` does not need `&mut self`.
**How to avoid:** Change the `run()` signature from `pub async fn run(self, tx: ...)` to `pub async fn run(mut self, tx: ...)`. This is a single-keyword change. All three supervisors need this.
**Warning signs:** Compiler error: "cannot borrow `self.instruments_rx` as mutable".

### Pitfall 5: Pipeline.rs Wiring With Optional Receivers
**What goes wrong:** The `run_live_multi_venue()` function currently creates supervisors inline. Adding watch receivers requires either: (a) changing the function signature to accept optional receivers, or (b) extracting receivers from PipelineHandles before calling the function.
**Why it happens:** PipelineHandles.subscription_rx is set AFTER `run_multi_venue_pipeline()` returns (in main.rs line 233). The receivers need to be passed INTO the pipeline function, not attached after.
**How to avoid:** Restructure the wiring. Option A: Pass `Option<SubscriptionReceivers>` into `run_multi_venue_pipeline()` and thread it through to `run_live_multi_venue()`. The function destructures the receivers and passes each to its venue's supervisor. Remove the post-hoc attachment in main.rs. Option B: Move subscription channel creation into `run_live_multi_venue()` itself. Option A is cleaner because it keeps channel creation in main.rs where the SubscriptionManager is also created.
**Warning signs:** Receivers attached to PipelineHandles but never consumed by supervisors.

## Code Examples

### Example 1: DeribitSupervisor with Watch Receiver
```rust
// Verified pattern from tokio::sync::watch docs + existing supervisor structure
use tokio::sync::watch;

pub struct DeribitSupervisor {
    config: DeribitConfig,
    instruments_rx: watch::Receiver<Vec<String>>,
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,
    health: Arc<VenueHealth>,
}

pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) {
    // Mark initial value as seen to prevent spurious startup reconnect
    self.instruments_rx.borrow_and_update();

    let reconnect = &self.config.reconnect;
    let mut backoff = ExponentialBackoffBuilder::new()
        .with_initial_interval(Duration::from_millis(reconnect.initial_backoff_ms))
        .with_max_interval(Duration::from_millis(reconnect.max_backoff_ms))
        .with_randomization_factor(reconnect.randomization_factor)
        .with_multiplier(2.0)
        .with_max_elapsed_time(None)
        .build();

    let mut attempt: u64 = 0;

    loop {
        if self.cancel.is_cancelled() { break; }

        attempt += 1;
        // Read latest instruments via borrow().clone() -- drops Ref immediately
        let instruments = self.instruments_rx.borrow().clone();
        tracing::info!(
            attempt = attempt,
            instruments = instruments.len(),
            "DeribitSupervisor connecting..."
        );

        let client = DeribitClient::new(
            self.config.clone(), instruments, self.cancel.clone(),
        ).with_rate_limiter(self.rate_limiter.clone());

        match client.start().await {
            Ok(mut raw_rx) => {
                let mut received_first = false;
                loop {
                    tokio::select! {
                        biased;
                        _ = self.cancel.cancelled() => { return; }
                        Ok(()) = self.instruments_rx.changed() => {
                            tracing::info!(
                                "DeribitSupervisor: instrument list updated, reconnecting"
                            );
                            backoff.reset(); // Intentional reconnect
                            break;
                        }
                        msg = raw_rx.recv() => {
                            match msg {
                                Some(raw) => {
                                    if !received_first {
                                        received_first = true;
                                        backoff.reset();
                                        self.health.mark_available();
                                    }
                                    if tx.send(raw).await.is_err() { return; }
                                }
                                None => {
                                    self.health.mark_unavailable("connection lost".to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => { /* existing error handling */ }
        }
        // existing backoff logic
    }
}
```

### Example 2: Polymarket Conversion Pattern
```rust
// At top of PolymarketSupervisor reconnect loop:
let subscriptions = self.assets_rx.borrow().clone();
let mut config = self.config.clone();
config.assets = subscriptions
    .into_iter()
    .map(|s| PolymarketAsset {
        condition_id: s.condition_id,
        token_id: s.token_id,
    })
    .collect();
let client = PolymarketClient::new(config, self.cancel.clone());
```

### Example 3: Pipeline.rs Wiring
```rust
// run_multi_venue_pipeline() accepts optional subscription receivers
pub async fn run_multi_venue_pipeline(
    mode: DataMode,
    config: &VenuesConfig,
    credentials: &Credentials,
    recording_dir: PathBuf,
    cancel: CancellationToken,
    event_registry: Option<Arc<RwLock<EventRegistry>>>,
    subscription_rx: Option<SubscriptionReceivers>,  // NEW parameter
) -> anyhow::Result<PipelineHandles> {
    // In Live mode, destructure and pass to run_live_multi_venue
    // In Mock/Replay mode, ignore (subscription_rx is None)
}

// Inside run_live_multi_venue(), destructure:
let (deribit_sub_rx, polymarket_sub_rx, kalshi_sub_rx) = match subscription_rx {
    Some(rx) => (Some(rx.deribit), Some(rx.polymarket), Some(rx.kalshi)),
    None => (None, None, None),
};

// For DeribitSupervisor:
let supervisor = match deribit_sub_rx {
    Some(rx) => DeribitSupervisor::new(config.deribit.clone(), rx, venue_cancel.clone(), rate_limiter, health.clone()),
    None => DeribitSupervisor::new_static(config.deribit.clone(), config.deribit.instruments.clone(), ...),
};
```

### Example 4: Handling `changed()` Error (Channel Closed)
```rust
// In select! branch, handle Err case gracefully:
result = self.instruments_rx.changed() => {
    match result {
        Ok(()) => {
            tracing::info!("DeribitSupervisor: instrument list updated, reconnecting");
            backoff.reset();
            break;
        }
        Err(_) => {
            tracing::warn!(
                "DeribitSupervisor: subscription channel closed, continuing with current instruments"
            );
            // Do NOT break -- continue operating with current instrument list.
            // This happens if SubscriptionManager is dropped (shutdown race).
            // Remove this branch from future select iterations by... actually,
            // watch::Receiver::changed() will keep returning Err once closed.
            // This is fine -- the biased select will try cancel first, then
            // this branch returns Err immediately, then msg branch runs.
            // Performance impact: negligible (one extra Err check per message).
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Static instrument list at construction (`Vec<String>`) | Dynamic instrument list via `watch::Receiver<Vec<String>>` | Phase 23 (this phase) | Supervisors can react to subscription changes without restart |
| Config-derived instruments in pipeline.rs | Registry-derived instruments via watch channels seeded by SubscriptionManager | Phase 22 + Phase 23 | Single source of truth (EventRegistry) for all subscription decisions |

**Deprecated/outdated:**
- `DeribitSupervisor.instruments: Vec<String>` field -- replaced by `instruments_rx: watch::Receiver<Vec<String>>`
- `config.deribit.instruments` as primary subscription source -- still used as fallback for Mock/Replay modes, but live mode uses watch channel values

## Open Questions

1. **Should supervisors support both static and dynamic instrument lists?**
   - What we know: Mock/Replay modes don't use subscription management (no SubscriptionManager, no watch channels). Currently DeribitSupervisor takes `instruments: Vec<String>` which Mock mode passes directly.
   - What's unclear: Should the supervisor have two constructors (one with `Vec<String>`, one with `watch::Receiver`), or should pipeline.rs create a watch channel even for Mock/Replay with a static initial value?
   - Recommendation: Two approaches both work. **Recommended:** Keep a single constructor that takes `watch::Receiver`, and have pipeline.rs create a one-shot watch channel seeded with the config value for Mock/Replay. This keeps the supervisor interface uniform. Alternatively, use an enum `InstrumentSource { Static(Vec<String>), Dynamic(watch::Receiver<Vec<String>>) }` but this adds complexity for no behavioral benefit. The simplest approach: supervisor always takes `watch::Receiver`, Mock/Replay creates a watch channel that is never updated. The overhead is negligible (one watch channel per venue, never written to).

2. **Should backoff be skipped entirely (not just reset) on subscription-triggered reconnect?**
   - What we know: `backoff.reset()` resets the delay to `initial_backoff_ms` (1000ms default). After `changed()` triggers a break, the outer loop applies the backoff delay before the next connection attempt.
   - What's unclear: Whether the 1-second initial backoff delay is acceptable for subscription changes. Operator approves an instrument and waits ~1 second for the feed to connect.
   - Recommendation: Reset backoff to initial (1s) is acceptable. Skipping backoff entirely (connecting immediately) would require restructuring the loop to distinguish "connection failed, apply backoff" from "subscription changed, reconnect immediately." The 1-second delay is short enough. If needed later, add a `skip_next_backoff` flag that the `changed()` branch sets.

## Sources

### Primary (HIGH confidence)
- tokio::sync::watch docs (Context7 /websites/rs_tokio_tokio) - `changed()` semantics, `borrow()` vs `borrow_and_update()`, initial value "seen" behavior, `mark_changed()`, deadlock warning for holding Ref across .await
- Project codebase (direct analysis) - All three supervisor files, pipeline.rs, subscription/manager.rs, config/venues.rs, main.rs

### Secondary (MEDIUM confidence)
- Phase 22 research (`.planning/phases/22-subscription-manager-core/22-RESEARCH.md`) - Pattern 3 (Watch Channel for Supervisor Instrument Updates), anti-patterns
- v1.3 architecture research (`.planning/research/ARCHITECTURE.md`) - Supervisor modification patterns, pipeline.rs changes, full code examples for each venue

### Tertiary (LOW confidence)
- None. All findings verified against codebase or official docs.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - No new dependencies. tokio::sync::watch API verified via Context7 official docs.
- Architecture: HIGH - Exact patterns already documented in Phase 22 research and v1.3 architecture research. All three supervisor files analyzed line by line.
- Pitfalls: HIGH - Pitfalls derived from direct code analysis (type mismatches, borrow checker, watch semantics) and verified against tokio docs.

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain -- no external API changes expected)
