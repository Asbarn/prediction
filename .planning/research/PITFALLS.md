# Pitfalls Research

**Domain:** Prediction market WebSocket connectivity, REST polling fallback, and spread/signal engine generalization for Rust arbitrage system
**Researched:** 2026-03-09
**Confidence:** HIGH (codebase analysis, Polymarket GitHub issues), MEDIUM (Polymarket API behavior from community reports), HIGH (Rust refactoring patterns from codebase)

## Critical Pitfalls

### Pitfall 1: Polymarket WebSocket Silently Freezes -- Connection Alive but No Data

**What goes wrong:**
The Polymarket CLOB WebSocket at `wss://ws-subscriptions-clob.polymarket.com/ws/market` accepts connections and maintains healthy ping/pong heartbeats, but stops delivering `book` and `price_change` events after 15-30 minutes. The connection appears healthy to the supervisor (ping/pong succeeds, no errors, no close frames), so the exponential backoff reconnection never triggers. The system sits idle, believing it has a live feed, while Polymarket data goes stale. Staleness detection eventually flags the data, but no reconnection occurs because the WebSocket connection itself is still alive.

**Why it happens:**
This is a documented server-side issue on Polymarket's infrastructure (GitHub issues #292 on py-clob-client, #26 on real-time-data-client, open as of March 2026). The server enters a state where it silently stops publishing to certain connections. Contributing factors may include: too many subscribed tokens per connection, server-side load shedding, or backend state corruption. The current `PolymarketSupervisor` only reconnects when the stream returns `None` (connection closed) or a read error -- it has no concept of "connection alive but data stopped."

**How to avoid:**
Add a data inactivity watchdog to `PolymarketSupervisor`:
- Track `last_data_received_at` timestamp (not ping/pong, only actual market data messages)
- If no data message arrives within a configurable threshold (e.g., 120 seconds), force-close the WebSocket and trigger reconnection
- This is distinct from the existing staleness detection in the spread engine, which operates per-instrument; the watchdog operates at the connection level
- The watchdog must reset on every data message, not on ping/pong
- Log the watchdog trigger distinctly from connection-lost events so the two failure modes can be distinguished in Grafana

**Warning signs:**
- `VenueHealth` shows Polymarket as "available" but `staleness_threshold_exceeded` counters climbing
- Spread engine stops producing Polymarket-related spread results despite Polymarket being "connected"
- Supervisor log shows zero reconnections over hours while no data arrives
- Prometheus `polymarket_messages_received_total` counter flatlines while connection state shows healthy

**Phase to address:**
WebSocket connectivity fix phase (first). This is the primary blocker for getting Polymarket data flowing in production.

---

### Pitfall 2: "Connection Reset by Peer" from EC2 Is Not a Code Bug -- It Is a Network/Infrastructure Issue

**What goes wrong:**
The Polymarket WebSocket connection fails with "Connection reset by peer" specifically from AWS EC2 in us-east-1, but works fine from local development machines. Developers spend hours debugging the Rust WebSocket client code, adding headers, changing TLS settings, or modifying the connection handshake -- none of which fixes the issue because the problem is at the network layer, not the application layer.

**Why it happens:**
Several possible causes, all infrastructure-related:
1. **AWS NAT/security group TCP idle timeout:** AWS NAT gateways and VPC endpoints have idle timeouts (350s for NAT, configurable for others). If no TCP traffic flows for the timeout period (ping/pong may not count as TCP-level keepalive), the connection is silently dropped and the next packet gets RST.
2. **Polymarket IP-based rate limiting or geo-blocking:** Cloud provider IP ranges (AWS, GCP, Hetzner) may be throttled differently than residential IPs. Polymarket is built on Polygon and may have CDN/WAF rules that treat datacenter IPs with suspicion.
3. **Missing TCP keepalive at the socket level:** The current `tokio_tungstenite::connect_async` uses default socket options. WebSocket-level PING/PONG is application-layer; TCP keepalive is OS-layer. Some intermediary firewalls only respect TCP keepalive, not WebSocket ping.
4. **TLS renegotiation failures:** Long-lived TLS connections may fail renegotiation through certain AWS network paths.

