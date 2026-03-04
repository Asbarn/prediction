# Phase 31: Derive Feed and Normalization - Research

**Researched:** 2026-03-04
**Domain:** WebSocket feed client, message parsing, order book state, USDC price normalization
**Confidence:** HIGH

## Summary

Phase 31 implements a standalone Derive.xyz WebSocket feed that connects to `wss://api.lyra.finance/ws`, subscribes to `orderbook.*` and `ticker_slim.*` channels, maintains per-instrument book state from snapshot-only updates, and emits `MarketSnapshot` events with correctly normalized USDC prices. The implementation closely follows the existing Deribit feed architecture (7 files: `mod.rs`, `client.rs`, `supervisor.rs`, `normalize.rs`, `messages.rs`, `channels.rs`, `book.rs`) but is simpler in two ways: (1) Derive uses snapshot-only book updates (no delta reconciliation or `change_id`/`prev_change_id` sequencing), and (2) Derive heartbeats are standard WS PING/PONG (no application-level `set_heartbeat`/`test_request` protocol).

The critical new logic is USDC-to-BTC price normalization. Deribit quotes option premiums in BTC (inverse contracts), and the existing `PricingEngine` converts `price_btc * forward` to get USD prices for IV solving. Derive quotes premiums directly in USDC. For the `MarketSnapshot` to be venue-agnostic, Derive bid/ask prices must be stored as USDC values on the snapshot and the `PricingEngine` must handle Derive snapshots without the BTC-to-USD conversion step. The simplest correct approach: store Derive prices as-is (already USDC-denominated) in the `MarketSnapshot` and gate the `price_btc * forward` conversion in `PricingEngine` on `venue == Venue::Deribit`.

All API details (channel format, message structure, field names, data types) are CONFIRMED from live production capture conducted 2026-03-04 (see `DERIVE-API-FINDINGS.md`).

**Primary recommendation:** Create `src/feed/derive/` module (7 files) by adapting the Deribit feed pattern, with Derive-specific message types (string-valued prices, `ticker_slim` abbreviated keys), a simplified book (no sequence verification), and a normalization layer that stores USDC prices directly in `MarketSnapshot` fields.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FEED-01 | Derive WebSocket client connects to `wss://api.lyra.finance/ws` with JSON-RPC 2.0 and auto-reconnection | Deribit client pattern (`src/feed/deribit/client.rs`) provides exact template. Derive uses same JSON-RPC 2.0 protocol but simpler: subscribe method is `subscribe` (not `public/subscribe`), no heartbeat setup needed. Auto-reconnection via supervisor pattern. |
| FEED-02 | Derive orderbook state maintenance from WebSocket subscription with bid/ask depth | Derive book is snapshot-only (CONFIRMED). Every message contains full bids/asks arrays. No delta processing, no `change_id` sequencing. Simpler than `InstrumentBook` in `src/feed/deribit/book.rs`. Prices are strings that must be parsed to `Decimal`. |
| FEED-03 | Derive ticker data parsing (mark price, mark IV, bid IV, ask IV, underlying price, greeks) | `ticker_slim` format uses abbreviated single-letter keys (CONFIRMED from live capture). `option_pricing` nested object contains `d` (delta), `g` (gamma), `v` (vega), `t` (theta), `i` (IV mid), `bi` (IV bid), `ai` (IV ask), `f` (forward), `m` (mark price). All values are strings. |
| FEED-04 | DeriveSupervisor with heartbeat monitoring, reconnection, and watch channel for dynamic subscriptions | Exact copy of `DeribitSupervisor` pattern (`src/feed/deribit/supervisor.rs`). The only difference: no heartbeat setup request (Derive uses WS PING/PONG). Dead connection detection via message timeout (60s recommended). Watch channel for instrument list updates already proven in Deribit supervisor. |
| FEED-05 | JSONL raw feed recording for Derive messages (same pattern as existing venues) | `RecordLine` already supports `Venue::Derive` (venue enum extended in Phase 30). `JsonlWriter` is venue-generic -- `JsonlWriter::new(base_dir, Venue::Derive)` creates `recordings/derive/YYYY-MM-DD.jsonl`. No new recording code needed, just wire `record_tx` into the processor. |
| NORM-01 | USDC-linear to normalized price conversion for Derive option premiums | Derive prices are already in USDC. Deribit prices are in BTC and multiplied by `forward` in `PricingEngine` (line 230-232 of `pricing/engine.rs`). For Derive, bid/ask prices go directly into `MarketSnapshot` as USDC values. The `PricingEngine` must skip the `price_btc * forward` conversion for Derive. Validation: compare Derive vs Deribit implied probabilities for same strike/expiry -- should be within 5%. |
| NORM-02 | Derive instrument name parser for `BTC-YYYYMMDD-STRIKE-C/P` format with unit tests | Derive uses `BTC-20260305-69500-P` format (CONFIRMED). This differs from Deribit's `BTC-27JUN25-69500-P` (DDMMMYY). A new parser function `parse_derive_instrument()` must handle YYYYMMDD dates. Must reject Deribit format (DDMMMYY) and vice versa. Returns same `ParsedInstrument` struct. |
| NORM-03 | MarketSnapshot emission from Derive data with all required fields | `build_snapshot()` in `src/feed/deribit/normalize.rs` provides the template. Derive version sets `venue: Venue::Derive`, fills bid/ask from orderbook, mark_price/mark_iv/greeks from `ticker_slim`, and USDC-denominated prices. All `MarketSnapshot` fields are already venue-agnostic. |
| NORM-04 | Staleness detection for Derive snapshots using configurable threshold | Same `is_exchange_data_stale()` function used by Deribit. Derive messages include `timestamp` field (milliseconds since epoch, CONFIRMED). `DeriveConfig.staleness_threshold_ms` already defined (5000ms default). No new staleness logic needed. |
</phase_requirements>

