# Roadmap: Prediction Market Arbitrage System

## Overview

Cross-venue arbitrage signal generator in Rust. Detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options-implied probabilities (Deribit). Single-binary service with TOML configuration, Prometheus metrics, and deterministic replay.

## Milestones

- v1.0 MVP -- Phases 1-13 (shipped 2026-02-24) | [Full details](milestones/v1.0-ROADMAP.md)
- v1.1 Paper Trading Validation -- Phases 14-17 (shipped 2026-02-26) | [Full details](milestones/v1.1-ROADMAP.md)
- v1.2 Automated Event Management -- Phases 18-21 (in progress)

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

### v1.2 Automated Event Management (In Progress)

**Milestone Goal:** Eliminate manual events.toml curation -- system discovers markets across all three venues, proposes cross-venue mappings with confidence scoring, detects resolved events, manages lifecycle cleanup, and runs as an integrated background task. Operator intervention reduced to reviewing and approving proposals.

- [x] **Phase 18: Discovery Infrastructure Hardening** - Production-safe venue polling with shared rate limiters, consecutive-absence guards, and batched TOML writes (completed 2026-02-26)
- [ ] **Phase 19: Polymarket Discovery and Cross-Venue Matching** - Three-venue structured discovery with expiry tolerance matching and confidence-scored candidate proposals
- [ ] **Phase 20: Proposal Workflow and Operator Interface** - Atomic TOML proposal writing, structured logging, Prometheus metrics, and approval validation
- [ ] **Phase 21: Lifecycle Management and Integration** - Event archival, unapproved candidate cleanup, Retired status, and background task wiring into ContractLifecycleManager

## Phase Details

### Phase 18: Discovery Infrastructure Hardening
**Goal**: Venue discovery polling is production-safe with shared rate limiters, consecutive-absence expiry guards that prevent false expirations, and batched TOML writes that eliminate race conditions
**Depends on**: Phase 17 (v1.1 complete)
**Requirements**: DISC-02, LIFE-04, INTG-03
**Success Criteria** (what must be TRUE):
  1. Deribit and Kalshi discovery polls use shared VenueRateLimiter instances (not per-component limiters) and respect venue rate limits under sustained polling
  2. An instrument absent from a single API response is NOT marked expired -- only N consecutive absences (configurable, default 3) trigger expiry transition
  3. All TOML modifications within a single poll cycle are batched into one atomic write (not one write per candidate)
  4. A partial API response (instrument count drop >20%) is logged as suspect and does not trigger expirations
**Plans:** 2/2 plans complete
Plans:
- [ ] 18-01-PLAN.md — Config extensions and batch TOML mutation functions
- [ ] 18-02-PLAN.md — Shared rate limiters, absence tracking, partial-response detection, and batched poll cycle

### Phase 19: Polymarket Discovery and Cross-Venue Matching
**Goal**: System discovers structured instrument data from all three venues and matches cross-venue instruments using asset/strike/direction with configurable expiry date tolerance, producing candidate proposals with confidence scoring
**Depends on**: Phase 18
**Requirements**: DISC-01, DISC-03, DISC-04, INTG-02
**Success Criteria** (what must be TRUE):
  1. Polymarket Gamma API polling with crypto category filtering extracts asset, strike, direction, and expiry from groupItemTitle patterns (e.g., "Will Bitcoin be above $150,000 on June 27?" yields asset=BTC, strike=150000, direction=Above, expiry=2025-06-27)
  2. Polymarket discovery returns Vec<DiscoveredInstrument> (same type as Deribit/Kalshi), enabling unified cross-venue matching pipeline
  3. Cross-venue matching uses exact asset/strike/direction with configurable expiry tolerance window (default 7 days) -- Deribit Friday expiry and Kalshi end-of-month expiry for the same target period produce a match
  4. Each candidate proposal includes instruments from all matched venues with an expiry confidence score (HIGH/MEDIUM/LOW based on date difference between venues)
**Plans:** 2 plans
Plans:
- [ ] 19-01-PLAN.md — Polymarket structured discovery, config extensions, ExpiryConfidence type
- [ ] 19-02-PLAN.md — FuzzyMatchKey cross-venue matching with expiry tolerance, lifecycle integration

### Phase 20: Proposal Workflow and Operator Interface
**Goal**: Discovered candidates are written to events.toml as unapproved proposals with full operator visibility via structured logs and Prometheus metrics, and approved mappings are validated for safety on config reload
**Depends on**: Phase 19
**Requirements**: PROP-01, PROP-02, PROP-03, PROP-04
**Success Criteria** (what must be TRUE):
  1. Candidate mappings are written to events.toml with approved = false, preserving existing formatting and comments via atomic TOML writes
  2. Each new proposal emits a structured WARN-level tracing log containing event_id, matched venues, instrument identifiers, expiry dates, and confidence score
  3. Prometheus gauges expose current pending (unapproved) proposal count and a total proposals counter increments on each new proposal
  4. On config reload (SIGHUP), approved mappings are validated: at least 2 venue instruments present, instruments still active on their venues, and expiry date not already passed -- invalid mappings are rejected with a warning log
**Plans**: TBD

### Phase 21: Lifecycle Management and Integration
**Goal**: The system autonomously manages event lifecycle from active through retired, archives stale entries, cleans up unapproved candidates, and runs the entire discovery-match-propose pipeline as a periodic background task
**Depends on**: Phase 20
**Requirements**: LIFE-01, LIFE-02, LIFE-03, INTG-01
**Success Criteria** (what must be TRUE):
  1. Expired events older than the configurable retention period (default 30 days) are moved from events.toml to events_archive.toml, reducing active config size
  2. Unapproved candidate mappings whose expiry date has passed are automatically removed from events.toml without operator intervention
  3. LifecycleStatus includes a Retired variant for fully settled and archived events, distinguishing them from merely expired events
  4. The discovery manager runs as a periodic background task within the ContractLifecycleManager poll cycle, executing the full discover-match-propose pipeline each cycle
  5. After one complete poll cycle, the operator can observe new candidate entries in events.toml (approved=false) for any newly detected cross-venue instrument matches
**Plans**: TBD

## Progress

**Execution Order:** Phases execute sequentially: 18 -> 19 -> 20 -> 21

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 18. Discovery Infrastructure Hardening | 2/2 | Complete    | 2026-02-26 | - |
| 19. Polymarket Discovery and Cross-Venue Matching | v1.2 | 0/2 | Planned | - |
| 20. Proposal Workflow and Operator Interface | v1.2 | 0/TBD | Not started | - |
| 21. Lifecycle Management and Integration | v1.2 | 0/TBD | Not started | - |
