# Project Research Summary

**Project:** prediction
**Domain:** Cross-venue crypto prediction market / options market arbitrage system
**Researched:** 2026-02-21
**Confidence:** HIGH

## Executive Summary

This project is a Rust-based system that detects arbitrage opportunities across three venue types: prediction markets (Polymarket, Kalshi) and crypto options markets (Deribit). The core thesis is novel -- no existing open-source system compares prediction market prices against options-derived binary probabilities. All existing bots (poly-kalshi-arb, etc.) only compare prediction markets to each other. The cross-asset dimension accesses a fundamentally different and deeper source of price discovery: Deribit's $185B/month BTC options volume dwarfs prediction market volumes. The recommended approach is a Rust async pipeline built on tokio, using custom thin clients per venue (not community SDKs), with a channel-based actor architecture that naturally provides backpressure and fault isolation.

The system should launch as a signal generator with paper trading, not as an execution engine. This is a hard architectural decision, not a deferral of convenience. Cross-venue execution introduces leg risk (one fill, other fails), and prediction markets have proven settlement divergence risk (the Cardi B incident where Kalshi and Polymarket resolved the same event differently). Proving signal quality through paper trading with rigorous P&L tracking must precede any capital deployment. The v1 system ingests feeds from all three venues, normalizes orderbooks, maps events across venues, computes fee-adjusted spreads, and logs every signal with hypothetical P&L.

The three critical risks are: (1) conflating risk-neutral probabilities from options with prediction market prices -- the systematic wedge between them is the volatility risk premium, not alpha; (2) settlement basis risk where "the same event" resolves differently across venues; and (3) naive digital option pricing using N(d2) instead of skew-adjusted call spread replication, which introduces 2-5% probability errors that exceed typical arbitrage spreads. All three must be addressed in the pricing and event-mapping layers before signal generation produces meaningful output.

## Key Findings

### Recommended Stack

The stack is pure Rust with tokio as the async runtime. All exchange clients should be custom-built (each is ~200 lines) rather than using community SDKs that are outdated, pull unnecessary dependencies, and lag behind API changes. TLS is handled by rustls (no OpenSSL dependency), prices use rust_decimal for lossless decimal arithmetic, and observability uses the tracing ecosystem with Prometheus metrics. The full stack compiles to a single binary of ~15-25 MB.

**Core technologies:**
- **tokio 1.49 + tokio-tungstenite 0.28**: Async runtime and WebSocket client -- the only production-grade combination for async Rust networking
- **rust_decimal 1.40**: 128-bit decimal arithmetic -- exact price/probability representation, no floating-point drift; critical because exchanges send decimal strings
- **serde + serde_json**: Universal serialization -- every exchange API message deserializes through serde; zero runtime overhead with derive macros
- **statrs 0.18**: Normal distribution CDF for Black-76 pricing -- note MSRV constraint of Rust 1.87+ (pin to 0.17.x or implement CDF manually if toolchain is older)
- **postcard 1.1**: Binary serialization for feed recording/replay -- do NOT use bincode (RUSTSEC-2025-0141, abandoned)
- **tracing 0.1 + prometheus-client 0.24**: Structured logging with async-aware spans and official Prometheus metrics export
- **dashmap 6.1**: Concurrent HashMap for shared orderbook state under read-heavy workloads

**Critical version note:** statrs 0.18 requires Rust 1.87+. All other crates work with Rust 1.70+. This is the binding MSRV constraint.

### Expected Features

**Must have (table stakes -- v1 signal generation):**
- Multi-venue WebSocket feed ingestion (Polymarket CLOB, Kalshi orderbook, Deribit options chain)
- Feed recording from day one (every raw WS message to JSONL with receive timestamp)
- Orderbook normalization to unified internal representation (cents to probability, BTC-denominated to probability)
- Staleness detection per instrument per venue (configurable threshold, default 5s)
- Cross-platform spread detection (4 patterns: Poly YES + Kalshi NO, inverse, and each direction)
- Fee-adjusted net spread (Polymarket dynamic taker fee up to 3.15%, Kalshi 7% profit fee, Deribit maker/taker)
- Event mapping with structured field matching (asset, strike, expiry, direction) plus manual registry
- Settlement basis risk scoring per event pair
- Automatic reconnection with exponential backoff and jitter
- Paper trade P&L tracking (hypothetical entry/exit at signal time)
- TOML configuration, structured JSON logging, Prometheus metrics
- Graceful degradation on feed loss, mock data layer for testing

