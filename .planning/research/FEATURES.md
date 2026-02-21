# Feature Research

**Domain:** Cross-venue crypto prediction market / options market arbitrage
**Researched:** 2026-02-21
**Confidence:** HIGH (core features) / MEDIUM (execution features)

## Feature Landscape

### Table Stakes (Users Expect These)

Features that any cross-venue arbitrage signal system must have. Missing these means the system produces unreliable or unusable output.

#### TS-1: Multi-Venue Market Data Ingestion

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Polymarket WebSocket feed (CLOB + RTDS) | Primary prediction market venue; ~37% market share. Without it you miss half the arb. | MEDIUM | Two WS endpoints: `wss://ws-subscriptions-clob.polymarket.com` for orderbook, `wss://ws-live-data.polymarket.com` for prices. 3000 req/10min REST rate limit. Up to 10 instruments per WS. |
| Kalshi WebSocket feed (orderbook_delta, ticker, trade) | Primary regulated prediction market venue; ~62% market share. The other half of the arb. | MEDIUM | Public channels (ticker, trade, orderbook_delta) need no auth. Rate limits tiered: Basic 20 read/s 10 write/s, Premier 100/100, Prime 400/400. |
| Deribit WebSocket feed (options chain, Greeks, ticker) | The options market side of the cross-asset arb. ~85% BTC options market share. | MEDIUM | Up to 500 channels per subscription, 32 connections per IP. Deribit natively provides Greeks computed via Black-76. Use `instrument.state` feed to catch new listings/expirations. |
| Orderbook normalization | Each venue uses different formats, price representations, and semantics. Unified internal representation required for comparison. | MEDIUM | Polymarket: prices in cents (0-100); Kalshi: prices in cents; Deribit: options priced in BTC (inverse) or USDC (linear). Must normalize to common probability representation. |

#### TS-2: Feed Reliability and Data Quality

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Feed recording (raw WS messages to line-delimited JSON) | Mandatory for debugging, replay, backtesting, and audit. Without historical data you cannot improve the system. | LOW | Record every raw WS message with receive timestamp. Line-delimited JSON (JSONL) for streaming writes. Compress/rotate daily. |
| Staleness detection per feed | Stale data produces phantom spreads. A 5-second-old quote on a volatile BTC market is worthless. | MEDIUM | Track last-update timestamp per instrument per venue. Configurable threshold (default 5s). Reject any spread calculation involving a stale leg. Surface staleness in metrics. |
| Automatic reconnection with exponential backoff | WS feeds drop. A system that crashes on disconnect is useless. | MEDIUM | Exponential backoff with jitter. Detect stale connections (no messages for N seconds even when market is open). Mark feed as degraded during reconnect. Never silently serve stale data. |
| Heartbeat monitoring | Distinguish "no updates because market is quiet" from "connection is dead." | LOW | Polymarket and Deribit send heartbeats. Kalshi requires periodic pings. Implement per-venue heartbeat logic. |

#### TS-3: Cross-Venue Spread Calculation

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Prediction market parity check (YES + NO vs $1.00) | The most basic arb: when YES + NO < $1.00 on a single platform, profit is guaranteed. Foundational calculation. | LOW | Must account for fees (Polymarket dynamic taker fee up to 3.15%; Kalshi 7% on profits). |
| Cross-platform spread detection | When Polymarket YES + Kalshi NO < $1.00 (or vice versa), cross-platform arb exists. This is the core signal. | MEDIUM | Four patterns: Poly YES + Kalshi NO, Kalshi YES + Poly NO, and inverse. Must compare executable prices (best bid/ask), not last-trade. |
| Options-derived implied probability | Extract the options market's view on the same binary event and compare against prediction market prices. This is the unique cross-asset signal. | HIGH | Use Deribit options chain to construct call spread replication of binary payoff. Black-76 pricing. Vertical spread price / strike distance approximates probability. Must handle vol skew. |
| Fee-adjusted net spread | Raw spreads are meaningless; net-of-fees spread is what determines if the arb is real. | MEDIUM | Polymarket: dynamic taker fee up to 3.15%. Kalshi: 7% profit fee. Deribit: maker/taker fees on options. Gas costs for Polymarket (Polygon). Must model all costs. |
| Continuous spread logging | Every spread computation must be persisted. Periodic aggregates to metrics. | LOW | Write every computation to file (append-only). Separate periodic aggregation (1s, 10s, 60s windows) to metrics/dashboard layer. |

