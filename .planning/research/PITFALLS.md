# Domain Pitfalls

**Domain:** Adding Derive.xyz (DeFi options, Ethereum L2) as a fourth venue to an existing multi-venue cross-venue arbitrage system (v1.5)
**Researched:** 2026-03-03
**Confidence:** HIGH (instrument naming, settlement mechanics, date format differences, Venue enum impact — verified against codebase), MEDIUM (WebSocket channel names, rate limit specifics — Derive API docs not fully accessible during research), LOW (L2 sequencer reliability specifics — general OP Stack patterns applied)

---

## Critical Pitfalls

Mistakes that cause rewrites, phantom signals, data corruption, or incorrect probability extraction.

### Pitfall 1: Instrument Name Format Mismatch Between Derive and Deribit

**What goes wrong:**
Derive uses `YYYYMMDD` date encoding in instrument names (e.g., `BTC-20250627-100000-C`), while Deribit uses `DDMMMYY` format (e.g., `BTC-27JUN25-100000-C`). The existing `FuzzyMatchKey` and `DiscoveredInstrument` normalizer for Deribit parses `DDMMMYY` format. When Derive instruments are added, the discovery and matching code must independently parse `YYYYMMDD` — and any function that assumes a single parse strategy will silently produce wrong expiry dates.

