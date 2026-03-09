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
- v1.8 Signal Quality Validation -- Phases 44-48 (in progress) | [Full details](milestones/v1.8-ROADMAP.md)

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

<details>
<summary>v1.7 Prediction Market Signal Pipeline (Phases 40-43) -- SHIPPED 2026-03-09</summary>

- [x] Phase 40: Polymarket WS Diagnosis and Data Watchdog (2/2 plans) -- completed 2026-03-09
- [x] Phase 41: Signal Engine Generalization (1/1 plan) -- completed 2026-03-09
- [x] Phase 42: REST Polling Fallback and Source Coordination (2/2 plans) -- completed 2026-03-09
- [x] Phase 43: E2E Production Verification (2/2 plans) -- completed 2026-03-09

</details>

### v1.8 Signal Quality Validation (In Progress)

- [x] **Phase 44: Critical Bug Fixes and Data Pipeline Repair** - Fix cost model unit mismatch, Kalshi fee rounding, and spread logger silence
- [x] **Phase 45: Instrument Quality and Event Mapping** - Populate events.toml with near-the-money BTC pairs, build match-audit CLI, add OTM filtering (completed 2026-03-09)
- [ ] **Phase 46: Diagnostic CLI Tools** - Cost-audit and book-depth CLIs plus stats module extensions for signal analysis
- [ ] **Phase 47: Cost Model Validation** - Validate fee parameters against exchange docs, run sensitivity analysis, estimate on-chain costs
- [ ] **Phase 48: Statistical Validation and Go/No-Go** - Autocorrelation-corrected analysis, out-of-sample validation, final confidence-interval report

### Phase 44: Critical Bug Fixes and Data Pipeline Repair
**Goal**: Cost computations are mathematically correct and spread logger produces data for downstream analysis
**Depends on**: Nothing (first phase of v1.8)
**Requirements**: FIX-01, FIX-02, FIX-03
**Success Criteria** (what must be TRUE):
  1. Running the system produces spread_logs JSONL files containing SpreadResult entries for active Polymarket-vs-options pairs
  2. Cost model subtracts fees in probability space (same units as raw spread), not dollar space -- verifiable by inspecting net_edge values that are the same order of magnitude as raw_spread
  3. Kalshi taker fee on a $0.25 contract computes to $0.02 (not $1.00) -- verifiable by unit test
  4. Signal log entries show net_edge values in a plausible range (not uniformly -19.5)
**Plans:** 2/2 plans complete
Plans:
- [x] 44-01-PLAN.md -- Fix Kalshi fee ceiling rounding and normalize cost units to probability space
- [x] 44-02-PLAN.md -- Wire SpreadLogger into CrossAssetEngine for spread_logs output

### Phase 45: Instrument Quality and Event Mapping
**Goal**: Production system analyzes near-the-money BTC instruments where prediction market prices and options-implied probabilities measure the same economic bet
**Depends on**: Phase 44
**Requirements**: INST-01, INST-02, INST-03
**Success Criteria** (what must be TRUE):
  1. events.toml contains at least 3 active BTC instrument mappings with strikes within 10% of current spot price
  2. match-audit CLI confirms all active mappings have aligned strike, expiry (within tolerance), and direction across venues
  3. Discovery pipeline skips Polymarket contracts where bid-ask spread exceeds configurable threshold (no more deep OTM phantom liquidity pairs)
  4. Operator can run match-audit at any time to validate instrument quality before approving new mappings
**Plans**: 2 plans
Plans:
- [ ] 45-01-PLAN.md -- Polymarket bid-ask filtering and match-audit CLI
- [ ] 45-02-PLAN.md -- Populate events.toml with near-the-money BTC mappings

### Phase 46: Diagnostic CLI Tools
**Goal**: Operator can decompose signal economics and book quality to answer "where does negative edge come from?"
**Depends on**: Phase 44, Phase 45
**Requirements**: DIAG-01, DIAG-02, DIAG-03
**Success Criteria** (what must be TRUE):
  1. cost-audit CLI reads signal_logs and prints per-event cost breakdown showing which components (venue fees, slippage, basis risk, gas) dominate negative edge
  2. book-depth CLI reads signal_logs and reports effective spread, fill simulation at configurable sizes, and depth quality scores per instrument
  3. Stats module provides Pearson correlation and KS test functions usable by cost-audit and signal-scoring CLIs
  4. Both CLIs support --output json and --by-event flags consistent with existing spread-analytics and signal-scoring patterns
**Plans**: 2 plans
Plans:
- [ ] 45-01-PLAN.md -- Polymarket bid-ask filtering and match-audit CLI
- [ ] 45-02-PLAN.md -- Populate events.toml with near-the-money BTC mappings

### Phase 47: Cost Model Validation
**Goal**: Every cost parameter is justified by external evidence (exchange docs or on-chain data), not by what makes signals look profitable
**Depends on**: Phase 46
**Requirements**: COST-01, COST-02, COST-03
**Success Criteria** (what must be TRUE):
  1. Cost model parameters for Deribit, Derive, and Polymarket fees are documented with citations to exchange fee schedules, and any discrepancies are corrected
  2. Sensitivity analysis output shows which cost components have the largest impact on net edge (ranked by magnitude)
  3. Polymarket leg cost model includes estimated on-chain execution costs (gas, bridging) with documented source for estimates
  4. config.toml cost parameters reflect validated values, with each change traceable to an external data source
**Plans**: 2 plans
Plans:
- [ ] 45-01-PLAN.md -- Polymarket bid-ask filtering and match-audit CLI
- [ ] 45-02-PLAN.md -- Populate events.toml with near-the-money BTC mappings

### Phase 48: Statistical Validation and Go/No-Go
**Goal**: Statistically valid assessment of whether profitable cross-venue arbitrage opportunities exist after all fixes, with honest confidence intervals
**Depends on**: Phase 47
**Requirements**: STAT-01, STAT-02, STAT-03
**Success Criteria** (what must be TRUE):
  1. Signal analysis reports effective sample size (autocorrelation-corrected), not raw signal count, for all statistical tests
  2. Evaluation uses out-of-sample data that was not used during cost model tuning (explicit train/test split documented)
  3. Final go/no-go report states expected edge with confidence intervals, effective sample size, and a clear recommendation on whether to proceed to execution readiness
**Plans**: 2 plans
Plans:
- [ ] 45-01-PLAN.md -- Polymarket bid-ask filtering and match-audit CLI
- [ ] 45-02-PLAN.md -- Populate events.toml with near-the-money BTC mappings

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
| 40-43 | v1.7 Signal Pipeline | 7/7 | Complete | 2026-03-09 |
| 44 | v1.8 Bug Fixes | Complete    | 2026-03-09 | 2026-03-09 |
| 45 | 2/2 | Complete   | 2026-03-09 | - |
| 46 | v1.8 Signal Quality | 0/TBD | Not started | - |
| 47 | v1.8 Signal Quality | 0/TBD | Not started | - |
| 48 | v1.8 Signal Quality | 0/TBD | Not started | - |
