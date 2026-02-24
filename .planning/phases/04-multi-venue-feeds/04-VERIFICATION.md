---
phase: 04-multi-venue-feeds
verified: 2026-02-24T15:15:00Z
status: passed
score: 4/4 must-haves verified
re_verification: false
---

# Phase 04: Multi-Venue Feeds Verification Report
**Phase Goal:** Polymarket and Kalshi feeds are operational alongside Deribit, all publishing normalized MarketSnapshot events through the same channel, with the system continuing to function when any individual feed drops.
**Verified:** 2026-02-24T15:15:00Z
**Status:** passed
**Re-verification:** No -- initial formal verification (replaces placeholder)

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | System connects to Polymarket CLOB WebSocket and receives order book updates for target condition IDs, normalized from probability space (0-1) with bid/ask/depth (FEED-03 + FEED-04) | VERIFIED | **Connection:** PolymarketClient::start() at client.rs:53-59 calls `connect_async(&ws_url)`. **Subscription:** client.rs:66-76 builds `{"assets_ids": [token_ids], "type": "market"}` with token IDs from config. **Reader loop:** client.rs:105-168 `tokio::select!` loop forwards text frames as `RawMessage` with `DualTimestamp::now()` at lines 129-132. **PING heartbeat:** client.rs:116-122 sends `Message::Ping` at `ping_interval_ms` to keep connection alive. **Supervisor:** PolymarketSupervisor at supervisor.rs:35 runs reconnection loop with ExponentialBackoffBuilder (lines 37-43), backoff reset on first message (lines 79-81), health.mark_available() on reconnect (line 82). **Pipeline wiring:** pipeline.rs:181-186 creates PolymarketSupervisor in `run_live_multi_venue()`. **Normalization:** PolymarketProcessor at normalize.rs:61-86 processes raw messages. Polymarket prices ARE probabilities -- bid_probability from best bid price (normalize.rs:166-167), ask_probability from best ask price (normalize.rs:168-169). Full depth_bids (lines 149-153) and depth_asks (lines 156-160). Staleness gate with exchange timestamp (lines 135-146). Latency metrics: histogram/gauge/counter at lines 172-178. |
| 2 | System connects to Kalshi feed and normalizes contracts into probability + expiry schema (FEED-05) | VERIFIED | **RSA-PSS auth:** auth.rs:30-44 `sign_kalshi_request()` using `BlindedSigningKey::<Sha256>`, message format `"{timestamp_ms}{method}{path}"`. Key loading at auth.rs:17-21 `load_kalshi_private_key()` from PKCS#8 PEM. **Connection:** client.rs:78-95 builds HTTP request with `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP` headers. `connect_async(request)` at client.rs:97-102. **Subscription:** client.rs:109-131 subscribes to `orderbook_delta` channel per market ticker with `{"cmd": "subscribe", "params": {"channels": ["orderbook_delta"]}}`. **Incremental book:** book.rs:13-18 `KalshiBook` with `BTreeMap<i64, i64>` for YES/NO sides. `apply_snapshot()` at book.rs:30-44, `apply_delta()` at book.rs:49-64. Best bid via `.last()` (BTreeMap ascending, line 68). **Cents-to-probability:** normalize.rs:30-32 `Decimal::new(cents, 2)`. **Derived asks:** book.rs:79-81 `best_yes_ask_from_no()` = `100 - best_no_bid`. Depth asks at normalize.rs:220-230 with `100 - cents` conversion. **Phase 12 hardening:** Heartbeat timeout at client.rs:138-142 (`heartbeat_timeout_ms`), dead-connection detection at client.rs:160-168 with `feed_heartbeat_timeouts` counter. Exchange timestamp propagation: normalize.rs:138-141 tracks `last_exchange_ts` HashMap from delta `ts` field. Latency metrics at normalize.rs:256-262. **Supervisor:** KalshiSupervisor at supervisor.rs:51-161 creates fresh client (fresh auth signature) per attempt (lines 74-79), backoff reset on first message (lines 101-103), health callbacks (lines 104, 117, 130). |
| 3 | All three venue feeds publish through the same bounded async channel, and downstream consumers process events from any venue identically | VERIFIED | **Shared fan-in channel:** pipeline.rs:114 `mpsc::channel::<MarketSnapshot>(FAN_IN_BUFFER)` where `FAN_IN_BUFFER = 1024` (line 65). Each venue gets `snapshot_tx.clone()`: Deribit at line 155, Polymarket at line 196, Kalshi at line 249. **Forward tasks:** `forward_snapshots()` at pipeline.rs:320-369 pipes per-venue snapshot receiver to shared fan-in sender with event_id annotation from EventRegistry (lines 341-345) and health.record_message() (lines 347-349). **Drop original sender:** pipeline.rs:283 `drop(snapshot_tx)` ensures channel closes when all venue tasks complete. **Uniform MarketSnapshot type:** All three processors (DeribitProcessor, PolymarketProcessor, KalshiProcessor) produce the same `MarketSnapshot` struct with identical fields. |
| 4 | When any single feed drops, remaining feeds continue operating -- affected instruments are marked unavailable, degraded state is surfaced in metrics, and the system does not crash or stall (RELY-04) | VERIFIED | **Independent CancellationToken per venue:** pipeline.rs:121 `cancel.child_token()` for Deribit, line 173 for Polymarket, line 224 for Kalshi. Child token cancellation does not propagate to parent or siblings. **Missing Kalshi credentials produce warning and skip:** pipeline.rs:270-278 logs warning with `has_api_key` and `has_private_key` diagnostics, remaining venues continue. Invalid RSA key handled at lines 261-267. **VenueHealth tracker:** health.rs:21-27 struct with `AtomicBool` is_available, `Mutex<Option<String>>` last_error, `Mutex<Option<DateTime<Utc>>>` last_message_at, `AtomicU64` connection_count. `mark_available()` at health.rs:46-51 sets gauge to 1.0. `mark_unavailable(error)` at health.rs:56-61 sets gauge to 0.0 and stores error. Observable state: `is_available()` line 64, `last_error()` line 86, `last_message_at()` line 91, `connection_count()` line 96. **Supervisor health callbacks:** Both PolymarketSupervisor (supervisor.rs:82 mark_available, line 95 mark_unavailable) and KalshiSupervisor (supervisor.rs:104 mark_available, line 117 mark_unavailable) call health methods on state transitions. **Metrics gauges:** health.rs:49 and health.rs:59 emit `feed_available` gauge with venue label. |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| src/feed/polymarket/client.rs | WebSocket client connecting to CLOB market channel | VERIFIED | 183 lines. PolymarketClient::start() connects, subscribes with token IDs, PING heartbeat, forwards text frames. Implements RawDataSource (line 179). |
| src/feed/polymarket/normalize.rs | Probability normalization processor | VERIFIED | 409 lines. PolymarketProcessor converts book events to MarketSnapshot. Prices are probabilities (no conversion). Staleness gate, latency metrics. 5 unit tests. |
| src/feed/polymarket/messages.rs | Message types for Polymarket events | VERIFIED | 263 lines. PolymarketEvent enum (Book, PriceChange, TickSizeChange, Unknown). parse_events() handles both array and single object formats. 7 unit tests. |
| src/feed/polymarket/supervisor.rs | Reconnection supervisor with exponential backoff | VERIFIED | 140 lines. PolymarketSupervisor with ExponentialBackoffBuilder, health callbacks, backoff reset on first message. |
| src/feed/polymarket/mod.rs | Module exports | VERIFIED | Exports client, messages, normalize, supervisor submodules. |
| src/feed/kalshi/client.rs | WebSocket client with RSA-PSS auth headers | VERIFIED | 291 lines. KalshiClient::start() with auth headers, orderbook_delta subscription, heartbeat timeout (Phase 12). Implements RawDataSource (line 257). |
| src/feed/kalshi/normalize.rs | Cents-to-probability processor | VERIFIED | 583 lines. KalshiProcessor with per-market BTreeMap book state. cents_to_probability via Decimal::new(cents, 2). Derived asks from complementary side. Exchange timestamp propagation (Phase 12). 8 unit tests. |
| src/feed/kalshi/auth.rs | RSA-PSS authentication signing | VERIFIED | 128 lines. sign_kalshi_request() with BlindedSigningKey, load_kalshi_private_key() from PKCS#8 PEM. 4 unit tests. |
| src/feed/kalshi/book.rs | BTreeMap incremental order book | VERIFIED | 244 lines. KalshiBook with yes_bids/no_bids BTreeMap. apply_snapshot, apply_delta, best_yes_bid (via .last()), best_yes_ask_from_no (100 - NO bid). 12 unit tests. |
| src/feed/kalshi/messages.rs | Serde types for Kalshi WebSocket events | VERIFIED | 401 lines. KalshiMessage enum with parse() handling nested and flat formats. OrderbookSnapshotData, OrderbookDeltaData (with optional ts field from Phase 12), SubscribedData, ErrorData. 12 unit tests. |
| src/feed/kalshi/supervisor.rs | Reconnection supervisor with fresh auth per attempt | VERIFIED | 162 lines. KalshiSupervisor creates fresh KalshiClient per attempt (fresh auth signature), ExponentialBackoffBuilder, health callbacks. |
| src/feed/kalshi/mod.rs | Module exports | VERIFIED | Exports auth, book, client, messages, normalize, supervisor submodules. |
| src/feed/pipeline.rs | Multi-venue pipeline assembly with fan-in | VERIFIED | 455 lines. run_multi_venue_pipeline() with Live/Replay/Mock modes. run_live_multi_venue() spawns independent pipelines per venue with child_token(). Shared mpsc fan-in channel. forward_snapshots() with event_id annotation. Graceful Kalshi credential skip. 1 unit test. |
| src/feed/health.rs | VenueHealth tracker for graceful degradation visibility | VERIFIED | 187 lines. VenueHealth struct with atomics/mutex interior mutability. mark_available/mark_unavailable with metrics gauges. 8 unit tests (lifecycle, connection counting, cycle). |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| pipeline.rs | polymarket/supervisor.rs | Live mode creates PolymarketSupervisor | VERIFIED | pipeline.rs:181-186: `PolymarketSupervisor::new(config, cancel, health)`, spawned with `supervisor.run(supervisor_tx)` |
| pipeline.rs | kalshi/supervisor.rs | Live mode creates KalshiSupervisor | VERIFIED | pipeline.rs:232-239: `KalshiSupervisor::new(config, key_id, private_key, cancel, health)`, spawned with `supervisor.run(supervisor_tx)` |
| pipeline.rs | fan-in channel | Shared mpsc::channel for all venues | VERIFIED | pipeline.rs:114: `mpsc::channel::<MarketSnapshot>(FAN_IN_BUFFER)`. Clone to Deribit (line 155), Polymarket (line 196), Kalshi (line 249). Drop original at line 283. |
| supervisor.rs | client.rs | Creates fresh client per reconnect attempt (both venues) | VERIFIED | Polymarket supervisor.rs:57: `PolymarketClient::new(config, cancel)`. Kalshi supervisor.rs:74-79: `KalshiClient::new(config, key_id, key, cancel)` with fresh auth signature per attempt. |
| client.rs | normalize.rs | Raw frames forwarded to processor (both venues) | VERIFIED | Both clients forward text frames as RawMessage. Processor receives via `raw_rx.recv()`: PolymarketProcessor at normalize.rs:73-75, KalshiProcessor at normalize.rs:88-90. |
| health.rs | pipeline.rs | VenueHealth created and passed to supervisors | VERIFIED | pipeline.rs:119 `VenueHealth::new(Venue::Deribit)`, line 170 Polymarket, line 211 Kalshi. Passed via `health.clone()` to supervisor constructors and forward_snapshots. |
| forward_snapshots | fan-in | Processor output forwarded to shared channel | VERIFIED | pipeline.rs:320-369: `forward_snapshots()` receives from per-venue `venue_rx`, annotates event_id from EventRegistry (lines 341-345), calls `health.record_message()` (line 348), sends to `fan_in_tx` (line 350). |

