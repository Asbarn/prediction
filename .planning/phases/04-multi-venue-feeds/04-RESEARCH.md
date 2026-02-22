# Phase 4: Multi-Venue Feeds - Research

**Researched:** 2026-02-22
**Domain:** Polymarket CLOB WebSocket, Kalshi REST/WebSocket, multi-feed fan-in, graceful degradation
**Confidence:** MEDIUM (Polymarket HIGH, Kalshi MEDIUM, fan-in architecture HIGH)

## Summary

Phase 4 adds Polymarket and Kalshi as venue feeds alongside the existing Deribit feed. Polymarket uses a public WebSocket at `wss://ws-subscriptions-clob.polymarket.com/ws/market` that delivers full order book snapshots (`book` events) with prices in 0-1 probability space -- no authentication needed for market data. Kalshi uses a WebSocket at `wss://api.elections.kalshi.com/trade-api/ws/v2` that requires RSA-PSS authentication on the handshake and delivers `orderbook_snapshot` followed by incremental `orderbook_delta` messages with prices in cents (1-99). Both venues' data must be normalized into the existing `MarketSnapshot` format and published through a shared `mpsc` channel.

The existing architecture is well-suited for this. Each venue already follows the `RawDataSource -> Processor -> MarketSnapshot` pattern. The key architectural question is the fan-in: multiple venue processors publish `MarketSnapshot` events into a single bounded channel that downstream consumers read. The simplest approach is a shared `mpsc::Sender<MarketSnapshot>` (cloned per venue), which Tokio's mpsc natively supports. Graceful degradation means each venue's supervisor runs independently with its own `CancellationToken` child, and feed health is tracked per-venue with metrics.

**Primary recommendation:** Build custom WebSocket clients for both venues (matching the Deribit pattern), use `tokio::sync::mpsc` for fan-in with a shared sender, and implement per-venue health tracking for graceful degradation.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio-tungstenite | 0.28 | WebSocket client (already in Cargo.toml) | Same library as Deribit client, supports custom headers via `IntoClientRequest` |
| rsa | 0.9 | RSA-PSS signing for Kalshi auth | RustCrypto ecosystem standard, pure-Rust, supports PSS with SHA256 |
| sha2 | 0.10 | SHA-256 for Kalshi signature digest | RustCrypto companion to `rsa` crate |
| base64 | 0.22 | Base64 encoding of Kalshi signatures | Standard base64 crate |
| reqwest | 0.12 | HTTP client for Kalshi REST polling fallback and Polymarket Gamma API | Async HTTP with TLS, widely used |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio-stream | 0.1 | `ReceiverStream` adapter (optional) | Only if stream-based fan-in preferred over raw mpsc |
| metrics | 0.24 | Feed health metrics (already in Cargo.toml) | Per-venue gauges and counters for degradation visibility |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom WS client | polymarket-client-sdk (ws feature) | Adds heavy dependency chain (alloy, etc.); we only need market channel read |
| Custom WS client | kalshi-trade-rs | Has WS + RSA-PSS, but version 0.2.0 and unclear maintenance; easier to own the 200-line client |
| Custom Kalshi client | kalshi crate (0.9.0) | WebSocket support explicitly incomplete per docs; REST only |
| rsa crate | ring | ring uses C/asm, not pure-Rust; rsa crate integrates better with RustCrypto ecosystem |
| reqwest for Kalshi REST | hyper | reqwest is higher-level, already needed; hyper adds complexity |

