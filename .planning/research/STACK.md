# Stack Research: v1.7 Prediction Market Signal Pipeline

**Domain:** Polymarket WebSocket/REST connectivity, spread engine generalization
**Researched:** 2026-03-09
**Confidence:** HIGH (zero new crate dependencies; all changes are Rust code refactoring using existing deps)

## Scope

This document covers ONLY the stack additions/changes needed for v1.7 Prediction Market Signal Pipeline. The existing Rust application stack (v1.0-v1.6) is unchanged. This milestone is a **pure code change milestone** -- no new crate dependencies, no new infrastructure.

---

## Executive Finding: Zero New Dependencies

v1.7 adds zero new Rust crate dependencies. Every capability needed is already in the dependency tree:

- **reqwest 0.12** -- already used for Polymarket Gamma API, Kalshi REST, Derive REST discovery. Sufficient for REST-based order book polling fallback.
- **tokio-tungstenite 0.28** -- already used for all 4 venue WebSocket feeds. No changes needed for Polymarket WS fix.
- **serde/serde_json** -- already used for all JSON parsing. Sufficient for REST book response deserialization.
- **governor 0.8** -- already used for Polymarket rate limiting. Reuse existing shared rate limiter for REST polling.
- **backoff 0.4** -- already used in all 4 supervisors for reconnection. No changes.
- **rust_decimal 1.40** -- already used for all price/probability arithmetic. No changes.

The work is entirely **refactoring existing Rust code** to:
1. Diagnose and fix Polymarket WebSocket connectivity from EC2
2. Add REST-based book polling as a fallback data source
3. Generalize the spread engine to work with single prediction market venues (not just Polymarket+Kalshi pairs)
4. Generalize the signal engine to work with any single prediction market venue

---

## Recommended Stack

### Existing Technologies (No Version Changes)

| Technology | Version | Purpose | Status for v1.7 |
|------------|---------|---------|-----------------|
| tokio-tungstenite | 0.28 | Polymarket WebSocket client | Keep as-is; fix connectivity logic, not library |
| reqwest | 0.12 | REST API calls (Gamma, Kalshi, Derive, now Polymarket CLOB) | Reuse for REST book polling fallback |
| governor | 0.8 | Rate limiting | Reuse existing Polymarket rate limiter (CLOB: 9000 req/10s general, 500-1500 req/10s market data) |
| serde + serde_json | 1.0 | JSON serialization | Reuse for REST book response parsing |
| backoff | 0.4 | Exponential backoff reconnection | Reuse in supervisor; extend for REST fallback retry |
| tokio | 1.x | Async runtime | No changes |
| rust_decimal | 1.40 | Decimal arithmetic | No changes |
| tracing | 0.1 | Structured logging | No changes |
| metrics | 0.24 | Prometheus metrics | Add new metric labels for REST vs WS data source |

### No New Libraries Needed

The REST fallback pattern for Polymarket follows the exact same architecture as the existing Derive feed (snapshot-only via REST) and Kalshi settlement checker (periodic REST polling). Both use `reqwest` with `governor` rate limiting and `backoff` retry logic.

---

## Polymarket API Stack Details

### WebSocket Channel (Primary -- Existing)

| Attribute | Value | Source |
|-----------|-------|--------|
| URL | `wss://ws-subscriptions-clob.polymarket.com/ws/market` | Official docs |
| Auth | None (public market channel) | Official docs |
| Subscription | `{"assets_ids": [...], "type": "market"}` | Official docs |
| Heartbeat | PING every 10s | Official docs; already implemented |
| Known Issue | Silent freeze -- accepts connections but sends no data for hours | GitHub issue #292 (2026-03-05) |

**Connectivity Problem Analysis:**