### Requirements Coverage

| Success Criterion | Status | Blocking Issue |
|-------------------|--------|----------------|
| SC-1: Polymarket CLOB WebSocket connection and subscription with probability normalization (FEED-03 + FEED-04) | SATISFIED | None |
| SC-2: Kalshi feed connection and normalization into probability + expiry schema (FEED-05) | SATISFIED | None |
| SC-3: All three feeds publish through same bounded async channel (unified pipeline) | SATISFIED | None |
| SC-4: Single feed drop degrades gracefully with remaining feeds continuing (RELY-04) | SATISFIED | None |

### Anti-Patterns Found

None detected. No TODO/FIXME/PLACEHOLDER/HACK comments in any Phase 4 source file (verified by grep across all 14 files). No stub returns, no empty handlers, no placeholder components. All implementations are substantive and wired.

Note: The `NormalizedDataSource` trait in `src/feed/traits.rs` is dead code with zero implementations (identified by the v1.0 audit). This is being addressed separately by Phase 13 Plan 02 and is not a Phase 4 anti-pattern -- it was defined speculatively in Phase 2.

### Human Verification Required

#### 1. Live Polymarket WebSocket Connection

**Test:** Run `cargo run` in live mode with Polymarket assets configured in `venues.toml`. Watch logs for 60 seconds.
**Expected:** Connection established, subscription acknowledged, book events received and normalized to MarketSnapshot with probability fields. On network disruption, PolymarketSupervisor reconnects with exponential backoff (1s initial, 60s max, 0.5 jitter). Backoff resets after first message received post-reconnect.
**Why human:** Cannot verify actual Polymarket WebSocket behavior without live internet connection. Code path is correct but real reconnection timing and CLOB data integrity must be observed empirically.

