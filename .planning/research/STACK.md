# Stack Research: v1.3 Live Subscription Management

**Domain:** Dynamic WebSocket subscription/unsubscription, config-driven feed reconciliation, tech debt cleanup
**Researched:** 2026-02-27
**Confidence:** HIGH

## Scope

This document covers ONLY the stack additions and changes needed for v1.3 Live Subscription Management and tech debt cleanup. The existing v1.0-v1.2 stack is validated and unchanged. See prior STACK.md for those decisions.

## Executive Finding: Zero New Dependencies Required

v1.3 requires **no new crate dependencies**. Every capability needed for dynamic subscription management is already present in the existing dependency tree. The work is purely architectural -- restructuring how existing components (supervisors, clients, config watcher) interact.

This continues the project's pattern of minimal dependency growth: v1.1 added zero new crates, v1.2 added one (strsim, already transitively compiled). v1.3 adds zero.

---

## Existing Stack Covering v1.3 Needs

### Core Infrastructure (unchanged)

| Technology | Version | v1.3 Usage | Why Sufficient |
|------------|---------|------------|----------------|
| tokio | 1.x (full) | mpsc channels for subscription commands, watch channels for config changes | Already the async runtime; subscription commands are just another channel message type |
| tokio-tungstenite | 0.28 | Dynamic subscribe/unsubscribe messages on live WebSocket connections | Already handles WS read/write split; sending additional subscribe/unsubscribe frames is the same `write.send()` path |
| futures-util | 0.3 | SinkExt for write half of WebSocket | Already used for `write.send()` in all three venue clients |
| tokio-util | 0.7 | CancellationToken for per-venue lifecycle | Already used for shutdown signaling; child tokens enable per-subscription cancellation |
| notify + notify-debouncer-mini | 8 / 0.7 | File watcher triggers config reload -> subscription reconciliation | Already watching config directory; config changes already propagate via `watch::channel` |
| serde + serde_json | 1.0 | Subscribe/unsubscribe message construction | Already used for JSON-RPC (Deribit) and JSON (Polymarket, Kalshi) message building |
| toml + toml_edit | 0.8 / 0.22 | Config reload parsing, events.toml reading | Already integrated in ConfigReloader and lifecycle manager |
| tracing | 0.1 | Subscription change logging | Already structured logging throughout |
| metrics | 0.24 | Subscription count gauges, reconciliation metrics | Already the metrics facade; just new metric names |
| backoff | 0.4 | Reconnection with updated subscription lists | Already used in all three supervisors |

### Why No New Dependencies

| Capability Needed | How Existing Stack Handles It |
|-------------------|------------------------------|
| Send subscribe command to live WS | `write.send(Message::text(json))` -- already done at connection startup in all 3 clients |
| Send unsubscribe command to live WS | Same `write.send()` path with unsubscribe payload -- no new API needed |
| Receive subscription commands from outside | `tokio::sync::mpsc::Receiver<SubscriptionCommand>` -- standard bounded channel, already the primary IPC mechanism |
| Diff old vs new subscription sets | `HashSet::difference()` / `HashSet::symmetric_difference()` -- stdlib, no crate needed |
| Config change notification | `tokio::sync::watch::Receiver<AppConfig>` -- already distributed to all config consumers |
| Per-subscription cleanup | `HashMap` of active subscriptions with instrument IDs as keys -- stdlib |
| Atomic TOML reads during reconciliation | `std::fs::read_to_string` + `toml::from_str` -- already the config loading path |

---

## Venue-Specific Subscription API Details

### Deribit: Full Dynamic Subscribe/Unsubscribe Support

**Confidence:** HIGH (verified against official docs)

Deribit natively supports dynamic subscription changes on a live WebSocket connection.

**Subscribe (already implemented):**
```json
{
    "jsonrpc": "2.0",
    "id": 42,
    "method": "public/subscribe",
    "params": { "channels": ["book.BTC-27JUN25-100000-C.none.20.100ms", "ticker.BTC-27JUN25-100000-C.raw"] }
}
```

