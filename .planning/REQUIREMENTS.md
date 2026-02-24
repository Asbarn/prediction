# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-02-21
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1 Requirements

Requirements for initial release (paper trading / signal generation). Each maps to roadmap phases.

### Market Data Ingestion

- [ ] **FEED-01**: System connects to Deribit WebSocket and subscribes to `book.{instrument}.raw` and `ticker.{instrument}.raw` channels with JSON-RPC 2.0 parsing
- [ ] **FEED-02**: System maintains local Deribit order book with incremental delta application
- [x] **FEED-03**: System connects to Polymarket CLOB WebSocket and subscribes to order book updates for target condition IDs
- [x] **FEED-04**: System normalizes Polymarket order books from probability space (0-1) with bid/ask/depth
- [x] **FEED-05**: System connects to Kalshi feed (REST polling or WebSocket) and normalizes contracts into probability + expiry schema
- [ ] **FEED-06**: All feeds publish normalized MarketSnapshot events onto a bounded async channel with venue, instrument, bid/ask probability, depth, timestamps, and sequence numbers
- [ ] **FEED-07**: System records every raw WebSocket message to line-delimited JSON with local receive timestamp and venue identifier
- [x] **FEED-08**: System logs exchange-reported timestamps alongside local receipt timestamps for each message, documenting per-feed latency characteristics

### Feed Reliability

- [ ] **RELY-01**: Each feed reconnects automatically with exponential backoff and jitter on connection loss
- [x] **RELY-02**: System detects stale connections via per-venue heartbeat monitoring (distinguish "quiet market" from "dead connection")
- [ ] **RELY-03**: Staleness detection rejects any data older than a configurable threshold (default 5s) per instrument per venue
- [x] **RELY-04**: Feed drops degrade gracefully — remaining feeds continue operating, affected instruments marked unavailable, degraded state surfaced in metrics
- [ ] **RELY-05**: Per-venue rate limiters enforce API rate limits (Deribit 20 req/s private, Kalshi tiered, Polymarket gas-aware) baked into feed and future execution layers
- [x] **RELY-06**: System shuts down gracefully on SIGINT/SIGTERM — clean WS disconnect, flush pending writes, complete in-flight computations

### Event Mapping

- [ ] **EVNT-01**: Config-driven event registry (TOML) maps equivalent instruments across Polymarket, Kalshi, and Deribit using structured fields (asset, strike, expiry, direction)
- [x] **EVNT-02**: Settlement basis analyzer quantifies per-mapping: expiry/settlement time differences, settlement source differences, resolution criteria differences, producing a basis_risk_score
- [x] **EVNT-03**: Expiry alignment validation quantifies temporal mismatch between options expiry (Deribit Friday 08:00 UTC) and prediction market resolution as basis risk
- [ ] **EVNT-04**: Contract lifecycle manager continuously discovers new contracts, detects expiring/expired ones, and handles Deribit expiry rolls — not just at startup
- [x] **EVNT-05**: Contracts approaching expiry receive special handling flags (pricing character change, liquidity warnings, elevated settlement risk)

### Pricing Engine

- [x] **PRIC-01**: Implied volatility solver extracts IV from Deribit option mid-prices using Newton-Raphson or Brent's method with Black-76 model
- [x] **PRIC-02**: IV solver handles edge cases: deep ITM/OTM options, near-expiry theta collapse, negative time value
- [x] **PRIC-03**: Probability extractor computes P(S > K) using multiple methods: naive N(d2), strike-specific vol N(d2), call spread replication, and full smile interpolation
- [x] **PRIC-04**: Call spread replication `(C(K-e) - C(K+e)) / 2e` is the primary digital pricing method, producing skew-adjusted probabilities
- [x] **PRIC-05**: Implied volatility surface construction interpolates across strikes for pricing at non-traded strikes
- [x] **PRIC-06**: Each ImpliedProbability output includes: probability value, confidence (based on bid-ask width/depth), pricing method used, skew adjustment factor, and timestamp
- [x] **PRIC-07**: Greeks calculator computes delta, gamma, vega, theta for position monitoring and downstream risk assessment

