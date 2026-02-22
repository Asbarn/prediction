# Phase 3: Feed Infrastructure - Research

**Researched:** 2026-02-22
**Domain:** WebSocket reliability, rate limiting, latency tracking, production feed hardening
**Confidence:** HIGH

## Summary

Phase 3 hardens the existing Deribit feed pipeline (Phase 2) to survive production conditions. The five distinct capabilities are: automatic reconnection with exponential backoff, Deribit-native heartbeat monitoring to detect dead connections, per-instrument staleness gating, per-venue rate limiting, and timestamp-based latency tracking.

The codebase is well-positioned for this work. The `DeribitClient` already isolates the WebSocket connection in a background task behind `mpsc::Receiver<RawMessage>`, and the `DeribitProcessor` pipeline runs independently. The reconnection layer wraps the client, not the processor. The existing `DualTimestamp` type already captures both monotonic and wall-clock time. The `MarketSnapshot` already carries `exchange_timestamp` and `is_stale`. Several "Phase 3" TODOs are explicitly noted in comments (client.rs line 5, normalize.rs line 243, writer.rs line 52).

**Primary recommendation:** Use the `backoff` crate (0.4 with tokio feature) for exponential backoff, the `governor` crate for rate limiting, and the `metrics` crate facade for latency tracking. Hand-roll the reconnection supervisor and heartbeat monitor as thin tokio tasks wrapping existing components -- these are domain-specific and straightforward with the existing architecture.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| backoff | 0.4 | Exponential backoff with jitter | Production-proven, tokio-native, distinguishes Permanent vs Transient errors, built-in `retry_notify` for logging attempts |
| governor | 0.8+ | Per-venue rate limiting (GCRA/leaky bucket) | De facto Rust rate limiter, async `until_ready()`, 64-bit atomic state, no background tasks needed |
| metrics | 0.24+ | Latency tracking facade (counter, gauge, histogram) | Ecosystem standard facade (like `tracing` for logs), decouples instrumentation from exporter |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| metrics-exporter-prometheus | 0.16+ | Prometheus HTTP exporter for `metrics` | Phase 6 (OBSV-03 requires Prometheus); install recorder in main.rs when ready |
| rand | 0.8 (already in deps) | Jitter source for backoff | Already a dependency; used by `backoff` internally |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| backoff | tokio-retry | tokio-retry is simpler but lacks `retry_notify` and Permanent/Transient error distinction |
| backoff | Hand-rolled loop + tokio::time::sleep | Misses jitter, randomization factor, max_elapsed_time; more code for less reliability |
| governor | Hand-rolled token bucket | governor handles async awaiting, burst capacity, and thread-safe atomic updates correctly |
| metrics | prometheus (direct) | prometheus crate locks you to one exporter; metrics facade allows swapping backends later |

**Installation:**
```toml
# Add to Cargo.toml [dependencies]
backoff = { version = "0.4", features = ["tokio"] }
governor = "0.8"
metrics = "0.24"
```

## Architecture Patterns

### Recommended Project Structure
```
src/
  feed/
    deribit/
      client.rs           # Existing -- unchanged (single-connection logic)
      supervisor.rs        # NEW: reconnection loop wrapping DeribitClient
      heartbeat.rs         # NEW: heartbeat monitor (set_heartbeat + test_request handler)
      messages.rs          # MODIFY: add heartbeat message variants
      normalize.rs         # MODIFY: add staleness gate before publishing
      book.rs              # Existing -- unchanged
      channels.rs          # Existing -- unchanged
      mod.rs               # MODIFY: export new modules
    reliability/
      rate_limiter.rs      # NEW: per-venue rate limiter wrapper around governor
      staleness.rs         # NEW: per-instrument staleness gate
      mod.rs               # NEW
    metrics/
      latency.rs           # NEW: per-feed latency tracker (exchange_ts vs local_ts)
      mod.rs               # NEW
    pipeline.rs            # MODIFY: wire supervisor instead of raw client
    traits.rs              # Existing -- unchanged
    recording/
      writer.rs            # MODIFY: periodic flush instead of per-write flush
      mod.rs               # Existing
    mock/                  # Existing -- unchanged
    mod.rs                 # MODIFY: export new modules
  config/
    venues.rs              # MODIFY: add staleness_threshold_ms, reconnect config
```