**How to avoid:**
Investigate systematically before changing application code:
1. Test from EC2 with `websocat` or a minimal Rust test binary to isolate whether the issue is application-specific or network-level
2. Enable TCP keepalive on the socket: configure `tokio_tungstenite` with a custom `TcpStream` that has `set_keepalive(true)` and `set_keepalive_interval(60s)`
3. Check VPC security group outbound rules -- ensure outbound TCP 443 is unrestricted (it should be by default, but verify)
4. Try connecting through the EC2 instance's public IP vs. through a NAT gateway -- different network paths may behave differently
5. If the problem persists, the REST polling fallback (Pitfall 4) becomes the primary path, not a fallback

**Warning signs:**
- Error message specifically contains "Connection reset by peer" or "connection was forcibly closed"
- Connection succeeds from local machine but fails from EC2
- Connection succeeds on first attempt but fails after idle period
- Adding User-Agent or other headers has no effect

**Phase to address:**
WebSocket connectivity fix phase (first). Must be diagnosed before deciding whether REST polling is a fallback or the primary data source.

---

### Pitfall 3: SpreadPattern Enum Is Structurally Hardcoded to Polymarket+Kalshi -- Cannot Be Parameterized Without Breaking Serialization

**What goes wrong:**
The `SpreadPattern` enum has 4 variants literally named `BuyPolyYesSellKalshiYes`, `SellPolyYesBuyKalshiYes`, etc. These names are serialized to JSONL spread logs, used in Prometheus metric labels, embedded in pattern-matching throughout `spread/engine.rs`, and referenced in 40+ test assertions in `spread/patterns.rs`. Simply adding new variants for "Polymarket vs Deribit" or "Polymarket vs Derive" would create a combinatorial explosion (4 patterns x N venue pairs), and renaming existing variants breaks deserialization of historical JSONL logs.

**Why it happens:**
The spread engine was designed for a single use case: comparing two prediction markets against each other. The v1.7 goal is fundamentally different: comparing one prediction market against options-implied probability. The `SpreadPattern` encodes venue identity in enum variant names, making it impossible to generalize without either (a) accepting a breaking schema change to JSONL logs or (b) building a parallel system.

**How to avoid:**
Do NOT try to refactor `SpreadPattern` to be venue-generic. Instead:
1. The signal engine (`signal/engine.rs`) already has the correct architecture for v1.7 -- it compares prediction market price vs. options-implied probability, not two prediction markets against each other
2. Leave the existing `SpreadEngine` (prediction-vs-prediction) intact but dormant (it already returns early when the Polymarket+Kalshi venue pair is incomplete)
3. Generalize only the `SignalEngine` to accept any prediction market venue, not just Polymarket and Kalshi
4. The signal engine's `handle_prediction_snapshot` already has the right data flow: cache prediction market snapshot, compare against options-implied probability. The only hardcoding is the venue filter at line 292
5. If historical JSONL compatibility is needed, keep old `SpreadPattern` values as-is and add new patterns with new names

**Warning signs:**
- Planning documents mention "refactoring SpreadPattern to be venue-generic"
- Someone adds `BuyPolyYesSellDeribitYes` and 3 more variants per venue pair
- Test count explodes (currently 40+ tests for 4 patterns x combinations)
- Historical spread logs cannot be parsed after schema change

**Phase to address:**
Engine generalization phase. Must decide upfront: generalize SignalEngine (small change) vs. refactor SpreadEngine (large, risky change). SignalEngine generalization is the correct path.

---

### Pitfall 4: REST Polling Fallback Produces Stale or Inconsistent Order Book Snapshots

**What goes wrong:**
Polymarket's REST `/book` endpoint (GET `https://clob.polymarket.com/book?token_id=...`) has a documented issue where it returns stale "ghost market" data (best bid 0.01, best ask 0.99) while the WebSocket and `/price` endpoint show correct prices (GitHub issue #180 on py-clob-client). A REST polling fallback that uses `/book` will produce snapshots that look valid (they have bids and asks) but contain stale prices, generating false spread signals.

**Why it happens:**
The `/book` endpoint appears to serve from a different backend cache than `/price` or `/midpoint`. The order book endpoint lags behind or serves disconnected snapshots. This is a known Polymarket infrastructure issue, not a client bug. If the fallback polls `/book` and feeds the result into the same `MarketSnapshot` pipeline as WebSocket data, the staleness is invisible because the snapshot has the right structure -- just wrong values.

