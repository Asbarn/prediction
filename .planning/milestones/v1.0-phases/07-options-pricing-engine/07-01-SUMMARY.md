---
phase: 07-options-pricing-engine
plan: 01
subsystem: pricing
tags: [black76, statrs, options, iv, deribit, instrument-parser]

# Dependency graph
requires:
  - phase: 02-deribit-feed
    provides: "TickerData with bid_iv, ask_iv, underlying_price, underlying_index fields"
  - phase: 01-foundation
    provides: "Domain types (InstrumentId, Probability, DualTimestamp), SystemConfig"
provides:
  - "src/pricing/ module with Black-76 pricer, types, config, instrument parser"
  - "PricingConfig with solver, vol_surface, probability, confidence sub-configs"
  - "MarketSnapshot extended with bid_iv, ask_iv, underlying_price, underlying_index"
  - "Deribit instrument name parser (asset, expiry, strike, option_type)"
affects: [07-02-iv-solver, 07-03-vol-surface, 07-04-probability, 07-05-integration, 08-signal-generation]

# Tech tracking
tech-stack:
  added: ["statrs 0.18 (Normal CDF/PDF for Black-76)"]
  patterns: ["f64 internal pricing math", "pub(crate) free functions for Black-76", "serde(default) nested config"]

key-files:
  created:
    - src/pricing/mod.rs
    - src/pricing/types.rs
    - src/pricing/config.rs
    - src/pricing/black76.rs
    - src/pricing/instrument.rs
  modified:
    - Cargo.toml
    - src/lib.rs
    - src/config/system.rs
    - config/config.toml
    - src/types/snapshot.rs
    - src/feed/deribit/normalize.rs
    - src/feed/kalshi/normalize.rs
    - src/feed/polymarket/normalize.rs
    - src/spread/engine.rs
    - src/spread/patterns.rs
    - src/paper_trade/tracker.rs
    - tests/smoke_test.rs

key-decisions:
  - "Black-76 functions are pub(crate) free functions (no struct wrapper needed)"
  - "Instrument parser handles both 1-digit and 2-digit day formats"
  - "statrs Normal::standard() used per-call (no global lazy_static needed)"

patterns-established:
  - "f64 internal pricing math with Decimal only at output boundary"
  - "Edge case handling: t <= 0 or sigma <= 0 returns intrinsic value"
  - "Nested serde(default) config pattern for PricingConfig sub-structs"

requirements-completed: [PRIC-01, PRIC-06]

# Metrics
duration: 11min
completed: 2026-02-23
---

# Phase 7 Plan 01: Pricing Module Scaffold Summary

**Black-76 pricer with put-call parity verification, Deribit instrument parser, PricingConfig, and MarketSnapshot options data extension**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-23T14:02:01Z
- **Completed:** 2026-02-23T14:13:21Z
- **Tasks:** 2/2
- **Files modified:** 17

## Accomplishments
- Black-76 call/put pricing verified against known ATM value (~7.97) and put-call parity (within 1e-10)
- Vega computation verified via finite-difference check (within 1e-4)
- MarketSnapshot extended with bid_iv, ask_iv, underlying_price, underlying_index from Deribit ticker
- PricingConfig with all solver, vol_surface, confidence, probability sub-configs with sensible defaults
- Deribit instrument name parser correctly handles "BTC-27JUN25-100000-C" format

## Task Commits

Each task was committed atomically:

1. **Task 1: Pricing module types, config, and instrument parser** - `152abd5` (feat)
2. **Task 2: Black-76 pricer and MarketSnapshot extension** - `4848826` (feat)

## Files Created/Modified
- `src/pricing/mod.rs` - Module exports for pricing sub-modules
- `src/pricing/types.rs` - ImpliedProbability, SolverResult, PricingMethod, ConfidenceComponents, OptionType, ParsedInstrument, InstrumentGreeks
- `src/pricing/config.rs` - PricingConfig with SolverConfig, VolSurfaceConfig, ConfidenceConfig, ProbabilityConfig
- `src/pricing/black76.rs` - Black-76 call_price, put_price, vega, d1_d2, intrinsic_value
- `src/pricing/instrument.rs` - parse_deribit_instrument() extracts asset/expiry/strike/type
- `src/types/snapshot.rs` - Added bid_iv, ask_iv, underlying_price, underlying_index fields
- `src/feed/deribit/normalize.rs` - Extended TickerState and build_snapshot for options data
- `src/config/system.rs` - Added PricingConfig to SystemConfig
- `Cargo.toml` - Added statrs 0.18 dependency

## Decisions Made
- Black-76 functions implemented as free functions (no struct needed for stateless math)
- Instrument parser handles both 1-digit and 2-digit day formats (e.g., "3JAN26" and "27JUN25")
- Normal::standard() created per function call rather than static/lazy (trivial allocation, simpler code)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed ambiguous numeric type in put-call parity test**
- **Found during:** Task 2 (Black-76 test compilation)
- **Issue:** `(-r * t).exp()` failed with "can't call method exp on ambiguous numeric type" when local variables were untyped float literals
- **Fix:** Added explicit `_f64` suffix to local variables in test
- **Files modified:** src/pricing/black76.rs
- **Verification:** Test compiles and passes
- **Committed in:** 4848826 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial type annotation fix, no scope change.

## Issues Encountered
None beyond the type annotation fix above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Black-76 pricer ready for IV solver (Plan 02) to use for Newton-Raphson root finding
- MarketSnapshot now carries all data needed for IV cross-validation against exchange mark_iv
- PricingConfig structure ready for all subsequent pricing plans to read their parameters
- Instrument parser ready for per-expiry vol surface grouping (Plan 03)

---
*Phase: 07-options-pricing-engine*
*Completed: 2026-02-23*