**Unsubscribe (new for v1.3):**
```json
{
    "jsonrpc": "2.0",
    "id": 43,
    "method": "public/unsubscribe",
    "params": { "channels": ["book.BTC-27JUN25-100000-C.none.20.100ms", "ticker.BTC-27JUN25-100000-C.raw"] }
}
```

**Key details:**
- Both methods accept batch channel lists (up to 500 channels per request)
- Unsubscribe response contains only the channels that were successfully removed
- Can be sent at any time on an active connection without reconnection
- Rate limited under the same 20 req/s private endpoint limit (public subscribe/unsubscribe is public but still counted)
- The `build_subscription_channels()` function in `channels.rs` already generates the 3 channels per instrument + price index; it can be reused for both subscribe and unsubscribe

**Integration point:** The `DeribitClient` currently sends subscribe once during `start()` then enters a read-only loop. For v1.3, the write half of the WebSocket must remain accessible to the supervisor/controller so it can send additional subscribe/unsubscribe messages.

**Architecture change needed:** The spawned task in `DeribitClient::start()` currently owns both read and write halves. The write half must be extractable (returned alongside the `mpsc::Receiver<RawMessage>`, or held behind an `Arc<Mutex<SplitSink>>`, or commands routed through a channel that the task reads). The channel approach is cleanest because it avoids locking the write half.

### Polymarket: Subscribe Supported, Unsubscribe NOT Supported

**Confidence:** HIGH (verified against official WSS docs)

Polymarket supports dynamic subscription additions but **does NOT support unsubscription**.

**Subscribe (additional assets on live connection):**
```json
{
    "assets_ids": ["new_token_id_1", "new_token_id_2"],
    "type": "market"
}
```

**Unsubscribe:** NOT AVAILABLE. Once subscribed to an asset, the only way to stop receiving updates is to close the connection and reconnect with the desired subscription set.

**Implication for v1.3:** For adding new instruments, send another subscribe message on the existing connection. For removing expired instruments, the supervisor must trigger a reconnection cycle with the updated subscription list. This is not a problem -- the existing supervisor already handles reconnection with exponential backoff. The reconnection just needs to use the latest instrument list instead of the static one from startup.

**Integration point:** The `PolymarketClient` currently reads `self.config.assets` at connection time. For v1.3, the supervisor must be able to provide the current asset list to each new client instance, and must be able to trigger subscribe for additions without reconnecting.

### Kalshi: Subscribe and Unsubscribe Both Supported

**Confidence:** MEDIUM (subscribe confirmed from existing code + docs; unsubscribe confirmed from error codes and API reference but exact message format not fully documented in public docs)

Kalshi supports both subscribe and unsubscribe via WebSocket commands.

**Subscribe (per-market, already implemented):**
```json
{
    "id": 1,
    "cmd": "subscribe",
    "params": {
        "channels": ["orderbook_delta"],
        "market_ticker": "KXBTCD-25JUN30-T100000"
    }
}
```

**Unsubscribe (new for v1.3):**
```json
{
    "id": 2,
    "cmd": "unsubscribe",
    "params": {
        "sids": [<subscription_id_from_subscribe_response>]
    }
}
```

**Key details:**
- Subscribe returns a subscription ID (sid) in the "subscribed" response message
- Unsubscribe requires the sid, not the market_ticker
- v1.3 must track the sid returned for each subscription to enable clean unsubscription
- Alternative: Kalshi also allows subscribing to multiple market_tickers in one request via `"market_tickers": [...]`

**Integration point:** The `KalshiClient` currently sends per-ticker subscribe messages during `start()`. For v1.3, the client must (a) capture the sid from subscribe responses, (b) expose a way to send new subscribe/unsubscribe commands on the live connection, and (c) maintain a sid-to-ticker mapping for cleanup.

---

## Architecture Changes Required (No New Crates)

### 1. Subscription Command Channel

Each venue supervisor needs an inbound command channel in addition to its outbound raw message channel.