**Should have (differentiators -- v1.x enhanced analytics):**
- Black-76 pricing engine with skew-adjusted call spread replication for options-derived probabilities
- Implied volatility surface construction (SVI or cubic spline interpolation)
- Basis risk decomposition (pure pricing discrepancy vs settlement risk vs fee drag vs temporal basis)
- Liquidity-aware position sizing based on visible orderbook depth
- Greeks exposure reporting for cross-asset positions
- Deterministic replay from recorded feeds, historical spread analytics, signal quality scoring
- Configurable alert thresholds with Telegram/webhook notifications

**Defer (v2+):**
- Order execution engine (requires proven signal quality first -- hard gate)
- Cross-venue order management and leg risk management
- Circuit breakers, live position tracking, gas optimization
- AI/ML signal prediction (overfitting risk, arbs are event-driven not pattern-driven)
- Multi-chain/DEX arbitrage (fundamentally different problem domain)
- Web dashboard (use Grafana + Prometheus instead)

### Architecture Approach

The system follows an async actor pipeline pattern: independent tokio tasks communicate through bounded typed channels (mpsc for point-to-point, broadcast for fan-out, watch for latest-value config). Feed actors parse venue-specific wire formats into normalized MarketSnapshot events. A normalization bus fans in all feeds and fans out to downstream consumers. The pricing engine is deliberately synchronous (pure CPU computation, no async overhead). Signal generation combines mapped event pairs with priced outcomes, applies cost adjustments and staleness gates, and routes actionable signals to logging, metrics, and (v2) execution sinks. Graceful shutdown uses a CancellationToken tree.

**Major components:**
1. **Feed Actors** (src/feeds/) -- one tokio task per venue; owns WS connection, reconnection, heartbeat, auth; produces MarketSnapshot
2. **Normalization Bus** (src/feeds/normalizer.rs) -- fan-in via tokio::select!, attaches receive timestamp and sequence number, publishes via broadcast channel
3. **Event Mapping** (src/events/) -- cross-venue instrument registry, settlement basis analyzer, canonical event ID system
4. **Pricing Engine** (src/pricing/) -- Black-76, Newton-Raphson IV solver, call spread replication, Greeks; pure sync functions, no async
5. **Signal Generator** (src/signals/) -- spread calculator, cost adjustments, staleness detection, threshold engine
6. **Signal Router** -- fan-out to logging, metrics, recording; v2 adds execution forwarding
7. **Telemetry** (src/telemetry/) -- tracing setup, Prometheus metrics, feed recording for replay

### Critical Pitfalls

1. **Risk-neutral vs real-world probability conflation** -- Options-implied probabilities (Q-measure) systematically overstate bad-state probabilities due to the volatility risk premium. Do not treat the raw options-implied probability as "the market's true belief." Frame signals as deviations from the *historical* wedge between options and prediction market probabilities, not absolute mispricings. Log both raw and adjusted probabilities; track directional bias.

2. **Settlement basis risk** -- Polymarket and Kalshi can resolve the same real-world event differently (proven by the Cardi B incident; Kalshi Rule 6.3(c) allows settlement at last-traded price). Build formal settlement specifications per event pair. Score each pair LOW/MEDIUM/HIGH. Widen required spread for HIGH-risk pairs.

3. **Naive digital option pricing (N(d2) under skew)** -- N(d2) is correct only when implied vol is constant across strikes. BTC options on Deribit have significant skew; the N(d2) error is 2-5% in probability terms, which exceeds typical arbitrage spreads. Use tight call spread replication with per-strike interpolated vol instead.

4. **Stale data generating false signals** -- During fast-moving events, prediction markets move instantly while options market makers widen/pull quotes. A fresh prediction market price compared against a stale options price looks like a huge mispricing but is not. Gate all signals on both venues having fresh data. Widen thresholds during known high-vol windows (FOMC, CPI).

