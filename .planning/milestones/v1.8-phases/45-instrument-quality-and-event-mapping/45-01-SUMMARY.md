---
phase: 45-instrument-quality-and-event-mapping
plan: 01
subsystem: events
tags: [polymarket, discovery, filtering, cli, instrument-quality, bid-ask-spread]

requires:
  - phase: 44-critical-bug-fixes
    provides: "Fixed cost normalization and spread logger for accurate pipeline operation"
provides:
  - "Polymarket price/spread filtering in discovery pipeline (min_polymarket_price, max_polymarket_spread)"
  - "match-audit CLI binary for instrument mapping validation"
  - "PolymarketMarketInfo with bestBid/bestAsk/spread fields"
affects: [45-02, signal-quality, production-deployment]

tech-stack:
  added: []
  patterns: ["string-to-f64 serde deserializer for Gamma API numeric fields", "inline discovery filtering with metrics counters"]

key-files:
  created:
    - src/bin/match_audit.rs
  modified:
    - src/events/discovery.rs
    - src/config/events.rs
    - src/events/lifecycle.rs
    - Cargo.toml

key-decisions:
  - "Gamma API returns bestBid/bestAsk/spread as JSON strings; custom deserializer handles string/number/null"
  - "Filter logic extracted as testable helper predicate alongside async function for unit test coverage"
  - "match-audit parses Deribit DDMMMYY and Derive YYYYMMDD expiry formats independently"

patterns-established:
  - "deserialize_option_f64_from_string: reusable serde deserializer for APIs returning numbers as strings"
  - "match-audit CLI pattern: load events.toml, validate mappings, table/json output, exit code 1 on errors"

requirements-completed: [INST-02, INST-03]

duration: 6min
completed: 2026-03-09
---

# Phase 45 Plan 01: Instrument Quality Filtering and Match Audit Summary

**Polymarket bid-ask spread filtering in discovery pipeline with configurable thresholds, plus match-audit CLI for instrument mapping validation**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-09T18:22:08Z
- **Completed:** 2026-03-09T18:28:18Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Polymarket markets with bestBid below $0.02 or spread above $0.10 are now filtered during discovery, preventing phantom liquidity contracts from entering the pipeline
- match-audit CLI validates venue count, expiry alignment, direction consistency, and moneyness for all approved+active event mappings
- 6 new unit tests for filtering logic and deserialization; all 655 existing tests continue to pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Polymarket price/spread filtering to discovery pipeline** - `90694e3` (feat)
2. **Task 2: Build match-audit CLI binary** - `40f2e3d` (feat)

## Files Created/Modified
- `src/config/events.rs` - Added max_polymarket_spread and min_polymarket_price fields to DiscoveryConfig
- `src/events/discovery.rs` - Added bestBid/bestAsk/spread to PolymarketMarketInfo, filtering in discover_polymarket_structured, custom serde deserializer, 6 new tests
- `src/events/lifecycle.rs` - Updated call site to pass filter thresholds
- `src/bin/match_audit.rs` - New match-audit CLI binary with venue/expiry/direction/moneyness validation
- `Cargo.toml` - Added [[bin]] entry for match-audit

## Decisions Made
- Used custom `deserialize_option_f64_from_string` to handle Gamma API returning numeric fields as JSON strings
- Extracted filter predicate as a testable helper function to enable unit testing without async runtime
- match-audit parses Deribit (DDMMMYY) and Derive (YYYYMMDD) expiry formats independently from instrument names

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added string-to-f64 serde deserializer for Gamma API fields**
- **Found during:** Task 1 (Polymarket filtering)
- **Issue:** Gamma API returns bestBid/bestAsk/spread as JSON strings, not numbers; naive f64 deserialization would fail
- **Fix:** Created `deserialize_option_f64_from_string` handling string, number, null, and empty string cases
- **Files modified:** src/events/discovery.rs
- **Verification:** Deserialization tests pass with string values
- **Committed in:** 90694e3 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Essential for correct deserialization of Gamma API responses. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Discovery pipeline now filters low-quality Polymarket markets automatically
- match-audit CLI ready for operator use to validate events.toml mappings
- Ready for Phase 45 Plan 02 (event mapping population and cross-venue matching)

---
*Phase: 45-instrument-quality-and-event-mapping*
*Completed: 2026-03-09*