```rust
/// Commands that can be sent to a venue's subscription manager.
enum SubscriptionCommand {
    /// Subscribe to new instruments (e.g., after config approval)
    Subscribe(Vec<String>),
    /// Unsubscribe from instruments (e.g., after expiry/archival)
    Unsubscribe(Vec<String>),
    /// Replace entire subscription set (reconciliation)
    Reconcile(HashSet<String>),
}
```

**Implementation with existing crates:**
- `tokio::sync::mpsc::channel::<SubscriptionCommand>(32)` -- bounded channel, same pattern used everywhere
- Supervisor's `run()` loop adds a `select!` branch reading from this channel
- Commands are forwarded to the write half of the WebSocket connection

### 2. Write Half Access Pattern

The WebSocket write half must be accessible for dynamic commands. Three patterns are possible with existing crates:

| Pattern | Crate(s) | Pros | Cons |
|---------|----------|------|------|
| **Command channel (recommended)** | tokio::sync::mpsc | Clean separation; write half stays owned by single task; no locking | Extra channel hop for commands |
| Arc<Mutex<SplitSink>> | tokio::sync::Mutex | Direct write access from outside | Lock contention with heartbeat responses; complexity |
| Return write half alongside receiver | None | Simple ownership | Caller must manage write half lifetime; doesn't compose with supervisor pattern |

**Recommendation:** Command channel. The spawned WS loop task already handles writes (heartbeat responses, subscribe). Adding a `select!` branch for inbound commands is the natural extension. Zero new crates. Zero lock contention. Clean task ownership.

### 3. Config-to-Subscription Reconciliation

The config hot-reload path already exists: `ConfigReloader` -> `watch::channel` -> consumer tasks. Currently the consumer only refreshes the `EventRegistry`. For v1.3, the consumer also computes subscription diffs and sends commands.

```rust
// Pseudocode using existing types
let old_instruments = current_subscriptions.clone();
let new_instruments = compute_desired_subscriptions(&new_config.events, &new_config.venues);
let to_add: Vec<_> = new_instruments.difference(&old_instruments).collect();
let to_remove: Vec<_> = old_instruments.difference(&new_instruments).collect();

if !to_add.is_empty() {
    deribit_cmd_tx.send(SubscriptionCommand::Subscribe(to_add)).await;
    polymarket_cmd_tx.send(SubscriptionCommand::Subscribe(to_add)).await;
    kalshi_cmd_tx.send(SubscriptionCommand::Subscribe(to_add)).await;
}
if !to_remove.is_empty() {
    deribit_cmd_tx.send(SubscriptionCommand::Unsubscribe(to_remove)).await;
    // Polymarket: reconnect with new set (no unsubscribe API)
    polymarket_cmd_tx.send(SubscriptionCommand::Reconcile(new_instruments)).await;
    kalshi_cmd_tx.send(SubscriptionCommand::Unsubscribe(to_remove)).await;
}
```

**All types used above** (`HashSet`, `Vec`, `mpsc::Sender`) are stdlib or tokio -- zero new crates.

### 4. Supervisor Changes

Current supervisors take `instruments: Vec<String>` at construction and use them immutably for every reconnection. For v1.3:

- Supervisor holds `Arc<RwLock<Vec<String>>>` (or `watch::Receiver<Vec<String>>`) for the current instrument list
- On reconnection, reads the latest list (not the original static list)
- Between reconnections, forwards subscription commands to the active client's command channel
- When the client disconnects, the supervisor automatically subscribes to the full current set on the next connection

**No new crates.** `Arc<RwLock<T>>` is `tokio::sync::RwLock`, already used for `EventRegistry`. `watch::Receiver` is already used for config distribution.

---

## Tech Debt Cleanup: No New Dependencies

The 15 tech debt items from v1.0-v1.2 are all code-level fixes that require no new crates:

