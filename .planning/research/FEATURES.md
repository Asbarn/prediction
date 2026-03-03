# Feature Research: v1.5 Derive.xyz Venue Integration

**Domain:** Options venue feed integration for cross-venue arbitrage (Derive.xyz / Lyra v2)
**Researched:** 2026-03-03
**Confidence:** MEDIUM (API structure confirmed from docs.derive.xyz and CCXT implementation; specific field names inferred from documentation search results and analogous Deribit implementation; WebSocket channel format MEDIUM confidence pending direct API testing)

**Scope note:** This research covers ONLY the new features needed to add Derive.xyz as a fourth venue. The existing pipeline (MarketSnapshot bus, Black-76 pricing, spread calculator, signal engine, subscription manager, discovery framework) is already complete and operational. This milestone slots Derive into the existing architecture.

**Existing infrastructure this builds on:**

| Component | Location | How Derive Reuses It |
|-----------|----------|----------------------|
| MarketSnapshot bus | `src/types/snapshot.rs` | Unchanged -- Derive emits the same struct |
| Black-76 IV solver + call spread replication | `src/pricing/` | Unchanged -- Derive BTC options are European-style, same math |
| SpreadEngine + signal generation | `src/signal/` | Unchanged -- Derive is just another venue source |
| SubscriptionManager (watch channel + reconnect) | `src/feed/pipeline.rs` | Derive supervisor receives same `watch::Receiver<Vec<String>>` |
| DeribitProcessor (normalize.rs + book.rs) | `src/feed/deribit/` | Template for DeriveProcessor; channel parsing differs |
| DeribitSupervisor + backoff reconnect | `src/feed/deribit/supervisor.rs` | Verbatim copy-and-adapt for Derive |
| VenueRateLimiter | `src/feed/reliability/rate_limiter.rs` | Reused with Derive rate limit config |
| VenueHealth + heartbeat monitoring | `src/feed/health.rs` | Reused unchanged |
| RecordLine JSONL recording | `src/feed/recording/` | Reused with `venue = Venue::Derive` |
| DiscoveredInstrument + FuzzyMatchKey | `src/events/discovery.rs` | New `discover_derive()` function added |
| EventRegistry + TOML writer | `src/events/` | Unchanged -- Derive instruments get `approved = false` proposals |
| Settlement checker framework | `src/settlement/` | New `DeriveChecker` added |
| Prometheus metrics | `src/metrics_export/` | Reused with `venue = "derive"` label |

---

## Table Stakes

Features required for Derive to be a functional fourth venue. Missing any of these means the venue cannot contribute to spread calculations.

### TS-1: Derive WebSocket Client (connect + subscribe)

| Attribute | Detail |
|-----------|--------|
| Why Expected | Every venue integration starts with a WebSocket connection. Without a connected feed, nothing else works. |
| Complexity | LOW |
| Dependencies | Existing tokio-tungstenite infrastructure from DeribitClient |

**What it is:** A `DeriveClient` that connects to the Derive WebSocket endpoint and sends a JSON-RPC subscribe request for orderbook and ticker channels per instrument.

**API specifics (MEDIUM confidence from docs.derive.xyz):**
- Production WS URL: `wss://api.lyra.finance/ws` (testnet: `wss://api-demo.lyra.finance/ws`)
- Protocol: JSON-RPC 2.0 over WebSocket (same as Deribit)
- Transport agnostic: same `method` and `params` work over both HTTP and WebSocket
- Authentication: public market data channels (orderbook, ticker) do NOT require auth. Auth is only required for private/trading endpoints.
- Subscribe method: `public/subscribe` with a `channels` array (confirmed -- same pattern as Deribit)
- Heartbeat: Derive uses its own keepalive mechanism (details need live API testing; assume WebSocket-level ping/pong initially)

**Channel names to subscribe per instrument (MEDIUM confidence):**
- Orderbook: `{instrument_name}.orderbook` or similar -- exact format needs live verification from docs.derive.xyz/reference/json-rpc
- Ticker: `{instrument_name}.ticker` or similar -- exact format needs live verification

**Key difference from Deribit:** Deribit uses dot-separated channels like `book.{instrument}.none.20.100ms`. Derive is expected to use a different channel naming convention. The exact format must be verified from the live documentation or API testing before implementation.

**Implementation:** DeriveClient follows DeribitClient exactly:
- `AtomicU64` request ID counter
- Batch subscribe in a single `public/subscribe` call
- Forward non-heartbeat text frames to `mpsc::Receiver<RawMessage>`
- Cancellation via `CancellationToken`
- Rate limiter on outbound subscribe messages

