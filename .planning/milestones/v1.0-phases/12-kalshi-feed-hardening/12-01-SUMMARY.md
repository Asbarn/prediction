---
phase: 12-kalshi-feed-hardening
plan: 01
subsystem: feed
tags: [kalshi, websocket, heartbeat, exchange-timestamp, latency-metrics, dead-connection]

# Dependency graph
requires:
  - phase: 04-multi-venue-feeds
    provides: "Kalshi WebSocket client, message parsing, and normalization processor"
  - phase: 03-feed-infrastructure
    provides: "Heartbeat timeout pattern (Deribit client), metrics facade"
provides:
  - "Kalshi dead-connection detection via heartbeat timeout (30s default)"
  - "Nested {type, msg: {...}} message envelope parsing for live API"
  - "Exchange timestamp propagation from orderbook_delta ts field"
  - "feed_latency_ms histogram and feed_last_latency_ms gauge for venue=kalshi"
affects: [kalshi-feed, reliability, metrics, monitoring]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Dead-connection timeout via tokio::time::sleep_until in biased select (same as Deribit)"
    - "Nested/flat message format detection via value.get(\"msg\").filter(|v| v.is_object())"
    - "Best-effort exchange timestamp from per-market HashMap tracking"

key-files:
  created: []
  modified:
    - "src/config/venues.rs"
    - "src/feed/kalshi/client.rs"
    - "src/feed/kalshi/messages.rs"
    - "src/feed/kalshi/normalize.rs"
    - "tests/pipeline_test.rs"

key-decisions:
  - "Heartbeat timeout 30s default (3x Kalshi 10s Ping interval) matches Deribit 2x pattern philosophy"
  - "Nested format detected by checking if msg field is an object (distinguishes from SubscribedData string msg)"
  - "Exchange timestamp tracked per-market in HashMap, only from delta messages (snapshots lack ts)"
  - "Latency metrics use same pattern as Deribit/Polymarket: histogram + gauge with venue label"
  - "Protocol limitation documented: second-precision ts means up to 999ms jitter in latency values"

patterns-established:
  - "All three venues now have dead-connection detection with consistent metrics (feed_heartbeat_timeouts)"
  - "All three venues now emit feed_latency_ms and feed_last_latency_ms with venue label"

requirements-completed: [RELY-02, FEED-08, TIME-02, TIME-03]

# Metrics
duration: 9min
completed: 2026-02-24
---

# Phase 12 Plan 01: Kalshi Feed Hardening Summary

**Dead-connection detection via 30s heartbeat timeout, nested message parsing with ts field, exchange timestamp propagation to MarketSnapshot, and feed_latency_ms/feed_last_latency_ms metrics for Kalshi venue**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-24T14:22:10Z
- **Completed:** 2026-02-24T14:31:03Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Kalshi supervisor now detects dead connections within 30s and reconnects (closes RELY-02 gap)
- OrderbookDeltaData.ts parsed from both nested and flat message formats (backward compatible)
- MarketSnapshot.exchange_timestamp populated from last delta's ts (closes FEED-08, TIME-02)
- feed_latency_ms histogram and feed_last_latency_ms gauge emitted for venue=kalshi (closes TIME-03)
- All three venues now have consistent heartbeat timeout, exchange timestamp, and latency metrics

## Task Commits

Each task was committed atomically:

1. **Task 1: Add heartbeat timeout and nested message parsing** - `87ebaf6` (feat)
2. **Task 2: Propagate exchange timestamps and emit latency metrics** - `8da73a6` (feat)

## Files Created/Modified
- `src/config/venues.rs` - Added heartbeat_timeout_ms field (default 30s) to KalshiConfig
- `src/feed/kalshi/client.rs` - 3-branch tokio::select! with cancel, sleep_until timeout, and message handling
- `src/feed/kalshi/messages.rs` - ts: Option<String> on OrderbookDeltaData, nested envelope support in parse()
- `src/feed/kalshi/normalize.rs` - last_exchange_ts tracking, chrono parsing, latency metrics, exchange_timestamp on snapshot
- `tests/pipeline_test.rs` - Added heartbeat_timeout_ms to KalshiConfig constructors in integration tests

## Decisions Made
- Heartbeat timeout defaults to 30s (3x Kalshi 10s Ping), configurable via TOML heartbeat_timeout_ms
- Nested format detected by `value.get("msg").filter(|v| v.is_object())` to distinguish from SubscribedData's string msg field
- Exchange timestamp tracked per-market, only from deltas (snapshots after connect may have None until first delta)
- Latency metrics match Deribit/Polymarket pattern exactly: histogram + gauge with venue label, counter always emitted
- Second-precision protocol limitation documented in code comments

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed incorrect expected timestamp in test**
- **Found during:** Task 2 (exchange timestamp propagation tests)
- **Issue:** Plan specified expected millis 1705312200000 for "2024-01-15T10:30:00Z" but correct value is 1705314600000
- **Fix:** Updated test assertion to use correct epoch milliseconds
- **Files modified:** src/feed/kalshi/normalize.rs
- **Verification:** Test passes with correct value
- **Committed in:** 8da73a6 (Task 2 commit)

**2. [Rule 3 - Blocking] Fixed missing heartbeat_timeout_ms in integration test**
- **Found during:** Task 2 (full crate test suite)
- **Issue:** tests/pipeline_test.rs constructs KalshiConfig without new heartbeat_timeout_ms field
- **Fix:** Added heartbeat_timeout_ms: 30_000 to both KalshiConfig constructors in pipeline_test.rs
- **Files modified:** tests/pipeline_test.rs
- **Verification:** cargo test -p prediction passes (417 tests, 0 failures)
- **Committed in:** 8da73a6 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All four v1.0 audit gaps for Kalshi are now closed (RELY-02, FEED-08, TIME-02, TIME-03)
- All three venues have consistent heartbeat timeout, exchange timestamp, and latency metrics
- Ready for any remaining kalshi-feed-hardening plans or milestone verification

---
*Phase: 12-kalshi-feed-hardening*
*Completed: 2026-02-24*