| Tech Debt Item | Fix Type | Dependencies Needed |
|----------------|----------|-------------------|
| iv_spread always 0.0 in signal engine | Code fix: populate from solver metadata | None |
| Expired test instrument in events.toml | Config cleanup: remove or archive | None |
| Empty Kalshi default config | Config fix: add sensible defaults | None |
| Options book depth hardcoded to 0 | Code fix: read from config | None |
| Unused exact-match functions (backward compat) | Code cleanup: remove dead code | None |
| expiry_confidence TOML field write-only | Code fix: add to EventMapping deserialization or remove from writes | None |
| Stale REQUIREMENTS.md checkboxes | Doc cleanup | None |
| Other v1.0 accumulated items (9 total) | Various code/config fixes | None |

---

## Cargo.toml Changes

**Zero lines added.** No changes to Cargo.toml for v1.3.

The existing dependency tree is fully sufficient:

```toml
# ALL OF THESE ALREADY EXIST -- no additions needed for v1.3

# Subscription command channels
tokio = { version = "1", features = ["full"] }  # mpsc, watch, RwLock, select!

# WebSocket write (subscribe/unsubscribe messages)
tokio-tungstenite = { version = "0.28", features = ["native-tls"] }
futures-util = { version = "0.3", default-features = false, features = ["sink"] }

# JSON message construction
serde_json = "1.0"

# Config hot-reload (triggers reconciliation)
notify = "8"
notify-debouncer-mini = "0.7"

# Metrics for subscription tracking
metrics = "0.24"

# Reconnection on Polymarket (no unsubscribe API)
backoff = { version = "0.4", features = ["tokio"] }
```

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| Any WebSocket management crate (ws-tool, ezsockets) | tokio-tungstenite is already integrated with proper split read/write pattern | Existing tokio-tungstenite 0.28 |
| Channel abstraction crate (crossbeam-channel, flume) | tokio::sync::mpsc is the project standard for bounded async channels | Existing tokio mpsc channels |
| State machine crate (statig, machine) | Subscription state is simple (active set + pending adds/removes); enum + match is sufficient | Rust enum + match |
| Actor framework (actix, xactor, ractor) | Supervisors are already actor-like (own their state, receive messages via channels, run in spawned tasks). Adding a framework adds API surface without simplifying the code. | Existing supervisor pattern |
| Any config-watching crate beyond notify | notify + debouncer is already integrated and working | Existing notify 8 |
| tower / tower-http | Server-side middleware. We are a client consuming venue APIs. | Direct reqwest + WS calls |
| dashmap | No concurrent map needed. Subscription state is owned by a single task per venue. | HashMap (stdlib) |
| parking_lot | No contention on subscription state. Each supervisor owns its state exclusively. tokio Mutex/RwLock sufficient for rare cross-task access. | tokio::sync::RwLock (existing) |

---

## Version Compatibility Verification

All existing crates remain at their current versions. No upgrades needed.

| Crate | Current Version | Rust 2024 Edition | Status |
|-------|----------------|-------------------|--------|
| tokio | 1.x | Compatible | Unchanged |
| tokio-tungstenite | 0.28 | Compatible | Unchanged |
| futures-util | 0.3 | Compatible | Unchanged |
| tokio-util | 0.7 | Compatible | Unchanged |
| serde_json | 1.0 | Compatible | Unchanged |
| notify | 8 | Compatible | Unchanged |
| notify-debouncer-mini | 0.7 | Compatible | Unchanged |
| metrics | 0.24 | Compatible | Unchanged |
| backoff | 0.4 | Compatible | Unchanged |

**Rust compiler:** 1.85+ (2024 edition) -- no issues with any existing dependency.

---

## Integration Points Summary

### Current Data Flow (v1.2)

```
ConfigReloader --watch::channel--> EventRegistry refresh (read-only)
                                   (no feed-side effect)

Pipeline startup:
  config.venues.deribit.instruments --> DeribitSupervisor (static Vec<String>)
  config.venues.polymarket.assets   --> PolymarketSupervisor (static config)
  config.venues.kalshi.market_tickers -> KalshiSupervisor (static Vec<String>)
```

### Target Data Flow (v1.3)

