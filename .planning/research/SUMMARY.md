# Project Research Summary

**Project:** v1.7 Prediction Market Signal Pipeline
**Domain:** Polymarket WebSocket connectivity fix, REST polling fallback, spread/signal engine generalization
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

v1.7 is a pure Rust code milestone with zero new crate dependencies and zero infrastructure changes. The 42,732 LOC prediction market arbitrage system (v1.0-v1.6 complete) needs three things to unlock cross-asset signal generation: (1) fix Polymarket WebSocket connectivity from AWS EC2, where a documented server-side silent freeze (GitHub #292) causes the feed to appear healthy while delivering zero data for hours; (2) add a REST polling fallback using existing `reqwest` + `governor` crates to ensure data continuity when WebSocket is unreliable; and (3) generalize the CrossAssetEngine to correctly attribute options-implied probabilities from both Deribit and Derive venues, fixing two hardcoded `Venue::Deribit` references. The SpreadEngine (prediction-vs-prediction) should be left mostly untouched -- the CrossAssetEngine (prediction-vs-options) is the correct engine for v1.7's goal.

The recommended approach is investigation-first for the WebSocket issue (diagnose from EC2 before writing code), followed by a small data model change (add `source_venue` to `ImpliedProbability`), then two-line fixes in the CrossAssetEngine, and finally REST fallback implementation as insurance against Polymarket's unreliable WebSocket. The critical architectural insight across all four research files is consistent: do NOT refactor SpreadEngine or SpreadPattern for venue generalization. SpreadEngine is correct for its purpose (Polymarket-vs-Kalshi) and should remain dormant while Kalshi is geo-blocked. CrossAssetEngine already has the right architecture for single-prediction-market-vs-options comparison and needs only minor hardcoding fixes.

The top risks are: (1) the Polymarket WebSocket silent freeze is server-side and may not be fixable -- REST fallback must be production-ready, not an afterthought; (2) the REST `/book` endpoint has documented staleness issues (GitHub #180) returning ghost market data -- use `/price` or `/midpoint` endpoints instead; and (3) running dual data sources (WS + REST) without exclusive-mode coordination creates price oscillation and spurious signals. All three are preventable with the patterns documented in PITFALLS.md.

## Key Findings

### Recommended Stack

Zero new Rust crate dependencies. Every capability is already in `Cargo.toml`. This is entirely a code refactoring and connectivity debugging milestone. See [STACK.md](STACK.md) for full details including Polymarket API endpoints, rate limits, and REST response schemas.

**Core technologies (all existing):**
- **tokio-tungstenite 0.28:** WebSocket client for Polymarket -- the library is not the problem; the server silently freezes
- **reqwest 0.12:** REST API calls -- reuse for `GET /price` and `GET /midpoint` polling fallback
- **governor 0.8:** Rate limiting -- reuse existing Polymarket rate limiter for REST polling (1,500 req/10s for market data endpoints)
- **backoff 0.4:** Exponential backoff -- extend for REST fallback retry logic
- **metrics 0.24:** Prometheus metrics -- add labels for WS vs REST data source mode

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape, dependency graph, and Polymarket API reference.

**Must have (table stakes):**
- Polymarket WS data-level liveness detection -- force reconnect on data silence, not just TCP drops
- REST-based price polling fallback -- `GET /price` or `/midpoint` at configurable interval when WS is down
- SpreadEngine gate relaxation -- skip gracefully when venue pair is incomplete instead of hard-requiring both
- CrossAssetEngine venue generalization -- remove hardcoded `Venue::Deribit` lookups (2 sites)
- SpreadResult struct generalization -- venue-agnostic timestamp fields
- End-to-end production signal verification on AWS EC2

**Should have (differentiators):**
- Automatic WS/REST mode switching with hysteresis (3 consecutive WS messages before switching back)
- Staleness-aware REST freshness marking in MarketSnapshot
- Prometheus gauges for current data source mode per venue
- Configurable spread venue pairs in TOML

**Defer (v2+):**
- Full Polymarket CLOB REST client library (only need 2-3 GET endpoints)
- Polymarket authentication for private channels
- Multi-venue spread engine supporting 3+ venues simultaneously
- New Grafana dashboards (existing dashboards likely sufficient)
- WebSocket connection via proxy/VPN infrastructure

### Architecture Approach

The existing fan-out architecture (4 venue feeds -> fan-in -> SpreadEngine + PricingEngine + CrossAssetEngine) requires no structural changes. The REST fallback integrates at the supervisor level, producing MarketSnapshot directly (bypassing RawMessage since REST responses are structured JSON). The key data model change is adding `source_venue: Venue` to `ImpliedProbability` so CrossAssetEngine can distinguish Deribit from Derive probabilities. SpreadEngine remains prediction-vs-prediction only; CrossAssetEngine handles prediction-vs-options. See [ARCHITECTURE.md](ARCHITECTURE.md) for component boundaries, data flow diagrams, and build order.

**Major components (changes only):**
1. **ImpliedProbability struct** -- add `source_venue: Venue` field (1 line, propagated from PricingEngine)
2. **CrossAssetEngine** -- replace 2 hardcoded `Venue::Deribit` references with `prob.source_venue`
3. **PolymarketSupervisor** -- add data inactivity watchdog timer; coordinate WS/REST source priority
4. **PolymarketRestPoller (new)** -- minimal REST poller using existing `reqwest` + `governor`, producing MarketSnapshot
5. **SpreadEngine** -- relax Poly+Kalshi gate to "2+ prediction markets" (no-op currently, forward-compatible)
6. **SignalGenerationConfig** -- rename `deribit_taker_fee_rate` to `options_taker_fee_rate`

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for all 8 pitfalls with recovery strategies and phase mapping.

1. **WebSocket silent freeze (Pitfall 1)** -- Connection alive, ping/pong works, zero data for hours. Add data inactivity watchdog that tracks `last_data_received_at` (not ping/pong). Force reconnect after configurable timeout (e.g., 120s).
2. **REST `/book` endpoint returns stale ghost data (Pitfall 4)** -- Use `/price` or `/midpoint` instead of `/book`. Cross-validate REST prices against last WS price within tolerance threshold.
3. **Dual WS+REST source creates price oscillation (Pitfall 8)** -- Implement exclusive-mode source priority: only one active at a time per venue. REST activates on WS silence, deactivates after N consecutive WS data messages.
4. **SpreadPattern refactoring breaks JSONL serialization (Pitfall 3)** -- Do NOT refactor SpreadPattern. Leave SpreadEngine unchanged. CrossAssetEngine is the correct target for v1.7 generalization.
5. **EC2 connection resets are network-level, not application-level (Pitfall 2)** -- Investigate with `websocat` from EC2 before changing Rust code. Enable TCP keepalive. REST fallback may become primary path.

## Implications for Roadmap

Based on combined research, the build decomposes into 4 phases with clear dependency ordering. The first phase is investigative (may change subsequent plans), the second is a safe data model change, the third is the core value delivery, and the fourth is production verification.

### Phase 1: Polymarket WebSocket Diagnosis and Data Watchdog
**Rationale:** Without Polymarket data flowing from EC2, nothing else in v1.7 matters. The WebSocket issue may be trivially fixable (headers, URL change) or fundamentally unfixable (Cloudflare datacenter IP blocking). The diagnosis outcome determines whether REST is a fallback or the primary data source.
**Delivers:** Root cause identified; data inactivity watchdog implemented in PolymarketSupervisor; decision documented on WS viability from EC2.
**Addresses:** Data-level liveness detection (table stakes), EC2 connectivity investigation
**Avoids:** Pitfall 1 (silent freeze -- watchdog prevents indefinite stale state), Pitfall 2 (connection reset -- systematic diagnosis before code changes)

### Phase 2: ImpliedProbability Source Venue and CrossAssetEngine Fix
**Rationale:** This is a pure additive data model change (add one field) plus two one-line fixes. Zero behavior change for existing tests. Unblocks correct Derive-sourced signal attribution. Can be done independently of WebSocket diagnosis.
**Delivers:** `source_venue` field on ImpliedProbability; CrossAssetEngine correctly pairs Derive probabilities with prediction market snapshots; `options_taker_fee_rate` config rename; SpreadEngine gate relaxed to "2+ prediction markets".
**Addresses:** CrossAssetEngine venue generalization (table stakes), SpreadEngine gate relaxation (table stakes), SpreadResult generalization (table stakes)
**Avoids:** Pitfall 3 (SpreadPattern left untouched), Pitfall 5 (venue filter change paired with registry validation), Pitfall 6 (spread engine stays dormant, signal engine is the correct target)

### Phase 3: REST Polling Fallback and Source Coordination
**Rationale:** Depends on Phase 1 diagnosis to determine whether REST is fallback or primary. Depends on Phase 2 data model being in place so REST-sourced snapshots flow through the generalized engine correctly. This is the most complex phase -- new code path, rate limiting, source coordination.
**Delivers:** PolymarketRestPoller producing MarketSnapshot from `/price` endpoint; exclusive-mode WS/REST coordination in supervisor; Prometheus metrics for data source mode; configurable poll interval.
**Addresses:** REST-based price polling (table stakes), automatic WS/REST mode switching (differentiator), data source metrics (differentiator)
**Avoids:** Pitfall 4 (stale `/book` data -- uses `/price` instead), Pitfall 7 (aggressive polling -- starts at 30s interval), Pitfall 8 (dual source conflicts -- exclusive mode prevents oscillation)

### Phase 4: End-to-End Production Verification
**Rationale:** All code changes complete. This phase proves the system works on AWS EC2 with real Polymarket data flowing through the generalized engines to produce ArbSignals.
**Delivers:** Verified signal flow on EC2; Prometheus metrics confirming data source, signal emission, and paper trade recording; JSONL logs with correct venue attribution; documented test results.
**Addresses:** End-to-end production signal verification (table stakes), Grafana dashboard verification (differentiator)
**Avoids:** "Looks done but isn't" checklist from PITFALLS.md -- systematic verification of all 7 items.

### Phase Ordering Rationale

- Phase 1 first because it is investigative and may redirect the approach for Phase 3 (REST as fallback vs primary)
- Phase 2 second because it is low-risk, no behavior change, and unblocks correct signal attribution regardless of WS outcome
- Phase 3 third because it is the most complex new code and depends on both Phase 1 (diagnosis) and Phase 2 (data model)
- Phase 4 last because it is integration verification that depends on all prior phases being complete
- Phases 1 and 2 can be parallelized since they have no code dependencies on each other

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1 (WS Diagnosis):** Investigative phase by nature. The root cause is unknown until EC2 testing is done. May require researching Cloudflare bypass techniques, TCP keepalive configuration for tokio-tungstenite, or Polymarket API URL changes.
- **Phase 3 (REST Fallback):** Source coordination (exclusive-mode switching with hysteresis) is a non-trivial state machine. Validate Polymarket `/price` endpoint reliability and batch `/prices` endpoint availability during phase planning.

Phases with standard patterns (skip research-phase):
- **Phase 2 (Engine Generalization):** Two-line fix plus one struct field addition. The exact code locations and changes are already identified in ARCHITECTURE.md. No ambiguity.
- **Phase 4 (Production Verification):** Standard deployment and metric verification. The metrics and log formats are already documented. Verification criteria are enumerated in PITFALLS.md "Looks Done But Isn't" checklist.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new dependencies. All crate versions verified from Cargo.toml. Polymarket API endpoints verified from official docs. |
| Features | HIGH | Feature list derived from direct codebase analysis of hardcoded lines. All code locations identified with line numbers. |
| Architecture | HIGH | Based on direct source code inspection of all affected files. Data flow traced through fan-out, engines, and signal output. |
| Pitfalls | HIGH | 8 pitfalls identified from Polymarket GitHub issues (first-hand bug reports), official docs, and codebase analysis. REST `/book` staleness confirmed by GitHub #180. |

**Overall confidence:** HIGH

### Gaps to Address

- **Polymarket WebSocket root cause from EC2:** Unknown until Phase 1 diagnosis. Could be Cloudflare, could be server-side, could be trivial. This is the single largest uncertainty in the milestone.
- **REST `/prices` (plural) batch endpoint availability:** PITFALLS.md recommends batching. FEATURES.md and STACK.md document single-token `/price` endpoint but the batch variant needs verification during Phase 3 planning. If unavailable, per-token polling at 30s interval is within rate limits for single-digit token counts.
- **TOML config migration for `options_taker_fee_rate` rename:** The `deribit_taker_fee_rate` -> `options_taker_fee_rate` rename requires updating the deployed config file. This is a trivial change but must be coordinated with deployment in Phase 4. Verify the existing config file path and deployment process.
- **`Venue::is_prediction_market()` method placement:** PITFALLS.md recommends this as the venue filter replacement. Decide during Phase 2 whether this is a method on the Venue enum or a config-driven check. The enum method is simpler and preferred at 4 venues.

## Sources

### Primary (HIGH confidence)
- [Polymarket WSS Overview](https://docs.polymarket.com/developers/CLOB/websocket/wss-overview) -- WebSocket URLs, subscription format, heartbeat
- [Polymarket CLOB Public Methods](https://docs.polymarket.com/developers/CLOB/clients/methods-public) -- REST endpoints for book, price, midpoint
- [Polymarket Rate Limits](https://docs.polymarket.com/quickstart/introduction/rate-limits) -- CLOB rate limits per endpoint
- [GitHub #292: CLOB WSS Silent Freeze](https://github.com/Polymarket/py-clob-client/issues/292) -- Server-side silent data freeze (2026-03-05)
- [GitHub #180: REST /book Stale Data](https://github.com/Polymarket/py-clob-client/issues/180) -- Order book endpoint returns ghost market data
- [GitHub #26: WebSocket Stream Stops](https://github.com/Polymarket/real-time-data-client/issues/26) -- Data stream stops after ~20 minutes
- Direct codebase analysis: `src/spread/engine.rs`, `src/signal/engine.rs`, `src/pricing/engine.rs`, `src/feed/polymarket/client.rs`, `src/feed/polymarket/supervisor.rs`

### Secondary (MEDIUM confidence)
- [Cloudflare WAF Blocking](https://community.cloudflare.com/t/cloudflare-waf-blocking-legitimate-api-requests-from-supabase-edge-functions-to-pol/869437) -- Datacenter IP blocking for Polymarket
- [Polymarket rs-clob-client](https://github.com/Polymarket/rs-clob-client) -- Official Rust SDK v0.3 (decided against due to heavy dependency tree)
- [tokio-tungstenite Connection Reset #296](https://github.com/snapview/tokio-tungstenite/issues/296) -- Protocol-level reset without closing handshake

### Tertiary (LOW confidence)
- None. All findings verified against at least two sources or direct code inspection.

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*