## Standard Stack

### Core

No new Cargo dependencies required. All libraries are already in the workspace.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio-tungstenite | 0.28 (existing) | WebSocket client connection | Same as Deribit/Polymarket/Kalshi clients |
| serde / serde_json | 1.0 (existing) | JSON-RPC message deserialization | All venue message types use serde |
| rust_decimal | 1.x (existing) | String-to-Decimal price parsing | Derive sends prices as strings; `Decimal::from_str()` preserves precision |
| backoff | 0.4 (existing) | Exponential backoff in supervisor | Same pattern as DeribitSupervisor |
| tokio | 1.x (existing) | Async runtime, channels, cancellation | Core async infrastructure |
| chrono | 0.4 (existing) | Timestamp handling, YYYYMMDD date parsing | `NaiveDate::parse_from_str` for Derive date format |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 (existing) | Structured logging | All feed components use tracing |
| metrics | 0.23 (existing) | Prometheus metric recording | Feed latency, message count gauges |
| thiserror | 1.0 (existing) | Error types | If DeriveBook needs error variants |

### What NOT to Add

- No `k256` -- public channels confirmed unauthenticated
- No new crate for WS -- `tokio-tungstenite` already handles everything
- No custom JSON-RPC library -- raw `serde_json::Value` routing works for 2 message types

## Architecture Patterns

### Recommended Project Structure

```
src/feed/derive/
    mod.rs              # pub mod declarations (6 submodules)
    client.rs           # DeriveClient: connect, subscribe, forward raw frames
    supervisor.rs       # DeriveSupervisor: reconnection loop with backoff
    normalize.rs        # DeriveProcessor: parse messages, maintain state, emit MarketSnapshot
    messages.rs         # DeriveMessage enum, DeriveBookData, DeriveTickerSlimData
    channels.rs         # DeriveChannelKind, build_subscription_channels, extract_instrument
    book.rs             # DeriveBook: simplified snapshot-only book state

src/pricing/
    instrument.rs       # ADD: parse_derive_instrument() alongside existing parse_deribit_instrument()

src/feed/mod.rs         # ADD: pub mod derive;
```

### Pattern 1: DeriveClient (Simplified Deribit Client)

