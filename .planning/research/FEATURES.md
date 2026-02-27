# Feature Landscape: v1.3 Live Subscription Management & Tech Debt Cleanup

**Domain:** Dynamic WebSocket feed subscription/unsubscription management for cross-venue prediction market arbitrage system
**Researched:** 2026-02-27
**Confidence:** HIGH (venue API subscription capabilities verified via official docs) / MEDIUM (reconciliation patterns, edge cases)

**Scope note:** This research covers ONLY the new features for v1.3: dynamic feed subscription management and tech debt cleanup. Existing v1.0-v1.2 features (feeds, pricing, spread engines, paper trading, settlement, signal analysis, persistence, alerting, automated event discovery, fuzzy matching, proposal workflow, archive/cleanup) are already built and operational at 34,753 LOC Rust.

**Critical existing architecture context:**
- **Supervisors are fire-and-forget.** Each venue supervisor (`DeribitSupervisor`, `PolymarketSupervisor`, `KalshiSupervisor`) takes a fixed instrument list at construction time and owns the reconnection loop. There is NO command channel into the supervisor -- it cannot receive subscribe/unsubscribe instructions after spawning.
- **Clients subscribe once at startup.** `DeribitClient::start()` sends a single batch `public/subscribe` with all instruments. `PolymarketClient::start()` sends one subscribe message with all `assets_ids`. `KalshiClient::start()` iterates `market_tickers` and sends individual subscribe commands per ticker.
- **Config reload updates EventRegistry but NOT feeds.** The config watch subscriber in `main.rs` calls `registry.refresh()` on TOML file changes. The feed supervisors and clients are unaware that the set of active instruments changed.
- **The pipeline is static.** `run_live_multi_venue()` builds all three venue pipelines at startup with fixed instrument lists cloned from config. No mechanism exists to inject new instruments or remove expired ones without restarting.

---

## Table Stakes

Features the system must have for v1.3 to deliver its stated goal. Without these, the operator must still restart the process when event mappings change.

### TS-1: Command Channel into Supervisors

| Attribute | Detail |
|-----------|--------|
| Why Expected | Supervisors currently have zero communication channel after spawn. Dynamic subscription requires a way to send subscribe/unsubscribe commands to a running supervisor. |
| Complexity | Medium |
| Dependencies | Existing supervisor pattern (all three venues) |

**What it is:** An `mpsc` or `watch` command channel that each supervisor monitors in its `select!` loop alongside the existing forwarding and cancellation branches. Commands would include `Subscribe(Vec<String>)`, `Unsubscribe(Vec<String>)`, and potentially `Reconcile(HashSet<String>)` (full desired-state replacement).

**Why it matters:** This is the architectural prerequisite for every other subscription feature. Without it, nothing downstream can trigger subscription changes.

**Venue-specific considerations:**
- **Deribit:** Supervisor creates fresh `DeribitClient` per reconnect. The command must be buffered so it survives reconnects -- the supervisor must replay the current desired subscription set on each new connection.
- **Polymarket:** Supervisor creates fresh `PolymarketClient` per reconnect. Same buffering requirement.
- **Kalshi:** Supervisor creates fresh `KalshiClient` per reconnect. Same pattern, but Kalshi also supports `update_subscription` with `add_markets`/`delete_markets` actions on a live connection via subscription IDs (sids).

**Design consideration:** The simplest correct approach is to maintain a `desired_instruments: HashSet<String>` inside each supervisor that is the authoritative source of truth. Commands mutate this set. On reconnect, the client subscribes to the full desired set. On a live connection, incremental subscribe/unsubscribe messages are sent.


### TS-2: Dynamic Subscribe for Newly Approved Instruments

| Attribute | Detail |
|-----------|--------|
| Why Expected | When an operator approves a proposed event mapping in `events.toml`, the system must start subscribing to the new venue instruments without restart. This is the primary stated goal of v1.3. |
| Complexity | Medium |
| Dependencies | TS-1 (command channel), existing config reload + EventRegistry refresh |

**What it is:** When `events.toml` changes (operator sets `approved = true` on a candidate mapping), the config reload detects the change, refreshes the EventRegistry, and a new component computes the diff between currently-subscribed instruments and the new desired set. For each venue, newly needed instruments are sent as Subscribe commands to the appropriate supervisor.