**How to avoid:**
For REST polling, use `/price` (best bid/ask) or `/midpoint` instead of `/book`:
- `/price` returns current best bid and best ask -- sufficient for probability extraction (prediction market prices ARE probabilities)
- `/midpoint` returns the average -- even simpler
- These endpoints are reported as reliable even when `/book` is stale
- For v1.7, full order book depth is not needed -- the signal engine compares midpoint probability against options-implied probability
- Add a cross-validation check: if REST price diverges from the last WebSocket price by more than a configurable threshold (e.g., 10%), flag the data as suspicious rather than silently using it
- Include the data source (WebSocket vs REST) in the `MarketSnapshot` or as a log field so you can audit which source produced each signal

**Warning signs:**
- REST-sourced spreads show abnormally wide spreads (0.01/0.99 produces ~98% spread)
- Signal rate spikes when falling back to REST (false signals from stale data)
- REST book data shows no change across multiple polling intervals while WebSocket (when working) shows activity

**Phase to address:**
REST polling fallback phase. Must validate REST endpoint reliability before trusting it as a data source.

---

### Pitfall 5: Generalizing the Signal Engine Venue Filter Breaks the Event Registry Lookup Contract

**What goes wrong:**
The signal engine's `handle_prediction_snapshot` (line 292) filters to `Venue::Polymarket` and `Venue::Kalshi` only. The obvious fix is to remove this filter or make it configurable. But the downstream `registry.lookup_by_instrument(snap.venue, &snap.instrument_id)` depends on the event registry having a venue entry for the given venue. If a new prediction market venue is added but the event registry's `EventMapping` struct does not have a corresponding venue field, the lookup returns `None` and the snapshot is silently dropped -- with no error, no warning, and no metric.

**Why it happens:**
The `EventMapping` struct in the event registry has explicit optional fields: `venues.polymarket`, `venues.kalshi`, `venues.deribit`, `venues.derive`. Adding a new venue requires adding a new field to `EventMapping`, updating the TOML parser, updating the registry lookup, and updating the discovery pipeline. The venue filter in the signal engine is just the visible tip of the iceberg -- removing it without updating the registry pathway creates a silent data hole.

**How to avoid:**
For v1.7, the generalization is minimal because Polymarket is already supported in the registry:
1. Change the signal engine venue filter from `snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi` to a configurable list or a trait-based check (`venue.is_prediction_market()`)
2. Add a `Venue::is_prediction_market()` method that returns true for Polymarket and Kalshi (expandable later)
3. Do NOT remove the filter entirely -- Deribit and Derive snapshots should still be routed to `handle_options_snapshot`, not `handle_prediction_snapshot`
4. Add a Prometheus counter for "prediction snapshot received but no registry mapping found" to detect silent drops early

**Warning signs:**
- Signal engine processes zero Polymarket snapshots despite feed being connected and data flowing
- `lookup_by_instrument` returns `None` for instruments that have valid event mappings
- Adding a new venue to the filter produces zero signals because the registry has no mapping for it

**Phase to address:**
Engine generalization phase. The venue filter change and registry validation must be done together.

---

### Pitfall 6: Spread Engine Hardcoded Two-Leg Requirement Blocks Single-Venue-vs-Options Signals

**What goes wrong:**
The spread engine at line 228 checks `mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none()` and returns early. This was correct for v1.0 (Polymarket vs Kalshi arbitrage) but blocks v1.7 entirely. With Kalshi geo-blocked and disabled, EVERY event mapping has `kalshi = None`, so the spread engine produces zero output for every snapshot. The system appears to run normally -- feeds connect, data flows, no errors -- but zero spreads and zero signals are generated.

**Why it happens:**
The spread engine was designed for prediction-market-vs-prediction-market arbitrage (Polymarket vs Kalshi). The v1.7 goal is prediction-market-vs-options (Polymarket vs Deribit/Derive). These are architecturally different computations: the spread engine compares two probability snapshots, while the signal engine compares a probability snapshot against an options-implied probability. The spread engine's two-leg requirement is not a bug -- it is correct for its intended purpose. The mistake would be trying to force the spread engine to handle a fundamentally different computation.

**How to avoid:**
Do not modify the spread engine's two-leg check. Instead:
1. Recognize that the signal engine (`signal/engine.rs`) already implements the correct architecture for v1.7
2. The signal engine receives options-implied probability from the pricing engine and prediction market snapshots separately, then compares them
3. The only changes needed are: (a) remove the Polymarket/Kalshi venue filter (Pitfall 5), and (b) ensure events.toml mappings work with a single prediction market venue
4. The spread engine can remain dormant (returning early on every snapshot) until Kalshi becomes available again

