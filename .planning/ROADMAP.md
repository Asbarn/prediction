# Roadmap: Prediction Market Arbitrage System

## Overview

This roadmap delivers a production-grade cross-venue arbitrage signal generator in Rust. The system progresses from foundational types and configuration through single-feed data pipeline validation, feed hardening, multi-venue expansion, event mapping, prediction market spread detection, options pricing, cross-asset signal generation, and finally replay/analytics hardening. Each phase delivers a coherent, verifiable capability that builds on the previous, culminating in a paper trading system that detects pricing discrepancies between crypto prediction markets and options-implied probabilities.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Foundation** - Project skeleton with shared types, configuration, structured logging, and graceful shutdown
- [x] **Phase 2: Deribit Feed and Data Pipeline** - End-to-end data path from Deribit WebSocket through normalization bus to JSONL recording, with mock data abstraction
- [x] **Phase 3: Feed Infrastructure** - Reliability layer with automatic reconnection, heartbeat monitoring, staleness detection, rate limiting, and timestamp tracking
- [x] **Phase 4: Multi-Venue Feeds** - Polymarket and Kalshi feed integration with graceful degradation on feed loss
- [x] **Phase 5: Event Mapping** - Cross-venue instrument registry with settlement basis risk scoring and contract lifecycle management
- [ ] **Phase 6: Prediction Market Spreads** - Cross-platform spread detection between prediction markets with fee-adjusted costs, paper P&L tracking, and Prometheus metrics
- [ ] **Phase 7: Options Pricing Engine** - Black-76 pricing, IV solver, call spread replication for digital payoffs, vol surface construction, and Greeks
- [ ] **Phase 8: Cross-Asset Signal Generation** - Full spread calculation between options-implied probabilities and prediction market prices with signal generation and threshold engine
- [ ] **Phase 9: Replay and Hardening** - Deterministic replay from recorded feeds, stable JSONL schema, and health endpoint for operational monitoring

## Phase Details

### Phase 1: Foundation
**Goal**: The project compiles and runs as a single binary with TOML-driven configuration, structured JSON logging, and clean shutdown behavior -- establishing the shared types and infrastructure every subsequent phase imports.
**Depends on**: Nothing (first phase)
**Requirements**: RELY-06, OBSV-01, OBSV-02
**Success Criteria** (what must be TRUE):
  1. Running `cargo run` produces a binary that starts, logs a structured JSON startup message, and exits cleanly on SIGINT/SIGTERM
  2. All configuration parameters (venue credentials, strike filters, staleness thresholds, fee assumptions, signal thresholds, log rotation) load from a TOML file, and the binary refuses to start with invalid config
  3. Log output is structured JSON with tracing spans, log levels, and correlation ID infrastructure ready for downstream use
  4. Shared domain types (Venue, Instrument, MarketSnapshot, Decimal price/probability wrappers, Timestamp, error types) compile and are importable by downstream modules
  5. Graceful shutdown on SIGINT/SIGTERM flushes pending writes and completes in-flight computations before exit
**Plans**: 3 plans

Plans:
- [x] 01-01-PLAN.md -- Project scaffold with dependencies, shared domain types, and error types
- [x] 01-02-PLAN.md -- Configuration loading system and dual-output structured logging
- [x] 01-03-PLAN.md -- Graceful shutdown, config hot-reload, binary entrypoint, and integration tests

### Phase 2: Deribit Feed and Data Pipeline
**Goal**: The system connects to Deribit, maintains a live order book, publishes normalized MarketSnapshot events through a bounded async channel, records every raw message to JSONL, and supports a mock data source for testing -- proving the entire data pipeline end-to-end with a single venue.
**Depends on**: Phase 1
**Requirements**: FEED-01, FEED-02, FEED-06, FEED-07, FEED-08, TEST-01
**Success Criteria** (what must be TRUE):
  1. System connects to Deribit WebSocket, subscribes to book and ticker channels via JSON-RPC 2.0, and receives live market data
  2. Local order book applies incremental deltas correctly, producing accurate bid/ask/depth snapshots that match exchange state
  3. Every raw WebSocket message is recorded to line-delimited JSON with local receive timestamp and venue identifier
  4. Normalized MarketSnapshot events (venue, instrument, bid/ask probability, depth, timestamps, sequence numbers) flow through a bounded async channel to downstream consumers
  5. The full pipeline runs identically against a mock data source (trait-based abstraction) without any live venue connection, enabling development and testing offline
