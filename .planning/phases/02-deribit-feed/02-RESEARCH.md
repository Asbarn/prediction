# Phase 02: Deribit Feed and Data Pipeline - Research

**Researched:** 2026-02-22
**Domain:** WebSocket market data feed, order book management, JSONL recording, mock data abstraction (Rust/Tokio)
**Confidence:** HIGH

## Summary

This phase implements an end-to-end data pipeline: Deribit WebSocket connection, JSON-RPC 2.0 message parsing, order book state management, normalized MarketSnapshot publishing through bounded async channels, raw message recording to JSONL, and a trait-based mock data layer. The existing codebase (Phase 1) provides domain types (`MarketSnapshot`, `Venue`, `Price`, `InstrumentId`, `DualTimestamp`), config loading (`DeribitConfig` with `ws_url`), CancellationToken-based shutdown, and a tracing/logging stack.

The Deribit WebSocket API uses JSON-RPC 2.0 over WSS. Subscription notifications arrive as `{"jsonrpc":"2.0","method":"subscription","params":{"channel":"...","data":{...}}}`. The `book.{instrument}.none.20.100ms` grouped channel delivers complete top-20 snapshots (not deltas requiring application), simplifying the book management to a replace-on-receive model with `change_id`/`prev_change_id` verification. The ticker channel provides greeks, mark price, index price, and implied volatility. Trades provide individual fill data. The `deribit_price_index.btc_usd` channel delivers the underlying BTC index price.

**Primary recommendation:** Use `tokio-tungstenite` 0.28 with `native-tls` for WSS, strongly-typed serde structs for all Deribit message types, `tokio::sync::mpsc` bounded channels for the normalization bus, a dedicated recording task using `tokio::sync::mpsc` with `try_send` (dropping on full buffer), and trait-based `DataSource`/`Recorder` abstractions from day one.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
- Public channels only in Phase 2 -- no authentication
- Single multiplexed WebSocket connection to Deribit, all instrument subscriptions over one connection
- Instrument list is dynamic -- comes from config in Phase 2
- Subscribe to 4 channel types: `book.{instrument}.none.20.100ms`, `ticker.{instrument}.raw`, `trades.{instrument}.raw`, `deribit_price_index.btc_usd`
- Top 20 levels only, using grouped `book.{instrument}.none.20.100ms` -- NOT the raw delta channel
- No delta application logic: each book message is a complete top-20 snapshot that replaces the previous state
- Strict `change_id` verification: every message's `prev_change_id` must match our last `change_id`
- On sequence gap: immediately mark instrument data as stale/unavailable downstream, then re-subscribe
- JSONL recording with BOTH raw WS frame AND parsed metadata per line
- Daily file rotation: one file per day per venue (e.g., `recordings/deribit/2026-02-22.jsonl`)
- Async recording with bounded buffer: pipeline never blocks on I/O; buffer overflow drops oldest unwritten messages
- Generic `Recorder` trait from day one
- Two mock data modes: Replay (from JSONL) and Synthetic (generated)
- Trait abstraction at two levels: WS message level and normalized snapshot level
- Configurable speed multiplier for replay: 1x, 0 (instant), 10x
- Accessible from integration tests (programmatic) and CLI (`--mock` or `--replay` flags)
- No reconnection logic, no heartbeat monitoring, no staleness detection (Phase 3)

### Claude's Discretion
- Exact channel message parsing implementation (serde structs vs manual JSON)
- Bounded channel buffer sizes for normalization bus and recording
- Internal thread/task architecture for the WS client
- File naming convention details for recordings beyond the daily pattern
- Synthetic data generation algorithm and default parameters

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope

</user_constraints>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio-tungstenite | 0.28 | WebSocket client (WSS) | De facto Tokio WebSocket library; 0.28 aligns with tungstenite 0.28; performance improvements post-0.26.2 |
| futures-util | 0.3 | Stream/Sink combinators (`StreamExt`, `SinkExt`) | Required by tokio-tungstenite for `split()`, `next()`, `send()`; already implicit in tokio-tungstenite deps |
| serde + serde_json | 1.0 (already in Cargo.toml) | JSON-RPC parsing, JSONL serialization | Already a project dependency; strongly-typed deserialization of Deribit messages |
| tokio | 1 (already in Cargo.toml) | Runtime, channels, file I/O, timers | Already a project dependency with `full` features |
| chrono | 0.4 (already in Cargo.toml) | Timestamp handling, daily file rotation | Already a project dependency |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio (fs, io) | 1 | Async file writing for JSONL recording | Already included via `full` feature |
| rand | 0.8 | Synthetic mock data generation | Only needed for synthetic order book generation |
| tracing | 0.1 (already in Cargo.toml) | Structured logging throughout pipeline | Already a project dependency |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| tokio-tungstenite | fastwebsockets | Slightly faster but less mature ecosystem; tokio-tungstenite 0.28+ closes the performance gap |
| native-tls feature | rustls-tls-native-roots | rustls is pure-Rust (no OpenSSL dependency) but native-tls uses platform TLS which is simpler on Windows |
| url crate | Raw string URLs | `connect_async` accepts `&str` directly via `IntoClientRequest` trait; no separate `url` crate needed |

