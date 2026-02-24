# Phase 12: Kalshi Feed Hardening - Research

**Researched:** 2026-02-24
**Domain:** WebSocket connection reliability, exchange timestamp handling, feed latency metrics
**Confidence:** HIGH

## Summary

This phase addresses four v1.0 audit gaps where the Kalshi feed lacks features already present on Deribit and Polymarket: dead connection detection (RELY-02), exchange timestamp logging (FEED-08), and per-feed latency metrics (TIME-02, TIME-03). The audit found that Kalshi supervisor has no heartbeat/dead-connection detection protocol, `exchange_timestamp` is always `None`, and latency metrics are never emitted.

The research reveals two key discoveries that change the implementation approach from what the audit assumed:

1. **Kalshi DOES have a server-side heartbeat**: Kalshi sends WebSocket Ping frames (opcode 0x9) every 10 seconds with body "heartbeat". `tokio-tungstenite` automatically responds with Pong frames, but the current code has no timeout to detect when pings STOP arriving (dead connection). The fix is to add a client-side timeout in the Kalshi WS loop (analogous to Deribit's `last_message_at` + timeout pattern), not to build an application-level heartbeat protocol.

2. **Kalshi orderbook_delta messages contain a `ts` field**: The Kalshi API wraps messages as `{type, sid, seq, msg: {...}}` where the `msg` object includes a `ts` field in ISO 8601 format (e.g., `"2022-11-22T20:44:01Z"`). The current parser ignores this field. However, orderbook_snapshot messages may NOT include `ts`, meaning timestamp availability is partial. The best-effort approach is: parse `ts` when present, fall back to local receipt time when absent, and always document the limitation.

**Primary recommendation:** Add dead-connection timeout to KalshiClient WS loop using the existing Deribit pattern (track `last_message_at`, timeout at 30s = 3x Kalshi's 10s ping interval), parse the `ts` field from orderbook_delta messages for best-effort exchange timestamps, and emit `feed_latency_ms` metrics where exchange timestamps are available.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| RELY-02 | Detect stale connections via per-venue heartbeat monitoring | Kalshi sends WS Ping every 10s; add timeout detection in client WS loop (30s = 3x interval). Pattern: Deribit `last_message_at` + `sleep_until` timeout in tokio::select. |
| FEED-08 | Log exchange-reported timestamps alongside local receipt timestamps | Kalshi `orderbook_delta` messages include `ts` field (ISO 8601). Parse in message layer, propagate through normalizer to `MarketSnapshot.exchange_timestamp`. Document that snapshots may lack `ts`. |
| TIME-02 | All logged data includes both local receipt and exchange-reported timestamps | Parse `ts` from Kalshi messages where available, set `exchange_timestamp` on MarketSnapshot. Best-effort: `Some(ts_millis)` for deltas with `ts`, `None` for snapshots without. |
| TIME-03 | Per-feed latency characteristics documented and tracked in metrics | Emit `feed_latency_ms` and `feed_last_latency_ms` histograms/gauges for Kalshi venue label, matching Deribit/Polymarket pattern. Only emit when `exchange_timestamp` is `Some`. Document that coverage is partial. |
</phase_requirements>

## Standard Stack

### Core

No new dependencies required. All work uses existing crate infrastructure.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | (existing) | Async runtime, `tokio::time::Instant`, `sleep_until` for timeout | Already used everywhere |
| tokio-tungstenite | (existing) | WS Ping/Pong handled automatically by library | Already used in all 3 clients |
| chrono | (existing) | ISO 8601 timestamp parsing via `DateTime::parse_from_rfc3339` | Already used throughout |
| metrics | (existing) | `histogram!`, `gauge!`, `counter!` macros for latency metrics | Already emitting for Deribit/Polymarket |
| serde / serde_json | (existing) | Deserialize `ts` field from Kalshi messages | Already used in message parsing |

### Supporting

No additional libraries needed.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| chrono `parse_from_rfc3339` | Manual string parsing | chrono is already a dep; manual parsing is error-prone for timezone handling |
| 30s timeout (3x ping interval) | 20s timeout (2x interval) | Deribit uses 2x but Kalshi has no formal spec for what happens on missed pong; 3x is more conservative and avoids false positives |
| Application-level ping from client | Rely on server pings only | Kalshi docs say clients CAN send Ping frames; however server already sends at 10s interval, client ping is redundant for detection |

## Architecture Patterns

### Recommended Changes

```
src/feed/kalshi/
  client.rs       # Add dead-connection timeout (Deribit pattern)
  messages.rs     # Update message structs to parse nested {type, sid, seq, msg} wrapper + ts field
  normalize.rs    # Propagate exchange_timestamp to MarketSnapshot + emit latency metrics
  supervisor.rs   # No changes needed (already has backoff/reconnect)
```

### Pattern 1: Dead Connection Timeout (from Deribit client)

**What:** Track `last_message_at: Instant` in the WS read loop, add a `tokio::time::sleep_until` branch in `tokio::select!` that fires if no message arrives within the timeout period.

**When to use:** Always -- this is the primary mechanism for detecting dead Kalshi connections.

**Key difference from Deribit:** Deribit heartbeat is application-level (JSON-RPC `set_heartbeat` + `test_request` response). Kalshi heartbeat is transport-level (WebSocket Ping/Pong frames). Since `tokio-tungstenite` handles Pong automatically AND surfaces `Message::Ping` / `Message::Pong` in the stream, we track liveness on ALL received messages (including Ping/Pong frames), not just text messages.

**Example (from Deribit client.rs, adapted for Kalshi):**
```rust
// In KalshiClient WS loop:
let timeout_duration = Duration::from_secs(30); // 3x Kalshi's 10s ping interval
let mut last_message_at = Instant::now();

loop {
    let timeout_deadline = last_message_at + timeout_duration;

    tokio::select! {
        biased;

        _ = cancel.cancelled() => {
            let _ = write.send(Message::Close(None)).await;
            break;
        }

        _ = tokio::time::sleep_until(timeout_deadline) => {
            tracing::warn!(
                elapsed_ms = last_message_at.elapsed().as_millis() as u64,
                "Kalshi heartbeat timeout -- no messages/pings received"
            );
            break; // Supervisor will reconnect
        }

        msg = read.next() => {
            match msg {
                Some(Ok(Message::Text(text))) => {
                    last_message_at = Instant::now();
                    // ... forward to raw channel
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                    // tokio-tungstenite auto-responds with Pong.
                    // Update liveness tracker -- this IS the Kalshi heartbeat.
                    last_message_at = Instant::now();
                    tracing::trace!("Kalshi WS ping/pong received (heartbeat)");
                }
                // ... other arms unchanged
            }
        }
    }
}
```

### Pattern 2: Nested Message Wrapper Parsing

**What:** Kalshi WS messages use a `{type, sid, seq, msg: {...}}` envelope. The actual data fields are inside `msg`. The current code parses a flat structure (fields at top level), which may work with some API versions but misses `sid`, `seq`, and nested `ts`.

**When to use:** When updating message parsing to extract exchange timestamps.

**Approach:** Update `KalshiMessage::parse()` to handle BOTH flat format (backward compat with existing recordings) and nested `msg` wrapper format. Use defensive parsing: check if `msg` key exists, parse from there; otherwise fall back to flat parsing.

**Example:**
```rust
// New message wrapper types:
#[derive(Deserialize)]
struct KalshiEnvelope {
    #[serde(rename = "type")]
    msg_type: String,
    sid: Option<u64>,
    seq: Option<u64>,
    msg: Option<serde_json::Value>,
}

// In parse():
// 1. Try to deserialize as KalshiEnvelope
// 2. If `msg` field present, parse inner payload from msg value
// 3. If `msg` field absent, parse from top-level value (legacy compat)
```

### Pattern 3: Best-Effort Exchange Timestamp Estimation

**What:** Parse `ts` (ISO 8601) from Kalshi orderbook_delta `msg` payload, convert to epoch millis, set on `MarketSnapshot.exchange_timestamp`.

**When to use:** Whenever processing Kalshi orderbook messages.

**Key constraints:**
- `ts` may not be present on all message types (orderbook_snapshot may lack it)
- `ts` precision is seconds (ISO 8601 format like `"2022-11-22T20:44:01Z"`) -- much coarser than Deribit's millisecond timestamps
- When `ts` is absent, `exchange_timestamp` stays `None` (same as current behavior)
- When `ts` is present, it enables latency metrics for that message

**Example:**
```rust
// In normalize.rs, when building snapshot:
let exchange_ts_ms: Option<i64> = kalshi_ts_iso.and_then(|ts_str| {
    DateTime::parse_from_rfc3339(&ts_str)
        .ok()
        .map(|dt| dt.timestamp_millis())
});
```

### Pattern 4: Latency Metrics Emission (from Deribit/Polymarket normalizers)

**What:** Emit `feed_latency_ms` histogram and `feed_last_latency_ms` gauge when `exchange_timestamp` is `Some`.

**When to use:** In `KalshiProcessor::produce_snapshot()`.

**Example (exact pattern from Deribit normalize.rs:496-504):**
```rust
if let Some(exchange_ts_ms) = exchange_ts {
    let local_ms = received_at.wall().timestamp_millis();
    let latency_ms = (local_ms - exchange_ts_ms) as f64;
    metrics::histogram!("feed_latency_ms", "venue" => "kalshi").record(latency_ms);
    metrics::gauge!("feed_last_latency_ms", "venue" => "kalshi").set(latency_ms);
}
metrics::counter!("feed_messages_total", "venue" => "kalshi").increment(1);
```

### Anti-Patterns to Avoid

- **Building a custom application-level heartbeat for Kalshi:** Kalshi already has transport-level WS Ping/Pong. Do NOT send `public/set_heartbeat` or similar -- Kalshi has no such API.
- **Blocking on timestamp parsing failures:** If `ts` parsing fails, log a warning and continue with `None`. Never crash or reject a message because of a timestamp parse error.
- **Assuming `ts` is always present:** Some message types (snapshots, subscribed acks) will not have `ts`. Always treat exchange timestamp as `Option`.
- **Using `received_at` as a substitute for `exchange_timestamp`:** The whole point of FEED-08/TIME-02 is to distinguish local receipt time from exchange time. When `ts` is absent, leave `exchange_timestamp: None` -- do not fake it with local time.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ISO 8601 timestamp parsing | Custom string splitting | `chrono::DateTime::parse_from_rfc3339` | Handles timezone offsets, sub-second precision, edge cases |
| Dead connection detection | Custom timer task | `tokio::time::sleep_until` in select loop | Exact pattern from Deribit client, battle-tested |
| Pong response to Kalshi Ping | Manual Pong frame construction | `tokio-tungstenite` automatic Pong | Library handles this transparently; manual pong would conflict |
| Prometheus histogram buckets | Custom bucket configuration for Kalshi | Existing `feed_latency_ms` matcher in `metrics_export/mod.rs` | Already configured with 1ms-10s buckets for all venues |

**Key insight:** Every mechanism needed is already implemented for Deribit or Polymarket. This phase is purely about applying existing patterns to Kalshi, not inventing new mechanisms.

## Common Pitfalls

### Pitfall 1: tokio-tungstenite Pong Interference

**What goes wrong:** Manually sending a Pong frame in response to Kalshi's Ping, which conflicts with tokio-tungstenite's automatic Pong response.
**Why it happens:** Developer sees `Message::Ping` in the stream and instinctively writes a Pong response, not knowing the library already queued one.
**How to avoid:** Do NOT write any Pong-sending code. The existing `Some(Ok(Message::Ping(_)))` arm in the Kalshi client already correctly ignores Ping (with a comment about automatic handling). Just add `last_message_at` update.
**Warning signs:** Kalshi server receiving duplicate Pong frames per Ping; potential protocol violation.

### Pitfall 2: Message Format Ambiguity (Flat vs Nested)

**What goes wrong:** Breaking existing recordings and tests that use the flat message format when switching to nested `msg` wrapper parsing.
**Why it happens:** Kalshi API may have changed format between versions, or different channels use different formats. Existing test fixtures use flat format.
**How to avoid:** Parse defensively -- try nested format first, fall back to flat format. Keep all existing test cases passing. Add NEW test cases for the nested format alongside existing ones.
**Warning signs:** Existing Kalshi processor tests fail after message parser changes.

### Pitfall 3: Second-Precision Timestamp Skewing Latency Metrics

**What goes wrong:** Kalshi `ts` field appears to be second-precision ISO 8601 (e.g., `"2022-11-22T20:44:01Z"` -- no fractional seconds). Converting to millis gives a value that's always 0-999ms off from the actual event time. Latency metrics will have inherent jitter of up to 1 second.
**Why it happens:** Kalshi rounds timestamps to seconds; Deribit provides millisecond-precision timestamps.
**How to avoid:** Document this limitation. Label Kalshi latency metrics as "best-effort (second-precision exchange timestamps)". Consider adding a `feed_timestamp_precision` metadata label or log annotation. Do NOT treat Kalshi latency as authoritative as Deribit/Polymarket latency.
**Warning signs:** Kalshi latency metrics show step-function patterns or unexpected distributions compared to Deribit.

### Pitfall 4: Timeout Too Aggressive (False Positives)

**What goes wrong:** Setting timeout at 20s (2x Kalshi's 10s ping interval) causes false reconnections during brief network hiccups or server-side jitter.
**Why it happens:** Kalshi documentation does not specify what happens if a Pong is not received or if Ping delivery is delayed. Unlike Deribit (which has a documented heartbeat protocol with explicit server-side enforcement), Kalshi's Ping is best-effort.
**How to avoid:** Use 30s timeout (3x interval) as default. Make it configurable via `KalshiConfig` (new field: `heartbeat_timeout_ms`). Log a metric when timeout triggers to tune later.
**Warning signs:** Frequent Kalshi reconnections in metrics without corresponding actual disconnections.

### Pitfall 5: Breaking the Existing Staleness Logic

**What goes wrong:** After adding `exchange_timestamp` to Kalshi snapshots, the staleness gate in `SpreadEngine` now rejects Kalshi data for being "too old" based on exchange timestamp age.
**Why it happens:** The spread engine checks `exchange_timestamp` age against `staleness_threshold_ms`. If Kalshi's second-precision `ts` is even slightly delayed, it may trip the gate.
**How to avoid:** Kalshi staleness threshold is already 15s (vs Polymarket 5s) per decision `[06-03]`. The existing `staleness_threshold_ms` in `KalshiConfig` should be sufficient. But verify that second-precision timestamps don't cause edge cases at exactly the threshold boundary.
**Warning signs:** Kalshi data rejected by staleness gate immediately after adding exchange timestamps.

## Code Examples

### Example 1: Dead Connection Timeout Addition to KalshiClient

Source: Adapted from `src/feed/deribit/client.rs:140-208`

The current KalshiClient WS loop (client.rs:140-196) has NO timeout branch. Add `last_message_at` tracking and a `sleep_until` timeout:

```rust
// Add to KalshiClient::start(), inside the spawned task:
use tokio::time::{Duration, Instant};

let timeout_duration = Duration::from_secs(30); // 3x Kalshi 10s ping interval
let mut last_message_at = Instant::now();

loop {
    let timeout_deadline = last_message_at + timeout_duration;

    tokio::select! {
        biased;

        _ = cancel.cancelled() => {
            tracing::info!("Kalshi WS loop cancelled");
            let _ = write.send(Message::Close(None)).await;
            break;
        }

        _ = tokio::time::sleep_until(timeout_deadline) => {
            let elapsed = last_message_at.elapsed();
            tracing::warn!(
                elapsed_ms = elapsed.as_millis() as u64,
                timeout_ms = timeout_duration.as_millis() as u64,
                "Kalshi heartbeat timeout -- connection assumed dead"
            );
            metrics::counter!("feed_heartbeat_timeouts", "venue" => "kalshi").increment(1);
            break; // Exit loop -> channel closes -> supervisor reconnects
        }

        msg = read.next() => {
            match msg {
                Some(Ok(Message::Text(text))) => {
                    last_message_at = Instant::now();
                    // ... existing text handling
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                    last_message_at = Instant::now();
                    tracing::trace!("Kalshi WS ping/pong (heartbeat liveness)");
                }
                // ... existing arms for Close, Binary, Frame, Err, None
            }
        }
    }
}
```

### Example 2: Nested Message Parsing with Backward Compatibility

Source: Project codebase pattern + Kalshi API docs

```rust
// In messages.rs:

/// Kalshi WS envelope: {type, sid, seq, msg: {...}}
#[derive(Debug, Deserialize)]
struct KalshiEnvelope {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    sid: Option<u64>,
    seq: Option<u64>,
    msg: Option<serde_json::Value>,
}

/// Extended OrderbookDeltaData with optional ts field
#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookDeltaData {
    #[serde(alias = "market_id")]
    pub market_ticker: String,
    pub price: i64,
    pub delta: i64,
    pub side: String,
    pub seq: Option<u64>,
    /// Exchange timestamp (ISO 8601), if provided by API.
    /// e.g., "2022-11-22T20:44:01Z"
    pub ts: Option<String>,
}

impl KalshiMessage {
    pub fn parse(text: &str) -> Self {
        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return KalshiMessage::Unknown(text.to_string()),
        };

        // Determine if this is wrapped (has "msg" object) or flat format
        let (msg_type, payload) = if let Some(msg_obj) = value.get("msg") {
            // Wrapped format: {type, sid, seq, msg: {...}}
            let t = value.get("type").and_then(|t| t.as_str()).map(String::from);
            (t, msg_obj.clone())
        } else {
            // Flat format (legacy/existing recordings)
            let t = value.get("type").and_then(|t| t.as_str()).map(String::from);
            (t, value.clone())
        };

        // ... rest of parsing uses `payload` instead of `value`
    }
}
```

### Example 3: Exchange Timestamp Propagation in Normalizer

Source: Adapted from `src/feed/deribit/normalize.rs:496-504` and `src/feed/polymarket/normalize.rs:172-177`

```rust
// In normalize.rs produce_snapshot():

// Parse exchange timestamp from the last delta's ts field (best-effort)
let exchange_ts_ms: Option<i64> = last_ts_iso.and_then(|ts_str| {
    chrono::DateTime::parse_from_rfc3339(&ts_str)
        .map(|dt| dt.timestamp_millis())
        .map_err(|e| {
            tracing::warn!(
                ts = %ts_str,
                error = %e,
                "failed to parse Kalshi exchange timestamp"
            );
            e
        })
        .ok()
});

// Latency metrics (only when exchange timestamp available)
if let Some(exchange_ts_ms) = exchange_ts_ms {
    let local_ms = received_at.wall().timestamp_millis();
    let latency_ms = (local_ms - exchange_ts_ms) as f64;
    metrics::histogram!("feed_latency_ms", "venue" => "kalshi").record(latency_ms);
    metrics::gauge!("feed_last_latency_ms", "venue" => "kalshi").set(latency_ms);
}
metrics::counter!("feed_messages_total", "venue" => "kalshi").increment(1);

let snapshot = MarketSnapshot {
    // ...
    exchange_timestamp: exchange_ts_ms,  // Was: None
    // ...
};
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No Kalshi heartbeat detection | Rely on Kalshi's WS Ping (10s) + client-side timeout | This phase | Dead connections detected within 30s |
| Kalshi exchange_timestamp: None | Parse `ts` from orderbook_delta `msg` payload | This phase | Best-effort latency metrics; partial coverage |
| Latency metrics for Deribit/Polymarket only | All 3 venues emit `feed_latency_ms` | This phase | Complete venue coverage (Kalshi best-effort) |

**Protocol limitation (must be documented):**
Kalshi's `ts` field is second-precision ISO 8601 vs Deribit's millisecond-precision epoch timestamps. Kalshi latency metrics will have up to 999ms of inherent jitter. This is a fundamental protocol limitation, not a bug.

## Open Questions

1. **Does `orderbook_snapshot` include a `ts` field?**
   - What we know: `orderbook_delta` confirmed to have `ts` in the `msg` payload. Snapshot format less documented.
   - What's unclear: Whether snapshots also carry `ts`. The Go SDK's snapshot struct does NOT include a timestamp field.
   - Recommendation: Parse `ts` from both message types if present. If snapshot lacks `ts`, the first delta will set `exchange_timestamp`. This is acceptable since snapshots are infrequent (only on initial subscribe or reconnect).

2. **Is the nested `msg` wrapper format the only format, or do some channels still use flat format?**
   - What we know: The Go SDK and API docs show nested format. Existing codebase tests use flat format.
   - What's unclear: Whether the Kalshi production API currently uses flat or nested format. The existing code works (per Phase 4 integration), suggesting either flat format is still valid or the parser happens to extract fields correctly from nested structure.
   - Recommendation: Support BOTH formats defensively. Try nested first, fall back to flat. This preserves backward compatibility with recordings and handles API format changes gracefully.

3. **Exact timeout value for dead connection detection.**
   - What we know: Kalshi pings every 10s. Deribit uses 2x heartbeat interval.
   - What's unclear: How reliably Kalshi sends pings. Whether 20s or 30s is better.
   - Recommendation: Default to 30s (3x), make configurable via `KalshiConfig.heartbeat_timeout_ms`. Can be tuned with operational data.

## Sources

### Primary (HIGH confidence)

- [Kalshi Connection Keep-Alive Docs](https://docs.kalshi.com/websockets/connection-keep-alive) - Confirms Ping frames every 10s with body "heartbeat", clients should respond with Pong (tokio-tungstenite does this automatically)
- [Kalshi Orderbook Updates Docs](https://docs.kalshi.com/websockets/orderbook-updates) - Confirms orderbook_snapshot then orderbook_delta pattern
- [Kalshi API Changelog](https://docs.kalshi.com/changelog) - Confirms client_order_id addition to orderbook_delta (Aug 2025), ticker enhancements (Feb 2026)
- Project codebase: `src/feed/deribit/client.rs` - Reference implementation for heartbeat timeout pattern (lines 140-208)
- Project codebase: `src/feed/deribit/normalize.rs` - Reference implementation for latency metrics (lines 496-504)
- Project codebase: `src/feed/polymarket/normalize.rs` - Reference implementation for latency metrics (lines 172-177)

### Secondary (MEDIUM confidence)

- [Kalshi Quick Start WebSockets](https://docs.kalshi.com/getting_started/quick_start_websockets) - Subscribe message format, auth headers
- [Kalshi Orderbook Responses](https://docs.kalshi.com/getting_started/orderbook_responses) - Confirms price in cents, yes/no arrays
- [ammario/kalshi Go SDK feed.go](https://github.com/ammario/kalshi/blob/main/feed.go) - Third-party Go SDK showing message struct layout: nested {type, sid, seq, msg} format, no `ts` in snapshot struct but present in delta
- Web search results showing `orderbook_delta` `msg` payload includes `ts: "2022-11-22T20:44:01Z"` (ISO 8601)

### Tertiary (LOW confidence)

- [tokio-tungstenite Issue #88](https://github.com/snapview/tokio-tungstenite/issues/88) - Confirms automatic Pong handling in async mode (needs verification against current version, but behavior is consistent in our codebase)
- Kalshi `ts` field precision: Example shows second-precision only ("2022-11-22T20:44:01Z" with no fractional seconds). Actual production data may differ. Flag for validation during implementation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - No new dependencies, all patterns already proven in Deribit/Polymarket code
- Architecture: HIGH - Direct adaptation of existing Deribit heartbeat timeout pattern and Deribit/Polymarket latency metric pattern
- Pitfalls: MEDIUM - Message format (flat vs nested) needs defensive handling; `ts` field presence and precision need runtime validation
- Kalshi heartbeat mechanism: HIGH - Officially documented at docs.kalshi.com/websockets/connection-keep-alive
- `ts` field existence: MEDIUM - Confirmed by web search and Go SDK, but not fully verified in official docs schema

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable domain -- WS protocols rarely change)
