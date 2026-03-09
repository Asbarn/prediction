# Feature Landscape

**Domain:** Prediction market signal pipeline -- Polymarket connectivity fix, spread engine generalization, cross-asset signal generation
**Researched:** 2026-03-09
**Confidence:** HIGH (codebase fully examined; Polymarket API docs verified; known connectivity issues documented)

**Scope note:** This research covers ONLY v1.7 features: fixing Polymarket WebSocket connectivity from AWS, generalizing the SpreadEngine beyond Polymarket+Kalshi hardcoding, generalizing CrossAssetEngine for any single prediction market venue, and end-to-end production verification. The system is fully deployed at 42,732 LOC Rust with 4-venue feeds, both engines, paper trading, and production infrastructure.

**Existing code this builds on:**

| Asset | Location | Status | How v1.7 Changes It |
|-------|----------|--------|---------------------|
| PolymarketClient | `src/feed/polymarket/client.rs` | Connects to `wss://ws-subscriptions-clob.polymarket.com/ws/market` | May need REST fallback for silent-freeze issue |
| PolymarketSupervisor | `src/feed/polymarket/supervisor.rs` | Exponential backoff reconnection with watch channel | Needs data-level liveness detection (not just TCP) |
| SpreadEngine | `src/spread/engine.rs` | Hardcoded: requires BOTH Polymarket AND Kalshi (line 228) | Must generalize to work with single prediction market venue |
| SpreadPattern enum | `src/spread/patterns.rs` | 4 patterns all named `BuyPoly*SellKalshi*` | Must support venue-generic patterns |
| compute_gross_spread | `src/spread/patterns.rs` | Takes `poly: &MarketSnapshot, kalshi: &MarketSnapshot` | Must accept generic venue pair |
| CrossAssetEngine | `src/signal/engine.rs` | Hardcoded loop: `for venue in [Venue::Polymarket, Venue::Kalshi]` (line 273) | Must iterate over all available prediction venues |
| handle_prediction_snapshot | `src/signal/engine.rs` | Filter: `snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi` (line 292) | Must accept any prediction market venue |
| Pipeline fan-out | `src/main.rs` (line 363+) | 3-way fan-out to SpreadEngine + PricingEngine + CrossAssetEngine | No structural change needed |
| PolymarketProcessor | `src/feed/polymarket/normalize.rs` | Parses book + price_change events | No change needed |
| VenueHealth | `src/feed/health.rs` | Tracks feed availability per venue | Used by REST fallback liveness detection |
| SpreadResult struct | `src/spread/patterns.rs` | Has `poly_exchange_ts` and `kalshi_exchange_ts` fields | Must generalize to venue-agnostic timestamps |

---

## Table Stakes