### Signal Generation

- [x] **SGNL-01**: Spread calculator computes spread between prediction market price and options-implied probability for each mapped event
- [x] **SGNL-02**: Spread calculation adjusts for: transaction fees (Deribit maker/taker, Polymarket dynamic fees up to ~1.56% at 50/50, Kalshi 7% profit fee), slippage estimate from available depth, funding/carry cost, settlement basis risk premium
- [x] **SGNL-03**: Every spread calculation validates both sides are fresh (staleness gate) and rejects with logging if either side exceeds threshold
- [x] **SGNL-04**: Cross-platform prediction market spread detection (Polymarket vs Kalshi) for 4 patterns: Poly YES + Kalshi NO, inverse, and each direction
- [x] **SGNL-05**: Signal generation produces ArbSignal with: event ID, direction, raw spread, net edge after costs, confidence, constituent legs, timestamp, and TTL
- [x] **SGNL-06**: Configurable minimum edge threshold after all costs, with dynamic thresholds based on volatility regime and available liquidity
- [x] **SGNL-07**: Every spread computation logged to file (not just signals above threshold) for distribution analysis, regime detection, and threshold tuning
- [x] **SGNL-08**: Periodic aggregate spread statistics (mean, stddev, percentiles) emitted to metrics and stdout

### Clock & Timestamps

- [ ] **TIME-01**: Spread calculator reasons about when each price was valid, not just what it was — using exchange-reported timestamps where available
- [x] **TIME-02**: All logged data includes both local receipt timestamp and exchange-reported timestamp for post-hoc latency analysis
- [x] **TIME-03**: Per-feed latency characteristics are documented and tracked in metrics (exchange_ts vs local_ts delta)

### Observability

- [x] **OBSV-01**: All parameters configurable via TOML: strike filters, staleness thresholds, fee assumptions, signal thresholds, log rotation, venue credentials
- [x] **OBSV-02**: Structured logging via `tracing` with JSON output, including correlation IDs linking signals to their constituent market data
- [x] **OBSV-03**: Prometheus metrics exporter with key metrics: spread by event (histogram), signal count, fill rate proxy, feed-to-signal latency, feed health, margin utilization proxy
- [x] **OBSV-04**: Paper trade P&L tracking: hypothetical entry/exit at signal time, per-signal P&L assuming fill at quoted price, daily/weekly aggregates
- [x] **OBSV-05**: HTTP `/health` endpoint reporting: per-feed connection status, last update time per feed, active event count, system uptime
- [x] **OBSV-06**: JSONL schema for all recorded data (feeds, spreads, signals, P&L) is stable and documented for offline analysis tooling (Python/Jupyter)

### Development & Testing

- [x] **TEST-01**: Mock data layer via trait-based abstraction over data sources — full pipeline runnable without live venue connections
- [x] **TEST-02**: Deterministic replay from recorded JSONL feeds through the full pipeline with identical computation
- [x] **TEST-03**: Feed recordings serve as replay corpus for backtesting and debugging pricing discrepancies

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Execution

- **EXEC-01**: Order execution engine placing orders on both venues concurrently via tokio::join!
- **EXEC-02**: Venue-specific executor adapters implementing common VenueExecutor trait (Deribit, Polymarket, Kalshi)
- **EXEC-03**: Venue authentication for private/trading endpoints (API keys, wallet signing for Polymarket)
- **EXEC-04**: Order state machine per arb trade: Pending -> PartialFill -> Filled -> Settled with timeout transitions
- **EXEC-05**: Leg risk manager: retry with adjusted price on second-leg failure, evaluate hedging vs unwinding, configurable max one-leg exposure

### Risk Management

- **RISK-01**: Real-time P&L per event, per venue, and aggregate with mark-to-market
- **RISK-02**: Limits engine: max notional per event/venue/total, max open positions, max one-leg exposure
- **RISK-03**: Kill switch: flatten all positions if aggregate loss exceeds threshold
- **RISK-04**: Margin monitor: track available margin on Deribit, USDC balance on Polymarket, alert on threshold breach