**What:** WebSocket client that connects, subscribes, and forwards raw text frames.
**Difference from Deribit:** No `public/set_heartbeat` request, no heartbeat response handling, subscribe method is `subscribe` (not `public/subscribe`). Dead connection detected by 60s message timeout.

```rust
// src/feed/derive/client.rs
// Subscribe message format (CONFIRMED from live capture):
let subscribe_msg = serde_json::json!({
    "jsonrpc": "2.0",
    "id": request_id,
    "method": "subscribe",  // NOT "public/subscribe"
    "params": {
        "channels": subscription_channels
    }
});

// No heartbeat setup needed -- WS PING/PONG handled by tokio-tungstenite
// Dead connection timeout: 60s with no messages
let timeout_duration = Duration::from_secs(60);
```

### Pattern 2: DeriveMessage (String-Valued Fields)

**What:** Serde types for Derive JSON-RPC messages. Key difference: all numeric values are strings.
**Critical:** `ticker_slim` uses abbreviated single-letter keys, NOT full names.

```rust
// src/feed/derive/messages.rs

/// Top-level Derive JSON-RPC message.
/// Simpler than Deribit -- no heartbeat variant needed.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DeriveMessage {
    Response(RpcResponse),
    Notification(RpcNotification),
}

// Reuse RpcResponse/RpcNotification/NotificationParams from deribit::messages
// (or define Derive-specific copies to avoid coupling)

/// Derive orderbook data (snapshot-only model).
/// All values are strings, not numbers.
#[derive(Debug, Deserialize)]
pub struct DeriveBookData {
    pub timestamp: i64,
    pub instrument_name: String,
    pub publish_id: i64,
    /// [[price_string, amount_string], ...]
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
}

/// Derive ticker_slim data with abbreviated keys.
#[derive(Debug, Deserialize)]
pub struct DeriveTickerSlimData {
    /// Server timestamp (ms since epoch)
    #[serde(rename = "t")]
    pub timestamp: i64,
    /// Best ask amount
    #[serde(rename = "A")]
    pub best_ask_amount: Option<String>,
    /// Best ask price (USDC)
    #[serde(rename = "a")]
    pub best_ask_price: Option<String>,
    /// Best bid amount
    #[serde(rename = "B")]
    pub best_bid_amount: Option<String>,
    /// Best bid price (USDC)
    #[serde(rename = "b")]
    pub best_bid_price: Option<String>,
    /// Option pricing data (nested)
    pub option_pricing: Option<DeriveOptionPricing>,
    /// Underlying index price
    #[serde(rename = "I")]
    pub index_price: Option<String>,
    /// Mark price
    #[serde(rename = "M")]
    pub mark_price: Option<String>,
}

/// Option pricing nested object in ticker_slim.
#[derive(Debug, Deserialize)]
pub struct DeriveOptionPricing {
    /// Delta
    #[serde(rename = "d")]
    pub delta: Option<String>,
    /// Theta
    #[serde(rename = "t")]
    pub theta: Option<String>,
    /// Gamma
    #[serde(rename = "g")]
    pub gamma: Option<String>,
    /// Vega
    #[serde(rename = "v")]
    pub vega: Option<String>,
    /// IV (mid)
    #[serde(rename = "i")]
    pub iv: Option<String>,
    /// Rate
    #[serde(rename = "r")]
    pub rate: Option<String>,
    /// Forward price
    #[serde(rename = "f")]
    pub forward: Option<String>,
    /// Mark price
    #[serde(rename = "m")]
    pub mark_price: Option<String>,
    /// Discount factor
    #[serde(rename = "df")]
    pub discount_factor: Option<String>,
    /// Bid IV
    #[serde(rename = "bi")]
    pub bid_iv: Option<String>,
    /// Ask IV
    #[serde(rename = "ai")]
    pub ask_iv: Option<String>,
}
```

### Pattern 3: DeriveBook (Simplified, No Sequence Verification)

**What:** Per-instrument book state that stores full snapshot on every update.
**Difference from Deribit:** No `change_id`/`prev_change_id` sequencing, no `SequenceError`. Every message replaces the full state. Uses `publish_id` for monotonicity logging but NOT for gating.

