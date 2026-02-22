# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** Phase 2: Deribit Feed

## Current Position

Phase: 2 of 9 (Deribit Feed)
Plan: 4 of 4 in current phase
Status: Phase Complete
Last activity: 2026-02-22 -- Completed 02-04 (mock data, pipeline assembly, CLI integration)

Progress: [########..] 78%

## Performance Metrics

**Velocity:**
- Total plans completed: 7
- Average duration: 8min
- Total execution time: 0.9 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 24min | 8min |
| 02-deribit-feed | 4/4 | 33min | 8min |

**Recent Trend:**
- Last 5 plans: 9min, 9min, 9min, 7min, 8min
- Trend: stable

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 9-phase comprehensive structure -- split original 6-phase research suggestion into 9 for clearer delivery boundaries (separated feed reliability from connection, multi-venue from event mapping, pricing engine from signal generation)
- [Roadmap]: Deribit feed first -- proves entire pipeline architecture before adding Polymarket/Kalshi complexity
- [Roadmap]: Prediction market arb before cross-asset -- validates pipeline end-to-end with simpler probability-vs-probability math before adding Black-76
- [01-01]: Added uuid serde feature flag -- required for TraceId serialization, not in original research spec
- [01-01]: 16 smoke tests covering all domain types, error severity, serde roundtrips
- [01-02]: load_credentials() returns Credentials directly (not Result) -- all fields optional in Phase 1
- [01-02]: Logging filter strings scoped to crate (prediction={level}) for independent per-layer filtering
- [01-02]: URL validation uses simple prefix checking rather than full URL parser
- [01-03]: ConfigReloader returns (ConfigReloader, Receiver) -- Sender moved into watcher thread, not clonable
- [01-03]: Upgraded notify-debouncer-mini 0.5->0.7 to resolve notify 7/8 version conflict
- [01-03]: Shutdown handler: Ctrl+C + SIGTERM only, no SIGHUP -- file watcher handles config reload cross-platform
- [02-01]: RawDataSource returns mpsc::Receiver<RawMessage> from start() -- avoids RPITIT lifetime complexity
- [02-01]: f64 at serde boundary -- Decimal conversion deferred to normalization layer (Plan 02)
- [02-01]: BookData bids/asks as Vec<[f64; 2]> -- matches grouped channel snapshot format
- [02-01]: Testnet URL in venues.toml default config for safe development
- [02-02]: f64 to Decimal via from_f64_retain (never panics) instead of try_from for edge-case floats
- [02-02]: Ticker updates produce snapshots even without prior book data (empty book fallback)
- [02-02]: Stale snapshots still published downstream so consumers see is_stale flag
- [02-02]: Trades and price_index do not produce MarketSnapshot events in Phase 2
- [02-03]: 8192-message bounded channel for recording buffer -- balances memory with burst tolerance
- [02-03]: Flush on every write in Phase 2 for correctness -- optimize to periodic flush in Phase 3
- [02-03]: Drop newest on buffer overflow via try_send -- never block data pipeline
- [02-03]: Append mode file opens for crash safety -- existing recordings preserved on restart
- [02-04]: StdRng::from_entropy instead of thread_rng -- ThreadRng not Send across await in tokio::spawn
- [02-04]: Replay reads entire JSONL into memory upfront -- simpler than streaming, adequate for dev recordings
- [02-04]: Pipeline takes DeribitConfig directly not VenuesConfig -- narrower interface, clearer dependency
- [02-04]: Added Deserialize to RecordLine -- required for replay to parse JSONL back into structured data

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 4]: Polymarket has two separate WS endpoints (CLOB and RTDS) with different semantics -- needs research during planning
- [Phase 4]: Kalshi uses RSA-PSS auth which requires the `rsa` crate -- needs research during planning
- [Phase 7]: statrs 0.18 requires Rust 1.87+ -- verify toolchain or implement Normal CDF manually
- [Phase 7]: Risk premium calibration needs 2-4 weeks of parallel data collection before signals are meaningful

## Session Continuity

Last session: 2026-02-22
Stopped at: Completed 02-04-PLAN.md (Phase 2 complete -- mock data, pipeline, CLI)
Resume file: .planning/phases/02-deribit-feed/02-04-SUMMARY.md
