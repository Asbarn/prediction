---
phase: 11-basis-risk-consumption
plan: 01
subsystem: events
tags: [basis-risk, cache, lifecycle, risk-scoring, settlement]

# Dependency graph
requires:
  - phase: 05-event-mapping
    provides: BasisRiskScore computation, ExpiryWarning, inflate_risk_score
  - phase: 10-critical-pipeline-wiring
    provides: Pipeline wiring with EventRegistry and lifecycle manager
provides:
  - BasisRiskCache shared type (Arc<RwLock<HashMap<String, CachedRiskInfo>>>)
  - CachedRiskInfo struct with base_score, expiry_warning, effective_composite, temporal_mismatch_hours
  - new_basis_risk_cache() helper for initialization
  - ContractLifecycleManager cache population on every poll cycle
  - basis_risk_scale config fields on SpreadConfig and SignalGenerationConfig
affects: [11-02-PLAN, spread-engine, signal-engine, cross-asset-engine]

# Tech tracking
tech-stack:
  added: []
  patterns: [shared-cache-via-arc-rwlock, serde-default-for-backward-compat]

key-files:
  created: []
  modified:
    - src/events/risk.rs
    - src/events/mod.rs
    - src/events/lifecycle.rs
    - src/spread/config.rs
    - src/signal/config.rs
    - src/main.rs

key-decisions:
  - "BasisRiskCache uses Arc<RwLock<HashMap>> for thread-safe sharing between lifecycle manager and engines"
  - "Cache cleared and rebuilt from scratch each poll cycle to evict expired mappings"
  - "temporal_mismatch_hours reverse-derived from settlement_time_risk / time_per_hour (original hours not stored)"
  - "basis_risk_scale defaults to 0.01 (1% of composite score) with serde(default) for backward compat"
  - "main.rs updated in Task 2 (Rule 3 auto-fix) to pass new_basis_risk_cache() to constructor"

patterns-established:
  - "Shared cache pattern: Arc<RwLock<HashMap>> populated by producer task, read by consumer engines"
  - "Config field addition: serde(default = fn_name) + Default impl for zero-breakage migration"

requirements-completed: [EVNT-02, EVNT-03, EVNT-05]

# Metrics
duration: 5min
completed: 2026-02-24
---

# Phase 11 Plan 01: BasisRiskCache and Lifecycle Integration Summary

**BasisRiskCache shared type with CachedRiskInfo populated every lifecycle poll cycle, plus basis_risk_scale config fields for downstream engine consumption**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-24T13:24:16Z
- **Completed:** 2026-02-24T13:29:28Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Created BasisRiskCache type alias and CachedRiskInfo struct for thread-safe risk data sharing
- Integrated cache population into ContractLifecycleManager poll cycle for all active_approved mappings
- Added basis_risk_scale field to SpreadConfig and SignalGenerationConfig (default 0.01, backward-compatible)
- Wired new_basis_risk_cache() into main.rs lifecycle manager construction

## Task Commits

Each task was committed atomically:

1. **Task 1: Create BasisRiskCache types and config fields** - `a13c85f` (feat)
2. **Task 2: Integrate BasisRiskCache into ContractLifecycleManager** - `bb33488` (feat)

## Files Created/Modified
- `src/events/risk.rs` - Added CachedRiskInfo struct, BasisRiskCache type alias, new_basis_risk_cache() helper
- `src/events/mod.rs` - Re-exported BasisRiskCache, CachedRiskInfo, new_basis_risk_cache
- `src/events/lifecycle.rs` - Added basis_risk_cache field, constructor param, poll cycle cache population
- `src/spread/config.rs` - Added basis_risk_scale field with default 0.01
- `src/signal/config.rs` - Added basis_risk_scale field with default 0.01
- `src/main.rs` - Wired new_basis_risk_cache() into lifecycle manager construction

## Decisions Made
- BasisRiskCache uses Arc<RwLock<HashMap<String, CachedRiskInfo>>> matching existing codebase patterns for shared state
- Cache is cleared and rebuilt from active_approved() mappings each poll cycle, ensuring expired mappings are automatically evicted
- temporal_mismatch_hours is reverse-derived from settlement_time_risk / time_per_hour since the original hours value is not stored in BasisRiskScore
- time_per_hour division uses max(0.001) clamp to prevent division by zero in edge cases
- basis_risk_scale defaults to Decimal(1, 2) = 0.01 using serde(default = "fn_name") pattern for backward-compatible TOML loading

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated main.rs to pass BasisRiskCache to lifecycle manager constructor**
- **Found during:** Task 2 (Integrate BasisRiskCache into ContractLifecycleManager)
- **Issue:** Constructor signature change from adding basis_risk_cache parameter broke main.rs compilation
- **Fix:** Added `use prediction::events::new_basis_risk_cache;` import and passed `new_basis_risk_cache()` to constructor call
- **Files modified:** src/main.rs
- **Verification:** `cargo check --bin prediction` passes cleanly
- **Committed in:** bb33488 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Auto-fix was necessary for compilation. Plan noted this might be deferred to Plan 02 but fixing inline was simpler and correct.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- BasisRiskCache is created and populated but not yet consumed by engines
- Plan 02 will wire cache into SpreadEngine, PricingEngine, and CrossAssetEngine for premium calculation
- basis_risk_scale config fields are ready for engines to use in premium = effective_composite * scale computation

---
*Phase: 11-basis-risk-consumption*
*Completed: 2026-02-24*