### Pattern 1: Reconnection Supervisor (wraps DeribitClient)
**What:** A long-lived supervisor task that owns the reconnection loop. On each connection attempt, it creates a fresh `DeribitClient`, calls `start()`, and feeds messages into the existing processor pipeline. When the connection drops (read loop ends), it re-enters the backoff loop.
**When to use:** Always in production Live mode.
**Key insight:** The supervisor does NOT replace `DeribitClient` -- it wraps it. The client stays as a simple "connect once, read until done" component.

```rust
// Sketch of supervisor pattern
pub struct DeribitSupervisor {
    config: DeribitConfig,
    instruments: Vec<String>,
    cancel: CancellationToken,
}

impl DeribitSupervisor {
    pub async fn run(self, tx: mpsc::Sender<RawMessage>) {
        let backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_secs(1))
            .with_max_interval(Duration::from_secs(60))
            .with_randomization_factor(0.5)
            .with_max_elapsed_time(None) // Never give up
            .build();

        loop {
            if self.cancel.is_cancelled() { break; }

            // Create fresh client for each attempt
            let client = DeribitClient::new(
                self.config.clone(),
                self.instruments.clone(),
                self.cancel.clone(),
            );

            match client.start().await {
                Ok(mut raw_rx) => {
                    tracing::info!("connected, forwarding messages");
                    // Reset backoff on successful connection
                    // Forward messages until channel closes
                    while let Some(msg) = raw_rx.recv().await {
                        if tx.send(msg).await.is_err() { return; }
                    }
                    tracing::warn!("connection lost, reconnecting...");
                }
                Err(e) => {
                    tracing::error!(error = %e, "connection attempt failed");
                }
            }

            // Apply backoff before retry
            let delay = backoff.next_backoff().unwrap_or(Duration::from_secs(60));
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = self.cancel.cancelled() => break,
            }
        }
    }
}
```

### Pattern 2: Deribit Heartbeat Protocol
**What:** After connecting, send `public/set_heartbeat` with the configured interval. The server then sends `"method": "heartbeat"` notifications. When a notification contains a test_request, respond with `"method": "public/test"`. If no heartbeat arrives within 2x the interval, consider the connection dead.
**When to use:** On every live Deribit connection.

The Deribit V2 API heartbeat works as follows:
1. Client sends: `{"jsonrpc":"2.0","id":<N>,"method":"public/set_heartbeat","params":{"interval":10}}`
2. Server responds: `{"jsonrpc":"2.0","id":<N>,"result":"ok"}`
3. Server periodically sends: `{"jsonrpc":"2.0","method":"heartbeat","params":{"type":"test_request"}}` (and `"type":"heartbeat"`)
4. Client must respond to test_request with: `{"jsonrpc":"2.0","id":<N>,"method":"public/test","params":{}}`
5. If client fails to respond, server closes the connection immediately

**Implementation approach:** The heartbeat handling must happen in the WS read/write loop inside `DeribitClient`. The client needs write access to send `public/test` responses. This means the read loop must be extended to detect heartbeat method messages and respond inline. A separate heartbeat timeout timer (2x interval) detects dead connections when no messages arrive at all.

### Pattern 3: Per-Instrument Staleness Gate
**What:** Before publishing any `MarketSnapshot` downstream, check whether `(now - exchange_timestamp) > staleness_threshold`. If stale, set `is_stale = true` and log a warning. The snapshot is still sent (per decision 02-02) but marked stale.
**When to use:** In `DeribitProcessor::handle_raw_message` or in `build_snapshot`.
**Key insight:** The staleness gate operates on `exchange_timestamp` (milliseconds since epoch from Deribit), NOT on `received_at`. This correctly handles delayed messages.