**Warning signs:**
- Planning documents describe "making the spread engine work with single venues"
- Spread engine modifications introduce conditional logic for "if only one prediction market, use options probability instead"
- The spread engine's clean two-venue comparison gets polluted with options-probability logic that already exists in the signal engine

**Phase to address:**
Engine generalization phase. Critical architectural decision: signal engine generalization (correct) vs. spread engine modification (wrong).

---

### Pitfall 7: REST Polling Interval Too Aggressive -- Silent Rate Limiting Causes Data Gaps

**What goes wrong:**
Polymarket's REST API has undocumented rate limits. Polling `/price` or `/midpoint` every 1-2 seconds for multiple tokens triggers rate limiting. Unlike WebSocket disconnection (which is visible), REST rate limiting may manifest as: HTTP 429 responses, HTTP 200 with stale/cached data, or connection timeouts. The system detects rate limiting differently than connection failure, and the existing exponential backoff (designed for WebSocket reconnection) does not apply to REST polling.

**Why it happens:**
Developers set aggressive polling intervals to approximate WebSocket-like latency. But prediction market arbitrage windows are minutes-to-hours (per PROJECT.md "arb windows are minutes-to-hours"), not milliseconds. A 30-second polling interval is more than sufficient for signal generation, but developers instinctively choose 1-5 second intervals because "faster is better."

**How to avoid:**
- Start with a 30-second polling interval for REST. This is fast enough for minutes-to-hours arbitrage windows
- Use the existing `rate_limit_per_second = 10` config for Polymarket to gate REST requests through the shared rate limiter (already used by discovery and settlement polling)
- Batch multiple token price requests using `/prices` (plural) instead of calling `/price` per token -- one request for all tokens instead of N requests
- Monitor HTTP response codes: track 429 responses as a Prometheus counter; alert on sustained 429s
- Implement adaptive backoff specific to REST: double the polling interval on 429, halve it (down to minimum) after sustained success
- Do NOT use the WebSocket reconnection backoff for REST polling -- they are fundamentally different retry patterns

**Warning signs:**
- HTTP 429 responses from Polymarket REST API
- REST polling returns identical data across multiple consecutive polls (stale cache)
- Polymarket REST request counter shows more requests than expected based on configured interval
- Rate limiter token bucket stays empty (requests consuming all available tokens)

**Phase to address:**
REST polling fallback phase. Polling interval and rate limiting must be validated against Polymarket's actual limits before production deployment.

---

### Pitfall 8: Dual Data Source (WebSocket + REST) Creates Conflicting Snapshots in the Pipeline

**What goes wrong:**
With both WebSocket and REST polling active simultaneously (WebSocket as primary, REST as fallback), both sources feed `MarketSnapshot` into the same channel. When WebSocket recovers from a freeze, two snapshot sources produce competing updates for the same instrument. The signal engine sees rapid oscillation between WebSocket prices and REST prices (which may differ due to timing), generating spurious signals from the jitter between sources.

**Why it happens:**
The existing pipeline assumes one snapshot source per venue. The `latest_pred` HashMap in the signal engine keys on `(event_id, venue)` -- if both WebSocket and REST produce snapshots for the same `(event_id, Venue::Polymarket)`, they overwrite each other. The signal engine does not know which source produced the snapshot, so it cannot prefer one over the other or detect source-switching jitter.

**How to avoid:**
Implement source priority, not source duplication:
1. **Exclusive mode:** Only one source active at a time per venue. REST activates only when WebSocket data watchdog triggers (Pitfall 1). REST deactivates when WebSocket recovers (first data message received after reconnection).
2. **Switchover hysteresis:** When WebSocket recovers, wait for N consecutive data messages (e.g., 3) before switching back from REST. This prevents rapid switching during intermittent WebSocket freezes.
3. **Source tagging:** Add an optional `data_source: DataSource` field (WebSocket/REST) to `MarketSnapshot` for logging and debugging, but do NOT use it in signal computation logic.
4. **Single channel, gated input:** The REST poller checks a shared `AtomicBool` (or similar) controlled by the supervisor. If WebSocket is healthy, REST poller skips sending to the channel.