#### TS-4: Event Mapping

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Cross-venue event matching | Must identify that "BTC above $100k by Dec 31" on Polymarket is the same event as a similar contract on Kalshi. | HIGH | Text similarity matching is fragile. Use structured fields (underlying asset, strike price, expiry date, direction) for robust matching. Maintain a manual mapping registry with automated suggestions. |
| Settlement basis risk scoring | Polymarket and Kalshi have DIFFERENT resolution criteria for the same event. The Cardi B Super Bowl incident proved identical events can settle oppositely. This is the #1 risk. | HIGH | Score each cross-venue pair on settlement divergence risk. Factors: same source agency? Same resolution wording? Same dispute mechanism? Different platforms resolved the 2024 government shutdown event differently. Flag high-risk pairs. |
| Expiry alignment validation | Prediction market expiry must align with options expiry for cross-asset arb. Misaligned expiries create basis risk. | MEDIUM | Deribit options expire Friday 08:00 UTC (30-min TWAP settlement). Prediction markets have varied expiry mechanics. Quantify the temporal mismatch as basis risk. |

#### TS-5: Configuration and Observability

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| TOML configuration for all parameters | Thresholds, venue credentials, staleness windows, fee assumptions -- all must be configurable without recompilation. | LOW | Single `config.toml` with sections per venue, per strategy, per risk limit. Validate on startup. |
| Structured logging (JSON) | Machine-parseable logs for debugging, alerting, and audit. | LOW | Use `tracing` with JSON subscriber. Include span context for request tracing across async tasks. |
| Metrics emission (Prometheus-compatible) | Operational monitoring: feed health, spread distributions, signal rates, latencies. | MEDIUM | Key metrics: feed_last_update_age_seconds, spread_bps, signal_count, ws_reconnect_count, computation_latency_ms. |
| Paper trade P&L tracking | In signal-only mode, track what *would have* happened. Without this, you cannot evaluate signal quality. | MEDIUM | Record hypothetical entry/exit at signal time. Track per-signal P&L assuming fill at quoted price. Aggregate daily/weekly. |

#### TS-6: System Resilience

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Graceful degradation on feed loss | If one feed drops, the system should continue computing spreads for available pairs, not crash entirely. | MEDIUM | Mark affected instruments as unavailable. Continue operating on healthy feeds. Surface degraded state in metrics and alerts. |
| Mock data layer for testing | Must be able to run the entire pipeline without live venue connections. | MEDIUM | Trait-based abstraction over data sources. Mock implementations that replay recorded JSONL files. Essential for CI and development. |
| Graceful shutdown | Clean WS disconnection, flush pending writes, complete in-flight computations. | LOW | Handle SIGINT/SIGTERM. Drain channels. Flush log/recording buffers. |

---

### Differentiators (Competitive Advantage)

Features that set this system apart from the dozens of Polymarket-Kalshi arb bots on GitHub. These are not expected but create meaningful edge.

#### D-1: Cross-Asset Arbitrage (Prediction Markets vs Options Markets)

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Black-76 pricing engine for options-derived probabilities | Most arb bots only compare prediction markets to each other. Comparing against the much deeper, more liquid options market (Deribit: $185B/month volume) extracts a fundamentally different -- and often more accurate -- fair value signal. | HIGH | Implement Black-76 closed-form for European options. Requires forward price, strike, time-to-expiry, risk-free rate, implied vol. Deribit provides Greeks natively, but you need your own pricing for call spread replication. |
| Call spread replication for digital/binary payoff | The core pricing innovation: replicate a binary option payoff using a tight call spread (K-dK, K+dK) with quantity 1/(2*dK). This converts vanilla options prices into implied binary probabilities comparable to prediction market prices. | HIGH | Vertical spread price / strike distance approximates probability of finishing above midpoint. Must handle vol skew -- BTC has positive skew (unlike equity negative skew). Tighter spreads = more accurate but less liquid. |
| Implied volatility surface construction | Build a local vol surface from Deribit options chain to price binary events at arbitrary strikes, not just traded strikes. | HIGH | Interpolate across strikes and expiries. BTC vol surface exhibits forward skew (commodity-class). Newton-Raphson for IV extraction (converges faster than bisection). Use Deribit's native vol smile enforcement as cross-check. |
| Skew-adjusted digital pricing | Naive Black-76 digital pricing ignores skew. Adjusting for skew via call spread replication with market-observed vol surface is the correct approach and produces meaningfully different probabilities. | HIGH | The "skew correction" is the difference between the naive N(d2) digital price and the call-spread-replicated price. This correction can be 2-5% on BTC, which is larger than many arb spreads. |

