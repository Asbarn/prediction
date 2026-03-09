# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit, Derive). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (shipped 2026-02-27) | [Full details](milestones/v1.2-ROADMAP.md)
- v1.3 Live Subscription Management -- Phases 22-25 (shipped 2026-02-28) | [Full details](milestones/v1.3-ROADMAP.md)
- v1.4 Analysis Tooling -- Phases 26-29 (shipped 2026-03-02) | [Full details](milestones/v1.4-ROADMAP.md)
- v1.5 Derive.xyz Venue Integration -- Phases 30-33 (shipped 2026-03-06) | [Full details](milestones/v1.5-ROADMAP.md)
- v1.6 Production Deployment -- Phases 34-39 (shipped 2026-03-09) | [Full details](milestones/v1.6-ROADMAP.md)
- v1.7 Prediction Market Signal Pipeline -- Phases 40-43 (shipped 2026-03-09) | [Full details](milestones/v1.7-ROADMAP.md)

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

<details>
<summary>v1.5 Derive.xyz Venue Integration (Phases 30-33) -- SHIPPED 2026-03-06</summary>

- [x] Phase 30: Venue Type Foundation (2/2 plans) -- completed 2026-03-04
- [x] Phase 31: Derive Feed and Normalization (4/4 plans) -- completed 2026-03-04
- [x] Phase 32: Pipeline Wiring and Observability (2/2 plans) -- completed 2026-03-05
- [x] Phase 33: Discovery and Matching (2/2 plans) -- completed 2026-03-06

</details>

<details>
<summary>v1.6 Production Deployment (Phases 34-39) -- SHIPPED 2026-03-09</summary>

- [x] Phase 34: CDK Infrastructure Foundation (2/2 plans) -- completed 2026-03-07
- [x] Phase 35: Compute, Secrets, and Hardening (2/2 plans) -- completed 2026-03-07
- [x] Phase 36: CloudWatch Logging (2/2 plans) -- completed 2026-03-07
- [x] Phase 37: Prometheus + AMP + Managed Grafana (2/2 plans) -- completed 2026-03-07
- [x] Phase 38: GitLab CI/CD Pipeline (2/2 plans) -- completed 2026-03-08
- [x] Phase 39: Grafana Dashboards and Alert Rules (2/2 plans) -- completed 2026-03-08

</details>

### v1.7 Prediction Market Signal Pipeline -- SHIPPED 2026-03-09

- [x] **Phase 40: Polymarket WS Diagnosis and Data Watchdog** - Investigate WS failure from EC2, implement data inactivity detection (completed 2026-03-09)
- [x] **Phase 41: Signal Engine Generalization** - Remove hardcoded venue references in CrossAssetEngine (completed 2026-03-09)
- [x] **Phase 42: REST Polling Fallback and Source Coordination** - REST price polling with exclusive-mode WS/REST switching (completed 2026-03-09)
- [x] **Phase 43: E2E Production Verification** - Prove signal pipeline on AWS EC2 with live data (completed 2026-03-09)

### Phase 40: Polymarket WS Diagnosis and Data Watchdog
**Goal**: Polymarket data flows reliably from production EC2, with automatic recovery from silent freezes
**Depends on**: Nothing (first phase of v1.7)
**Requirements**: POLY-01, POLY-02, POLY-03
**Plans**: 2 plans
**Success Criteria**:
  1. Operator can see a documented diagnosis of the Polymarket WebSocket failure mode from EC2
  2. Polymarket supervisor automatically detects data inactivity and triggers reconnection after configurable timeout
  3. Polymarket WebSocket feed delivers order book data from the production EC2 instance (or diagnosis conclusively shows it cannot)
  4. Prometheus metrics reflect Polymarket feed liveness state and reconnection events

Plans:
- [ ] 40-01-PLAN.md — Config extension (data_timeout_secs) and WS diagnostic integration test
- [ ] 40-02-PLAN.md — Supervisor data inactivity watchdog with tokio::time::timeout

### Phase 41: Signal Engine Generalization
**Goal**: CrossAssetEngine correctly generates arbitrage signals using options-implied probabilities from any venue
**Depends on**: Nothing (independent of Phase 40)
**Requirements**: SIG-01, SIG-02, SIG-03
**Plans**: 1 plan
**Success Criteria**:
  1. ImpliedProbability values carry their source venue (Deribit or Derive), not a hardcoded assumption
  2. CrossAssetEngine generates ArbSignals when only one prediction market venue has data (Polymarket alone, without Kalshi)
  3. CrossAssetEngine pairs Derive-sourced implied probabilities with prediction market snapshots to produce correctly attributed signals

Plans:
- [ ] 41-01-PLAN.md — Add source_venue to ImpliedProbability, wire through CrossAssetEngine, dynamic prediction venue iteration

### Phase 42: REST Polling Fallback and Source Coordination
**Goal**: Polymarket price data is available via REST polling when WebSocket is unreliable, with exclusive-mode switching
**Depends on**: Phase 40, Phase 41
**Requirements**: POLY-04, POLY-05
**Plans**: 2 plans

Plans:
- [ ] 42-01-PLAN.md — REST poller module and config extensions (PolymarketRestPoller, /midpoint endpoint)
- [ ] 42-02-PLAN.md — Source coordinator state machine and pipeline integration (exclusive WS/REST switching)

### Phase 43: End-to-End Production Verification
**Goal**: Complete signal pipeline verified working on production EC2 with real market data
**Depends on**: Phase 40, Phase 41, Phase 42
**Requirements**: VER-01, VER-02
**Plans**: 2 plans

Plans:
- [x] 43-01-PLAN.md — Fix signal_logs Docker volume mount (docker-compose.yml + CDK)
- [x] 43-02-PLAN.md — Deploy to production and verify signals in Grafana + JSONL logs

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14-17 | v1.1 Paper Trading | 11/11 | Complete | 2026-02-26 |
| 18-21 | v1.2 Automated Event Mgmt | 8/8 | Complete | 2026-02-27 |
| 22-25 | v1.3 Subscription Mgmt | 7/7 | Complete | 2026-02-28 |
| 26-29 | v1.4 Analysis Tooling | 7/7 | Complete | 2026-03-02 |
| 30-33 | v1.5 Derive Integration | 10/10 | Complete | 2026-03-06 |
| 34-39 | v1.6 Production Deployment | 12/12 | Complete | 2026-03-09 |
| 40 | 2/2 | Complete    | 2026-03-09 | - |
| 41 | 1/1 | Complete    | 2026-03-09 | - |
| 42 | 2/2 | Complete    | 2026-03-09 | - |
| 43 | 2/2 | Complete | 2026-03-09 | - |
