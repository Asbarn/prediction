# Prediction Market Arbitrage System

## What This Is

A production-grade cross-venue arbitrage system in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit). It compares a prediction market binary contract's price against the equivalent implied probability derived from options pricing, generates trading signals when the spread exceeds costs, and logs everything for analysis. Built as a single-binary Linux service for a solo trader.

## Core Value

Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities — with every false signal caught before it costs money.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Deribit WebSocket feed with order book maintenance and JSON-RPC 2.0 parsing
- [ ] Polymarket CLOB WebSocket feed with probability-space order books
- [ ] Kalshi feed (REST polling or WebSocket) normalized to same schema
- [ ] Unified MarketSnapshot bus via bounded async channels
- [ ] Raw feed recording: every WebSocket message to line-delimited JSON with timestamps
- [ ] Event registry mapping equivalent instruments across venues (config-driven via TOML)
- [ ] Settlement basis analyzer quantifying expiry/oracle/resolution differences per mapping
- [ ] Implied volatility solver (Newton-Raphson/Brent) for Black-76 options pricing
- [ ] Probability extractor: N(d2), call spread replication, and smile interpolation methods
- [ ] Greeks calculator (delta, gamma, vega, theta) for position monitoring
- [ ] Spread calculator with transaction cost, slippage, funding, and basis risk adjustments
- [ ] Staleness detection: reject spread calculations where either side exceeds configurable threshold (default 5s)
- [ ] Signal generation with configurable edge thresholds
- [ ] Continuous spread logging: every computation to file, periodic aggregates to metrics
- [ ] Prometheus metrics exporter (spreads, signal count, latency, feed health)
- [ ] Structured logging via `tracing` with JSON output and correlation IDs
- [ ] Mock/replay data layer for development and backtesting alongside real feeds
- [ ] Config-driven everything: strikes, staleness, fees, thresholds, rotation — all in TOML
- [ ] Graceful degradation: feed drops don't crash the system, partial observability maintained

### Out of Scope

- Order execution / trade placement — v2 after paper trading validation
- Venue API authentication for private/trading endpoints — v2
- Position tracking and P&L — v2
- Risk limits engine and kill switch — v2
- Margin monitoring — v2
- Multi-asset support (ETH, SOL) — after BTC binary events validated
- State persistence / restart reconciliation — v2
- UI / dashboard — solo trader monitors via logs and metrics

## Context

**Arbitrage thesis:** A Polymarket contract like "BTC above $100K by June 30" at $0.42 implies 42% probability. Deribit BTC options with strike $100K and similar expiry imply a different probability via options pricing. When these diverge beyond transaction costs, there's an arbitrage. The system detects these opportunities across venues.

**Starting scope:** BTC binary price-threshold events only. Three venues: Deribit (options), Polymarket (prediction), Kalshi (prediction).

**Key challenge — digital pricing:** Naive N(d2) is biased for binary/digital payoffs due to volatility skew. Call spread replication `(C(K-ε) - C(K+ε)) / 2ε` is more robust. The pricing engine must support multiple methods and track which method produced each probability estimate.

**Key challenge — staleness:** Around major events (FOMC, ETF decisions), prediction markets can move 10%+ in seconds while options markets lag. This creates apparent arb that's actually stale pricing. Staleness detection is more important than latency.

**Key challenge — settlement basis risk:** Expiry conventions differ (Deribit settles 08:00 UTC, prediction markets often resolve "end of day" in ambiguous timezones). Settlement sources differ (Deribit index price vs. Polymarket UMA oracle). These differences are real risk that must be quantified per event mapping.

**Development approach:** Mock data layer for testing + real feeds as API access becomes available. Feed recording from day one builds the replay corpus for backtesting and debugging.

**Paper trading goal:** Log signals without executing for extended period. Measure theoretical edge, spread distributions, mean-reversion characteristics. Use spread distributions to set sensible thresholds and detect regime changes.

## Constraints

- **Language**: Rust (latest stable, 2024 edition) — performance and correctness requirements
- **Async runtime**: tokio — all I/O is async
- **Decimal arithmetic**: `rust_decimal` — never f64 for prices or probabilities
- **Deployment**: Single-binary Linux service, <1ms internal processing latency target
- **Deribit API**: 20 req/s rate limit on private endpoints; batch where possible
- **Polymarket**: On-chain (Polygon) — gas, wallet management, approvals matter for v2 execution
- **Kalshi**: US-regulated, different API semantics and fee structures
- **Options liquidity**: Concentrated near ATM on Deribit; far OTM/ITM strikes have wide spreads

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Paper trading before execution | Validate signal quality, measure spread distributions, tune thresholds before risking capital | — Pending |
| BTC-only initially | Highest liquidity on both prediction markets and Deribit options; validate approach before expanding | — Pending |
| Line-delimited JSON for feed recording | Easy to grep/parse with standard tools (jq, grep); human-readable for debugging | — Pending |
| Call spread replication as primary digital pricing method | More robust than naive N(d2) under skew; less sensitive to smile interpolation errors | — Pending |
| Config-driven via TOML | Constant tuning expected during paper trading; no recompilation for parameter changes | — Pending |
| Mock + real data layers | Enables development without API keys; feed recordings become replay corpus for testing | — Pending |

---
*Last updated: 2026-02-21 after initialization*
