# Project Research Summary

**Project:** v1.5 Derive.xyz Venue Integration
**Domain:** Cross-venue options arbitrage — DeFi venue feed addition (Rust, multi-venue, real-time)
**Researched:** 2026-03-03
**Confidence:** MEDIUM-HIGH (architecture HIGH from direct codebase analysis; API specifics MEDIUM/LOW pending live verification)

## Executive Summary

v1.5 adds Derive.xyz (formerly Lyra v2) as a fourth venue to an existing, production-validated cross-venue options arbitrage system. Derive is a decentralized CLOB options exchange running on an Ethereum OP Stack L2, with a JSON-RPC WebSocket API structurally similar to Deribit. The integration is architecturally additive: no changes to downstream engines, no changes to the MarketSnapshot schema, no changes to existing venue supervisors. Every new component mirrors an existing one, with six new source files modeled directly on `src/feed/deribit/`. The primary value delivered is a **Deribit vs Derive options spread** — a direct options-vs-options arbitrage signal uncontaminated by prediction market basis risk — plus a three-way spread (Deribit vs Derive vs Polymarket) that activates automatically once the feed is wired in.

The recommended approach is a strict copy-and-adapt of the Deribit feed stack, with two critical deviations: (1) instrument name parsing must independently handle Derive's `YYYYMMDD` date format (`BTC-20250627-100000-C`) vs Deribit's `DDMMMYY` format (`BTC-27JUN25-100000-C`) — the formats look similar but require completely separate parsers; and (2) Derive prices are USDC-denominated (linear/cash-settled contracts) while Deribit prices are BTC-denominated (inverse contracts), requiring normalization before probability extraction. One new Cargo dependency is required — `k256 = "0.13"` for secp256k1 ECDSA signing — though pre-implementation live API testing should first confirm whether `public/login` authentication is actually needed for read-only orderbook data, as it may not be required.

The primary risks are: a price denomination bug producing phantom arbitrage signals (high severity, detectable by comparing Derive vs Deribit implied probabilities for the same instrument), an instrument name format mismatch causing zero cross-venue candidates (medium severity, caught by a unit test before any wiring), and the book update model assumption — Derive may send incremental deltas rather than full snapshots, which would silently corrupt the book if the wrong model is used. All three risks are caught early if the implementation begins with live API inspection of the testnet feed before committing to any implementation details.

## Key Findings

### Recommended Stack

The existing v1.0–v1.4 validated stack is almost entirely sufficient. The only new dependency is `k256 = { version = "0.13", default-features = false, features = ["ecdsa", "std"] }` for Ethereum wallet-based session signing. All WebSocket, JSON-RPC, rate limiting, reconnection, decimal arithmetic, and recording needs are covered by existing crates. The `sha3` crate (Keccak256) may also be needed if `k256`'s ecdsa feature does not re-export it — verify at implementation time before adding.

**Core technologies:**
- `k256 = "0.13"` (new): secp256k1 ECDSA signing for Derive `public/login` — pure Rust, RustCrypto family, NCC Group audited, no C FFI; may be deferred if live testing shows public channels work unauthenticated
- `tokio-tungstenite` (existing): WebSocket to `wss://api.lyra.finance/ws` — same `connect_async` + `split()` read loop pattern as Deribit
- `reqwest` (existing): REST calls to `POST /public/get_instruments` for discovery — already used for Polymarket Gamma API and Kalshi
- `serde_json` (existing): JSON-RPC 2.0 message construction and parsing — same `json!{}` macro pattern as Deribit client
- `rust_decimal` (existing): All price and size fields — normalize USDC prices to BTC-equivalent using Decimal arithmetic before IV solver

**What NOT to add:** `alloy` / `alloy-signer` (50+ transitive deps for one `personal_sign` call), `ethers-rs` (deprecated), `secp256k1 0.29` (requires C build step), any Derive-specific SDK (none exists as a library crate), `tokio-websockets` (would create a second WS library alongside `tokio-tungstenite`).

### Expected Features

All features required for a functional fourth venue are table stakes — there are no differentiators that require extra work. The most valuable outcomes (three-way spread and Black-76 implied probability) activate automatically from the base integration.

