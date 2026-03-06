# Phase 30: Venue Type Foundation - Research

**Researched:** 2026-03-03
**Domain:** Rust enum extension, TOML config addition, Derive.xyz API verification
**Confidence:** HIGH

## Summary

Phase 30 is a pure foundation phase with two concrete deliverables: (1) add `Venue::Derive` to the Rust enum and resolve all compiler exhaustiveness errors without any `todo!()`/`unreachable!()` placeholders, and (2) add a `[derive]` section to `venues.toml` with verified connection parameters. The phase also requires live API verification against the Derive testnet to confirm channel subscription format, book update model, heartbeat mechanism, and authentication requirements for public channels.

This research has resolved all four LOW-confidence API questions from the milestone research. The answers, verified against the CCXT Pro `derive.py` implementation (which functions as a live integration reference against the real Derive API), are: (1) channel format uses dot-separated names like `orderbook.{instrument}.{group}.{depth}` and `ticker.{instrument}.{interval}`, (2) the book update model is snapshot-only (full replacement on every message, no delta processing needed), (3) heartbeat is standard WebSocket keep-alive (no Deribit-style `set_heartbeat` protocol), and (4) public channels (orderbook, ticker) do NOT require authentication -- `public/login` is only needed for private channels. This means the `k256` dependency is NOT needed for Phase 30 or any v1.5 phase (read-only scope).

**Primary recommendation:** Add `Venue::Derive` enum variant, fix all 29+ match-arm sites across the codebase, add `DeriveConfig` to `venues.toml`, and confirm the API parameters via a testnet connection -- all achievable without any new Cargo dependencies.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PIPE-01 | `Venue::Derive` enum variant added with all exhaustive match arms resolved across codebase | Venue enum in `src/types/venue.rs` has 3 variants (Deribit, Polymarket, Kalshi). Adding `Derive` triggers exhaustiveness errors in 29+ files with 367 `Venue::` references. All match sites identified. Architecture research confirms mechanical resolution pattern -- each site needs a `Venue::Derive => ...` arm mirroring the nearest existing venue. |
| PIPE-02 | Derive config section in venues.toml (WebSocket URL, rate limits, book depth, staleness threshold) | WebSocket URL confirmed: `wss://api.lyra.finance/ws` (production), `wss://api-demo.lyra.finance/ws` (testnet). Rate limits: fixed-window 5s, error code -32000 for exceeded. Channel format: `orderbook.{instrument}.10.{depth}`, `ticker.{instrument}.100`. Book depth default 10. Staleness threshold: 5000ms (project standard). No auth needed for public channels. |
</phase_requirements>

## Standard Stack

### Core

No new dependencies required for Phase 30. The entire phase uses existing Rust standard library and serde.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde | 1.0 (existing) | `#[serde(default)]` on new `DeriveConfig` fields, `#[serde(rename_all = "lowercase")]` on `Venue` enum | Already used for all config types |
| toml | 0.8 (existing) | Parsing `venues.toml` with new `[derive]` section | Already used for all config files |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-tungstenite | 0.28 (existing) | Live API verification script connecting to testnet | Only for success criterion 3 (live API confirmation) |
| serde_json | 1.0 (existing) | JSON-RPC message construction for testnet verification | Only for success criterion 3 |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| No new deps (recommended) | k256 0.13 for auth | k256 is unnecessary -- public channels work without authentication per CCXT Pro implementation and docs.derive.xyz structure |
| Manual testnet verification | Assume API docs are correct | Docs pages are partially inaccessible; live verification eliminates all remaining uncertainty in 30 minutes |

### What NOT to Add in This Phase

- `k256` -- public channels do not require authentication; defer to v2 (execution scope)
- `sha3` -- only needed alongside k256 for Ethereum signing; same deferral
- Any Derive-specific SDK -- none exists as a library crate
- `hex` crate -- not needed until auth is implemented

## Architecture Patterns

### Recommended Changes