**Reuses:** `DeribitClient` as structural template; `VenueRateLimiter`, `CancellationToken`, `tokio-tungstenite`.

---

### TS-2: Derive Instrument Name Format Parsing

| Attribute | Detail |
|-----------|--------|
| Why Expected | Instrument IDs are the keys that tie book data, ticker data, and event mappings together. Without correct ID parsing, the entire pipeline breaks. |
| Complexity | LOW |
| Dependencies | None (pure string processing) |

**What it is:** Functions to parse and construct Derive instrument names.

**Confirmed format (HIGH confidence from CCXT derive.py and docs.derive.xyz):**
```
{ASSET}-{YYYYMMDD}-{STRIKE}-{C|P}
```
Examples:
- `BTC-20250328-100000-C` (BTC call, expiry 2025-03-28, $100,000 strike)
- `BTC-20251226-80000-P` (BTC put, expiry 2025-12-26, $80,000 strike)
- `ETH-20250228-1000-P` (ETH put -- confirmed format from CCXT docs)

**Key difference from Deribit:** Deribit uses `BTC-27JUN25-100000-C` (DDMmmYY). Derive uses `BTC-YYYYMMDD-STRIKE-C/P` (ISO date). The parsers are completely different.

**Implementation:**
- `parse_derive_instrument(name: &str) -> Option<(String, NaiveDate, Decimal, Direction)>`
  - Split by `-`, parts 0=asset, 1=YYYYMMDD, 2=strike, 3=C/P
  - Parse date with `NaiveDate::parse_from_str(parts[1], "%Y%m%d")`
  - Parse strike as `Decimal`
  - Map C -> Direction::Above, P -> Direction::Below
- Unit tests covering: valid BTC calls, valid ETH puts, malformed inputs, edge strikes (100, 1000000)

**Reuses:** `NaiveDate`, `rust_decimal::Decimal`, `crate::config::Direction`.

---

### TS-3: Derive Order Book Maintenance

| Attribute | Detail |
|-----------|--------|
| Why Expected | Best bid/ask and depth are needed for spread calculation. Without a maintained order book, MarketSnapshot has no prices. |
| Complexity | LOW-MEDIUM |
| Dependencies | TS-1 (raw messages flowing in), TS-2 (instrument name parsing) |

**What it is:** A `DeriveBook` structure (equivalent to Deribit's `InstrumentBook`) that applies incoming order book snapshots and updates.

**Derive orderbook model (MEDIUM confidence):**
- Derive uses a central-limit-order-book (CLOB) with a Rust-powered offchain matching engine
- Orderbook updates come as either full snapshots or incremental deltas (exact format needs live API verification)
- Prices are in USD; amounts are in BTC (base asset units)
- Depth: configurable (likely 10 or 20 levels supported)

**Difference from Deribit:** Deribit's `book.{instrument}.none.20.100ms` channel sends periodic grouped snapshots. Derive likely sends incremental delta updates OR periodic snapshots -- the exact model must be verified. If incremental, a sequence-number gap handler (same as Deribit's `SequenceError::Gap`) is required.

**Implementation:** Model after `InstrumentBook` in `src/feed/deribit/book.rs`:
- `BTreeMap<Price, Notional>` for bids (desc) and asks (asc)
- `apply_snapshot()` and optionally `apply_delta()` methods
- `best_bid()` / `best_ask()` accessors
- `is_stale` flag for sequence gaps

**Reuses:** `InstrumentBook` pattern verbatim with Derive-specific snapshot/delta deserialization.

---

### TS-4: Derive Ticker Feed (mark_iv, bid_iv, ask_iv, index_price)

| Attribute | Detail |
|-----------|--------|
| Why Expected | The Black-76 IV solver needs bid_iv and ask_iv to work. Without ticker data, options pricing cannot produce implied volatilities, and the whole spread pipeline breaks. |
| Complexity | LOW-MEDIUM |
| Dependencies | TS-1 (raw messages flowing) |

**What it is:** Parse Derive ticker notifications to extract pricing data for options.

**Confirmed available fields from docs.derive.xyz (MEDIUM confidence):**
- `best_bid_price` / `best_ask_price` -- top-of-book prices (USD, option contract value)
- `best_bid_amount` / `best_ask_amount` -- sizes
- `mark_price` -- exchange mark price (USD)
- `index_price` -- BTC/USD spot index price
- `mark_iv` -- exchange-computed mark implied volatility (%)
- `bid_iv` / `ask_iv` -- bid/ask implied volatilities (%)
- `instrument_name` -- identifies which instrument this ticker belongs to
- `timestamp` -- millisecond epoch timestamp