**Warning signs:**
- Signal logs show rapid alternation between slightly different prices for the same instrument
- Signal rate doubles when both sources are active
- Spread values oscillate with a period matching the REST polling interval

**Phase to address:**
REST polling fallback phase. Source coordination must be designed before implementing the REST poller.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Keeping `SpreadPattern` Polymarket+Kalshi hardcoded | Zero risk to existing JSONL schema and tests | Cannot use spread engine for new venue pairs without combinatorial variant explosion | Acceptable permanently -- signal engine is the correct path for v1.7+ |
| REST polling without full order book depth | Simpler implementation, avoids stale `/book` endpoint | Cannot do walk-the-book slippage estimation on REST data | Acceptable for v1.7 -- midpoint comparison is sufficient for signal generation at this stage |
| Hardcoded `Venue::is_prediction_market()` list | Simple, no trait complexity | Must update function body when adding venue | Acceptable at 4 venues; reconsider at 8+ |
| Keeping both WebSocket and REST code paths for Polymarket | Resilience against either path failing | Two code paths to maintain, test, and monitor | Acceptable -- Polymarket WebSocket reliability is genuinely poor |
| Not generalizing `SpreadEngine` for v1.7 | Zero regression risk on proven code | Spread engine is dormant code that still compiles and runs (returning early) | Acceptable -- dormant code is cheaper than broken refactored code |

## Integration Gotchas