**Flow:**
```
events.toml change detected
  -> ConfigReloader fires new AppConfig
  -> EventRegistry refreshes
  -> SubscriptionReconciler computes diff:
       desired = registry.active_approved() instruments per venue
       current = tracked set of currently-subscribed instruments
       to_add = desired - current
       to_remove = current - desired
  -> For each venue: send Subscribe(to_add) and Unsubscribe(to_remove)
```

**Venue API capabilities (verified):**
- **Deribit:** `public/subscribe` adds channels to existing subscriptions on the same connection. Up to 500 channels per subscribe message. Rate limited at ~3.3 req/s sustained. [Deribit docs](https://docs.deribit.com/)
- **Polymarket:** Supports `"operation": "subscribe"` with new `assets_ids` on existing connection. [Polymarket WSS docs](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview)
- **Kalshi:** `subscribe` command adds new market tickers. Also supports `update_subscription` with `"action": "add_markets"` on existing subscription IDs. [Kalshi docs](https://docs.kalshi.com/websockets/websocket-connection)


### TS-3: Dynamic Unsubscribe for Expired/Retired Instruments

| Attribute | Detail |
|-----------|--------|
| Why Expected | When events expire and are archived to `events_archive.toml` (v1.2 archival flow), their instruments should be unsubscribed to stop wasting bandwidth and processing on stale data. |
| Complexity | Medium |
| Dependencies | TS-1 (command channel), TS-2 (reconciliation infrastructure) |

**What it is:** The reverse of TS-2. When the lifecycle manager marks events as Expired/Retired and archives them, the reconciler detects that instruments are no longer needed and sends Unsubscribe commands.

**Venue API capabilities (verified):**
- **Deribit:** `public/unsubscribe` removes specific channels from subscription. Also `public/unsubscribe_all` to clear everything (useful during reconnect for clean slate). [Deribit docs](https://docs.deribit.com/)
- **Polymarket:** Supports `"operation": "unsubscribe"` with `assets_ids` to remove. [Polymarket WSS docs](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview)
- **Kalshi:** `unsubscribe` command using `sids` (subscription IDs), and `update_subscription` with `"action": "delete_markets"` on a specific subscription. [Kalshi docs](https://docs.kalshi.com/getting_started/quick_start_websockets)

**Important nuance:** Unsubscribing does not need to be instant. It is acceptable for there to be a brief period where stale data continues arriving after an instrument is marked expired. The processor already handles unknown instruments gracefully (they just do not match any event in the registry). The downstream impact of delayed unsubscription is wasted bandwidth, not incorrect behavior.


### TS-4: Config-Change-Driven Subscription Reconciliation

| Attribute | Detail |
|-----------|--------|
| Why Expected | This is the orchestration layer that ties TS-1 through TS-3 together. A single reconciliation function compares desired state (EventRegistry active+approved instruments) with current state (what supervisors are actually subscribed to) and issues the minimal set of subscribe/unsubscribe commands. |
| Complexity | Medium-High |
| Dependencies | TS-1, TS-2, TS-3, existing ConfigReloader + EventRegistry |

**What it is:** A `SubscriptionReconciler` component that:
1. Listens to `watch::Receiver<AppConfig>` for config changes (same channel the EventRegistry subscriber uses)
2. Extracts the desired per-venue instrument sets from the new EventRegistry state
3. Diffs against currently-tracked subscription state
4. Sends targeted Subscribe/Unsubscribe commands per venue
5. Updates its tracked state on successful subscription changes

**Why "reconciliation" and not just "diff":** On reconnect, the client subscribes to the full desired set (not just the diff). The reconciler must handle the case where a reconnect happened between config changes, meaning the supervisor already subscribed to the new set. The reconciler's tracked state must be resynchronized on reconnect events. This is the "reconcile" operation: assert the full desired state regardless of what the current state might be.

**Edge cases the reconciler must handle:**
- Config change while a venue is disconnected (supervisor in backoff) -- must queue and apply on reconnect
- Multiple rapid config changes -- must coalesce into a single reconciliation pass
- Partial subscription failures -- Deribit returns only successfully subscribed channels; must track partial state
- Venue connection drops after subscribe command sent but before confirmation -- must re-reconcile on reconnect


### TS-5: Tech Debt Sweep (v1.0-v1.2 Accumulated Items)

| Attribute | Detail |
|-----------|--------|
| Why Expected | 15 accumulated tech debt items (13 from v1.0, 2 from v1.2) have been carried forward for 3 milestones. This milestone explicitly includes a tech debt cleanup. |
| Complexity | Low-Medium (individually simple, but there are 15 items) |
| Dependencies | None (can be done independently of subscription features) |

**The full tech debt inventory:**

**From v1.0 (13 items):**
1. `RecordLine.channel` set to empty String for all recorded messages (info)
2. Gamma omitted from Greeks calculator (accepted user decision -- leave as-is)
3. `pricing_brent_fallbacks_total` Prometheus counter specified but not implemented (info)
4. `iv_spread` field always 0.0 in `ArbSignal` (warning -- metadata incomplete)
5. `options book_depth_levels` hardcoded to 0 (info)
6. Replay processor JoinHandle silently dropped in `replay/mod.rs:221` (info)
7. Kalshi `is_stale` always false -- staleness gate not computed from exchange_timestamp (info)
8. 10 stale `REQUIREMENTS.md` checkboxes marked `[ ] Pending` for satisfied requirements (medium)
9. Expired instrument `BTC-27JUN25-100000-C` in events.toml (medium)
10. Kalshi `market_tickers = []` -- no markets in default config (medium)
11. `[health]` and `[signal_generation]` sections absent from config.toml (low)
12. Mock mode lacks event_id annotation (accepted limitation)
13. Polymarket condition_id not used at runtime (info)

**From v1.2 (2 items):**
14. Old exact-match functions (`find_cross_venue_candidates`, `filter_new_candidates`) preserved but unused (low)
15. `expiry_confidence` TOML field is write-only -- not round-tripped via EventMapping struct (low)

**Recommendation for which to fix:**
- **Fix:** Items 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14 (straightforward, meaningful improvements)
- **Leave as-is:** Items 2 (user decision), 12 (accepted), 13 (informational, no harm), 15 (write-only is acceptable for human-readable annotation)
- **Total fixable:** 11 items, mostly trivial individual changes

---

## Differentiators

Features that go beyond the stated v1.3 goals but would add significant operational value. Not expected for the milestone but worth documenting.

### DIFF-1: Subscription State Observability

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Prometheus gauges showing per-venue active subscription count, subscribe/unsubscribe event counters, and reconciliation timing. Gives operator visibility into dynamic subscription behavior. |
| Complexity | Low |
| Dependencies | TS-4 (reconciler) |

**What it is:** Metrics like `feed_subscriptions_active{venue="deribit"}`, `feed_subscription_changes_total{venue="deribit", action="subscribe"}`, `feed_reconciliation_duration_seconds`. These are cheap to implement (a few `metrics::counter!` / `metrics::gauge!` calls) and provide essential operational visibility.

**Recommendation:** Include. The system already has comprehensive Prometheus metrics across all components. Adding subscription metrics is consistent with the existing observability philosophy and costs almost nothing.


### DIFF-2: Subscription Health Validation

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Periodic check that expected instruments are actually producing data. Detects silent subscription failures where the subscribe command succeeded but the venue stopped sending data for that instrument. |
| Complexity | Medium |
| Dependencies | TS-4, existing VenueHealth infrastructure |

**What it is:** A watchdog that checks each subscribed instrument against the last-seen timestamp in the snapshot pipeline. If an instrument that should be producing data has not been seen for N seconds, emit a warning. This catches cases where:
- The venue accepted the subscribe but quietly dropped it
- The instrument became inactive (no trading activity)
- A partial subscription failure was not detected

**Recommendation:** Defer to future milestone. The existing staleness detection (`is_stale` on MarketSnapshot) and alerting (`AlertMonitor` with feed silence detection) already cover most of this. The marginal value is low for a paper trading system.


### DIFF-3: Graceful Subscription Transition (Overlap Period)

| Attribute | Detail |
|-----------|--------|
| Value Proposition | When rolling from an expiring instrument to its replacement (e.g., BTC-27JUN25 -> BTC-26SEP25), subscribe to the new instrument before unsubscribing the old one. Ensures continuous coverage during the transition window. |
| Complexity | Medium-High |
| Dependencies | TS-2, TS-3, lifecycle status awareness |

**What it is:** Instead of unsubscribing expired instruments immediately, maintain a brief overlap period where both old and new instruments are subscribed. This is relevant for the Deribit expiry roll scenario where the lifecycle manager detects a roll candidate.

**Recommendation:** Defer. The current lifecycle manager already handles `Expiring` -> `Expired` -> `Retired` state transitions with configurable thresholds. The overlap period would add complexity without meaningful benefit for paper trading. Subscriptions bandwidth is not a bottleneck at the current scale (dozens of instruments, not thousands).


### DIFF-4: Dry-Run Reconciliation Mode

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Log what subscribe/unsubscribe actions WOULD be taken without actually sending them. Useful for validating reconciliation logic before enabling it. |
| Complexity | Low |
| Dependencies | TS-4 |

**Recommendation:** Include as a config flag. Trivial to implement (wrap the send calls in an `if !dry_run` check and log the actions regardless). Provides a safety net during initial deployment. Can be removed once the feature is validated.

---

## Anti-Features

Features to explicitly NOT build in v1.3.

### AF-1: Per-Instrument Connection Isolation

| Anti-Feature | One WebSocket connection per instrument per venue |
|--------------|--------------------------------------------------|
| Why Avoid | Deribit can handle 500 channels on a single connection. Kalshi supports multiple market subscriptions per connection. One-connection-per-instrument would create hundreds of connections, violating rate limits and consuming excessive resources. |
| What to Do Instead | Use the existing single-connection-per-venue architecture. Add/remove instruments on the existing connection. |


### AF-2: Full Pipeline Restart on Subscription Change

| Anti-Feature | Tear down and rebuild the entire venue pipeline (supervisor + processor + forwarder) when instruments change |
|--------------|---------------------------------------------|
| Why Avoid | This is the current workaround (restart the process). It is disruptive, loses in-flight state, and defeats the purpose of v1.3. The whole point is to add/remove subscriptions without disrupting the existing pipeline. |
| What to Do Instead | Send incremental subscribe/unsubscribe commands on the existing WebSocket connection within the running supervisor. |


### AF-3: Automatic Approval of Subscription Changes

| Anti-Feature | System automatically subscribes to newly discovered (unapproved) instrument candidates |
|--------------|----------------------------------------------------------------------------------------|
| Why Avoid | Violates the `approved = false` safety gate that is a "non-negotiable safety mechanism" (per PROJECT.md Key Decisions). Subscribing to unapproved instruments would generate signals on unvalidated cross-venue mappings, potentially leading to false arbitrage signals. |
| What to Do Instead | Only subscribe to instruments from `active_approved()` event mappings. The operator must explicitly set `approved = true` in events.toml before the system will subscribe. |


### AF-4: WebSocket Multiplexing / Connection Pooling

| Anti-Feature | Generic WebSocket connection pool with dynamic routing |
|--------------|--------------------------------------------------------|
| Why Avoid | Over-engineered for the current scale. Three venues with one connection each is perfectly adequate. Connection pooling adds complexity (connection affinity, subscription routing, failover) with zero benefit at dozens-of-instruments scale. |
| What to Do Instead | Keep the existing one-supervisor-per-venue architecture. |


### AF-5: Bidirectional Subscription Sync (Venue -> System)

| Anti-Feature | Query the venue to discover what we are currently subscribed to, and sync our internal state from the venue's response |
|--------------|--------------------------------------------------------|
| Why Avoid | Only Kalshi supports `list_subscriptions`. Deribit and Polymarket have no equivalent. Building this for one venue creates an inconsistent abstraction. The system's own tracked state should be authoritative. |
| What to Do Instead | Track desired and confirmed subscription state internally. On reconnect, subscribe to the full desired set (clean slate). |

---

## Feature Dependencies

```
TS-1: Command Channel into Supervisors
  |
  +-- TS-2: Dynamic Subscribe (needs command channel to send subscribe commands)
  |     |
  |     +-- TS-4: Reconciliation (needs subscribe capability)
  |
  +-- TS-3: Dynamic Unsubscribe (needs command channel to send unsubscribe commands)
        |
        +-- TS-4: Reconciliation (needs unsubscribe capability)

TS-4: Reconciliation
  |
  +-- DIFF-1: Subscription Observability (add metrics to reconciler)
  |
  +-- DIFF-4: Dry-Run Mode (add config flag to reconciler)

TS-5: Tech Debt Sweep (independent -- no dependencies on subscription features)
```

**Critical path:** TS-1 -> TS-2 + TS-3 -> TS-4 -> DIFF-1 + DIFF-4

**Parallel work:** TS-5 can be done at any point, including before or alongside the subscription features.

---

## MVP Recommendation

**Prioritize (must ship for v1.3):**
1. **TS-1:** Command channel into supervisors -- architectural prerequisite
2. **TS-2:** Dynamic subscribe for newly approved instruments -- primary user-facing feature
3. **TS-3:** Dynamic unsubscribe for expired instruments -- completes the lifecycle
4. **TS-4:** Config-change-driven reconciliation -- ties it all together
5. **TS-5:** Tech debt sweep -- explicit milestone goal, 11 fixable items
6. **DIFF-1:** Subscription observability metrics -- cheap, high operational value
7. **DIFF-4:** Dry-run reconciliation mode -- cheap safety net

**Defer:**
- **DIFF-2:** Subscription health validation -- existing alerting covers most cases
- **DIFF-3:** Graceful subscription transition with overlap -- paper trading does not need this level of continuity

---

## Venue API Subscription Capabilities Summary

| Capability | Deribit | Polymarket | Kalshi |
|-----------|---------|------------|--------|
| Subscribe on existing connection | `public/subscribe` with channels array | `subscribe` operation with `assets_ids` | `subscribe` cmd with `channels` + `market_ticker` |
| Unsubscribe on existing connection | `public/unsubscribe` with channels array | `unsubscribe` operation with `assets_ids` | `unsubscribe` cmd with `sids` |
| Unsubscribe all | `public/unsubscribe_all` (no params) | Not documented | Not documented |
| Update existing subscription | N/A (subscribe is additive) | N/A | `update_subscription` with `add_markets`/`delete_markets` |
| List current subscriptions | Not available | Not available | `list_subscriptions` |
| Max channels per message | 500 | Not documented | Not documented |
| Subscribe rate limit | ~3.3 req/s sustained | Not documented | Not documented |
| Subscription ID tracking | Not used | Not used | Required (`sids` for unsubscribe) |

**Key architectural implication:** Kalshi's subscription ID (`sid`) model requires the system to track subscription IDs returned from subscribe responses. Deribit and Polymarket are channel-name-based (subscribe/unsubscribe by channel name). This means the supervisor command channel abstraction must be venue-aware, or each supervisor must handle the venue-specific protocol internally (recommended: keep venue specifics inside each supervisor, expose a uniform command interface).

**Confidence levels:**
- Deribit subscribe/unsubscribe: HIGH (verified via official docs at docs.deribit.com)
- Polymarket subscribe/unsubscribe: MEDIUM (verified via WSS overview docs; unsubscribe documented but less commonly used in community examples)
- Kalshi subscribe/unsubscribe/update: MEDIUM (verified via quick start docs; update_subscription with sids verified via search results but exact response format not confirmed from official docs)

---

## Sources

- [Deribit API Documentation](https://docs.deribit.com/) -- subscribe/unsubscribe methods, 500-channel limit, rate limits
- [Deribit Market Data Collection Best Practices](https://docs.deribit.com/articles/market-data-collection-best-practices) -- batch subscription, instrument.state lifecycle feed
- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- subscribe/unsubscribe operations, dynamic modification
- [Kalshi WebSocket Connection](https://docs.kalshi.com/websockets/websocket-connection) -- subscribe/unsubscribe/update_subscription commands
- [Kalshi Quick Start WebSockets](https://docs.kalshi.com/getting_started/quick_start_websockets) -- message formats, subscription IDs
- [Tokio Channels Tutorial](https://tokio.rs/tokio/tutorial/channels) -- command channel pattern for async task management