**Key fields for the pipeline:**
- `bid_iv` + `ask_iv` feed directly into `IvSpread` for the Black-76 vol surface
- `index_price` provides the BTC/USD spot for `underlying_price` in MarketSnapshot
- `mark_price` provides the exchange's mid-market option price

**How this maps to MarketSnapshot:** Same field mapping as Deribit's `TickerData` -> `TickerState` -> `MarketSnapshot`. The `underlying_index` field on Derive may differ (Deribit uses futures contract name like "BTC-27JUN25"; Derive may use a simpler string).

**Reuses:** `TickerState` struct from Deribit can be reused almost unchanged; Derive's ticker field names differ but map to the same slots.

---

### TS-5: Derive Message Processor and Normalization

| Attribute | Detail |
|-----------|--------|
| Why Expected | Raw WebSocket frames must become MarketSnapshot events before they're useful. This is the core translation layer. |
| Complexity | MEDIUM |
| Dependencies | TS-2 (instrument parsing), TS-3 (book maintenance), TS-4 (ticker state) |

**What it is:** A `DeriveProcessor` that consumes `RawMessage` frames, parses them into Derive-specific message types, routes by channel, maintains book and ticker state, and emits `MarketSnapshot`.

**Structure (modeled exactly on `DeribitProcessor` in `src/feed/deribit/normalize.rs`):**
- `derive_messages.rs` -- Derive-specific serde structs (equivalent to `deribit/messages.rs`)
- `derive_channels.rs` -- channel name parsing and routing (equivalent to `deribit/channels.rs`)
- `derive_normalize.rs` -- main processor loop with `build_snapshot()` call

**Key difference from Deribit:** Deribit's `build_snapshot()` is already generic -- it takes `InstrumentBook` and `TickerState` and produces `MarketSnapshot`. Derive's processor calls the same function with Derive-specific inputs. No changes to `build_snapshot()` are required; only the upstream parsing differs.

**MarketSnapshot fields populated:**
- `venue: Venue::Derive` (new enum variant needed)
- `instrument_id`: Derive instrument name (e.g., `BTC-20250328-100000-C`)
- `bid`, `ask`, `bid_size`, `ask_size`, `depth_bids`, `depth_asks`: from order book
- `mark_price`, `index_price`: from ticker
- `mark_iv`, `bid_iv`, `ask_iv`: from ticker (feeds IV solver)
- `underlying_price`: index_price (BTC/USD spot)
- `exchange_timestamp`: from message timestamp field
- `is_stale`: sequence gap OR exchange timestamp staleness

**Venue enum change:** Add `Venue::Derive` to `src/types/ids.rs` (or wherever `Venue` is defined). This ripples through: venue labels in metrics, recording, settlement dispatch, alert monitoring. Mechanical but non-trivial in volume.

**Reuses:** `build_snapshot()` from `deribit/normalize.rs` unchanged; `InstrumentBook` with new Derive-specific snapshot deserialization; `TickerState`/`GreeksState` reused.

---

### TS-6: Derive Reconnection Supervisor

| Attribute | Detail |
|-----------|--------|
| Why Expected | Network drops are expected in production. The supervisor ensures the feed self-heals. |
| Complexity | LOW |
| Dependencies | TS-1 (DeriveClient), TS-5 (processor) |

**What it is:** A `DeriveSupervisor` that wraps `DeriveClient` with exponential backoff reconnection.

**Implementation:** Verbatim copy of `DeribitSupervisor` with types changed. The reconnect pattern (watch channel, backoff, first-message reset) is identical.

**Reuses:** `ExponentialBackoffBuilder`, `watch::Receiver<Vec<String>>`, `VenueHealth`, `VenueRateLimiter` -- all unchanged.

---

### TS-7: Derive Discovery via REST API

| Attribute | Detail |
|-----------|--------|
| Why Expected | Without instrument discovery, the system cannot find which BTC option pairs to compare across venues. Manual TOML entry is not sustainable. |
| Complexity | MEDIUM |
| Dependencies | TS-2 (instrument name parsing), existing `DiscoveredInstrument` framework |

**What it is:** A `discover_derive()` function that fetches active BTC options from the Derive REST API and normalizes them into `DiscoveredInstrument`.