**Must have (table stakes — all P1):**
- `Venue::Derive` enum variant — hard blocker for everything; add first and fix all match arms completely, no `todo!()` shortcuts
- Instrument name parser (`parse_derive_instrument_name`) — `YYYYMMDD` format only, entirely independent of Deribit parser
- DeriveClient — WebSocket connect + `public/subscribe` + forward `RawMessage` frames
- DeriveBook — order book state (snapshot or incremental delta model; verify from live API before writing any code)
- Ticker feed — `bid_iv`, `ask_iv`, `index_price`, `mark_price` extraction
- DeriveProcessor — normalization to `MarketSnapshot { venue: Venue::Derive }` with USDC price normalization
- DeriveSupervisor — exponential backoff reconnection watching `watch::Receiver<Vec<String>>`
- `discover_derive()` — REST-based instrument discovery proposing BTC options for human approval via events.toml
- DeriveChecker — settlement tracking via `POST /public/get_option_settlement_prices` (08:00 UTC, 30-min TWAP, USDC payout)
- Pipeline wiring — Derive block in `run_live_multi_venue()`, SubscriptionManager extended, EventRegistry extended

**Included at no additional code cost (activates automatically once pipeline wired):**
- Three-way spread (Deribit vs Derive vs Polymarket) — SpreadEngine already handles multi-venue by event_id; no code change needed
- Black-76 implied probability for Derive — PricingEngine already handles European-style BTC options with bid_iv/ask_iv populated

**Defer to v2+:**
- On-chain settlement verification via Ethereum RPC — redundant given REST `get_option_settlement_prices` endpoint
- Session key authentication for private endpoints — execution is out of scope for v1.5; read-only only
- Perpetuals feed (BTC-PERP) — incompatible with binary probability extraction pipeline
- Multi-collateral accounting (wBTC, stETH collateral) — irrelevant until execution planning

### Architecture Approach

The integration is purely additive. Six new source files in `src/feed/derive/` mirror `src/feed/deribit/` exactly. Five existing files require targeted extensions: `src/types/venue.rs` (add `Venue::Derive` variant), `src/config/events.rs` (add `DeriveMapping`, add `Option<DeriveMapping>` to `EventVenues`), `src/config/venues.rs` (add `DeriveConfig`), `src/events/registry.rs` (one new `if let Some` block in `build_indexes()`), and `src/subscription/manager.rs` (add `derive_tx`, `current_derive`). All downstream engines — SpreadEngine, OptionsEngine, SignalEngine, PaperTradeTracker, AlertManager, PrometheusExporter — are completely unchanged. They operate on `MarketSnapshot` and `EventId` abstractions that are venue-agnostic.

**Major new components:**
1. `src/feed/derive/client.rs` — WebSocket connect, subscribe JSON-RPC, forward `RawMessage`; no Deribit-style heartbeat protocol needed (standard WS ping/pong)
2. `src/feed/derive/supervisor.rs` — reconnection loop watching `watch::Receiver<Vec<String>>`; verbatim copy of DeribitSupervisor with type changes
3. `src/feed/derive/normalize.rs` — DeriveProcessor: parses wire format, maintains book/ticker state, emits `MarketSnapshot`; includes USDC-to-BTC price normalization
4. `src/feed/derive/messages.rs` — Derive-specific serde deserialization types (from live API capture)
5. `src/feed/derive/book.rs` — DeriveBook order book state; snapshot-only or snapshot+delta depending on live API behavior
6. `src/feed/derive/auth.rs` — `sign_derive_login()` using k256 secp256k1 + Ethereum `personal_sign`; only needed if auth confirmed required

**Key architectural decision:** Derive instrument names (`BTC-20250627-100000-C`) map to the same `event_id` as Deribit names (`BTC-27JUN25-100000-C`) via the EventRegistry. No name translation occurs in the processor — each venue retains its native format, and `build_indexes()` maps each independently to the shared `event_id`. The existing `build_snapshot()` function in Deribit's normalize.rs requires zero changes; DeriveProcessor calls it with Derive-specific inputs.

### Critical Pitfalls

1. **Price denomination mismatch (Derive USDC vs Deribit BTC)** — Derive options are USDC-denominated (linear, cash-settled in USDC). Deribit options are BTC-denominated (inverse). Without normalization, the probability extractor receives `550` (USDC premium) and interprets it as a BTC fraction, producing implied probabilities near 1.0 and spurious spread signals. Fix: divide USDC premium by BTC/USD index price (available from the same ticker message) before passing to the IV solver. Gate: Derive implied probability must be within 5% of Deribit's for the same instrument before proceeding to pipeline wiring.