Specifically: `27JUN25` and `20250627` represent the same expiry, but if the Derive parser is accidentally given Deribit format strings (or vice versa), the date parse will either fail or produce a plausible-but-wrong date (e.g., `271025` parsed as YYYYMMDD gives October 25, 0027 — Rust's `NaiveDate` will reject this, but a panic or fallback could produce a wrong date). The cross-venue matching system uses `NaiveDate` as the canonical expiry key; a wrong date produces zero matches.

**Why it happens:**
The Deribit parser is already working and the developer copies it to build the Derive parser. The two formats look similar enough that the difference is easy to miss. The existing `channels.rs` and `discover_deribit()` code in `discovery.rs` has the Deribit format deeply embedded.

**How to avoid:**
1. Create a `parse_derive_instrument_name(s: &str) -> Option<(asset, expiry: NaiveDate, strike: Decimal, direction)>` function that exclusively handles `YYYYMMDD` format using `NaiveDate::parse_from_str(date_str, "%Y%m%d")`.
2. Keep the Deribit parser (`parse_deribit_instrument_name`) entirely separate — no shared code path.
3. Add a unit test that verifies `BTC-20250627-100000-C` and `BTC-27JUN25-100000-C` both parse to expiry `2025-06-27`.
4. Add a cross-venue matching integration test: feed one Derive instrument and one Deribit instrument with the same underlying details, verify a match is produced.

**Warning signs:**
- Discovery runs but produces zero cross-venue candidates between Derive and Deribit despite both having BTC options for the same expiry.
- `DiscoveredInstrument.expiry` values for Derive instruments have year before ~2020 (misparse of `DDMMMYY` as `YYYYMMDD`).
- Match attempts fail silently with "no candidates" despite clear visual overlap in BTC option chains.

**Phase to address:** Phase 1 (Derive feed and discovery) — the instrument name parser must be correct before any matching or subscription logic is built.

---

### Pitfall 2: Price Denomination Difference — Derive Prices Are in USDC, Deribit Prices Are in BTC

**What goes wrong:**
Deribit BTC options are quoted in BTC per contract (inverse contracts). A price of `0.0055` means 0.0055 BTC. The existing Black-76 pipeline and probability extractor in `src/pricing/` operates on these BTC-denominated prices and converts them to probability space using the underlying BTC/USD price.

Derive BTC options are quoted in USDC per contract (linear/cash-settled). A price of `$550` means 550 USDC. If the Derive processor feeds raw USDC prices into the same probability extractor without the denomination flag, the extractor will attempt to interpret `550` as a BTC-denominated premium. With spot BTC at ~$100,000, interpreting `550` as a BTC fraction gives `550 / 100000 = 0.55` — an absurdly high probability that produces spurious signals.

**Why it happens:**
The Deribit normalization pipeline uses BTC-denomination throughout, and the existing `MarketSnapshot.bid/ask` fields carry BTC-denominated values. When a Derive processor slot is added by copying the Deribit processor, the denomination difference is not immediately obvious — the bid/ask values are just numbers.

**How to avoid:**
1. Add a `price_denomination` metadata field to `MarketSnapshot` or document a convention: Derive prices must be normalized to BTC-denominated values before entering the snapshot (divide USDC price by the BTC/USD index price at snapshot time).
2. Alternatively, add `quote_currency: QuoteCurrency` to `MarketSnapshot` and update the probability extractor to handle both `Btc` and `Usd` denominations.
3. The Derive processor must track the BTC/USD index price (available from the same feed) and normalize on every snapshot update.
4. Add a sanity check: probability extraction should reject any implied probability > 0.99 or < 0.01 for normal market conditions. A USDC price accidentally treated as BTC-denominated will often exceed 1.0 before the extractor can catch it.

**Warning signs:**
- Implied probabilities from Derive instruments are consistently near 1.0 or near 0.0 across all strikes.
- Cross-venue spreads (Derive implied prob vs. Polymarket implied prob) are consistently extreme (e.g., +0.80 spread on every instrument).
- The probability extractor logs "option price exceeds intrinsic value" or IV solver fails to converge on Derive instruments.

**Phase to address:** Phase 1 (normalization layer) — must be resolved before any probability extraction is attempted. Do not proceed to Phase 2 until a single Derive instrument's probability matches Deribit's implied probability for the same instrument within a reasonable spread.

---

### Pitfall 3: Missing `Venue::Derive` in the Venue Enum Breaks Compilation Everywhere

**What goes wrong:**
The `Venue` enum in `src/types/venue.rs` currently has three variants: `Deribit`, `Polymarket`, `Kalshi`. Adding `Derive` as a fourth variant will cause compile errors at every `match` statement that does not handle the new arm. This is desirable — the compiler forces exhaustive coverage. However, the risk is that a developer adds `Venue::Derive` and then hurriedly patches each `match` arm with a `todo!()` or `unreachable!()` placeholder to make the code compile, then forgets to implement the real logic.

The affected `match` sites are pervasive: `Display::fmt`, `env_prefix()`, the settlement checker's `VenueChecker`, the TOML writer's `CandidateVenues`, the subscription manager's per-venue diff logic, metric labels (`"venue" => "deribit"`), and every place that branches on venue to invoke venue-specific code.

**Why it happens:**
Adding an enum variant to `Venue` is a single-line change that compiles fine in isolation. The compile errors at all other sites appear immediately, and the temptation is to patch them minimally to get compilation, then return later. "Later" often does not come.

**How to avoid:**
1. Add `Venue::Derive` to the enum and then resolve ALL compilation errors fully and correctly — not with `todo!()`. Do this in one focused phase.
2. The `env_prefix()` method should return `"DERIVE"` (for future credential loading, even if not needed in v1.5).
3. The `Display::fmt` implementation should return `"derive"` (lowercase, consistent with `"deribit"`, `"polymarket"`, `"kalshi"`).
4. Every Prometheus metric label that emits `"venue" => "deribit"` in Deribit code should have a corresponding `"venue" => "derive"` in Derive code.
5. Settlement checker: `VenueChecker` should get a `Derive` arm that returns a no-op or stub for v1.5 (Derive settlement is on-chain; REST polling may not apply in the same way).

**Warning signs:**
- Compilation succeeds with `todo!()` or `unreachable!()` in match arms.
- Derive instruments produce no Prometheus metrics (label was not added to gauge/counter registrations).
- `VenueChecker` panics at runtime when a Derive instrument hits the settlement path.

**Phase to address:** Phase 1, first task — add `Venue::Derive` and resolve all compilation errors correctly before writing any Derive-specific logic.

---

### Pitfall 4: Assuming Derive Expiry Dates Match Deribit's Friday-Only Schedule

**What goes wrong:**
Deribit BTC options expire only on Fridays at 08:00 UTC. The existing `FuzzyMatchKey` and `DiscoveredInstrument` system was tuned for Deribit's weekly/monthly schedule. Derive supports user-defined expiry dates (any date up to 400 days out) and not just Fridays. This means:

1. The expiry tolerance window in `find_cross_venue_candidates_fuzzy` (originally tuned for +/- a few days to handle Deribit-Kalshi differences) may need adjustment for Derive instruments that could have expiry dates that are mid-week.
2. The existing validation in `ValidatedMapping` that warns when an expiry is not a Friday may produce false-positive warnings for legitimate Derive instruments.
3. A Derive BTC option expiring on a Wednesday does NOT match a Deribit BTC option expiring the following Friday, even if both are near-dated "weekly" options. The cross-venue matcher must not be too aggressive in fuzzy-matching misaligned expiries.

Both Derive and Deribit settle at 08:00 UTC on expiry, so the settlement TIME is the same. Only the expiry DATE can differ.

**Why it happens:**
All prior venues (Deribit, Kalshi) have predictable expiry schedules (Friday or end-of-month). Derive is the first venue where expiry is truly arbitrary. The discovery and matching code implicitly assumes constrained expiry dates.

**How to avoid:**
1. Remove any hard-coded "must be Friday" validation from Derive-specific discovery code.
2. When matching Derive instruments against Deribit instruments for cross-venue candidates, restrict the expiry tolerance to 0 days (exact date match only) for the initial implementation. Only relax this if actual data shows near-date Derive instruments that should match Deribit weekly options.
3. The cross-venue matching proposal output should log the Derive expiry date explicitly so the operator can verify alignment before approval.

**Warning signs:**
- Warnings logged "Derive instrument expiry is not a Friday" for every Derive instrument.
- Cross-venue candidates include Derive/Deribit pairs with different expiry dates (the match key should require equal `NaiveDate`).
- Zero proposals generated because the fuzzy matcher's tolerance window is too narrow for Derive's date format differences.

**Phase to address:** Phase 1 (discovery integration) — define the exact matching rules for Derive before implementing the discovery loop.

---

### Pitfall 5: No-Auth Public Orderbook vs. Auth-Required Private Data — Confusing Public and Private Channels

**What goes wrong:**
Derive.xyz requires wallet-based authentication (Ethereum wallet signature via `X-LyraWallet` header + `X-LyraTimestamp` + `X-LyraSignature`) for private endpoints. Public market data (orderbook, ticker) does NOT require authentication.

The risk in v1.5 is the opposite of what it seems: the code will work correctly for public data without any authentication setup. But if the developer later adds subscription channels that require authentication (e.g., RFQ channels, private fills), accidentally mixing them into the public WebSocket connection will produce silent failures or authentication errors logged as warnings but not treated as fatal.

For v1.5 specifically: the system only needs public data (orderbook, ticker). The entire authentication mechanism (wallet, session keys, signatures) is out of scope. The pitfall is accidentally importing or scaffolding the authentication layer in v1.5 code, creating dead code that confuses future v2 (execution) work.

**How to avoid:**
1. Confirm via the Derive API docs before writing any code: orderbook and ticker subscription channels are public (no auth required).
2. Do not add any credential loading (`DERIVE_WALLET_ADDRESS`, `DERIVE_PRIVATE_KEY`) to the config for v1.5.
3. Add a prominent comment in the Derive client: `// v1.5: Public market data only. Authentication for trading is a v2.0 concern.`
4. The `Venue::Derive.env_prefix()` should return `"DERIVE"` for future use, but the credentials module should not attempt to load Derive credentials in v1.5.

**Warning signs:**
- WebSocket connection to Derive works for public channels but fails with 401 when private channels are accidentally subscribed.
- Config file gains `[venues.derive]` section with credential fields that are never loaded.
- Authentication skeleton code checked in that has no tests and no usage.

**Phase to address:** Phase 1 (client setup) — make the auth/no-auth boundary explicit in code comments before writing the subscription logic.

---

### Pitfall 6: L2 Sequencer Downtime Causes WebSocket Silence, Not Explicit Error

**What goes wrong:**
Derive runs on Derive Chain, an OP Stack L2. When the OP Stack sequencer experiences downtime (rare but documented in OP Stack specifications), the exchange may stop publishing new data to its WebSocket feed. From the client's perspective, the WebSocket connection stays open but no messages arrive — the same symptom as a network partition or a silent exchange.

The existing heartbeat timeout detection in `DeribitClient` (`timeout_duration = heartbeat_interval_ms * 2`) would catch this: if no messages arrive within 2x the heartbeat interval, the connection is assumed dead and the supervisor reconnects. However, Derive's heartbeat protocol may differ from Deribit's. If Derive does not send periodic heartbeats (or uses a different mechanism than `public/set_heartbeat`), the timeout detection relies entirely on the absence of orderbook/ticker data — which could be legitimately absent for low-liquidity instruments during quiet market hours.

**How to avoid:**
1. Verify the Derive heartbeat protocol from their API documentation before implementing the client. If Derive uses standard WebSocket ping/pong (handled automatically by `tokio-tungstenite`), implement a heartbeat deadline based on expected market data cadence, not a protocol heartbeat.
2. Implement a "data silence" detector: if a subscribed Derive instrument produces no updates for `X` seconds during market hours (e.g., 60 seconds), log a warning. This is distinct from the heartbeat timeout.
3. Set a generous but not infinite heartbeat timeout for Derive (e.g., 120 seconds) to distinguish "quiet market" from "sequencer down."
4. Add a Prometheus gauge `derive_sequencer_suspected_down` that fires when the supervisor reconnects more than N times within a rolling window.
5. The `VenueHealth` system already tracks connection status. Make sure Derive instruments do NOT contribute to cross-venue spread calculations when `VenueHealth.is_available() == false` for Derive.

**Warning signs:**
- DerivetSupervisor reconnects repeatedly with short-lived connections (connects, gets no messages, times out, reconnects).
- Cross-venue spreads during Derive downtime use stale Derive data (is_stale flag should prevent this, but verify).
- Alerts fire for "feed silence" on Derive even during known sequencer maintenance windows.

**Phase to address:** Phase 1 (supervisor and heartbeat) — the heartbeat strategy must be determined before production deployment of the feed.

---

### Pitfall 7: Reusing Deribit's Snapshot-Only Book Model When Derive Sends Incremental Updates

**What goes wrong:**
The existing `InstrumentBook` in `src/feed/deribit/book.rs` was designed for Deribit's grouped book channel (`book.{instrument}.none.20.100ms`), which sends complete 20-level snapshots on every message. There is no delta/incremental update processing needed — each message replaces the entire book state.

Derive's orderbook WebSocket feed may use incremental delta updates (a common pattern in DeFi exchange APIs: first message is a full snapshot, subsequent messages are deltas with price levels to add/remove/update). If the Derive book implementation assumes every message is a full snapshot, applying a delta message as a snapshot will corrupt the book — partially clearing levels and replacing them with the small delta set.

Conversely, if Derive does send full snapshots on every update, implementing delta processing is wasted complexity.

**How to avoid:**
1. Before implementing the book module, read a sample of Derive WebSocket feed messages (using a simple WebSocket client script) to determine: does every message include `"type": "snapshot"` or are there `"type": "change"` (delta) messages after initial connection?
2. If Derive uses delta updates: implement a `derive::DeriveBook` that handles both snapshot initialization and delta application, similar to how Deribit's grouped channel handles `change_id` sequences. Do NOT reuse `InstrumentBook` from Deribit without modifications.
3. If Derive uses snapshot-only: reuse the existing snapshot model but rename types clearly (`DeriveBook` wrapping or re-exporting `InstrumentBook` patterns).
4. Add a test that feeds: (1) a full snapshot message, (2) a delta that removes one level and adds another, and verifies the book state after step 2.

**Warning signs:**
- After the initial snapshot, the Derive book has only 1-2 levels instead of the expected depth (delta applied as snapshot cleared the previous levels).
- Logged book state shows bid or ask levels that were not in the most recent received message.
- Sequence gap errors or `prev_change_id` mismatches if the delta tracking is incorrectly implemented.

**Phase to address:** Phase 1 (book implementation) — determine the update model from actual API inspection before writing any book code.

---

### Pitfall 8: IV Data May Not Be Available From the Feed — Forcing Fallback to Order-Implied IV

**What goes wrong:**
The existing Deribit pipeline provides `bid_iv` and `ask_iv` directly from the ticker channel, along with `mark_iv`. The Black-76 probability extractor in `src/pricing/` can use these directly, avoiding the need to solve for IV numerically from raw bid/ask prices.

Derive's oracle-based IV feed (powered by Block Scholes) posts implied volatility data on-chain and via their REST API. However, the real-time WebSocket ticker channel may or may not include bid_iv/ask_iv fields in the format the existing `TickerState` struct expects. If Derive's ticker does not include IV fields, the probability extractor must fall back to solving for IV from the USDC bid/ask prices — which requires the BTC/USD spot price, interest rate, and time-to-expiry to be present and correct.

The fallback path (IV solver from raw prices) already exists in the codebase (Newton-Raphson + Brent in `src/pricing/`). The risk is not absence of the fallback, but rather: (1) the fallback being triggered without the operator knowing, or (2) the IV solver receiving USDC-denominated prices without normalization (see Pitfall 2).

**How to avoid:**
1. When implementing the Derive ticker parser, check whether `mark_iv`, `bid_iv`, `ask_iv` are present in actual Derive ticker WebSocket messages.
2. Add a metric `derive_iv_source` with labels `direct_from_ticker` vs `solver_fallback` to track which path is active.
3. If the solver fallback is used, ensure prices have been normalized from USDC to BTC-denominated (Pitfall 2) before passing to the solver.
4. Log a warning on startup if Derive instruments are producing probability estimates via solver fallback rather than direct IV fields, so the operator is aware.

**Warning signs:**
- Derive implied probabilities have higher variance than Deribit probabilities for similar instruments (solver noise vs. direct IV).
- IV solver convergence warnings in logs for Derive instruments.
- `derive_iv_source = solver_fallback` metric is non-zero but no operator warning was emitted.

**Phase to address:** Phase 2 (probability extraction integration) — the IV source detection must be implemented before the first end-to-end probability calculation.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Copy-paste Deribit client as Derive client | Fast initial connection | Heartbeat protocol differences cause silent connection failures; denominiation bugs propagate | Never — create `derive/client.rs` from scratch, referencing Deribit's as a model but implementing independently |
| Use `f64` for Derive USDC prices before normalization | Avoids intermediate type | f64 imprecision compounds in BTC normalization step; hard to trace precision errors in probability extraction | Never — use `Decimal` throughout, normalize USDC→BTC using `Decimal` arithmetic |
| Use `todo!()` to satisfy Venue::Derive match arms | Code compiles immediately | Runtime panics in settlement checker, subscription manager, or cleanup channels | Only during initial compilation to identify all affected sites; replace within the same phase |
| Skip heartbeat protocol for v1.5 soak test | Fewer lines to implement | Derive connection silently dies under sequencer slowdown; supervisor reconnects unnecessarily | Acceptable only for initial local testing; implement before soak test deployment |
| Hard-code Derive WebSocket URL in config defaults | Simplifies initial setup | Derive may have separate testnet/mainnet URLs; config-driven approach already validated in v1.0 | Never — all URLs must be config-driven (the pattern is already established) |
| Reuse Deribit `InstrumentBook` for Derive without testing delta behavior | Works for snapshot channels | If Derive uses deltas, the book silently corrupts on every update after the first | Never — verify Derive's update model before choosing the book implementation |

---

## Integration Gotchas

Common mistakes when connecting to external services.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Derive WebSocket | Assuming the same JSON-RPC subscribe format as Deribit (`public/subscribe` with `channels` array) | Verify Derive's exact subscription method name and parameter structure from docs/live testing before implementing |
| Derive instrument naming | Parsing `BTC-20250627-100000-C` with the Deribit parser (which expects `BTC-27JUN25-100000-C`) | Implement a separate `parse_derive_instrument_name()` with `%Y%m%d` date format |
| Derive price normalization | Treating USDC-denominated prices as BTC-denominated in the probability extractor | Divide USDC premium by BTC/USD index price to get BTC-equivalent before passing to `extract_probability()` |
| Derive discovery | Calling the Derive REST endpoint without rate limiting (using the shared `VenueRateLimiter`) | Add a `VenueRateLimiter` for Derive discovery, configured separately from the Deribit limiter |
| Derive settlement | Using the existing `VenueChecker` REST polling pattern (designed for Deribit/Kalshi) for Derive on-chain settlement | Derive settlement is on-chain; v1.5 only needs feed data, not settlement tracking — stub the settlement checker for now |
| Derive subscription manager | Not adding Derive to the `SubscriptionManager`'s per-venue watch channel infrastructure | The reconnect-based subscription model from v1.3 applies to Derive; the supervisor must receive `watch::Receiver<Vec<String>>` for instrument updates |
| Derive stale state cleanup | Not adding a Derive cleanup channel to the `CleanupEvent` struct | `CleanupEvent` must get a `derive_instruments: Vec<String>` field; the Derive processor must handle cleanup events |

---

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Subscribing to all Derive BTC option strikes at once | WebSocket message flood; snapshot channel saturated at 1024 messages | Subscribe only to instruments matching approved events; use the same `SubscriptionManager` gating as Deribit | At ~50+ active Derive instruments simultaneously |
| Storing Derive USDC prices as `f64` for normalization | Floating-point drift visible in probability comparisons across soak test | Use `Decimal` for all price arithmetic; only convert to `f64` at the statistics boundary (IV solver input) | Immediately, but only detectable via careful test comparisons |
| Fetching the full Derive instrument list on every discovery poll cycle | REST rate limit hit; discovery takes minutes | Cache the full list and diff against the previous result; only request if last_modified or ETags indicate change | At 100+ Derive active instruments (BTC options list) |
| Using the Deribit `change_id` sequence model for Derive deltas | If Derive uses a different sequence numbering, gap detection misfires constantly | Use Derive-specific sequence field names; verify the sequence semantics before implementing gap detection | On first connection attempt |

---

## Security Mistakes

Domain-specific security issues for DeFi venue integration.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Loading a private key for Derive in v1.5 config | If the key has funds associated, it is an attack surface with no benefit (execution is out of scope) | Do not add any wallet/private key config for v1.5; authentication is a v2.0 concern |
| Logging the full Derive WebSocket URL including any query-string credentials | Credentials in logs if auth tokens are ever added as query params | Use the same log-sanitization pattern as existing code; log only the host, not the full URL |
| Trusting Derive-provided prices without staleness checks | A compromised or lagging Derive feed could produce arbitrarily stale prices that trigger false signals | Apply the same `is_exchange_data_stale()` gate to Derive snapshots as to Deribit snapshots; configure a Derive-specific staleness threshold |

---

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Venue::Derive added:** Check that ALL match arms are fully implemented (not `todo!()`) — run `cargo check 2>&1 | grep -i "todo\|unreachable\|unimplemented"` after adding the variant.
- [ ] **Instrument name parsing:** Verify with a test that `BTC-20250627-100000-C` parses to expiry `2025-06-27` and `BTC-27JUN25-100000-C` (Deribit) is NOT accepted by the Derive parser.
- [ ] **Price normalization:** Verify that a Derive instrument with a known USDC bid/ask produces an implied probability within 5% of the Deribit implied probability for the same expiry and strike.
- [ ] **Subscription manager wiring:** Confirm that when a Derive event is approved in `events.toml`, the DerivesSupervisor receives the updated instrument list within one config reload cycle (without restart).
- [ ] **Cleanup channels:** Confirm that unsubscribing a Derive instrument sends a `CleanupEvent` with `derive_instruments` populated, and the Derive processor evicts the stale book and ticker state.
- [ ] **Stale data gate:** Confirm that `is_stale = true` propagates correctly for Derive snapshots when the exchange timestamp is older than the configured threshold.
- [ ] **Prometheus metrics:** Confirm that `feed_latency_ms`, `feed_messages_total`, `subscription_active` gauges all emit with `venue = "derive"` label (not missing from metric registration code).
- [ ] **Feed recording:** Confirm that Derive raw messages are recorded to JSONL with `Venue::Derive` tag for offline replay.
- [ ] **Discovery produces proposals:** Run the discovery pipeline with a real Derive BTC option and verify an `events.toml` proposal is written with `derive` venue entry.

---

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Instrument name format mismatch (Pitfall 1) | LOW | Fix `parse_derive_instrument_name()`, add tests, re-run discovery — no data corruption, proposals are just empty |
| Price denomination error producing phantom signals (Pitfall 2) | MEDIUM | Fix normalization, delete any spurious paper trades from JSONL, restart service — existing soak test data is corrupted for the affected window |
| `Venue::Derive` match arms with `todo!()` causing runtime panic (Pitfall 3) | LOW | Implement the missing arm, recompile, restart — no persistent data corruption |
| Expiry date mismatch causing zero cross-venue matches (Pitfall 4) | LOW | Fix parsing, re-run discovery, check proposals — no data loss |
| Heartbeat timeout misconfiguration causing constant reconnects (Pitfall 6) | LOW | Adjust heartbeat interval in config, restart — no data loss but soak test window has gaps |
| Delta book model misapplication corrupting order books (Pitfall 7) | MEDIUM | Fix book logic, restart service — order book state is reconstructed from scratch on reconnect; in-memory corruption does not persist |
| IV data source confusion causing wrong probability extraction (Pitfall 8) | MEDIUM | Fix normalization path, restart — paper trades entered during the wrong-probability window must be manually reviewed |

---

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Instrument name format mismatch (Pitfall 1) | Phase 1: Discovery integration | Unit test: both Derive and Deribit parsers reject each other's format; integration test: cross-venue match produced for identical instruments |
| Price denomination error (Pitfall 2) | Phase 1: Normalization layer | End-to-end test: Derive instrument probability within 5% of Deribit probability for same contract |
| Venue::Derive match arms (Pitfall 3) | Phase 1, first task | `cargo check` with no `todo!()` or `unreachable!()` in Venue match sites |
| Expiry date matching rules (Pitfall 4) | Phase 1: Discovery | Discovery run on live API produces correct proposals with matching expiry dates |
| Auth/public channel boundary (Pitfall 5) | Phase 1: Client setup | Code review: no credential loading for Derive in v1.5; WebSocket connects successfully with no auth headers |
| L2 sequencer downtime handling (Pitfall 6) | Phase 2: Supervisor hardening | Manually kill the WebSocket connection and verify supervisor reconnects; silence alert fires correctly |
| Book update model (Pitfall 7) | Phase 1: Book implementation | Test sequence: snapshot + delta produces correct merged book state |
| IV data source (Pitfall 8) | Phase 2: Probability extraction | `derive_iv_source` metric is logged; probability extraction test with known IV |

---

## Sources

- Derive.xyz API documentation overview: [https://docs.derive.xyz/reference/overview](https://docs.derive.xyz/reference/overview)
- Derive.xyz JSON-RPC reference: [https://docs.derive.xyz/reference/json-rpc](https://docs.derive.xyz/reference/json-rpc)
- Derive instrument naming (example `ETH-20231027-1500-P`): [https://docs.derive.xyz/docs/submit-order](https://docs.derive.xyz/docs/submit-order)
- Derive supported products and settlement: [https://docs.derive.xyz/docs/supported-products-1](https://docs.derive.xyz/docs/supported-products-1)
- Derive expiration and settlement (8am UTC, 30-min TWAP): [https://help.derive.xyz/en/articles/8691491-expiration-settlement](https://help.derive.xyz/en/articles/8691491-expiration-settlement)
- Derive session keys and authentication: [https://docs.derive.xyz/reference/session-keys](https://docs.derive.xyz/reference/session-keys)
- Derive fees reference: [https://docs.derive.xyz/reference/fees-1](https://docs.derive.xyz/reference/fees-1)
- Derive technical overview (Block Scholes IV oracle, USDC settlement): [https://insights.derive.xyz/a-technical-overview-of-lyra-v2/](https://insights.derive.xyz/a-technical-overview-of-lyra-v2/)
- OP Stack sequencer outages documentation: [https://docs.optimism.io/stack/rollup/outages](https://docs.optimism.io/stack/rollup/outages)
- Derive chain on L2BEAT (OP Stack, sequencer failure category): [https://l2beat.com/scaling/projects/derive](https://l2beat.com/scaling/projects/derive)
- Codebase analysis: All pattern references cite structures from `src/feed/deribit/`, `src/types/venue.rs`, `src/events/discovery.rs`, `src/subscription/`, and `src/pricing/` as read during research

---

*Pitfalls research for: Derive.xyz DeFi options venue integration into existing multi-venue Rust arbitrage system*
*Researched: 2026-03-03*