**Installation:**
```toml
# Add to Cargo.toml [dependencies]
rsa = { version = "0.9", features = ["sha2"] }
sha2 = "0.10"
base64 = "0.22"
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── feed/
│   ├── deribit/           # Existing -- DeribitClient, DeribitProcessor, supervisor
│   ├── polymarket/        # NEW -- PolymarketClient, PolymarketProcessor, supervisor
│   │   ├── mod.rs
│   │   ├── client.rs      # WebSocket client (market channel)
│   │   ├── messages.rs    # Serde types for WS events (book, price_change)
│   │   ├── normalize.rs   # PolymarketProcessor: book events -> MarketSnapshot
│   │   └── supervisor.rs  # Reconnection supervisor (reuse backoff pattern)
│   ├── kalshi/            # NEW -- KalshiClient, KalshiProcessor, supervisor
│   │   ├── mod.rs
│   │   ├── auth.rs        # RSA-PSS signing helper
│   │   ├── client.rs      # WebSocket client with auth headers
│   │   ├── messages.rs    # Serde types for WS events (orderbook_snapshot, orderbook_delta)
│   │   ├── book.rs        # Incremental book management (apply deltas to snapshot)
│   │   ├── normalize.rs   # KalshiProcessor: book state -> MarketSnapshot
│   │   └── supervisor.rs  # Reconnection supervisor
│   ├── fanin.rs           # NEW -- Multi-venue fan-in coordinator
│   ├── health.rs          # NEW -- Per-venue health tracker (RELY-04)
│   ├── pipeline.rs        # MODIFY -- Expand to multi-venue pipeline assembly
│   ├── traits.rs          # Existing -- RawDataSource, Recorder, etc.
│   ├── mock/              # Existing
│   ├── recording/         # Existing
│   └── reliability/       # Existing -- VenueRateLimiter
└── config/
    └── venues.rs          # Existing -- already has PolymarketConfig, KalshiConfig stubs
```

### Pattern 1: Shared mpsc::Sender Fan-In
**What:** All venue processors write MarketSnapshot to the same `mpsc::Sender<MarketSnapshot>` (cloned per venue). Downstream consumers read from the single `Receiver`.
**When to use:** When all producers emit the same type and ordering between venues is irrelevant (first-come-first-served).
**Why this over StreamMap:** StreamMap requires wrapping receivers as Streams. A shared Sender is simpler, requires no additional crate, and is what mpsc was designed for (multi-producer, single-consumer).

```rust
// Fan-in: create one channel, clone sender to each venue
let (snapshot_tx, snapshot_rx) = mpsc::channel::<MarketSnapshot>(1024);

// Each venue processor gets a clone
let deribit_tx = snapshot_tx.clone();
let polymarket_tx = snapshot_tx.clone();
let kalshi_tx = snapshot_tx.clone();
drop(snapshot_tx); // Drop original so channel closes when all producers done

// Spawn each venue pipeline independently
tokio::spawn(deribit_pipeline(deribit_tx, cancel.child_token()));
tokio::spawn(polymarket_pipeline(polymarket_tx, cancel.child_token()));
tokio::spawn(kalshi_pipeline(kalshi_tx, cancel.child_token()));

// Downstream reads from single receiver
while let Some(snapshot) = snapshot_rx.recv().await {
    // Process snapshot from ANY venue identically
}
```

### Pattern 2: Per-Venue Supervisor Independence (RELY-04)
**What:** Each venue runs in its own supervisor task with its own child `CancellationToken`. A feed drop in one venue does not propagate to others.
**When to use:** Always for multi-venue systems -- isolation is non-negotiable.

```rust
// Each venue has an independent supervisor loop
struct VenueSupervisor {
    venue: Venue,
    cancel: CancellationToken,  // Child of global cancel
    health: Arc<VenueHealth>,
}

// A venue failure marks it unavailable but does NOT cancel other venues
impl VenueSupervisor {
    async fn run(self, tx: mpsc::Sender<MarketSnapshot>) {
        loop {
            match self.connect_and_stream(&tx).await {
                Ok(()) => { /* Clean shutdown */ }
                Err(e) => {
                    self.health.mark_unavailable(e.to_string());
                    metrics::gauge!("feed_available", "venue" => self.venue.to_string()).set(0.0);
                    // Backoff and retry -- same pattern as DeribitSupervisor
                }
            }
        }
    }
}
```

### Pattern 3: Polymarket Price-is-Probability Normalization
**What:** Polymarket prices ARE probabilities (0.0 to 1.0). Direct mapping to `bid_probability`/`ask_probability` on MarketSnapshot.
**When to use:** All Polymarket data processing.

```rust
// Polymarket book event: prices are strings "0.50" = 50% probability
fn normalize_polymarket_book(book: &PolymarketBook) -> MarketSnapshot {
    let best_bid = book.bids.first();
    let best_ask = book.asks.first();

    MarketSnapshot {
        venue: Venue::Polymarket,
        // Price fields map directly from probability-space prices
        bid: best_bid.map(|b| Price::new(parse_decimal(&b.price))),
        ask: best_ask.map(|a| Price::new(parse_decimal(&a.price))),
        // Probability fields are identical to prices for prediction markets
        bid_probability: best_bid.map(|b| Probability::new(parse_decimal(&b.price)).unwrap()),
        ask_probability: best_ask.map(|a| Probability::new(parse_decimal(&a.price)).unwrap()),
        // ...
    }
}
```