2. **Instrument name format mismatch** — `YYYYMMDD` (Derive) vs `DDMMMYY` (Deribit). A shared parser or a copy-paste error produces wrong expiry dates, causing zero cross-venue candidates. Fix: implement `parse_derive_instrument_name()` independently using `NaiveDate::parse_from_str(s, "%Y%m%d")`. Unit test that each parser rejects the other's format. This is Phase 1, task 2.

3. **Book update model assumption** — Derive may send incremental deltas after an initial snapshot rather than full snapshots on every update. Applying deltas as snapshots silently corrupts the book after the first message. Fix: capture 20+ messages from `wss://api-demo.lyra.finance/ws` before writing any book code. Implement `apply_snapshot()` and `apply_delta()` as separate methods if delta updates are confirmed.

4. **`Venue::Derive` match arm `todo!()` shortcuts** — Adding the enum variant triggers compiler exhaustiveness errors across all `match venue` sites (metrics, health, settlement, subscription, recording). The temptation is to patch with `todo!()` and return later. Fix: resolve ALL match arms completely in Phase 1 before writing any feed logic. Run `cargo check 2>&1 | grep -i "todo\|unreachable\|unimplemented"` to verify zero placeholders remain.

5. **Non-Friday Derive expiry dates causing false validation warnings** — Derive supports arbitrary expiry dates; Deribit only exposes weekly Fridays and monthly end-of-month. Any "must be Friday" validation produces false positives for legitimate Derive instruments. Fix: remove Friday-only assertions from Derive discovery code. Use exact date matching (0-day tolerance) for initial cross-venue candidate matching; only relax if real data shows alignment requires it.

## Implications for Roadmap

The integration divides cleanly into two phases: foundation work (types, live API verification, core feed components) followed by integration and validation work (pipeline wiring, discovery, settlement, correctness validation). The architectural analysis is high-confidence because it is based on direct codebase inspection. The API specifics require live verification against the testnet before committing to implementation details — this is not optional research overhead, it resolves four LOW-confidence questions in under 30 minutes.

### Phase 1: Foundation — Type Extension, API Verification, Core Feed

**Rationale:** `Venue::Derive` is a hard blocker — the compiler cannot progress until the enum is added and all match arms resolved without placeholders. Live API inspection of the testnet (`wss://api-demo.lyra.finance/ws`) must precede implementation to determine: exact channel names, book update model (snapshot vs delta), heartbeat mechanism, and whether `public/login` is required. The core feed components (messages, book, client, supervisor, processor) are independently testable before any pipeline wiring.

**Delivers:** A working Derive feed emitting `MarketSnapshot { venue: Venue::Derive }` with correct USDC price normalization — testable standalone without touching pipeline.

**Addresses:** Venue::Derive enum, TS-2 (instrument name parser), TS-1 (DeriveClient), TS-3 (DeriveBook), TS-4 (ticker feed), TS-5 (DeriveProcessor), TS-6 (DeriveSupervisor)

**Avoids:** Pitfall 3 (Venue::Derive todo!() shortcuts — resolves all match arms before any logic), Pitfall 1 (instrument name mismatch — independent parser with unit tests), Pitfall 7 (wrong book update model — live API first), Pitfall 5 (auth/no-auth — test unauthenticated before adding k256)

**Build order within phase:**
1. Add `Venue::Derive` to enum; fix all match arm exhaustiveness errors; `cargo check` to zero placeholders
2. Live API session: connect to `wss://api-demo.lyra.finance/ws`, capture 20+ messages; resolve channel names, book model, heartbeat, auth requirement
3. `src/feed/derive/messages.rs` — serde types from captured live messages
4. `src/feed/derive/book.rs` — DeriveBook (snapshot-only or snapshot+delta per verified model)
5. `src/feed/derive/client.rs` + `supervisor.rs` — connect, subscribe, reconnect; add `auth.rs` only if auth confirmed needed
6. `src/feed/derive/normalize.rs` — DeriveProcessor with USDC-to-BTC normalization; IV source detection metric

### Phase 2: Pipeline Integration, Discovery, and Correctness Validation