**Plans**: 4 plans

Plans:
- [x] 02-01-PLAN.md -- Feed traits, Deribit message types, channel routing, and WebSocket client
- [x] 02-02-PLAN.md -- Order book state management and MarketSnapshot normalization pipeline
- [x] 02-03-PLAN.md -- JSONL recording pipeline with daily rotation and non-blocking writes
- [x] 02-04-PLAN.md -- Mock data layer (replay + synthetic), pipeline assembly, and main.rs integration

### Phase 3: Feed Infrastructure
**Goal**: The Deribit feed operates reliably in production conditions -- surviving connection drops, detecting dead connections vs quiet markets, rejecting stale data, respecting API rate limits, and tracking latency characteristics for every message.
**Depends on**: Phase 2
**Requirements**: RELY-01, RELY-02, RELY-03, RELY-05, TIME-01, TIME-02, TIME-03
**Success Criteria** (what must be TRUE):
  1. When the WebSocket connection drops, the feed automatically reconnects with exponential backoff and jitter, resuming data flow without operator intervention
  2. Heartbeat monitoring distinguishes "quiet market with no trades" from "dead connection with no messages" and triggers reconnection only for genuinely dead connections
  3. Any market data older than the configurable staleness threshold (default 5s) is rejected with a log entry, never passed downstream
  4. API rate limits (Deribit 20 req/s private) are enforced by a per-venue rate limiter, preventing throttling or ban
  5. Every logged data point includes both local receipt timestamp and exchange-reported timestamp, and per-feed latency characteristics (exchange_ts vs local_ts delta) are tracked in metrics
**Plans**: 3 plans

Plans:
- [x] 03-01-PLAN.md -- Config extensions (reconnect, staleness), heartbeat message types, bidirectional WS client with heartbeat protocol
- [x] 03-02-PLAN.md -- Per-instrument staleness gate, latency metrics via metrics crate, periodic flush for recording writer
- [x] 03-03-PLAN.md -- Reconnection supervisor with exponential backoff, per-venue rate limiter, pipeline integration

### Phase 4: Multi-Venue Feeds
**Goal**: Polymarket and Kalshi feeds are operational alongside Deribit, all publishing normalized MarketSnapshot events through the same channel, with the system continuing to function when any individual feed drops.
**Depends on**: Phase 3
**Requirements**: FEED-03, FEED-04, FEED-05, RELY-04
**Success Criteria** (what must be TRUE):
  1. System connects to Polymarket CLOB WebSocket and receives order book updates for target condition IDs, normalized from probability space (0-1) with bid/ask/depth
  2. System connects to Kalshi (REST polling or WebSocket) and normalizes contracts into probability + expiry schema matching the unified MarketSnapshot format
  3. All three venue feeds publish through the same bounded async channel, and downstream consumers process events from any venue identically
  4. When any single feed drops, remaining feeds continue operating -- affected instruments are marked unavailable, degraded state is surfaced in metrics, and the system does not crash or stall
**Plans**: 3 plans

Plans:
- [x] 04-01-PLAN.md -- Polymarket CLOB WebSocket client, message types, probability normalization, and reconnection supervisor
- [x] 04-02-PLAN.md -- Kalshi RSA-PSS auth, WebSocket client, incremental order book, cents-to-probability normalization, and supervisor
- [x] 04-03-PLAN.md -- Multi-feed fan-in with shared mpsc channel, per-venue health tracking, graceful degradation, and main.rs integration