Features users expect. Missing = the v1.7 milestone goals are not met.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| Polymarket WS data-level liveness detection | Current supervisor only detects TCP drops. Known Polymarket issue: connection stays OPEN, PINGs succeed, but zero book/price_change events delivered for hours. Supervisor must detect this "silent freeze" and force reconnect. | Low-Med | Existing PolymarketSupervisor | Add timer: if no book/price_change received within N seconds (e.g., 60s), force reconnect. Reset timer on each data message. Distinct from PING which continues working during freeze. |
| REST-based price polling fallback | When WS is in silent-freeze state (or fails entirely), system needs price data to continue generating signals. Polymarket CLOB REST API provides `GET /book` for full order book and `GET /price` for top-of-book. | Medium | Polymarket CLOB REST endpoint `https://clob.polymarket.com`, rate limiter | Poll `GET /book` per token at configurable interval (e.g., 5-10s). REST rate limit is 1,500 req/10s for book/price endpoints. Normalize REST response to same MarketSnapshot as WS. |
| SpreadEngine venue generalization | Engine hardcodes `mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none()` gate (line 228). Only pairs Polymarket with Kalshi. Must work with ANY single prediction market venue vs options-implied probability (handled by CrossAssetEngine), or any two prediction market venues. | Medium | SpreadPattern refactor | Remove Polymarket+Kalshi requirement. Accept any two venues with bid/ask probabilities. SpreadPattern becomes venue-parameterized. |
| CrossAssetEngine venue generalization | Engine hardcodes `for venue in [Venue::Polymarket, Venue::Kalshi]` and filters `snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi`. Must accept any prediction market venue. | Low | Venue enum already has all venues | Replace hardcoded venue list with config-driven or dynamic detection. A venue is a "prediction market" if it has bid/ask probabilities. |
| SpreadResult struct generalization | Contains `poly_exchange_ts` and `kalshi_exchange_ts` fields. Must support arbitrary venue pairs. | Low | SpreadPattern refactor | Replace with `buy_venue_exchange_ts` / `sell_venue_exchange_ts` or generic `venue_timestamps: HashMap<Venue, Option<i64>>` |
| SpreadPattern venue-generic naming | Enum variants `BuyPolyYesSellKalshiYes` etc. are venue-specific. Need venue-agnostic pattern representation. | Low-Med | None | Options: (1) parameterize pattern with buy_venue/sell_venue, (2) use directional enum (BuyVenueAYesSellVenueBYes) with venue fields. Pattern 1 is cleaner. |
| End-to-end production signal verification | Prove: Polymarket data arrives -> PricingEngine produces ImpliedProbability -> CrossAssetEngine pairs them -> ArbSignal emitted -> Paper trade logged. Must work on AWS EC2. | Low | All above features complete | Verify via Prometheus metrics, JSONL logs, and Grafana dashboards. Structured tracing makes this observable. |

---

## Differentiators

Features that improve quality beyond the minimum v1.7 scope. Not required but high value.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| Automatic WS/REST mode switching | Instead of manual config toggle, supervisor auto-switches to REST polling when WS silent-freeze is detected, and back to WS when reconnection succeeds. Seamless data continuity. | Medium | Both WS and REST paths implemented | State machine: WS_ACTIVE -> REST_FALLBACK (on silence timeout) -> WS_ACTIVE (on successful reconnect). Log mode transitions. |
| REST polling interval tuning via config | Different markets have different liquidity. Active BTC markets may need 5s polling; dormant markets could use 30s. Per-asset or per-category poll intervals. | Low | REST fallback implemented | Add `rest_poll_interval_ms` to PolymarketConfig. Could be per-asset in config but probably not worth it initially. |
| Staleness-aware REST freshness marking | REST-polled data is inherently older than WS-pushed data. Mark REST-sourced snapshots with appropriate staleness metadata so downstream engines can adjust thresholds. | Low | REST fallback implemented | Set `exchange_timestamp` from REST response. CrossAssetEngine already has per-venue staleness thresholds. |
| Spread engine configurable venue pairs | Allow config to specify which venue pairs to compute spreads for, rather than computing all possible pairs. Reduces noise. | Low | Venue generalization | TOML config: `spread_venue_pairs = [["polymarket", "kalshi"], ["polymarket", "deribit"]]` |
| Metrics for WS vs REST mode | Prometheus gauge showing current data source mode per venue (WS=1, REST=0). Counter for mode switches. Enables Grafana alerting on degraded mode. | Low | Mode switching implemented | `feed_data_source_mode{venue="polymarket"}` gauge, `feed_mode_switches_total{venue="polymarket"}` counter |

---

## Anti-Features

