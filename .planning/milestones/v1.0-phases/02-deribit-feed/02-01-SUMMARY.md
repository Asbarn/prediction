---
phase: 02-deribit-feed
plan: 01
subsystem: feed
tags: [websocket, deribit, json-rpc, serde, tokio-tungstenite, mpsc]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: "DualTimestamp, MarketSnapshot, DeribitConfig, Venue, CancellationToken, tracing"
provides:
  - "RawDataSource, NormalizedDataSource, Recorder trait definitions"
  - "All Deribit JSON-RPC serde structs (DeribitMessage, BookData, TickerData, TradeData, PriceIndexData)"
  - "ChannelKind routing and subscription channel construction"
  - "DeribitClient WebSocket connection with batch subscribe and raw frame forwarding"
affects: [02-deribit-feed, 03-feed-reliability, 04-multi-venue]

# Tech tracking
tech-stack:
  added: [tokio-tungstenite 0.28 (native-tls), futures-util 0.3, rand 0.8]
  patterns: [trait-based data source abstraction, mpsc channel pipeline, JSON-RPC 2.0 message routing]

key-files:
  created:
    - src/feed/mod.rs
    - src/feed/traits.rs
    - src/feed/deribit/mod.rs
    - src/feed/deribit/messages.rs
    - src/feed/deribit/channels.rs
    - src/feed/deribit/client.rs
  modified:
    - Cargo.toml
    - src/lib.rs
    - src/config/venues.rs
    - config/venues.toml
    - tests/smoke_test.rs

key-decisions:
  - "RawDataSource returns mpsc::Receiver<RawMessage> from start() -- avoids RPITIT lifetime complexity"
  - "f64 at serde boundary -- Decimal conversion deferred to normalization layer (Plan 02)"
  - "BookData bids/asks as Vec<[f64; 2]> -- matches grouped channel snapshot format"
  - "Testnet URL in venues.toml default config for safe development"
  - "Atomic counter for JSON-RPC request IDs -- simple, thread-safe"

patterns-established:
  - "Trait-based data source: RawDataSource::start() -> Receiver pattern for all venue feeds"
  - "Channel-per-concern: WS reader task sends through mpsc, consumer tasks receive"
  - "Untagged serde enum for JSON-RPC messages: Response vs Notification disambiguation"

# Metrics
duration: 9min
completed: 2026-02-22
---

# Phase 02 Plan 01: Feed Traits and Deribit WS Client Summary

**Feed module with trait abstractions (RawDataSource, NormalizedDataSource, Recorder), all Deribit JSON-RPC serde structs, channel routing, and a DeribitClient that connects, subscribes, and forwards raw WS frames via mpsc**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-22T12:15:39Z
- **Completed:** 2026-02-22T12:24:52Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Feed trait abstractions (RawDataSource, NormalizedDataSource, Recorder) establish the data source abstraction boundary for live and mock sources
- All 4 Deribit channel data types (BookData, TickerData, TradeData, PriceIndexData) deserialize from realistic JSON with 25 unit tests
- Channel routing (ChannelKind::parse) and subscription construction (build_subscription_channels) with instrument extraction
- DeribitClient connects to Deribit WSS, sends batch subscription, and reads raw frames through mpsc(1024)

## Task Commits

Each task was committed atomically:

1. **Task 1: Feed traits, Deribit message types, and channel routing** - `02de861` (feat)
2. **Task 2: Deribit WebSocket client with connection and subscription** - `fed1b2c` (feat)

## Files Created/Modified
- `src/feed/mod.rs` - Feed module root: declares traits and deribit submodules
- `src/feed/traits.rs` - RawDataSource, NormalizedDataSource, Recorder traits + RawMessage, RecordLine types
- `src/feed/deribit/mod.rs` - Deribit submodule root: declares messages, channels, client
- `src/feed/deribit/messages.rs` - All Deribit JSON-RPC serde structs with 12 unit tests
- `src/feed/deribit/channels.rs` - ChannelKind enum, parse/extract/build functions with 13 unit tests
- `src/feed/deribit/client.rs` - DeribitClient with connect_async, batch subscribe, WS read loop
- `Cargo.toml` - Added tokio-tungstenite, futures-util, rand dependencies
- `src/lib.rs` - Added `pub mod feed`
- `src/config/venues.rs` - Added instruments field to DeribitConfig
- `config/venues.toml` - Switched to testnet URL, added sample instruments
- `tests/smoke_test.rs` - Updated expected URL to match testnet

## Decisions Made
- Used `mpsc::Receiver<RawMessage>` return from `start()` instead of `impl Stream` to avoid RPITIT lifetime complexity
- Kept all price fields as `f64` at the serde boundary -- Decimal conversion will happen in the normalization layer (Plan 02) with explicit rounding
- BookData uses `Vec<[f64; 2]>` for bids/asks matching the grouped channel snapshot format (not the raw delta `[action, price, amount]` format)
- Switched `venues.toml` to Deribit testnet URL for safe development
- Used `Message::text()` helper method for tungstenite 0.28's Utf8Bytes API

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Updated smoke test for testnet URL change**
- **Found during:** Task 2 (DeribitConfig instruments field)
- **Issue:** Changing venues.toml to testnet URL broke existing smoke test that asserted production URL
- **Fix:** Updated test assertion to match new testnet URL
- **Files modified:** tests/smoke_test.rs
- **Verification:** All 65 tests pass
- **Committed in:** fed1b2c (Task 2 commit)

**2. [Rule 3 - Blocking] Fixed tungstenite 0.28 Utf8Bytes API changes**
- **Found during:** Task 2 (DeribitClient implementation)
- **Issue:** tungstenite 0.28 uses `Utf8Bytes` not `String` for `Message::Text`; also `connect_async` needed `&str` reference for type inference
- **Fix:** Used `Message::text()` helper, `.to_string()` on received text, `&ws_url` for connect_async
- **Files modified:** src/feed/deribit/client.rs
- **Verification:** Clean build, no warnings
- **Committed in:** fed1b2c (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both auto-fixes necessary for correctness with the actual library versions. No scope creep.

## Issues Encountered
- tungstenite 0.28 has a different API from what the research examples showed (Utf8Bytes instead of String) -- resolved by using the `Message::text()` convenience constructor and `.to_string()` on received frames
- Private module access: `config::venues` is private, needed to use re-exported `config::DeribitConfig` path

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Feed traits and Deribit message types ready for Plan 02 (order book state + normalization)
- Channel routing ready for Plan 02's message dispatch to typed handlers
- DeribitClient ready for integration testing against testnet
- No Phase 3 concerns (reconnection, heartbeat, staleness) implemented -- clean boundary maintained

## Self-Check: PASSED

- All 6 created files verified present on disk
- Commit 02de861 (Task 1) verified in git log
- Commit fed1b2c (Task 2) verified in git log
- 65 tests pass (25 lib + 16 integration + 22 smoke + 2 doctests)
- Zero compiler warnings

---
*Phase: 02-deribit-feed*
*Completed: 2026-02-22*
