# Phase 2: Deribit Feed and Data Pipeline - Context

**Gathered:** 2026-02-22
**Status:** Ready for planning

<domain>
## Phase Boundary

End-to-end data path from Deribit WebSocket through normalization to JSONL recording, with mock data abstraction. This phase proves the entire pipeline architecture with a single venue: connect to Deribit, maintain order book state, publish normalized MarketSnapshot events through a bounded channel, record every raw message, and support offline development via mock data sources. No reconnection logic, no heartbeat monitoring, no staleness detection — those are Phase 3.

</domain>

<decisions>
## Implementation Decisions

### WebSocket Connection Behavior
- Public channels only in Phase 2 — no authentication. Auth comes when private endpoints are needed (Phase 3+)
- Single multiplexed WebSocket connection to Deribit, all instrument subscriptions over one connection
- Instrument list is dynamic — the client takes a list of instrument names as input. In Phase 2 these come from config; in Phase 5 the event registry drives them
- Subscribe to 4 channel types:
  - `book.{instrument}.none.20.100ms` — top 20 levels, grouped snapshots every 100ms
  - `ticker.{instrument}.raw` — last price, mark price, index price, greeks
  - `trades.{instrument}.raw` — actual trade flow for volume profiling
  - `deribit_price_index.btc_usd` — underlying BTC index price (needed for Black-76 forward price in Phase 7)

### Order Book Representation
- Top 20 levels only, using the grouped `book.{instrument}.none.20.100ms` channel — NOT the raw delta channel
- This means no delta application logic: each book message is a complete top-20 snapshot that replaces the previous state
- The 100ms throttling is a feature — prediction markets update far less frequently, so microsecond book precision is unnecessary
- Implicit snapshot on first message after subscribe (Deribit sends full state as first grouped message)
- Strict change_id verification: every message's prev_change_id must match our last change_id
- On sequence gap or inconsistency: immediately mark instrument data as stale/unavailable downstream, then re-subscribe to recover

### Data Recording Format
- Each JSONL line contains BOTH the raw WebSocket text frame AND parsed metadata: `{"raw": "<exact WS frame>", "local_ts": ..., "venue": "deribit", "channel": ..., "instrument": ...}`
- Maximum fidelity — raw frame preserved for re-parsing if logic changes, metadata enables efficient filtering without re-parsing
- Daily file rotation: one file per day per venue (e.g., `recordings/deribit/2026-02-22.jsonl`)
- Async recording with bounded buffer: messages sent to a recording channel, dedicated writer task flushes to disk. Pipeline never blocks on I/O. Buffer overflow drops oldest unwritten messages (accept data loss over pipeline stall)
- Generic `Recorder` trait from day one — takes venue + raw message + timestamp. Polymarket and Kalshi reuse it in Phase 4

### Mock Data Layer Design
- Two mock data modes:
  - **Replay**: Read previously recorded JSONL files and feed through the pipeline as if live
  - **Synthetic**: Generate realistic order book snapshots with configurable parameters (spread, depth, volatility) for development before recordings exist
- Trait abstraction at two levels:
  - **WS message level**: Mock produces raw text frames identical to Deribit format. Tests the full parsing + normalization pipeline
  - **Normalized snapshot level**: Mock produces MarketSnapshot directly, bypassing WS parsing. Tests downstream consumers in isolation
- Configurable speed multiplier for replay: 1x = real-time pacing, 0 = instant (CI/fast tests), 10x = fast-forward
- Accessible from both integration tests (programmatic API — pass mock as DataSource) and CLI (--mock or --replay flags for manual testing)

### Claude's Discretion
- Exact channel message parsing implementation (serde structs vs manual JSON)
- Bounded channel buffer sizes for normalization bus and recording
- Internal thread/task architecture for the WS client
- File naming convention details for recordings beyond the daily pattern
- Synthetic data generation algorithm and default parameters

</decisions>

<specifics>
## Specific Ideas

- The `book.{instrument}.none.20.100ms` channel choice is deliberate — it eliminates delta application complexity and the 100ms throttle matches the system's temporal resolution needs. This is not a book-building system; it's a probability extraction system that happens to read order books.
- Recording both raw + metadata in the same JSONL line means one file handles both "what did Deribit actually send?" and "when did we receive it?" without cross-referencing files.
- The two-level mock trait (WS-level and snapshot-level) enables testing the Deribit parser independently from testing downstream consumers. Different test suites pick the right level.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-deribit-feed*
*Context gathered: 2026-02-22*