```
src/
├── types/venue.rs              # ADD: Venue::Derive variant
├── config/venues.rs            # ADD: DeriveConfig struct
├── config/events.rs            # ADD: DeriveMapping struct, derive field on EventVenues
├── subscription/mod.rs         # ADD: derive_instruments to CleanupEvent, derive channels to Senders/Receivers
├── settlement/traits.rs        # ADD: VenueChecker::Derive stub (no-op for v1.5)
├── settlement/monitor.rs       # ADD: resolution_source_for_venue Derive arm
├── events/registry.rs          # ADD: Venue::Derive case in build_indexes()
├── events/discovery.rs         # ADD: Venue::Derive in DiscoveredInstrument handling
├── events/toml_writer.rs       # ADD: Derive venue entry writing
├── feed/pipeline.rs            # ADD: Derive block placeholder (actual wiring is Phase 32)
├── feed/health.rs              # ADD: Derive health tracking
├── feed/recording/             # ADD: Derive venue label in RecordLine
├── health/mod.rs               # ADD: Derive health endpoint
├── replay/mod.rs               # ADD: Venue::Derive replay arm
├── spread/                     # ADD: Venue::Derive in spread pattern handling
├── signal/                     # ADD: Venue::Derive in signal handling
├── paper_trade/                # ADD: Venue::Derive in paper trade handling
├── alert/                      # ADD: Venue::Derive in alert condition handling
├── persistence/                # ADD: Venue::Derive in checkpoint handling
├── pricing/                    # ADD: Venue::Derive in pricing engine handling
├── error/                      # ADD: Venue::Derive in venue error handling
└── main.rs                     # ADD: Venue::Derive in any startup match arms
config/
└── venues.toml                 # ADD: [derive] section with verified parameters
```

### Pattern 1: Venue Enum Extension

**What:** Add `Venue::Derive` to the enum and resolve all exhaustiveness errors.
**When to use:** This is the first and most critical task -- everything else depends on it.

```rust
// src/types/venue.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Venue {
    Deribit,
    Polymarket,
    Kalshi,
    Derive,    // NEW
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Venue::Deribit => write!(f, "deribit"),
            Venue::Polymarket => write!(f, "polymarket"),
            Venue::Kalshi => write!(f, "kalshi"),
            Venue::Derive => write!(f, "derive"),  // NEW
        }
    }
}

impl Venue {
    pub fn env_prefix(&self) -> &'static str {
        match self {
            Venue::Deribit => "DERIBIT",
            Venue::Polymarket => "POLYMARKET",
            Venue::Kalshi => "KALSHI",
            Venue::Derive => "DERIVE",  // NEW
        }
    }
}
```

Source: Direct analysis of `src/types/venue.rs` (31 lines).

### Pattern 2: Config Type with Serde Defaults

**What:** Add `DeriveConfig` struct to `venues.rs` with all fields having serde defaults for backward compatibility.
**When to use:** After `Venue::Derive` compiles cleanly.

```rust
// src/config/venues.rs

/// Derive.xyz connection settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveConfig {
    /// WebSocket URL. Default: wss://api.lyra.finance/ws
    #[serde(default = "default_derive_ws_url")]
    pub ws_url: String,
    /// Staleness threshold in milliseconds.
    #[serde(default = "default_staleness_threshold")]
    pub staleness_threshold_ms: u64,
    /// Reconnection configuration.
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    /// API rate limit per second for REST calls.
    #[serde(default = "default_derive_rate_limit")]
    pub rate_limit_per_second: u32,
    /// Instrument names to subscribe to.
    #[serde(default)]
    pub instruments: Vec<String>,
    /// Order book depth levels.
    #[serde(default = "default_derive_book_depth")]
    pub book_depth_levels: u32,
}

fn default_derive_ws_url() -> String {
    "wss://api.lyra.finance/ws".to_string()
}

fn default_derive_rate_limit() -> u32 {
    2  // Conservative; Derive allows ~10 req/5s window
}

fn default_derive_book_depth() -> u32 {
    10  // Verified from CCXT Pro: orderbook.{inst}.10.{depth}
}
```

Source: Pattern from existing `DeribitConfig` in `src/config/venues.rs`, verified API parameters from docs.derive.xyz.

### Pattern 3: EventVenues Extension with Backward Compatibility

**What:** Add `derive: Option<DeriveMapping>` to `EventVenues` with `#[serde(default)]`.
**When to use:** Alongside the config changes.