#### 2. Live Kalshi WebSocket Connection with RSA-PSS Auth

**Test:** Set `KALSHI_API_KEY_ID` and `KALSHI_PRIVATE_KEY` environment variables, run `cargo run` in live mode. Watch logs for Kalshi subscription acknowledgments and orderbook data.
**Expected:** RSA-PSS signature accepted by Kalshi, `{"id": N, "msg": "subscribed"}` received for each market ticker, orderbook snapshots and deltas arrive and are normalized to MarketSnapshot. Heartbeat timeout at 30s detects dead connections. On credential absence, system logs warning and continues with Deribit and Polymarket only.
**Why human:** Requires valid Kalshi API credentials and live network. RSA-PSS signing correctness with Kalshi's server cannot be verified offline. Phase 12 heartbeat timeout behavior should be observed under real network conditions.

## Gaps Summary

No gaps found. All four success criteria are fully implemented, substantive, and wired end-to-end.

1. **Polymarket CLOB WebSocket (FEED-03):** PolymarketClient connects to the market channel, subscribes with token IDs (not condition IDs per Pitfall 1), sends PING heartbeat to keep connection alive, and forwards raw text frames. PolymarketSupervisor wraps with exponential backoff reconnection, backoff resets only after first message received.

2. **Polymarket probability normalization (FEED-04):** PolymarketProcessor parses book events, applies Polymarket's probability-space prices directly (prices ARE probabilities, no conversion needed). Populates bid_probability, ask_probability, full depth_bids/depth_asks. Staleness gate with exchange timestamp. Latency metrics via histogram/gauge/counter.