### Extended Analytics

- **ANLT-01**: Signal quality scoring (hit rate, average P&L, Sharpe ratio, max drawdown)
- **ANLT-02**: Historical spread analytics (time-of-day, event-type, liquidity regime patterns)
- **ANLT-03**: Configurable alert thresholds with Telegram/webhook notifications

### Multi-Asset

- **MAST-01**: ETH binary event support
- **MAST-02**: SOL and other assets with liquid options on Deribit

## Out of Scope

| Feature | Reason |
|---------|--------|
| Live order execution | v2 -- must prove signal quality through paper trading first |
| AI/ML signal prediction | Overfitting risk on small dataset; arbs are event-driven not pattern-driven |
| Multi-chain/DEX arbitrage | Fundamentally different problem domain (AMM math, MEV, block-time execution) |
| Web dashboard / custom frontend | Use Grafana + Prometheus; zero custom frontend code |
| Sub-millisecond latency optimization | Arb windows are minutes-to-hours; bottleneck is pricing accuracy, not speed |
| Simultaneous support for many prediction markets | Start with Poly + Kalshi + Deribit; add venues only when these are solid |
| Full portfolio management | Different product; single-strategy focus for now |
| State persistence / restart reconciliation | v2 -- paper trading can afford cold restarts |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| FEED-01 | Phase 2 | Pending |
| FEED-02 | Phase 2 | Pending |
| FEED-03 | Phase 13 | Complete |
| FEED-04 | Phase 13 | Complete |
| FEED-05 | Phase 13 | Complete |
| FEED-06 | Phase 2 | Pending |
| FEED-07 | Phase 2 | Pending |
| FEED-08 | Phase 12 | Complete |
| RELY-01 | Phase 3 | Pending |
| RELY-02 | Phase 12 | Complete |
| RELY-03 | Phase 3 | Pending |
| RELY-04 | Phase 13 | Complete |
| RELY-05 | Phase 3 | Pending |
| RELY-06 | Phase 1 | Complete |
| EVNT-01 | Phase 5 | Pending |
| EVNT-02 | Phase 11 | Complete |
| EVNT-03 | Phase 11 | Complete |
| EVNT-04 | Phase 5 | Pending |
| EVNT-05 | Phase 11 | Complete |
| PRIC-01 | Phase 7 | Complete |
| PRIC-02 | Phase 7 | Complete |
| PRIC-03 | Phase 7 | Complete |
| PRIC-04 | Phase 7 | Complete |
| PRIC-05 | Phase 7 | Complete |
| PRIC-06 | Phase 7 | Complete |
| PRIC-07 | Phase 7 | Complete |
| SGNL-01 | Phase 8 | Complete |
| SGNL-02 | Phase 11 | Complete |
| SGNL-03 | Phase 6 | Complete |
| SGNL-04 | Phase 6 | Complete |
| SGNL-05 | Phase 10 | Complete |
| SGNL-06 | Phase 8 | Complete |
| SGNL-07 | Phase 6 | Complete |
| SGNL-08 | Phase 6 | Complete |
| TIME-01 | Phase 3 | Pending |
| TIME-02 | Phase 12 | Complete |
| TIME-03 | Phase 12 | Complete |
| OBSV-01 | Phase 10 | Complete |
| OBSV-02 | Phase 1 | Complete |
| OBSV-03 | Phase 6 | Complete |
| OBSV-04 | Phase 10 | Complete |
| OBSV-05 | Phase 9 | Complete |
| OBSV-06 | Phase 9 | Complete |
| TEST-01 | Phase 13 | Complete |
| TEST-02 | Phase 9 | Complete |
| TEST-03 | Phase 9 | Complete |

**Coverage:**
- v1 requirements: 46 total
- Mapped to phases: 46
- Unmapped: 0

---
*Requirements defined: 2026-02-21*
*Last updated: 2026-02-24 after gap closure phase creation (phases 10-13)*