#### D-2: Advanced Risk Analytics

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Settlement divergence probability model | Quantify the probability that two venues resolve the same event differently. No existing bot does this. | HIGH | Historical analysis of resolution disputes. Factors: shared source agencies, wording specificity, dispute mechanism type (UMA oracle vs Kalshi internal). Output: a "settlement risk score" per pair that adjusts the required spread threshold. |
| Basis risk decomposition | Break down the total spread into: pure pricing discrepancy, settlement risk premium, fee drag, and temporal basis. Traders see where the edge actually comes from. | MEDIUM | Pure pricing = options-implied minus prediction market price. Settlement risk = basis risk score * potential loss. Fee drag = total fees. Temporal basis = expiry mismatch cost. |
| Liquidity-aware position sizing | Signal is useless if you cannot actually fill at the quoted prices. Size recommendations based on visible orderbook depth. | MEDIUM | Parse orderbook depth at each level. Compute fill price for target size including slippage. Adjust signal strength based on executable (not quoted) spread. |
| Greeks exposure reporting | When comparing options positions to prediction market positions, surface the residual Greeks (delta, gamma, vega, theta) of the combined position. | MEDIUM | A "hedged" arb between a prediction market position and an options position still has residual vega and theta exposure. Surfacing this prevents false confidence in "risk-free" signals. |

#### D-3: Replay and Backtesting Infrastructure

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Deterministic replay from recorded feeds | Replay recorded JSONL feeds through the full pipeline with identical computation. Essential for strategy iteration. | MEDIUM | Same trait-based data source abstraction used for mocks. Feed timestamps drive replay clock. Verify deterministic output (same input = same signals). |
| Historical spread analytics | Aggregate historical spreads to identify: time-of-day patterns, event-type patterns, liquidity regime patterns. | MEDIUM | Post-process recorded spread logs. Identify when arbs occur, how long they persist, and how they resolve. This informs strategy parameters. |
| Signal quality scoring (hit rate, avg P&L, Sharpe) | Evaluate signal quality over historical data. Required before going live. | MEDIUM | Track: hit rate (% of signals that would have been profitable), average P&L per signal, Sharpe ratio, max drawdown of hypothetical portfolio. |

#### D-4: Alerting and Notification

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Configurable alert thresholds with priority levels | Not all signals deserve attention. Priority routing (High: >10% ROI, Medium: >5%, Low: below) prevents alert fatigue. | LOW | TOML-configurable thresholds. Priority levels map to notification channels. |
| Multi-channel alerts (Telegram, webhook, log) | Reach the operator wherever they are. | LOW | Telegram bot API for mobile alerts. Generic webhook for integration flexibility. Always log regardless. |

