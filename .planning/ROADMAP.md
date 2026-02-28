# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (shipped 2026-02-27) | [Full details](milestones/v1.2-ROADMAP.md)
- v1.3 Live Subscription Management -- Phases 22-25 (shipped 2026-02-28) | [Full details](milestones/v1.3-ROADMAP.md)
- v1.4 Analysis Tooling -- Phases 26-29 (in progress)

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

### v1.4 Analysis Tooling (In Progress)

**Milestone Goal:** Build CLI-based analysis tools to evaluate signal quality and spread patterns from soak test data, enabling statistically rigorous go/no-go decisions before v2 execution.

- [x] **Phase 26: Analysis Infrastructure** - Shared stats module, JSONL data loading, and output formatting for both CLIs (completed 2026-02-28)
- [x] **Phase 27: Spread Analytics CLI** - Complete spread-analytics binary with distribution stats, hourly buckets, and venue-pair breakdown (completed 2026-02-28)
- [x] **Phase 28: Signal Scoring CLI** - Complete signal-scoring binary with hit rate, Sharpe, PSR, drawdown, and cost-adjusted edge (completed 2026-02-28)
- [x] **Phase 29: End-to-End Verification** - Validate both CLIs against real soak test data and fix edge cases (completed 2026-02-28)

## Phase Details

### Phase 26: Analysis Infrastructure
**Goal**: Both CLIs have a tested foundation of shared statistical functions, streaming JSONL data loading with date-range filtering, and formatted output rendering
**Depends on**: Phase 25 (v1.3 complete)
**Requirements**: INFRA-01, INFRA-02, INFRA-03, INFRA-04
**Success Criteria** (what must be TRUE):
  1. User can invoke `spread-analytics --help` and `signal-scoring --help` and see valid CLI usage with `--from`, `--to`, `--last`, `--output`, and `--by-event` flags documented
  2. User can pass `--from 2026-02-25 --to 2026-02-28` and the tool loads only JSONL files within that date range (files outside the range are not opened)
  3. User sees aligned terminal table output with numeric columns right-justified and section headers when running either CLI with default output mode
  4. User can pass `--output json` and receive valid JSON that parses without error, containing the same data as the table output
**Plans**: 2 plans
Plans:
- [x] 26-01-PLAN.md -- Shared analysis module foundation (stats.rs + io.rs)
- [x] 26-02-PLAN.md -- Output formatting and CLI binary entry points

### Phase 27: Spread Analytics CLI
**Goal**: User can analyze spread distribution patterns, hourly opportunity clustering, and venue-pair performance from recorded spread data
**Depends on**: Phase 26
**Requirements**: SPREAD-01, SPREAD-02, SPREAD-03
**Success Criteria** (what must be TRUE):
  1. User can run `spread-analytics --from <date> --to <date>` and see summary statistics table (count, mean, median, stddev, min, max, p5/p25/p75/p95) for both net and gross spreads
  2. User can see a 24-row hourly breakdown showing per-UTC-hour spread statistics that reveals when arbitrage opportunities cluster
  3. User can see spread statistics grouped by venue pair (Polymarket-Kalshi, Deribit-Polymarket, Deribit-Kalshi) with directional detail, never mixed into a single aggregate
  4. User can pass `--by-event` and see all three analyses additionally broken down per event_id
**Plans**: 1 plan
Plans:
- [x] 27-01-PLAN.md -- Spread analytics computation module and CLI binary wiring

### Phase 28: Signal Scoring CLI
**Goal**: User can make a statistically rigorous go/no-go decision for v2 execution based on hit rate confidence intervals, edge significance, risk-adjusted returns, and drawdown analysis
**Depends on**: Phase 26
**Requirements**: SIGNAL-01, SIGNAL-02, SIGNAL-03, SIGNAL-04, SIGNAL-05
**Success Criteria** (what must be TRUE):
  1. User can see hit rate (gross and net) with Wilson score confidence intervals at 95% and 99% levels, with sample size (n=X) displayed alongside every interval
  2. User can see cost-adjusted mean edge with t-statistic, p-value, and 95% CI that answers whether the edge is statistically distinguishable from zero
  3. User can see per-trade Sharpe ratio (primary, no annualization) and frequency-adjusted annualized Sharpe, with PSR showing the probability that true Sharpe exceeds zero
  4. User can see maximum drawdown in absolute and percentage terms with drawdown start date, trough date, and recovery date (or "ongoing" if not recovered)
  5. User can pass `--by-event` and see all scoring metrics additionally broken down per event_id
**Plans**: 2 plans
Plans:
- [x] 28-01-PLAN.md -- Scoring computation module (stats additions + five pure scoring functions)
- [x] 28-02-PLAN.md -- CLI binary wiring with table/JSON rendering and by-event support

### Phase 29: End-to-End Verification
**Goal**: Both CLIs produce correct, trustworthy output when run against actual soak test data, with all edge cases handled gracefully
**Depends on**: Phase 27, Phase 28
**Requirements**: (cross-cutting verification of all requirements)
**Success Criteria** (what must be TRUE):
  1. Both CLIs run against real soak test JSONL data and produce output without errors, panics, or malformed tables
  2. At least one known data subset has hand-verified expected values that match CLI output for spread stats, hit rate, and Sharpe ratio
  3. Edge cases produce graceful output: empty date ranges show "No data in range", zero settled positions show "Insufficient data" rather than division-by-zero panics, and malformed JSONL lines are skipped with a warning count
**Plans**: 2 plans
Plans:
- [ ] 29-01-PLAN.md -- Spread analytics E2E golden value and edge case tests
- [ ] 29-02-PLAN.md -- Signal scoring E2E golden value and edge case tests

## Progress

**Execution Order:**
Phases execute in numeric order: 26 -> 27 -> 28 -> 29
(Note: Phases 27 and 28 are independent after 26 completes, but sequential execution is simpler for a solo developer.)

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14-17 | v1.1 Paper Trading | 11/11 | Complete | 2026-02-26 |
| 18-21 | v1.2 Automated Event Mgmt | 8/8 | Complete | 2026-02-27 |
| 22-25 | v1.3 Subscription Mgmt | 7/7 | Complete | 2026-02-28 |
| 26. Analysis Infrastructure | 2/2 | Complete    | 2026-02-28 | - |
| 27. Spread Analytics CLI | 1/1 | Complete    | 2026-02-28 | - |
| 28. Signal Scoring CLI | 2/2 | Complete    | 2026-02-28 | - |
| 29. Verification | 2/2 | Complete    | 2026-02-28 | - |