**API endpoint (MEDIUM confidence from docs.derive.xyz):**
- Method: `POST /public/get_instruments` (Derive uses HTTP POST for all methods)
- Parameters: `{"currency": "BTC", "instrument_type": "option", "expired": false}` (exact field names need verification)
- Response: array of instrument objects with `instrument_name`, `expiry_timestamp` (or `expiry_date`), `strike`, `option_type`, `is_active` (field names MEDIUM confidence -- inferred from CCXT derive.py)
- No authentication required for public endpoints
- REST base URL: `https://api.lyra.finance` (or `https://api-demo.lyra.finance` for testnet)

**Instrument parsing:** Use `parse_derive_instrument()` from TS-2 to extract asset, date, strike, direction from the `instrument_name` field. This avoids dependency on API field names for the core data.

**Rate limits (MEDIUM confidence from docs.derive.xyz):**
- Fixed-window algorithm, refill every 5 seconds
- Public endpoints: lower limit (exact number not found; assume 10 req/5s as conservative estimate)
- Market makers eligible for higher limits
- Use existing `VenueRateLimiter` with configured rate

**Integration with existing framework:**
- `discover_derive()` returns `Vec<DiscoveredInstrument>`
- Feeds into `find_cross_venue_candidates_fuzzy()` alongside Deribit and Polymarket results
- Auto-proposes matches to `events.toml` with `approved = false`
- Existing `approved = false` human gate applies unchanged

**Config addition:** New `DeriveConfig` struct in `venues.toml` with `rest_url`, `ws_url`, `rate_limit_per_second`, `staleness_threshold_ms`, `reconnect`, `instruments` fields -- mirrors `DeribitConfig` structure.

**Reuses:** `DiscoveredInstrument`, `FuzzyMatchKey`, `compute_expiry_confidence()`, `VenueRateLimiter`, `reqwest::Client`.

---

### TS-8: Derive Settlement Outcome Tracking

| Attribute | Detail |
|-----------|--------|
| Why Expected | Settlement outcomes are needed to validate paper trade P&L and signal quality. Without settlement data, the signal scoring CLI has no ground truth. |
| Complexity | MEDIUM |
| Dependencies | Existing `VenueChecker` settlement framework in `src/settlement/` |

**What it is:** A `DeriveChecker` that polls the Derive settlement REST API for option settlement prices and resolves settled positions.

**API endpoint (confirmed from docs.derive.xyz):**
- `POST /public/get_option_settlement_prices` -- gets settlement prices by expiry for each currency
- Parameters: `{"currency": "BTC"}` (currency is required parameter)
- Response: settlement prices keyed by expiry with BTC/USD TWAP settlement values
- `POST /public/get_option_settlement_history` -- historical settlement records

**Settlement mechanics (HIGH confidence from help.lyra.finance and docs.derive.xyz):**
- All options expire at 08:00 UTC
- Settlement price = 30-minute TWAP of BTC/USD spot price ending at 08:00 UTC
- Payout in USDC (not in BTC -- cash-settled)
- Oracle: Block Scholes provides settlement data, posted on-chain for transparency
- Call settles to max(0, index_price - strike); put settles to max(0, strike - index_price), in USD, paid in USDC

**Difference from Deribit settlement:** Deribit settles options in BTC (inverse contracts -- BTC-margined). Derive settles in USDC (linear contracts -- USD-margined). This affects the payout calculation but NOT the probability extraction pipeline (which works in normalized 0-1 probability space regardless).

**Implementation:** Add `DeriveChecker` implementing `VenueChecker` trait. The 4-tier polling cadence (startup backfill, hourly, post-expiry, on-demand) applies unchanged.

**Reuses:** `VenueChecker` trait, `SettlementMonitor`, 4-tier polling cadence logic.

---

### TS-9: Derive Feed Integration into Main Pipeline

| Attribute | Detail |
|-----------|--------|
| Why Expected | The venue is useless if it's not wired into the running system. |
| Complexity | MEDIUM |
| Dependencies | TS-1 through TS-8, existing pipeline in `src/main.rs` and `src/feed/pipeline.rs` |

**What it is:** Wire the Derive feed into the main application startup:
1. Add `DeriveConfig` to `VenuesConfig` (in `src/config/venues.rs`)
2. Spawn `DeriveSupervisor` alongside existing Deribit and Polymarket supervisors
3. Add Derive `instruments_tx` watch channel to `SubscriptionManager`
4. Add `DeriveProcessor` to the pipeline (snapshot fan-out)
5. Add Derive cleanup channel to the 5-engine cleanup list
6. Add `DeriveChecker` to `SettlementMonitor`'s venue checker list
7. Add Derive discovery to the discovery background task