### Phase 5: Event Mapping
**Goal**: Equivalent instruments across Polymarket, Kalshi, and Deribit are mapped together through a config-driven registry, with each mapping carrying quantified settlement basis risk and lifecycle status, enabling downstream spread calculations to compare the right instruments.
**Depends on**: Phase 4
**Requirements**: EVNT-01, EVNT-02, EVNT-03, EVNT-04, EVNT-05
**Success Criteria** (what must be TRUE):
  1. A TOML-driven event registry maps equivalent instruments across all three venues using structured fields (asset, strike, expiry, direction), and the mapping is queryable at runtime
  2. Each mapping carries a computed basis_risk_score quantifying: settlement time differences (Deribit Friday 08:00 UTC vs prediction market resolution), settlement source differences (index price vs oracle), and resolution criteria differences
  3. The contract lifecycle manager continuously discovers new contracts, detects expiring/expired ones, and handles Deribit expiry rolls -- not just at startup
  4. Contracts approaching expiry receive special handling flags (pricing character change warnings, liquidity warnings, elevated settlement risk) that downstream consumers can act on
**Plans**: 3 plans

Plans:
- [x] 05-01-PLAN.md -- Extended EventsConfig schema with approval/lifecycle/settlement fields, EventRegistry with dual-index lookup, format-preserving TOML writer
- [x] 05-02-PLAN.md -- Settlement basis risk scoring (time/source/criteria components) and near-expiry warning system with configurable tiers
- [x] 05-03-PLAN.md -- Per-venue REST discovery, cross-venue candidate matching, ContractLifecycleManager with expiry rolls, main.rs integration

### Phase 6: Prediction Market Spreads
**Goal**: The system detects cross-platform prediction market arbitrage (Polymarket vs Kalshi), computes fee-adjusted net spreads, logs every computation for analysis, tracks hypothetical paper trade P&L, and exports key metrics to Prometheus -- delivering the first actionable trading signals.
**Depends on**: Phase 5
**Requirements**: SGNL-02, SGNL-03, SGNL-04, SGNL-07, SGNL-08, OBSV-03, OBSV-04
**Success Criteria** (what must be TRUE):
  1. Cross-platform spread detection identifies all 4 patterns (Poly YES + Kalshi NO, Poly NO + Kalshi YES, and each inverse direction) for every mapped event pair
  2. Spread calculations adjust for all transaction costs (Polymarket dynamic fees up to ~1.56% at 50/50, Kalshi 7% profit fee, slippage from available depth, funding/carry cost, settlement basis risk premium) and both sides must pass the staleness gate
  3. Every spread computation (not just signals above threshold) is logged to file for distribution analysis, regime detection, and threshold tuning
  4. Periodic aggregate spread statistics (mean, stddev, percentiles) are emitted to Prometheus metrics and stdout
  5. Paper trade P&L tracking records hypothetical entry/exit at signal time, computes per-signal P&L assuming fill at quoted price, and produces daily/weekly aggregates
**Plans**: 4 plans

Plans:
- [x] 06-01-PLAN.md -- Spread config, cost model (Polymarket/Kalshi fees), book walker, rolling statistics
- [x] 06-02-PLAN.md -- Prometheus metrics exporter setup, SpreadPattern enum and SpreadResult types
- [x] 06-03-PLAN.md -- SpreadEngine with staleness gate, 4-pattern detection, JSONL logging, dynamic threshold
- [ ] 06-04-PLAN.md -- Paper trade P&L tracker (next-tick entry, MTM, daily rollups) and main.rs integration

### Phase 7: Options Pricing Engine
**Goal**: The system extracts implied probabilities from Deribit options data using rigorous quantitative methods -- IV solving, multiple probability extraction methods with call spread replication as primary, vol surface interpolation, and Greeks -- producing ImpliedProbability outputs that carry method, confidence, and skew adjustment metadata.
**Depends on**: Phase 2 (Deribit feed data), Phase 5 (event mapping for strike/expiry context)
**Requirements**: PRIC-01, PRIC-02, PRIC-03, PRIC-04, PRIC-05, PRIC-06, PRIC-07
**Success Criteria** (what must be TRUE):
  1. IV solver extracts implied volatility from Deribit option mid-prices using Newton-Raphson or Brent's method with Black-76, handling edge cases (deep ITM/OTM, near-expiry theta collapse, negative time value)
  2. Probability extractor computes P(S > K) via multiple methods (naive N(d2), strike-specific vol N(d2), call spread replication, smile interpolation) with call spread replication as the primary method
  3. Implied volatility surface interpolates across strikes, enabling pricing at non-traded strikes needed for call spread replication epsilon offsets
  4. Each ImpliedProbability output includes: probability value, confidence (based on bid-ask width/depth), pricing method used, skew adjustment factor, and timestamp
  5. Greeks calculator (delta, gamma, vega, theta) produces values for each priced instrument, ready for downstream position monitoring