```rust
// src/feed/derive/book.rs
pub struct DeriveBook {
    pub instrument: InstrumentId,
    pub bids: Vec<(Price, Notional)>,
    pub asks: Vec<(Price, Notional)>,
    pub last_publish_id: Option<i64>,
    pub timestamp: Option<DualTimestamp>,
    pub is_stale: bool,
}

impl DeriveBook {
    pub fn apply_snapshot(&mut self, data: &DeriveBookData, received_at: DualTimestamp) {
        // Parse string prices to Decimal
        self.bids = data.bids.iter()
            .filter_map(|[price, amount]| string_pair_to_level(price, amount))
            .collect();
        self.asks = data.asks.iter()
            .filter_map(|[price, amount]| string_pair_to_level(price, amount))
            .collect();

        // Sort bids descending, asks ascending
        self.bids.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        self.asks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        self.last_publish_id = Some(data.publish_id);
        self.timestamp = Some(received_at);
        self.is_stale = false;
    }
}

fn string_pair_to_level(price: &str, amount: &str) -> Option<(Price, Notional)> {
    let p = Decimal::from_str(price).ok()?;
    let a = Decimal::from_str(amount).ok()?;
    Some((Price::new(p), Notional::new(a)))
}
```

### Pattern 4: USDC Price Normalization

**What:** Derive prices are already in USDC. The `MarketSnapshot` stores them as-is.
**Critical difference from Deribit:** Deribit bid/ask are in BTC (e.g., 0.0055 BTC for a $100k call). Derive bid/ask are in USDC (e.g., 340 USDC for the same call).

```rust
// In DeriveProcessor::build_snapshot():
// Derive prices go directly into MarketSnapshot -- they're already USDC.
// No conversion needed at the feed level.
MarketSnapshot {
    venue: Venue::Derive,
    bid: derive_book.best_bid().map(|(p, _)| p),  // USDC price
    ask: derive_book.best_ask().map(|(p, _)| p),  // USDC price
    // ... other fields from ticker_slim
}

// In PricingEngine::process_snapshot() (existing file, must be modified):
// Gate the BTC-to-USD conversion on venue
if snapshot.venue == Venue::Deribit {
    // Deribit inverse: price_usd = price_btc * forward
    let bid_price_usd = bid_price * forward;
    let ask_price_usd = ask_price * forward;
} else {
    // Derive (and future USDC venues): prices already in USD
    let bid_price_usd = bid_price;
    let ask_price_usd = ask_price;
}
```

### Pattern 5: Derive Instrument Name Parser

**What:** Parse `BTC-YYYYMMDD-STRIKE-C/P` into `ParsedInstrument`.
**Key difference from Deribit parser:** Date format is `YYYYMMDD` (e.g., `20260305`), not `DDMMMYY` (e.g., `27JUN25`).

```rust
// src/pricing/instrument.rs (add alongside parse_deribit_instrument)
pub fn parse_derive_instrument(name: &str) -> Option<ParsedInstrument> {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 4 { return None; }

    let asset = parts[0].to_string();
    let date_str = parts[1];  // "20260305"
    let strike_str = parts[2];
    let type_str = parts[3];

    // Parse date: YYYYMMDD format
    if date_str.len() != 8 { return None; }
    let expiry = NaiveDate::parse_from_str(date_str, "%Y%m%d").ok()?;

    let strike: f64 = strike_str.parse().ok()?;
    let option_type = match type_str {
        "C" => OptionType::Call,
        "P" => OptionType::Put,
        _ => return None,
    };

    Some(ParsedInstrument { asset, expiry, strike, option_type })
}

// CRITICAL TEST: Derive parser must reject Deribit format and vice versa
#[test]
fn derive_parser_rejects_deribit_format() {
    assert!(parse_derive_instrument("BTC-27JUN25-100000-C").is_none());
}

#[test]
fn deribit_parser_rejects_derive_format() {
    assert!(parse_deribit_instrument("BTC-20260305-69500-P").is_none());
}
```

### Pattern 6: Channel Construction