**Venue enum ripple:** Adding `Venue::Derive` requires updates in all `match venue` exhaustive arms:
- `src/signal/` -- SpreadEngine venue labeling
- `src/settlement/` -- checker dispatch
- `src/alert/` -- liveness monitoring
- `src/metrics_export/` -- Prometheus labels
- `src/feed/recording/` -- JSONL venue tags
- `src/events/discovery.rs` -- discovery dispatch
- `src/paper_trade/` -- position tracking

**Reuses:** All existing pipeline components; main.rs startup pattern.

---

## Differentiators

Features that add value beyond the minimum Derive integration but are not strictly required for cross-venue signal generation.

### DIFF-1: Three-Way Cross-Venue Spread (Deribit vs Derive vs Polymarket)

| Attribute | Detail |
|-----------|--------|
| Value Proposition | With two options venues (Deribit + Derive), the system can detect divergences between venues for the same contract -- a purer arbitrage signal than options-vs-prediction. |
| Complexity | LOW |
| Dependencies | TS-5 (Derive producing MarketSnapshot), existing SpreadEngine |

**What it is:** The existing `SpreadEngine` computes cross-venue spreads between any two venues with a matched `event_id`. Adding `Venue::Derive` means:
- **Deribit vs Polymarket** (existing)
- **Derive vs Polymarket** (new -- same math, different venue label)
- **Deribit vs Derive** (new -- direct options-vs-options comparison; both run Black-76 pricing)

The Deribit vs Derive pair is particularly interesting: both are European-style BTC options with the same underlying. Any pricing divergence between them is a purer options arbitrage, less contaminated by prediction market basis risk.

**Implementation:** No code changes to SpreadEngine required. When Derive instruments are approved in events.toml with a Derive venue entry, SpreadEngine automatically picks up the new venue pairs. The only required change is ensuring `Venue::Derive` is handled in venue-pair labeling functions used by spread_analytics CLI.

**Value for the project:** This is the primary motivation for adding Derive. The Deribit vs Derive spread is an actionable institutional arbitrage signal that does not depend on prediction market liquidity.

---

### DIFF-2: Derive Options Implied Probability (via existing Black-76 pipeline)

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Derive options can be converted to implied probabilities using the same Black-76 call spread replication already in production for Deribit. This makes Derive a full substitute for Deribit in cross-venue comparisons. |
| Complexity | LOW (pipeline already exists) |
| Dependencies | TS-4 (bid_iv, ask_iv fields populated), existing `PricingEngine` in `src/pricing/` |

**What it is:** The existing `PricingEngine` already processes MarketSnapshots from Deribit to compute `bid_probability` and `ask_probability`. Adding `Venue::Derive` to the list of venues the PricingEngine processes is likely a one-line change or a new config entry.

**Key prerequisite:** Derive's `bid_iv` and `ask_iv` must be populated correctly in MarketSnapshot (from TS-4). The Black-76 math is identical for European cash-settled options on BTC.

**Settlement alignment note:** Derive settles options at 08:00 UTC (confirmed). Deribit also settles at 08:00 UTC. This means the time-to-expiry calculation is aligned between venues -- no adjustment needed in the Black-76 `t` parameter.

**Reuses:** `PricingEngine`, `IvSolver`, `CallSpreadReplicator`, `VolSurface` -- all unchanged.

---

### DIFF-3: Derive-Specific Discovery Configuration

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Discovery can be tuned to Derive's expiry schedule (flexible user-defined expiries vs Deribit's fixed weekly/monthly schedule). This prevents flood of proposals for far-dated or illiquid contracts. |
| Complexity | LOW |
| Dependencies | TS-7 (discover_derive()) |

**What it is:** Config parameters in `derive.toml` to filter discovery output:
- `max_expiry_days`: skip instruments expiring more than N days out (default: 90)
- `min_open_interest`: skip instruments below OI threshold (if OI available in API response)
- `currencies`: list of currencies to discover (default: `["BTC"]`)

**Derive expiry schedule (MEDIUM confidence):** Unlike Deribit which lists fixed weekly Friday and monthly end-of-month expirations, Derive supports "any expiry and strike" provided an oracle data feed exists. In practice, liquid expiries on Derive cluster around the same dates as Deribit (weekly Fridays). However, the API may return dozens of thinly-traded custom expiries that should be filtered.

**Reuses:** Existing `DeriveDiscoveryConfig` struct (new), merged into `DiscoveryConfig`.

---

## Anti-Features

Features to explicitly NOT build in v1.5.

### AF-1: Derive On-Chain Settlement Verification