**Installation (additions to Cargo.toml):**
```toml
# WebSocket
tokio-tungstenite = { version = "0.28", features = ["native-tls"] }
futures-util = { version = "0.3", default-features = false, features = ["sink"] }

# Mock data generation
rand = "0.8"
```

**Note on `futures-util`:** tokio-tungstenite depends on futures-util internally, but we need it explicitly for `StreamExt` and `SinkExt` traits on the WebSocket stream. The `sink` feature enables `SinkExt::send()`.

## Architecture Patterns

### Recommended Project Structure
```
src/
  feed/
    mod.rs              # pub mod deribit; pub mod mock; pub mod recording; pub mod traits;
    traits.rs           # DataSource, MessageSink, Recorder trait definitions
    deribit/
      mod.rs            # DeribitFeed struct, connect/subscribe/run loop
      messages.rs       # Serde structs for all Deribit JSON-RPC messages
      channels.rs       # Channel name builders, channel type parsing
      book.rs           # OrderBook state manager (replace-on-receive + change_id verification)
      normalize.rs      # Convert Deribit messages -> MarketSnapshot
    mock/
      mod.rs            # MockDataSource enum (Replay | Synthetic)
      replay.rs         # JSONL file replay with speed control
      synthetic.rs      # Synthetic order book/ticker generation
    recording/
      mod.rs            # RecordingService (spawns writer task)
      writer.rs         # Async JSONL writer with daily rotation
      types.rs          # RecordLine struct (raw + metadata)
  types/
    snapshot.rs         # MarketSnapshot (already exists, may need expansion)
```

### Pattern 1: Trait-Based DataSource Abstraction
**What:** A `DataSource` trait that unifies live WebSocket feeds and mock data behind a common interface.
**When to use:** Always -- all downstream code receives data through this trait, never directly from the WebSocket.
**Example:**
```rust
// Source: Architecture decision from CONTEXT.md

/// Raw WebSocket-level data source.
/// Produces text frames identical to Deribit format.
pub trait RawDataSource: Send + 'static {
    /// Stream of raw WebSocket text frames with receive timestamps.
    fn raw_messages(&mut self) -> impl Stream<Item = RawMessage> + Send;
}

/// Normalized data source.
/// Produces MarketSnapshot directly, bypassing WS parsing.
pub trait NormalizedDataSource: Send + 'static {
    fn snapshots(&mut self) -> impl Stream<Item = MarketSnapshot> + Send;
}

pub struct RawMessage {
    pub text: String,          // Exact WebSocket text frame
    pub received_at: DualTimestamp,
}
```

### Pattern 2: Task-Per-Concern Architecture
**What:** Separate tokio tasks for WS reading, message processing/normalization, recording, and downstream publishing.
**When to use:** Always -- isolation prevents one slow component from blocking others.
**Example:**
```rust
// Task architecture for the Deribit feed pipeline:
//
// [WS Reader Task] --raw frames--> mpsc(1024) --> [Processor Task]
//                                                    |
//                                  mpsc(4096) <------+-----> mpsc(256) --> [Recorder Task]
//                                  (to downstream)           (to disk)
//
// - WS Reader: reads from WebSocket stream, timestamps, sends raw frames
// - Processor: parses JSON-RPC, updates book state, normalizes, fans out
// - Recorder: receives raw frames, writes JSONL with buffered I/O
// - All tasks respect CancellationToken for graceful shutdown
```

