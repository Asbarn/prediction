# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** Phase 6: Pricing Engine

## Current Position

Phase: 6 of 9 (Pricing Engine)
Plan: 2 of 4 in current phase
Status: In Progress
Last activity: 2026-02-23 -- Completed 06-01 (spread computation primitives)

Progress: [####----------] 25% Phase 6 in progress (1/4 plans done)

## Performance Metrics

**Velocity:**
- Total plans completed: 16
- Average duration: 9min
- Total execution time: ~2.4 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 24min | 8min |
| 02-deribit-feed | 4/4 | 33min | 8min |
| 03-feed-infrastructure | 3/3 | 32min | 11min |
| 04-multi-venue-feeds | 3/3 | 13min | 4min |
| 05-event-mapping | 3/3 | 29min | 10min |
| 06-prediction-market-spreads | 1/4 | 11min | 11min |

**Recent Trend:**
- Last 5 plans: 13min, 11min, 4min, 14min, 11min
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
- [03-01]: Heartbeat detection via fast string check (contains method:heartbeat) rather than relying solely on serde untagged ordering
- [03-01]: Heartbeat responses exempt from rate limiting -- Deribit closes connection on delayed test_request response
- [03-01]: Staleness gate uses OR logic: book.is_stale || exchange_data_stale (exchange timestamp age check)
- [03-01]: metrics facade added with no recorder (zero-cost no-ops) -- Prometheus recorder deferred to Phase 6
- [03-01]: Heartbeat timeout at 2x interval for dead connection detection
- [03-02]: Staleness gate uses OR logic: is_stale = book.is_stale || exchange_data_stale
- [03-02]: metrics facade macros are zero-cost no-ops without recorder (Prometheus exporter deferred to Phase 6)
- [03-02]: Processor async tests use u64::MAX staleness threshold for hardcoded JSON timestamps
- [03-02]: biased select in recording_task: cancel > recv > flush tick
- [03-02]: Periodic flush resolves Phase 2 TODO (writer.rs line 51-52)
- [03-03]: backoff::Backoff trait must be explicitly imported for reset() and next_backoff() methods
- [03-03]: Backoff reset on first message received (not on connection success) prevents burn-through with accept-then-close servers
- [03-03]: Rate limiter is Optional on DeribitClient (None for Mock/Replay, Some for Live via supervisor)
- [03-03]: Heartbeat test_request responses exempt from rate limiting per research pitfall 6
- [04-03]: Fan-in forwarding task pattern: processors keep (Processor, Receiver) API, forwarding tasks pipe to shared sender
- [04-03]: Kalshi graceful degradation: missing credentials log warning and skip, no crash
- [04-03]: Private key loading: KALSHI_PRIVATE_KEY env var priority, falls back to config file path
- [04-03]: Per-venue recording directories (recordings/deribit, recordings/polymarket, recordings/kalshi)
- [05-01]: Direction enum replaces String for type-safe above/below handling
- [05-01]: LifecycleStatus enum (Active/Expiring/Expired) with Default=Active for backward compat
- [05-01]: All new EventMapping fields use #[serde(default)] for zero-breakage migration
- [05-01]: EventRegistry indexes Polymarket by token_id (not condition_id) for pipeline instrument lookup
- [05-01]: Expiry threshold validation checks uniqueness rather than ordering
- [05-02]: Unknown SourcePair uses index_oracle weight (0.5) as conservative default
- [05-02]: compute_risk_for_mapping uses expiry date at 00:00:00 UTC as prediction resolution estimate
- [05-02]: inflate_risk_score uses default weights for composite recalculation (global config, not per-score)
- [05-03]: DiscoveryConfig.min_poll_interval_secs() used as lifecycle tick interval; venues polled independently
- [05-03]: Kalshi asset extracted from ticker prefix (KX{ASSET}D pattern) rather than separate API field
- [05-03]: Polymarket discovery limited to deactivation monitoring in v1 (no structured field extraction)
- [05-03]: Pipeline accepts optional EventRegistry parameter (pass-through for Phase 6 annotation)
- [Phase 06]: SpreadConfig added to SystemConfig with serde(default) for backward-compatible config loading
- [Phase 06]: RollingStats uses f64 (not Decimal) per research recommendation for Welford's algorithm at metrics boundary
- [Phase 06]: Kalshi ceil() uses Decimal::ceil() (integer ceiling) for conservative per-contract fee estimation

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 7]: statrs 0.18 requires Rust 1.87+ -- verify toolchain or implement Normal CDF manually
- [Phase 7]: Risk premium calibration needs 2-4 weeks of parallel data collection before signals are meaningful

## Session Continuity

Last session: 2026-02-23
Stopped at: Completed 06-01-PLAN.md (spread computation primitives)
Resume file: .planning/phases/06-prediction-market-spreads/06-01-SUMMARY.md