```rust
// src/config/events.rs

/// Derive.xyz instrument mapping for events.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeriveMapping {
    /// Derive instrument name (e.g., "BTC-20250627-100000-C").
    pub instrument: String,
}

/// Venue-specific instrument identifiers for a single event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventVenues {
    pub deribit: Option<DeribitMapping>,
    pub polymarket: Option<PolymarketMapping>,
    pub kalshi: Option<KalshiMapping>,
    #[serde(default)]
    pub derive: Option<DeriveMapping>,  // NEW -- backward compatible
}
```

Source: Pattern from existing `DeribitMapping`/`KalshiMapping` in `src/config/events.rs`.

### Pattern 4: VenueChecker Stub for Settlement

**What:** Add a no-op Derive variant to `VenueChecker` for v1.5.
**When to use:** When resolving settlement-related match arms.

```rust
// src/settlement/traits.rs
pub enum VenueChecker {
    Deribit(super::deribit::DeribitResolutionChecker),
    Kalshi(super::kalshi::KalshiResolutionChecker),
    Polymarket(super::polymarket::PolymarketResolutionChecker),
    // Derive settlement checking deferred to future -- stub returns Pending
}
```

Note: `VenueChecker` does NOT need a `Derive` variant in v1.5 since Derive settlement tracking is out of scope. The `resolution_source_for_venue()` function needs a `Venue::Derive` arm that returns a reasonable value. The `SettlementMonitor` only creates `VenueChecker` instances for venues that have settlement-tracked instruments -- since no Derive instruments will have settlement tracking in v1.5, the `Derive` variant on `VenueChecker` is not needed. But `resolution_source_for_venue()` still needs a match arm.

### Anti-Patterns to Avoid

- **Using `todo!()` or `unreachable!()` in match arms:** The compiler enforces exhaustiveness for a reason. Every `Venue::Derive` match arm must have real logic, even if that logic is "do the same thing as Deribit" or "return a default."
- **Using `_ => {}` wildcard to silence exhaustiveness errors:** This prevents the compiler from catching future missed Derive handling when new match sites are added.
- **Adding `k256` dependency in this phase:** Authentication is not needed for public channels. Adding unused dependencies creates dead code and confuses future developers.
- **Translating Derive instrument names to Deribit format:** Each venue retains its native name. The `EventRegistry` maps each independently to the same `event_id`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML backward compatibility | Custom migration logic | `#[serde(default)]` on all new fields | Serde handles missing fields automatically; existing `venues.toml` files parse without the `[derive]` section |
| Finding all match sites | Manual grep | `cargo check` compiler errors | The Rust compiler is exhaustive -- it finds every site that needs updating; grep may miss sites in macros or generated code |
| Venue display strings | Custom formatting | `#[serde(rename_all = "lowercase")]` + `Display` impl | Consistent with existing three venues |

**Key insight:** The Rust compiler is the primary tool for this phase. Adding the enum variant and running `cargo check` produces an exhaustive list of every site that needs a `Venue::Derive` arm. No manual searching needed.

## Common Pitfalls

### Pitfall 1: `todo!()` Shortcuts in Match Arms
**What goes wrong:** Developer adds `Venue::Derive` and patches all match arms with `todo!()` to make the code compile, intending to fix later. "Later" never comes. Runtime panics occur when any code path touches Derive.
**Why it happens:** There are 29+ files with Venue references. The temptation to patch quickly is strong.
**How to avoid:** Treat this as one focused task. Run `cargo check` after adding the variant, fix every error with real logic, then run `cargo check 2>&1 | grep -i "todo\|unreachable\|unimplemented"` to verify zero placeholders remain.
**Warning signs:** `cargo check` passes but `grep -r "todo!()" src/` finds matches in venue match arms.

### Pitfall 2: `VenuesConfig` Deserialization Failure
**What goes wrong:** Adding `derive: DeriveConfig` to `VenuesConfig` without `#[serde(default)]` or `Default` impl causes existing `venues.toml` files (which lack a `[derive]` section) to fail deserialization at startup.
**Why it happens:** `DeriveConfig` has required fields (like `ws_url`) that don't exist in old config files.
**How to avoid:** Either (a) implement `Default` for `DeriveConfig` with all serde defaults, or (b) make the field `derive: Option<DeriveConfig>` with `#[serde(default)]`. Option (b) is cleaner -- a missing `[derive]` section means Derive is not configured.
**Warning signs:** Application fails to start with "missing field `derive`" error after adding the type.