**Plans**: TBD

Plans:
- [ ] 07-01: Black-76 model and IV solver (Newton-Raphson/Brent)
- [ ] 07-02: Digital probability extraction methods
- [ ] 07-03: Implied volatility surface construction
- [ ] 07-04: Greeks calculator
- [ ] 07-05: ImpliedProbability output assembly with metadata

### Phase 8: Cross-Asset Signal Generation
**Goal**: The system computes spreads between options-implied probabilities and prediction market prices for each mapped event, generates ArbSignal outputs with full metadata, and applies configurable edge thresholds with dynamic adjustment -- completing the core arbitrage detection pipeline.
**Depends on**: Phase 6 (spread infrastructure, cost model), Phase 7 (options-implied probabilities)
**Requirements**: SGNL-01, SGNL-05, SGNL-06
**Success Criteria** (what must be TRUE):
  1. Spread calculator computes the spread between each prediction market price and its corresponding options-implied probability for every mapped event, using the pricing engine output
  2. Signal generation produces ArbSignal outputs with: event ID, direction, raw spread, net edge after costs, confidence, constituent legs, timestamp, and TTL
  3. Configurable minimum edge threshold after all costs filters signals, with dynamic thresholds that adjust based on volatility regime and available liquidity
**Plans**: TBD

Plans:
- [ ] 08-01: Cross-asset spread calculator
- [ ] 08-02: ArbSignal generation with full metadata
- [ ] 08-03: Dynamic threshold engine

### Phase 9: Replay and Hardening
**Goal**: The system supports deterministic replay from recorded feed data, exposes a health endpoint for operational monitoring, and stabilizes the JSONL schema for offline analysis -- turning accumulated data into a validated testing and analysis corpus.
**Depends on**: Phase 6 (recorded feed data accumulated over time), Phase 8 (full pipeline to replay through)
**Requirements**: OBSV-05, OBSV-06, TEST-02, TEST-03
**Success Criteria** (what must be TRUE):
  1. Deterministic replay from recorded JSONL feeds produces identical computation results when run through the full pipeline, enabling backtesting and debugging
  2. Feed recordings serve as a reusable replay corpus -- any historical period can be replayed to reproduce signals and investigate pricing discrepancies
  3. HTTP /health endpoint reports: per-feed connection status, last update time per feed, active event count, and system uptime
  4. JSONL schema for all recorded data (feeds, spreads, signals, P&L) is stable and documented, enabling offline analysis with Python/Jupyter tooling
**Plans**: TBD

Plans:
- [ ] 09-01: Deterministic replay engine
- [ ] 09-02: Health endpoint
- [ ] 09-03: JSONL schema stabilization and documentation

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 3/3 | Complete | 2026-02-22 |
| 2. Deribit Feed and Data Pipeline | 4/4 | Complete | 2026-02-22 |
| 3. Feed Infrastructure | 3/3 | Complete | 2026-02-22 |
| 4. Multi-Venue Feeds | 3/3 | Complete | 2026-02-22 |
| 5. Event Mapping | 3/3 | Complete    | 2026-02-22 |
| 6. Prediction Market Spreads | 0/4 | Not started | - |
| 7. Options Pricing Engine | 0/5 | Not started | - |
| 8. Cross-Asset Signal Generation | 0/3 | Not started | - |
| 9. Replay and Hardening | 0/3 | Not started | - |