**What:** Build subscription channel names for Derive.
**Format (CONFIRMED):** `orderbook.{instrument}.{group}.{depth}` and `ticker_slim.{instrument}.{interval_ms}`

```rust
// src/feed/derive/channels.rs
pub fn build_subscription_channels(instruments: &[String], book_depth: u32) -> Vec<String> {
    let mut channels = Vec::with_capacity(instruments.len() * 2);
    for inst in instruments {
        channels.push(format!("orderbook.{inst}.10.{book_depth}"));
        channels.push(format!("ticker_slim.{inst}.100"));
    }
    channels
}

pub enum DeriveChannelKind {
    Orderbook,
    TickerSlim,
    Unknown(String),
}

impl DeriveChannelKind {
    pub fn parse(channel: &str) -> Self {
        if channel.starts_with("orderbook.") {
            DeriveChannelKind::Orderbook
        } else if channel.starts_with("ticker_slim.") {
            DeriveChannelKind::TickerSlim
        } else {
            DeriveChannelKind::Unknown(channel.to_string())
        }
    }
}

pub fn extract_instrument(channel: &str) -> Option<String> {
    let kind = DeriveChannelKind::parse(channel);
    match kind {
        DeriveChannelKind::Orderbook => {
            // Format: orderbook.{instrument}.{group}.{depth}
            // Strip "orderbook." prefix, then take everything before ".10." or ".1."
            let rest = channel.strip_prefix("orderbook.")?;
            // Find the group separator -- instrument names contain dashes, not dots
            // Pattern: BTC-20260305-69500-P.10.10
            // Find the last two ".X" segments
            let parts: Vec<&str> = rest.rsplitn(3, '.').collect();
            if parts.len() >= 3 {
                Some(parts[2].to_string())
            } else {
                None
            }
        }
        DeriveChannelKind::TickerSlim => {
            // Format: ticker_slim.{instrument}.{interval}
            let rest = channel.strip_prefix("ticker_slim.")?;
            let (instrument, _interval) = rest.rsplit_once('.')?;
            Some(instrument.to_string())
        }
        DeriveChannelKind::Unknown(_) => None,
    }
}
```

### Anti-Patterns to Avoid

- **Reusing `DeribitMessage`/`BookData` types for Derive messages:** Derive sends prices as strings, uses different field names (`publish_id` vs `change_id`), and `ticker_slim` uses abbreviated keys. Sharing types would require complex `#[serde(alias)]` and lose type safety. Define separate types.
- **Applying BTC-inverse conversion to Derive prices:** Derive prices are USDC-denominated. Multiplying by `forward` would produce wrong values (double-counting). The normalization gate MUST check venue.
- **Implementing `change_id` sequence verification for Derive:** Derive has no delta model. `publish_id` is monotonic but skipped IDs are normal (server-side filtering). Just log `publish_id` for debugging, don't gate on it.
- **Using `f64` for Derive price deserialization:** Derive sends prices as strings ("340", "0.4"). Using `f64` loses precision. Deserialize as `String`, convert to `Decimal`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| WebSocket connection | Custom TCP/TLS code | `tokio_tungstenite::connect_async` | Already proven in 3 venue clients |
| Reconnection backoff | Custom timer logic | `backoff::ExponentialBackoffBuilder` | Already used in DeribitSupervisor |
| String-to-Decimal conversion | Custom parser | `Decimal::from_str()` from `rust_decimal` | Handles all Derive price formats correctly |
| YYYYMMDD date parsing | Manual substring extraction | `NaiveDate::parse_from_str(s, "%Y%m%d")` | chrono handles validation, leap years, etc. |
| JSONL recording | Custom file writer | Existing `JsonlWriter::new(base_dir, Venue::Derive)` | Already venue-generic, handles daily rotation |
| Dead connection detection | Custom ping implementation | Message timeout (60s with no frames) | tokio-tungstenite auto-responds to WS PINGs; just check message frequency |

**Key insight:** The Deribit feed is the template. 80% of the code is structural copy with Derive-specific message types and the absence of heartbeat/delta logic.

## Common Pitfalls