5. **Transaction cost blindness** -- A 3% gross spread becomes negative after Polymarket dynamic fees (up to 1.56% at 50/50 odds), Deribit fees, bid-ask spreads (5-15% of option price for OTM), and slippage on thin prediction market books. Model all-in cost from day one. Expect 90%+ signal reduction when costs are added.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Foundation and Core Types
**Rationale:** Every other module imports shared types, config, and telemetry. Getting these right prevents cascading refactors across the entire codebase. The architecture research explicitly identifies this as the first build dependency.
**Delivers:** Compilable project skeleton with shared domain types, configuration loading, structured logging, and error handling.
**Addresses:** TS-5 (TOML configuration, structured logging), foundational types for all downstream work.
**Avoids:** Pitfall of cascading type refactors by establishing Venue, Decimal wrappers, Timestamp, MarketSnapshot, NormalizedSnapshot, and error types early.

### Phase 2: Single Feed Integration and Data Pipeline
**Rationale:** The feed layer is the most complex integration point (external APIs, reconnection, auth). Getting one feed working end-to-end proves the entire pipeline architecture. Start with Deribit -- best-documented API and existing Rust crate for reference patterns. Feed recording must be baked in from day one (not bolted on later).
**Delivers:** One working venue feed (Deribit) with WebSocket connection, reconnection with backoff, heartbeat monitoring, normalization bus, and raw message recording to JSONL.
**Addresses:** TS-1 (Deribit feed), TS-2 (feed recording, staleness detection, reconnection, heartbeat), partial TS-6 (mock data layer trait).
**Avoids:** Pitfall 4 (stale data) by building staleness detection into the data layer from the start, not as a later bolt-on.

### Phase 3: Event Mapping and Multi-Venue Feeds
**Rationale:** Signal generation requires mapped event pairs across venues. The event mapping layer is domain-specific logic with no external dependencies and can be tested with synthetic data. Adding Polymarket and Kalshi feeds here completes the data pipeline. Event mapping must capture settlement specifications from day one to avoid Pitfall 2.
**Delivers:** All three venue feeds operational. Cross-venue event registry with settlement basis risk scoring. Full data pipeline from raw WS to normalized, mapped event pairs.
**Addresses:** TS-1 (Polymarket + Kalshi feeds), TS-4 (event mapping, settlement risk scoring, expiry alignment), TS-6 (graceful degradation on feed loss).
**Avoids:** Pitfall 2 (settlement basis risk) by building formal settlement specs into the mapping layer. Pitfall 6 (expiry mismatch) by enforcing temporal and strike gap cutoffs.

### Phase 4: Prediction Market Spread Detection and Paper Trading
**Rationale:** Before tackling the complex cross-asset pricing (options vs prediction markets), get the simpler cross-platform prediction market arbitrage working (Polymarket vs Kalshi). This validates the entire pipeline end-to-end with a simpler pricing model (both sides are probabilities in cents, no Black-76 needed).
**Delivers:** Cross-platform spread detection (4 patterns), fee-adjusted net spread, continuous spread logging, paper trade P&L tracking. First actionable signals.
**Addresses:** TS-3 (parity check, cross-platform spreads, fee-adjusted net spread, spread logging), TS-5 (paper trade P&L, metrics).
**Avoids:** Pitfall 5 (transaction cost blindness) by integrating all-in cost model from the start. Polymarket dynamic fee curve must be implemented correctly.

### Phase 5: Options Pricing Engine (Cross-Asset Dimension)
**Rationale:** This is the core differentiator but also the most complex component. It depends on a working data pipeline (Phase 2-3) and validated event mapping (Phase 3). The pricing engine is pure synchronous math with no async -- it can be exhaustively unit-tested with known analytical solutions. Must address Pitfalls 1 and 3 during this phase.
**Delivers:** Black-76 pricing, Newton-Raphson IV solver, call spread replication for digital payoffs, implied volatility surface construction, skew-adjusted probability extraction. Cross-asset signals comparing options-derived probabilities against prediction market prices.
**Addresses:** D-1 (Black-76, call spread replication, IV surface, skew-adjusted digital pricing), partial D-2 (basis risk decomposition, Greeks exposure).
**Avoids:** Pitfall 1 (risk-neutral vs real-world conflation) by logging both raw and adjusted probabilities and tracking directional bias. Pitfall 3 (naive N(d2)) by implementing call spread replication from the start, never exposing N(d2)-only signals.