### Pattern 3: Replace-on-Receive Book with change_id Verification
**What:** Instead of delta-application, the grouped book channel provides complete snapshots. Store the latest snapshot and verify sequence continuity via `change_id`/`prev_change_id`.
**When to use:** Always for `book.{instrument}.none.20.100ms` channel.
**Example:**
```rust
pub struct InstrumentBook {
    pub instrument: InstrumentId,
    pub bids: Vec<(Price, Notional)>,  // Top 20, descending by price
    pub asks: Vec<(Price, Notional)>,  // Top 20, ascending by price
    pub last_change_id: Option<i64>,
    pub timestamp: DualTimestamp,
    pub is_stale: bool,
}

impl InstrumentBook {
    pub fn apply_snapshot(&mut self, data: &BookData) -> Result<(), SequenceError> {
        // First message: no prev_change_id check needed
        if let Some(last_id) = self.last_change_id {
            if let Some(prev_id) = data.prev_change_id {
                if prev_id != last_id {
                    self.is_stale = true;
                    return Err(SequenceError::Gap {
                        expected: last_id,
                        got: prev_id,
                    });
                }
            }
        }
        // Replace entire book state
        self.bids = data.bids.iter().map(|&[p, a]| /* convert */).collect();
        self.asks = data.asks.iter().map(|&[p, a]| /* convert */).collect();
        self.last_change_id = Some(data.change_id);
        self.is_stale = false;
        Ok(())
    }
}
```

### Pattern 4: JSONL Recording with Bounded Buffer
**What:** A dedicated recording task that receives raw frames through a bounded channel and writes JSONL asynchronously. Uses `try_send` to drop messages on buffer overflow rather than blocking the pipeline.
**When to use:** Always -- recording must never slow down the data pipeline.
**Example:**
```rust
// Recording line format:
// {"raw":"<exact WS frame>","local_ts":"2026-02-22T15:30:00.123Z","venue":"deribit","channel":"book.BTC-27JUN25-100000-C.none.20.100ms","instrument":"BTC-27JUN25-100000-C"}

#[derive(Serialize)]
pub struct RecordLine {
    pub raw: String,
    pub local_ts: DateTime<Utc>,
    pub venue: Venue,
    pub channel: String,
    pub instrument: Option<String>,
}
```

### Anti-Patterns to Avoid
- **Blocking I/O on the hot path:** Never call `std::fs::write` or synchronous file I/O from the message processing loop. Always use the recording channel.
- **Unbounded channels:** An unbounded recording channel could consume all memory if disk I/O stalls. Always use bounded channels.
- **Parsing raw JSON twice:** Parse the Deribit message once into a typed struct, extract both the normalized data and the recording metadata from that parse. Keep the raw string separately for the recording `raw` field.
- **Global mutable book state:** Each instrument should have its own `InstrumentBook` instance, stored in a `HashMap<InstrumentId, InstrumentBook>`, not global state.
- **Mixing concerns in the WebSocket task:** The WS reader task should only read frames, timestamp them, and forward. Parsing and normalization happen in a separate task.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| WebSocket protocol | Custom WS framing/handshake | tokio-tungstenite | WS protocol has subtleties (masking, ping/pong, close frames, fragmentation) |
| TLS | Manual TLS setup | tokio-tungstenite `native-tls` feature | Platform TLS handles certificate validation, cipher suites, SNI |
| JSON-RPC 2.0 parsing | Manual string parsing | serde_json + typed structs with `#[serde(tag)]` or untagged enums | Edge cases in JSON parsing (Unicode escapes, number precision, null handling) |
| Async file I/O | `std::fs` + `spawn_blocking` | `tokio::fs` + `tokio::io::BufWriter` | tokio::fs already uses spawn_blocking internally with proper integration |
| Decimal arithmetic | f64 for prices | `rust_decimal::Decimal` (already in project) | f64 has rounding errors; Decimal preserves exact values for financial data |
| Daily file rotation | Custom date-checking logic | Check `chrono::Utc::now().format("%Y-%m-%d")` on each write, open new file when date changes | Simple approach, no external crate needed; tracing-appender pattern |

**Key insight:** The Deribit JSON-RPC messages have complex, nested structures with optional fields (greeks only on options, funding only on perpetuals). Typed serde structs catch schema mismatches at compile time and make the code self-documenting. Manual JSON parsing would be fragile and hard to maintain across 4+ channel types.

## Common Pitfalls

### Pitfall 1: TLS Feature Not Enabled
**What goes wrong:** `connect_async("wss://...")` compiles but panics at runtime with "TLS not available" or similar error.
**Why it happens:** tokio-tungstenite has no TLS backend enabled by default. Without `native-tls` or `rustls-tls-*` feature, it cannot connect to `wss://` URLs.
**How to avoid:** Always specify `features = ["native-tls"]` in Cargo.toml dependency.
**Warning signs:** Any connection to Deribit's `wss://www.deribit.com/ws/api/v2` failing immediately.

