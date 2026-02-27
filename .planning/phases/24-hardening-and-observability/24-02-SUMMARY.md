---
phase: 24-hardening-and-observability
plan: 02
subsystem: subscription
tags: [mpsc, cleanup, stale-state, select-branch, hashmap-retain, event-driven]

# Dependency graph
requires:
  - phase: 24-hardening-and-observability
    plan: 01
    provides: "CleanupEvent struct, cleanup_txs mpsc sender infrastructure in SubscriptionManager"
  - phase: 22-subscription-manager-core
    provides: "SubscriptionManager with reconcile(), watch channel push, per-venue HashSet diff"
  - phase: 23-dynamic-supervisor-subscriptions
    provides: "Supervisor watch::Receiver wiring, pipeline threading"
provides:
  - "SpreadEngine cleanup_rx select branch evicting stale latest/stats entries"
  - "CrossAssetEngine cleanup_rx select branch evicting stale latest_prob/latest_pred/stats entries"
  - "PricingEngine cleanup_rx select branch evicting stale iv_cache entries (smiles/smile_points intact)"
  - "DeribitProcessor cleanup_rx select branch evicting stale books/tickers entries"
  - "KalshiProcessor cleanup_rx select branch evicting stale books/last_exchange_ts entries"
  - "Full cleanup channel wiring: pipeline.rs creates channels, main.rs distributes receivers, SubscriptionManager holds senders"
affects: [future-memory-profiling, future-graceful-degradation]

# Tech tracking
tech-stack:
  added: []
  patterns: [registry-retain-for-event-cleanup, instrument-id-retain-for-processor-cleanup, dummy-channel-for-mock-replay]

key-files:
  created: []
  modified:
    - src/spread/engine.rs
    - src/signal/engine.rs
    - src/pricing/engine.rs
    - src/feed/deribit/normalize.rs
    - src/feed/kalshi/normalize.rs
    - src/feed/pipeline.rs
    - src/replay/mod.rs
    - src/main.rs

key-decisions:
  - "SpreadEngine/CrossAssetEngine use registry active_approved() for cleanup (event_id-keyed entries retained by active set)"
  - "PricingEngine uses cleanup.deribit_instruments directly (no registry needed, instrument-keyed cache)"
  - "DeribitProcessor/KalshiProcessor use cleanup.deribit_instruments/kalshi_tickers directly for books/tickers eviction"
  - "smiles/smile_points NOT cleaned in PricingEngine (Research Pitfall 5: multiple instruments share expiry)"
  - "Mock/Replay modes get dummy cleanup channels with immediately-dropped senders (receiver returns None gracefully)"
  - "Cleanup channels capacity 8 (infrequent events, sufficient for burst)"
  - "engine_cleanup_rxs returned via PipelineHandles tuple for engines spawned in main.rs"

patterns-established:
  - "Registry-retain pattern: read active_approved() into HashSet, retain HashMap entries matching active set"
  - "Instrument-retain pattern: collect instrument IDs from CleanupEvent into HashSet, retain entries not in removal set"
  - "Dummy-channel pattern: create mpsc channel, drop sender immediately for Mock/Replay modes"

requirements-completed: [SUB-05]

# Metrics
duration: 11min
completed: 2026-02-27
---

# Phase 24 Plan 02: Stale State Cleanup via Downstream Cleanup Channels Summary

**Event-driven cleanup channels wired from SubscriptionManager to all 5 stateful engines, evicting stale order books, snapshots, rolling stats, and IV cache entries after instrument unsubscribe**

## Performance

- **Duration:** 11 min
- **Started:** 2026-02-27T22:00:50Z
- **Completed:** 2026-02-27T22:12:15Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- All 5 stateful engines (SpreadEngine, CrossAssetEngine, PricingEngine, DeribitProcessor, KalshiProcessor) now have cleanup_rx mpsc receivers with select! branches that evict stale entries
- SpreadEngine/CrossAssetEngine use registry-retain pattern: read active_approved() event_ids, retain only matching entries in latest/stats/latest_prob/latest_pred HashMaps
- PricingEngine evicts iv_cache entries for removed Deribit instruments while leaving smiles/smile_points intact (multiple instruments share expiry dates)
- DeribitProcessor evicts books/tickers for removed instruments; KalshiProcessor evicts books/last_exchange_ts for removed tickers
- Full channel wiring: pipeline.rs creates 5 bounded(8) channels, passes processor receivers directly, returns engine receivers via PipelineHandles, main.rs passes to engines, cleanup_txs go to SubscriptionManager
- All 548 unit + 22 integration + 3 doc tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add cleanup_rx select branch to SpreadEngine, CrossAssetEngine, PricingEngine** - `dac2405` (feat)
2. **Task 2: Add cleanup to DeribitProcessor and KalshiProcessor, wire channels in pipeline.rs and main.rs** - `5f6ac9d` (feat)

## Files Created/Modified
- `src/spread/engine.rs` - Added CleanupEvent import, cleanup_rx parameter to run(), select branch retaining active event_id entries
- `src/signal/engine.rs` - Added CleanupEvent import, cleanup_rx parameter to run(), select branch retaining active event_id entries in latest_prob/latest_pred/stats
- `src/pricing/engine.rs` - Added CleanupEvent import, cleanup_rx parameter to run(), select branch evicting iv_cache entries by deribit_instruments
- `src/feed/deribit/normalize.rs` - Added CleanupEvent import, cleanup_rx field on struct, constructor parameter, select branch evicting books/tickers
- `src/feed/kalshi/normalize.rs` - Added CleanupEvent import, cleanup_rx field on struct, constructor parameter, select branch evicting books/last_exchange_ts
- `src/feed/pipeline.rs` - Added CleanupEvent import, PipelineHandles cleanup_txs/engine_cleanup_rxs fields, channel creation in run_live_multi_venue
- `src/replay/mod.rs` - Added CleanupEvent import, dummy cleanup channels for replay processors, engine_cleanup_rxs: None
- `src/main.rs` - Extracts cleanup_txs and engine_cleanup_rxs from PipelineHandles, passes to engines and SubscriptionManager

## Decisions Made
- SpreadEngine/CrossAssetEngine use registry active_approved() for cleanup rather than event_ids from CleanupEvent (which are empty per Plan 01) -- the registry-retain approach is simpler and authoritative
- PricingEngine uses deribit_instruments from CleanupEvent directly since iv_cache is keyed by InstrumentId, not event_id -- no registry needed
- smiles and smile_points are NOT cleaned up per Research Pitfall 5: multiple instruments share an expiry date and smiles expire naturally
- Mock/Replay modes get dummy cleanup channels with immediately-dropped senders; the select branch's `Some(cleanup) = self.cleanup_rx.recv()` pattern handles None (sender dropped) gracefully by never matching
- Cleanup channel capacity is 8: cleanup events are infrequent (config reload only) and bounded prevents unbounded memory growth
- engine_cleanup_rxs returned as Option tuple via PipelineHandles since engines are spawned in main.rs, not in pipeline.rs

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 24 (Hardening & Observability) is now complete: subscription metrics, dry-run mode, and stale state cleanup are all wired
- After instrument unsubscribe, no stale order books, snapshots, rolling stats, or IV cache entries persist in any downstream engine
- All cleanup is event-driven via mpsc channels (not periodic polling), ensuring immediate state eviction on unsubscribe
- Phase 25 (Tech Debt Sweep) can proceed with a clean bisectable codebase

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 24-hardening-and-observability*
*Completed: 2026-02-27*
