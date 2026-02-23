---
phase: 08-cross-asset-signal-generation
plan: 02
subsystem: signal
tags: [cross-asset, arb-signal, dual-input, fan-out, threshold, spread-computation]

# Dependency graph
requires:
  - phase: 08-cross-asset-signal-generation
    plan: 01
    provides: "ArbSignal, ArbDirection, ThresholdStatus, CostBreakdown, LegInfo types; SignalGenerationConfig; SignalLogger"
  - phase: 06-prediction-market-spreads
    provides: "walk_the_book, polymarket_fee, kalshi_taker_fee, carry_cost, RollingStats, compute_threshold"
  - phase: 07-options-pricing-engine
    provides: "PricingEngine producing ImpliedProbability, 2-way fan-out in main.rs"
provides:
  - "CrossAssetEngine pairing options-implied probabilities with prediction market prices"
  - "3-way fan-out distributing MarketSnapshots to SpreadEngine, PricingEngine, and CrossAssetEngine"
  - "ArbSignal output channel ready for Phase 9 consumption"
  - "signal_generation field in SystemConfig with serde(default) backward compatibility"
affects: [09-backtesting-validation]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Dual-input tokio::select! loop for cross-asset pairing (prob_rx + pred_snap_rx)"
    - "3-way fan-out: blocking send to primary + try_send to secondary + try_send to tertiary"
    - "Liquidity factor reduces effective edge (not threshold): net_edge = (raw_spread - total_cost) * min(pred_factor, options_factor)"

key-files:
  created:
    - src/signal/engine.rs
  modified:
    - src/signal/mod.rs
    - src/config/system.rs
    - src/main.rs

key-decisions:
  - "Liquidity factor computed as min(prediction_fill_ratio, options_ba_proxy) where options proxy = max(0.1, 1.0 - ba_spread * 5.0)"
  - "Options fee estimate in same space as prediction fees (USD-scale): deribit_taker_fee_rate * underlying_price * |delta|"
  - "Both ArbDirection variants computed per event update regardless of sign, all logged to JSONL"

patterns-established:
  - "CrossAssetEngine follows SpreadEngine pattern: struct + run() async method, HashMap caches, biased select! loop"
  - "Fan-out clones before blocking send to preserve originals for try_send consumers"

requirements-completed: [SGNL-01, SGNL-05, SGNL-06]

# Metrics
duration: 10min
completed: 2026-02-23
---

# Phase 8 Plan 02: CrossAssetEngine and Pipeline Wiring Summary

**CrossAssetEngine with dual-input event loop pairing options-implied probability with prediction market prices, full cost model, dynamic threshold, 3-way fan-out, and ArbSignal output channel**

## Performance

- **Duration:** 10 min
- **Started:** 2026-02-23T16:21:28Z
- **Completed:** 2026-02-23T16:31:45Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- CrossAssetEngine pairs ImpliedProbability (from PricingEngine via EventRegistry) with prediction market MarketSnapshots, computing both spread directions with full cost model (prediction fees, options fee estimate, carry, slippage, options spread cost, combined liquidity factor)
- Dynamic threshold evaluation using existing compute_threshold infrastructure with three-tier status logging (PassedBoth, PassedStaticOnly, Filtered) -- all computations logged to JSONL
- 3-way fan-out in main.rs distributing snapshots to SpreadEngine (blocking), PricingEngine (try_send), and CrossAssetEngine (try_send)
- 348 total tests passing with 0 regressions, 3 new engine tests (staleness gate, both directions, threshold classification)

## Task Commits

Each task was committed atomically:

1. **Task 1: CrossAssetEngine with dual-input event loop and spread computation** - `af340c8` (feat)
2. **Task 2: Pipeline wiring with 3-way fan-out and config integration** - `b20d5e4` (feat)

## Files Created/Modified
- `src/signal/engine.rs` - CrossAssetEngine with dual-input event loop, event pairing, directional spread computation, cost model, dynamic threshold, JSONL logging, Prometheus metrics, 3 unit tests
- `src/signal/mod.rs` - Added pub mod engine and pub use CrossAssetEngine re-export
- `src/config/system.rs` - Added signal_generation: SignalGenerationConfig field with #[serde(default)]
- `src/main.rs` - Extended fan-out from 2-way to 3-way, consumed probability_rx (was _probability_rx), spawned CrossAssetEngine with ArbSignal output channel

## Decisions Made
- Liquidity factor computed as min(prediction market fill_ratio, options bid-ask proxy) where options proxy = max(0.1, 1.0 - ba_spread * 5.0). This follows the user decision that liquidity reduces effective edge, not threshold
- Options fee estimate uses deribit_taker_fee_rate * underlying_price * |delta| as approximate taker fee for the options leg, consistent with the cost model operating in USD-scale quantities
- Both ArbDirection variants (BuyPredictionSellOptions, SellPredictionBuyOptions) computed on every event update and all logged to JSONL regardless of threshold status per user decision
- Fan-out clones snapshots before the blocking send to SpreadEngine so that PricingEngine and CrossAssetEngine receive independent copies via try_send

## Deviations from Plan

None -- plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None -- no external service configuration required.

## Next Phase Readiness
- Phase 8 complete: all cross-asset signal generation requirements fulfilled (SGNL-01, SGNL-05, SGNL-06)
- ArbSignal output channel (_arb_signal_rx) held in scope and ready for Phase 9 backtesting/validation consumption
- CrossAssetEngine starts and shuts down cleanly with CancellationToken
- 348 tests passing across all modules with zero regressions

---
*Phase: 08-cross-asset-signal-generation*
*Completed: 2026-02-23*