---

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems. Deliberately exclude these.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Auto-execution in v1 | "Why just signal when you can trade?" | Non-atomic cross-venue execution means leg risk (one fill, other fails = naked directional exposure). Settlement divergence risk means "risk-free" arbs are not risk-free. Requires capital, custody, exchange accounts, and regulatory compliance. Premature execution before signal quality is proven destroys capital. | Paper trading with rigorous P&L tracking. Prove signals first. v2 adds execution only after demonstrated edge. |
| AI/ML-based signal prediction | "Predict when arbs will appear" | Overfitting on small dataset. Arb opportunities are driven by external events (news, liquidations), not predictable patterns. ML adds complexity without clear edge in this domain. Academic research (Okasova 2026) shows ML can predict arb *occurrence* but not *direction* or *magnitude*. | Statistical analysis of historical patterns (time-of-day, event-type). Simple heuristics beat complex models here. |
| Multi-chain/DEX arbitrage | "Why not also scan Uniswap, Jupiter, etc.?" | Fundamentally different problem domain. DEX arb is about AMM math, MEV, and block-time execution. Prediction market arb is about event mapping and settlement risk. Combining them creates an unfocused system that does neither well. | Stay focused on prediction market vs options market cross-venue arb. This is already a novel and underserved niche. |
| Real-time everything (sub-millisecond latency) | "Lower latency = better" | Prediction market arb opportunities persist for minutes to hours, not milliseconds. Over-investing in latency optimization is wasted effort. The bottleneck is not data speed but pricing model accuracy and settlement risk assessment. Cross-venue arb windows measured in minutes, not microseconds. | Target sub-second data freshness and 1-5 second signal computation. This is more than fast enough for the opportunity window. |
| Full portfolio management / multi-strategy framework | "Build a general trading platform" | Scope explosion. A portfolio management system is a different product. Building one adds months of work (position netting, margin calculation, multi-strategy allocation) that delays the core value: detecting cross-asset mispricings. | Single-strategy focus. Paper trade log with daily P&L summary. Add portfolio features only if v2 execution reveals the need. |
| Web dashboard with real-time charts | "Need a beautiful UI" | Significant frontend development effort that does not improve signal quality. At the signal generation phase, a terminal-based log viewer or simple TUI is sufficient. | Structured JSON logs parseable by Grafana/Kibana. Prometheus metrics with Grafana dashboards. Zero custom frontend code in v1. |
| Simultaneous support for dozens of prediction markets | "Cover everything: PredictIt, Metaculus, Manifold, Drift..." | Each venue has different APIs, fee structures, settlement mechanisms, and resolution criteria. Supporting many venues creates an O(n^2) testing matrix. Diminishing returns on venues with low liquidity. | Start with Polymarket + Kalshi (>99% of liquid BTC prediction market volume) + Deribit (>85% BTC options). Add venues only when these three are rock-solid. |

---

## Feature Dependencies

```
[Feed Recording (TS-2)]
    |
    v
[Market Data Ingestion (TS-1)]
    |
    +-----> [Orderbook Normalization (TS-1)]
    |           |
    |           v
    |       [Staleness Detection (TS-2)]
    |           |
    |           v
    |       [Cross-Platform Spread Detection (TS-3)]
    |           |
    |           +-----> [Fee-Adjusted Net Spread (TS-3)]
    |           |           |
    |           |           v
    |           |       [Paper Trade P&L Tracking (TS-5)]
    |           |
    |           +-----> [Continuous Spread Logging (TS-3)]
    |
    +-----> [Event Mapping (TS-4)]
    |           |
    |           +-----> [Settlement Basis Risk Scoring (TS-4)]
    |           |
    |           +-----> [Expiry Alignment Validation (TS-4)]
    |
    +-----> [Black-76 Pricing Engine (D-1)]
                |
                v
            [Call Spread Replication (D-1)]
                |
                v
            [Options-Derived Implied Probability (TS-3)]
                |
                v
            [Skew-Adjusted Digital Pricing (D-1)]
                |
                v
            [Basis Risk Decomposition (D-2)]

[Feed Recording (TS-2)] -----> [Deterministic Replay (D-3)]
                                    |
                                    v
                                [Historical Spread Analytics (D-3)]
                                    |
                                    v
                                [Signal Quality Scoring (D-3)]

[Mock Data Layer (TS-6)] -----> [Deterministic Replay (D-3)]

[TOML Configuration (TS-5)] ----enhances----> [Everything]

[Graceful Degradation (TS-6)] ----enhances----> [Market Data Ingestion (TS-1)]

[Paper Trade P&L (TS-5)] ----required-before----> [Live Execution (v2)]
[Signal Quality Scoring (D-3)] ----required-before----> [Live Execution (v2)]
```

### Dependency Notes