### Pitfall 2: WebSocket Ping/Pong Timeout
**What goes wrong:** Connection drops silently after ~30-60 seconds of apparent inactivity.
**Why it happens:** Deribit (and most WS servers) send ping frames. If the client doesn't respond with pong, the server closes the connection. tokio-tungstenite handles ping/pong automatically at the protocol level, but only if you're actively reading from the stream.
**How to avoid:** Ensure the read loop is always running. Never pause reading for extended periods. The reader task should be a simple loop that forwards all messages.
**Warning signs:** Connection drops after a fixed interval. No disconnect error logged (just stops receiving).

### Pitfall 3: JSON Number Precision Loss
**What goes wrong:** Prices like `0.0055` become `0.005499999999999999` when deserialized as f64.
**Why it happens:** IEEE 754 floating-point cannot represent all decimal fractions exactly. Deribit sends prices as JSON numbers.
**How to avoid:** Deserialize Deribit prices as `f64` in the serde struct (since Deribit sends them as JSON numbers, not strings), then immediately convert to `rust_decimal::Decimal` using `Decimal::try_from(f64_val)` or use serde's `deserialize_with` to go directly to Decimal. The existing project types (`Price`, `Notional`) use `Decimal` with `serde-with-str`, but Deribit sends numbers not strings, so the deserialization layer needs an adapter.
**Warning signs:** Prices that don't match what the exchange shows. Assertion failures in tests comparing expected vs actual prices.