```rust
fn is_data_stale(exchange_ts_ms: i64, threshold: Duration) -> bool {
    let now_ms = Utc::now().timestamp_millis();
    let age = Duration::from_millis((now_ms - exchange_ts_ms).max(0) as u64);
    age > threshold
}
```

### Pattern 4: Rate Limiter Integration
**What:** A `governor::RateLimiter` instance per venue, checked before any outgoing API call (subscribe, heartbeat response, future order placement).
**When to use:** Before every outbound message on the WebSocket.
**Key insight:** For Phase 3, only the heartbeat response and re-subscribe messages are rate-limited. The real value comes in Phase 4+ with authenticated trading endpoints.

```rust
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;

let limiter = RateLimiter::direct(
    Quota::per_second(NonZeroU32::new(20).unwrap())
);

// Before sending any request:
limiter.until_ready().await;
ws_write.send(message).await?;
```

### Pattern 5: Latency Tracking
**What:** On every MarketSnapshot, compute `local_receipt_ms - exchange_reported_ms` and record it in a histogram metric. This captures one-way latency characteristics per feed.
**When to use:** In the normalization pipeline, when both timestamps are available.

```rust
if let Some(exchange_ts) = exchange_timestamp {
    let local_ms = received_at.wall().timestamp_millis();
    let latency_ms = (local_ms - exchange_ts) as f64;
    metrics::histogram!("feed_latency_ms", "venue" => "deribit")
        .record(latency_ms);
}
```