- **Market Data Ingestion requires Feed Recording from day one:** Recording must be baked into the data layer, not bolted on later. Every raw WS message is persisted before processing.
- **Spread Detection requires Staleness Detection:** A spread computed from stale quotes is worse than no spread -- it generates false signals. Staleness gating must be upstream of spread calculation.
- **Options-Derived Probability requires Black-76 + Call Spread Replication:** You cannot meaningfully compare prediction markets to options markets without proper binary pricing. This is the entire thesis of the cross-asset approach.
- **Signal Quality Scoring requires Feed Recording + Replay:** You need historical data to score signals. Replay infrastructure reuses the mock data abstraction.
- **Live Execution (v2) requires proven signal quality:** Paper trading must demonstrate edge before risking capital. This is a hard gate.
- **Settlement Basis Risk Scoring enhances Spread Detection:** Without settlement risk scoring, "risk-free" cross-venue arbs carry hidden risk. This does not block spread detection but dramatically improves signal quality.

---

## MVP Definition

### Launch With (v1 -- Paper Trading / Signal Generation)

Minimum viable system to detect and evaluate cross-venue arbitrage signals.

- [ ] **Polymarket WebSocket feed** -- ingest CLOB orderbook data for BTC binary markets
- [ ] **Kalshi WebSocket feed** -- ingest orderbook data for BTC binary markets
- [ ] **Deribit WebSocket feed** -- ingest options chain data for BTC (ticker, Greeks, orderbook)
- [ ] **Feed recording** -- every raw WS message to JSONL from day one
- [ ] **Orderbook normalization** -- unified internal representation across venues
- [ ] **Staleness detection** -- per-instrument, per-venue, configurable threshold (default 5s)
- [ ] **Cross-platform spread detection** -- Polymarket vs Kalshi parity and cross-platform spreads
- [ ] **Fee-adjusted net spread** -- model all venue fees in spread calculation
- [ ] **Event mapping** -- structured matching (asset, strike, expiry, direction) with manual registry
- [ ] **Settlement basis risk scoring** -- flag pairs with divergent resolution criteria
- [ ] **Continuous spread logging** -- every computation to file
- [ ] **TOML configuration** -- all parameters externalized
- [ ] **Structured logging** -- JSON tracing
- [ ] **Graceful degradation** -- feed drops do not crash system
- [ ] **Mock data layer** -- trait-based abstraction, replay from recorded JSONL
- [ ] **Paper trade P&L tracking** -- hypothetical entry/exit tracking per signal
- [ ] **Automatic reconnection** -- exponential backoff with jitter

### Add After Validation (v1.x -- Enhanced Analytics)

Features to add once core pipeline is stable and generating signals.

- [ ] **Black-76 pricing engine** -- when signal pipeline is stable and generating consistent cross-platform spreads; adds the cross-asset dimension
- [ ] **Call spread replication** -- when Black-76 engine is working; enables options-derived probability comparison
- [ ] **Implied volatility surface construction** -- when call spread replication reveals need for non-traded strikes
- [ ] **Skew-adjusted digital pricing** -- when IV surface is available; the "correct" way to price binary events from options
- [ ] **Basis risk decomposition** -- when cross-asset signals are flowing; breaks down where edge comes from
- [ ] **Liquidity-aware position sizing** -- when paper trade P&L reveals fill-rate issues
- [ ] **Greeks exposure reporting** -- when cross-asset positions are being tracked
- [ ] **Deterministic replay** -- when enough recorded data exists (2-4 weeks of feeds)
- [ ] **Historical spread analytics** -- when replay infrastructure is working
- [ ] **Signal quality scoring** -- when 30+ days of paper trade data exists
- [ ] **Configurable alert thresholds** -- when signal volume warrants filtering
- [ ] **Telegram/webhook alerts** -- when alert thresholds are tuned

### Future Consideration (v2+ -- Live Execution)

Features to defer until paper trading demonstrates consistent, measurable edge.

