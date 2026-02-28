# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (shipped 2026-02-27) | [Full details](milestones/v1.2-ROADMAP.md)
- v1.3 Live Subscription Management -- Phases 22-25 (in progress) | [Full details](milestones/v1.3-ROADMAP.md)

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

### v1.3 Live Subscription Management (In Progress)

- [x] **Phase 22: Subscription Manager Core** - SubscriptionManager with reconciliation logic, registry ordering, and diff logging (completed 2026-02-27)
- [x] **Phase 23: Dynamic Supervisor Subscriptions** - Wire watch channels into all three venue supervisors for reconnect-based subscribe/unsubscribe (completed 2026-02-27)
- [x] **Phase 24: Hardening and Observability** - Stale state cleanup, Prometheus subscription metrics, and dry-run reconciliation mode (completed 2026-02-27)
- [x] **Phase 25: Tech Debt Sweep** - Fix iv_spread, options book depth, and Kalshi staleness computation (completed 2026-02-28)

## Phase Details

### Phase 22: Subscription Manager Core
**Goal**: System can detect instrument changes from config reload and compute per-venue subscription diffs with correct ordering guarantees
**Depends on**: Nothing (first phase of v1.3; builds on v1.2 config reload infrastructure)
**Requirements**: SUB-03, SUB-04, SUB-06, OBS-03, OPS-02
**Success Criteria** (what must be TRUE):
  1. When events.toml changes, the system computes which instruments to add and remove per venue and logs the diff as structured tracing output
  2. Registry refresh always completes before subscription reconciliation reads registry state (ordering guaranteed via Notify)
  3. Only instruments from active_approved() event mappings appear in the computed subscription set
  4. When a supervisor reconnects (e.g., from network drop), it uses the latest instrument list from the registry, not the static startup config
**Plans:** 2/2 plans complete
Plans:
- [x] 22-01-PLAN.md — SubscriptionManager module with reconciliation logic, diff computation, and structured logging
- [x] 22-02-PLAN.md — Wire SubscriptionManager into main.rs with Notify ordering and watch channel lifecycle

### Phase 23: Dynamic Supervisor Subscriptions
**Goal**: Operator can approve new instruments or archive expired ones and see the system subscribe/unsubscribe feeds without restart
**Depends on**: Phase 22
**Requirements**: SUB-01, SUB-02
**Success Criteria** (what must be TRUE):
  1. When operator sets approved = true on a new event mapping in events.toml, the system subscribes to that instrument's feeds on the relevant venues within one config reload cycle -- no restart required
  2. When an event is archived (moved to events_archive.toml with Retired status), the system unsubscribes from that instrument's feeds on the relevant venues within one config reload cycle -- no restart required
  3. All three venue supervisors (Deribit, Polymarket, Kalshi) accept watch channel updates and reconnect with the updated instrument list
**Plans:** 1/1 plans complete
Plans:
- [x] 23-01-PLAN.md — Wire watch::Receiver into all three venue supervisors and thread receivers through pipeline.rs

### Phase 24: Hardening and Observability
**Goal**: Subscription lifecycle is observable via metrics and safe to operate with dry-run mode, and unsubscribed instruments leave no stale state
**Depends on**: Phase 23
**Requirements**: SUB-05, OBS-01, OBS-02, OPS-01
**Success Criteria** (what must be TRUE):
  1. After an instrument is unsubscribed, its order books, snapshots, and rolling stats are cleaned up -- no phantom spread signals from stale data paired with live data
  2. Prometheus gauges show the current number of active subscriptions per venue (queryable as subscription_active{venue="deribit"})
  3. Prometheus counters track cumulative subscription activations and removals per venue (queryable as subscription_activations_total and subscription_removals_total)
  4. When dry_run = true in config, reconciliation logs what subscribe/unsubscribe actions would be taken without sending any commands to venues
**Plans:** 2/2 plans complete
Plans:
- [ ] 24-01-PLAN.md — Subscription metrics (gauges + counters), dry-run reconciliation mode, SubscriptionConfig, and CleanupEvent infrastructure
- [ ] 24-02-PLAN.md — Wire cleanup channels into all 5 stateful engines for stale state eviction after unsubscribe

### Phase 25: Tech Debt Sweep
**Goal**: Three behavior-changing tech debt items from v1.0 are fixed so metrics and staleness detection reflect real data
**Depends on**: Phase 23 (can run after core subscription works; independent of Phase 24)
**Requirements**: FIX-01, FIX-02, FIX-03
**Success Criteria** (what must be TRUE):
  1. iv_spread field in spread computations is populated from the IV solver's actual bid/ask IV metadata instead of always being 0.0
  2. Options book_depth_levels is read from the [deribit] config section instead of being hardcoded to 0
  3. Kalshi is_stale is computed from the exchange_timestamp field instead of always returning false
**Plans:** 2/2 plans complete
Plans:
- [ ] 25-01-PLAN.md — IV spread propagation (FIX-01) and config-driven book depth (FIX-02)
- [ ] 25-02-PLAN.md — Kalshi staleness computation from exchange_timestamp (FIX-03)

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14-17 | v1.1 Paper Trading | 11/11 | Complete | 2026-02-26 |
| 18-21 | v1.2 Automated Event Mgmt | 8/8 | Complete | 2026-02-27 |
| 22 | v1.3 Subscription Mgmt | 2/2 | Complete | 2026-02-27 |
| 23 | v1.3 Subscription Mgmt | Complete    | 2026-02-27 | 2026-02-27 |
| 24 | 2/2 | Complete    | 2026-02-27 | - |
| 25 | 2/2 | Complete    | 2026-02-28 | - |