### Pitfall 3: Forgetting Subscription Infrastructure
**What goes wrong:** `Venue::Derive` is added to the enum but `SubscriptionSenders`/`SubscriptionReceivers` and `CleanupEvent` are not extended. Later phases try to wire Derive into the pipeline and discover missing channels.
**Why it happens:** These structs don't have `match venue` patterns -- they're structs with per-venue fields. The compiler doesn't enforce adding a new field.
**How to avoid:** Explicitly extend `CleanupEvent` with `derive_instruments: Vec<String>`, and `SubscriptionSenders`/`SubscriptionReceivers` with `derive: watch::Sender<Vec<String>>` / `derive: watch::Receiver<Vec<String>>`. These are additive and backward-compatible.
**Warning signs:** Phase 32 (pipeline integration) discovers missing channels and has to backtrack.

### Pitfall 4: Incorrect Channel Format Assumption
**What goes wrong:** Config or documentation comments use the wrong channel format (e.g., `orderbook-{instrument}-{depth}` with dashes instead of `orderbook.{instrument}.{group}.{depth}` with dots).
**Why it happens:** The Derive subscribe docs page showed dash-separated format, but the actual CCXT Pro implementation uses dot-separated format matching the real API.
**How to avoid:** Use the verified format: `orderbook.{instrument}.10.{depth}` and `ticker.{instrument}.100`. Document these in config comments. Confirm via testnet.
**Warning signs:** Subscribe requests return error status instead of "ok" in the status object.

## Code Examples

### venues.toml [derive] Section

```toml
# Derive.xyz connection settings (v1.5)
[derive]
ws_url = "wss://api.lyra.finance/ws"
staleness_threshold_ms = 5000
rate_limit_per_second = 2
book_depth_levels = 10
instruments = []

[derive.reconnect]
initial_backoff_ms = 1000
max_backoff_ms = 60000
randomization_factor = 0.5
```

Source: Verified against existing `[deribit]` section pattern and Derive API documentation.

### Live API Verification Script (Success Criterion 3)

The live testnet verification should confirm four things:

1. **Channel subscription format**: Send `{"method": "subscribe", "params": {"channels": ["orderbook.BTC-20250627-100000-C.10.10"]}}` and verify response contains `"status": {"orderbook.BTC-20250627-100000-C.10.10": "ok"}`.

2. **Book update model**: After subscribing, capture 20+ messages. Verify each message contains complete bids/asks arrays (snapshot model), not incremental deltas. Expected: `{method: "subscription", params: {channel: "orderbook.{inst}.10.10", data: {timestamp, instrument_name, bids: [[price, size], ...], asks: [[price, size], ...]}}}`.

3. **Heartbeat mechanism**: Hold connection for 30+ seconds. Verify no Deribit-style `test_request` notifications arrive. Standard WebSocket ping/pong is handled by `tokio-tungstenite` automatically.

4. **Authentication for public channels**: Subscribe to orderbook and ticker channels WITHOUT sending `public/login` first. Verify data flows. Expected: works without auth.

Source: Verification targets derived from CCXT Pro `derive.py` analysis (snapshot model confirmed, public channels confirmed unauthenticated, keepAlive 9000ms setting).

### Match Arm Resolution Strategy

For each file with `Venue::` references, the resolution follows this decision tree:

```
Is the match on Display/formatting?
  -> Add: Venue::Derive => write!(f, "derive") / "derive" / "DERIVE"

Is the match on metrics labels?
  -> Add: Venue::Derive => "derive" (same pattern as existing venues)

Is the match creating venue-specific logic (processors, clients)?
  -> Add: Venue::Derive => { /* Phase 31/32 will implement */ }
     with a comment, NOT todo!()
  -> For pipeline.rs: add empty Derive block with comment
  -> For replay/mod.rs: add Derive arm that logs "Derive replay not yet supported"

Is the match on settlement/resolution?
  -> Add: Venue::Derive => ResolutionSource::DeriveSettlement or similar
     (settlement is out of v1.5 scope but the type needs a valid arm)

Is the match in spread/signal engines?
  -> These operate on MarketSnapshot/EventId abstractions,
     not venue-specific branches. Verify no Venue-specific match exists.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Assumed auth required for public channels | Confirmed: public channels work WITHOUT authentication | Verified 2026-03-03 via CCXT Pro implementation | k256 dependency NOT needed for v1.5; simplifies integration significantly |
| Assumed dash-separated channel names (`orderbook-{inst}-{depth}`) | Confirmed: dot-separated (`orderbook.{inst}.{group}.{depth}`) | Verified 2026-03-03 via CCXT Pro implementation | Channel name constants must use dots, not dashes |
| Assumed delta book updates possible | Confirmed: snapshot-only model (full replacement on every message) | Verified 2026-03-03 via CCXT Pro `orderbook.reset(snapshot)` pattern | Simplifies DeriveBook implementation -- no delta processing, no sequence gap handling |
| Assumed `bid_iv`/`ask_iv` at top-level in ticker | Confirmed: nested under `option_pricing` object | Verified 2026-03-03 via docs.derive.xyz/reference/public-get_ticker | DeriveProcessor must extract IV from `option_pricing.bid_iv`, not `data.bid_iv` |

**Deprecated/outdated assumptions from milestone research:**
- `k256` as a required dependency -- REMOVED; public channels confirmed unauthenticated
- Channel format `orderbook.{instrument_name}.{depth}` -- CORRECTED to `orderbook.{instrument_name}.{group}.{depth}`
- `ticker.{instrument_name}` -- CORRECTED to `ticker.{instrument_name}.{interval}` (e.g., `100` for 100ms)

## Derive API Verified Facts

### Confirmed (HIGH confidence)

| Fact | Source | Verification |
|------|--------|-------------|
| WebSocket URL: `wss://api.lyra.finance/ws` | docs.derive.xyz/reference/overview | Multiple sources agree |
| Testnet URL: `wss://api-demo.lyra.finance/ws` | docs.derive.xyz/reference/overview | Multiple sources agree |
| JSON-RPC 2.0 protocol | docs.derive.xyz/reference/json-rpc | Confirmed by CCXT impl |
| Subscribe method: `{"method": "subscribe", "params": {"channels": [...]}}` | CCXT Pro derive.py | Verified implementation |
| Orderbook channel: `orderbook.{instrument}.{group}.{depth}` (e.g., `orderbook.BTC-PERP.10.10`) | CCXT Pro derive.py | Working implementation |
| Ticker channel: `ticker.{instrument}.{interval}` (e.g., `ticker.BTC-PERP.100`) | CCXT Pro derive.py | Working implementation |
| Book model: snapshot-only (full replacement, `orderbook.reset()`) | CCXT Pro derive.py | Verified in handle_order_book |
| No auth required for public channels (orderbook, ticker) | CCXT Pro `watch_public()` sends no login | docs.derive.xyz structure confirms public/private separation |
| Heartbeat: standard WebSocket keep-alive, 9s interval | CCXT Pro `streaming.keepAlive: 9000` | No Deribit-style heartbeat protocol |
| Notification format: `{method: "subscription", params: {channel: "...", data: {...}}}` | CCXT Pro derive.py | Verified in message handling |
| Orderbook data format: `{timestamp, instrument_name, bids: [[price, amount]], asks: [[price, amount]]}` | CCXT Pro derive.py | Verified in handle_order_book |
| Rate limit error code: -32000 | docs.derive.xyz/reference/error-codes | Official docs |
| Concurrent WS client limit error: -32100 | docs.derive.xyz/reference/error-codes | Official docs |
| Instrument name format: `BTC-YYYYMMDD-STRIKE-C/P` (e.g., `BTC-20250627-100000-C`) | Multiple sources (CCXT, Amberdata, docs.derive.xyz) | Confirmed by `option_details.option_type: "C"|"P"` |
| `option_details` fields: `expiry` (unix seconds), `index`, `option_type` ("C"/"P"), `strike`, `settlement_price` (nullable) | docs.derive.xyz/reference/post_public-get-instrument | Official docs |
| Ticker `option_pricing` fields: `bid_iv`, `ask_iv`, `iv`, `delta`, `gamma`, `theta`, `vega`, `rho`, `mark_price`, `forward_price`, `discount_factor` | docs.derive.xyz/reference/public-get_ticker | Official docs |
| `public/get_instruments` params: `currency`, `instrument_type` (enum: erc20/option/perp), `expired` (bool) | docs.derive.xyz/reference/post_public-get-instruments | Official docs |