- [ ] **Order execution engine** -- requires proven signal quality, exchange accounts, and custody setup
- [ ] **Cross-venue order management** -- simultaneous order submission to Polymarket CLOB + Kalshi + Deribit
- [ ] **Leg risk management** -- detect and handle partial fills (one leg fills, other fails)
- [ ] **Circuit breaker** -- automatic position limits, daily loss caps, kill switch
- [ ] **Position tracking (live)** -- real-time position and P&L across venues
- [ ] **Gas optimization** -- Polygon gas cost modeling for Polymarket execution
- [ ] **Rate limit management** -- token bucket per venue, tiered access
- [ ] **Expiry alignment hedging** -- manage temporal basis risk between options and prediction market expiries

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority | Phase |
|---------|------------|---------------------|----------|-------|
| Feed recording (JSONL) | HIGH | LOW | P1 | v1 |
| Polymarket WS feed | HIGH | MEDIUM | P1 | v1 |
| Kalshi WS feed | HIGH | MEDIUM | P1 | v1 |
| Deribit WS feed | HIGH | MEDIUM | P1 | v1 |
| Orderbook normalization | HIGH | MEDIUM | P1 | v1 |
| Staleness detection | HIGH | MEDIUM | P1 | v1 |
| Cross-platform spread detection | HIGH | MEDIUM | P1 | v1 |
| Fee-adjusted net spread | HIGH | MEDIUM | P1 | v1 |
| Event mapping (structured) | HIGH | HIGH | P1 | v1 |
| Settlement basis risk scoring | HIGH | HIGH | P1 | v1 |
| Continuous spread logging | HIGH | LOW | P1 | v1 |
| TOML configuration | HIGH | LOW | P1 | v1 |
| Structured logging | HIGH | LOW | P1 | v1 |
| Graceful degradation | HIGH | MEDIUM | P1 | v1 |
| Mock data layer | HIGH | MEDIUM | P1 | v1 |
| Paper trade P&L | HIGH | MEDIUM | P1 | v1 |
| Auto-reconnection | HIGH | MEDIUM | P1 | v1 |
| Black-76 pricing engine | HIGH | HIGH | P2 | v1.x |
| Call spread replication | HIGH | HIGH | P2 | v1.x |
| IV surface construction | MEDIUM | HIGH | P2 | v1.x |
| Skew-adjusted digital pricing | HIGH | HIGH | P2 | v1.x |
| Basis risk decomposition | MEDIUM | MEDIUM | P2 | v1.x |
| Liquidity-aware sizing | MEDIUM | MEDIUM | P2 | v1.x |
| Greeks exposure reporting | MEDIUM | MEDIUM | P2 | v1.x |
| Deterministic replay | MEDIUM | MEDIUM | P2 | v1.x |
| Historical spread analytics | MEDIUM | MEDIUM | P2 | v1.x |
| Signal quality scoring | HIGH | MEDIUM | P2 | v1.x |
| Alert thresholds | LOW | LOW | P2 | v1.x |
| Telegram/webhook alerts | LOW | LOW | P2 | v1.x |
| Order execution engine | HIGH | HIGH | P3 | v2 |
| Cross-venue OMS | HIGH | HIGH | P3 | v2 |
| Leg risk management | HIGH | HIGH | P3 | v2 |
| Circuit breaker | HIGH | MEDIUM | P3 | v2 |
| Live position tracking | HIGH | MEDIUM | P3 | v2 |
| Gas optimization | LOW | MEDIUM | P3 | v2 |
| Rate limit management | MEDIUM | MEDIUM | P3 | v2 |

**Priority key:**
- P1: Must have for launch (v1 signal generation)
- P2: Should have, add when pipeline is stable (v1.x enhanced analytics)
- P3: Future consideration, requires proven edge (v2 execution)

---

## Competitor Feature Analysis

| Feature | poly-kalshi-arb (Rust, 409 stars) | polymarket-arbitrage (Python, ImMike) | polymarket-kalshi-btc-arbitrage-bot (Python) | **Our Approach** |
|---------|----------------------------------|--------------------------------------|----------------------------------------------|-----------------|
| Prediction market cross-platform arb | Yes (Poly + Kalshi) | Yes (Poly + Kalshi, 10k+ markets) | Yes (BTC 1-hour price markets) | Yes, but also cross-asset vs Deribit options |
| Options market comparison | No | No | No | **Yes -- core differentiator** |
| Black-76 / binary pricing | No | No | No | **Yes -- skew-adjusted digital pricing** |
| Settlement risk scoring | No | No | No | **Yes -- quantified per pair** |
| Feed recording | No | No | No | **Yes -- from day one** |
| Staleness detection | No | No | No | **Yes -- configurable per instrument** |
| Paper trading mode | Yes (DRY_RUN) | Not documented | Yes (dry run) | **Yes -- with P&L tracking and signal quality scoring** |
| Event matching | SIMD-optimized text matching | Automated text similarity | Strike price comparison | **Structured field matching + manual registry** |
| Circuit breaker | Yes (position limits, daily loss) | Not documented | Not documented | v2 scope |
| Execution | Yes (concurrent leg execution) | Yes | No (signal only) | v2 scope (prove signals first) |
| Language | Rust | Python | Python | **Rust (tokio async runtime)** |
| Backtesting / replay | No | No | No | **Yes -- deterministic replay from recorded feeds** |

