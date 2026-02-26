---
phase: 16-settlement-outcome-tracking
plan: 03
subsystem: settlement
tags: [settlement, paper-trade, checkpoint, pipeline, mpsc, jsonl, prometheus, divergence, pnl]

# Dependency graph
requires:
  - phase: 16-settlement-outcome-tracking
    provides: SettlementOutcome, SettledLeg, SettlementRecord, SettlementDivergence, VenueChecker, SettlementConfig, SettlementMonitor
  - phase: 15-state-persistence
    provides: CheckpointState v1, atomic_write, PaperTradeTracker checkpoint/restore
  - phase: 13-paper-trade
    provides: PaperTradeTracker, PaperPosition, DailyAggregator, TradeLogger
  - phase: 03-feed-handlers
    provides: VenueRateLimiter (Arc<GovernorLimiter>) per venue
provides:
  - PaperTradeTracker settlement_rx channel arm consuming SettlementOutcome
  - Per-leg settlement with raw and fee-adjusted P&L computation
  - PartiallySettled position status for multi-venue positions
  - Cross-venue divergence detection and annotation
  - SettlementLogger with daily-rotating JSONL for settlement records
  - Recently settled positions with bounded eviction (48h/100 entries)
  - CheckpointState v2 with settlement_tracking HashMap
  - SettlementTrackingEntry for polling tier persistence across restarts
  - SettlementConfig on SystemConfig with TOML defaults
  - Shared VenueRateLimiters from PipelineHandles for settlement
  - SettlementMonitor wired in main.rs with checkpoint restore and backfill
  - Prometheus metrics at settlement (settled_total, net_pnl, latency, divergence)
affects: [17-signal-quality]

# Tech tracking
tech-stack:
  added: []
  patterns: [shared-arc-rwlock-for-cross-task-state, per-leg-settlement-with-independent-venue-outcomes, channel-based-settlement-delivery]

key-files:
  created: []
  modified:
    - src/paper_trade/position.rs
    - src/paper_trade/tracker.rs
    - src/persistence/checkpoint.rs
    - src/config/system.rs
    - src/main.rs
    - src/feed/pipeline.rs
    - src/settlement/monitor.rs
    - src/replay/mod.rs
    - src/persistence/recovery.rs
    - config/config.toml

key-decisions:
  - "Arc<RwLock<HashMap>> shared state between SettlementMonitor and PaperTradeTracker for checkpoint inclusion"
  - "Net P&L (fee-adjusted) as headline settlement_pnl per CONTEXT.md decision"
  - "Timeout positions evicted immediately from recently_settled per CONTEXT.md"
  - "Rate limiters created for Polymarket/Kalshi in pipeline at 5 req/s default for settlement REST calls"
  - "Settlement channel kept open when monitor disabled via _settlement_tx_hold variable"
  - "CheckpointState version bumped to 2 with backward-compatible serde(default) on settlement_tracking"

patterns-established:
  - "Shared state via Arc<RwLock> for cross-task checkpoint persistence: monitor writes, tracker reads during checkpoint snapshot"
  - "Per-leg independent settlement: each venue SettlementOutcome settles one leg, position finalizes when all legs present"
  - "Settlement-disabled graceful degradation: channel sender held alive, tracker select! arm never fires"

requirements-completed: [STTL-05, STTL-06]

# Metrics
duration: 17min
completed: 2026-02-26
---

# Phase 16 Plan 03: Paper Trade Integration Summary

**Per-leg settlement with fee-adjusted P&L, SettlementMonitor wired into main.rs runtime pipeline, checkpoint v2 with settlement tracking, and Prometheus metrics at settlement time**

## Performance

- **Duration:** 17 min
- **Started:** 2026-02-25T23:02:43Z
- **Completed:** 2026-02-25T23:20:12Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- Full settlement integration into PaperTradeTracker: new settlement_rx channel arm in select! loop, handle_settlement with per-leg P&L computation, SettlementLogger for daily-rotating JSONL, recently_settled VecDeque with bounded eviction
- Position lifecycle extended with PartiallySettled status, record_settled_leg/finalize_settlement methods, and cross-venue divergence detection (BinaryDisagree, AmbiguousResolution, TimingGap)
- CheckpointState v2 with settlement_tracking HashMap preserving per-event polling tier and last-check timestamps across restarts via shared Arc<RwLock> between monitor and tracker
- SettlementMonitor spawned in main.rs with shared VenueRateLimiters from PipelineHandles, checkpoint restore, registry init, and backfill
- Prometheus metrics: paper_trades_settled_total (venue/outcome), paper_trade_net_pnl histogram, paper_trade_settlement_latency_seconds, paper_trade_divergence_total
- 14 new tests across position (7), tracker (4), and checkpoint (3) modules

## Task Commits

Each task was committed atomically:

1. **Task 1: Integrate settlement into PaperTradeTracker and position lifecycle** - `dfbe246` (feat)
2. **Task 2: Extend checkpoint, add config, and wire SettlementMonitor into main.rs** - `9c7a8c6` (feat)

## Files Created/Modified
- `src/paper_trade/position.rs` - Added PartiallySettled status, settled_legs/divergence fields, record_settled_leg/finalize_settlement/compute_divergence methods
- `src/paper_trade/tracker.rs` - Added settlement_rx channel arm, handle_settlement, SettlementLogger, recently_settled management, shared settlement_tracking_state, Prometheus metrics
- `src/persistence/checkpoint.rs` - Bumped to v2, added SettlementTrackingEntry struct, settlement_tracking HashMap with serde(default)
- `src/persistence/recovery.rs` - Updated test for new checkpoint fields
- `src/config/system.rs` - Added SettlementConfig field to SystemConfig
- `src/main.rs` - Full SettlementMonitor wiring: channel creation, shared tracking state, checkpoint restore, registry init, backfill, tokio::spawn
- `src/feed/pipeline.rs` - Added venue_rate_limiters HashMap to PipelineHandles, create rate limiters for Polymarket/Kalshi
- `src/settlement/monitor.rs` - Added settlement_tracking_state field, update_shared_tracking_state, restore_tracking methods
- `src/replay/mod.rs` - Updated PipelineHandles constructors with venue_rate_limiters
- `config/config.toml` - Added commented [settlement] section with configurable parameters

## Decisions Made
- **Arc<RwLock> for shared state:** SettlementMonitor updates settlement tracking state after each poll_cycle; PaperTradeTracker reads it during checkpoint snapshot. This avoids direct coupling between the two tasks while ensuring checkpoint captures both position and settlement polling state atomically.
- **Net P&L as headline number:** Per CONTEXT.md locked decision, `settlement_pnl` on PaperPosition stores fee-adjusted net P&L (not raw). Raw P&L available via settled_legs drill-down.
- **Timeout immediate eviction:** Timeout positions skip the recently_settled VecDeque per CONTEXT.md -- operator investigates via JSONL logs.
- **Rate limiters for non-feed venues:** Polymarket and Kalshi feeds use WebSocket (not rate-limited), but settlement checkers need REST rate limiting. Created VenueRateLimiter at 5 req/s default in pipeline, shared via PipelineHandles.
- **Settlement-disabled graceful handling:** When settlement monitoring is disabled, the channel sender is kept alive via `_settlement_tx_hold` so the tracker's settlement_rx arm never receives a channel-closed event.
- **CheckpointState Clone derive:** Added Clone to CheckpointState so main.rs can preserve the full checkpoint for SettlementMonitor restore while also passing it to PaperTradeTracker.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Pulled forward checkpoint v2 changes into Task 1**
- **Found during:** Task 1 (compilation)
- **Issue:** Task 1 imports SettlementTrackingEntry and adds settlement_tracking to CheckpointState, but these were planned for Task 2. Without them, Task 1 cannot compile.
- **Fix:** Added SettlementTrackingEntry struct and settlement_tracking field to checkpoint.rs in Task 1 (minimal version), then expanded tests in Task 2.
- **Files modified:** src/persistence/checkpoint.rs, src/persistence/recovery.rs
- **Verification:** `cargo check` succeeds, all tests pass
- **Committed in:** dfbe246 (Task 1 commit)

**2. [Rule 3 - Blocking] Updated main.rs PaperTradeTracker::new signature in Task 1**
- **Found during:** Task 1 (compilation)
- **Issue:** Adding settlement_log_dir parameter to PaperTradeTracker::new breaks main.rs compilation. Also needed placeholder settlement_rx channel to match new run() signature.
- **Fix:** Updated main.rs with minimal settlement_log_dir and placeholder channel in Task 1, replaced with full wiring in Task 2.
- **Files modified:** src/main.rs
- **Verification:** `cargo check` succeeds for both binary and library
- **Committed in:** dfbe246 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both were compilation dependency issues between Task 1 and Task 2. No scope creep -- same work was done, just distributed across tasks slightly differently than planned.

## Issues Encountered
None beyond the auto-fixed blocking issues.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Settlement outcome tracking is fully operational end-to-end: SettlementMonitor detects resolutions, sends SettlementOutcome on channel, PaperTradeTracker settles positions with per-leg P&L, logs to JSONL, persists state in checkpoint
- Phase 17 (Signal Quality Analysis) has all the data it needs: SettlementRecord JSONL contains complete per-leg P&L, divergence annotations, fee model versions, and timing data
- 491 total tests pass (14 new + 477 existing), no regressions

## Self-Check: PASSED

- FOUND: 16-03-SUMMARY.md
- FOUND: commit dfbe246 (Task 1)
- FOUND: commit 9c7a8c6 (Task 2)
- FOUND: All 10 modified files verified on disk

---
*Phase: 16-settlement-outcome-tracking*
*Completed: 2026-02-26*
