---
phase: 25-tech-debt-sweep
plan: 01
subsystem: pricing, signal
tags: [iv-solver, options-pricing, deribit, config, book-depth]

# Dependency graph
requires:
  - phase: 08-pricing-engine
    provides: "PricingEngine, ImpliedProbability struct, IV solver pipeline"
  - phase: 09-signal-generation
    provides: "CrossAssetEngine, ArbSignal struct, signal pipeline"
  - phase: 02-deribit-feed
    provides: "DeribitClient, build_subscription_channels, DeribitConfig"
provides:
  - "iv_spread flows from IV solver through ImpliedProbability to ArbSignal (not hardcoded 0.0)"
  - "options_book_depth on ImpliedProbability carries actual snapshot depth to signal layer"
  - "book_depth_levels on DeribitConfig controls Deribit book channel subscription depth"
  - "Backward-compatible serde defaults for book_depth_levels (20)"
affects: [confidence-scoring, signal-analysis, deribit-config]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Serde default functions for backward-compatible config additions"
    - "Propagating runtime data (snapshot depth) through typed pipeline stages"

key-files:
  created: []
  modified:
    - src/pricing/types.rs
    - src/pricing/engine.rs
    - src/signal/engine.rs
    - src/config/venues.rs
    - src/feed/deribit/channels.rs
    - src/feed/deribit/client.rs
    - src/config/validation.rs
    - tests/pipeline_test.rs

key-decisions:
  - "iv_spread clamped with .max(0.0) in normal pricing path to prevent negative values"
  - "options_book_depth uses depth_bids.len() as the proxy for snapshot depth (bids and asks always symmetric from Deribit)"
  - "Near-expiry path sets iv_spread=0.0 and options_book_depth=0 (no IV solver, no snapshot depth)"

patterns-established:
  - "Data propagation through ImpliedProbability: runtime values from PricingEngine flow through to CrossAssetEngine/ArbSignal"

requirements-completed: [FIX-01, FIX-02]

# Metrics
duration: 7min
completed: 2026-02-28
---

# Phase 25 Plan 01: Fix iv_spread Propagation and Config-Driven Book Depth Summary

**iv_spread now carries actual ask_iv - bid_iv from IV solver to ArbSignal; Deribit book depth configurable via DeribitConfig with actual snapshot depth reported on options leg**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-28T06:56:49Z
- **Completed:** 2026-02-28T07:04:11Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- iv_spread on ArbSignal is populated from the actual IV solver bid-ask spread (ask_iv - bid_iv), replacing the hardcoded 0.0 that has been present since v1.0
- Deribit book subscription channel depth is now config-driven via DeribitConfig.book_depth_levels (valid: 1, 10, 20), with backward-compatible default of 20
- Options leg book_depth_levels on ArbSignal now reflects the actual number of depth levels in the source Deribit snapshot, replacing hardcoded 0
- All 548 unit tests + 3 doc-tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Propagate iv_spread through ImpliedProbability** - `d9fbe43` (fix)
2. **Task 2: Config-driven book depth and actual options depth** - `d02564e` (fix)

## Files Created/Modified
- `src/pricing/types.rs` - Added iv_spread and options_book_depth fields to ImpliedProbability
- `src/pricing/engine.rs` - Populated iv_spread and options_book_depth in both normal and near-expiry paths
- `src/signal/engine.rs` - Replaced hardcoded 0.0 iv_spread with prob.iv_spread; replaced hardcoded 0 book_depth_levels with prob.options_book_depth
- `src/config/venues.rs` - Added book_depth_levels field to DeribitConfig with serde default of 20
- `src/feed/deribit/channels.rs` - Parameterized build_subscription_channels with book_depth_levels argument
- `src/feed/deribit/client.rs` - Updated call site to pass config.book_depth_levels
- `src/config/validation.rs` - Added book_depth_levels to test DeribitConfig construction
- `tests/pipeline_test.rs` - Added book_depth_levels to all DeribitConfig test constructions

## Decisions Made
- iv_spread clamped with `.max(0.0)` in normal pricing path: prevents negative spread values from floating-point edge cases (ask_iv and bid_iv are independently solved)
- Used `depth_bids.len()` as options_book_depth proxy: Deribit grouped book always returns symmetric bid/ask depth, so either side length is correct
- Near-expiry path uses iv_spread=0.0 and options_book_depth=0: these are semantically correct since near-expiry uses intrinsic pricing with no IV solver and no book snapshot

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated DeribitConfig struct literals in validation.rs and pipeline_test.rs**
- **Found during:** Task 2
- **Issue:** Plan only mentioned updating call sites in channels.rs tests and client.rs. Adding a non-defaulted struct field to DeribitConfig caused compilation errors in test files that construct DeribitConfig with struct literal syntax (validation.rs, pipeline_test.rs)
- **Fix:** Added `book_depth_levels: 20` to all 4 DeribitConfig struct literal constructions in test code
- **Files modified:** src/config/validation.rs, tests/pipeline_test.rs
- **Verification:** cargo build + cargo test pass
- **Committed in:** d02564e (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to compile. The serde default only works for deserialization, not struct literal construction. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Plan 25-02 (remaining tech debt items) can proceed
- iv_spread and book depth data now available for downstream confidence scoring improvements
- Operators can tune Deribit book depth via config without code changes

## Self-Check: PASSED

All 9 modified/created files verified present. Both task commits (d9fbe43, d02564e) verified in git log.

---
*Phase: 25-tech-debt-sweep*
*Completed: 2026-02-28*
