# Project Research Summary

**Project:** Prediction Market Arbitrage System
**Domain:** Dynamic WebSocket subscription management for cross-venue arbitrage
**Researched:** 2026-02-27
**Confidence:** HIGH

## Executive Summary

v1.3 requires zero new crate dependencies. The entire milestone is an architectural change: bridging the existing config hot-reload path to the existing feed supervisors via a new SubscriptionManager component that uses `tokio::sync::watch` channels to push updated instrument lists. Each supervisor adds a single `changed()` branch to its `select!` loop and reconnects with the new list. The reconnect-based approach keeps all three venues uniform and leverages battle-tested reconnection logic.

The recommended architecture introduces one new component (`SubscriptionManager` in `events/subscription.rs`, ~200 LOC) and minor modifications to three supervisors (~30 LOC each), pipeline wiring (~40 LOC), and main.rs (~50 LOC). Total estimated delta: ~590 LOC including tests. The critical ordering constraint -- registry must refresh before subscription diff -- is solved with `tokio::sync::Notify`.

The primary risk is stale internal state after unsubscription: SpreadEngine, DeribitProcessor, and KalshiProcessor all maintain per-instrument HashMaps that grow monotonically. Unsubscribing without cleanup produces phantom spread signals from stale data paired with live data. This must be addressed in the same phase as unsubscribe implementation.

## Key Findings

### Recommended Stack

**Zero additions to Cargo.toml.** All capabilities are covered by existing dependencies: tokio (mpsc, watch, select!), tokio-tungstenite (WS write half), serde_json (message construction), notify (config watching), metrics (subscription gauges). This continues the project's pattern of minimal dependency growth (v1.1: 0 new, v1.2: 1 new, v1.3: 0 new).

**Core technologies (all existing):**
- `tokio::sync::watch` -- push latest instrument list to supervisors (latest-value semantics, no queue buildup)
- `tokio::sync::Notify` -- coordinate registry refresh before subscription diff
- `HashSet::difference()` (stdlib) -- compute per-venue instrument diffs

### Expected Features

**Must have (table stakes):**
- TS-1: Command channel into supervisors (architectural prerequisite)
- TS-2: Dynamic subscribe for newly approved instruments
- TS-3: Dynamic unsubscribe for expired/retired instruments
- TS-4: Config-change-driven subscription reconciliation
- TS-5: Tech debt sweep (15 items, 11 fixable)

**Should have (cheap, high value):**
- DIFF-1: Subscription observability metrics (Prometheus gauges/counters)
- DIFF-4: Dry-run reconciliation mode (config flag, safety net for deployment)

**Defer (v2+):**
- DIFF-2: Subscription health validation (existing alerting covers most cases)
- DIFF-3: Graceful subscription transition with overlap period

### Architecture Approach

A single new `SubscriptionManager` tokio task watches for registry refresh notifications, reads `EventRegistry::active_approved()`, computes per-venue instrument diffs, and pushes updated lists via `watch::Sender` to each supervisor. Supervisors detect changes via `watch::Receiver::changed()` in their inner `select!` loop, break to reconnect, and create a fresh client with the updated instrument list. The hot path (feeds -> SpreadEngine -> SignalEngine) is never blocked or disrupted.

**Major components:**
1. `SubscriptionManager` (NEW) -- bridges config changes to supervisor instrument lists
2. `DeribitSupervisor` / `PolymarketSupervisor` / `KalshiSupervisor` (MODIFIED) -- accept watch receivers, reconnect on change
3. `pipeline.rs` (MODIFIED) -- accept and pass subscription channels to supervisors

**Venue-specific subscription semantics:**

| Venue | Subscribe | Unsubscribe | v1.3 Approach |
|-------|-----------|-------------|---------------|
| Deribit | `public/subscribe` batch (up to 500 channels) | `public/unsubscribe` batch | Reconnect-based (uniform with other venues) |
| Polymarket | Subscribe message with `assets_ids` | Unsubscribe support ambiguous | Reconnect-based (safe fallback) |
| Kalshi | Per-ticker `subscribe` cmd | Requires `sids` (not currently tracked) | Reconnect-based (avoids sid complexity) |

### Critical Pitfalls

1. **Stale state after unsubscribe** -- SpreadEngine, DeribitProcessor, KalshiProcessor retain per-instrument HashMaps. Must add explicit `cleanup_instrument()` / `cleanup_event()` methods called after unsubscribe. Cannot be deferred.

