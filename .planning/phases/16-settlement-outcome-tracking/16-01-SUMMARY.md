---
phase: 16-settlement-outcome-tracking
plan: 01
subsystem: settlement
tags: [settlement, resolution, deribit, kalshi, polymarket, polling, serde, decimal]

# Dependency graph
requires:
  - phase: 03-feed-handlers
    provides: VenueRateLimiter (governor-based rate limiting per venue)
  - phase: 04-kalshi-feed
    provides: sign_kalshi_request RSA-PSS auth function
  - phase: 01-config-types
    provides: Venue enum, Direction enum, EventMapping types
provides:
  - SettlementOutcome type for cross-venue outcome normalization
  - ResolutionResult enum for internal resolution pipeline
  - PollingTier state machine for four-tier cadence control
  - SettlementConfig with TOML-deserializable per-venue parameters
  - VenueChecker enum dispatch for Deribit/Kalshi/Polymarket
  - DeribitResolutionChecker (delivery price by index + expiry date match)
  - KalshiResolutionChecker (RSA-PSS auth, status dispatch, Rule 6.3(c))
  - PolymarketResolutionChecker (Gamma API two-stage closed + price lock)
  - SettlementRecord and SettledLeg for JSONL logging
  - SettlementDivergence for cross-venue disagreement annotation
affects: [16-02-settlement-monitor, 16-03-paper-trade-integration, 17-signal-quality]

# Tech tracking
tech-stack:
  added: []
  patterns: [enum-dispatch-for-async-traits, tagged-serde-enums, option-decimal-str-serde, two-stage-resolution-check]

key-files:
  created:
    - src/settlement/mod.rs
    - src/settlement/types.rs
    - src/settlement/traits.rs
    - src/settlement/config.rs
    - src/settlement/deribit.rs
    - src/settlement/kalshi.rs
    - src/settlement/polymarket.rs
  modified:
    - src/lib.rs

key-decisions:
  - "VenueChecker enum dispatch instead of async-trait crate -- zero new dependencies"
  - "CheckContext struct passes expiry/strike/direction to venue checkers alongside event_id/instrument"
  - "Deribit date field handled as serde_json::Value to support both string and timestamp formats"
  - "Kalshi scalar detection checks settlement_value_dollars for non-binary values even on yes/no results"
  - "Polymarket outcome_prices parsed as Vec<String> (JSON-in-JSON) with configurable threshold"

patterns-established:
  - "Enum dispatch for async venue abstractions: VenueChecker wraps concrete types, delegates via match"
  - "option_decimal_str serde module for Option<Decimal> with string representation"
  - "Tagged serde for enums: OutcomeKind uses #[serde(tag = \"kind\")] for human-readable JSON"
  - "Two-stage resolution: check authoritative status first, then validate with price data"

requirements-completed: [STTL-01, STTL-02, STTL-03, STTL-04]

# Metrics
duration: 8min
completed: 2026-02-25
---

# Phase 16 Plan 01: Settlement Types and Resolution Checkers Summary

**Settlement type system with PollingTier state machine and three venue-specific resolution checkers (Deribit delivery price, Kalshi RSA-PSS auth with 6.3(c) handling, Polymarket Gamma API two-stage check)**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-25T22:41:20Z
- **Completed:** 2026-02-25T22:49:18Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Complete settlement type system (SettlementOutcome, SettledLeg, SettlementRecord, SettlementDivergence) all Serialize/Deserialize with Decimal string fields
- PollingTier four-tier state machine (Waiting -> Aggressive -> Patient -> Lazy -> TimedOut) with configurable time thresholds
- Three venue resolution checkers covering all API-specific edge cases: Deribit index-keyed delivery prices, Kalshi multi-stage status with Rule 6.3(c) scalar ambiguity, Polymarket JSON-in-JSON price parsing with configurable lock threshold
- VenueChecker enum dispatch avoiding async-trait crate dependency
- 59 unit tests covering serde roundtrips, tier transitions, and all resolution logic paths

## Task Commits

Each task was committed atomically:

1. **Task 1: Create settlement types, config, and ResolutionChecker trait** - `84ce0db` (feat)
2. **Task 2: Implement Deribit, Kalshi, and Polymarket resolution checkers** - `a561b4c` (feat)

## Files Created/Modified
- `src/settlement/mod.rs` - Module root with re-exports
- `src/settlement/types.rs` - All settlement domain types (SettlementOutcome, SettledLeg, SettlementRecord, PollingTier, TrackedEvent, etc.)
- `src/settlement/traits.rs` - CheckContext struct and VenueChecker enum dispatch
- `src/settlement/config.rs` - SettlementConfig with serde(default) and sensible defaults
- `src/settlement/deribit.rs` - DeribitResolutionChecker with delivery price matching
- `src/settlement/kalshi.rs` - KalshiResolutionChecker with RSA-PSS auth and status dispatch
- `src/settlement/polymarket.rs` - PolymarketResolutionChecker with two-stage check
- `src/lib.rs` - Added `pub mod settlement;`

## Decisions Made
- **VenueChecker enum dispatch over async-trait:** Avoids adding async-trait crate dependency. The SettlementMonitor (Plan 02) will own concrete checker types, so object safety is not needed. Enum match dispatch is simpler and zero-cost.
- **CheckContext struct:** Deribit needs expiry/strike/direction to determine outcomes, but the trait-like interface only takes event_id and venue_instrument. Added CheckContext as a third parameter to carry this additional context uniformly.
- **Deribit date as serde_json::Value:** The delivery price API may return date as string or millisecond timestamp. Using Value with runtime parsing handles both formats gracefully.
- **Kalshi scalar detection on yes/no results:** Even when Kalshi reports result="yes", if settlement_value_dollars is fractional (not 0 or 1), it indicates a Rule 6.3(c) ambiguous resolution. The checker detects this automatically.
- **Polymarket prices as Vec<String>:** The Gamma API returns outcomePrices as a JSON string containing a JSON array of strings (double-encoded). Parsing as Vec<String> then converting to f64 handles this correctly.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed private module import paths**
- **Found during:** Task 1 (initial compilation)
- **Issue:** Plan referenced `crate::config::events::Direction` and `crate::types::venue::Venue` but these are private modules with public re-exports
- **Fix:** Changed to `crate::config::Direction` and `crate::types::Venue` per the actual re-export paths
- **Files modified:** src/settlement/types.rs, src/settlement/traits.rs, src/settlement/deribit.rs
- **Verification:** `cargo check` succeeds

**2. [Rule 3 - Blocking] Added missing Datelike trait import in Kalshi tests**
- **Found during:** Task 2 (test compilation)
- **Issue:** `DateTime::year()` requires `chrono::Datelike` trait import which was missing
- **Fix:** Added `use chrono::Datelike;` to Kalshi test module
- **Files modified:** src/settlement/kalshi.rs
- **Verification:** `cargo test --lib settlement::kalshi::tests` passes

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both were import path corrections required for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed import path issues.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All settlement types and venue checkers are ready for Plan 02 (SettlementMonitor task)
- VenueChecker enum can be constructed with concrete checker instances in the monitor
- SettlementConfig is ready for integration into SystemConfig (Plan 02/03)
- PollingTier state machine ready for the monitor's per-event tracking
- 462 total tests pass (59 new + 403 existing), no regressions

---
*Phase: 16-settlement-outcome-tracking*
*Completed: 2026-02-25*
