# Phase 40: Polymarket WS Diagnosis and Data Watchdog - Research

**Researched:** 2026-03-09
**Domain:** WebSocket reliability, data inactivity detection, Polymarket CLOB API
**Confidence:** HIGH

## Summary

Phase 40 addresses three Polymarket WebSocket issues from production EC2: (1) diagnosing the failure mode ("connection reset by peer" vs silent freeze vs geo-block), (2) implementing a data inactivity watchdog in the supervisor to force-reconnect on silent freezes, and (3) getting order book data flowing from EC2.

The Polymarket CLOB WebSocket has a well-documented server-side silent freeze issue (GitHub #292) where the server accepts connections and subscriptions but sends zero data events while PING/PONG continues working. This is distinct from connection resets. The existing `PolymarketSupervisor` handles connection drops (channel close) but has no mechanism to detect silent freezes where the connection stays open but no data arrives.

**Primary recommendation:** Add a configurable `data_timeout_secs` to `PolymarketConfig`, implement a `tokio::time::timeout`-based inactivity watchdog in the supervisor's forwarding loop, and create a diagnostic script/binary that tests WS connectivity from EC2 and reports the failure mode.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| POLY-01 | System diagnoses Polymarket WebSocket failure mode from EC2 (connection reset vs silent freeze vs geo-block) | Diagnostic tool that connects, subscribes, waits for data, and reports the result. Three distinct failure modes identified with specific detection signatures. |
| POLY-02 | Polymarket supervisor detects data inactivity (silent freeze) and triggers reconnection after configurable timeout | `tokio::time::timeout` wrapping `raw_rx.recv()` in the supervisor forwarding loop, with configurable `data_timeout_secs` in `PolymarketConfig`. VenueHealth already tracks `last_message_at`. |
| POLY-03 | Polymarket WebSocket feed connects and delivers order book data from production EC2 instance | Depends on POLY-01 diagnosis. If connection works, POLY-02 handles recovery. If geo-blocked, phase documents this and defers to Phase 42 REST fallback. |
</phase_requirements>

## Standard Stack

### Core (already in project)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | (workspace) | Async runtime, `tokio::time::timeout` for inactivity detection | Already used everywhere |
| tokio-tungstenite | (workspace) | WebSocket client | Already used by PolymarketClient |
| backoff | (workspace) | Exponential backoff with jitter | Already used by all supervisors |
| metrics | (workspace) | Prometheus metric emission | Already used for `feed_available`, `feed_reconnections_total` |
| tracing | (workspace) | Structured logging | Already used everywhere |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| reqwest | (workspace) | REST endpoint validation in diagnostic | Already in project for REST calls |
| governor | (workspace) | Rate limiting for REST calls | Already in project |

### No New Dependencies Required
This phase requires zero new crate additions. All functionality is achievable with `tokio::time::timeout` and the existing supervisor pattern.

## Architecture Patterns

### Current Supervisor Structure
```
src/feed/polymarket/
  client.rs       -- WS connect, subscribe, read loop (spawned task)
  messages.rs     -- Event types (Book, PriceChange, TickSizeChange)
  normalize.rs    -- PolymarketProcessor: RawMessage -> MarketSnapshot
  supervisor.rs   -- Reconnection loop with exponential backoff
```

### Pattern 1: Data Inactivity Watchdog in Supervisor
**What:** Add a `tokio::time::timeout` around `raw_rx.recv()` in the supervisor's forwarding loop to detect silent freezes.
**When to use:** When the WS connection stays alive (PING/PONG works) but the server stops sending data events.
**Why here (not client):** The supervisor owns the reconnection decision. The client is "connect once, read until done." The supervisor decides when to force-reconnect.

```rust
// In PolymarketSupervisor::run(), replace the raw_rx.recv() arm:
msg = raw_rx.recv() => { ... }

// With a timeout-wrapped version:
result = tokio::time::timeout(
    Duration::from_secs(data_timeout_secs),
    raw_rx.recv()
) => {
    match result {
        Ok(Some(raw)) => {
            // Normal message processing (existing code)
        }
        Ok(None) => {
            // Channel closed = connection lost (existing code)
        }
        Err(_elapsed) => {
            // SILENT FREEZE DETECTED
            // No data for data_timeout_secs -- force reconnect
            self.health.mark_unavailable("data inactivity timeout".to_string());
            metrics::counter!("feed_data_timeout_total", "venue" => "polymarket").increment(1);
            tracing::warn!(
                timeout_secs = data_timeout_secs,
                "PolymarketSupervisor: data inactivity detected, forcing reconnect"
            );
            break; // -> reconnect loop
        }
    }
}
```

**Key design decisions:**
- Timeout wraps `recv()`, not the entire `select!` -- we still want cancel and subscription-change to be responsive
- The timeout resets every time a message arrives (each loop iteration calls `timeout()` fresh)
- On timeout, treat it like a connection drop: `mark_unavailable`, `break` to reconnect
- Backoff is NOT reset on timeout (it is a failure, not intentional reconnect)

### Pattern 2: Diagnostic Tool for EC2 Failure Mode
**What:** A standalone binary or integration test that connects to Polymarket WS from the current host, subscribes, and reports what happens.
**When to use:** Run once from EC2 to diagnose the failure mode (POLY-01).

```rust
// Diagnostic flow:
// 1. Attempt TCP connection to wss://ws-subscriptions-clob.polymarket.com/ws/market
//    - Fails immediately? -> "connection refused" or "connection reset" -> report
// 2. If connected, send subscription message with a known active token_id
// 3. Wait up to 30 seconds for first book event
//    - Book event received? -> "WS working from this host"
//    - Empty array [] then silence? -> "silent freeze (GitHub #292)"
//    - Connection reset during subscribe? -> "connection reset by peer"
//    - TLS handshake fails? -> possible geo-block or TLS issue
// 4. Also test REST /midpoint as baseline
//    - GET https://clob.polymarket.com/midpoint?token_id={TOKEN_ID}
//    - If REST works but WS doesn't -> confirms WS-specific issue
```

**Implementation choice:** This can be a `#[tokio::test] #[ignore]` integration test or a small binary in `src/bin/`. An `#[ignore]` test is simpler -- it gets compiled with the project and can be run via `cargo test --test polymarket_diag -- --ignored` on EC2.

### Pattern 3: Configuration Extension
**What:** Add `data_timeout_secs` to `PolymarketConfig`.
```toml
[polymarket]
ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
# ... existing fields ...
data_timeout_secs = 120  # Force reconnect after 2 minutes of silence
```

```rust
// In PolymarketConfig:
#[serde(default = "default_data_timeout_secs")]
pub data_timeout_secs: u64,

fn default_data_timeout_secs() -> u64 { 120 }
```

**Why 120 seconds default:** The GitHub #292 reporter uses 120s. Polymarket sends book snapshots on subscription and price_change events on any order book change. For active markets, silence >60s is abnormal. 120s avoids false positives on low-activity markets while catching genuine freezes.

### Anti-Patterns to Avoid
- **Watchdog in the client:** The client is "connect once, read until done." Moving reconnection logic there violates the supervisor pattern used by all four venues.
- **Application-level heartbeat messages:** Polymarket has no application-level heartbeat request. PING/PONG works even during silent freezes, so it cannot detect them.
- **Polling `last_message_at` from a separate task:** Adds complexity. The `tokio::time::timeout` in the select loop is simpler, zero-allocation, and follows existing patterns.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Exponential backoff | Custom retry loop | `backoff` crate (already used) | Edge cases: jitter, overflow, reset semantics |
| Inactivity timer | `Instant::now()` tracking + manual checks | `tokio::time::timeout` | Handles cancellation, edge cases, no manual state |
| Prometheus metrics | Custom metric tracking | `metrics` crate macros (already used) | Consistent with all other venues |

## Common Pitfalls

### Pitfall 1: Silent Freeze vs Connection Drop
**What goes wrong:** Treating all failures as connection drops. The WS stays OPEN during a silent freeze -- `raw_rx.recv()` blocks forever, no reconnect happens.
**Why it happens:** Current supervisor only breaks on `None` (channel closed). Silent freeze keeps the channel open.
**How to avoid:** `tokio::time::timeout` on `recv()` -- the core of this phase.
**Warning signs:** `feed_available` gauge shows 1.0 (connected) but `feed_messages_total` counter stops incrementing.

### Pitfall 2: Timeout Resets on Any Message, Not Just Data
**What goes wrong:** Resetting the inactivity timer on PING/PONG frames.
**Why it happens:** PING/PONG works during silent freezes (confirmed in GitHub #292).
**How to avoid:** The timeout wraps `raw_rx.recv()` which only receives `RawMessage` (text frames). The client's spawned task filters out PING/PONG internally and only sends text frames through the channel. So this pitfall is already avoided by the existing architecture.

### Pitfall 3: Aggressive Reconnect on Low-Activity Markets
**What goes wrong:** Timeout fires because a market genuinely has no trades for 2+ minutes, causing unnecessary reconnections.
**Why it happens:** Not all markets are liquid. Some may go minutes without order book changes.
**How to avoid:** 120s default is generous. The subscription includes multiple token_ids, so if any market is active, the timeout resets. Log the reconnect reason so operators can tune the timeout.

### Pitfall 4: Backoff Reset on Timeout
**What goes wrong:** Resetting backoff when a timeout triggers reconnection, leading to rapid reconnect cycles if the server-side issue persists.
**Why it happens:** Confusing "timeout" with "intentional reconnect" (like subscription change).
**How to avoid:** Do NOT reset backoff on timeout. Only reset on first successful message (existing behavior).

### Pitfall 5: REST /book Endpoint Returns Stale Data
**What goes wrong:** Using `/book` or `/get_order_book` as fallback/diagnostic -- returns 0.99/0.01 ghost data.
**Why it happens:** Known Polymarket bug (GitHub #180). The `/book` endpoint is disconnected from live data.
**How to avoid:** Use `/midpoint` or `/price` for REST validation. Phase 42 REST fallback will use these endpoints.

### Pitfall 6: Diagnostic Uses Placeholder Token IDs
**What goes wrong:** Diagnostic test subscribes with placeholder token IDs from config.toml and gets no data (because the market may be resolved/inactive).
**Why it happens:** The default config has example token IDs that may not correspond to active markets.
**How to avoid:** Diagnostic should either use a known-active market token ID or first query the Gamma API for active markets.

## Code Examples

### Data Inactivity Timeout in Supervisor Select Loop
```rust
// Source: Adaptation of existing PolymarketSupervisor::run() pattern
// The key change is wrapping raw_rx.recv() with tokio::time::timeout

let data_timeout = Duration::from_secs(self.config.data_timeout_secs);

loop {
    tokio::select! {
        biased;

        _ = self.cancel.cancelled() => {
            tracing::info!("PolymarketSupervisor cancelled during forwarding");
            return;
        }

        result = self.assets_rx.changed() => {
            match result {
                Ok(()) => {
                    tracing::info!("PolymarketSupervisor: asset list updated, reconnecting");
                    backoff.reset();
                    break;
                }
                Err(_) => {
                    tracing::warn!("PolymarketSupervisor: subscription channel closed");
                }
            }
        }

        result = tokio::time::timeout(data_timeout, raw_rx.recv()) => {
            match result {
                Ok(Some(raw)) => {
                    if !received_first {
                        received_first = true;
                        backoff.reset();
                        self.health.mark_available();
                    }
                    if tx.send(raw).await.is_err() {
                        return;
                    }
                }
                Ok(None) => {
                    self.health.mark_unavailable("connection lost".to_string());
                    break;
                }
                Err(_elapsed) => {
                    // Silent freeze detected
                    self.health.mark_unavailable("data inactivity timeout".to_string());
                    metrics::counter!(
                        "feed_data_timeout_total",
                        "venue" => "polymarket"
                    ).increment(1);
                    tracing::warn!(
                        timeout_secs = self.config.data_timeout_secs,
                        "PolymarketSupervisor: data inactivity detected, forcing reconnect"
                    );
                    break;
                }
            }
        }
    }
}
```

### New Prometheus Metrics
```rust
// New metric for data inactivity timeouts (counter)
metrics::counter!("feed_data_timeout_total", "venue" => "polymarket").increment(1);

// Existing metrics already in place:
// metrics::gauge!("feed_available", "venue" => "polymarket")   -- 0/1 liveness
// metrics::counter!("feed_reconnections_total", "venue" => "polymarket")  -- connection attempts
// metrics::counter!("feed_messages_total", "venue" => "polymarket")  -- messages received
// metrics::histogram!("feed_latency_ms", "venue" => "polymarket")  -- exchange-to-local latency
```

### Diagnostic Test Structure
```rust
// tests/polymarket_diag.rs or src/bin/polymarket_diag.rs

#[tokio::test]
#[ignore] // Only run manually on EC2
async fn diagnose_polymarket_ws_from_this_host() {
    // 1. Test REST /midpoint as baseline
    let rest_result = reqwest::get(
        "https://clob.polymarket.com/midpoint?token_id=KNOWN_ACTIVE_TOKEN_ID"
    ).await;
    println!("REST /midpoint: {:?}", rest_result);

    // 2. Test WS connection
    let ws_result = tokio_tungstenite::connect_async(
        "wss://ws-subscriptions-clob.polymarket.com/ws/market"
    ).await;

    match ws_result {
        Err(e) => println!("DIAGNOSIS: Connection failed: {e}"),
        Ok((ws, _)) => {
            let (mut write, mut read) = ws.split();
            // 3. Subscribe
            write.send(Message::text(subscribe_json)).await.unwrap();
            // 4. Wait for first message with timeout
            match tokio::time::timeout(Duration::from_secs(30), read.next()).await {
                Err(_) => println!("DIAGNOSIS: Silent freeze (no data in 30s)"),
                Ok(Some(Ok(msg))) => println!("DIAGNOSIS: Working! Got: {:?}", msg),
                Ok(Some(Err(e))) => println!("DIAGNOSIS: Read error: {e}"),
                Ok(None) => println!("DIAGNOSIS: Connection closed immediately"),
            }
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Detect only connection drops | Detect both drops AND silent freezes | This phase | Covers GitHub #292 failure mode |
| No REST validation | REST /midpoint as diagnostic baseline | This phase | Independent data source confirms market is active |
| Manual EC2 debugging | Automated diagnostic test | This phase | Repeatable, documented diagnosis |

## Open Questions

1. **Is the EC2 "connection reset by peer" a geo-block or rate limit?**
   - What we know: EC2 us-east-1 gets "connection reset" errors. Polymarket is US-based, so geo-blocking is unlikely for us-east-1.
   - What's unclear: Whether it's rate limiting, IP reputation, or transient infrastructure issue.
   - Recommendation: The diagnostic tool will answer this definitively. If connection works but data doesn't flow, it's silent freeze. If connection fails consistently, document and defer to Phase 42 REST fallback.

2. **Optimal data_timeout_secs value?**
   - What we know: GitHub #292 reporter uses 120s. Active markets should produce events every few seconds.
   - What's unclear: How quiet low-activity markets get.
   - Recommendation: Default 120s, configurable via TOML. Operators can tune based on observed patterns.

3. **Should we limit token_ids per connection to avoid triggering server-side throttling?**
   - What we know: GitHub #292 reporter subscribes 250 tokens/connection. Our system currently subscribes dynamically via SubscriptionManager.
   - What's unclear: Whether Polymarket has undocumented per-connection limits.
   - Recommendation: Out of scope for this phase. Current subscription counts are likely small. Document as future optimization if needed.

## Sources

### Primary (HIGH confidence)
- Existing codebase: `src/feed/polymarket/supervisor.rs`, `client.rs`, `normalize.rs` -- current architecture
- Existing codebase: `src/feed/health.rs` -- VenueHealth tracker with `last_message_at`, `mark_available/unavailable`
- Existing codebase: `src/config/venues.rs` -- PolymarketConfig and ReconnectConfig structures

### Secondary (MEDIUM confidence)
- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- Official WS docs: PING every 10s, subscription format, dynamic subscribe/unsubscribe
- [GitHub #292: Silent freeze issue](https://github.com/Polymarket/py-clob-client/issues/292) -- Detailed reproduction of silent freeze, workaround (120s data watchdog + REST spot-checks)
- [GitHub #180: Stale /book endpoint](https://github.com/Polymarket/py-clob-client/issues/180) -- REST `/book` returns ghost data; use `/midpoint` or `/price` instead
- [GitHub #26: RTDS stream stops](https://github.com/Polymarket/real-time-data-client/issues/26) -- Similar freeze on RTDS endpoint, confirmed server-side issue

### Tertiary (LOW confidence)
- None -- all findings verified with multiple sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- zero new dependencies, all patterns from existing codebase
- Architecture: HIGH -- direct extension of existing supervisor pattern with `tokio::time::timeout`
- Pitfalls: HIGH -- confirmed by GitHub issues with reproduction details and multiple independent reporters
- Diagnostic approach: MEDIUM -- depends on EC2 runtime behavior which cannot be verified from dev machine

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable domain; Polymarket API changes slowly)
