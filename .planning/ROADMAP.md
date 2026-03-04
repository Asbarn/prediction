# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (shipped 2026-02-27) | [Full details](milestones/v1.2-ROADMAP.md)
- v1.3 Live Subscription Management -- Phases 22-25 (shipped 2026-02-28) | [Full details](milestones/v1.3-ROADMAP.md)
- v1.4 Analysis Tooling -- Phases 26-29 (shipped 2026-03-02) | [Full details](milestones/v1.4-ROADMAP.md)
- v1.5 Derive.xyz Venue Integration -- Phases 30-33 (in progress) | [Full details](milestones/v1.5-ROADMAP.md)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-13) -- SHIPPED 2026-02-24</summary>

- [x] Phase 1: Foundation (3/3 plans) -- completed 2026-02-22
- [x] Phase 2: Deribit Feed and Data Pipeline (4/4 plans) -- completed 2026-02-22
- [x] Phase 3: Feed Infrastructure (3/3 plans) -- completed 2026-02-22
- [x] Phase 4: Multi-Venue Feeds (3/3 plans) -- completed 2026-02-22
- [x] Phase 5: Event Mapping (3/3 plans) -- completed 2026-02-22
- [x] Phase 6: Prediction Market Spreads (4/4 plans) -- completed 2026-02-23
- [x] Phase 7: Options Pricing Engine (5/5 plans) -- completed 2026-02-23
- [x] Phase 8: Cross-Asset Signal Generation (2/2 plans) -- completed 2026-02-23
- [x] Phase 9: Replay and Hardening (3/3 plans) -- completed 2026-02-23
- [x] Phase 10: Critical Pipeline Wiring (1/1 plan) -- completed 2026-02-24
- [x] Phase 11: BasisRiskScore Downstream Consumption (2/2 plans) -- completed 2026-02-24
- [x] Phase 12: Kalshi Feed Hardening (1/1 plan) -- completed 2026-02-24
- [x] Phase 13: Phase 4 Verification & Cleanup (2/2 plans) -- completed 2026-02-24

</details>

<details>
<summary>v1.1 Paper Trading Validation (Phases 14-17) -- SHIPPED 2026-02-26</summary>

- [x] Phase 14: Failure Alerting (2/2 plans) -- completed 2026-02-24
- [x] Phase 15: State Persistence (2/2 plans) -- completed 2026-02-24
- [x] Phase 16: Settlement Outcome Tracking (4/4 plans) -- completed 2026-02-26
- [x] Phase 17: Signal Analysis Tooling (3/3 plans) -- completed 2026-02-26

</details>

<details>
<summary>v1.2 Automated Event Management (Phases 18-21) -- SHIPPED 2026-02-27</summary>

- [x] Phase 18: Discovery Infrastructure Hardening (2/2 plans) -- completed 2026-02-26
- [x] Phase 19: Polymarket Discovery and Cross-Venue Matching (2/2 plans) -- completed 2026-02-27
- [x] Phase 20: Proposal Workflow and Operator Interface (2/2 plans) -- completed 2026-02-27
- [x] Phase 21: Lifecycle Management and Integration (2/2 plans) -- completed 2026-02-27

</details>

<details>
<summary>v1.3 Live Subscription Management (Phases 22-25) -- SHIPPED 2026-02-28</summary>

- [x] Phase 22: Subscription Manager Core (2/2 plans) -- completed 2026-02-27
- [x] Phase 23: Dynamic Supervisor Subscriptions (1/1 plan) -- completed 2026-02-27
- [x] Phase 24: Hardening and Observability (2/2 plans) -- completed 2026-02-27
- [x] Phase 25: Tech Debt Sweep (2/2 plans) -- completed 2026-02-28

</details>

<details>
<summary>v1.4 Analysis Tooling (Phases 26-29) -- SHIPPED 2026-03-02</summary>

- [x] Phase 26: Analysis Infrastructure (2/2 plans) -- completed 2026-02-28
- [x] Phase 27: Spread Analytics CLI (1/1 plan) -- completed 2026-02-28
- [x] Phase 28: Signal Scoring CLI (2/2 plans) -- completed 2026-02-28
- [x] Phase 29: End-to-End Verification (2/2 plans) -- completed 2026-02-28

</details>

### v1.5 Derive.xyz Venue Integration (In Progress)

**Milestone Goal:** Add Derive.xyz as fourth venue -- decentralized options exchange on Ethereum L2 with BTC options CLOB. Enables Deribit vs Derive options spread and three-way cross-venue signals.

- [x] **Phase 30: Venue Type Foundation** - Add Venue::Derive enum, resolve all match arms, add config section, verify live API (completed 2026-03-04)
- [ ] **Phase 31: Derive Feed and Normalization** - Complete standalone feed emitting correct MarketSnapshot with USDC normalization
- [ ] **Phase 32: Pipeline Wiring and Observability** - Wire Derive into live multi-venue pipeline with subscription management and metrics
- [ ] **Phase 33: Discovery and Matching** - REST-based instrument discovery with cross-venue matching and proposal workflow

