---
phase: 02-deribit-feed
plan: 04
subsystem: feed
tags: [pipeline, mock, replay, synthetic, cli, mpsc, deribit, integration, clap]

# Dependency graph
requires:
  - phase: 02-deribit-feed
    plan: 01
    provides: "RawDataSource trait, RawMessage, RecordLine, DeribitClient, DeribitMessage types, channel routing"
  - phase: 02-deribit-feed
    plan: 02
    provides: "DeribitProcessor, InstrumentBook, build_snapshot, TickerState, MarketSnapshot normalization"
  - phase: 02-deribit-feed
    plan: 03
    provides: "RecordingService, JsonlWriter, Recorder trait, bounded-channel recording"
provides:
  - "ReplayDataSource: reads JSONL recordings at configurable speed (0/1x/10x)"
  - "SyntheticDataSource: generates realistic Deribit-format JSON-RPC messages for offline dev"
  - "run_pipeline: assembles data source -> processor -> recorder -> downstream channel"
  - "DataMode enum: Live, Replay, Mock for pipeline mode selection"
  - "CLI integration: --mock, --replay, --speed flags via clap"
  - "Complete Phase 2 deliverable: end-to-end data pipeline runnable via cargo run"
affects: [03-feed-reliability, 04-multi-venue, 06-spread-calculator]

# Tech tracking
tech-stack:
  added: []
  patterns: [data-source-polymorphism-via-trait, pipeline-assembly-function, cli-mode-dispatch]

key-files:
  created:
    - src/feed/mock/mod.rs
    - src/feed/mock/replay.rs
    - src/feed/mock/synthetic.rs
    - src/feed/pipeline.rs
    - tests/pipeline_test.rs
  modified:
    - src/feed/mod.rs
    - src/feed/traits.rs
    - src/main.rs

key-decisions:
  - "StdRng::from_entropy instead of thread_rng -- ThreadRng is not Send, cannot be used across await points in tokio::spawn"
  - "Replay reads entire JSONL file into memory upfront -- simpler than streaming, adequate for development recordings"
  - "Pipeline function takes DeribitConfig directly instead of full VenuesConfig -- narrower interface, clearer dependency"
  - "Added Deserialize derive to RecordLine -- required for replay source to parse JSONL recordings"

patterns-established:
  - "Pipeline assembly: single run_pipeline function wires all components based on DataMode enum"
  - "CLI mode dispatch: --mock/--replay/--speed flags determine DataMode before pipeline start"
  - "Integration testing: spawn mock pipeline, verify MarketSnapshots arrive, verify graceful shutdown"

# Metrics
duration: 8min
completed: 2026-02-22
---

# Phase 02 Plan 04: Mock Data Layer, Pipeline Assembly, and CLI Integration Summary

**ReplayDataSource and SyntheticDataSource for offline development, run_pipeline assembly function wiring all components, and CLI with --mock/--replay/--speed flags for three operational modes**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-22T12:40:18Z
- **Completed:** 2026-02-22T12:48:19Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- ReplayDataSource reads JSONL recordings at configurable speed with inter-message timing preservation
- SyntheticDataSource generates valid Deribit-format JSON-RPC book, ticker, and trade notifications
- Pipeline assembly function wires data source -> processor -> recorder -> downstream in a single call
- CLI supports `cargo run -- --mock`, `cargo run -- --replay FILE --speed 0`, and `cargo run` (live) modes
- 7 new tests (4 unit + 3 integration) covering replay, synthetic generation, pipeline end-to-end, and shutdown
- Complete Phase 2 deliverable: 99 tests pass, zero warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Mock data sources -- replay and synthetic** - `1610f63` (feat)
2. **Task 2: Pipeline assembly and main.rs integration** - `e2a0284` (feat)

## Files Created/Modified
- `src/feed/mock/mod.rs` - Module declarations and re-exports for ReplayDataSource and SyntheticDataSource
- `src/feed/mock/replay.rs` - ReplayDataSource reading JSONL files at configurable speed, 2 unit tests
- `src/feed/mock/synthetic.rs` - SyntheticDataSource generating Deribit-format book/ticker/trade messages, 2 unit tests
- `src/feed/pipeline.rs` - run_pipeline assembly function, DataMode enum (Live/Replay/Mock)
- `src/feed/mod.rs` - Added mock and pipeline module declarations
- `src/feed/traits.rs` - Added Deserialize derive to RecordLine for replay parsing
- `src/main.rs` - CLI flags (--mock, --replay, --speed), pipeline launch, snapshot consumer logging
- `tests/pipeline_test.rs` - 3 integration tests: mock snapshots, graceful shutdown, replay round-trip

## Decisions Made
- Used `StdRng::from_entropy()` instead of `rand::thread_rng()` in SyntheticDataSource because `ThreadRng` is not `Send` and cannot be held across `.await` points in `tokio::spawn`
- ReplayDataSource reads the entire JSONL file into memory before replaying -- simpler implementation, adequate for development-sized recordings (streaming can be added in Phase 3 if needed)
- Pipeline function takes `&DeribitConfig` directly rather than `&VenuesConfig` -- narrower interface makes the dependency explicit and simplifies testing
- Added `Deserialize` to `RecordLine` struct -- was only `Serialize` before, but replay source needs to parse JSONL files back into RecordLine structs

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added Deserialize derive to RecordLine**
- **Found during:** Task 1 (ReplayDataSource implementation)
- **Issue:** RecordLine only derived Serialize; ReplayDataSource needs to deserialize JSONL lines back into RecordLine structs
- **Fix:** Added `serde::Deserialize` to the derive list on RecordLine
- **Files modified:** src/feed/traits.rs
- **Verification:** Replay test reads JSONL and produces RawMessages successfully
- **Committed in:** 1610f63 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed ThreadRng not Send in SyntheticDataSource**
- **Found during:** Task 1 (SyntheticDataSource implementation)
- **Issue:** `rand::thread_rng()` returns `ThreadRng` which is not `Send`, causing compilation error when used across await points in `tokio::spawn`
- **Fix:** Replaced with `StdRng::from_entropy()` which is `Send`
- **Files modified:** src/feed/mock/synthetic.rs
- **Verification:** Build succeeds, synthetic tests pass
- **Committed in:** 1610f63 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both auto-fixes necessary for compilation correctness. No scope creep.

## Issues Encountered
- ThreadRng is not Send in rand 0.8 when used with tokio::spawn -- resolved by switching to StdRng::from_entropy()

## User Setup Required
None - no external service configuration required. All three modes work without any external service setup.

## Next Phase Readiness
- Phase 2 is fully complete: all 4 plans executed, end-to-end pipeline operational
- Live mode requires internet connection to Deribit testnet (wss://test.deribit.com)
- Mock and replay modes work entirely offline
- Phase 3 (feed reliability) can build on this foundation: reconnection, heartbeat, and circuit-breaking
- The pipeline assembly function provides a clean integration point for Phase 4 (multi-venue)
- MarketSnapshot receiver is ready for Phase 6 (spread calculator) consumption

## Self-Check: PASSED

- All 5 created files verified present on disk
- All 3 modified files verified
- Commit 1610f63 (Task 1) verified in git log
- Commit e2a0284 (Task 2) verified in git log
- 99 tests pass (55 lib + 16 integration + 3 pipeline + 22 smoke + 3 doctests)
- Zero compiler warnings

---
*Phase: 02-deribit-feed*
*Completed: 2026-02-22*
