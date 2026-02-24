---
phase: 02-deribit-feed
verified: 2026-02-22T12:53:56Z
status: passed
score: 5/5 must-haves verified
re_verification: false
gaps: []
human_verification:
  - test: Run cargo run then check MarketSnapshot log output
    expected: JSON log lines with venue=deribit, instrument, bid/ask, seq numbers incrementing
    why_human: Requires running binary; automated tests cover same code path but terminal output is human-only
  - test: Run cargo run --mock then check recordings/deribit/
    expected: JSONL file with valid JSON lines containing raw, local_ts, venue, channel, instrument
    why_human: File output from a live run; tests verify write path but not the actual cargo run file
---

# Phase 02: Deribit Feed and Data Pipeline Verification Report

**Phase Goal:** The system connects to Deribit, maintains a live order book, publishes normalized MarketSnapshot events through a bounded async channel, records every raw message to JSONL, and supports a mock data source for testing.
**Verified:** 2026-02-22T12:53:56Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | System connects to Deribit WebSocket, subscribes via JSON-RPC 2.0, receives live market data | VERIFIED | DeribitClient::start() calls connect_async (line 67), sends public/subscribe with 4 channel types; RawDataSource trait implemented; wired in run_pipeline for DataMode::Live |
| 2 | Local order book applies incremental deltas correctly, producing accurate bid/ask/depth snapshots | VERIFIED | InstrumentBook::apply_snapshot verifies prev_change_id continuity, replaces full depth, sorts correctly; 9 unit tests pass confirming sequence gap detection, staleness marking, sort order |
| 3 | Every raw WebSocket message recorded to line-delimited JSON with local receive timestamp and venue | VERIFIED | RecordingService with 8192-message bounded channel and try_send; JsonlWriter writes {raw, local_ts, venue, channel, instrument} per line; daily rotation; round-trip test confirms fields |
| 4 | Normalized MarketSnapshot events flow through bounded async channel to downstream consumers | VERIFIED | DeribitProcessor publishes to mpsc::channel(256); build_snapshot assembles all required fields; snapshot_tx.send() in book and ticker handlers; pipeline integration test confirms 3+ snapshots received |
| 5 | Full pipeline runs identically against mock data source without live connection | VERIFIED | RawDataSource trait implemented by DeribitClient, ReplayDataSource, and SyntheticDataSource; DataMode enum in run_pipeline selects source; 3 integration tests pass without network |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/feed/traits.rs` | RawDataSource, NormalizedDataSource, Recorder traits | VERIFIED | All three traits defined; RawMessage and RecordLine structs present; RecordLine derives Serialize and Deserialize |
| `src/feed/deribit/messages.rs` | All Deribit JSON-RPC serde structs | VERIFIED | DeribitMessage, BookData, TickerData, TradeData, PriceIndexData with correct fields; 12 unit tests pass |
| `src/feed/deribit/channels.rs` | Channel name construction and routing | VERIFIED | ChannelKind enum with parse(), extract_instrument(), build_subscription_channels(); 13 unit tests pass |
| `src/feed/deribit/client.rs` | DeribitClient with connect, subscribe, WS read loop | VERIFIED | connect_async at line 67, batch subscribe, tokio::select! read loop, implements RawDataSource |
| `src/feed/deribit/book.rs` | InstrumentBook with change_id verification | VERIFIED | apply_snapshot verifies prev_change_id, replaces full state, sorts correctly; SequenceError::Gap returned; 9 unit tests pass |
| `src/feed/deribit/normalize.rs` | Normalization pipeline to MarketSnapshot | VERIFIED | DeribitProcessor routes 4 channel types; build_snapshot helper; snapshot_tx.send() in handlers; 10 unit tests pass |
| `src/types/snapshot.rs` | Expanded MarketSnapshot with all fields | VERIFIED | depth_bids/asks, bid/ask_probability, last/mark/index_price, mark_iv, open_interest, volume_24h, greeks, exchange_timestamp, sequence, is_stale all present |
| `src/feed/recording/mod.rs` | RecordingService with bounded channel | VERIFIED | 8192-message channel, recording_task spawned, Recorder trait implemented, drain-on-shutdown |
| `src/feed/recording/writer.rs` | JsonlWriter with async I/O and daily rotation | VERIFIED | BufWriter of tokio fs File; rotation to {base_dir}/{venue}/{date}.jsonl; append mode; 4 unit tests |
| `src/feed/mock/replay.rs` | ReplayDataSource reading JSONL at configurable speed | VERIFIED | Reads entire file, replays with timing scaled by speed; speed=0 is instant; implements RawDataSource; 2 unit tests |
| `src/feed/mock/synthetic.rs` | SyntheticDataSource generating realistic messages | VERIFIED | Generates book/ticker/trade notifications with correct change_id sequencing; implements RawDataSource; 2 unit tests |
| `src/feed/pipeline.rs` | Pipeline assembly function | VERIFIED | run_pipeline wires RecordingService, data source, DeribitProcessor; DataMode enum selects source |
| `src/main.rs` | CLI with --mock, --replay, --speed flags | VERIFIED | clap CLI with all flags; DataMode selected from flags; run_pipeline called; snapshot consumer task logs all snapshots |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| src/feed/deribit/client.rs | tokio-tungstenite connect_async | WSS connection | WIRED | Line 67: tokio_tungstenite::connect_async(&ws_url) |
| src/feed/deribit/client.rs | src/feed/deribit/messages.rs | serde_json::from_str | WIRED | Parsing in processor normalize.rs line 138; client forwards raw text frames as designed |
| src/feed/deribit/channels.rs | src/feed/deribit/messages.rs | ChannelKind routing | WIRED | normalize.rs lines 177/181/185/188: ChannelKind::parse then match dispatches to typed from_value |
| src/feed/deribit/normalize.rs | src/feed/deribit/book.rs | apply_snapshot | WIRED | Line 226: book.apply_snapshot(&book_data, received_at) |
| src/feed/deribit/normalize.rs | src/feed/deribit/messages.rs | typed deserialization | WIRED | Lines 206/270/337/362: serde_json::from_value(data) in each typed handler |
| src/feed/deribit/normalize.rs | tokio::sync::mpsc | snapshot_tx.send | WIRED | Lines 258/330: snapshot_tx.send(snapshot).await in book and ticker handlers |
| src/feed/recording/mod.rs | src/feed/recording/writer.rs | recording_task spawned | WIRED | Line 50: tokio::spawn(recording_task(...)); line 95: writer.write_line(&line).await |
| src/feed/recording/mod.rs | tokio::sync::mpsc | try_send for non-blocking | WIRED | Lines 47/68: mpsc::channel(8192) and self.tx.try_send(line) |
| src/feed/recording/writer.rs | tokio::fs | async BufWriter | WIRED | Line 19: Option of BufWriter of File; lines 49-53: write_all and flush |
| src/feed/pipeline.rs | src/feed/deribit/client.rs | live mode source | WIRED | Lines 15/57-62: use DeribitClient; DeribitClient::new(...).start().await |
| src/feed/pipeline.rs | src/feed/mock/replay.rs | replay mode source | WIRED | Lines 17/65-66: use ReplayDataSource; source.start().await |
| src/feed/pipeline.rs | src/feed/deribit/normalize.rs | processor wired | WIRED | Lines 16/79-83: DeribitProcessor::new(raw_rx, Some(recording_svc.sender()), cancel) |
| src/feed/pipeline.rs | src/feed/recording/mod.rs | recording service wired | WIRED | Lines 19/48-52: RecordingService::start(recording_dir, Venue::Deribit, cancel.clone()) |
| src/main.rs | src/feed/pipeline.rs | run_pipeline called | WIRED | Lines 5/109-115: use prediction::feed::pipeline; pipeline::run_pipeline(mode, &config.venues.deribit, ...) |

### Requirements Coverage

| Requirement | Status | Blocking Issue |
|-------------|--------|----------------|
| FEED-01 (Deribit WS connection + JSON-RPC subscribe) | SATISFIED | None |
| FEED-02 (Order book state management) | SATISFIED | None |
| FEED-06 (Normalized MarketSnapshot events) | SATISFIED | None |
| FEED-07 (Bounded async channel for downstream) | SATISFIED | None |
| FEED-08 (Exchange timestamp recorded) | SATISFIED | None |
| TEST-01 (Mock data source for offline testing) | SATISFIED | None |

### Anti-Patterns Found

| File | Location | Pattern | Severity | Impact |
|------|----------|---------|----------|--------|
| src/feed/deribit/normalize.rs | Line 132 | channel field set to empty String before parse | Info | RecordLine.channel is empty string for all recorded messages; raw frame preserved intact; channel extraction requires restructuring fan-out; not a blocker for any success criterion |

### Human Verification Required

#### 1. Live MarketSnapshot Log Output

**Test:** Run `cargo run -- --mock` in the project directory, wait 5 seconds, then press Ctrl+C.
**Expected:** Structured JSON log lines at info level with venue=deribit, instrument name, bid/ask price values, stale=false, and incrementing seq numbers. Binary exits cleanly with shutdown complete message.
**Why human:** Requires running the binary. Integration tests exercise the same code path but terminal output format is only confirmable by a human.

#### 2. JSONL Recording File on Disk

**Test:** After running `cargo run -- --mock` for several seconds and stopping, check that `recordings/deribit/{today}.jsonl` exists in the project directory.
**Expected:** File at `recordings/deribit/2026-02-22.jsonl` with multiple newline-delimited JSON lines. Each line parseable as JSON with raw, local_ts, venue, channel, instrument fields. Note: channel will be empty string (known behavior).
**Why human:** Confirms the actual file created by a cargo run invocation. Unit tests verify the write path but do not run the binary.

### Gaps Summary

No gaps. All 5 observable truths are verified against the actual codebase. All 13 required artifacts exist, are substantive (not stubs), and are wired into the pipeline. All 14 key links are confirmed in source code. The test suite passes completely: 99 tests (55 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc tests), zero failures, zero warnings.

The one anti-pattern (empty channel field in RecordLine) is a minor data quality issue. The raw field preserves the complete WebSocket frame text, so recordings are complete and fully replayable. This does not affect any of the 5 success criteria.

---

_Verified: 2026-02-22T12:53:56Z_
_Verifier: Claude (gsd-verifier)_