| Anti-Feature | Verify settlement via Derive Chain (OP Stack / Ethereum L2) RPC calls |
|--------------|-----------------------------------------------------------------------|
| Why Requested | "The settlement is on-chain -- we could verify it directly" |
| Why Problematic | Requires an Ethereum RPC endpoint, on-chain data parsing, ABI decoding, and understanding of Derive's settlement contract state. This is orders-of-magnitude more complex than polling the REST API. The REST `public/get_option_settlement_prices` endpoint provides the same settlement prices already verified by the Derive protocol. On-chain verification adds no signal value -- it's redundant infrastructure. |
| Alternative | Use `public/get_option_settlement_prices` REST endpoint -- already provides Block Scholes oracle-verified settlement prices. |

---

### AF-2: Derive Authentication / Session Keys

| Anti-Feature | Implement Derive session key auth for private endpoint access |
|--------------|---------------------------------------------------------------|
| Why Requested | "Private endpoints give more market data" |
| Why Problematic | Derive's authentication uses EIP-712 session keys (on-chain signature scheme, not simple API keys). Implementation requires: wallet key management, EIP-712 signing, session key registration on-chain. All public market data channels (orderbook, ticker, instruments) are unauthenticated. Private endpoints are for order placement and account data -- both out of scope for v1.5 (paper trading only). |
| Alternative | All required data (orderbook, ticker, settlement prices, instrument listing) is available via public endpoints. No auth needed for v1.5. |

---

### AF-3: Derive Perpetuals Feed

| Anti-Feature | Subscribe to Derive perpetual futures (BTC-PERP) |
|--------------|--------------------------------------------------|
| Why Requested | "Derive also has perpetuals, could be interesting" |
| Why Problematic | Perpetuals require funding rate modeling, mark price basis tracking, and a different probability extraction approach. The pipeline is purpose-built for binary options pricing. Perpetuals provide no direct comparison to Polymarket binary contracts. |
| Alternative | Subscribe to BTC options only (European calls and puts). The perpetual feed produces no actionable signal for cross-venue binary arbitrage. |

---

### AF-4: Full Derive Instrument Universe

| Anti-Feature | Subscribe to all active BTC options on Derive |
|--------------|-----------------------------------------------|
| Why Requested | "More instruments = more signals" |
| Why Problematic | Derive supports custom expiries and strikes, resulting in potentially hundreds of active option instruments. Subscribing to all of them creates unmanaged memory growth, excessive WebSocket bandwidth, and no additional signal value (instruments without a matching Polymarket question are useless for cross-venue arb). |
| Alternative | Subscribe only to instruments that have been approved in events.toml (same `instruments_tx` watch channel pattern as Deribit). Discovery proposes candidates; human approves before subscription. |

---

### AF-5: Multi-Collateral Accounting

| Anti-Feature | Track and account for Derive's multi-collateral positions (wBTC, stETH, etc.) |
|--------------|-------------------------------------------------------------------------------|
| Why Requested | "Derive uses USDC AND wBTC as collateral -- affects cost basis" |
| Why Problematic | Collateral accounting is a v2 execution concern, not a v1.5 signal generation concern. Paper trading uses normalized probability and USD-denominated spreads. The collateral composition of a potential trade is irrelevant until execution planning. |
| Alternative | Model all Derive spreads as USDC-denominated (linear). BasisRiskCache already handles basis risk adjustments. Collateral-specific cost modeling deferred to v2. |

---

## Feature Dependencies

```
Venue::Derive enum variant
    |
    +--requires--> TS-2: Instrument Name Parser
    |                   |
    |                   +--requires--> TS-7: Discovery (derive_discover())
    |                   |
    |                   +--requires--> TS-5: Message Processor (DeriveProcessor)
    |
    +--requires--> TS-1: WebSocket Client (DeriveClient)
                        |
                        +--requires--> TS-3: Order Book Maintenance (DeriveBook)
                        |                   |
                        |                   +--feeds--> TS-5: Processor -> MarketSnapshot
                        |
                        +--requires--> TS-4: Ticker Feed
                        |                   |
                        |                   +--feeds--> TS-5: Processor -> MarketSnapshot
                        |                   |
                        |                   +--enables--> DIFF-2: Black-76 IV Pipeline
                        |
                        +--requires--> TS-6: Supervisor (DeriveSupervisor)

TS-5 (MarketSnapshot with Venue::Derive)
    |
    +--enables--> DIFF-1: Three-way spread (Deribit/Derive/Polymarket)
    |
    +--requires--> TS-9: Pipeline Wiring (all venue enum match arms)

TS-8: Settlement Checker (DeriveChecker)
    |
    +--independent of feed pipeline (REST polling, separate task)
    +--required for signal validation (paper trade P&L ground truth)

TS-7: Discovery
    |
    +--feeds--> EventRegistry (approved matches drive TS-1 subscriptions via SubscriptionManager)
```

