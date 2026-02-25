# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (in progress)

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

### v1.1 Paper Trading Validation

**Milestone Goal:** Prove signal quality is real and the system is operationally trustworthy enough for extended unattended paper trading. Answer the question: "Are the cross-venue arbitrage signals generating real alpha, or are they artifacts?"

- [x] **Phase 14: Failure Alerting** - Detect silent degradation, stale data, and partial feeds before they corrupt validation data (completed 2026-02-24)
- [x] **Phase 15: State Persistence** - Survive restarts without losing weeks of paper trade and signal history (completed 2026-02-24)
- [x] **Phase 16: Settlement Outcome Tracking** - Know how events actually resolved so signal predictions can be verified (completed 2026-02-26)
- [ ] **Phase 17: Signal Analysis Tooling** - Measure hit rate, edge, false positive rate, and time-to-convergence to answer "are signals real?"

## Phase Details

### Phase 14: Failure Alerting
**Goal**: Operator can trust that silent degradation, stale data, and partial feeds are detected and surfaced before they corrupt the paper trading validation dataset
**Depends on**: Phase 13 (v1.0 complete)
**Requirements**: ALRT-01, ALRT-02, ALRT-03, ALRT-04, ALRT-05, ALRT-06
**Success Criteria** (what must be TRUE):
  1. System logs a structured warning within 60 seconds when a venue feed goes silent (connected but no messages) beyond the configured threshold
  2. System logs a structured warning when fewer venues are reporting data than the expected count (partial coverage)
  3. System logs a structured warning when no signals have been evaluated for longer than the configured gap threshold
  4. Operator can query Prometheus for active alert conditions (feed silence, partial coverage, signal gap, pipeline stage liveness)
  5. Each pipeline stage (spread computation, signal evaluation, settlement check) has a liveness timestamp that the alert monitor inspects
**Plans**: 2 plans

Plans:
- [ ] 14-01-PLAN.md -- Alert types, config, and pipeline liveness infrastructure
- [ ] 14-02-PLAN.md -- AlertMonitor implementation and pipeline wiring

### Phase 15: State Persistence
**Goal**: Multi-week paper trading sessions survive process restarts without data loss -- paper trade positions, daily rollups, and signal analysis accumulators are recoverable
**Depends on**: Phase 13 (v1.0 complete)
**Requirements**: PRST-01, PRST-02, PRST-03, PRST-04, PRST-05
**Success Criteria** (what must be TRUE):
  1. After a clean shutdown and restart, paper trade positions and daily P&L rollups are restored to their pre-shutdown state
  2. After a crash (kill -9) and restart, state recovers from the last checkpoint with no corrupted files left on disk
  3. Signal analysis accumulator state (counters, running totals) survives restart via checkpoint
  4. Recovery replays JSONL trade events after the checkpoint timestamp to reconstruct any state changes between the last checkpoint and the shutdown
  5. Checkpoint files are written atomically (write-to-temp-then-rename) so partial writes never corrupt the active checkpoint
**Plans**: 2 plans

Plans:
- [ ] 15-01-PLAN.md -- CheckpointState types, atomic write utility, PersistenceConfig, snapshot/restore methods
- [ ] 15-02-PLAN.md -- Recovery loading, JSONL replay, periodic checkpointing, main.rs integration

### Phase 16: Settlement Outcome Tracking
**Goal**: The system knows how prediction market events and options expirations actually resolved, enabling paper trade positions to be settled and providing ground truth for signal analysis
**Depends on**: Phase 14, Phase 15
**Requirements**: STTL-01, STTL-02, STTL-03, STTL-04, STTL-05, STTL-06, STTL-07
**Research flag**: NEEDS `/gsd:research-phase` -- venue settlement APIs are heterogeneous (Polymarket has no clean resolution endpoint, Deribit instruments may delist post-expiry, Kalshi requires auth for settlement data)
**Success Criteria** (what must be TRUE):
  1. After a Deribit options expiry, the system polls the delivery price and determines whether each tracked binary outcome settled YES or NO
  2. After a Kalshi event closes, the system polls the resolution result and records the settlement outcome
  3. After a Polymarket event resolves, the system detects resolution via the Gamma API and records the settlement outcome
  4. Settlement outcomes from all three venues are normalized to a single SettlementOutcome type and logged to JSONL for historical analysis
  5. Paper trade positions are automatically marked as settled (with realized P&L) when the corresponding settlement outcome arrives
**Plans**: 4 plans

Plans:
- [x] 16-01-PLAN.md -- Settlement types, config, ResolutionChecker trait, and venue implementations (Deribit, Kalshi, Polymarket)
- [x] 16-02-PLAN.md -- SettlementMonitor task with four-tier polling cadence and startup backfill
- [x] 16-03-PLAN.md -- PaperTradeTracker settlement integration, checkpoint extension, and main.rs wiring
- [ ] 16-04-PLAN.md -- Gap closure: Wire venue resolution checkers into SettlementMonitor in main.rs

### Phase 17: Signal Analysis Tooling
**Goal**: Operator can answer "are the arbitrage signals generating real alpha?" with statistical evidence -- hit rate, cost-adjusted edge, false positive rate, and time-to-convergence computed from settled positions
**Depends on**: Phase 16
**Requirements**: ANLZ-01, ANLZ-02, ANLZ-03, ANLZ-04, ANLZ-05, ANLZ-06, ANLZ-07
**Success Criteria** (what must be TRUE):
  1. After positions settle, operator can see hit rate (profitable-at-settlement / total-settled) and false positive rate (loss-at-settlement / total-settled) in logs and Prometheus
  2. Cost-adjusted average edge per settled position is computed and exposed, accounting for fees, slippage, and adverse selection
  3. Time-to-convergence (duration from signal generation to price convergence) is measured and reported for each settled position
  4. Threshold effectiveness is visible: operator can compare settlement outcomes across ThresholdStatus categories (PassedBoth vs PassedStaticOnly vs Filtered) to decide whether to tighten or loosen thresholds
  5. All analysis metrics are available both as structured JSONL logs (for post-hoc analysis) and as Prometheus gauges (for live monitoring)
**Plans**: TBD

Plans:
- [ ] 17-01: TBD
- [ ] 17-02: TBD
- [ ] 17-03: TBD

## Progress

**Execution Order:**
Phases 14 and 15 can execute in parallel (no dependency between them). Phase 16 depends on both. Phase 17 depends on 16.

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1-13 | v1.0 MVP | 36/36 | Complete | 2026-02-24 |
| 14. Failure Alerting | 2/2 | Complete    | 2026-02-24 | - |
| 15. State Persistence | 2/2 | Complete    | 2026-02-24 | - |
| 16. Settlement Outcome Tracking | 4/4 | Complete   | 2026-02-25 | - |
| 17. Signal Analysis Tooling | v1.1 | 0/TBD | Not started | - |
