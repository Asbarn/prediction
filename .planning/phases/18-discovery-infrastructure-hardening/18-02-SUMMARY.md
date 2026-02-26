---
phase: 18-discovery-infrastructure-hardening
plan: 02
subsystem: events
tags: [rate-limiter, absence-tracking, partial-response, batched-writes, toml_edit, lifecycle, discovery]

# Dependency graph
requires:
  - phase: 18-discovery-infrastructure-hardening
    plan: 01
    provides: "DiscoveryConfig thresholds, batch TOML mutation functions (append_candidates_to_doc, mark_expired_batch_in_doc)"
  - phase: 16-settlement-verification
    provides: "VenueRateLimiter in feed pipeline, shared via pipeline_handles.venue_rate_limiters"
provides:
  - "Hardened poll_cycle with AbsenceTracker, partial-response detection, batched writes, shared rate limiters"
  - "Rate-limited discover_deribit and discover_kalshi accepting VenueRateLimiter parameter"
  - "Shared venue_rate_limiters passed from pipeline to ContractLifecycleManager"
  - "Windows-safe atomic_write with remove-before-rename"
affects: [discovery-polling, lifecycle-manager, venue-rate-budgets]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Shared Arc-wrapped VenueRateLimiter instances between feed, settlement, and discovery"
    - "Consecutive-absence tracking before instrument expiry (configurable threshold)"
    - "Partial-response detection skipping expiry evaluation on suspect API responses"
    - "Single batched TOML write per poll cycle (parse once, mutate N, write once)"

key-files:
  created: []
  modified:
    - "src/events/lifecycle.rs"
    - "src/events/discovery.rs"
    - "src/main.rs"

key-decisions:
  - "Refactored handle_deribit_roll to pure find_deribit_roll returning Option<CandidateMapping> for batched write compatibility"
  - "Kalshi absence tracking uses ticker field (not instrument) matching KalshiMapping struct"
  - "Windows atomic write uses remove-before-rename (#[cfg(target_os = windows)]) to prevent rename failures"

patterns-established:
  - "consecutive absence: N polls must confirm instrument gone before marking expired"
  - "partial response guard: >threshold% drop in instrument count skips expiry evaluation"
  - "batched TOML write: all mutations from one poll cycle in a single atomic write"

requirements-completed: [DISC-02, LIFE-04, INTG-03]

# Metrics
duration: 8min
completed: 2026-02-26
---

# Phase 18 Plan 02: Lifecycle Integration Summary

**Hardened poll_cycle with shared rate limiters from pipeline, consecutive-absence expiry guards, partial-response detection, and single batched TOML write per cycle**

## Performance

- **Duration:** 8m 23s
- **Started:** 2026-02-26T21:59:51Z
- **Completed:** 2026-02-26T22:08:14Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Wired shared VenueRateLimiter instances from feed pipeline through to discovery polling, ensuring discovery, feeds, and settlement all share the same rate budget per venue
- Replaced single-absence expiry with AbsenceTracker requiring N consecutive absences (default 3) before marking instruments expired, preventing false expirations from transient API issues
- Added PreviousPollCounts-based partial-response detection that logs a warning and skips expiry evaluation when instrument count drops >20% from previous poll
- Replaced per-item TOML writes (append_candidate + mark_expired per mapping) with a single batched_toml_write per poll cycle using parse-once-mutate-N-write-once pattern
- Added Windows #[cfg(target_os = "windows")] remove-before-rename guard in atomic_write

## Task Commits

Each task was committed atomically:

1. **Task 1: Add rate limiter parameter to discovery functions and add AbsenceTracker to lifecycle** - `23de6f1` (feat)
2. **Task 2: Refactor poll_cycle for batched writes, absence tracking, and partial-response detection** - `85ab1e8` (feat)
3. **Task 3: Wire shared venue rate limiters from pipeline into lifecycle manager in main.rs** - `87434ca` (feat)

## Files Created/Modified
- `src/events/discovery.rs` - Added VenueRateLimiter import, Option<&VenueRateLimiter> param to discover_deribit and discover_kalshi with limiter.wait() before HTTP requests
- `src/events/lifecycle.rs` - Added AbsenceTracker, PreviousPollCounts structs; refactored poll_cycle for batched writes, absence tracking, partial-response detection; replaced handle_deribit_roll with find_deribit_roll; added batched_toml_write method; Windows atomic_write guard
- `src/main.rs` - Cloned pipeline_handles.venue_rate_limiters and passed to ContractLifecycleManager::new()

## Decisions Made
- Refactored handle_deribit_roll from an async method that wrote directly to TOML into a pure find_deribit_roll returning Option<CandidateMapping>, enabling batched write compatibility
- Used Kalshi's `ticker` field (not `instrument`) for absence tracking since KalshiMapping struct uses `ticker` while DeribitMapping uses `instrument`
- Added Windows-specific remove-before-rename in atomic_write using #[cfg(target_os = "windows")] since Windows rename fails when destination exists

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed Kalshi field name in absence tracking**
- **Found during:** Task 2 (poll_cycle refactor)
- **Issue:** Plan pseudocode used `kalshi.instrument` but KalshiMapping struct has `ticker` field
- **Fix:** Changed all Kalshi absence tracking references from `kalshi.instrument` to `kalshi.ticker`
- **Files modified:** src/events/lifecycle.rs
- **Verification:** cargo check passes, field access correct
- **Committed in:** 85ab1e8 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Essential field name correction. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 18 (Discovery Infrastructure Hardening) is complete -- both plans (foundation types + lifecycle integration) are done
- All 576 tests pass, zero compilation errors
- The hardened poll cycle is production-ready: shared rate limiters prevent venue bans, consecutive-absence prevents false expirations, partial-response detection catches degraded API responses, batched writes eliminate write/file-watcher race conditions
- Ready for Phase 19 (next v1.2 phase per ROADMAP)

---
*Phase: 18-discovery-infrastructure-hardening*
*Completed: 2026-02-26*