### Anti-Patterns to Avoid
- **Reconnect inside the read loop:** The read loop should exit cleanly on connection drop. Reconnection belongs in the supervisor, not in the client. Mixing them creates tangled state and makes testing impossible.
- **Blocking on rate limiter in the read path:** Only outbound messages need rate limiting. Never rate-limit the inbound message read loop.
- **Wall-clock staleness checks without monotonic backup:** If the system clock jumps, wall-clock staleness checks produce false positives. Use `exchange_timestamp` (from the exchange's clock) for staleness, and `DualTimestamp.mono` for connection liveness.
- **Resetting backoff on every attempt:** Backoff should reset only on successful connection + first message received, not on connection attempt start. Otherwise a server that accepts TCP but immediately closes the WebSocket will burn through retries with no backoff.
- **Per-message flush after Phase 3:** The writer currently flushes on every write (noted in writer.rs line 51-52). Phase 3 should switch to periodic flush (every N seconds or N messages) for throughput.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Exponential backoff with jitter | Custom sleep loop with `rand` | `backoff` crate | Jitter distribution, max_elapsed_time, Permanent/Transient error types, tested edge cases |
| Rate limiting (token bucket / GCRA) | AtomicU64 counter with periodic reset | `governor` crate | Burst handling, async await integration, no background thread, handles clock wrap |
| Metrics facade | Custom HashMap of counters | `metrics` crate | Standardized ecosystem, swappable backends, thread-safe, zero-cost when no recorder installed |
| Heartbeat protocol | N/A (Deribit-specific) | Hand-roll (it's 30 lines) | Protocol is simple and venue-specific; no crate wraps Deribit's exact heartbeat flow |
| Reconnection supervisor | N/A (domain-specific) | Hand-roll (thin wrapper) | Architecture-specific: needs to wire into existing mpsc channels and CancellationToken |

**Key insight:** The three things worth hand-rolling (heartbeat protocol, reconnection supervisor, staleness gate) are domain-specific glue code that is simpler than any generic abstraction would be. The three things NOT worth hand-rolling (backoff, rate limiting, metrics) have subtle edge cases that established crates handle correctly.

## Common Pitfalls

### Pitfall 1: Heartbeat Timeout False Positives During Quiet Markets
**What goes wrong:** Setting the heartbeat timeout too low triggers false disconnections during quiet overnight options markets where no trades occur and book snapshots are infrequent.
**Why it happens:** The heartbeat timeout is confused with "no subscription data" timeout. They are different: heartbeat messages come from the server at the configured interval regardless of market activity.
**How to avoid:** Set Deribit's heartbeat interval to 10s (minimum). Set the client-side timeout to 2x-3x the heartbeat interval (20-30s). The heartbeat mechanism operates independently of subscription data -- if heartbeats stop, the connection is dead regardless of market activity.
**Warning signs:** Frequent reconnections during low-volume hours (UTC 00:00-08:00).

### Pitfall 2: Reconnection Thundering Herd
**What goes wrong:** After a server-side restart, all clients reconnect simultaneously, overwhelming the server and causing cascading failures.
**Why it happens:** Exponential backoff without jitter produces identical retry schedules.
**How to avoid:** Always use randomized jitter (the `backoff` crate's `randomization_factor` defaults to 0.5, which is appropriate). The `backoff` crate handles this correctly out of the box.
**Warning signs:** Logs showing multiple connection failures in rapid succession at the same timestamps.

### Pitfall 3: Stale Threshold Too Aggressive
**What goes wrong:** Setting staleness threshold to 1-2 seconds causes legitimate data to be marked stale during normal network jitter.
**Why it happens:** One-way latency between Deribit servers and the client can spike to 1-2 seconds under load. The exchange timestamp reflects when Deribit generated the message, not when it was sent.
**How to avoid:** Default threshold of 5 seconds is appropriate. Log latency statistics for a week before tightening. Make threshold configurable per-instrument in `venues.toml`.
**Warning signs:** `is_stale=true` appearing frequently during normal market hours.

### Pitfall 4: Rate Limiter Applied to Wrong Scope
**What goes wrong:** Rate limiting WebSocket subscription data reads, causing message backpressure and eventual disconnection.
**Why it happens:** Confusion between inbound (received) and outbound (sent) message rate limits. Deribit's rate limit applies to requests the client SENDS, not to subscription data the client RECEIVES.
**How to avoid:** Rate limiter guards only outbound WebSocket writes (subscribe, heartbeat response, future order placement). Never rate-limit the read path.
**Warning signs:** Increasing `raw_rx` channel backpressure, eventual `buffer full` warnings.

### Pitfall 5: Forgetting to Re-subscribe After Reconnect
**What goes wrong:** After reconnection, the client has a fresh WebSocket but no active subscriptions. No data flows.
**Why it happens:** Subscriptions are per-connection state on Deribit's side. A new WebSocket connection starts with zero subscriptions.
**How to avoid:** The existing `DeribitClient::start()` already sends the subscribe request as part of connection setup. The supervisor pattern (create new client per attempt) gets this for free. Never try to "resume" an old connection.
**Warning signs:** Connection succeeds but no messages appear in the pipeline.

### Pitfall 6: Heartbeat Response Blocked by Rate Limiter
**What goes wrong:** The rate limiter delays the heartbeat response (public/test) long enough that the server closes the connection.
**Why it happens:** If the rate limiter is at capacity from subscribe requests, the heartbeat response queues behind them.
**How to avoid:** Either (a) exempt heartbeat responses from rate limiting entirely, or (b) give heartbeat responses priority in the send queue. Option (a) is simpler and correct -- heartbeat responses are a tiny fraction of traffic and Deribit expects them promptly.
**Warning signs:** Connections dropping at exactly the heartbeat timeout interval.

## Code Examples

### Backoff Configuration for Reconnection
```rust
// Source: backoff crate docs + Deribit best practices
use backoff::ExponentialBackoffBuilder;
use std::time::Duration;

let backoff = ExponentialBackoffBuilder::new()
    .with_initial_interval(Duration::from_secs(1))   // Start at 1s
    .with_max_interval(Duration::from_secs(60))        // Cap at 60s
    .with_randomization_factor(0.5)                    // +/- 50% jitter
    .with_multiplier(2.0)                              // Double each time
    .with_max_elapsed_time(None)                       // Never give up
    .build();
```

### Deribit Heartbeat Setup (JSON-RPC)
```rust
// Source: Deribit API docs + Python code samples
// After successful WebSocket connection, send:
let heartbeat_msg = serde_json::json!({
    "jsonrpc": "2.0",
    "id": REQUEST_ID.fetch_add(1, Ordering::Relaxed),
    "method": "public/set_heartbeat",
    "params": {
        "interval": 10  // seconds, minimum 10
    }
});
ws_write.send(Message::text(heartbeat_msg.to_string())).await?;

// When receiving a message with method == "heartbeat" and
// params.type == "test_request", respond with:
let test_response = serde_json::json!({
    "jsonrpc": "2.0",
    "id": REQUEST_ID.fetch_add(1, Ordering::Relaxed),
    "method": "public/test",
    "params": {}
});
ws_write.send(Message::text(test_response.to_string())).await?;
```

### Governor Rate Limiter for Outbound Messages
```rust
// Source: governor crate docs
use governor::{Quota, RateLimiter, clock::DefaultClock, state::InMemoryState,
               state::NotKeyed, middleware::NoOpMiddleware};
use std::num::NonZeroU32;

type VenueRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

pub fn deribit_rate_limiter() -> VenueRateLimiter {
    // Deribit: 20 requests/second for private API
    // Public API is more generous but using same limit is safe
    RateLimiter::direct(
        Quota::per_second(NonZeroU32::new(20).unwrap())
    )
}

// Usage before any outbound WS message:
rate_limiter.until_ready().await;
ws_write.send(message).await?;
```

### Latency Tracking with metrics Crate
```rust
// Source: metrics crate docs
use metrics::{counter, gauge, histogram};

// In build_snapshot or handle_book/handle_ticker:
if let Some(exchange_ts_ms) = exchange_timestamp {
    let local_ms = received_at.wall().timestamp_millis();
    let latency_ms = (local_ms - exchange_ts_ms) as f64;

    histogram!("feed_latency_ms", "venue" => "deribit").record(latency_ms);
    gauge!("feed_last_latency_ms", "venue" => "deribit").set(latency_ms);
    counter!("feed_messages_total", "venue" => "deribit").increment(1);
}
```

### Periodic Flush for Recording Writer
```rust
// Replace per-write flush with periodic flush
// In JsonlWriter or recording_task:
let mut flush_interval = tokio::time::interval(Duration::from_secs(1));
let mut messages_since_flush = 0u64;

loop {
    tokio::select! {
        msg = rx.recv() => {
            match msg {
                Some(line) => {
                    writer.write_line_no_flush(&line).await?;
                    messages_since_flush += 1;
                }
                None => break,
            }
        }
        _ = flush_interval.tick() => {
            if messages_since_flush > 0 {
                writer.flush().await?;
                messages_since_flush = 0;
            }
        }
        _ = cancel.cancelled() => {
            // Drain + final flush on shutdown
            break;
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tokio-retry` for backoff | `backoff` crate with tokio feature | 2023+ | `backoff` has richer configuration, Permanent/Transient distinction, `retry_notify` |
| `ratelimit_meter` | `governor` (renamed) | 2021 | Same maintainer, better API, async support |
| Custom metrics HashMap | `metrics` facade crate | 2022+ | Standardized ecosystem; same pattern as `tracing` for logs |
| Per-write flush for recording | Periodic flush (1s interval) | This phase | 10-100x throughput improvement for JSONL recording |

**Deprecated/outdated:**
- `ratelimit_meter`: Renamed to `governor`; do not use the old name
- `tokio-retry` 0.2.x: Old version without proper async support; 0.3 is current but `backoff` is preferred

## Open Questions

1. **Heartbeat Notification Exact V2 Format**
   - What we know: Server sends `"method": "heartbeat"` notifications. Client responds with `"method": "public/test"`. The params likely contain `"type": "test_request"` or `"type": "heartbeat"` to distinguish the two.
   - What's unclear: The exact `params` structure is not fully documented in publicly-accessible Deribit docs. Python examples check `message['method'] == 'heartbeat'` without inspecting params.type.
   - Recommendation: Implement detection on `"method": "heartbeat"` only. If the message is a heartbeat notification, always respond with `public/test` regardless of params.type. Test against Deribit testnet to confirm exact format. Add the heartbeat as a new variant to `DeribitMessage` enum. **Confidence: MEDIUM** -- the protocol is well-established but V2 specifics require testnet validation.

2. **Write Path Split for Heartbeat**
   - What we know: Currently `DeribitClient::start()` creates `(write, read)` from `ws_stream.split()`. The write half sends the subscribe request, then the spawned task only reads. Heartbeat requires write access inside the read loop.
   - What's unclear: Best way to share the write half between the subscribe logic and the heartbeat response handler.
   - Recommendation: Move the write half into the spawned task. The task handles both reading (forwarding to mpsc) and writing (heartbeat responses). Use an internal mpsc or direct write access since only one task owns the write half. **Confidence: HIGH** -- this is a standard tokio pattern.

3. **Metrics Recorder Installation Timing**
   - What we know: `metrics` crate requires a global recorder to be installed before metrics are emitted. Phase 6 adds Prometheus exporter.
   - What's unclear: Whether to install a no-op recorder now or defer recorder installation to Phase 6.
   - Recommendation: Install metrics infrastructure in Phase 3 with a simple logging recorder (or no recorder) for now. The `metrics` macros are zero-cost when no recorder is installed -- metrics calls become no-ops. Install the Prometheus exporter recorder in Phase 6. **Confidence: HIGH** -- this is exactly how the metrics crate is designed to work.

4. **Config Shape for New Parameters**
   - What we know: `DeribitConfig` in `venues.rs` already has `heartbeat_interval_ms` and `rate_limit_per_second`. New config needed: `staleness_threshold_ms`, reconnect parameters (initial_backoff_ms, max_backoff_ms).
   - What's unclear: Whether these belong on `DeribitConfig` or a shared `ReliabilityConfig`.
   - Recommendation: Add `staleness_threshold_ms` to `DeribitConfig` since it's per-venue. Add reconnect config as nested struct `ReconnectConfig` inside `DeribitConfig`. This follows the decision [02-04] that pipeline takes `DeribitConfig` directly. **Confidence: HIGH** -- follows existing patterns.

## Sources

### Primary (HIGH confidence)
- Deribit API official docs (set_heartbeat) - https://docs.deribit.com/api-reference/session-management/public-set_heartbeat.md - heartbeat setup, interval minimum 10s, server closes on missed test_request
- Deribit API gitbooks (WebSocket RPC) - https://deribitexchange.gitbooks.io/deribit-api/api-websocket.html - heartbeat mechanism overview, test_request response requirement
- governor crate docs.rs - https://docs.rs/governor/latest/governor/struct.RateLimiter.html - RateLimiter::direct(), Quota, until_ready(), async support
- backoff crate docs.rs - https://docs.rs/backoff - ExponentialBackoff config, retry_notify, tokio feature, Permanent/Transient errors
- metrics crate docs.rs - https://docs.rs/metrics - counter!, gauge!, histogram! macros, Recorder trait pattern

### Secondary (MEDIUM confidence)
- Deribit Python WebSocket example - https://github.com/ElliotP123/crypto-exchange-code-samples/blob/master/deribit/websockets/dbt-ws-authenticated-example.py - heartbeat response format {"method":"public/test","params":{}}
- Deribit rate limits documentation - https://www.deribit.com/kb/deribit-rate-limits - 20 req/s sustained, 50K credit pool, 10 credits/ms refill
- deribit-rs Rust client - https://github.com/dovahcrow/deribit-rs - confirms set_heartbeat and public/test in V2 API

### Tertiary (LOW confidence)
- Deribit connection management best practices - https://support.deribit.com/hc/en-us/articles/25944603459613-Connection-Management-Best-Practices - returned 403, could not verify content

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - backoff, governor, metrics are the clear ecosystem choices with extensive documentation
- Architecture: HIGH - supervisor pattern is well-established, existing codebase structure supports clean separation
- Pitfalls: HIGH - common issues are well-documented across WebSocket trading systems
- Deribit heartbeat protocol: MEDIUM - V2 format confirmed from multiple sources but exact params structure needs testnet validation

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days -- stable domain, libraries versioned)