### Pitfall 4: Forgetting to Handle the First Book Message
**What goes wrong:** The first book message after subscribe has no `prev_change_id` (or it's 0/null). If the verification logic requires `prev_change_id` to match, the first message is rejected.
**Why it happens:** The first grouped book message is implicitly a snapshot. It establishes the initial `change_id` but has no predecessor.
**How to avoid:** When `last_change_id` is `None` (no previous state), accept the message unconditionally and store its `change_id`. Only verify `prev_change_id` on subsequent messages.
**Warning signs:** Every instrument immediately marked as stale after subscription.

### Pitfall 5: Channel Backpressure Stalling the Pipeline
**What goes wrong:** If the normalization channel or recording channel is full and you use `.send().await`, the entire pipeline stalls waiting for the slow consumer.
**Why it happens:** `mpsc::Sender::send().await` blocks the sending task when the channel is full. For recording, this means disk I/O speed controls the data pipeline.
**How to avoid:** Use `try_send()` for the recording channel -- drop messages rather than block. For the normalization bus, use a reasonably sized buffer (256-1024) and `send().await` is acceptable since downstream should be fast.
**Warning signs:** Pipeline latency spikes correlated with disk I/O. Recording channel consistently full.

### Pitfall 6: Recording Channel "Drop Oldest" is Not Built-In
**What goes wrong:** The CONTEXT.md specifies "buffer overflow drops oldest unwritten messages." `tokio::sync::mpsc` does NOT support dropping the oldest message -- `try_send` drops the **newest** (the one being sent), not the oldest.
**Why it happens:** `mpsc::try_send()` returns `Err(TrySendError::Full(value))` -- the value that failed to send (newest) is returned, not the oldest in the buffer.
**How to avoid:** For the recording channel, `try_send` dropping the newest message on overflow is the pragmatic choice. The semantic difference is minimal: under sustained overflow, both strategies lose data. Dropping newest is simpler and avoids the complexity of a ring buffer. Alternatively, `tokio::sync::broadcast` drops the oldest (receiver gets `Lagged` error), but it's multi-consumer which adds overhead. **Recommendation: Use `mpsc` with `try_send`, dropping newest on overflow. Document this as an acceptable approximation of the "drop oldest" requirement.** If exact drop-oldest is needed, consider the `ring-channel` crate (lock-free ring buffer), but this adds a dependency for marginal benefit.
**Warning signs:** None -- the failure mode is equivalent under sustained load.

### Pitfall 7: Deribit Book Data Format Confusion
**What goes wrong:** Parsing the `book.{instrument}.none.20.100ms` channel as if it uses the raw delta format (`["new"|"change"|"delete", price, amount]` tuples) when it actually sends complete snapshots as `[[price, amount], ...]` arrays.
**Why it happens:** Deribit has two book channel formats: (1) raw (`book.{instrument}.raw`) sends deltas with action tuples, and (2) grouped (`book.{instrument}.{group}.{depth}.{interval}`) sends complete snapshots with plain `[price, amount]` pairs. The documentation mixes these.
**How to avoid:** The grouped channel (`none.20.100ms`) sends data with a `type` field ("snapshot" or "change"). Despite the "change" type name, the bids/asks arrays in the grouped channel STILL contain action tuples `["new"|"change"|"delete", price, amount]` for change messages, but the first message is always type "snapshot" with plain `[[price, amount], ...]` arrays. Since each grouped message at the `none.20.100ms` interval contains the full top-20 state as a snapshot, we should primarily handle the snapshot format. **Verify against live data by connecting to `wss://test.deribit.com/ws/api/v2` (testnet) during development.**
**Warning signs:** Deserialization errors on the first message received after subscribe.

### Pitfall 8: Rust 2024 Edition + MSRV Resolver
**What goes wrong:** Cargo resolves to older dependency versions than expected.
**Why it happens:** Rust 2024 edition (which the project uses, `edition = "2024"`, `rust-version = "1.85"`) enables the MSRV-aware resolver by default. This means Cargo prefers dependency versions compatible with your declared `rust-version`, potentially selecting older versions.
**How to avoid:** This is generally beneficial. But if a needed feature is only in a newer version, you may need to bump `rust-version`. Current deps (tokio 1, serde 1, tokio-tungstenite 0.28) should all be compatible with Rust 1.85.
**Warning signs:** `cargo update` selecting unexpectedly old versions.

## Code Examples

### Deribit JSON-RPC 2.0 Message Types (Serde Structs)

```rust
// Source: Deribit API docs (https://docs.deribit.com/articles/json-rpc-overview)
// Cross-verified with: Go tradekit library, Tardis TypeScript mappers

use serde::{Deserialize, Serialize};

/// Top-level JSON-RPC message from Deribit.
/// Could be a response to our request or a subscription notification.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DeribitMessage {
    /// Response to a request we sent (has `id` field)
    Response(RpcResponse),
    /// Subscription notification (has `method: "subscription"`)
    Notification(RpcNotification),
}

#[derive(Debug, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,  // Always "subscription"
    pub params: NotificationParams,
}

#[derive(Debug, Deserialize)]
pub struct NotificationParams {
    pub channel: String,
    pub data: serde_json::Value,  // Parsed further based on channel type
}
```

### Deribit Channel Data Structures

```rust
// Source: Deribit API docs, verified against Go tradekit
// (https://pkg.go.dev/github.com/antibubblewrap/tradekit/deribit)
// and Tardis TypeScript mappers

/// Book channel: book.{instrument}.none.20.100ms
/// First message is type "snapshot", subsequent are "change".
/// For grouped channel, snapshot bids/asks are [[price, amount], ...].
#[derive(Debug, Deserialize)]
pub struct BookData {
    pub timestamp: i64,                     // ms since epoch
    pub instrument_name: String,
    pub change_id: i64,
    #[serde(default)]
    pub prev_change_id: Option<i64>,        // None on first snapshot
    #[serde(rename = "type")]
    pub update_type: Option<String>,        // "snapshot" or "change"
    pub bids: Vec<BookLevel>,               // See note below
    pub asks: Vec<BookLevel>,
}

/// A book level in the grouped channel.
/// Snapshots: [price, amount]  (2-element array)
/// Changes:   [action, price, amount]  (3-element array, action is string)
/// Use serde untagged or custom deserializer to handle both.
#[derive(Debug)]
pub enum BookLevel {
    Snapshot { price: f64, amount: f64 },
    Change { action: String, price: f64, amount: f64 },
}

/// Ticker channel: ticker.{instrument}.raw
#[derive(Debug, Deserialize)]
pub struct TickerData {
    pub timestamp: i64,
    pub instrument_name: String,
    pub state: String,
    pub last_price: Option<f64>,
    pub mark_price: f64,
    pub index_price: f64,
    pub best_bid_price: Option<f64>,
    pub best_bid_amount: Option<f64>,
    pub best_ask_price: Option<f64>,
    pub best_ask_amount: Option<f64>,
    pub open_interest: f64,
    pub min_price: f64,
    pub max_price: f64,
    // Options-specific fields
    pub underlying_price: Option<f64>,
    pub underlying_index: Option<String>,
    pub mark_iv: Option<f64>,
    pub bid_iv: Option<f64>,
    pub ask_iv: Option<f64>,
    pub interest_rate: Option<f64>,
    pub greeks: Option<TickerGreeks>,
    // Perpetual-specific fields
    pub funding_8h: Option<f64>,
    pub current_funding: Option<f64>,
    // Stats
    pub stats: Option<TickerStats>,
    pub estimated_delivery_price: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct TickerGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}

#[derive(Debug, Deserialize)]
pub struct TickerStats {
    pub volume: Option<f64>,
    pub volume_usd: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub price_change: Option<f64>,
}

/// Trades channel: trades.{instrument}.raw
/// Data is an ARRAY of trades (multiple trades per notification).
#[derive(Debug, Deserialize)]
pub struct TradeData {
    pub trade_id: String,
    pub instrument_name: String,
    pub timestamp: i64,
    pub direction: String,              // "buy" or "sell"
    pub price: f64,
    pub amount: f64,
    pub trade_seq: i64,
    pub tick_direction: Option<i32>,    // 0-3
    pub liquidation: Option<String>,    // "M", "T", or "MT"
    pub mark_price: Option<f64>,
    pub index_price: Option<f64>,
    pub iv: Option<f64>,                // Options only: implied volatility
}

/// Price index channel: deribit_price_index.btc_usd
#[derive(Debug, Deserialize)]
pub struct PriceIndexData {
    pub timestamp: i64,
    pub price: f64,
    pub index_name: String,             // "btc_usd"
}
```

### WebSocket Connection and Subscription

```rust
// Source: tokio-tungstenite docs (https://docs.rs/tokio-tungstenite/0.28)
// and Deribit API docs (https://docs.deribit.com/api-reference/subscription-management/public-subscribe)

use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};

async fn connect_and_subscribe(
    ws_url: &str,
    instruments: &[String],
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, anyhow::Error> {
    let (ws_stream, _response) = connect_async(ws_url).await?;
    let (mut write, read) = ws_stream.split();

    // Build channel list from instruments
    let mut channels: Vec<String> = Vec::new();
    for inst in instruments {
        channels.push(format!("book.{}.none.20.100ms", inst));
        channels.push(format!("ticker.{}.raw", inst));
        channels.push(format!("trades.{}.raw", inst));
    }
    channels.push("deribit_price_index.btc_usd".to_string());

    // Subscribe via JSON-RPC 2.0
    let subscribe_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "public/subscribe",
        "params": {
            "channels": channels
        }
    });

    write.send(Message::Text(subscribe_msg.to_string())).await?;
    // ... handle response and start reading notifications
    Ok(todo!()) // Illustrative
}
```

### JSONL Recording Writer

```rust
// Source: Architecture pattern for async file writing with daily rotation

use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};

pub struct JsonlWriter {
    base_dir: PathBuf,
    venue: Venue,
    current_date: String,
    writer: Option<BufWriter<File>>,
}

impl JsonlWriter {
    pub async fn write_line(&mut self, line: &RecordLine) -> std::io::Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        if self.current_date != today || self.writer.is_none() {
            self.rotate(&today).await?;
        }
        let json = serde_json::to_string(line)?;
        if let Some(w) = &mut self.writer {
            w.write_all(json.as_bytes()).await?;
            w.write_all(b"\n").await?;
            w.flush().await?; // or periodic flush for performance
        }
        Ok(())
    }

    async fn rotate(&mut self, date: &str) -> std::io::Result<()> {
        // Path: recordings/deribit/2026-02-22.jsonl
        let dir = self.base_dir.join(self.venue.to_string());
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{}.jsonl", date));
        let file = OpenOptions::new()
            .create(true).append(true).open(&path).await?;
        self.writer = Some(BufWriter::new(file));
        self.current_date = date.to_string();
        Ok(())
    }
}
```

### Recording Channel with try_send (Drop Newest on Overflow)

```rust
// Source: tokio docs (https://docs.rs/tokio/latest/tokio/sync/mpsc)

use tokio::sync::mpsc;

const RECORDING_BUFFER_SIZE: usize = 8192;

// Producer side (in processor task):
fn record_message(tx: &mpsc::Sender<RecordLine>, line: RecordLine) {
    match tx.try_send(line) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!("recording buffer full, dropping message");
            // Increment a counter metric for monitoring
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::error!("recording channel closed");
        }
    }
}

// Consumer side (recording task):
async fn recording_task(
    mut rx: mpsc::Receiver<RecordLine>,
    mut writer: JsonlWriter,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            Some(line) = rx.recv() => {
                if let Err(e) = writer.write_line(&line).await {
                    tracing::error!(error = %e, "failed to write recording line");
                }
            }
            _ = cancel.cancelled() => {
                // Drain remaining messages before exit
                while let Ok(line) = rx.try_recv() {
                    let _ = writer.write_line(&line).await;
                }
                break;
            }
        }
    }
}
```

### Mock Data Replay

```rust
// Source: Architecture decision from CONTEXT.md

use tokio::time::{sleep, Duration, Instant};

pub struct ReplayDataSource {
    lines: Vec<RecordLine>,
    speed: f64,  // 0 = instant, 1.0 = real-time, 10.0 = 10x
}

impl ReplayDataSource {
    pub async fn replay(&self, tx: mpsc::Sender<RawMessage>) -> anyhow::Result<()> {
        let mut prev_ts: Option<DateTime<Utc>> = None;

        for line in &self.lines {
            if self.speed > 0.0 {
                if let Some(prev) = prev_ts {
                    let delta = (line.local_ts - prev)
                        .to_std()
                        .unwrap_or(Duration::ZERO);
                    let scaled = delta.div_f64(self.speed);
                    sleep(scaled).await;
                }
            }
            prev_ts = Some(line.local_ts);

            let msg = RawMessage {
                text: line.raw.clone(),
                received_at: DualTimestamp::now(),
            };
            tx.send(msg).await?;
        }
        Ok(())
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `book.{inst}.raw` delta application | `book.{inst}.none.20.100ms` grouped snapshots | Always available, user decision | Eliminates delta logic, massively simplifies book management |
| Manual TLS configuration | tokio-tungstenite feature flags | v0.20+ | One line in Cargo.toml enables TLS |
| f64 for financial values | `rust_decimal::Decimal` | Project convention from Phase 1 | Exact decimal arithmetic, no rounding surprises |
| Single-threaded WS loop | Multi-task pipeline with channels | Tokio ecosystem maturity | Backpressure, isolation, graceful shutdown |
| tokio-tungstenite <0.26 | tokio-tungstenite 0.28 | 2024-2025 | Significant performance improvements, parity with fastwebsockets |

**Deprecated/outdated:**
- `websocket` crate: Abandoned. Use tokio-tungstenite.
- `tungstenite::connect()` (sync): Use `connect_async()` for tokio integration.
- Deribit API v1: Long deprecated. All examples use v2 (`/ws/api/v2`).

## Deribit API Specifics

### Subscription Rate Limit
The `public/subscribe` endpoint has a sustained rate of ~3.3 requests/second. Batch all channels into a single subscribe call rather than subscribing one-by-one. This is both faster and avoids rate limiting.

### Testnet for Development
Deribit provides a testnet at `wss://test.deribit.com/ws/api/v2` with the same API. Use this for development and integration testing without risking real connections.

### JSON-RPC ID Management
Each outgoing request needs a unique `id` (integer). Responses correlate by `id`. Subscription notifications have NO `id` field -- use the `method: "subscription"` field to distinguish notifications from responses. A simple atomic counter works for ID generation.

### Notification Envelope
All subscription notifications follow this exact shape:
```json
{
  "jsonrpc": "2.0",
  "method": "subscription",
  "params": {
    "channel": "<channel_name>",
    "data": { ... }
  }
}
```
The `channel` string determines how to parse `data`. Parse the channel name to route to the correct typed deserializer.

### Trades Channel Returns Arrays
Unlike book and ticker which send a single data object, the trades channel sends an **array** of trade objects in `data`. A single notification can contain multiple trades that occurred in the same 100ms window (for raw channel, it's every trade).

## Channel Buffer Size Recommendations

| Channel | Recommended Size | Rationale |
|---------|-----------------|-----------|
| WS reader -> Processor | 1024 | Raw frames are small (~1-5KB); buffer absorbs parsing latency spikes |
| Processor -> Downstream (normalization bus) | 256 | MarketSnapshots are small; downstream should consume quickly |
| Processor -> Recorder | 8192 | Recording is I/O-bound; large buffer absorbs disk write latency; uses `try_send` so overflow drops newest |

These are starting points. Profile under load and adjust. The system should log channel utilization metrics.

## Open Questions

1. **Grouped book channel exact format for "change" type messages**
   - What we know: The first message is type "snapshot" with `[[price, amount], ...]` arrays. The Go tradekit library models changes with action tuples `["new"|"change"|"delete", price, amount]`.
   - What's unclear: Whether the `none.20.100ms` grouped channel ever sends "change" type messages or always sends complete "snapshot" type. Some evidence suggests grouped channels at intervals always send full snapshots.
   - Recommendation: Connect to Deribit testnet early and observe actual message types. Design the serde structs to handle both formats. Given the user decision to "replace the previous state" on each message, even if changes arrive, we only need the resulting state.

2. **Deribit `change_id` on grouped channel snapshots**
   - What we know: The raw channel definitely uses `change_id`/`prev_change_id` for sequence verification. The grouped channel includes `change_id` in the data.
   - What's unclear: Whether `prev_change_id` is present on every grouped message or only on the first. The Go OrderbookDepth struct (for grouped) does NOT include `PrevChangeId`, only `ChangeId`.
   - Recommendation: Make `prev_change_id` optional (`Option<i64>`) in the serde struct. Verify behavior against testnet. If grouped snapshots don't include `prev_change_id`, sequence verification becomes: check that each new `change_id` is strictly greater than the previous.

3. **Price precision: f64 from Deribit -> Decimal conversion**
   - What we know: Deribit sends prices as JSON numbers (not strings). `Decimal::try_from(f64)` can lose precision.
   - What's unclear: Whether Deribit prices always have limited decimal places that survive f64 round-trip.
   - Recommendation: Deserialize as `serde_json::Number` or use `#[serde(deserialize_with = "...")]` to parse the raw JSON number string directly into `Decimal`, bypassing f64 entirely. The `serde_json` crate's `Number` type preserves the original text representation.

## Sources

### Primary (HIGH confidence)
- [Deribit API Documentation](https://docs.deribit.com/) - JSON-RPC overview, subscription management, ticker fields
- [Deribit JSON-RPC Overview](https://docs.deribit.com/articles/json-rpc-overview) - Request/response/notification format
- [Deribit public/subscribe](https://docs.deribit.com/api-reference/subscription-management/public-subscribe.md) - Subscribe request format, rate limit
- [Deribit public/ticker](https://docs.deribit.com/api-reference/market-data/public-ticker) - Complete ticker field list with greeks
- [Deribit public/get_order_book](https://docs.deribit.com/api-reference/market-data/public-get_order_book) - Order book fields, bids/asks format
- [tokio-tungstenite 0.28 docs](https://docs.rs/tokio-tungstenite/0.28) - connect_async signature, WebSocketStream API
- [tokio-tungstenite GitHub](https://github.com/snapview/tokio-tungstenite) - Client example, feature flags
- [tokio::sync::mpsc docs](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) - Bounded channel, send/try_send semantics
- [Go tradekit Deribit structs](https://pkg.go.dev/github.com/antibubblewrap/tradekit/deribit) - OrderbookUpdate, OrderbookDepth, PublicTrade field definitions

### Secondary (MEDIUM confidence)
- [Tardis Deribit TypeScript mappers](https://github.com/tardis-dev/tardis-node/blob/master/src/mappers/deribit.ts) - DeribitBookMessage, DeribitTickerMessage, DeribitTradesMessage type definitions
- [Deribit Rust crate (dovahcrow)](https://github.com/dovahcrow/deribit-rs) - Architecture pattern: DeribitAPIClient + DeribitSubscriptionClient split
- [tokio broadcast channel docs](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html) - Drop-oldest overflow behavior
- [ring-channel crate](https://docs.rs/ring-channel/latest/ring_channel/) - Lock-free ring buffer channel alternative

### Tertiary (LOW confidence)
- Exact format of grouped book "change" type messages - needs testnet verification
- Whether `prev_change_id` is present on all grouped channel messages - needs testnet verification

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - tokio-tungstenite 0.28 is well-documented, widely used, confirmed version and features
- Architecture: HIGH - task-per-concern with bounded channels is established Tokio pattern; trait-based abstraction is standard Rust
- Deribit message types: HIGH for ticker/trades/price_index (verified against multiple sources), MEDIUM for grouped book format (snapshot vs change type ambiguity)
- Pitfalls: HIGH - verified against official docs, community sources, and library documentation
- Recording pipeline: HIGH - standard tokio async I/O patterns; `try_send` behavior verified against tokio docs
- Mock data layer: MEDIUM - design is sound but implementation details (synthetic generation, replay timing) are discretionary

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days; Deribit API is stable, library versions unlikely to change significantly)
