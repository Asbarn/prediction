# Architecture Research: Derive.xyz Venue Integration

**Domain:** Adding Derive.xyz as a fourth venue to a cross-venue options arbitrage system
**Researched:** 2026-03-03
**Confidence:** HIGH (based on direct source analysis of 36,507 LOC codebase; MEDIUM for Derive API specifics due to search-only verification of official docs)

## Executive Summary

The v1.5 milestone adds Derive.xyz (formerly Lyra v2) as a fourth venue. Derive is a decentralized CLOB options exchange on an Ethereum L2 with a JSON-RPC WebSocket API structurally similar to Deribit. This is a pure additive integration: zero changes to existing venue supervisors, zero changes to MarketSnapshot schema, zero changes to downstream engines. Every new component mirrors an existing one.

The integration requires six new source files, changes to five existing files, and one new config section. The critical decision points are: how to handle Derive's different instrument naming format (`BTC-20250627-100000-C` vs Deribit's `BTC-27JUN25-100000-C`), how to extend EventVenues and EventRegistry without breaking Kalshi mapping, and how to add a fourth venue to SubscriptionManager without rewiring the existing three-venue architecture.

Because Derive is an options venue -- not a prediction market -- it slots directly into the existing Deribit flow rather than the Polymarket/Kalshi flow. Derive snapshots feed the OptionsEngine (Black-76) and SpreadEngine, not a binary probability pipeline.

## Verified Architecture Facts (Source-Confirmed)

### Existing Venue Supervisor Pattern

Every venue follows identical structure in `src/feed/{venue}/`:
- `supervisor.rs` -- reconnection loop, watches instrument list via `watch::Receiver<Vec<String>>`, exponential backoff
- `client.rs` -- connects to WS, subscribes, forwards raw frames via `mpsc::Sender<RawMessage>`
- `normalize.rs` -- `{Venue}Processor` consumes `RawMessage`, maintains per-instrument state, emits `MarketSnapshot`
- `messages.rs` -- serde deserialization types for venue wire format
- `mod.rs` -- module re-exports

`DeribitSupervisor`, `PolymarketSupervisor`, and `KalshiSupervisor` all share the same structural signature:
```rust
pub struct {Venue}Supervisor {
    config: {Venue}Config,
    instruments_rx: watch::Receiver<Vec<String>>,  // or Polymarket subscription type
    cancel: CancellationToken,
    rate_limiter: VenueRateLimiter,  // optional
    health: Arc<VenueHealth>,
}
impl {Venue}Supervisor {
    pub async fn run(mut self, tx: mpsc::Sender<RawMessage>) { ... }
}
```

`DeriveSupervisor` is a direct copy of `DeribitSupervisor` with the config type changed. No new patterns needed.

### Existing Pipeline Fan-In

`src/feed/pipeline.rs::run_live_multi_venue()` starts each venue in its own block:
1. Create `VenueHealth`, `RecordingService`, `VenueRateLimiter`
2. Create `mpsc::channel::<RawMessage>(1024)` for supervisor-to-processor
3. Create `watch::Receiver` from `SubscriptionReceivers` or config default
4. Spawn supervisor with `run(supervisor_tx)`
5. Create `{Venue}Processor` with `processor_rx, record_tx, cancel, staleness_threshold, cleanup_rx`
6. Spawn processor with `processor.run()`
7. Spawn `forward_snapshots(venue_snapshot_rx, fan_in_tx, Venue::Derive, cancel, health, registry)`

Adding Derive adds one more block of this pattern. Existing blocks are untouched.

### Existing SubscriptionManager

`src/subscription/manager.rs` currently manages three `watch::Sender` channels:
```rust
pub struct SubscriptionManager {
    deribit_tx: watch::Sender<Vec<String>>,
    polymarket_tx: watch::Sender<Vec<PolymarketSubscription>>,
    kalshi_tx: watch::Sender<Vec<String>>,
    current_deribit: HashSet<String>,
    current_polymarket: HashSet<PolymarketSubscription>,
    current_kalshi: HashSet<String>,
    ...
}
```

Adding Derive requires adding `derive_tx` and `current_derive` fields, extending `compute_desired_instruments()` to extract `mapping.venues.derive`, and extending `reconcile()` to push to the Derive channel. The existing three-venue code is unchanged.

### Existing EventRegistry

`src/events/registry.rs::build_indexes()` currently handles:
```rust
if let Some(ref deribit) = mapping.venues.deribit { ... }
if let Some(ref polymarket) = mapping.venues.polymarket { ... }
if let Some(ref kalshi) = mapping.venues.kalshi { ... }
```