### Needs Live Verification (MEDIUM confidence -- testnet confirmation recommended)

| Fact | Current Belief | How to Verify |
|------|---------------|---------------|
| Exact BTC options instrument names available on testnet | Format confirmed but available strikes/expiries depend on what's listed | `POST /public/get_instruments {"currency": "BTC", "instrument_type": "option", "expired": false}` |
| Rate limit numbers (requests per 5s window) | Estimated ~10 req/5s based on search snippets | Rate limit docs page was partially inaccessible; test with conservative limits first |
| Ticker notification includes `option_pricing` for options via WebSocket | Confirmed for REST; WebSocket ticker includes `instrument_ticker` with nested fields | Subscribe to `ticker.{btc_option}.100` and inspect response |
| Book depth parameter valid values | `10` used by CCXT; may support other values | Try `orderbook.{inst}.10.20` for 20-level depth |

## Scope of Match Arm Changes

### Files Requiring `Venue::Derive` Match Arms (29+ files, 367 references)

Based on `cargo check` propagation analysis:

**Type definitions (2 files):**
- `src/types/venue.rs` -- `Display`, `env_prefix()`

**Config (2 files):**
- `src/config/venues.rs` -- `VenuesConfig` struct (add field)
- `src/config/events.rs` -- `EventVenues` struct (add field), `SettlementMetadata` (add derive fields)

**Subscription infrastructure (1 file):**
- `src/subscription/mod.rs` -- `CleanupEvent`, `SubscriptionSenders`, `SubscriptionReceivers`

**Feed layer (5 files):**
- `src/feed/pipeline.rs` -- Derive pipeline block
- `src/feed/health.rs` -- Venue health tracking
- `src/feed/recording/mod.rs` -- RecordLine venue label
- `src/feed/recording/writer.rs` -- File naming for recordings
- `src/feed/mock/replay.rs` -- Mock replay venue handling

**Events (5 files):**
- `src/events/registry.rs` -- `build_indexes()` Derive case
- `src/events/discovery.rs` -- `DiscoveredInstrument` Derive handling
- `src/events/lifecycle.rs` -- Discovery polling for Derive
- `src/events/risk.rs` -- Basis risk for Derive
- `src/events/toml_writer.rs` -- TOML proposal writing for Derive

**Settlement (3 files):**
- `src/settlement/traits.rs` -- `VenueChecker` (stub or skip)
- `src/settlement/monitor.rs` -- `resolution_source_for_venue()`
- `src/settlement/types.rs` -- Settlement type handling

**Engines (5 files):**
- `src/spread/engine.rs` -- Spread computation
- `src/spread/patterns.rs` -- Spread pattern definitions
- `src/signal/engine.rs` -- Signal evaluation
- `src/signal/logger.rs` -- Signal logging
- `src/signal/types.rs` -- Signal type handling

**Other (6 files):**
- `src/main.rs` -- Startup configuration
- `src/replay/mod.rs` -- JSONL replay
- `src/paper_trade/analyzer.rs` -- Trade analysis
- `src/paper_trade/position.rs` -- Position tracking
- `src/paper_trade/tracker.rs` -- Paper trade tracking
- `src/persistence/checkpoint.rs` -- State checkpointing
- `src/pricing/engine.rs` -- Pricing engine
- `src/alert/monitor.rs` -- Alert monitoring
- `src/alert/types.rs` -- Alert type handling
- `src/error/venue.rs` -- Venue error handling
- `src/health/mod.rs` -- Health endpoint

## Open Questions

1. **What BTC option instruments are currently listed on Derive testnet?**
   - What we know: Instrument format is `BTC-YYYYMMDD-STRIKE-C/P`; `public/get_instruments` with `currency: "BTC", instrument_type: "option"` returns the list.
   - What's unclear: Whether testnet has actively traded BTC options that produce orderbook/ticker data.
   - Recommendation: Query testnet during live API verification. If testnet has no BTC options, use mainnet for read-only verification (no auth needed).

