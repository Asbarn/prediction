---
phase: 32-pipeline-wiring-and-observability
plan: 02
subsystem: feed
tags: [derive, pipeline, websocket, prometheus, metrics, reconnection]

# Dependency graph
requires:
  - phase: 32-01
    provides: SubscriptionManager Derive venue support and SubscriptionReceivers.derive field
  - phase: 31-04
    provides: DeriveProcessor and DeriveBook normalization pipeline
  - phase: 31-03
    provides: DeriveSupervisor and DeriveClient WebSocket stack
provides:
  - Derive pipeline block in run_live_multi_venue (4th venue fully wired)
  - feed_reconnections_total Prometheus counter for all venues
  - Derive venue availability logging at startup
affects: [33-final-integration-test, observability, monitoring]

# Tech tracking
tech-stack:
  added: []
  patterns: [venue-pipeline-block-pattern, reconnection-counter-metric]

key-files:
  created: []
  modified:
    - src/feed/pipeline.rs
    - src/main.rs
    - src/feed/health.rs

key-decisions:
  - "Derive pipeline follows identical Deribit block pattern (7-step: health, cancel, recording, rate-limiter, supervisor, processor, forward)"
  - "feed_reconnections_total is venue-generic (not Derive-specific) benefiting all 4 venues"

patterns-established:
  - "4-venue fan-in pipeline: all venues publish to shared snapshot_tx before drop(snapshot_tx)"

requirements-completed: [PIPE-04, PIPE-05]

# Metrics
duration: 5min
completed: 2026-03-05
---

# Phase 32 Plan 02: Pipeline Wiring and Derive Integration Summary

**Derive pipeline wired into run_live_multi_venue with DeriveSupervisor/DeriveProcessor spawn, fan-in forwarding, and feed_reconnections_total counter for all venues**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-05T22:53:16Z
- **Completed:** 2026-03-05T22:58:31Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Derive MarketSnapshots flow through shared fan-in channel to SpreadEngine, SignalEngine, PricingEngine, and PaperTradeTracker
- DeriveSupervisor receives dynamic instrument updates via watch channel from SubscriptionManager
- feed_reconnections_total Prometheus counter emitted by all venues on every connection attempt
- Derive pipeline has crash isolation via child CancellationToken
- Derive cleanup channel wired to SubscriptionManager for state eviction

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Derive pipeline block to run_live_multi_venue and update main.rs** - `a43c5a2` (feat)
2. **Task 2: Add feed_reconnections_total counter metric in VenueHealth** - `6553625` (feat)

## Files Created/Modified
- `src/feed/pipeline.rs` - Added Derive pipeline block (supervisor, processor, recording, forward_snapshots), derive_cleanup channel, updated imports and doc diagram
- `src/main.rs` - Added Derive venue availability log at startup
- `src/feed/health.rs` - Added feed_reconnections_total counter in increment_connections()

## Decisions Made
- Derive pipeline follows identical Deribit 7-step block pattern for consistency
- feed_reconnections_total is venue-generic (uses venue label), not Derive-specific -- benefits all 4 venues
- No auth guard needed for Derive (unlike Kalshi) -- pipeline always starts

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 4 venues (Deribit, Polymarket, Kalshi, Derive) are now fully wired in the live pipeline
- Phase 32 complete -- ready for Phase 33 final integration testing
- Prometheus metrics available: feed_available, feed_latency_ms, feed_messages_total, subscription_active, subscription_activations_total, subscription_removals_total, feed_reconnections_total

---
*Phase: 32-pipeline-wiring-and-observability*
*Completed: 2026-03-05*