```
ConfigReloader --watch::channel--> SubscriptionReconciler
                                      |
                              compute diff (HashSet difference)
                                      |
                    +--Subscribe/Unsubscribe commands--+
                    |                 |                 |
              DeribitSupervisor  PolySupervisor  KalshiSupervisor
                    |                 |                 |
              WS write half     WS write half     WS write half
              (subscribe/       (subscribe only;  (subscribe/
               unsubscribe)      reconnect for     unsubscribe via
                                 removal)          sids)
```

### Files That Change (Architecture, Not Dependencies)

| File | Change |
|------|--------|
| `src/feed/deribit/client.rs` | Add command channel; WS loop reads commands + messages |
| `src/feed/deribit/supervisor.rs` | Accept command channel; forward to active client; use latest instruments on reconnect |
| `src/feed/polymarket/client.rs` | Add subscribe command support; supervisor triggers reconnect for removals |
| `src/feed/polymarket/supervisor.rs` | Accept command channel; dynamic instrument list |
| `src/feed/kalshi/client.rs` | Add command channel; track subscription IDs (sids) |
| `src/feed/kalshi/supervisor.rs` | Accept command channel; forward to active client |
| `src/feed/pipeline.rs` | Wire command channels into pipeline assembly; expose command senders |
| `src/main.rs` | Create reconciliation task; connect config watch to subscription commands |
| Various tech debt files | Code fixes (15 items, no dependency changes) |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Subscription commands | mpsc channel per venue | Shared broadcast channel | mpsc is point-to-point (one controller, one supervisor). broadcast adds unnecessary cloning and back-pressure complexity. |
| Write half access | Command channel pattern | Arc<Mutex<SplitSink>> | Mutex adds lock contention between heartbeat responses and subscription commands. Channel pattern keeps single-owner semantics. |
| Instrument list sharing | watch::Receiver<Vec<String>> per venue | Arc<RwLock<Vec<String>>> | watch provides change notification for free (`.changed().await`). RwLock requires polling or separate notification channel. |
| Polymarket unsubscribe | Reconnect with new list | Ignore stale data and filter client-side | Reconnection is cleaner. Ignoring data wastes bandwidth and processing. Supervisor already handles reconnection gracefully. |
| Config diff computation | HashSet::difference (stdlib) | Custom diff algorithm | stdlib HashSet operations are O(n) and correct. No reason to add complexity. |
| Subscription state tracking | HashMap<String, SubscriptionState> per venue | External state store (Redis, SQLite) | State is small (dozens of instruments), ephemeral (rebuilt on restart), and per-process. External store adds latency and failure modes. |

---

## Sources

- [Deribit API Documentation](https://docs.deribit.com/) -- public/subscribe and public/unsubscribe methods, batch channel lists (HIGH confidence)
- [Deribit Connection Management Best Practices](https://support.deribit.com/hc/en-us/articles/25944603459613-Connection-Management-Best-Practices) -- Dynamic subscription, connection_too_slow warnings (HIGH confidence)
- [Polymarket CLOB WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- Subscribe supported, unsubscribe NOT supported, modify subscriptions note (HIGH confidence)
- [Kalshi WebSocket Quick Start](https://docs.kalshi.com/getting_started/quick_start_websockets) -- Subscribe command format, subscription IDs (MEDIUM confidence)
- [Kalshi WebSocket Connection](https://docs.kalshi.com/websockets/websocket-connection) -- Unsubscribe requires sids, error codes for unknown sid (MEDIUM confidence)
- [tokio-tungstenite on crates.io](https://crates.io/crates/tokio-tungstenite) -- v0.28, SplitSink write access (HIGH confidence)
- [tokio::sync module](https://docs.rs/tokio/latest/tokio/sync/index.html) -- mpsc, watch, RwLock, Mutex (HIGH confidence)
- Existing codebase analysis: `src/feed/*/client.rs`, `src/feed/*/supervisor.rs`, `src/feed/pipeline.rs`, `src/config/reload.rs`, `src/main.rs` -- Current architecture confirmed by direct code reading (HIGH confidence)

---
*Stack research for: v1.3 Live Subscription Management*
*Researched: 2026-02-27*
