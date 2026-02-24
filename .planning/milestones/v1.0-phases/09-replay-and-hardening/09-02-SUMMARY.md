---
phase: 09-replay-and-hardening
plan: 02
subsystem: replay
tags: [replay, jsonl, multi-venue, staleness-bypass, deterministic, backtesting]

# Dependency graph
requires:
  - phase: 02-deribit-feed
    provides: ReplayDataSource and RecordLine types
  - phase: 04-multi-venue-feeds
    provides: Multi-venue processor pipeline (DeribitProcessor, PolymarketProcessor, KalshiProcessor)
  - phase: 06-prediction-market-spreads
    provides: SpreadEngine with staleness gates
  - phase: 08-cross-asset-signal-generation
    provides: CrossAssetEngine with staleness gates
  - phase: 09-replay-and-hardening
    plan: 01
    provides: PipelineHandles struct, VenueHealth, JSONL schema stabilization
provides:
  - ReplayCorpus loading multi-venue JSONL recordings sorted by local_ts
  - run_replay_pipeline orchestrating per-venue processors through shared fan-in
  - ReplayDataSource::from_records() constructor for direct Vec<RecordLine> input
  - Recorded local_ts used for DualTimestamp wall clock (not Utc::now())
  - replay_mode staleness bypass on SpreadEngine and CrossAssetEngine
  - CLI --replay accepts recordings directory path for multi-venue replay
  - forward_snapshots made pub for reuse from replay module
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [ReplaySource enum for file vs in-memory records, replay_mode builder pattern on engines, shared replay_records helper function]

key-files:
  created:
    - src/replay/mod.rs
  modified:
    - src/feed/mock/replay.rs
    - src/feed/pipeline.rs
    - src/spread/engine.rs
    - src/signal/engine.rs
    - src/lib.rs
    - src/main.rs
    - tests/pipeline_test.rs

key-decisions:
  - "ReplayDataSource uses ReplaySource enum internally (File vs Records) to avoid temp file overhead"
  - "Recorded local_ts used for DualTimestamp wall clock; mono set to Instant::now() (no meaningful replay value)"
  - "replay_mode bypasses ALL wall-clock staleness gates (simplest approach, recommended for v1 per research)"
  - "forward_snapshots made pub to allow reuse from src/replay/mod.rs"
  - "Replay match arm in run_multi_venue_pipeline routes to run_replay_pipeline (multi-venue, not single-file)"

patterns-established:
  - "replay_mode builder pattern: engine.with_replay_mode(true) for staleness bypass"
  - "ReplayCorpus: load_directory scans venue subdirectories, sorts entries across venues by local_ts"
  - "from_records constructor: bypass file I/O by feeding Vec<RecordLine> directly to ReplayDataSource"

requirements-completed: [TEST-02, TEST-03]

# Metrics
duration: 12min
completed: 2026-02-23
---

# Phase 9 Plan 2: Multi-venue Replay Pipeline with Staleness Bypass Summary

**Deterministic multi-venue replay from recorded JSONL feeds through the full pipeline with staleness bypass, recorded-timestamp DualTimestamp, and graceful degradation for missing venues**

## Performance

- **Duration:** 12 min
- **Started:** 2026-02-23T19:53:18Z
- **Completed:** 2026-02-23T20:05:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- ReplayCorpus loads JSONL from multiple venue subdirectories (deribit/, polymarket/, kalshi/) sorted by local_ts across all venues
- SpreadEngine and CrossAssetEngine skip wall-clock staleness gates when replay_mode=true, enabling historical data to flow through the full pipeline
- ReplayDataSource extended with from_records() constructor and uses recorded local_ts for DualTimestamp wall clock (not Utc::now())
- CLI --replay now accepts a recordings directory path for multi-venue replay through per-venue processors
- Missing venue recording directories degrade gracefully with a warning (no crash)
- 8 new tests: 3 corpus loading tests, 1 from_records test with timestamp verification, 2 integration tests (multi-venue replay + empty dir graceful handling)

## Task Commits

Each task was committed atomically:

1. **Task 1: Multi-venue replay corpus and pipeline with staleness bypass** - `2c5e19b` (feat)
2. **Task 2: CLI integration and main.rs replay pipeline wiring** - `d955ad0` (feat)

## Files Created/Modified
- `src/replay/mod.rs` - ReplayCorpus, run_replay_pipeline, 3 unit tests
- `src/feed/mock/replay.rs` - ReplaySource enum, from_records() constructor, replay_records() helper, recorded-timestamp DualTimestamp, 1 new test
- `src/feed/pipeline.rs` - Replay match arm routes to run_replay_pipeline, forward_snapshots made pub
- `src/spread/engine.rs` - replay_mode field, with_replay_mode() builder, staleness gate bypass
- `src/signal/engine.rs` - replay_mode field, with_replay_mode() builder, staleness gate bypass
- `src/lib.rs` - pub mod replay registration
- `src/main.rs` - is_replay flag, replay_mode wired to engines, updated CLI help text, replay logging
- `tests/pipeline_test.rs` - 2 new integration tests for multi-venue replay

## Decisions Made
- Used ReplaySource enum (File vs Records) to avoid temp file overhead when feeding grouped records to per-venue processors
- Recorded local_ts used for DualTimestamp wall clock (mono set to Instant::now() since it has no meaningful replay value per research pitfall #2)
- replay_mode bypasses ALL wall-clock staleness gates (approach 1 from research: simplest, recommended for v1)
- forward_snapshots made pub to allow clean reuse from the replay module (previously crate-private)
- DataMode::Replay in run_multi_venue_pipeline now routes to run_replay_pipeline instead of single-venue run_pipeline

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 9 phases complete: the prediction market arbitrage signal generator v1 is fully implemented
- Replay pipeline enables backtesting with historical recordings via `cargo run -- --replay recordings/ --speed 0`
- Feed recordings serve as a reusable replay corpus -- any historical period can be replayed
- Staleness bypass ensures historical data flows through spread and signal computation without rejection

## Self-Check: PASSED

All created files verified present. All task commits verified in git log.

---
*Phase: 09-replay-and-hardening*
*Completed: 2026-02-23*