The Polymarket WebSocket has a known server-side issue (reported 2026-03-05, GitHub `py-clob-client` #292) where the server enters a state where:
- TCP connection succeeds
- Subscription message is accepted
- Application-level PING/PONG works
- **Zero book or price_change events are delivered** for extended periods (hours)

This is NOT a client-side bug. It is a server-side silent freeze. The current codebase (`PolymarketClient`) has no data inactivity watchdog -- it only breaks on connection errors or stream end, not on data silence.

**Required Fix (code only, no new deps):**
- Add a `data_inactivity_timeout` to `PolymarketConfig` (e.g., 120s)
- In the supervisor's forwarding loop, track last data message time
- Force reconnect when no book data received within timeout
- This is the same pattern already used in `KalshiSupervisor` heartbeat timeout

### REST API (Fallback -- New Usage of Existing reqwest)

| Attribute | Value | Source |
|-----------|-------|--------|
| Base URL | `https://clob.polymarket.com` | Official docs |
| Book Endpoint | `GET /book?token_id={token_id}` | Official docs (public methods) |
| Midpoint Endpoint | `GET /midpoint?token_id={token_id}` | Official docs |
| Price Endpoint | `GET /price?token_id={token_id}&side={BUY\|SELL}` | Official docs |
| Rate Limit | 9000 req/10s general; 500-1500 req/10s market data | Official docs |
| Auth | None for public endpoints | Official docs |

**REST Polling Architecture:**
- Poll `GET /book` every N seconds (configurable, e.g., 5-10s)
- Parse response into existing `MarketSnapshot` via a new normalizer (same pattern as Derive snapshot-only model)
- Use `reqwest` client (already in deps) with shared `governor` rate limiter (already exists for Polymarket)
- Fallback activates when WebSocket data inactivity timeout fires

**REST Book Response Schema (from official docs):**
```json
{
  "market": "condition_id",
  "asset_id": "token_id",
  "timestamp": "1234567890",
  "hash": "...",
  "bids": [{"price": "0.50", "size": "100"}],
  "asks": [{"price": "0.52", "size": "200"}],
  "min_tick_size": "0.01"
}
```

### Cloudflare Considerations

Polymarket routes through Cloudflare. Known issues for datacenter IPs:
- REST POST endpoints (trading) are sometimes blocked with 403
- REST GET endpoints (public data) are less affected
- WebSocket connections from datacenter IPs generally work but may be silenced

The EC2 instance uses a public IP in AWS datacenter space. The WebSocket silent freeze may be Cloudflare-related throttling for datacenter IPs. REST GET polling is a more resilient fallback because each request is independent (no persistent connection to silently fail).

---

## Spread Engine Generalization

### Current Hardcoding (What Changes)

The `SpreadEngine` is hardcoded to Polymarket+Kalshi pairs:

1. **`process_snapshot()`** line 228: `if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() { return; }` -- requires BOTH venues
2. **`SpreadPattern` enum**: All 4 variants are `BuyPoly*SellKalshi*` or vice versa
3. **`walk_both_sides()`**: Hardcoded to `poly` and `kalshi` parameters
4. **`compute_fees()`**: Only handles Polymarket and Kalshi fee models
5. **`SpreadResult`**: Fields named `poly_exchange_ts` and `kalshi_exchange_ts`

### Generalization Approach (Code Changes, No New Deps)

The spread engine currently serves the **prediction market vs prediction market** use case (Polymarket vs Kalshi). The signal engine (`CrossAssetEngine`) already handles the **prediction market vs options-implied probability** use case and already works with any single prediction market venue.

**Key insight:** The spread engine does NOT need to be generalized for v1.7. The `CrossAssetEngine` (signal engine) is the correct engine for the "single prediction market vs options" comparison, and it already:
- Accepts any `Venue::Polymarket` or `Venue::Kalshi` snapshot
- Pairs with options-implied probabilities from Deribit/Derive
- Computes venue-appropriate fees via match on `pred_venue`
- Supports per-venue staleness thresholds

**What needs fixing in `CrossAssetEngine`:**
1. The `options_leg.venue` is hardcoded to `Venue::Deribit` (line 548) -- should use the actual venue from the `ImpliedProbability` source
2. The `handle_probability()` method hardcodes `Venue::Deribit` for lookup (line 252) -- should accept any options venue

**What needs fixing in `SpreadEngine`:**
1. The guard on line 228 should be relaxed to work when only ONE prediction market venue is available (not require both)
2. When only one prediction market is available, skip the prediction-market-vs-prediction-market spread computation (there's no second leg)
3. Alternatively, the SpreadEngine can remain Polymarket+Kalshi-specific and simply not block startup when one venue has no data yet

**Recommended approach:** Make the `SpreadEngine` guard permissive (skip gracefully when a venue pair isn't available rather than hard-requiring both), and fix the two `CrossAssetEngine` hardcodings. This minimizes code changes and preserves existing test coverage.

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `polymarket-rs-clob-client` crate | Adds large dependency tree (alloy, k256, auth); we only need public GET endpoints | Use existing `reqwest` directly -- 3 endpoints, trivial to call |
| `tungstenite` feature flags changes | The WebSocket library is not the problem; the server silently freezes | Add data inactivity watchdog in supervisor logic |
| Any new async runtime features | tokio 1.x already has everything needed | -- |
| WebSocket compression (permessage-deflate) | Polymarket doesn't advertise it; would add complexity | -- |
| Connection pooling crate | Single REST endpoint, single token per request | reqwest's built-in connection pool is sufficient |
| Database for caching REST responses | REST responses are consumed immediately as MarketSnapshot | Existing in-memory `latest` HashMap in engine is correct |

---

## Integration Points

### How REST Fallback Integrates with Existing Architecture

```
Existing:
  PolymarketSupervisor -> PolymarketClient (WS) -> RawMessage -> PolymarketProcessor -> MarketSnapshot

With REST fallback:
  PolymarketSupervisor -> PolymarketClient (WS) -> RawMessage -> PolymarketProcessor -> MarketSnapshot
                       \-> PolymarketRestPoller  -> RawMessage -/
                       (activates on WS data silence)
```

The REST poller produces `RawMessage` structs identical to WS messages, feeding into the same `PolymarketProcessor` normalizer pipeline. This is the same pattern used for Derive (snapshot-only feed).

Alternatively, the REST poller can produce `MarketSnapshot` directly (bypassing the RawMessage layer), since REST responses are already structured JSON books rather than streaming deltas. This is simpler and avoids synthesizing fake "raw" messages.

### How Engine Generalization Integrates

No new channels or data flow changes. The `CrossAssetEngine` already receives:
- `ImpliedProbability` from `PricingEngine` (Deribit + Derive options data)
- `MarketSnapshot` from prediction market feeds (Polymarket + Kalshi)

It already pairs them by event ID and computes spreads. The only changes are:
- Remove `Venue::Deribit` hardcoding in options leg info
- Make the `SpreadEngine` guard permissive for single-venue events

---

## Existing Dependency Versions (Verified from Cargo.toml)

| Crate | Version | Relevant Feature Flags |
|-------|---------|----------------------|
| tokio | 1.x | full |
| tokio-tungstenite | 0.28 | native-tls |
| reqwest | 0.12 | json, rustls-tls |
| serde | 1.0 | derive |
| serde_json | 1.0 | -- |
| governor | 0.8 | -- |
| backoff | 0.4 | tokio |
| rust_decimal | 1.40 | maths, serde-with-str |
| tracing | 0.1 | -- |
| metrics | 0.24 | -- |
| chrono | 0.4 | serde |

No version bumps needed. All crates are at versions compatible with the new functionality.

---

## Polymarket Rate Limit Budget for REST Polling

| Scenario | Tokens | Poll Interval | Req/10s | Within Limit? |
|----------|--------|---------------|---------|---------------|
| 1 token, 10s poll | 1 | 10s | 1 | Yes (500-1500 allowed) |
| 5 tokens, 5s poll | 5 | 5s | 10 | Yes |
| 10 tokens, 3s poll | 10 | 3s | ~33 | Yes |
| 50 tokens, 1s poll | 50 | 1s | 500 | Borderline |

For the current scale (single-digit token count), REST polling every 5-10 seconds is well within limits and shares the existing `governor` rate limiter already configured for Polymarket in the codebase.

---

## Sources

- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- WebSocket URLs, subscription format, heartbeat requirements (HIGH confidence)
- [Polymarket CLOB Public Methods](https://docs.polymarket.com/developers/CLOB/clients/methods-public) -- REST endpoints for book, price, midpoint (HIGH confidence)
- [Polymarket Rate Limits](https://docs.polymarket.com/quickstart/introduction/rate-limits) -- CLOB 9000 req/10s general, market data 500-1500 req/10s (HIGH confidence)
- [Polymarket CLOB Endpoints](https://docs.polymarket.com/quickstart/reference/endpoints) -- Base URL `https://clob.polymarket.com` (HIGH confidence)
- [GitHub Issue #292: CLOB WSS silent freeze](https://github.com/Polymarket/py-clob-client/issues/292) -- Server-side silent data freeze documented 2026-03-05 (HIGH confidence, first-hand bug report)
- [Cloudflare WAF blocking datacenter API requests](https://community.cloudflare.com/t/cloudflare-waf-blocking-legitimate-api-requests-from-supabase-edge-functions-to-pol/869437) -- Cloudflare blocks some datacenter-origin requests (MEDIUM confidence)
- [Polymarket rs-clob-client](https://github.com/Polymarket/rs-clob-client) -- Official Rust SDK v0.3; decided against due to heavy dependency tree (MEDIUM confidence)
- Existing codebase analysis: `src/feed/polymarket/client.rs`, `src/spread/engine.rs`, `src/signal/engine.rs` -- direct code inspection (HIGH confidence)

---
*Stack research for: v1.7 Prediction Market Signal Pipeline*
*Researched: 2026-03-09*