### Dependency Notes

- **Venue::Derive enum variant is the blocker for everything.** Add it first, fix all match arms, then build the feed. Attempting to build the feed before the enum is added creates compile errors across the codebase.

- **TS-2 (instrument name parsing) should be built and tested first** -- it has no dependencies, is pure logic, and is required by both discovery (TS-7) and the processor (TS-5). Tests provide immediate verification.

- **TS-3 and TS-4 can be built in parallel** -- both consume raw messages but do independent state management.

- **TS-5 (processor) is the integration point** -- it depends on TS-2, TS-3, TS-4. Build last among the data-path components.

- **TS-8 (settlement) is independent of the feed pipeline** -- it polls REST endpoints on a schedule. Can be developed in parallel with the WebSocket feed.

- **DIFF-1 (three-way spread) requires no code changes** -- it activates automatically once TS-9 (pipeline wiring) is complete and instruments are approved in events.toml.

- **The existing `build_snapshot()` function does NOT need to change** -- it is fully generic over `InstrumentBook` and `TickerState`. Derive's processor calls it with Derive-specific inputs but the function signature and logic are identical.

---

## MVP Definition

### Must Have (v1.5 ship criteria)

- [ ] **Venue::Derive enum** -- ripple through all match arms first
- [ ] **TS-2: Instrument name parser** -- foundation for all other work
- [ ] **TS-1: DeriveClient** -- WebSocket connection + subscribe
- [ ] **TS-3: DeriveBook** -- order book maintenance
- [ ] **TS-4: Ticker feed** -- bid_iv, ask_iv, index_price
- [ ] **TS-5: DeriveProcessor** -- normalization to MarketSnapshot
- [ ] **TS-6: DeriveSupervisor** -- reconnection with backoff
- [ ] **TS-7: discover_derive()** -- REST discovery for instrument proposals
- [ ] **TS-8: DeriveChecker** -- settlement outcome tracking
- [ ] **TS-9: Pipeline wiring** -- plugged into main + SubscriptionManager + SpreadEngine

### Included With No Additional Cost

- [ ] **DIFF-1: Three-way spread** -- activates automatically from TS-9 + event approvals
- [ ] **DIFF-2: Black-76 IV probability** -- activates automatically from TS-4 + existing PricingEngine

### Add After Validation

- [ ] **DIFF-3: Discovery config tuning** -- after first discovery run reveals signal-to-noise ratio

---

## Feature Prioritization Matrix

| Feature | Value | Cost | Priority |
|---------|-------|------|----------|
| Venue::Derive enum | HIGH | LOW | P1 |
| TS-2: Instrument name parser | HIGH | LOW | P1 |
| TS-1: WebSocket client | HIGH | LOW | P1 |
| TS-3: Order book maintenance | HIGH | LOW-MEDIUM | P1 |
| TS-4: Ticker feed (bid_iv/ask_iv) | HIGH | LOW-MEDIUM | P1 |
| TS-5: Message processor | HIGH | MEDIUM | P1 |
| TS-6: Reconnection supervisor | HIGH | LOW | P1 |
| TS-7: REST discovery | HIGH | MEDIUM | P1 |
| TS-8: Settlement checker | HIGH | MEDIUM | P1 |
| TS-9: Pipeline wiring | HIGH | MEDIUM | P1 |
| DIFF-1: Three-way spread | HIGH | LOW (free) | P1 |
| DIFF-2: IV probability | HIGH | LOW (free) | P1 |
| DIFF-3: Discovery config | MEDIUM | LOW | P2 |
| AF-1: On-chain verification | NONE | VERY HIGH | OUT |
| AF-2: Auth/session keys | NONE | HIGH | OUT |
| AF-3: Perpetuals feed | LOW | MEDIUM | OUT |
| AF-4: Full instrument universe | NEGATIVE | MEDIUM | OUT |
| AF-5: Multi-collateral accounting | NONE | MEDIUM | OUT |

---

## Key API Facts Summary