Features to explicitly NOT build for v1.7.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Full Polymarket CLOB REST client library | Only need `GET /book` and maybe `GET /price`. Building a comprehensive REST client with auth, order placement, etc. is v2 scope. | Minimal REST fetch: single async function that GETs book JSON, parses to MarketSnapshot. |
| WebSocket reconnection via proxy/VPN | Cloudflare WAF blocking is documented for REST trading endpoints from cloud IPs, but WS market channel is public and not geo-blocked. The silent-freeze issue is server-side, not client-side. Proxy adds latency and complexity. | REST fallback handles data gaps. WS reconnection supervisor already has exponential backoff. |
| Polymarket authentication for private channels | v1.7 is read-only market data. Private channels require Polygon wallet signing. Not needed until v2 execution. | Public market channel only (already implemented). |
| Multi-venue spread engine supporting 3+ venues simultaneously | Two-venue pairwise comparison is the correct abstraction. A 3-venue spread engine adds combinatorial complexity for marginal benefit. | Pairwise: compute spreads for each venue pair independently. N venues = N*(N-1)/2 pairs. |
| Dynamic venue-type detection | Trying to auto-detect whether a venue is "prediction market" or "options market" based on data shape. Over-engineering. | Config-driven: `venue_type = "prediction"` or `venue_type = "options"` in TOML. Already implicit in EventRegistry venues structure. |
| Polymarket Gamma API integration changes | Gamma API for discovery already works (v1.2). No changes needed for v1.7. | Existing discovery pipeline continues unchanged. |
| New Grafana dashboards for v1.7 | Existing dashboards already show signal quality, feed health, and paper trade P&L. New cross-asset metrics (`arb_*`) are already emitted and visualized. | Verify existing dashboards show the new data flow. Add panels only if gaps found. |

---

## Feature Dependencies

```
Polymarket Data-Level Liveness Detection
  |
  +-> REST Fallback Implementation
  |     |
  |     +-> REST response normalization to MarketSnapshot
  |     +-> Rate limiter integration (reuse existing VenueRateLimiter)
  |     +-> Configurable poll interval
  |
  +-> Automatic WS/REST Mode Switching (differentiator, optional)

SpreadEngine Venue Generalization
  |
  +-> SpreadPattern refactoring (venue-parameterized)
  |     |
  |     +-> compute_gross_spread accepts generic venue pair
  |     +-> SpreadResult struct generalization (venue-agnostic timestamps)
  |
  +-> SpreadEngine.process_snapshot generalization
        |
        +-> Remove Polymarket+Kalshi gate
        +-> Dynamic venue pairing from EventRegistry

CrossAssetEngine Venue Generalization (independent of SpreadEngine changes)
  |
  +-> Remove hardcoded [Polymarket, Kalshi] venue list
  +-> Remove venue filter in handle_prediction_snapshot
  +-> Accept any venue that provides bid/ask probabilities

End-to-End Production Verification (depends on ALL above)
  |
  +-> Polymarket data flowing on AWS EC2
  +-> CrossAssetEngine producing ArbSignals
  +-> Paper trade tracker recording signals
  +-> Prometheus metrics confirming data flow
```

---

## MVP Recommendation

### Phase 1: Polymarket Connectivity Fix

Prioritize:
1. **Data-level liveness detection** in PolymarketSupervisor -- add silence timeout timer; force reconnect when no book/price_change events received within configurable window (e.g., 60s)
2. **REST polling fallback** -- minimal `GET /book` polling at configurable interval; normalize to MarketSnapshot; activate when WS is unavailable
3. **WS/REST mode metrics** -- gauge and counter for observability

Rationale: Without data flowing, nothing else matters. The silent-freeze issue is a known Polymarket server-side problem. REST fallback ensures data continuity regardless of WS health.

### Phase 2: Engine Generalization

Prioritize:
1. **SpreadPattern refactor** -- venue-parameterized patterns replacing hardcoded Poly/Kalshi names
2. **SpreadEngine generalization** -- remove Polymarket+Kalshi gate; accept any two prediction market venues
3. **CrossAssetEngine generalization** -- remove hardcoded venue list; accept any prediction market venue
4. **SpreadResult struct update** -- venue-agnostic timestamp fields

Rationale: Both engines have the same class of hardcoding. Refactoring SpreadPattern first unblocks SpreadEngine changes. CrossAssetEngine is independent and can be done in parallel.

### Phase 3: Production Verification