### Phase 6: Replay, Analytics, and Hardening
**Rationale:** By this point, 2-4 weeks of recorded feed data exists. Deterministic replay and historical analytics require this data. Signal quality scoring requires 30+ days of paper trade history. This phase turns raw signals into a validated strategy with measured edge.
**Delivers:** Deterministic replay from recorded JSONL feeds, historical spread analytics, signal quality scoring (hit rate, Sharpe, max drawdown), configurable alert thresholds, Telegram/webhook notifications.
**Addresses:** D-3 (replay, analytics, signal scoring), D-4 (alerts), D-2 (liquidity-aware sizing, full basis risk decomposition).
**Avoids:** Premature optimization -- by this phase, real data reveals which signals are genuine and which are artifacts of stale data, risk premiums, or cost model gaps.

### Phase Ordering Rationale

- **Foundation first:** Types and config are imported by every module. Changing them later causes cascading refactors across all other phases.
- **One feed before three:** Proving the pipeline architecture with a single feed (Deribit) is faster and cheaper than debugging three concurrent integrations. The normalization bus, channel patterns, and staleness detection are all validated with one feed.
- **Prediction market arb before cross-asset arb:** Polymarket-vs-Kalshi spreads are computationally simpler (probability vs probability) and validate the entire signal pipeline. Cross-asset pricing (Phase 5) adds the Black-76 engine on top of an already-working pipeline.
- **Pricing engine after event mapping:** The call spread replication requires knowing which options instruments correspond to which prediction market events. The mapping layer must exist first.
- **Replay and analytics last:** These require accumulated data. Starting them before weeks of recorded data exist produces meaningless results.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Deribit Feed):** Deribit's API has specific quirks around subscription limits (500 channels, 32 connections per IP), mark price vs last-traded price for illiquid options, and auth token refresh. The official docs are good but integration testing will reveal edge cases.
- **Phase 3 (Polymarket + Kalshi Feeds):** Polymarket has two separate WS endpoints (CLOB and RTDS) with different semantics. Kalshi uses RSA-PSS auth which requires the `rsa` crate. Settlement specification research is critical and venue-specific.
- **Phase 5 (Options Pricing Engine):** The risk-neutral vs real-world probability issue (Pitfall 1) requires careful design. Vol surface construction (SVI parameterization or spline interpolation) and butterfly arbitrage checking are mathematically nuanced. Academic sources provide formulas but implementation details matter.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Foundation):** Standard Rust project setup with tokio, serde, tracing, clap. Well-documented patterns everywhere.
- **Phase 4 (Spread Detection):** Straightforward arithmetic on normalized orderbooks. The fee models are documented in venue APIs.
- **Phase 6 (Replay/Analytics):** Deterministic replay from recorded JSONL is a well-understood pattern. Signal quality metrics (Sharpe, drawdown) are standard finance.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All crate versions verified via docs.rs. Compatibility confirmed. Only uncertainty is statrs 0.18 MSRV (Rust 1.87+) which may require pinning to 0.17.x. |
| Features | HIGH | Feature landscape well-defined by venue documentation and competitor analysis. Differentiator (cross-asset arb) is novel and validated by domain literature (Moontower Meta, Quant Next). |
| Architecture | HIGH | Actor-pipeline pattern is battle-tested in Rust crypto trading systems (kucoin_arbitrage, barter-rs). Channel patterns from official tokio documentation. |
| Pitfalls | HIGH | Core pricing pitfalls (risk-neutral conflation, N(d2) under skew) backed by quantitative finance literature. Settlement basis risk validated by real-world incidents (Cardi B). |

**Overall confidence:** HIGH

### Gaps to Address

