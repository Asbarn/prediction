---
phase: 30-venue-type-foundation
plan: 01
subsystem: types
tags: [venue, enum, config, toml, derive, type-system]

# Dependency graph
requires: []
provides:
  - "Venue::Derive enum variant with Display and env_prefix"
  - "DeriveConfig struct for venues.toml deserialization"
  - "DeriveMapping struct for event-venue instrument mapping"
  - "[derive] section in venues.toml with Lyra Finance WebSocket URL"
  - "All exhaustive match arms resolved across 15+ codebase sites"
affects: [31-derive-feed-stack, 32-derive-pipeline-integration, 33-usdc-normalization]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Derive follows Deribit pattern for options venue integration (zero fee, 08:00 UTC expiry)"

key-files:
  created: []
  modified:
    - src/types/venue.rs
    - src/config/venues.rs
    - src/config/events.rs
    - src/config/mod.rs
    - config/venues.toml
    - src/events/registry.rs
    - src/events/discovery.rs
    - src/events/lifecycle.rs
    - src/events/toml_writer.rs
    - src/events/risk.rs
    - src/spread/patterns.rs
    - src/settlement/monitor.rs
    - src/config/validation.rs
    - tests/pipeline_test.rs

key-decisions:
  - "Derive replay processor skips with warning (DeriveProcessor deferred to Phase 31)"
  - "Derive settlement uses DeribitDelivery resolution source (same options delivery model)"
  - "Derive discovery matching deferred to Phase 31 (empty match arms for now)"
  - "CandidateVenues extended with derive field for future cross-venue matching"

patterns-established:
  - "New venue integration pattern: add enum variant, config struct, mapping struct, TOML section, then resolve all match arms"
  - "Options venues (Deribit, Derive) use zero fee in prediction market spread context"

requirements-completed: [PIPE-01, PIPE-02]

# Metrics
duration: 21min
completed: 2026-03-04
---

# Phase 30 Plan 01: Venue Type Foundation Summary

**Venue::Derive enum variant with DeriveConfig/DeriveMapping structs, [derive] venues.toml section, and all exhaustive match arms resolved across 14 files**

## Performance

- **Duration:** 21 min
- **Started:** 2026-03-04T11:40:04Z
- **Completed:** 2026-03-04T12:01:01Z
- **Tasks:** 2
- **Files modified:** 14

## Accomplishments
- Added Venue::Derive to the type system with full Display, env_prefix, and serde support
- Created DeriveConfig (ws_url, rate_limit, book_depth, staleness, reconnect, instruments) and DeriveMapping structs
- Added [derive] section to venues.toml pointing to wss://api.lyra.finance/ws
- Resolved all exhaustive match arms across 14 files with semantically correct behavior (zero todo!/unreachable!)
- EventRegistry now indexes Derive instruments for O(1) lookup
- All 605 existing lib tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Venue::Derive variant and config structs** - `74b2a3b` (feat)
2. **Task 2: Resolve all exhaustive match arms and registry indexing** - `5756cdf` (feat)

## Files Created/Modified
- `src/types/venue.rs` - Added Derive variant to Venue enum
- `src/config/venues.rs` - Added DeriveConfig struct with all required fields
- `src/config/events.rs` - Added DeriveMapping struct and derive field on EventVenues
- `src/config/mod.rs` - Re-exported DeriveConfig and DeriveMapping
- `config/venues.toml` - Added [derive] section with Lyra Finance WebSocket URL
- `src/events/registry.rs` - Added Derive instrument indexing in build_indexes()
- `src/events/discovery.rs` - Added Derive no-op arms in exact/fuzzy matching, derive field in CandidateVenues
- `src/events/lifecycle.rs` - Added derive field to CandidateVenues and EventVenues constructions
- `src/events/toml_writer.rs` - Added derive field to CandidateVenues struct and TOML builder
- `src/events/risk.rs` - Added derive field to test EventVenues constructions
- `src/spread/patterns.rs` - Added Derive venue pair labels (deribit_derive, derive_polymarket, derive_kalshi)
- `src/settlement/monitor.rs` - Added Derive polling tier (08:00 UTC expiry) and resolution source
- `src/config/validation.rs` - Added DeriveConfig to test VenuesConfig
- `tests/pipeline_test.rs` - Added DeriveConfig to test VenuesConfig