### Pitfall 1: Not Handling `null` in ticker_slim Fields
**What goes wrong:** Some `ticker_slim` fields can be `null` (e.g., `"f": null` for forward price when market data is insufficient). Deserializing as `String` panics on `null`.
**Why it happens:** Live capture showed `"f": null` in some ticker_slim messages. Developer assumes all string fields are present.
**How to avoid:** All `ticker_slim` fields MUST be `Option<String>`, not `String`. Confirmed from live capture: `"f": null` observed in production data.
**Warning signs:** `serde_json::from_value` failures on ticker_slim messages with "invalid type: null, expected a string" error.

### Pitfall 2: Wrong Price Denomination in PricingEngine
**What goes wrong:** Derive prices are USDC but PricingEngine multiplies by `forward` (Deribit inverse convention). Result: Derive implied probabilities are wildly wrong (e.g., 0.001% instead of 25%).
**Why it happens:** `PricingEngine::process_snapshot()` currently applies `price_btc * forward` unconditionally. No venue check exists.
**How to avoid:** Gate the conversion: `if snapshot.venue == Venue::Deribit { price_usd = price * forward } else { price_usd = price }`. Test with real Derive + Deribit data for the same strike/expiry -- implied probabilities should be within 5%.
**Warning signs:** Success criterion 4 fails: Derive vs Deribit implied probability divergence exceeds 5%.

### Pitfall 3: Using Deribit's ticker Fields on Derive's ticker_slim
**What goes wrong:** Code expects `instrument_name`, `mark_price`, `delta`, `bid_iv` at top-level keys. Derive's `ticker_slim` uses `t`, `M`, nested `option_pricing.d`, `option_pricing.bi`.
**Why it happens:** Developer copies Deribit `TickerData` struct and assumes field names are the same.
**How to avoid:** Use completely separate `DeriveTickerSlimData` struct with correct `#[serde(rename = "...")]` attributes. Reference the DERIVE-API-FINDINGS.md field mapping table.
**Warning signs:** All ticker_slim deserializations fail silently (warn log floods).

### Pitfall 4: `DeriveProcessor` Not Wired to Recording
**What goes wrong:** Raw Derive messages are not recorded to JSONL despite FEED-05 requiring it.
**Why it happens:** Developer forgets to pass `record_tx: Option<mpsc::Sender<RecordLine>>` to the processor, or forgets to populate the `channel` and `instrument` fields on `RecordLine`.
**How to avoid:** Follow the exact `DeribitProcessor` pattern: `record_tx.try_send(RecordLine { raw, local_ts, venue: Venue::Derive, channel, instrument })`.
**Warning signs:** `recordings/derive/` directory is empty during integration testing.

### Pitfall 5: Incorrect Channel Name Extraction for Derive
**What goes wrong:** Instrument name extraction fails because Derive instrument names contain dashes (`BTC-20260305-69500-P`) and the channel format uses dots (`orderbook.BTC-20260305-69500-P.10.10`). Naive split-on-dot parsing breaks the instrument name.
**Why it happens:** Deribit's channel format is `book.BTC-27JUN25-100000-C.none.20.100ms` where `.none.` separates instrument from suffix. Derive has no `.none.` marker.
**How to avoid:** For orderbook channels, use `rsplitn(3, '.')` to strip the last two dot-separated segments (group and depth). For ticker_slim, use `rsplit_once('.')` to strip the interval.
**Warning signs:** All instrument lookups in the book/ticker state maps fail with "unknown instrument" debug logs.

### Pitfall 6: ticker_slim Outer Wrapper Structure
**What goes wrong:** Developer deserializes ticker_slim `data` directly as `DeriveTickerSlimData`, but the actual data is nested inside `data.instrument_ticker`.
**Why it happens:** The outer `data` wrapper has `timestamp` and `instrument_ticker` fields. The abbreviated-key fields are inside `instrument_ticker`, not at the top level.
**How to avoid:** Define a wrapper struct:
```rust
#[derive(Deserialize)]
struct TickerSlimWrapper {
    timestamp: i64,
    instrument_ticker: DeriveTickerSlimData,
}
```
**Warning signs:** All ticker_slim fields deserialize as `None` despite valid messages arriving.