### Phase 30: Venue Type Foundation
**Goal**: Codebase compiles with Derive awareness and all API unknowns are resolved
**Depends on**: Nothing (first phase of v1.5)
**Requirements**: PIPE-01, PIPE-02
**Plans:** 2/2 plans complete
Plans:
- [x] 30-01-PLAN.md -- Add Venue::Derive enum variant, config structs, venues.toml section, and resolve all exhaustive match arms
- [x] 30-02-PLAN.md -- Live API verification: probe Derive WebSocket, document channel format, book model, heartbeat, auth requirement
**Success Criteria**:
  1. `cargo check` passes with `Venue::Derive` variant and zero `todo!()`/`unreachable!()` placeholders in any match arm
  2. `venues.toml` contains a `[derive]` section with WebSocket URL, rate limits, book depth, and staleness threshold
  3. Live API connection to Derive testnet has confirmed: channel subscription format, book update model (snapshot vs delta), heartbeat mechanism, and whether authentication is required for public channels

### Phase 31: Derive Feed and Normalization
**Goal**: A standalone Derive feed emits correctly normalized MarketSnapshot with USDC-to-BTC price conversion
**Depends on**: Phase 30
**Requirements**: FEED-01, FEED-02, FEED-03, FEED-04, FEED-05, NORM-01, NORM-02, NORM-03, NORM-04
**Plans:** 4 plans
Plans:
- [ ] 31-01-PLAN.md -- Create Derive message types, channel helpers, and book state (messages.rs, channels.rs, book.rs)
- [ ] 31-02-PLAN.md -- Add Derive instrument parser and PricingEngine USDC venue gate
- [ ] 31-03-PLAN.md -- Create DeriveClient WebSocket client and DeriveSupervisor reconnection wrapper
- [ ] 31-04-PLAN.md -- Create DeriveProcessor for message parsing, MarketSnapshot emission, and JSONL recording
**Success Criteria**:
  1. Derive WebSocket client connects, subscribes to orderbook and ticker channels, and auto-reconnects on disconnect
  2. Derive order book state is maintained with correct bid/ask depth from WebSocket updates
  3. Derive instrument names in `BTC-YYYYMMDD-STRIKE-C/P` format parse correctly, and the parser rejects Deribit's `DDMMMYY` format (unit tested)
  4. MarketSnapshot emitted with USDC-normalized prices so Derive and Deribit implied probabilities for same strike/expiry are within 5% of each other
  5. Raw Derive WebSocket messages are recorded to JSONL in same pattern as existing venues

### Phase 32: Pipeline Wiring and Observability
**Goal**: Derive snapshots flow through the live multi-venue pipeline and SpreadEngine/SignalEngine produce cross-venue signals automatically
**Depends on**: Phase 31
**Requirements**: PIPE-03, PIPE-04, PIPE-05
**Success Criteria**:
  1. SubscriptionManager handles Derive instruments with HashSet diff reconciliation, watch channel push, and Notify ordering
  2. SpreadEngine, SignalEngine, and PaperTradeTracker receive and process Derive MarketSnapshots via `run_live_multi_venue()` without any downstream engine changes
  3. Prometheus metrics expose Derive feed state: connection status, message rate, active subscription count, and reconnection events

### Phase 33: Discovery and Matching
**Goal**: System automatically discovers Derive BTC options and proposes cross-venue matches for human approval
**Depends on**: Phase 32
**Requirements**: DISC-01, DISC-02, DISC-03, DISC-04
**Success Criteria**:
  1. `discover_derive()` fetches BTC options instruments from Derive REST API and returns `Vec<DiscoveredInstrument>` with correct strike, expiry, and direction
  2. Cross-venue matching between Derive and Deribit/Polymarket instruments uses existing FuzzyMatchKey with exact-date expiry matching
  3. Matched candidates are written to `events.toml` with `approved = false` and structured WARN logging
  4. Discovery runs as part of the ContractLifecycleManager periodic background pipeline alongside existing venue discovery

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14-17 | v1.1 Paper Trading | 11/11 | Complete | 2026-02-26 |
| 18-21 | v1.2 Automated Event Mgmt | 8/8 | Complete | 2026-02-27 |
| 22-25 | v1.3 Subscription Mgmt | 7/7 | Complete | 2026-02-28 |
| 26-29 | v1.4 Analysis Tooling | 7/7 | Complete | 2026-03-02 |
| 30. Venue Type Foundation | v1.5 | Complete    | 2026-03-04 | 2026-03-04 |
| 31. Derive Feed and Normalization | v1.5 | 0/4 | Not started | - |
| 32. Pipeline Wiring and Observability | v1.5 | 0/? | Not started | - |
| 33. Discovery and Matching | v1.5 | 0/? | Not started | - |