**Rationale:** Pipeline wiring, SubscriptionManager extension, EventRegistry changes, and discovery integration all depend on Phase 1 types and feed components. This phase wires everything into `run_live_multi_venue()` and validates that cross-venue signals are numerically correct. Settlement tracking (DeriveChecker) can be developed in parallel with pipeline wiring since it uses only REST endpoints.

**Delivers:** Derive instruments feeding SpreadEngine for live cross-venue signals; auto-discovery proposing BTC option candidates; settlement outcome tracking enabling paper trade validation; correctness gate (Derive implied prob within 5% of Deribit for same instrument).

**Implements:** TS-7 (discover_derive), TS-8 (DeriveChecker), TS-9 (pipeline wiring), DIFF-1 (three-way spread, automatic), DIFF-2 (Black-76 IV probability, automatic)

**Avoids:** Pitfall 2 (USDC price denomination in probability extraction — validation gate here), Pitfall 4 (expiry date matching — exact-date match for Derive discovery), Pitfall 6 (L2 sequencer silence — heartbeat/staleness thresholds configured), Pitfall 8 (IV source tracking — `derive_iv_source` metric)

**Validation gate before declaring complete:** Connect to live Derive feed, approve one BTC option in events.toml, verify that the Derive implied probability and Deribit implied probability for the same strike/expiry are within 5% of each other. This is the definitive end-to-end correctness test.

### Phase 3: Hardening and Post-Soak Tuning

**Rationale:** After initial soak test data is collected, tune discovery filters, heartbeat thresholds, and rate limits based on observed real-world behavior. Add discovery config filtering if initial discovery reveals excessive thinly-traded instrument proposals.

**Delivers:** Production-stable Derive feed with tuned staleness thresholds; discovery filtered to liquid instruments; Prometheus gauge `derive_reconnect_rate` for sequencer downtime detection.

**Addresses:** DIFF-3 (discovery config tuning), Pitfall 6 (sequencer downtime metric), rate limit configuration from actual observed behavior

### Phase Ordering Rationale

- `Venue::Derive` must come first because it cascades compiler errors across the entire codebase — no other work can proceed until this compiles cleanly with no placeholders
- Live API inspection before any implementation — channel names, book model, and auth requirement are all LOW confidence without live testing; building on assumptions risks discarding 2–3 days of work
- Core feed validated standalone before pipeline wiring — an incorrect DeriveProcessor wired into the full pipeline is substantially harder to debug than the same bug in an isolated unit test
- Settlement tracking (DeriveChecker) is independent of the WebSocket feed pipeline (REST polling, separate task) and can be developed in parallel with Phase 2 pipeline wiring
- Three-way spread and Black-76 IV probability require zero additional code — they activate automatically once the feed is wired and instruments are approved in events.toml

### Research Flags

Phases requiring live API verification before implementation:

- **Phase 1 (Live API Inspection — mandatory):** Channel subscription format, book update model (snapshot vs delta), heartbeat mechanism, and whether `public/login` is required for public channels are all LOW confidence from documentation search alone. These must be resolved by connecting to `wss://api-demo.lyra.finance/ws` and capturing real messages before writing any integration code. A 30-minute session resolves all four questions.

- **Phase 2 (Price Normalization Correctness — mandatory gate):** The USDC-to-BTC normalization path is new logic with no precedent in the existing codebase. A targeted integration test comparing Derive vs Deribit implied probabilities for the same instrument must pass before deployment to the soak environment.

Phases with standard patterns (no additional research needed):

- **Supervisor and reconnection:** Direct copy of `DeribitSupervisor` — identical structural pattern, no unknowns
- **Registry and SubscriptionManager extension:** Purely additive, identical pattern already established for Deribit, Polymarket, and Kalshi
- **Discovery pipeline:** `discover_derive()` mirrors `discover_deribit()` — same `DiscoveredInstrument` + `FuzzyMatchKey` return types and flow

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | One new dep (k256 0.13); all else covered by existing validated stack. k256 may not be needed if public channels work unauthenticated — test first. |
| Features | MEDIUM | All P1 features are clearly defined. Channel names (LOW), book update model (LOW), rate limits (LOW), and exact ticker field names in WebSocket messages (MEDIUM) need live API verification before committing to implementation. |
| Architecture | HIGH | Based on direct codebase analysis of 36,507 LOC. Additive integration pattern is unambiguous: 6 new files, 5 modified files, 0 downstream changes. Build order validated against dependency graph. |
| Pitfalls | HIGH | 8 pitfalls identified with concrete prevention strategies, recovery costs, and phase assignments. Price denomination and format mismatch pitfalls verified against codebase structure. Sequencer downtime pitfall verified against OP Stack L2BEAT data. |