3. **Kalshi connection and normalization (FEED-05):** KalshiClient authenticates with RSA-PSS (SHA-256, millisecond timestamps per Pitfall 4), subscribes to orderbook_delta channel. BTreeMap-based incremental book with apply_snapshot/apply_delta. Cents-to-probability via `Decimal::new(cents, 2)`. Derived asks from complementary side (`100 - NO_bid_cents`). Phase 12 added heartbeat timeout (30s default), nested message parsing, exchange timestamp propagation from delta `ts` field, and latency metrics.

4. **Graceful degradation (RELY-04):** Each venue gets an independent `CancellationToken` via `cancel.child_token()` -- one venue crashing does not affect siblings. Missing Kalshi credentials produce a warning and the venue is skipped. VenueHealth tracker exposes connection state through atomics with metrics gauges (`feed_available`). Both supervisors call health callbacks on state transitions (mark_available on first message, mark_unavailable on connection loss).

**Cross-phase integration note:** Phase 10 completed the event_id annotation in `forward_snapshots()`, enabling PaperTradeTracker and downstream consumers to correlate snapshots with mapped events. This integration point was not part of Phase 4's original scope but is now wired and functional.

**Test suite:** 417 tests pass (360 lib + 16 integration + 5 pipeline + 11 smoke + 22 additional + 3 doc), zero compiler warnings, zero compiler errors.

---

_Verified: 2026-02-24T15:15:00Z_
_Verifier: Claude (gsd-executor)_