## Decisions Made
- Derive replay processor uses skip-with-warning pattern (graceful degradation, not todo! placeholder) -- DeriveProcessor comes in Phase 31
- Derive settlement resolution maps to DeribitDelivery (same options delivery mechanism) -- a DeriveSettlement variant may be added in Phase 31+ if settlement logic diverges
- Derive discovery matching uses empty match arms -- full Derive instrument discovery is Phase 31 scope
- CandidateVenues extended with `derive: Option<String>` field and TOML serialization support for future cross-venue matching

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added derive field to CandidateVenues struct**
- **Found during:** Task 2
- **Issue:** Plan did not mention CandidateVenues (in toml_writer.rs), which also needs a derive field for completeness
- **Fix:** Added `pub derive: Option<String>` to CandidateVenues, added Derive TOML table building in build_candidate_table(), added derive: None to all CandidateVenues constructions
- **Files modified:** src/events/toml_writer.rs, src/events/discovery.rs, src/events/lifecycle.rs
- **Verification:** cargo check --tests passes
- **Committed in:** 5756cdf (Task 2 commit)

**2. [Rule 2 - Missing Critical] Added derive field to all EventVenues struct constructions in test code**
- **Found during:** Task 2
- **Issue:** Adding derive field to EventVenues causes all test struct literal constructions to fail compilation
- **Fix:** Added derive: None to EventVenues constructions in registry, discovery, lifecycle, risk, settlement, validation tests
- **Files modified:** src/events/registry.rs, src/events/discovery.rs, src/events/lifecycle.rs, src/events/risk.rs, src/settlement/monitor.rs, src/config/validation.rs
- **Verification:** cargo test --lib (605 tests pass)
- **Committed in:** 5756cdf (Task 2 commit)

**3. [Rule 2 - Missing Critical] Added DeriveConfig to test VenuesConfig constructions**
- **Found during:** Task 2
- **Issue:** VenuesConfig now requires a derive field, breaking test constructions in validation.rs and tests/pipeline_test.rs
- **Fix:** Added DeriveConfig with Lyra Finance defaults to all test VenuesConfig constructions
- **Files modified:** src/config/validation.rs, tests/pipeline_test.rs
- **Verification:** cargo check --tests passes
- **Committed in:** 5756cdf (Task 2 commit)

**4. [Rule 2 - Missing Critical] Re-exported DeriveConfig and DeriveMapping from config module**
- **Found during:** Task 2
- **Issue:** DeriveConfig and DeriveMapping not accessible via crate::config, only crate::config::venues/events
- **Fix:** Added re-exports to src/config/mod.rs
- **Files modified:** src/config/mod.rs
- **Verification:** cargo check --tests passes
- **Committed in:** 74b2a3b (Task 1 commit)

---

**Total deviations:** 4 auto-fixed (4 missing critical)
**Impact on plan:** All auto-fixes necessary for compilation correctness. Adding a new field to an existing struct requires updating all construction sites. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Venue::Derive fully integrated into type system -- all code compiles and tests pass
- DeriveConfig ready for deserialization from venues.toml
- EventRegistry indexes Derive instruments -- ready for DeriveMapping entries in events.toml
- Phase 31 (Derive feed stack) can proceed: DeriveProcessor, DeribitSupervisor-equivalent, WebSocket client
- Phase 32 (pipeline integration) can proceed: venue health, rate limiter, pipeline wiring

---
*Phase: 30-venue-type-foundation*
*Completed: 2026-03-04*