**Key competitive gap:** No existing open-source system compares prediction market prices against options-derived binary probabilities. All existing bots compare prediction markets to each other. The cross-asset dimension (prediction market vs options market) is novel and accesses a fundamentally different source of price discovery -- the much deeper, more liquid Deribit options market ($185B/month volume vs prediction market billions/year).

---

## Sources

### Venue Documentation (HIGH confidence)
- [Polymarket CLOB Introduction](https://docs.polymarket.com/developers/CLOB/introduction)
- [Polymarket Data Feeds](https://docs.polymarket.com/developers/market-makers/data-feeds)
- [Kalshi API Rate Limits](https://docs.kalshi.com/getting_started/rate_limits)
- [Kalshi Quick Start: Market Data](https://docs.kalshi.com/getting_started/quick_start_market_data)
- [Deribit API Documentation](https://docs.deribit.com/)
- [Deribit Market Data Best Practices](https://support.deribit.com/hc/en-us/articles/29592500256669-Market-Data-Collection-Best-Practices)
- [Deribit Settlement Rules](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement)

### Domain Analysis (MEDIUM confidence)
- [Prediction Market Arbitrage: Using Option Chains to Find Mispriced Bets -- Moontower Meta](https://moontowermeta.com/prediction-market-arbitrage-using-option-chains-to-find-mispriced-bets/)
- [The Math of Prediction Markets: Binary Options, Kelly Criterion, and CLOB Pricing](https://navnoorbawa.substack.com/p/the-math-of-prediction-markets-binary)
- [Building a Prediction Market Arbitrage Bot: Technical Implementation](https://navnoorbawa.substack.com/p/building-a-prediction-market-arbitrage)
- [How Kalshi and Polymarket Settle Markets (and Disputes)](https://defirate.com/prediction-markets/how-contracts-settle/)
- [Prediction Markets: The Rise of Event-Driven Finance -- Crypto.com Research](https://crypto.com/en/research/prediction-markets-oct-2025)

### Binary Options Pricing (HIGH confidence)
- [Binary Options: Replication and Skew Sensitivity -- Quant Next](https://quant-next.com/binary-options-pricing-replication-and-skew-sensitivity/)
- [Black-76 Option Pricing Formulas -- LME](https://www.lme.com/en/trading/contract-types/options/black-scholes-76-formula)
- [Black model -- Wikipedia](https://en.wikipedia.org/wiki/Black_model)

### Competitor Analysis (MEDIUM confidence)
- [poly-kalshi-arb (Rust)](https://github.com/taetaehoho/poly-kalshi-arb) -- 409 stars, most sophisticated existing implementation
- [polymarket-kalshi-btc-arbitrage-bot (Python)](https://github.com/CarlosIbCu/polymarket-kalshi-btc-arbitrage-bot) -- BTC-specific implementation
- [polymarket-arbitrage (Python)](https://github.com/ImMike/polymarket-arbitrage) -- 10k+ market scanner

### Academic/Research (MEDIUM confidence)
- [Implied volatility estimation of bitcoin options -- PMC](https://pmc.ncbi.nlm.nih.gov/articles/PMC8418903/) -- Newton-Raphson vs Bisection for BTC IV
- [Predicting Arbitrage Occurrences with Machine Learning (2026)](https://onlinelibrary.wiley.com/doi/full/10.1002/nem.70030)
- [Unravelling the Probabilistic Forest: Arbitrage in Prediction Markets](https://arxiv.org/abs/2508.03474) -- IMDEA $40M arb profit analysis

---
*Feature research for: Cross-venue crypto prediction market / options market arbitrage*
*Researched: 2026-02-21*
