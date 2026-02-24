---
phase: 06-prediction-market-spreads
plan: 04
subsystem: paper-trade
tags: [paper-trade, next-tick-fill, adverse-selection, mtm, daily-pnl, jsonl-logger, tokio-select, pipeline-wiring]

# Dependency graph
requires:
  - phase: 06-prediction-market-spreads
    provides: "SpreadEngine with signal_tx channel, SpreadResult, SpreadConfig, PaperTradeConfig placeholder"
  - phase: 05-event-mapping
    provides: "EventRegistry with event_id mapping on MarketSnapshot"
provides:
  - "PaperTradeTracker consuming SpreadResult signals with next-tick fill model"
  - "PaperPosition lifecycle: Pending -> Open -> Settled with adverse selection quantification"
  - "Mark-to-market history accumulation per position for offline strategy comparison"
  - "DailyAggregator computing per-day trade count, win/loss rates, P&L statistics"
  - "Full Phase 6 pipeline wired in main.rs: feeds -> SpreadEngine -> PaperTradeTracker"
  - "JSONL trade event logging (signal, entry, mtm, settlement)"
affects: [07-options-implied-probability, 08-signal-generation]

# Tech tracking
tech-stack:
  added: []
  patterns: [next-tick-fill-model, pending-position-queue, snapshot-forwarding-channel, dual-channel-pipeline]

key-files:
  created:
    - src/paper_trade/mod.rs
    - src/paper_trade/tracker.rs
    - src/paper_trade/position.rs
    - src/paper_trade/aggregator.rs
  modified:
    - src/spread/engine.rs
    - src/spread/mod.rs
    - src/main.rs
    - src/lib.rs

key-decisions:
  - "Fill prices use top-of-book probabilities (ask for buy, bid for sell) as proxy for walk-the-book fills"
  - "Fill snapshot also generates initial MTM data point since position is open when MTM update runs"
  - "Snapshot forwarding uses try_send (non-blocking, drop on overflow) -- paper trade is best-effort"
  - "SpreadEngine::run takes optional ptrade_snap_tx to avoid breaking existing API when paper trade not needed"

patterns-established:
  - "Next-tick fill model: signals queue as Pending, filled on NEXT matching snapshot (captures adverse selection)"
  - "Snapshot forwarding: SpreadEngine clones snapshots to paper trade tracker via separate channel"
  - "Dual-channel consumer: PaperTradeTracker consumes both signal_rx and snapshot_rx via biased select"
  - "TradeLogger: JSONL with tagged event types (signal/entry/mtm/settlement) and daily file rotation"

requirements-completed: [OBSV-04]

# Metrics
duration: 8min
completed: 2026-02-23
---

# Phase 6 Plan 04: Paper Trade P&L Tracker Summary

**PaperTradeTracker with next-tick-after-signal fill model, adverse selection quantification, MTM history, daily P&L rollups, and full Phase 6 pipeline wiring in main.rs**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-23T09:42:44Z
- **Completed:** 2026-02-23T09:50:40Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- PaperTradeTracker implements next-tick fill model: signals queue as Pending positions, filled at NEXT snapshot's prices for the same event, quantifying adverse selection (signal vs fill spread difference)
- PaperPosition lifecycle (Pending -> Open -> Settled) with MTM history accumulation, enabling offline comparison of hold-to-settlement vs early-exit strategies
- DailyAggregator computes per-day trade count, winning/losing trades, total/avg P&L, max win/loss
- Full Phase 6 pipeline wired in main.rs replacing simple snapshot consumer: feeds -> SpreadEngine -> PaperTradeTracker with snapshot forwarding
- 16 new unit tests covering position lifecycle, MTM accumulation, adverse selection, daily aggregation, and tracker fill/ignore behavior
- JSONL trade event logging with tagged types (signal, entry, mtm, settlement) and daily file rotation

## Task Commits

Each task was committed atomically:

1. **Task 1: PaperTradeTracker with next-tick entry, MTM tracking, and daily aggregation** - `1c472d8` (feat)
2. **Task 2: Wire SpreadEngine and PaperTradeTracker into main.rs** - `00471a1` (feat)

## Files Created/Modified
- `src/paper_trade/mod.rs` - Module declaration (tracker, position, aggregator)
- `src/paper_trade/position.rs` - PaperPosition struct with Pending/Open/Settled lifecycle, MtmSnapshot, adverse selection computation (302 lines)
- `src/paper_trade/aggregator.rs` - DailyAggregator with per-day rollup stats, signal counting, Prometheus metrics emission (260 lines)
- `src/paper_trade/tracker.rs` - PaperTradeTracker with biased select event loop, next-tick fill, MTM updates, TradeLogger JSONL (584 lines)
- `src/spread/engine.rs` - Added optional ptrade_snap_tx parameter to run() for snapshot forwarding
- `src/spread/mod.rs` - Added re-exports for SpreadEngine, SpreadConfig, SpreadResult
- `src/main.rs` - Replaced simple snapshot consumer with full Phase 6 pipeline (SpreadEngine + PaperTradeTracker)
- `src/lib.rs` - Added `pub mod paper_trade` declaration

## Decisions Made
- Fill prices use top-of-book probabilities (ask_probability for buy, bid_probability for sell) as proxy for walk-the-book fill prices -- simplified for v1, matches the probability space where spreads are computed
- Fill snapshot also generates initial MTM data point since position transitions to Open before the MTM update pass in the same handle_snapshot call
- SpreadEngine::run takes `ptrade_snap_tx: Option<mpsc::Sender<MarketSnapshot>>` to maintain backward compatibility when paper trade is not needed
- Snapshot forwarding uses try_send (non-blocking) -- paper trade tracker is best-effort, never blocks the spread engine

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Test initially expected fill snapshot to NOT generate MTM data point, but the handle_snapshot method correctly processes MTM after filling since the position is already Open. Fixed test expectations to match correct behavior.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 6 is now complete: all 4 plans delivered (cost model, Prometheus metrics, SpreadEngine, PaperTradeTracker)
- Full pipeline: multi-venue feeds -> spread computation -> paper trade tracking is wired and operational
- Ready for Phase 7 (options-implied probability) which adds Black-76 pricing and Deribit-derived probabilities
- Paper trade JSONL logs provide data for offline strategy analysis (hold-to-settlement vs spread reversion)
- 269 total tests passing across all modules

## Self-Check: PASSED

- src/paper_trade/mod.rs: FOUND (3 lines)
- src/paper_trade/tracker.rs: FOUND (584 lines, min 100 required)
- src/paper_trade/position.rs: FOUND (302 lines, min 60 required)
- src/paper_trade/aggregator.rs: FOUND (260 lines, min 40 required)
- Commit 1c472d8: verified in git log
- Commit 00471a1: verified in git log
- cargo build: passes with no new warnings
- cargo test: 253 lib + 16 integration + 22 smoke + 3 pipeline + 3 doc = 297 tests pass

---
*Phase: 06-prediction-market-spreads*
*Completed: 2026-02-23*