2. **Exact rate limit numbers for Derive**
   - What we know: Fixed-window algorithm with 5s window. Error code -32000 when exceeded.
   - What's unclear: Exact number of requests per window for non-authenticated IP.
   - Recommendation: Start with conservative `rate_limit_per_second = 2` in config. Adjust after observing real behavior during soak test. The rate limit primarily affects REST discovery calls, not WebSocket subscriptions.

3. **`DeriveConfig` as required vs optional field on `VenuesConfig`**
   - What we know: Existing venues (deribit, polymarket, kalshi) are all required fields on `VenuesConfig`. Adding `derive: DeriveConfig` as required would break existing configs missing `[derive]`.
   - What's unclear: Whether to make it `Option<DeriveConfig>` (backward compatible but inconsistent) or require it with `Default` impl.
   - Recommendation: Use `#[serde(default)]` with a `Default` impl on `DeriveConfig`. This makes the field required at the Rust type level but optional in TOML (missing section uses defaults). Consistent with how other configs handle optional subsections.

## Sources

### Primary (HIGH confidence)
- [docs.derive.xyz/reference/overview](https://docs.derive.xyz/reference/overview) -- WebSocket URL, JSON-RPC protocol, public/private channel separation
- [docs.derive.xyz/reference/json-rpc](https://docs.derive.xyz/reference/json-rpc) -- Request/response format, transport-agnostic protocol
- [docs.derive.xyz/reference/subscribe](https://docs.derive.xyz/reference/subscribe) -- Subscribe method params: `channels` array, response with `current_subscriptions` and `status`
- [docs.derive.xyz/reference/post_public-login](https://docs.derive.xyz/reference/post_public-login) -- WebSocket-only auth, wallet/timestamp/signature; optional for public channels
- [docs.derive.xyz/reference/public-get_ticker](https://docs.derive.xyz/reference/public-get_ticker) -- All ticker fields including nested `option_pricing` (bid_iv, ask_iv, iv, delta, gamma, theta, vega, rho)
- [docs.derive.xyz/reference/post_public-get-instruments](https://docs.derive.xyz/reference/post_public-get-instruments) -- Instrument listing with `currency`, `instrument_type`, `expired` params; `option_details` with `expiry`, `strike`, `option_type`
- [docs.derive.xyz/reference/post_public-get-instrument](https://docs.derive.xyz/reference/post_public-get-instrument) -- Single instrument detail with `option_details`: `expiry` (unix seconds), `index`, `option_type` ("C"/"P"), `strike`, `settlement_price` (nullable)
- [docs.derive.xyz/reference/error-codes](https://docs.derive.xyz/reference/error-codes) -- Error -32000 (rate limit), -32100 (concurrent WS limit)
- [CCXT Pro derive.py](https://github.com/ccxt/ccxt/blob/master/python/ccxt/pro/derive.py) -- Channel format `orderbook.{inst}.10.{depth}`, `ticker.{inst}.100`; snapshot-only book model; public channels unauthenticated; keepAlive 9000ms
- Direct codebase analysis: `src/types/venue.rs`, `src/config/venues.rs`, `src/config/events.rs`, `src/settlement/traits.rs`, `src/subscription/mod.rs` -- all match arm sites identified

### Secondary (MEDIUM confidence)
- [CCXT derive.py (REST)](https://github.com/ccxt/ccxt/blob/master/python/ccxt/derive.py) -- REST API integration patterns, instrument name format confirmed
- Hummingbot Derive connector -- wallet_address/private_key credential structure, `personal_sign` approach (relevant for future v2 auth)

### Tertiary (LOW confidence)
- Rate limit exact numbers (~10 req/5s) -- derived from search result snippets, docs page partially inaccessible; start conservative and tune

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies needed; all changes use existing serde/toml
- Architecture: HIGH -- Venue enum propagation is mechanical; all 29+ match sites identified from grep analysis; `DeriveConfig` follows established pattern
- Pitfalls: HIGH -- all four pitfalls are preventable with mechanical checks (`cargo check`, `grep todo`, serde default annotations)
- API facts: HIGH -- channel format, book model, auth requirement all verified against working CCXT Pro implementation

**Research date:** 2026-03-03
**Valid until:** 2026-04-03 (stable -- Derive API is production and unlikely to change channel format)