Common mistakes when connecting to external services.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Polymarket WebSocket | Treating ping/pong success as proof of data flow | Track `last_data_message_at` separately from connection health; implement data inactivity watchdog |
| Polymarket REST `/book` | Assuming order book endpoint returns current data | Use `/price` or `/midpoint` instead; `/book` has known staleness issues (GitHub #180) |
| Polymarket REST batch endpoints | Calling `/price` per token in a loop | Use `/prices` (plural) with multiple token IDs in one request to stay within rate limits |
| tokio-tungstenite from EC2 | Assuming connection failure is application-level | Test with `websocat` first to isolate network vs. application issues; enable TCP keepalive on socket |
| Polymarket token IDs | Using condition_id in WebSocket subscription | WebSocket `assets_ids` field requires token_id, not condition_id (existing code is correct -- preserve this) |
| Event registry with Kalshi disabled | Expecting events.toml entries to have both polymarket and kalshi venues | Signal engine must work with single prediction market venue; spread engine correctly returns early |

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| REST polling every token individually | Rate limit exhaustion, 429 responses | Batch via `/prices` endpoint; poll at 30s not 1s | >10 tokens being tracked |
| Spawning a new REST client per poll interval | Connection overhead, TCP handshake per request | Reuse `reqwest::Client` with connection pooling (already used by discovery/settlement) | Continuous polling over hours |
| Watchdog timer checked on every message | CPU waste processing frequent timer checks | Use `tokio::time::interval` or `tokio::select!` timeout branch, not manual timestamp comparison | >100 messages/second (unlikely for Polymarket) |
| HashMap clone in signal engine on every snapshot | Unnecessary allocation | Use references where possible; clone only the snapshot being computed | >50 events with frequent updates |

## Security Mistakes

Domain-specific security issues beyond general web security.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Logging full REST API response bodies | Potential information leak; large log volume | Log status code and data summary only; full response at TRACE level |
| REST polling endpoint configured to wrong URL | Could send requests to malicious endpoint mimicking Polymarket | Validate REST URL in config validation (add `validate_https_url` for any new REST endpoints) |
| No TLS certificate validation on REST client | Man-in-the-middle on price data could generate false signals | Use `reqwest` default TLS validation; do not set `danger_accept_invalid_certs` |

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **WebSocket fix:** Connection succeeds from EC2 but data watchdog not implemented -- verify data actually flows for >30 minutes continuously, not just that connection establishes
- [ ] **REST fallback:** Endpoint returns 200 but data is stale ghost-market -- verify returned prices match WebSocket prices within tolerance; cross-check with `/midpoint`
- [ ] **Signal engine generalization:** Venue filter removed but registry lookup returns None -- verify event registry has correct mappings for active prediction market venues; add counter for lookup misses
- [ ] **Dual source coordination:** Both WebSocket and REST active but no source priority -- verify only one source feeds snapshots at a time; check for price oscillation in signal logs
- [ ] **Spread engine dormancy:** Spread engine returns early on all events (correct) but this is not logged -- add a startup log message confirming spread engine is in dormant mode (single prediction market)
- [ ] **Event mappings:** events.toml has Polymarket entries but Kalshi entries are empty -- verify signal engine processes events with only one prediction market venue configured
- [ ] **Rate limiting:** REST poller respects Polymarket rate limits -- verify using shared rate limiter; check for 429 responses in first 24 hours of production

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| WebSocket silent freeze (no watchdog) | LOW | Add watchdog; reconnection supervisor already handles reconnection correctly once triggered |
| Connection reset from EC2 | MEDIUM | Switch to REST-primary mode if WebSocket is persistently unreachable; no data loss, just reduced update frequency |
| SpreadPattern refactored and JSONL broken | HIGH | Historical spread logs become unparseable; must write migration script or accept data loss; analysis CLIs break |
| REST stale data generating false signals | MEDIUM | Add cross-validation; filter signals generated from REST data retroactively; review paper trade entries from REST-sourced signals |
| Signal engine generalization breaks existing flow | LOW | Revert venue filter to original; regression is confined to 2 lines of code (line 292-293) |
| Dual source oscillation | LOW | Disable REST poller temporarily; signals from WebSocket-only are correct; fix source coordination logic |
| REST rate limiting | LOW | Increase polling interval; use batch endpoints; rate limiting is transient, not permanent |

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| WebSocket silent freeze (Pitfall 1) | WebSocket connectivity fix | Data flows continuously for >1 hour from EC2; watchdog triggers reconnection within 2 minutes of freeze |
| Connection reset from EC2 (Pitfall 2) | WebSocket connectivity fix | Connection persists for >1 hour; or, if unfixable, decision documented to use REST as primary |
| SpreadPattern hardcoding (Pitfall 3) | Engine generalization | SpreadPattern enum unchanged; signal engine handles single prediction market venue; no JSONL schema break |
| REST stale data (Pitfall 4) | REST polling fallback | REST prices cross-validated against WebSocket within tolerance; `/price` endpoint used, not `/book` |
| Signal engine venue filter (Pitfall 5) | Engine generalization | `Venue::is_prediction_market()` method exists; Prometheus counter shows prediction snapshots processed > 0 |
| Spread engine two-leg block (Pitfall 6) | Engine generalization | Spread engine confirmed dormant (logged); signal engine produces signals from single prediction market |
| REST polling rate limits (Pitfall 7) | REST polling fallback | Zero 429 responses in first 24 hours; batch endpoints used; polling interval >= 30s |
| Dual source conflicts (Pitfall 8) | REST polling fallback | Source coordination verified: only one source active at a time; no price oscillation in signal logs |

## Sources

- [Polymarket CLOB WSS Silent Freeze -- GitHub #292](https://github.com/Polymarket/py-clob-client/issues/292) -- active issue, server accepts connection but sends no data
- [Polymarket WebSocket Stream Stops -- GitHub #26](https://github.com/Polymarket/real-time-data-client/issues/26) -- data stream stops after ~20 minutes, server-side issue confirmed
- [Polymarket REST /book Stale Data -- GitHub #180](https://github.com/Polymarket/py-clob-client/issues/180) -- order book endpoint returns ghost market data while price endpoint is accurate
- [Polymarket WebSocket Reconnection Issues -- GitHub #185/186](https://github.com/Polymarket/rs-clob-client/issues/186) -- reconnection mechanism failures
- [Polymarket Public REST Methods](https://docs.polymarket.com/developers/CLOB/clients/methods-public) -- available endpoints for price, midpoint, book
- [Polymarket WebSocket Documentation](https://docs.polymarket.com/market-data/websocket/overview) -- WebSocket channel specification
- [tokio-tungstenite Connection Reset -- GitHub #296](https://github.com/snapview/tokio-tungstenite/issues/296) -- protocol-level connection reset without closing handshake
- Codebase analysis: `src/spread/engine.rs:228`, `src/signal/engine.rs:292`, `src/spread/patterns.rs`, `src/feed/polymarket/client.rs`, `src/feed/polymarket/supervisor.rs`

---
*Pitfalls research for: Polymarket WebSocket connectivity, REST polling fallback, and spread/signal engine generalization*
*Researched: 2026-03-09*
