---
phase: 05-event-mapping
plan: 01
subsystem: events
tags: [toml_edit, event-registry, config-schema, lifecycle, approval]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: "Config loading, EventsConfig, EventMapping types, validation framework"
provides:
  - "Extended EventsConfig with approval, lifecycle, settlement, risk, discovery, expiry sections"
  - "Direction and LifecycleStatus typed enums"
  - "EventRegistry with O(1) dual-index lookup by instrument and event_id"
  - "Format-preserving TOML writer for auto-discovery candidate appending"
  - "CandidateMapping type for discovery pipeline input"
affects: [05-02, 05-03, 06-pricing-engine]

# Tech tracking
tech-stack:
  added: [toml_edit 0.22]
  patterns: [dual-index registry, format-preserving config write-back, typed enums for domain values]

key-files:
  created:
    - src/events/mod.rs
    - src/events/registry.rs
    - src/events/toml_writer.rs
  modified:
    - Cargo.toml
    - config/events.toml
    - src/config/events.rs
    - src/config/mod.rs
    - src/config/validation.rs
    - src/lib.rs
    - tests/smoke_test.rs

key-decisions:
  - "Direction enum replaces String for type-safe above/below handling"
  - "LifecycleStatus enum (Active/Expiring/Expired) with Default=Active for backward compat"
  - "All new EventMapping fields use #[serde(default)] for zero-breakage migration"
  - "EventRegistry indexes Polymarket by token_id (not condition_id) for pipeline instrument lookup"
  - "Expiry threshold validation checks uniqueness rather than ordering (thresholds naturally descend by hours)"

patterns-established:
  - "Dual-index registry pattern: HashMap<(Venue, String), usize> for O(1) instrument lookup"
  - "Format-preserving config modification via toml_edit DocumentMut parse-modify-emit"
  - "CandidateMapping as pipeline input type separate from config EventMapping"

requirements-completed: [EVNT-01]

# Metrics
duration: 11min
completed: 2026-02-22
---

# Phase 5 Plan 01: Event Mapping Registry Summary

**Extended events.toml schema with approval/lifecycle/settlement fields, dual-index EventRegistry with O(1) lookups, and toml_edit-based format-preserving TOML writer for auto-discovery**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-22T20:57:43Z
- **Completed:** 2026-02-22T21:09:07Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- Extended EventsConfig with risk_weights, discovery, expiry_thresholds top-level sections plus per-mapping approval, lifecycle status, settlement metadata
- Built EventRegistry with dual-index HashMap lookups: (Venue, instrument_id) and event_id, both O(1)
- Implemented format-preserving TOML append and expire operations via toml_edit, keeping user comments intact
- Added Direction and LifecycleStatus typed enums replacing raw strings
- 17 new unit tests covering all registry lookups, filtering, refresh, and TOML writer operations
- All 177 tests pass (133 lib + 16 integration + 22 smoke + 6 doctests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend EventMapping config schema and events.toml** - `9d33eef` (feat)
2. **Task 2: Build EventRegistry and format-preserving TOML writer** - `5e9d345` (feat)

## Files Created/Modified
- `Cargo.toml` - Added toml_edit 0.22 dependency
- `config/events.toml` - Extended schema with risk_weights, discovery, expiry_thresholds, settlement metadata
- `src/config/events.rs` - Direction, LifecycleStatus enums; RiskWeightsConfig, DiscoveryConfig, ExpiryThreshold, SettlementMetadata, SourcePairWeights structs; extended EventMapping
- `src/config/mod.rs` - Re-exports for all new config types
- `src/config/validation.rs` - Added expiry date, strike decimal, and threshold uniqueness validation
- `src/events/mod.rs` - Module root for events subsystem
- `src/events/registry.rs` - EventRegistry with dual-index lookup, active_approved filtering, refresh support
- `src/events/toml_writer.rs` - CandidateMapping type, append_candidate_to_toml, mark_expired_in_toml
- `src/lib.rs` - Added pub mod events
- `tests/smoke_test.rs` - Updated event count and ID assertions for new events.toml

## Decisions Made
- Direction enum (Above/Below) replaces String for type-safe handling with serde rename_all = "lowercase"
- LifecycleStatus enum defaults to Active for backward compatibility with existing mappings
- EventRegistry indexes Polymarket by token_id (the instrument-level identifier used in pipeline) rather than condition_id
- Expiry threshold validation validates uniqueness of hours_before_expiry and risk_inflation_factor >= 1.0, but does not enforce ordering (thresholds naturally listed descending by hours in config)
- All new EventMapping fields default for backward compatibility: approved defaults to true, status defaults to Active

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed expiry threshold ordering validation**
- **Found during:** Task 1 (validation implementation)
- **Issue:** Plan specified "ascending hours_before_expiry" validation, but the research example and events.toml list thresholds in descending order (48h -> 24h -> 6h)
- **Fix:** Changed validation to check uniqueness of hours_before_expiry instead of ascending order
- **Files modified:** src/config/validation.rs
- **Verification:** All tests pass with descending threshold ordering
- **Committed in:** 9d33eef (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug -- plan/research inconsistency)
**Impact on plan:** Minor correction. Validation still catches misconfigured thresholds (duplicates, inflation < 1.0) without rejecting the natural descending order from research.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- EventRegistry and TOML writer provide foundation for Phase 5 Plan 02 (basis risk scoring) and Plan 03 (contract lifecycle/discovery)
- Registry can be wired into pipeline for MarketSnapshot event_id annotation
- Discovery pipeline can use CandidateMapping + append_candidate_to_toml for auto-discovery write-back

---
*Phase: 05-event-mapping*
*Completed: 2026-02-22*