| Fact | Value | Confidence |
|------|-------|------------|
| WS URL (production) | `wss://api.lyra.finance/ws` | MEDIUM |
| WS URL (testnet) | `wss://api-demo.lyra.finance/ws` | HIGH |
| REST URL (production) | `https://api.lyra.finance` | MEDIUM |
| Protocol | JSON-RPC 2.0 | HIGH |
| Auth required for market data | No (public endpoints) | HIGH |
| Instrument format | `{ASSET}-{YYYYMMDD}-{STRIKE}-{C/P}` | HIGH |
| Settlement time | 08:00 UTC | HIGH |
| Settlement price | 30-min TWAP of BTC/USD | HIGH |
| Settlement currency | USDC (linear/USDC-margined) | HIGH |
| Option style | European | HIGH |
| Rate limit window | 5-second fixed window | MEDIUM |
| Rate limit numeric value | Unknown -- needs live verification | LOW |
| WebSocket channel names | Unknown -- needs live API testing | LOW |
| Orderbook update type | Snapshot vs delta -- needs verification | LOW |
| `bid_iv` / `ask_iv` in ticker | Present (confirmed from Get Ticker docs) | MEDIUM |
| `index_price` in ticker | Present | MEDIUM |

---

## Low-Confidence Items Requiring Live API Verification

The following must be verified against the live API before or during implementation:

1. **Exact WebSocket channel name format** -- is it `{instrument_name}.orderbook` or `orderbook.{instrument_name}` or something else? Check `docs.derive.xyz/reference/json-rpc` Subscribe section.

2. **Order book update model** -- are updates full snapshots (like Deribit's grouped channel) or incremental deltas requiring sequence tracking?

3. **Heartbeat mechanism** -- does Derive send periodic keepalive messages? Does it have a server-side heartbeat command equivalent to Deribit's `public/set_heartbeat`? Or is it purely WebSocket-level ping/pong?

4. **Discovery endpoint field names** -- exact JSON field names for the instrument listing response (`expiry_timestamp` vs `expiry_date` vs `expiration_timestamp`, `option_type` vs `instrument_type`, etc.)

5. **Rate limit numbers** -- requests per 5-second window for public endpoints.

6. **Production REST/WS hostname** -- confirm `api.lyra.finance` vs any `api.derive.xyz` hostname.

**Recommended approach:** Write a minimal test harness (a few lines of Rust or even a curl command) that connects to `wss://api-demo.lyra.finance/ws` and sends a subscribe request before implementing the full client. Capture 10-20 raw messages to establish the exact format.

---

## Sources

- [Derive.xyz API Introduction](https://docs.derive.xyz/reference/overview) -- transport-agnostic JSON-RPC protocol, WebSocket URL, public/subscribe method
- [Derive.xyz JSON-RPC Reference](https://docs.derive.xyz/reference/json-rpc) -- method listing, subscribe channel documentation
- [Derive.xyz Get Instrument](https://docs.derive.xyz/reference/post_public-get-instrument) -- single instrument endpoint, instrument_name field
- [Derive.xyz Get Ticker](https://docs.derive.xyz/reference/public-get_ticker) -- best bid/ask, instrument constraints, fees info
- [Derive.xyz Get Option Settlement Prices](https://docs.derive.xyz/reference/public-get_option_settlement_prices) -- settlement prices by expiry
- [Derive.xyz Rate Limits](https://docs.derive.xyz/reference/rate-limits) -- fixed-window 5-second algorithm
- [Derive.xyz Supported Products](https://docs.derive.xyz/docs/supported-products-1) -- options, perpetuals, USDC settlement, multi-asset collateral
- [Lyra Help Center: Expiration and Settlement](https://help.lyra.finance/en/articles/8691491-expiration-settlement) -- 08:00 UTC expiry, 30-min TWAP, BTC/USD settlement
- [A Technical Overview of Derive](https://insights.derive.xyz/a-technical-overview-of-lyra-v2/) -- Rust-powered offchain orderbook, OP Stack L2, matching service
- [Announcing Crypto Options Derive (Lyra v2)](https://insights.derive.xyz/announcing-lyra-v2/) -- architecture overview, USDC settlement confirmation
- [CCXT derive.py](https://github.com/ccxt/ccxt/blob/master/python/ccxt/derive.py) -- confirmed instrument_name format (`BTC-YYYYMMDD-STRIKE-C/P`), `publicPostGetTicker()` method, response field mapping
- [derivexyz/cockpit](https://github.com/derivexyz/cockpit) -- Automated market maker algorithms for Lyra V2 markets
- [Derive.xyz by QuickNode](https://www.quicknode.com/builders-guide/tools/derive-xyz-by-lyra-technologies) -- Rust-powered orderbook, WebSocket streaming, REST and WebSocket API client libraries
- [Key Features of Lyra V2 (Amberdata)](https://blog.amberdata.io/ad-derivatives-insights-lyra-v2-protocol) -- European-style options, USDC settlement, TWAP mechanics

---
*Feature research for: v1.5 Derive.xyz Venue Integration*
*Researched: 2026-03-03*