Prioritize:
1. **End-to-end signal flow verification** on AWS EC2
2. **Prometheus metrics confirmation** -- `arb_signals_emitted_total`, `arb_computations_total`, `feed_available{venue=polymarket}`
3. **JSONL log inspection** -- verify ArbSignal records in signal logs
4. **Grafana dashboard verification** -- existing dashboards show cross-asset signal data

Rationale: This is the proof that v1.7 works. No new code, just systematic verification with test criteria.

### Defer:
- Automatic WS/REST mode switching (Phase 1 can use config-driven mode selection initially)
- Per-asset REST poll intervals (uniform interval sufficient for BTC-only)
- New Grafana panels (existing dashboards likely sufficient)

---

## Polymarket API Reference (for Implementation)

### WebSocket Market Channel
- **Endpoint:** `wss://ws-subscriptions-clob.polymarket.com/ws/market`
- **Auth:** None required (public channel)
- **Subscribe:** `{"assets_ids": ["token1", "token2"], "type": "market"}`
- **Messages:** `book` (full snapshot), `price_change` (incremental), `tick_size_change`, `last_trade_price`
- **Enhanced messages:** Set `custom_feature_enabled: true` for `best_bid_ask`, `new_market`, `market_resolved`
- **Heartbeat:** Client should send WebSocket PING every 10s (already implemented)
- **Known issue:** Silent freeze -- connection stays open, PINGs succeed, no data messages for hours

### REST CLOB API (Fallback)
- **Base URL:** `https://clob.polymarket.com`
- **Book endpoint:** `GET /book?token_id={token_id}` -- returns bids, asks, min_order_size, tick_size
- **Price endpoint:** `GET /price?token_id={token_id}&side={BUY|SELL}` -- returns single price string
- **Midpoint endpoint:** `GET /midpoint?token_id={token_id}` -- returns mid price
- **Rate limits:** 1,500 req/10s for book/price/midpoint endpoints (150 req/s effective)
- **Enforcement:** Cloudflare throttling (delayed, not rejected)
- **Cloud connectivity:** REST endpoints may see intermittent Cloudflare WAF blocks from datacenter IPs for trading (POST) endpoints, but GET endpoints for market data are generally accessible

### Rate Limit Summary
| API | Endpoint | Limit | Window |
|-----|----------|-------|--------|
| CLOB | General | 9,000 | 10s |
| CLOB | /book, /price, /midpoint | 1,500 | 10s |
| CLOB | Trading POST | 3,500 | 10s (burst) |
| Gamma | General | 4,000 | 10s |
| Gamma | /markets | 300 | 10s |

---

## Sources

- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- WebSocket channel documentation
- [Polymarket Market Channel](https://docs.polymarket.com/developers/CLOB/websocket/market-channel) -- Message types and subscription format
- [Polymarket Public Methods](https://docs.polymarket.com/developers/CLOB/clients/methods-public) -- REST API endpoints for book/price/midpoint
- [Polymarket API Rate Limits](https://docs.polymarket.com/quickstart/introduction/rate-limits) -- Per-endpoint rate limits and Cloudflare enforcement
- [Polymarket API Endpoints](https://docs.polymarket.com/quickstart/reference/endpoints) -- Base URLs for CLOB, Gamma, Data APIs
- [CLOB WSS Silent Freeze Issue #292](https://github.com/Polymarket/py-clob-client/issues/292) -- Documented silent-freeze bug (March 2026)
- [Cloudflare WAF Blocking Issue #143](https://github.com/Polymarket/py-clob-client/issues/143) -- Cloud IP blocking for trading endpoints
- [Cloudflare WAF Blocking Discussion](https://community.cloudflare.com/t/cloudflare-waf-blocking-legitimate-api-requests-from-supabase-edge-functions-to-pol/869437) -- Server-to-server blocking details
- [Polymarket Server Infrastructure](https://www.quantvps.com/blog/polymarket-servers-location) -- AWS eu-west-2 hosting, latency considerations

---
*Feature research for: v1.7 Prediction Market Signal Pipeline*
*Researched: 2026-03-09*