2. **Race between registry refresh and subscription diff** -- Two independent subscribers to `watch::channel<AppConfig>` have no ordering guarantee. Solve with `tokio::sync::Notify` signal from config subscriber to SubscriptionManager after `registry.refresh()` completes.

3. **Windows file watcher DELETE+RENAME race** -- Atomic TOML writes on Windows produce transient file-not-found. Existing 500ms debounce handles most cases; add retry on ReadFile error as belt-and-suspenders.

4. **Tech debt cleanup breaking working pipeline** -- Separate from feature work in git history. Execute AFTER subscription management is verified. Per-item commits with full test suite between each.

5. **Polymarket unsubscribe reliability unknown** -- Reconnect-based approach sidesteps this entirely.

## Implications for Roadmap

### Phase 22: Subscription Manager Core

**Rationale:** The SubscriptionManager is the architectural prerequisite for all subscription features. Build as read-only observer first (logs diffs but does not push them) to validate the diffing logic independently.
**Delivers:** SubscriptionManager struct, reconcile() logic, Notify-based coordination with config subscriber, structured diff logging
**Addresses:** TS-1 (command channel foundation), TS-4 (reconciliation logic)
**Avoids:** Pitfall 2 (race condition) via Notify ordering

### Phase 23: Dynamic Supervisor Subscriptions

**Rationale:** Supervisors must accept watch channels before end-to-end wiring can work. Modify all three supervisors in parallel (independent code paths).
**Delivers:** watch::Receiver in all three supervisors, `changed()` select branch, reconnect with updated list, pipeline.rs wiring, SubscriptionManager pushes real updates
**Implements:** TS-2 (subscribe), TS-3 (unsubscribe), full end-to-end lifecycle
**Avoids:** Pitfall 6 (supervisor has no command input) via watch channel

### Phase 24: Hardening and Observability

**Rationale:** After core functionality works, add metrics, state cleanup, edge case handling, and integration tests.
**Delivers:** Prometheus subscription metrics (DIFF-1), dry-run mode (DIFF-4), stale state cleanup after unsubscribe, integration tests for full lifecycle
**Avoids:** Pitfall 1 (stale state) via cleanup methods, Pitfall 9 (rate limiter contention) via batch subscribe

### Phase 25: Tech Debt Sweep

**Rationale:** Independent of subscription work. Execute last so regressions are cleanly bisectable. Per-item atomic commits.
**Delivers:** 11 tech debt items fixed (of 15 total; 4 left as-is by design)
**Avoids:** Pitfall 8 (tech debt breaking pipeline) via separate phase and per-item commits

### Phase Ordering Rationale

- Phase 22 before 23: Manager creates the sender side of watch channels; supervisors need receivers
- Phase 23 before 24: Core path must work before hardening makes sense
- Phase 25 independent: Can run after 24 without blocking; separate phase prevents debugging confusion between new features and cleanup
- All phases use zero new dependencies

### Research Flags

Phases with standard patterns (skip research-phase):
- **Phase 22:** Well-documented tokio::sync::watch and Notify patterns
- **Phase 23:** Supervisor modifications are straightforward select! branch additions
- **Phase 24:** Standard Prometheus metrics and cleanup methods
- **Phase 25:** Code-level fixes with no architectural decisions

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new dependencies confirmed; all primitives already in use |
| Features | HIGH | Table stakes clear; dependency chain validated |
| Architecture | HIGH | SubscriptionManager + watch channel pattern verified against codebase |
| Pitfalls | HIGH/MEDIUM | Stale state and race conditions confirmed from code; Polymarket unsubscribe MEDIUM |

**Overall confidence:** HIGH

### Gaps to Address

- Polymarket unsubscribe reliability: reconnect-based approach sidesteps this, but worth empirical testing during Phase 23
- Kalshi subscribe response format (sid): reconnect-based approach avoids needing this, but document actual behavior
- Empty venue instrument list behavior: design decision for Phase 24 (disconnect and idle vs stay connected)

## Sources

### Primary (HIGH confidence)
- Deribit API docs (docs.deribit.com) -- subscribe/unsubscribe, batch channels, rate limits
- tokio::sync module (docs.rs/tokio) -- watch, Notify, mpsc semantics
- Direct codebase analysis -- all 34,753 LOC, exact line references in research files

### Secondary (MEDIUM confidence)
- Polymarket WSS docs -- subscribe confirmed, unsubscribe documented but less verified
- Kalshi WebSocket docs -- subscribe/unsubscribe/update_subscription, sid model

---
*Research completed: 2026-02-27*
*Ready for roadmap: yes*
