---
phase: 05-event-mapping
plan: 02
subsystem: events
tags: [basis-risk, settlement-time, source-pair, expiry-warning, risk-scoring, chrono]

# Dependency graph
requires:
  - phase: 05-event-mapping
    plan: 01
    provides: "EventMapping with SettlementMetadata, RiskWeightsConfig, ExpiryThreshold, EventsConfig"
  - phase: 01-foundation
    provides: "chrono dependency, config loading framework"
provides:
  - "BasisRiskScore with three independent component scores plus weighted composite"
  - "SourcePair enum for categorical settlement source classification"
  - "settlement_time_diff_hours() for ISO 8601 temporal mismatch computation"
  - "ExpiryWarning with configurable tier detection and risk inflation"
  - "compute_risk_for_mapping() convenience for EventMapping-level risk scoring"
affects: [05-03, 06-pricing-engine, 07-signal-generation]

# Tech tracking
tech-stack:
  added: []
  patterns: [categorical risk scoring, linear time risk scaling, configurable tier thresholds with inflation]

key-files:
  created:
    - src/events/risk.rs
  modified:
    - src/events/mod.rs

key-decisions:
  - "Unknown SourcePair uses index_oracle weight (0.5) as conservative default rather than zero"
  - "compute_risk_for_mapping uses expiry date at 00:00:00 UTC as prediction market resolution estimate when no explicit resolution time is available"
  - "inflate_risk_score uses default weights for composite recalculation (weights are global config, not per-score)"

patterns-established:
  - "Categorical risk scoring: SourcePair enum maps string pairs to typed variants with config-driven weights"
  - "Linear risk scaling: settlement_time_risk = hours * time_per_hour, simple and interpretable"
  - "Tier threshold selection: pick the tightest (smallest hours_before_expiry) that still contains the remaining time"

requirements-completed: [EVNT-02, EVNT-03, EVNT-05]

# Metrics
duration: 4min
completed: 2026-02-22
---

# Phase 5 Plan 02: Basis Risk Scoring Summary

**Basis risk scoring with three independent components (settlement time, source, criteria) plus configurable near-expiry warning tiers with settlement_time_risk inflation**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-22T21:13:32Z
- **Completed:** 2026-02-22T21:17:56Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Implemented BasisRiskScore with settlement_time_risk (linear with hours), source_risk (categorical from config), criteria_risk, and weighted composite
- Built SourcePair enum with from_sources() that classifies string pairs (deribit_index, oracle, index) into typed variants
- Created ExpiryWarning system that detects configurable tiers (caution/warning/critical) and provides risk inflation factors
- Added settlement_time_diff_hours() and deribit_settlement_time() helpers for temporal computation
- compute_risk_for_mapping() extracts settlement metadata from EventMapping for one-call risk scoring
- 19 unit tests covering all scoring paths, edge cases, expiry tiers, and integration with EventMapping

## Task Commits

Each task was committed atomically:

1. **Task 1: Basis risk scoring and expiry warning system** - `42e7b91` (feat)

## Files Created/Modified
- `src/events/risk.rs` - BasisRiskScore, SourcePair, ExpiryWarning, compute_basis_risk, check_expiry_warning, inflate_risk_score, compute_risk_for_mapping (19 tests)
- `src/events/mod.rs` - Added `pub mod risk` export

## Decisions Made
- Unknown SourcePair uses index_oracle weight (0.5) as conservative default -- better to overestimate risk for unclassifiable source pairs than underestimate
- compute_risk_for_mapping derives prediction market resolution time from expiry date at 00:00:00 UTC when no explicit resolution timestamp is in the config -- reasonable default since most prediction markets resolve around midnight on the expiry date
- inflate_risk_score uses RiskWeightsConfig::default() for composite recalculation since weights are a global config property, not stored per-score

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Risk scoring module ready for use in Phase 5 Plan 03 (contract lifecycle/discovery) and Phase 6 (pricing engine)
- compute_risk_for_mapping() can be called on any EventMapping with settlement metadata
- ExpiryWarning integrates with existing ExpiryThreshold config from events.toml
- All 196 project tests pass (152 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc)

---
*Phase: 05-event-mapping*
*Completed: 2026-02-22*