### Pattern 4: Kalshi Cents-to-Probability Normalization
**What:** Kalshi prices are in cents (1-99). YES bid at 42 cents = 0.42 probability. A YES bid at X implies a NO ask at (100-X).
**When to use:** All Kalshi data processing.

```rust
// Kalshi: prices in cents (1-99), convert to probability (0.01-0.99)
fn cents_to_probability(cents: i64) -> Decimal {
    Decimal::new(cents, 2)  // 42 -> 0.42
}

// Kalshi orderbook only returns bids; asks are derived
// YES ask = 100 - best NO bid
// NO ask = 100 - best YES bid
fn normalize_kalshi_orderbook(yes_bids: &[(i64, i64)], no_bids: &[(i64, i64)]) -> (/* bids */, /* asks */) {
    let best_yes_bid = yes_bids.last(); // Sorted ascending, best is last
    let best_no_bid = no_bids.last();

    let yes_ask_price = best_no_bid.map(|(price, _)| 100 - price);
    let no_ask_price = best_yes_bid.map(|(price, _)| 100 - price);
    // ...
}
```

### Pattern 5: Kalshi Incremental Book Management
**What:** Kalshi sends a full `orderbook_snapshot` first, then `orderbook_delta` updates. Deltas modify the local book state incrementally.
**When to use:** When processing Kalshi WebSocket data (distinct from Deribit's grouped snapshots).

```rust
// Delta application: update a single price level
fn apply_delta(book: &mut BTreeMap<i64, i64>, price: i64, delta: i64) {
    let entry = book.entry(price).or_insert(0);
    *entry += delta;
    if *entry <= 0 {
        book.remove(&price);
    }
}
```

### Anti-Patterns to Avoid
- **Shared mutable state across venues:** Each venue's book state is owned by its processor task. No `Arc<Mutex<>>` on book state.
- **Cascading cancellation on feed drop:** A Polymarket disconnect must NOT cancel Deribit or Kalshi. Use child `CancellationToken`s.
- **Using third-party SDK crates for thin WS clients:** The Polymarket and Kalshi WebSocket protocols are simple enough that a 200-line client following the existing Deribit pattern is lower-risk than taking on complex SDK dependencies.
- **Polling when WebSocket is available:** Kalshi has WebSocket support with orderbook_delta; prefer it over REST polling. REST polling is only a fallback if WebSocket proves unreliable.
- **Blocking on auth signing:** RSA-PSS signing is CPU-bound; use `tokio::task::spawn_blocking` or keep it synchronous in the connection setup path (before async loop).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| RSA-PSS signing | Custom crypto | `rsa` crate + `sha2` | Cryptographic correctness is non-trivial; RustCrypto is audited |
| Exponential backoff | Custom retry loops | `backoff` crate (already used) | Jitter, max_interval, max_elapsed_time handled correctly |
| Rate limiting | Token bucket | `governor` crate (already used) | Thread-safe, async-ready, well-tested |
| Decimal parsing | `f64::parse` | `rust_decimal` (already used) | Polymarket sends string decimals; Decimal::from_str is exact |
| WebSocket framing | Raw TCP | `tokio-tungstenite` (already used) | Handles ping/pong, close frames, TLS transparently |
| Base64 encoding | Manual | `base64` crate | Standard, correct, zero-copy option |

**Key insight:** The venue clients are thin WebSocket wrappers (connect, subscribe, read frames). The complexity is in normalization and state management, not in the transport layer. Reuse the proven transport libraries.

## Common Pitfalls

### Pitfall 1: Polymarket Token ID vs Condition ID Confusion
**What goes wrong:** Subscribing with condition IDs on the market channel (which expects asset/token IDs), resulting in no data.
**Why it happens:** Polymarket has two ID systems: condition IDs (market-level) and token/asset IDs (outcome-level, YES/NO). The market WebSocket channel requires `assets_ids` (token IDs), NOT condition IDs.
**How to avoid:** Use the Gamma API (`GET https://gamma-api.polymarket.com/markets?condition_id=X`) to resolve condition IDs to token IDs at startup. Store the mapping in config.
**Warning signs:** WebSocket connects successfully but no `book` events arrive.

### Pitfall 2: Kalshi Orderbook Only Returns Bids
**What goes wrong:** Treating Kalshi's YES array as bids and looking for a separate asks array, finding none.
**Why it happens:** Kalshi's binary market model means a YES bid at price X is equivalent to a NO ask at (100-X). The API returns only bid-side data for both YES and NO.
**How to avoid:** Derive asks from the complementary side. Best YES ask = 100 - (best NO bid price). Best NO ask = 100 - (best YES bid price).
**Warning signs:** Ask side always empty or wrong; spread calculations nonsensical.

### Pitfall 3: Kalshi Price Arrays Sorted Ascending (Best is Last)
**What goes wrong:** Assuming best bid is at index 0 (like Polymarket/Deribit).
**Why it happens:** Kalshi sorts price arrays in ascending order, so the highest bid (best) is the LAST element.
**How to avoid:** Access best bid via `.last()` not `.first()`. Or reverse the array during normalization.
**Warning signs:** Best bid appears to be 1 cent; spread is enormous.

### Pitfall 4: Kalshi RSA-PSS Signature Timestamp in Milliseconds
**What goes wrong:** Using seconds instead of milliseconds for the timestamp in the signing message and header.
**Why it happens:** Most REST APIs use seconds. Kalshi uses milliseconds.
**How to avoid:** Use `chrono::Utc::now().timestamp_millis()` for the timestamp. The signing message is `"{timestamp_ms}{METHOD}{path}"`.
**Warning signs:** 401 Unauthorized on every request despite correct key.

### Pitfall 5: Polymarket Book Events Arrive as JSON Arrays
**What goes wrong:** Parsing a single JSON object when the WebSocket actually sends a JSON array of events.
**Why it happens:** Multiple events can be batched in a single WebSocket frame.
**How to avoid:** Always attempt to parse as `Vec<PolymarketEvent>` first, falling back to single object if needed. Or inspect the first byte -- `[` means array.
**Warning signs:** JSON parse errors on valid-looking messages.

### Pitfall 6: Kalshi WebSocket Requires Auth Even for Public Channels
**What goes wrong:** Attempting to connect without authentication headers, expecting public channels to work without auth.
**Why it happens:** The docs say some channels carry "only public market data" but the connection itself requires authentication. "Some channels carry only public market data, but the connection itself still requires authentication."
**How to avoid:** Always include `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, and `KALSHI-ACCESS-TIMESTAMP` headers in the WebSocket handshake, even for public channels like `ticker` or `orderbook_delta`.
**Warning signs:** Connection rejected immediately on handshake.

### Pitfall 7: Fan-In Channel Capacity and Backpressure
**What goes wrong:** A slow downstream consumer causes all venue feeds to back up, missing live data.
**Why it happens:** All venues share one bounded channel. If consumer is slow, senders block.
**How to avoid:** Size the shared channel generously (1024+ for 3 venues). Monitor channel lag. Consider per-venue overflow metrics using `try_send` with a counter for dropped messages.
**Warning signs:** Feed latency metrics suddenly spike across all venues simultaneously.

### Pitfall 8: Polymarket Tick Size Changes
**What goes wrong:** Fixed tick size assumption fails for extreme probabilities.
**Why it happens:** Polymarket changes tick sizes at price extremes (>0.96 or <0.04). A `tick_size_change` event notifies of this.
**How to avoid:** Track current tick size per asset. Update on `tick_size_change` events. Not critical for Phase 4 (read-only), but important for future order placement.
**Warning signs:** Price levels appear at unexpected granularity near 0 or 1.

## Code Examples

### Polymarket WebSocket Connection and Subscription
```rust
// Source: Polymarket official docs (docs.polymarket.com/developers/CLOB/websocket/market-channel)
use tokio_tungstenite::connect_async;

let url = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
let (ws_stream, _) = connect_async(url).await?;
let (mut write, mut read) = ws_stream.split();

// Subscribe to orderbook updates (no auth needed for market channel)
let subscribe = serde_json::json!({
    "assets_ids": ["71321045679252212594626385532706912750332728571942532289631379312455583992563"],
    "type": "market"
});
write.send(Message::text(subscribe.to_string())).await?;

// Heartbeat: send PING every 10 seconds, expect PONG
// Dynamic subscription: send { "assets_ids": [...], "operation": "subscribe" }
// Unsubscribe: send { "assets_ids": [...], "operation": "unsubscribe" }
```

### Polymarket Book Event Deserialization
```rust
// Source: Polymarket docs (market-channel) + deepwiki analysis
#[derive(Debug, Deserialize)]
pub struct PolymarketBookEvent {
    pub event_type: String,      // "book"
    pub asset_id: String,        // Token ID (not condition ID)
    pub market: String,          // Market/condition ID
    pub hash: String,            // Book verification hash
    pub bids: Vec<PriceLevel>,   // Sorted by system (need to sort descending)
    pub asks: Vec<PriceLevel>,   // Sorted by system (need to sort ascending)
    pub timestamp: String,       // Milliseconds as string
}

#[derive(Debug, Deserialize)]
pub struct PriceLevel {
    pub price: String,  // "0.50" = 50% probability (string, 0-1 range)
    pub size: String,   // "100.0" = share count (string)
}

#[derive(Debug, Deserialize)]
pub struct PolymarketPriceChange {
    pub event_type: String,             // "price_change"
    pub market: String,
    pub price_changes: Vec<PriceChangeEntry>,
    pub timestamp: String,
}

#[derive(Debug, Deserialize)]
pub struct PriceChangeEntry {
    pub asset_id: String,
    pub price: String,
    pub size: String,    // "0" means level removed
    pub side: String,    // "BUY" or "SELL"
    pub hash: String,
    pub best_bid: String,
    pub best_ask: String,
}
```

### Kalshi RSA-PSS Authentication
```rust
// Source: Kalshi docs (quick_start_authenticated_requests) + rsa crate docs
use rsa::{RsaPrivateKey, pss::BlindedSigningKey};
use rsa::signature::RandomizedSigner;
use sha2::Sha256;
use base64::Engine;

fn sign_kalshi_request(
    private_key: &RsaPrivateKey,
    timestamp_ms: i64,
    method: &str,
    path: &str,
) -> String {
    // Message: "{timestamp}{METHOD}{path}" (no query params)
    let message = format!("{}{}{}", timestamp_ms, method, path);

    let signing_key = BlindedSigningKey::<Sha256>::new(private_key.clone());
    let mut rng = rand::thread_rng();
    let signature = signing_key.sign_with_rng(&mut rng, message.as_bytes());

    base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
}
```

### Kalshi WebSocket Connection with Auth Headers
```rust
// Source: Kalshi docs (websocket-connection, quick_start_websockets)
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::connect_async;

let timestamp_ms = chrono::Utc::now().timestamp_millis();
let signature = sign_kalshi_request(
    &private_key,
    timestamp_ms,
    "GET",
    "/trade-api/ws/v2",
);

let request = Request::builder()
    .uri("wss://api.elections.kalshi.com/trade-api/ws/v2")
    .header("KALSHI-ACCESS-KEY", &api_key_id)
    .header("KALSHI-ACCESS-SIGNATURE", &signature)
    .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
    .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
    .header("Sec-WebSocket-Version", "13")
    .header("Connection", "Upgrade")
    .header("Upgrade", "websocket")
    .header("Host", "api.elections.kalshi.com")
    .body(())
    .unwrap();

let (ws_stream, _) = connect_async(request).await?;

// Subscribe to orderbook
let subscribe = serde_json::json!({
    "id": 1,
    "cmd": "subscribe",
    "params": {
        "channels": ["orderbook_delta"],
        "market_ticker": "KXBTC-26FEB22-T100000"
    }
});
```

### Kalshi Orderbook Delta Application
```rust
// Source: Kalshi docs (orderbook_responses) + Go client (ammario/kalshi/feed.go)
use std::collections::BTreeMap;

struct KalshiBook {
    yes_bids: BTreeMap<i64, i64>,  // price_cents -> quantity
    no_bids: BTreeMap<i64, i64>,
}

impl KalshiBook {
    fn apply_snapshot(&mut self, yes: &[(i64, i64)], no: &[(i64, i64)]) {
        self.yes_bids.clear();
        self.no_bids.clear();
        for &(price, qty) in yes { self.yes_bids.insert(price, qty); }
        for &(price, qty) in no { self.no_bids.insert(price, qty); }
    }

    fn apply_delta(&mut self, side: &str, price: i64, delta: i64) {
        let book = match side {
            "yes" => &mut self.yes_bids,
            "no" => &mut self.no_bids,
            _ => return,
        };
        let entry = book.entry(price).or_insert(0);
        *entry += delta;
        if *entry <= 0 { book.remove(&price); }
    }

    fn best_yes_bid(&self) -> Option<(i64, i64)> {
        self.yes_bids.iter().last().map(|(&p, &q)| (p, q))  // BTreeMap sorted ascending
    }

    fn best_yes_ask_from_no(&self) -> Option<i64> {
        // YES ask = 100 - best NO bid
        self.no_bids.iter().last().map(|(&p, _)| 100 - p)
    }
}
```

### Multi-Venue Fan-In Assembly
```rust
// Source: Architecture pattern derived from existing pipeline.rs + tokio mpsc docs
pub async fn run_multi_venue_pipeline(
    venues_config: &VenuesConfig,
    recording_dir: PathBuf,
    cancel: CancellationToken,
) -> anyhow::Result<mpsc::Receiver<MarketSnapshot>> {
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<MarketSnapshot>(1024);

    // Spawn Deribit feed (existing)
    let deribit_tx = snapshot_tx.clone();
    let deribit_cancel = cancel.child_token();
    tokio::spawn(async move {
        run_deribit_pipeline(deribit_tx, deribit_cancel).await;
    });

    // Spawn Polymarket feed
    let poly_tx = snapshot_tx.clone();
    let poly_cancel = cancel.child_token();
    tokio::spawn(async move {
        run_polymarket_pipeline(poly_tx, poly_cancel).await;
    });

    // Spawn Kalshi feed
    let kalshi_tx = snapshot_tx.clone();
    let kalshi_cancel = cancel.child_token();
    tokio::spawn(async move {
        run_kalshi_pipeline(kalshi_tx, kalshi_cancel).await;
    });

    drop(snapshot_tx); // Channel closes when all venue tasks complete

    Ok(snapshot_rx)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Kalshi username/password auth | RSA-PSS key-based auth | 2024 | Must use RSA private key, no more login tokens |
| Kalshi integer price fields | `*_dollars` and `*_fp` string fields | Jan 2026 | Use `_dollars` / `_fp` fields; legacy integers deprecated Feb 2026 |
| Polymarket single tick size | Dynamic tick sizes at extremes | 2024 | `tick_size_change` events at >0.96 or <0.04 |
| Kalshi REST-only orderbook | WebSocket with orderbook_delta | 2024-2025 | Prefer WS for real-time; REST for bootstrapping/fallback |

**Deprecated/outdated:**
- Kalshi `yes`/`no` integer-based price arrays: Use `yes_dollars`/`no_dollars` (string-based) instead. Legacy fields deprecated by Feb 26, 2026.
- Kalshi `notional_value`, `liquidity`, `response_price_units`: Use `*_dollars` equivalents.
- kalshi crate (0.9.0) WebSocket support: Documented as "not complete" as of 0.9.0.

## Open Questions

1. **Polymarket book event batching**
   - What we know: Multiple events can arrive as a JSON array in one WS frame
   - What's unclear: Whether `book` events specifically arrive as arrays, or only `price_change` events. Research from deepwiki suggests array wrapping; official docs are silent.
   - Recommendation: Parse as `serde_json::Value` first, check if array, then iterate. Defensive approach costs nothing.

2. **Kalshi WebSocket orderbook_delta full field structure**
   - What we know: From Go client -- fields are `market_id`, `price` (cents), `delta` (int), `side` (yes/no). Also has `sid` (subscription ID) and `seq` (sequence number).
   - What's unclear: Whether the `_dollars` and `_fp` variants apply to WS deltas (API changelog suggests they do as of Jan 2026).
   - Recommendation: Implement with cents-based fields first (proven via Go client), add `_dollars` support as we discover the exact format from live traffic.

3. **Kalshi orderbook_snapshot exact JSON format**
   - What we know: Contains `market_id`, `yes`, `no` arrays matching REST orderbook response format. Also has `yes_dollars`, `no_dollars`, `yes_dollars_fp`, `no_dollars_fp` per API changelog.
   - What's unclear: Whether the WS snapshot format is byte-for-byte identical to REST `/orderbook` response.
   - Recommendation: Use the REST format as the baseline; adjust if live WS traffic differs.

4. **Polymarket condition ID to token ID resolution at runtime**
   - What we know: Need token IDs (asset_ids) for WS subscription. Gamma API (`GET /markets?condition_id=X`) resolves them.
   - What's unclear: Whether token IDs are stable (never change for a given market) or can change.
   - Recommendation: Resolve at startup via Gamma API REST call. Cache in config. If token IDs change, it would be due to market recreation (rare). Add a REST helper but don't over-engineer caching.

5. **Kalshi WebSocket heartbeat / keepalive requirements**
   - What we know: Connection requires auth. Docs mention subscribe/unsubscribe commands.
   - What's unclear: Whether Kalshi has a heartbeat protocol like Deribit's test_request/public_test cycle, or if TCP keepalive suffices.
   - Recommendation: Implement a staleness-based liveness check (like Deribit's heartbeat timeout) rather than depending on a server heartbeat. If no messages arrive within N seconds, assume dead and reconnect.

## Sources

### Primary (HIGH confidence)
- [Polymarket Market Channel docs](https://docs.polymarket.com/developers/CLOB/websocket/market-channel) -- Complete event type schemas (book, price_change, etc.)
- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- WebSocket URLs, subscription format, heartbeat protocol
- [Polymarket Endpoints](https://docs.polymarket.com/quickstart/reference/endpoints) -- API base URLs (Gamma, CLOB, Data)
- [Kalshi Quick Start: Authenticated Requests](https://docs.kalshi.com/getting_started/quick_start_authenticated_requests) -- RSA-PSS signing algorithm, header format, message construction
- [Kalshi Get Market Orderbook](https://docs.kalshi.com/api-reference/market/get-market-orderbook) -- Orderbook response format with yes/no/dollars/fp arrays
- [Kalshi Orderbook Responses](https://docs.kalshi.com/getting_started/orderbook_responses) -- YES/NO bid relationship, price in cents, array sorting
- [Kalshi Quick Start: WebSockets](https://docs.kalshi.com/getting_started/quick_start_websockets) -- WS URL, subscribe command, channel types

### Secondary (MEDIUM confidence)
- [ammario/kalshi Go client - feed.go](https://github.com/ammario/kalshi/blob/main/feed.go) -- OrderbookDelta struct definition (Price, Delta, Side, MarketID fields)
- [deepwiki analysis of Polymarket WebSocket](https://deepwiki.com/barzoj/yet-another-polymarket-maker/7.1-polymarket-websocket) -- Book event JSON structure with price/size objects
- [Polymarket rs-clob-client](https://github.com/Polymarket/rs-clob-client) -- Official Rust SDK (polymarket-client-sdk crate) with ws feature
- [kalshi-trade-rs](https://docs.rs/kalshi-trade-rs/latest/kalshi_trade_rs/) -- Rust crate with WS + RSA-PSS (v0.2.0)
- [docs.rs/rsa](https://docs.rs/rsa) -- RSA crate PSS signing with BlindedSigningKey<Sha256>

### Tertiary (LOW confidence)
- Kalshi orderbook_delta exact `_dollars`/`_fp` WS field names -- inferred from API changelog, not verified against live traffic
- Polymarket JSON array batching for book events -- inferred from third-party code, not confirmed in official docs

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries are well-established Rust ecosystem crates already used or from RustCrypto
- Architecture: HIGH - fan-in with shared mpsc::Sender is a textbook tokio pattern; per-venue supervisor matches existing Deribit pattern
- Polymarket normalization: HIGH - prices are directly in 0-1 probability space; schema documented in official docs
- Kalshi normalization: MEDIUM - orderbook format confirmed via REST docs and Go client; WS delta format partially inferred
- Kalshi auth: MEDIUM - RSA-PSS algorithm fully documented; Rust implementation path clear but untested
- Pitfalls: MEDIUM - identified from API docs and third-party experience; some edge cases may surface during implementation
- Graceful degradation: HIGH - pattern is straightforward (independent tasks + child CancellationTokens + metrics)

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days -- APIs are stable but check for deprecation timeline on Kalshi legacy fields)
