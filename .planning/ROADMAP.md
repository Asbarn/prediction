# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)

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