**Overall confidence:** MEDIUM-HIGH

### Gaps to Address

- **WebSocket channel name format** — Inferred as `orderbook.{instrument_name}` and `ticker.{instrument_name}` but not confirmed from official docs. Resolve by connecting to `wss://api-demo.lyra.finance/ws` and sending a subscribe request before implementation. Do not write channel name constants until verified.

- **Book update model** — Unknown whether Derive sends full snapshots per update or incremental delta updates after initial snapshot. This determines whether `DeriveBook` needs delta processing. Resolve by capturing 20+ sequential messages from the testnet feed. Critical architectural decision — do not defer.

- **Authentication requirement for public market data** — `public/login` may or may not be required for read-only orderbook subscriptions. If not required, the `k256` dependency and auth module are deferred to v2. Resolve by attempting an unauthenticated subscribe before adding any auth code.

- **Exact rate limit numbers** — Estimated as ~10 req/5s from partial docs page access; not confirmed. Visit `docs.derive.xyz/reference/rate-limits` directly before configuring `VenueRateLimiter`. Setting too low wastes discovery throughput; setting too high risks temporary IP bans.

- **Ticker field names in WebSocket messages** — `bid_iv`, `ask_iv`, `mark_iv`, `index_price` confirmed present from REST ticker docs but not verified in live WebSocket ticker notification format. Verify field names match during live API inspection session.

## Sources

### Primary (HIGH confidence)
- `docs.derive.xyz/reference/overview` — WebSocket URL, JSON-RPC protocol, transport-agnostic confirmation
- `docs.derive.xyz/reference/json-rpc` — method naming, subscribe pattern
- `docs.derive.xyz/reference/post_public-get-instrument` — instrument schema, field names
- `docs.derive.xyz/reference/post_public-login` — endpoint exists, WebSocket-only, wallet/timestamp/signature params
- `help.lyra.finance/en/articles/8691491-expiration-settlement` — 08:00 UTC expiry, 30-min TWAP, USDC settlement confirmed
- Direct codebase analysis: `src/feed/deribit/` (supervisor 206 LOC, client 311 LOC, normalize 1076 LOC, messages 653 LOC), `src/types/venue.rs`, `src/subscription/manager.rs`, `src/events/registry.rs`, `src/events/discovery.rs`, `src/types/snapshot.rs` — architecture patterns, MarketSnapshot schema, all venue fields
- `crates.io/crates/k256` — version 0.13.4 stable, ecdsa feature confirmed, Ethereum signing

### Secondary (MEDIUM confidence)
- CCXT `derive.py` — instrument name format `{ASSET}-{YYYYMMDD}-{STRIKE}-{C/P}` confirmed; `publicPostGetTicker()` method, response field mapping
- Hummingbot Derive connector — wallet_address, private_key credential structure; `personal_sign` authentication approach
- `docs.derive.xyz/reference/rate-limits` — fixed-window 5s algorithm confirmed; specific request counts not captured (page partially inaccessible)
- `docs.derive.xyz/reference/public-get_ticker` — bid_iv, ask_iv, mark_iv, index_price confirmed present in REST response
- `github.com/derivexyz/cockpit` — official Rust market-maker reference; instrument name format confirmed in CLI context; confirms Rust integration is viable
- `insights.derive.xyz/a-technical-overview-of-lyra-v2/` — Rust-powered offchain CLOB, OP Stack L2, USDC settlement architecture
- Amberdata Derive integration references — European-style options, `{ASSET}-{YYYYMMDD}-{STRIKE}-{C/P}` naming confirmed

### Tertiary (LOW confidence)
- Channel subscription format — inferred from JSON-RPC docs overview and Deribit pattern similarity; requires live API verification before use
- Book update model (snapshot vs delta) — inferred from architecture documentation describing "complete snapshot model"; requires live message capture to confirm
- Rate limit numeric values — "10 req/s" estimated from search result snippets; requires direct docs page visit before configuring VenueRateLimiter

---
*Research completed: 2026-03-03*
*Ready for roadmap: yes*