## Code Examples

### Live Derive Subscribe Request (CONFIRMED)
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "subscribe",
  "params": {
    "channels": [
      "orderbook.BTC-20260305-69500-P.10.10",
      "ticker_slim.BTC-20260305-69500-P.100"
    ]
  }
}
```
Source: DERIVE-API-FINDINGS.md -- live production capture 2026-03-04

### Live Derive Orderbook Message (CONFIRMED)
```json
{
  "method": "subscription",
  "params": {
    "channel": "orderbook.BTC-20260305-69500-P.10.10",
    "data": {
      "timestamp": 1772624842966,
      "instrument_name": "BTC-20260305-69500-P",
      "publish_id": 56593,
      "bids": [["340", "0.4"], ["320", "1"], ["280", "0.70343"]],
      "asks": [["420", "0.4"], ["520", "0.70343"]]
    }
  }
}
```
Source: DERIVE-API-FINDINGS.md -- live production capture 2026-03-04

### Live Derive ticker_slim Message (CONFIRMED)
```json
{
  "method": "subscription",
  "params": {
    "channel": "ticker_slim.BTC-20260305-69500-P.100",
    "data": {
      "timestamp": 1772624842966,
      "instrument_ticker": {
        "t": 1772624842966,
        "A": "0.4",
        "a": "414",
        "B": "0.4",
        "b": "341",
        "f": null,
        "option_pricing": {
          "d": "-0.24967",
          "t": "-453.85103",
          "g": "0.00013192",
          "v": "10.84014",
          "i": "0.70513",
          "r": "0.84114",
          "f": "71067",
          "m": "364",
          "df": "1",
          "bi": "0.68323",
          "ai": "0.75013"
        },
        "I": "71078",
        "M": "364",
        "stats": { "c": "1.3", "v": "91411.632", "pr": "787.353", "n": 2, "oi": "1.3", "h": "943.464", "l": "504.314", "p": "-0.465" },
        "minp": "4",
        "maxp": "1968"
      }
    }
  }
}
```
Source: DERIVE-API-FINDINGS.md -- live production capture 2026-03-04

### USDC-to-BTC Price Normalization Validation
```rust
// Test: Derive and Deribit implied probabilities for same strike/expiry should match within 5%
//
// Example: BTC $69,500 put, BTC index ~$71,000
//
// Deribit (inverse/BTC):
//   bid = 0.0048 BTC, ask = 0.0059 BTC
//   price_usd = 0.00535 * 71000 = $379.85
//
// Derive (linear/USDC):
//   bid = 341 USDC, ask = 414 USDC
//   price_usd = (341 + 414) / 2 = $377.50
//
// These are within 1% -- expected because they reference the same options market.
// If the delta is > 5%, there's a normalization bug.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ticker.{inst}.{interval}` channel | `ticker_slim.{inst}.{interval}` (abbreviated keys) | Pre-2026; `ticker` deprecated on Derive | Must use `ticker_slim` or get error `-32602` |
| Assumed Derive needed delta book processing | Confirmed snapshot-only (no deltas) | Live probe 2026-03-04 | Simpler book implementation, no sequence gap handling |
| Assumed prices were numeric | Confirmed all prices/amounts are strings | Live probe 2026-03-04 | Must deserialize as String, convert to Decimal |
| PricingEngine assumes all venues are BTC-inverse | Must gate conversion on venue | Phase 31 (new) | Derive prices are USDC; skip `price_btc * forward` |

## Key Differences: Derive vs Deribit Feed Implementation