- **Risk premium calibration:** How large is the typical wedge between options-implied and prediction market probabilities for BTC events? Need 2-4 weeks of parallel data collection to establish a baseline before signals can be meaningfully interpreted. Plan to collect data without acting on signals during initial operation.
- **Polymarket international vs US resolution mechanics:** UMA Optimistic Oracle resolution (2hr challenge window, possible 48-96hr DVM escalation) affects capital lockup for 1.5% of events. Need to determine if this is material for the target event set.
- **Kalshi API access tier:** Rate limits are tiered (Basic 20/s, Premier 100/s, Prime 400/s). The access tier determines feed refresh rate and will affect data quality. Confirm tier before building rate-limiting logic.
- **Deribit daily options strike grid:** Strike spacing ($125 for dailies) and range (~5% around ATM) determines whether tradeable event-option pairs exist for the target prediction market events. Need to catalog live strike grids against active prediction market events to confirm the opportunity set is non-empty.
- **statrs MSRV:** If Rust 1.87+ is not available on the build toolchain, implement the Normal CDF manually (~20 lines) or pin statrs to 0.17.x. Validate before Phase 5.

## Sources

### Primary (HIGH confidence)
- [Deribit API Documentation](https://docs.deribit.com/) -- WebSocket subscriptions, settlement rules, market data best practices
- [Deribit Settlement Rules](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) -- 30-min TWAP from 450 index samples
- [Polymarket CLOB Introduction](https://docs.polymarket.com/developers/CLOB/introduction) -- CLOB WebSocket, REST API, fee structure
- [Polymarket Trading Fees](https://docs.polymarket.com/polymarket-learn/trading/fees) -- dynamic fee formula verified
- [Kalshi API Documentation](https://docs.kalshi.com/welcome) -- REST + WebSocket, rate limits, RSA-PSS auth
- [Quant Next: Binary Options Pricing, Replication and Skew Sensitivity](https://quant-next.com/binary-options-pricing-replication-and-skew-sensitivity/) -- call spread replication formulas
- [Black-76 Formula (LME)](https://www.lme.com/en/trading/contract-types/options/black-scholes-76-formula) -- Black-76 closed-form pricing
- [Tokio Tutorial: Channels](https://tokio.rs/tokio/tutorial/channels) -- mpsc, broadcast, watch patterns
- [barter-rs Trading Framework](https://github.com/barter-rs/barter-rs) -- reference Rust trading architecture
- All crate versions verified via docs.rs (tokio, serde, rust_decimal, tracing, etc.)

### Secondary (MEDIUM confidence)
- [Moontower Meta: Prediction Market Arbitrage Using Option Chains](https://moontowermeta.com/prediction-market-arbitrage-using-option-chains-to-find-mispriced-bets/) -- domain thesis validation
- [Toward Black-Scholes for Prediction Markets (arXiv)](https://arxiv.org/html/2510.15205v1) -- risk-neutral vs real-world probability framework
- [FactSet: Mind Your Ps and Qs](https://insight.factset.com/mind-your-ps-and-qs-real-world-vs.risk-neutral-probabilities) -- probability measure explanation
- [Cardi B Settlement Dispute](https://www.gamblinginsider.com/news/110468/kalshi-polymarket-cardi-b-halftime-settlement-cftc-complaint) -- settlement divergence evidence
- [poly-kalshi-arb (Rust, 409 stars)](https://github.com/taetaehoho/poly-kalshi-arb) -- competitor architecture reference
- [PMC: Implied Volatility Estimation of Bitcoin Options](https://pmc.ncbi.nlm.nih.gov/articles/PMC8418903/) -- Newton-Raphson vs Bisection for BTC IV

### Tertiary (LOW confidence)
- [SettleRisk - Resolution Risk Scoring](https://settlerisk.com/) -- settlement risk framework (single source)
- [Bitcoin Implied Volatility Surface from Deribit (Medium)](https://medium.com/coinmonks/bitcoin-implied-volatility-surface-from-deribit-70fba845102a) -- implementation-relevant but blog source

---
*Research completed: 2026-02-21*
*Ready for roadmap: yes*