Adding Derive adds one more `if let Some(ref derive) = mapping.venues.derive { ... }` block.

### Existing EventVenues Config Type

`src/config/events.rs::EventVenues` is:
```rust
pub struct EventVenues {
    pub deribit: Option<DeribitMapping>,
    pub polymarket: Option<PolymarketMapping>,
    pub kalshi: Option<KalshiMapping>,
}
```

Adding `pub derive: Option<DeriveMapping>` with `#[serde(default)]` is backward-compatible -- all existing TOML files parse correctly, and existing three-venue entries continue to work.

### Existing CleanupEvent

`src/subscription/mod.rs::CleanupEvent` currently has:
```rust
pub struct CleanupEvent {
    pub deribit_instruments: Vec<String>,
    pub kalshi_tickers: Vec<String>,
    pub polymarket_token_ids: Vec<String>,
    pub event_ids: Vec<String>,
}
```

Adding `pub derive_instruments: Vec<String>` is backward-compatible. The `DeribitProcessor` uses `deribit_instruments`; the new `DeriveProcessor` uses `derive_instruments`. No existing processor is affected.

### Existing MarketSnapshot

`src/types/snapshot.rs::MarketSnapshot` already contains all fields required for options data: `bid`, `ask`, `bid_iv`, `ask_iv`, `underlying_price`, `underlying_index`, `mark_price`, `index_price`, `mark_iv`, `greeks`, `depth_bids`, `depth_asks`. These were designed for Deribit and map directly to Derive's data. **Zero schema changes required.**

## Derive.xyz API Architecture (Research-Verified)

### Protocol

- **Wire protocol:** JSON-RPC 2.0 over WebSocket (same family as Deribit)
- **WebSocket URL:** `wss://api.lyra.finance/ws` (production); `wss://api-demo.lyra.finance/ws` (testnet)
- **No authentication required** for public market data subscriptions (Derive is a DEX with public orderbooks)
- **Request format:** `{"id": string, "method": string, "params": object}`
- **Response format:** `{"id": string, "result": object}` or `{"id": string, "error": {"code": int, "message": string}}`

Confidence: HIGH (confirmed by docs.derive.xyz/reference/json-rpc and multiple search results)

### Instrument Naming

Derive uses a different naming convention than Deribit:

| Venue | Format | Example |
|-------|--------|---------|
| Deribit | `{ASSET}-{DDMMMYY}-{STRIKE}-{C\|P}` | `BTC-27JUN25-100000-C` |
| Derive | `{ASSET}-{YYYYMMDD}-{STRIKE}-{C\|P}` | `BTC-20250627-100000-C` |

Both are call options on BTC expiring 2025-06-27 at $100,000 strike. The format difference is only in date representation.

Confidence: MEDIUM (instrument naming confirmed from multiple search results referencing Derive docs and Amberdata integration; specific channel subscription format inferred from Deribit similarity and JSON-RPC pattern)

### Market Data Subscriptions

Derive's subscription API follows the same JSON-RPC subscribe pattern as Deribit. Based on the `public/get_ticker` endpoint and the Derive JSON-RPC overview, the channels are:

**Orderbook subscription** (equivalent to Deribit's `book.{instrument}.none.20.100ms`):
```json
{
  "id": "1",
  "method": "subscribe",
  "params": {
    "channels": ["orderbook.{instrument_name}.{depth}"]
  }
}
```

**Ticker subscription** (equivalent to Deribit's `ticker.{instrument}.raw`):
```json
{
  "id": "2",
  "method": "subscribe",
  "params": {
    "channels": ["ticker.{instrument_name}"]
  }
}
```

Confidence: LOW (inferred from Derive JSON-RPC docs overview and Deribit pattern similarity; exact channel strings require validation against live API at implementation time)

### Discovery REST Endpoint

Derive exposes `POST https://api.lyra.finance/public/get_instruments` returning all active instruments with `instrument_name`, `expiration_timestamp`, `strike`, `option_type` (`"call"`/`"put"`), and `base_currency` fields. The structure mirrors Deribit's discovery endpoint.

Confidence: MEDIUM (confirmed `post_public-get-instrument` endpoint exists; plural `get_instruments` endpoint inferred from pattern)

### Key Differences vs Deribit

| Aspect | Deribit | Derive |
|--------|---------|--------|
| Auth | Public endpoints unauthenticated | Fully unauthenticated for market data |
| Heartbeat | `public/set_heartbeat` + `public/test` response | Standard WebSocket ping/pong (no Deribit-style heartbeat protocol) |
| Book updates | Snapshot + delta (change_id sequencing) | Full snapshot per message (no delta, no sequence gaps) |
| Instrument ID | `BTC-27JUN25-100000-C` | `BTC-20250627-100000-C` |
| Settlement | Weekly Friday 08:00 UTC | Protocol-specific, check docs |
| L2 settlement | N/A (CeFi) | Ethereum (withdrawals involve L1 settlement) |

The absence of Deribit's heartbeat protocol simplifies the `DeriveClient` -- no `set_heartbeat` RPC or `test_request` response handling needed.

Confidence: MEDIUM for heartbeat and book update differences (inferred from Derive architecture documentation describing "complete snapshot" model); requires implementation verification.

## System Overview After Integration

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         Venue Layer (feed supervisors)                    │
├──────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌────────────────┐  ┌─────────────┐  ┌────────────┐  │
│  │ Deribit      │  │ Derive.xyz     │  │ Polymarket  │  │ Kalshi     │  │
│  │ Supervisor   │  │ Supervisor NEW │  │ Supervisor  │  │ Supervisor │  │
│  └──────┬───────┘  └───────┬────────┘  └──────┬──────┘  └─────┬──────┘  │
│         │ RawMessage       │ RawMessage        │               │         │
│  ┌──────▼───────┐  ┌───────▼────────┐  ┌──────▼──────┐  ┌─────▼──────┐  │
│  │ Deribit      │  │ Derive         │  │ Polymarket  │  │ Kalshi     │  │
│  │ Processor    │  │ Processor NEW  │  │ Processor   │  │ Processor  │  │
│  └──────┬───────┘  └───────┬────────┘  └──────┬──────┘  └─────┬──────┘  │
│         │ MarketSnapshot   │                   │               │         │
├─────────┴──────────────────┴───────────────────┴───────────────┴─────────┤
│                         shared mpsc fan-in channel                        │
│                         (MarketSnapshot, all venues)                      │
├──────────────────────────────────────────────────────────────────────────┤
│                         Downstream Engines                                │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ SpreadEngine │  │ OptionsEngine│  │ SignalEngine  │  │ PaperTrade  │  │
│  │ (unchanged)  │  │ (unchanged)  │  │ (unchanged)  │  │ (unchanged) │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘  │
├──────────────────────────────────────────────────────────────────────────┤
│                         Control Plane                                     │
│  ┌──────────────────────┐     ┌──────────────────────────────────────┐    │
│  │ SubscriptionManager  │     │ EventRegistry + ContractLifecycle    │    │
│  │ (+Derive channel NEW)│     │ (+Derive discovery/matching NEW)     │    │
│  └──────────────────────┘     └──────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────────┘
```

## New vs Modified vs Unchanged Components

### New Components (6 files)

| File | Mirrors | Responsibility |
|------|---------|---------------|
| `src/feed/derive/mod.rs` | `src/feed/deribit/mod.rs` | Module re-exports |
| `src/feed/derive/messages.rs` | `src/feed/deribit/messages.rs` | Derive wire format serde types |
| `src/feed/derive/client.rs` | `src/feed/deribit/client.rs` | WS connect, subscribe, forward raw frames |
| `src/feed/derive/supervisor.rs` | `src/feed/deribit/supervisor.rs` | Reconnection loop, watch channel |
| `src/feed/derive/normalize.rs` | `src/feed/deribit/normalize.rs` | DeriveProcessor, build_snapshot |
| `src/feed/derive/book.rs` | `src/feed/deribit/book.rs` | Order book state (if Derive sends incremental; otherwise simpler) |

`DeriveProcessor` is structurally identical to `DeribitProcessor`. It maintains `HashMap<InstrumentId, BookState>` and `HashMap<InstrumentId, TickerState>`, and calls a `build_snapshot()` function that outputs `MarketSnapshot` with `venue: Venue::Derive`. The existing `build_snapshot` in `deribit/normalize.rs` can be extracted to a shared utility or copied with `Venue::Deribit` changed to `Venue::Derive`.

### Modified Components (5 files)

| File | Change | Risk |
|------|--------|------|
| `src/types/venue.rs` | Add `Venue::Derive` variant | LOW -- requires adding `Derive` to all `match` arms; compiler enforces exhaustiveness |
| `src/config/events.rs` | Add `DeriveMapping` struct; add `derive: Option<DeriveMapping>` to `EventVenues` with `#[serde(default)]` | LOW -- additive only; existing TOML parses without `derive` field |
| `src/config/venues.rs` | Add `DeriveConfig` struct; add `derive: DeriveConfig` to `VenuesConfig` | LOW -- requires default for TOML backward compat |
| `src/events/registry.rs` | Add `Venue::Derive` case to `build_indexes()` | LOW -- one new `if let Some` block |
| `src/subscription/manager.rs` | Add `derive_tx`, `current_derive` fields; extend `compute_desired_instruments()` and `reconcile()` | LOW -- mirroring existing Kalshi/Deribit structure |
| `src/subscription/mod.rs` | Add `derive: watch::Sender<Vec<String>>` to `SubscriptionSenders`/`SubscriptionReceivers`; add `derive_instruments: Vec<String>` to `CleanupEvent` | LOW -- additive |
| `src/feed/mod.rs` | Add `pub mod derive;` | Trivial |
| `src/feed/pipeline.rs` | Add Derive pipeline block in `run_live_multi_venue()`; plumb Derive cleanup channel | LOW -- copy Deribit block, change types |
| `src/events/discovery.rs` | Add `discover_derive()` function; add `DiscoveryConfig` fields for Derive | LOW -- same pattern as `discover_deribit()` |

### Modified: Venue Enum (Propagation Points)

Adding `Venue::Derive` to the enum triggers compiler exhaustiveness errors in every `match venue { ... }` expression. These are locatable with `cargo check` -- they are not bugs but required additions. Expected match arm additions:

- `Venue::Display` impl: add `Derive => write!(f, "derive")`
- `Venue::env_prefix()`: add `Derive => "DERIVE"`
- `RecordLine` deserialization: serde handles automatically
- Any `match` in metrics, health, logging: add `Derive` case

This is mechanical but must be done completely. The compiler enforces it.

### Unchanged Components (All downstream engines)

`SpreadEngine`, `OptionsEngine`, `SignalEngine`, `PaperTradeTracker`, `SettlementTracker`, `SpreadLogger`, `SignalLogger`, `AlertManager`, `HealthServer`, `PrometheusExporter` -- all unchanged. They operate on `MarketSnapshot` and `EventId` abstractions. They do not know which venue produced a snapshot.

## Data Flow: Derive MarketSnapshot

```
Derive WebSocket (wss://api.lyra.finance/ws)
    |
    | JSON-RPC subscription notifications
    v
DeriveClient::start()
    |
    | RawMessage (text frame + received_at timestamp)
    v
DeriveSupervisor::run() -- reconnect loop
    |
    | mpsc::Sender<RawMessage>
    v
DeriveProcessor::run()
    |
    +-- parse JSON as DeriveMessage (RPC response / notification)
    |
    +-- route by channel type:
    |       orderbook.{instrument} -> handle_book() -> update BookState
    |       ticker.{instrument}    -> handle_ticker() -> update TickerState
    |
    +-- build_snapshot(instrument, book, ticker, seq, received_at, exchange_ts, staleness_ms)
    |
    | MarketSnapshot { venue: Venue::Derive, instrument_id, bid, ask, depth_bids,
    |                  depth_asks, bid_iv, ask_iv, underlying_price, mark_iv, ... }
    v
fan_in mpsc::Sender<MarketSnapshot>
    |
    | (same channel as Deribit/Polymarket/Kalshi snapshots)
    v
forward_snapshots() -- annotates event_id from EventRegistry
    |
    | MarketSnapshot { event_id: Some("BTC-100K-2025-06-27"), ... }
    v
downstream engines (SpreadEngine, OptionsEngine, ...)
```

The Derive `MarketSnapshot` is indistinguishable from a Deribit `MarketSnapshot` from the perspective of downstream engines. Both have `bid_iv`, `ask_iv`, `underlying_price` populated from their respective ticker channels. The `SpreadEngine` and `OptionsEngine` compare snapshots by `event_id`, not by venue, so they automatically handle Derive-vs-Deribit spread computation once both are mapped to the same event.

## Derive-Specific Schema Mapping

### MarketSnapshot Field Population

| MarketSnapshot field | Deribit source | Derive source |
|----------------------|---------------|---------------|
| `bid`, `ask` | Book channel best bid/ask | Same (orderbook channel) |
| `depth_bids`, `depth_asks` | Book channel top-N levels | Same |
| `bid_iv`, `ask_iv` | Ticker channel `bid_iv`, `ask_iv` | Same (options-only) |
| `mark_price` | Ticker `mark_price` | Ticker `mark_price` |
| `index_price` | Ticker `index_price` | Index price (BTC spot) |
| `underlying_price` | Ticker `underlying_price` | Futures forward price |
| `underlying_index` | Ticker `underlying_index` | Futures instrument name |
| `mark_iv` | Ticker `mark_iv` | Ticker mark IV |
| `greeks` | Ticker greek fields | Ticker greek fields (if provided) |
| `exchange_timestamp` | Book/ticker `timestamp` ms | Same |
| `bid_probability`, `ask_probability` | None (set by OptionsEngine) | None (set by OptionsEngine) |

The `bid_probability` and `ask_probability` fields are computed by `OptionsEngine` using Black-76, not populated by the processor. This is the same flow as Deribit. No special handling needed.

### Instrument ID Translation

Derive instrument names (`BTC-20250627-100000-C`) must map to Deribit-equivalent names (`BTC-27JUN25-100000-C`) in `events.toml`. They are different strings for the same underlying contract. The `EventMapping.venues.derive.instrument` field stores Derive's format; `EventMapping.venues.deribit.instrument` stores Deribit's format. The registry looks up each independently.

No translation is needed at runtime -- each venue's snapshot carries its own `instrument_id` string, and the registry maps both independently to the same `event_id`.

### DeriveMapping Config Type

```rust
/// Derive.xyz instrument mapping for events.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveMapping {
    /// Derive instrument name, e.g., "BTC-20250627-100000-C"
    pub instrument: String,
}
```

This mirrors `DeribitMapping`:
```rust
pub struct DeribitMapping {
    pub instrument: String,
}
```

### DeriveConfig Venues Config Type

```rust
/// Derive.xyz connection settings.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DeriveConfig {
    /// WebSocket URL. Default: wss://api.lyra.finance/ws
    #[serde(default = "default_derive_ws_url")]
    pub ws_url: String,
    /// REST API base URL for discovery. Default: https://api.lyra.finance
    #[serde(default = "default_derive_rest_url")]
    pub rest_url: String,
    /// Staleness threshold in milliseconds.
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// API rate limit per second for discovery REST calls.
    #[serde(default = "default_derive_rate_limit")]
    pub rate_limit_per_second: u32,
    /// Instrument names to subscribe to.
    #[serde(default)]
    pub instruments: Vec<String>,
    /// Order book depth (number of levels to request).
    #[serde(default = "default_book_depth_levels")]
    pub book_depth_levels: u32,
}
```

## EventRegistry Changes

### EventVenues Extension

```rust
pub struct EventVenues {
    pub deribit: Option<DeribitMapping>,
    pub polymarket: Option<PolymarketMapping>,
    pub kalshi: Option<KalshiMapping>,
    #[serde(default)]               // NEW: backward compatible
    pub derive: Option<DeriveMapping>,  // NEW
}
```

### Registry Index Extension

`build_indexes()` gains one new block:
```rust
if let Some(ref derive) = mapping.venues.derive {
    self.instrument_index
        .insert((Venue::Derive, derive.instrument.clone()), idx);
}
```

### SubscriptionManager Extension

`compute_desired_instruments()` gains:
```rust
if let Some(ref d) = mapping.venues.derive {
    derive.insert(d.instrument.clone());
}
```

The `reconcile()` method gains a Derive diff computation and watch channel send, mirroring the existing Deribit block.

## Discovery Changes: Second Options Venue

### New discover_derive() Function

```rust
pub async fn discover_derive(
    client: &reqwest::Client,
    base_url: &str,
    currencies: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>>
```

This mirrors `discover_deribit()` but calls `POST {base_url}/public/get_instruments` with Derive's request format and parses Derive's instrument naming convention.

The returned `DiscoveredInstrument` has identical fields to what `discover_deribit()` returns -- `venue: Venue::Derive`, `instrument_id: "BTC-20250627-100000-C"`, `asset: "BTC"`, `strike: Decimal`, `expiry: NaiveDate`, `direction: Direction::Above`. The `MatchKey` and `FuzzyMatchKey` structs are shared and require no changes.

### Cross-Venue Matching: Two Options Venues

The existing fuzzy matching in `find_cross_venue_candidates_fuzzy()` groups by `FuzzyMatchKey { asset, strike, direction }` and checks expiry tolerance. It already supports N instruments per key.

With Derive added, a group at `FuzzyMatchKey { asset: "BTC", strike: 100000, direction: Above }` may contain:
- Deribit: `BTC-27JUN25-100000-C`
- Derive: `BTC-20250627-100000-C`
- Polymarket: some token_id

A three-way match (Deribit + Derive + Polymarket) becomes possible. The existing scoring and proposal code handles multi-venue groups -- it iterates `mapping.venues.*` fields. Adding Derive to the group just means the proposed `CandidateMapping` gains a `derive` field.

### DiscoveryConfig Extension

```rust
pub struct DiscoveryConfig {
    // ... existing fields ...
    /// Poll interval for Derive instrument discovery (seconds).
    #[serde(default = "default_derive_poll")]
    pub derive_poll_interval_secs: u64,
    /// Derive currencies to discover options for.
    #[serde(default = "default_derive_currencies")]
    pub derive_currencies: Vec<String>,
}
```

### ContractLifecycleManager Changes

`src/events/lifecycle.rs` runs the periodic discovery pipeline. It needs a new `derive_last_poll` timestamp and a call to `discover_derive()` when its interval elapses. This mirrors the existing `deribit_last_poll` pattern exactly.

## SubscriptionManager Data Flow

```
EventRegistry (refreshed by config reload)
    |
    | Notify signal
    v
SubscriptionManager::reconcile()
    |
    +-- compute desired: {deribit_instruments, polymarket_subs, kalshi_tickers, derive_instruments}
    |
    +-- diff vs current state per venue
    |
    +-- if Derive diff non-empty:
    |       derive_tx.send(updated_derive_instruments)
    |       -> DeriveSupervisor receives via watch::changed()
    |       -> reconnects with new instrument list
    |
    +-- broadcast CleanupEvent { derive_instruments: removed_derive } to cleanup channels
```

The reconnect-based subscription approach (validated in v1.3) applies unchanged to Derive: when the instrument list changes, the supervisor reconnects, subscribing to the updated list. No per-instrument subscribe/unsubscribe protocol needed.

## Build Order (Dependency-Respecting)

### Phase 1: Type Extension (no logic)

**Files:** `src/types/venue.rs`, `src/config/events.rs`, `src/config/venues.rs`, `src/subscription/mod.rs`

Add `Venue::Derive`, `DeriveMapping`, `DeriveConfig`, `derive` field to `EventVenues`, `derive_instruments` to `CleanupEvent`, and `derive` channels to `SubscriptionSenders`/`SubscriptionReceivers`. Run `cargo check` and fix all match arm exhaustiveness errors across the codebase.

**Why first:** Everything else depends on these types. Fixes all compiler errors before adding any logic.

**Risk:** Low. All additions are additive with serde defaults.

### Phase 2: Message Types and Book State

**Files:** `src/feed/derive/messages.rs`, `src/feed/derive/book.rs`, `src/feed/derive/mod.rs`

Define `DeriveMessage` (JSON-RPC envelope), `OrderbookData` (bid/ask levels), `TickerData` (options pricing fields), `BookState` (or reuse DeribitInstrumentBook if Derive sends full snapshots). Unit tests for deserialization against captured or realistic mock payloads.

**Why second:** Processor depends on these. Client does not depend on these (client forwards raw text, not parsed).

**Risk:** Medium. Derive channel payload format must be confirmed against live API. Plan for one iteration of format adjustment.

### Phase 3: Client and Supervisor

**Files:** `src/feed/derive/client.rs`, `src/feed/derive/supervisor.rs`

`DeriveClient` connects to `wss://api.lyra.finance/ws`, sends subscribe JSON-RPC for each instrument's orderbook and ticker channels, forwards raw text frames. `DeriveSupervisor` wraps `DeriveClient` with exponential backoff, watches `instruments_rx: watch::Receiver<Vec<String>>`.

Unlike `DeribitClient`, no heartbeat protocol needed (standard WS ping/pong is sufficient). Unlike `KalshiSupervisor`, no RSA key signing needed (Derive market data is unauthenticated).

**Why third:** Can be tested with a live connection independently of the processor.

**Risk:** Low. Direct copy of DeribitSupervisor with DeriveClient.

### Phase 4: Processor (Normalize)

**Files:** `src/feed/derive/normalize.rs`

`DeriveProcessor` consumes `RawMessage`, parses `DeriveMessage`, routes by channel prefix, updates `BookState` and `TickerState`, calls `build_snapshot()` to emit `MarketSnapshot { venue: Venue::Derive, ... }`.

The `build_snapshot()` function is identical to Deribit's except for `venue: Venue::Derive`. Consider extracting a shared `build_options_snapshot(venue, instrument, book, ticker, ...)` helper to `src/feed/options_snapshot.rs` to avoid duplication -- but this is optional refactoring.

**Why fourth:** Depends on message types (Phase 2).

**Risk:** Low. Logic mirrors DeribitProcessor.

### Phase 5: Pipeline Integration

**Files:** `src/feed/pipeline.rs`, `src/feed/mod.rs`

Add Derive pipeline block to `run_live_multi_venue()`. Create `DeriveConfig`, plumb `derive_rx` from `SubscriptionReceivers`, spawn `DeriveSupervisor` and `DeriveProcessor`, forward to fan-in. Add Derive cleanup channel. Update `PipelineHandles` comments.

**Why fifth:** Depends on supervisor and processor (Phases 3-4).

**Risk:** Low. Copy Deribit block, change type names.

### Phase 6: Registry and Subscription Manager

**Files:** `src/events/registry.rs`, `src/subscription/manager.rs`

Add Derive instrument indexing to `build_indexes()`. Add `derive_tx`, `current_derive` to `SubscriptionManager`. Extend `compute_desired_instruments()` and `reconcile()`. Write unit tests for Derive subscription reconciliation.

**Why sixth:** Depends on Type Extension (Phase 1). Can proceed in parallel with Phases 2-5 since it uses only config types, not feed types.

**Risk:** Low. Mechanical extension of existing patterns.

### Phase 7: Discovery Integration

**Files:** `src/events/discovery.rs`, `src/events/lifecycle.rs`, `src/config/events.rs` (DiscoveryConfig)

Implement `discover_derive()`. Add `derive_poll_interval_secs` and `derive_currencies` to `DiscoveryConfig`. Add Derive discovery poll to lifecycle manager. Extend `find_cross_venue_candidates_fuzzy()` to include Derive instruments in grouping. Extend `CandidateMapping` / `CandidateVenues` with `derive` field. Extend `toml_writer.rs` to write `derive` venue entries in proposed mappings.

**Why seventh:** Depends on type extensions (Phase 1) and can run after core feed is working.

**Risk:** Medium. Instrument name parsing for Derive's date format (`YYYYMMDD`) differs from Deribit's (`DDMMMYY`). The `discover_derive()` function parses from structured API fields (not the name string), which avoids this -- same approach as `discover_deribit()`.

### Phase 8: Settlement Tracking (Deferred if Needed)

Settlement resolution for Derive options (onchain oracle) differs from Deribit (exchange-reported). If settlement tracking is required for v1.5, add `DeriveChecker` to `VenueChecker` enum in `src/settlement/`. This is low priority -- paper trading does not require settlement; signal validation uses Polymarket/Deribit settlement which still works.

**Why deferred:** Settlement tracking is for signal validation post-expiry. The primary v1.5 goal is live spread generation, not settlement confirmation of Derive positions.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Modifying Existing Venue Processors

**What:** Changing `DeribitProcessor` or `PolymarketProcessor` to handle Derive data.
**Why bad:** Violates isolation guarantee. Existing venues have no test coverage for Derive message formats. A parse error in Derive messages would affect Deribit processing.
**Instead:** Create `DeriveProcessor` as a separate struct in `src/feed/derive/normalize.rs`.

### Anti-Pattern 2: Reusing Kalshi as "Third Options Venue"

**What:** Keeping Kalshi active alongside Derive, creating a four-venue live system.
**Why bad:** Kalshi is inaccessible from Poland (per PROJECT.md). Adding Derive is the replacement, not an addition alongside Kalshi.
**Instead:** Kalshi remains in config but will receive no subscriptions (empty instrument list) when running from Poland. The architecture supports this -- a zero-instrument watch channel causes no subscriptions.

### Anti-Pattern 3: Translating Instrument Names in the Processor

**What:** Converting `BTC-20250627-100000-C` to `BTC-27JUN25-100000-C` in `DeriveProcessor` before creating the `InstrumentId`.
**Why bad:** Creates hidden coupling to Deribit's naming convention. The registry stores each venue's native format and looks up each independently.
**Instead:** `MarketSnapshot.instrument_id` carries Derive's native format. The registry's `lookup_by_instrument(Venue::Derive, "BTC-20250627-100000-C")` returns the correct `EventMapping`.

### Anti-Pattern 4: Sharing the Deribit Cleanup Channel with Derive

**What:** Reusing `deribit_cleanup_tx` for both Deribit and Derive instrument cleanup events.
**Why bad:** `DeribitProcessor` reads `deribit_instruments` from `CleanupEvent` to evict its book state. `DeriveProcessor` needs to read `derive_instruments` to evict its book state. Sending Derive cleanup via the Deribit channel would corrupt Deribit's book state eviction.
**Instead:** Create a separate `derive_cleanup_tx/rx` channel pair. Add `derive_instruments: Vec<String>` to `CleanupEvent` (or use a venue-keyed lookup).

### Anti-Pattern 5: Skipping Venue::Derive in match Arms

**What:** Adding `_ => {}` wildcard to avoid exhaustiveness errors when adding `Venue::Derive`.
**Why bad:** Silences the compiler's only mechanism for catching missed Derive handling in metrics, logging, health, and recording paths.
**Instead:** Add explicit `Venue::Derive => ...` arms everywhere. The compiler will catch every missed location.

### Anti-Pattern 6: Require All Venues Present in events.toml Mapping

**What:** Requiring that every `EventMapping` have both `deribit` and `derive` venue entries.
**Why bad:** Some events may only have Deribit pricing (no matching Derive instrument), or only Derive (for instruments Deribit doesn't list). Requiring all venues blocks valid single-venue configurations.
**Instead:** All venue fields in `EventVenues` are `Option<_>`. `SpreadEngine` already handles missing venues gracefully -- it skips comparisons when one venue's `event_id` lookup returns `None`.

## Integration Points with Existing Components

### SpreadEngine

SpreadEngine receives `MarketSnapshot` via the shared channel and groups by `event_id`. When both a Deribit snapshot and a Derive snapshot arrive with the same `event_id`, it computes their spread. No changes needed -- it already processes multi-venue snapshots per event.

The spread result will carry `venue_pair: (Venue::Deribit, Venue::Derive)` which flows into the `SpreadResult` struct. The `spread-analytics` CLI and `signal-scoring` CLI are already venue-agnostic -- they group by `event_id` and venue pair string.

### OptionsEngine (Black-76)

OptionsEngine uses `MarketSnapshot.bid_iv` / `ask_iv` / `underlying_price` / `mark_iv` to compute implied probability. Since `DeriveProcessor` populates these fields the same way as `DeribitProcessor`, OptionsEngine processes Derive snapshots identically without modification. Both venues go through the same Black-76 / call spread replication pipeline.

### EventRegistry lookup_by_instrument

`forward_snapshots()` in `pipeline.rs` calls `registry.lookup_by_instrument(Venue::Derive, &snapshot.instrument_id)` to annotate `event_id`. This call works as soon as `build_indexes()` includes the `Venue::Derive` case (Phase 6).

### JSONL Recording

`RecordLine` includes a `venue: Venue` field serialized as a string. Adding `Venue::Derive` to the enum and its `Display` impl produces `"derive"` in recorded JSONL. The recording schema is self-describing -- no migration needed.

## Sources

- Direct source analysis of `src/feed/deribit/supervisor.rs` (supervisor pattern, 206 lines)
- Direct source analysis of `src/feed/deribit/client.rs` (client pattern, 311 lines)
- Direct source analysis of `src/feed/deribit/normalize.rs` (processor pattern, 1076 lines)
- Direct source analysis of `src/feed/deribit/messages.rs` (message types, 653 lines)
- Direct source analysis of `src/feed/pipeline.rs` (fan-in pipeline, multi-venue block pattern)
- Direct source analysis of `src/subscription/manager.rs` (SubscriptionManager structure)
- Direct source analysis of `src/subscription/mod.rs` (CleanupEvent, SubscriptionSenders)
- Direct source analysis of `src/events/registry.rs` (EventRegistry, build_indexes)
- Direct source analysis of `src/events/discovery.rs` (discover_deribit, FuzzyMatchKey, MatchKey)
- Direct source analysis of `src/config/events.rs` (EventVenues, EventMapping, DiscoveryConfig)
- Direct source analysis of `src/config/venues.rs` (DeribitConfig, VenuesConfig pattern)
- Direct source analysis of `src/types/snapshot.rs` (MarketSnapshot schema, all fields)
- Direct source analysis of `src/types/venue.rs` (Venue enum)
- [docs.derive.xyz/reference/overview](https://docs.derive.xyz/reference/overview) -- JSON-RPC transport, WebSocket URL, message format (MEDIUM confidence)
- [docs.derive.xyz/reference/json-rpc](https://docs.derive.xyz/reference/json-rpc) -- request/response format (MEDIUM confidence)
- [docs.derive.xyz/reference/post_public-get-instrument](https://docs.derive.xyz/reference/post_public-get-instrument) -- instrument endpoint exists (MEDIUM confidence)
- [docs.derive.xyz/reference/public-get_ticker](https://docs.derive.xyz/reference/public-get_ticker) -- ticker endpoint, instrument_name parameter (MEDIUM confidence)
- [insights.derive.xyz/a-technical-overview-of-lyra-v2/](https://insights.derive.xyz/a-technical-overview-of-lyra-v2/) -- architecture overview, CLOB model
- [github.com/derivexyz/cockpit](https://github.com/derivexyz/cockpit) -- confirmed instrument_name format in CLI context
- Amberdata / Derive integration references confirming `{ASSET}-{YYYYMMDD}-{STRIKE}-{C|P}` instrument naming (MEDIUM confidence)

---
*Architecture research for: Derive.xyz venue integration (v1.5)*
*Researched: 2026-03-03*