| Aspect | Deribit (`src/feed/deribit/`) | Derive (new `src/feed/derive/`) |
|--------|------|--------|
| Subscribe method | `public/subscribe` | `subscribe` |
| Book model | Snapshot + delta (change_id sequencing) | Snapshot only (publish_id, no sequencing) |
| Heartbeat | Application-level (`set_heartbeat` + `test_request` response) | WS PING/PONG (auto-handled) |
| Ticker channel | `ticker.{inst}.raw` (full field names, f64 values) | `ticker_slim.{inst}.100` (abbreviated keys, string values) |
| Price format | `f64` (numeric JSON) | `String` (quoted JSON) |
| Price denomination | BTC (inverse contracts) | USDC (linear contracts) |
| Channel count | 4 per instrument (book, ticker, trades, price_index) | 2 per instrument (orderbook, ticker_slim) |
| Message types | Response, Heartbeat, Notification (3 variants) | Response, Notification (2 variants) |
| Book data fields | `change_id`, `prev_change_id`, `type`, `bids: [[f64, f64]]` | `publish_id`, `bids: [[String, String]]` |

## Open Questions

1. **PricingEngine venue gating approach**
   - What we know: `PricingEngine` currently filters `venue != Venue::Deribit` (skips non-Deribit snapshots) and applies `price_btc * forward` conversion. It must handle Derive snapshots too.
   - What's unclear: Whether to extend the existing `PricingEngine` to accept `Venue::Derive` in Phase 31, or defer to Phase 32 (pipeline integration). The success criterion says "MarketSnapshot emitted with USDC-normalized prices" and implies comparison with Deribit probabilities.
   - Recommendation: Add `parse_derive_instrument()` and venue-gated price conversion in Phase 31 so the validation test (5% tolerance) can run. This is a ~10-line change to `PricingEngine`.

2. **ticker_slim top-level `f` field vs nested `option_pricing.f`**
   - What we know: Live capture shows `"f": null` at the top level of `instrument_ticker` AND `"f": "71067"` inside `option_pricing`. They appear to be different fields (top-level `f` may be last trade funding rate, only relevant for perps).
   - What's unclear: Whether top-level `f` is ever non-null for options.
   - Recommendation: Ignore top-level `f` field. Use `option_pricing.f` for forward price. Both are `Option<String>` in the struct, so null handling is safe.

3. **CleanupEvent derive_instruments field**
   - What we know: `CleanupEvent` has `deribit_instruments`, `kalshi_tickers`, `polymarket_token_ids`. Derive needs `derive_instruments: Vec<String>`.
   - What's unclear: Whether to add it in Phase 31 or Phase 32 (subscription/pipeline integration).
   - Recommendation: Add the field in Phase 31 (it's a one-line struct change). The `DeriveProcessor` can start using it immediately for state cleanup, even if `SubscriptionManager` doesn't populate it until Phase 32.

## Sources

### Primary (HIGH confidence)
- `DERIVE-API-FINDINGS.md` -- 30 messages captured from production `wss://api.lyra.finance/ws` on 2026-03-04
- `src/feed/deribit/client.rs` -- Template for DeriveClient (247 lines)
- `src/feed/deribit/supervisor.rs` -- Template for DeriveSupervisor (206 lines)
- `src/feed/deribit/normalize.rs` -- Template for DeriveProcessor (1076 lines with tests)
- `src/feed/deribit/messages.rs` -- Template for DeriveMessage types (653 lines with tests)
- `src/feed/deribit/channels.rs` -- Template for DeriveChannelKind (253 lines with tests)
- `src/feed/deribit/book.rs` -- Template for DeriveBook (326 lines with tests)
- `src/pricing/engine.rs` -- Lines 229-232: BTC-inverse conversion that must be venue-gated
- `src/pricing/instrument.rs` -- Template for Derive instrument name parser

### Secondary (MEDIUM confidence)
- docs.derive.xyz/reference -- API documentation (partially inaccessible pages)
- CCXT Pro derive.py -- Channel format, book model, keepAlive configuration

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- zero new dependencies, all existing libraries
- Architecture: HIGH -- direct adaptation of proven Deribit feed pattern with simplifications
- Message format: HIGH -- all message structures confirmed from live production capture
- USDC normalization: HIGH -- price denomination confirmed, PricingEngine conversion logic identified
- Pitfalls: HIGH -- derived from live capture analysis and Deribit implementation experience

**Research date:** 2026-03-04
**Valid until:** 2026-04-04 (stable -- Derive API is production, message format unlikely to change)
