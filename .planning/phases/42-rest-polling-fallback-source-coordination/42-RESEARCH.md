# Phase 42: REST Polling Fallback and Source Coordination - Research

**Researched:** 2026-03-09
**Domain:** Polymarket CLOB REST API, tokio async state machines, venue feed architecture
**Confidence:** HIGH

## Summary

Phase 42 adds a REST-based price poller for Polymarket as a fallback when WebSocket is unavailable, plus a source coordinator that ensures exactly one data source (WS or REST) is active at a time per venue.

The codebase already has all required infrastructure: `reqwest::Client` is used in settlement/discovery code, `VenueRateLimiter` wraps the `governor` crate, and the Polymarket pipeline follows a clear `Supervisor -> RawMessage -> Processor -> MarketSnapshot` pattern. The REST poller needs to produce `MarketSnapshot` values identical to those from the WS normalizer, feeding into the same fan-in channel.

The critical design decision is WHERE the source coordinator lives. The supervisor already owns the reconnection loop and VenueHealth state. The coordinator should wrap the supervisor level, controlling whether to spawn a WS supervisor loop iteration or a REST poll loop iteration, using the existing `data_timeout_secs` signal as the trigger to switch from WS to REST.

**Primary recommendation:** Build a `PolymarketRestPoller` that calls `/midpoint` (not `/book`) per token_id with rate limiting, and a `SourceCoordinator` that wraps the existing supervisor, switching between WS and REST modes based on VenueHealth state and data timeout signals.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| POLY-04 | REST polling fallback fetches Polymarket prices when WebSocket is unavailable, using existing reqwest/governor | Use `/midpoint` endpoint (not `/book` which has stale ghost data per GitHub #180). Existing `reqwest::Client` in codebase, `VenueRateLimiter` for governor rate limiting. Produce `MarketSnapshot` with bid/ask/probability from midpoint. |
| POLY-05 | Source coordinator switches between WebSocket and REST modes exclusively (no duplicate/conflicting prices) | Coordinator state machine with two modes (WS, REST). Uses `data_timeout_secs` as WS->REST trigger. REST->WS switchback after WS delivers sustained data. Exclusive mode prevents duplicate snapshots. |
</phase_requirements>

## Standard Stack

### Core (already in codebase)
| Library | Purpose | Why Standard |
|---------|---------|--------------|
| reqwest | HTTP client for REST API calls | Already used in settlement + discovery modules |
| governor | Rate limiting for REST polls | Already wrapped in `VenueRateLimiter` |
| tokio | Async runtime, timers, channels | Core runtime for all async code |
| serde/serde_json | JSON deserialization of REST responses | Already used throughout |
| rust_decimal | Decimal price parsing | Already used in `PolymarketProcessor` |

### No New Dependencies Required

All required libraries are already in `Cargo.toml`. No new crate additions needed.

## Architecture Patterns

### Current Polymarket Pipeline (reference)
```
PolymarketSupervisor (reconnection loop)
  -> mpsc::channel<RawMessage>
    -> PolymarketProcessor (normalize)
      -> mpsc::channel<MarketSnapshot>
        -> forward_snapshots (fan-in to shared channel)
```

### Recommended Architecture for Phase 42
```
src/feed/polymarket/
  client.rs          # WS client (existing)
  rest_poller.rs     # NEW: REST polling client
  coordinator.rs     # NEW: Source coordinator (WS/REST switching)
  supervisor.rs      # Existing: WS reconnection loop
  normalize.rs       # Existing: WS message processor
  messages.rs        # Existing: WS message types
  mod.rs             # Updated: export new modules
```

### Pattern 1: REST Poller
**What:** A struct that polls Polymarket `/midpoint` endpoint per token_id at a configurable interval, producing `MarketSnapshot` values on an mpsc channel.
**When to use:** When WS is unavailable (timeout, connection reset, silent freeze).

```rust
// REST poller produces MarketSnapshot directly (no RawMessage intermediate)
pub struct PolymarketRestPoller {
    config: PolymarketConfig,
    client: reqwest::Client,
    rate_limiter: VenueRateLimiter,
    cancel: CancellationToken,
    poll_interval: Duration,
}

impl PolymarketRestPoller {
    /// Poll all configured token_ids and produce MarketSnapshot values.
    /// Runs until cancelled or channel closes.
    pub async fn run(
        self,
        assets: Vec<PolymarketAsset>,
        tx: mpsc::Sender<MarketSnapshot>,
    ) { ... }
}
```

### Pattern 2: Source Coordinator (State Machine)
**What:** Controls which data source is active for Polymarket. Ensures exactly one source runs at a time.
**When to use:** Always -- replaces the direct supervisor spawn in `pipeline.rs`.

```rust
pub enum SourceMode {
    WebSocket,
    Rest,
}

pub struct SourceCoordinator {
    config: PolymarketConfig,
    health: Arc<VenueHealth>,
    current_mode: SourceMode,
    // ... channels, cancel tokens
}
```

**State transitions:**
- **WS -> REST:** When `data_timeout_secs` fires (no WS data received within timeout). This already triggers `mark_unavailable("data inactivity timeout")` in the current supervisor.
- **REST -> WS:** After REST is running, periodically attempt WS reconnection. If WS delivers `N` consecutive messages (e.g., first message received), switch back. Use a configurable `ws_recovery_check_secs` interval.
- **WS connection error -> REST:** Immediate fallback on connection failure after backoff exhaustion.

### Pattern 3: Integration Point in pipeline.rs
**What:** Replace the current direct `PolymarketSupervisor` spawn with `SourceCoordinator` spawn.
**Current code in pipeline.rs (lines 226-280):**
```rust
// Current: spawns supervisor directly
let supervisor = PolymarketSupervisor::new(...);
tokio::spawn(supervisor.run(supervisor_tx));
let (processor, venue_snapshot_rx) = PolymarketProcessor::new(supervisor_rx, ...);
tokio::spawn(processor.run());
```

**New pattern:**
```rust
// New: coordinator manages both WS and REST
let coordinator = SourceCoordinator::new(
    config.polymarket.clone(),
    assets_rx,
    venue_cancel.clone(),
    health.clone(),
    poly_rate_limiter.clone(),
    http_client.clone(), // shared reqwest client
);
// Coordinator sends MarketSnapshot directly to fan-in
tokio::spawn(coordinator.run(snapshot_fan_in_tx));
```

### Pattern 4: REST Response Handling
**What:** The `/midpoint` endpoint returns `{ "mid": "0.55" }`. This needs to be converted to a `MarketSnapshot`.

```rust
#[derive(Deserialize)]
struct MidpointResponse {
    mid: String,
}

// Convert midpoint to MarketSnapshot
fn midpoint_to_snapshot(
    token_id: &str,
    midpoint: Decimal,
    received_at: DualTimestamp,
    sequence: u64,
) -> MarketSnapshot {
    let price = Price::new(midpoint);
    let prob = Probability::new(midpoint).ok(); // Polymarket prices ARE probabilities
    MarketSnapshot {
        venue: Venue::Polymarket,
        instrument_id: InstrumentId::new(token_id),
        bid: Some(price),      // midpoint as best estimate
        ask: Some(price),      // midpoint as best estimate
        bid_probability: prob,
        ask_probability: prob,
        depth_bids: vec![],    // REST midpoint has no depth
        depth_asks: vec![],
        is_stale: false,
        // ... other fields None/default
    }
}
```

### Anti-Patterns to Avoid
- **Don't use `/book` endpoint:** Returns stale ghost data (GitHub #180). Use `/midpoint` or `/price` instead.
- **Don't run WS and REST simultaneously:** Creates duplicate/conflicting price updates. POLY-05 requires exclusive mode.
- **Don't poll too frequently:** Polymarket public API allows 60 requests/minute. With multiple token_ids, stay well under this. Use `governor` rate limiter.
- **Don't create a separate processor for REST:** REST produces `MarketSnapshot` directly since there is no raw WS frame to normalize. The coordinator can send directly to the fan-in channel.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rate limiting | Token bucket | `governor` via `VenueRateLimiter` | Already in codebase, handles burst/sustained correctly |
| HTTP client | Raw TCP | `reqwest::Client` | Already in codebase, handles TLS, connection pooling |
| Backoff | Custom delay logic | `backoff::ExponentialBackoffBuilder` | Already used in supervisors |
| Cancellation | Manual flag checking | `CancellationToken` | Already used throughout, propagates correctly |

## Common Pitfalls

### Pitfall 1: REST `/book` Returns Stale Ghost Data
**What goes wrong:** Using `/book` endpoint returns data that appears valid but is actually stale (GitHub #180).
**Why it happens:** Server-side caching or CDN layer serves stale order book snapshots.
**How to avoid:** Use `/midpoint` endpoint which returns current calculated midpoint. No depth data but accurate price.
**Warning signs:** Book data with timestamps far in the past.

### Pitfall 2: Dual-Source Race Condition
**What goes wrong:** During mode switch, both WS and REST might briefly send data, creating conflicting prices in the fan-in channel.
**Why it happens:** Asynchronous task cancellation is not instantaneous.
**How to avoid:** Cancel the old source FIRST, drain any remaining messages, THEN start the new source. Use a guard pattern: coordinator owns a single `mpsc::Sender<MarketSnapshot>` and only passes it to the active source.
**Warning signs:** Two snapshots for the same token_id arriving within milliseconds with different prices.

### Pitfall 3: Rate Limit Exhaustion with Multiple Tokens
**What goes wrong:** Polling N tokens at interval T means N/T requests per second. With 10 tokens at 5s interval, that is 2 req/s -- fine. But with 50 tokens at 2s, that is 25 req/s which exceeds the 1 req/s public limit (60/min).
**Why it happens:** Each token_id requires a separate `/midpoint` call.
**How to avoid:** Use the batch `/midpoints` POST endpoint (up to 500 tokens per call). One request covers all tokens.
**Warning signs:** HTTP 429 responses.

### Pitfall 4: Infinite WS->REST->WS Oscillation
**What goes wrong:** WS connects briefly, gets one message, coordinator switches to WS, then WS fails again, switches to REST, repeat.
**Why it happens:** WS connection is unstable (exactly the EC2 issue this phase addresses).
**How to avoid:** Require sustained WS data (e.g., N messages within T seconds) before switching back from REST. Implement hysteresis -- don't switch back too eagerly.
**Warning signs:** Rapid mode switches visible in metrics/logs.

### Pitfall 5: Missing event_id Annotation on REST Snapshots
**What goes wrong:** REST-sourced `MarketSnapshot` values lack `event_id`, so downstream engines (CrossAssetEngine) can't correlate them.
**Why it happens:** WS snapshots get `event_id` annotated in `forward_snapshots()`. REST snapshots must go through the same path.
**How to avoid:** Route REST snapshots through the same `forward_snapshots` function that does EventRegistry lookup.

## Code Examples

### REST Midpoint Fetch (verified from Polymarket docs)
```rust
// Source: https://docs.polymarket.com/trading/clients/public
// GET https://clob.polymarket.com/midpoint?token_id={token_id}
// Response: { "mid": "0.55" }

#[derive(Debug, serde::Deserialize)]
struct MidpointResponse {
    mid: String,
}

async fn fetch_midpoint(
    client: &reqwest::Client,
    base_url: &str,
    token_id: &str,
    rate_limiter: &VenueRateLimiter,
) -> anyhow::Result<Decimal> {
    rate_limiter.wait().await;
    let resp: MidpointResponse = client
        .get(format!("{}/midpoint", base_url))
        .query(&[("token_id", token_id)])
        .send()
        .await?
        .json()
        .await?;
    let mid: Decimal = resp.mid.parse()
        .map_err(|e| anyhow::anyhow!("invalid midpoint '{}': {}", resp.mid, e))?;
    Ok(mid)
}
```

### Batch Midpoints Fetch (for multiple tokens)
```rust
// Source: https://docs.polymarket.com/trading/clients/public
// POST https://clob.polymarket.com/midpoints
// Body: [{"token_id": "abc..."}, {"token_id": "def..."}]
// Response: { "abc...": { "mid": "0.55" }, "def...": { "mid": "0.60" } }

#[derive(Debug, serde::Serialize)]
struct MidpointRequest {
    token_id: String,
}

async fn fetch_midpoints_batch(
    client: &reqwest::Client,
    base_url: &str,
    token_ids: &[String],
    rate_limiter: &VenueRateLimiter,
) -> anyhow::Result<HashMap<String, Decimal>> {
    rate_limiter.wait().await;
    let body: Vec<MidpointRequest> = token_ids.iter()
        .map(|id| MidpointRequest { token_id: id.clone() })
        .collect();
    let resp: HashMap<String, MidpointResponse> = client
        .post(format!("{}/midpoints", base_url))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    // Parse each midpoint
    let mut result = HashMap::new();
    for (token_id, mid_resp) in resp {
        if let Ok(mid) = mid_resp.mid.parse::<Decimal>() {
            result.insert(token_id, mid);
        }
    }
    Ok(result)
}
```

### Source Coordinator State Machine
```rust
// Coordinator loop pseudocode
loop {
    match self.mode {
        SourceMode::WebSocket => {
            // Run WS supervisor (existing code)
            // If data_timeout fires or connection fails:
            self.mode = SourceMode::Rest;
            metrics::gauge!("feed_source_mode", "venue" => "polymarket")
                .set(1.0); // 0=WS, 1=REST
            tracing::info!("Switching to REST polling mode");
        }
        SourceMode::Rest => {
            // Run REST poll loop
            // Periodically attempt WS reconnection
            // If WS delivers sustained data:
            self.mode = SourceMode::WebSocket;
            metrics::gauge!("feed_source_mode", "venue" => "polymarket")
                .set(0.0);
            tracing::info!("Switching back to WebSocket mode");
        }
    }
}
```

## Config Additions

```toml
[polymarket]
# Existing fields unchanged...

# REST polling configuration (new)
rest_poll_interval_secs = 5       # How often to poll when in REST mode
ws_recovery_check_secs = 60      # How often to attempt WS reconnection from REST mode
ws_recovery_threshold = 3        # Messages needed to confirm WS recovery
```

Corresponding Rust config additions to `PolymarketConfig`:
```rust
/// REST polling interval in seconds (how often to poll /midpoint).
#[serde(default = "default_rest_poll_interval")]
pub rest_poll_interval_secs: u64,

/// How often to attempt WS reconnection while in REST mode (seconds).
#[serde(default = "default_ws_recovery_check")]
pub ws_recovery_check_secs: u64,

/// Number of WS messages needed to confirm WS is recovered.
#[serde(default = "default_ws_recovery_threshold")]
pub ws_recovery_threshold: u32,
```

## Metrics

| Metric | Type | Labels | Purpose |
|--------|------|--------|---------|
| `feed_source_mode` | Gauge | `venue=polymarket` | 0=WS, 1=REST. Shows current active source |
| `feed_rest_polls_total` | Counter | `venue=polymarket`, `status=success/error` | REST poll success/failure count |
| `feed_rest_poll_duration_ms` | Histogram | `venue=polymarket` | REST poll latency |
| `feed_source_switches_total` | Counter | `venue=polymarket`, `from=ws/rest`, `to=rest/ws` | Mode switch count |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Use `/book` for REST data | Use `/midpoint` or `/price` | GitHub #180 discovery | Avoid stale ghost data |
| Single data source only | WS with REST fallback | This phase (42) | Reliable data when WS fails |
| Direct supervisor spawn | Coordinator-managed sources | This phase (42) | Clean exclusive-mode switching |

## Open Questions

1. **Batch vs Individual Midpoint Calls**
   - What we know: `/midpoints` POST accepts up to 500 tokens in one call. Currently only 1 token configured.
   - What's unclear: Exact response format of the batch endpoint (needs runtime verification).
   - Recommendation: Implement individual `/midpoint` first (simpler, currently only 1 token). Add batch support as an optimization if token count grows. The rate limiter handles either approach.

2. **REST Snapshot Fidelity**
   - What we know: `/midpoint` returns a single price (average of best bid/ask). The WS `book` event returns full depth.
   - What's unclear: Whether downstream engines (CrossAssetEngine) require bid/ask spread or just a probability value.
   - Recommendation: Set both `bid` and `ask` to the midpoint value. This is a reasonable approximation for the arbitrage signal calculation which primarily uses `bid_probability`/`ask_probability`. CrossAssetEngine uses `latest_prob` cache which stores a single probability value, so midpoint is sufficient.

3. **WS Recovery Attempt Mechanism**
   - What we know: While in REST mode, we should periodically try WS. Current supervisor has reconnection logic.
   - What's unclear: Whether to run a "probe" WS connection in parallel or to switch fully to WS and switch back if it fails quickly.
   - Recommendation: Use a probe approach -- attempt WS connection, wait for first N messages within timeout. If successful, switch. If not, stay on REST. This prevents data gaps during switch attempts.

## Sources

### Primary (HIGH confidence)
- Polymarket CLOB docs: https://docs.polymarket.com/trading/orderbook - REST endpoints verified
- Polymarket CLOB docs: https://docs.polymarket.com/trading/clients/public - Public methods verified
- Codebase: `src/feed/polymarket/supervisor.rs` - Current WS supervisor pattern
- Codebase: `src/feed/polymarket/normalize.rs` - Current WS processor pattern
- Codebase: `src/feed/pipeline.rs` - Current multi-venue pipeline wiring
- Codebase: `src/feed/reliability/rate_limiter.rs` - Existing governor rate limiter
- Codebase: `src/config/venues.rs` - PolymarketConfig structure
- Codebase: `config/venues.toml` - rest_url = "https://clob.polymarket.com"

### Secondary (MEDIUM confidence)
- GitHub #180: `/book` endpoint stale ghost data (referenced in STATE.md blockers)
- GitHub #292: WS silent freeze issue (referenced in STATE.md blockers)
- Polymarket public API rate limit: 60 req/min (from web search, not verified in official docs)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in codebase, no new dependencies
- Architecture: HIGH - follows existing supervisor/processor patterns with clear integration points
- REST API endpoints: HIGH - verified against official Polymarket documentation
- Pitfalls: HIGH - stale `/book` data and WS issues documented in project's own GitHub issues
- Rate limits: MEDIUM - 60 req/min from web search, needs runtime validation

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable APIs, established patterns)
