# Prediction Market Arbitrage System

## What This Is

A production-grade cross-venue arbitrage signal generator in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit). Compares prediction market binary contract prices against options-implied probabilities derived via Black-76 pricing with call spread replication, generates trading signals when spreads exceed cost-adjusted thresholds, and logs everything for analysis. Built as a single-binary service for a solo trader with 22,751 lines of Rust, 417+ tests, and full deterministic replay capability.

## Core Value

Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## Requirements

### Validated

- v1.0 Deribit WebSocket feed with order book maintenance and JSON-RPC 2.0 parsing
- v1.0 Polymarket CLOB WebSocket feed with probability-space order books
- v1.0 Kalshi feed (WebSocket) normalized to same schema
- v1.0 Unified MarketSnapshot bus via bounded async channels
- v1.0 Raw feed recording to line-delimited JSON with timestamps
- v1.0 Event registry mapping equivalent instruments across venues (TOML-driven)
- v1.0 Settlement basis analyzer quantifying expiry/oracle/resolution differences
- v1.0 IV solver (Newton-Raphson/Brent) for Black-76 options pricing
- v1.0 Probability extractor: N(d2), call spread replication, smile interpolation
- v1.0 Greeks calculator (delta, vega, theta) for position monitoring
- v1.0 Spread calculator with cost, slippage, funding, and basis risk adjustments
- v1.0 Staleness detection rejecting stale data per configurable threshold
- v1.0 Signal generation with dynamic edge thresholds
- v1.0 Continuous spread logging and periodic aggregate metrics
- v1.0 Prometheus metrics exporter
- v1.0 Structured logging via tracing with JSON output and correlation IDs
- v1.0 Mock/replay data layer for development and backtesting
- v1.0 Config-driven TOML for all parameters
- v1.0 Graceful degradation on feed drops
- v1.0 Deterministic replay from recorded feeds
- v1.0 HTTP /health endpoint
- v1.0 Paper trade P&L tracking
- v1.0 Contract lifecycle management with expiry rolls
- v1.0 Per-venue heartbeat monitoring and reconnection supervisors

### Active

<!-- Current scope: v1.1 Paper Trading Validation -->

- [ ] Settlement outcome tracking from prediction markets and options expirations
- [ ] Signal analysis tooling (hit rate, edge, false positive rate, time-to-convergence)
- [ ] Failure alerting for degraded states (stale data, partial feeds, silent failures)
- [ ] Minimal file-based state persistence for paper P&L and signal history

### Out of Scope

- Order execution / trade placement -- v2 after paper trading validation
- Venue API authentication for private/trading endpoints -- v2
- Real-time P&L and position tracking -- v2
- Risk limits engine and kill switch -- v2
- Margin monitoring -- v2
- Multi-asset support (ETH, SOL) -- after BTC binary events validated
- State persistence / restart reconciliation -- v2
- UI / dashboard -- solo trader monitors via logs and metrics
- AI/ML signal prediction -- arbs are event-driven, not pattern-driven
- Sub-millisecond latency -- arb windows are minutes-to-hours

## Context

**Shipped v1.0 MVP** (2026-02-24) with 22,751 LOC Rust across 13 phases.
Tech stack: Rust (2024 edition), tokio, rust_decimal, serde, axum, metrics/prometheus, statrs, tracing.
417+ tests passing (unit, integration, pipeline, smoke, doc, schema golden).
3 venues operational: Deribit (WebSocket), Polymarket (CLOB WebSocket), Kalshi (WebSocket).

**Paper trading phase:** System is ready for extended paper trading to validate signal quality, measure spread distributions, and tune thresholds before risking capital. Risk premium calibration needs 2-4 weeks of parallel data collection.

**Known tech debt:** 13 non-blocking items carried from v1.0 (iv_spread metadata always 0.0, expired test instrument in config, empty Kalshi default market list, options book_depth_levels hardcoded). See MILESTONES.md for full list.

## Constraints

- **Language**: Rust (latest stable, 2024 edition)
- **Async runtime**: tokio
- **Decimal arithmetic**: `rust_decimal` for all prices and probabilities
- **Deployment**: Single-binary Linux service
- **Deribit API**: 20 req/s rate limit on private endpoints
- **Polymarket**: On-chain (Polygon) -- gas, wallet, approvals matter for v2 execution
- **Kalshi**: US-regulated, different API semantics and fee structures

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Paper trading before execution | Validate signal quality before risking capital | v1.0 Validated -- system ready for paper trading |
| BTC-only initially | Highest liquidity across all venues | v1.0 Validated -- BTC pipeline complete |
| JSONL feed recording | Easy to grep/parse; human-readable for debugging | v1.0 Validated -- stable schema, golden tests |
| Call spread replication as primary digital pricing | More robust than naive N(d2) under volatility skew | v1.0 Validated -- primary method with confidence scoring |
| Config-driven via TOML | No recompilation for parameter changes during tuning | v1.0 Validated -- all parameters configurable |
| Mock + real data layers | Development without API keys; recordings become replay corpus | v1.0 Validated -- deterministic replay operational |
| 9-phase structure (expanded from 6) | Clearer delivery boundaries (reliability separate from connection, etc.) | v1.0 Validated -- clean incremental delivery |
| Deribit feed first | Proves pipeline architecture before multi-venue complexity | v1.0 Validated -- architecture held through 3 venues |
| Prediction market arb before cross-asset | Validates pipeline with simpler math before Black-76 | v1.0 Validated -- both spread engines operational |
| Gamma omitted from Greeks | User decision: delta/vega/theta sufficient for paper trading | v1.0 Accepted |
| Flat extrapolation for vol surface | Returns boundary IV rather than None for extreme strikes | v1.0 Validated -- graceful degradation |
| Non-blocking try_send for secondary engines | Primary engine (SpreadEngine) blocking, others best-effort | v1.0 Validated -- no pipeline stalls |
| BasisRiskCache with try_read | Never blocks engine hot path; zero premium on lock contention | v1.0 Validated -- no measurable latency impact |

## Current Milestone: v1.1 Paper Trading Validation

**Goal:** Prove signal quality is real and the system is operationally trustworthy enough for extended unattended paper trading.

**Target features:**
- Settlement outcome tracking from venues, compared against generated signals
- Signal analysis tools: hit rate, edge measurement, false positive rate, time-to-convergence
- Failure alerting beyond reconnection — detect stale data, partial feeds, silent degradation
- Minimal file-based state persistence for paper P&L and signal history

---
*Last updated: 2026-02-24 after v1.1 milestone started*
