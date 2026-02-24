# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-21)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** Phase 10: Critical Pipeline Wiring

## Current Position

Phase: 10 of 10 (Critical Pipeline Wiring)
Plan: 1 of 1 in current phase
Status: Complete
Last activity: 2026-02-24 -- Completed 10-01 (Critical pipeline wiring - OBSV-04, SGNL-05, OBSV-01 gap closure)

Progress: [##############] 100% All 10 phases complete (1/1 plans done)

## Performance Metrics

**Velocity:**
- Total plans completed: 28
- Average duration: 10min
- Total execution time: ~4.3 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 24min | 8min |
| 02-deribit-feed | 4/4 | 33min | 8min |
| 03-feed-infrastructure | 3/3 | 32min | 11min |
| 04-multi-venue-feeds | 3/3 | 13min | 4min |
| 05-event-mapping | 3/3 | 29min | 10min |
| 06-prediction-market-spreads | 4/4 | 49min | 12min |
| 07-options-pricing-engine | 5/5 | 33min | 7min |
| 08-cross-asset-signal-generation | 2/2 | 17min | 9min |
| 09-replay-and-hardening | 3/3 | 47min | 16min |
| 10-critical-pipeline-wiring | 1/1 | 6min | 6min |

**Recent Trend:**
- Last 5 plans: 10min, 29min, 12min, 6min, 6min
- Trend: stable

*Updated after each plan completion*
| Phase 07 P04 | 7min | 2 tasks | 4 files |
| Phase 07 P05 | 9min | 2 tasks | 3 files |
| Phase 08 P01 | 7min | 2 tasks | 8 files |
| Phase 08 P02 | 10min | 2 tasks | 4 files |
| Phase 09 P01 | 29min | 2 tasks | 13 files |
| Phase 09 P02 | 12min | 2 tasks | 8 files |
| Phase 09 P03 | 6min | 1 task | 5 files |
| Phase 10 P01 | 6min | 2 tasks | 4 files |

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
- [06-02]: metrics-exporter-prometheus 0.18 (not 0.16) -- matches metrics ^0.24 with no hyper conflicts
- [06-02]: Prometheus setup failure is non-fatal -- logs warning and continues without metrics
- [06-02]: Probability import is cfg(test) only -- type resolved through field access in production code
- [06-03]: Staleness thresholds per-venue: 5s Polymarket (WebSocket), 15s Kalshi (REST-polled)
- [06-03]: min_samples in ThresholdConfig for configurable cold start transition (default 30)
- [06-03]: SpreadLogger periodic flush every 100 writes for I/O performance
- [06-03]: Signal delivery via try_send (non-blocking) -- engine never blocks on slow downstream
- [06-04]: Fill prices use top-of-book probabilities as proxy for walk-the-book fills in paper trade v1
- [06-04]: SpreadEngine::run takes optional ptrade_snap_tx for backward-compatible snapshot forwarding
- [06-04]: Fill snapshot generates initial MTM data point since position is Open when MTM pass runs
- [07-01]: Black-76 functions as pub(crate) free functions (no struct wrapper for stateless math)
- [07-01]: Instrument parser handles 1-digit and 2-digit day formats (e.g., "3JAN26" and "27JUN25")
- [07-01]: Normal::standard() created per function call (trivial allocation, simpler than lazy_static)
- [07-03]: partition_point binary search for O(log n) interpolation between observed strikes
- [07-03]: Flat extrapolation returns boundary IV (first or last) rather than None for extreme strikes
- [07-03]: Degraded quality returns flat ATM vol for any strike (graceful fallback)
- [07-03]: nearest_bracket on exact observed strike returns adjacent strikes (not self-bracket)
- [Phase 07-02]: near_expiry_cutoff_hours added to SolverConfig (duplicated from PricingConfig) for solver-level access
- [Phase 07-02]: Brent fallback uses full [iv_min, iv_max] bracket for maximum robustness
- [Phase 07-02]: Brenner-Subrahmanyam initial guess clamped to [iv_min, iv_max] for safe starting point
- [Phase 07-04]: ATM delta tolerance 0.05 (Black-76 N(d1) = 0.54 for sigma=0.20/T=1.0, not exactly 0.5)
- [Phase 07-04]: CallSpreadResult/Nd2Result pub visibility to match ProbabilityExtraction pub struct
- [Phase 07-04]: Vega normalized to per-1%-vol-move (raw_vega / 100) for practical interpretation
- [Phase 07-05]: Fan-out: blocking send to SpreadEngine (primary), try_send to PricingEngine (best-effort)
- [Phase 07-05]: Deribit inverse convention: option_price_usd = option_price_btc * forward for Black-76
- [Phase 07-05]: Near-expiry intrinsic: confidence=0.3, method=IntrinsicOnly, intrinsic delta, vega/theta=0
- [Phase 07-05]: _probability_rx held in main scope to prevent channel-closed errors for PricingEngine
- [Phase 08-01]: Added Deserialize to PricingMethod, ConfidenceComponents, SolverResult, SolverMethod, ThresholdComponents, DualTimestamp for ArbSignal JSON roundtrip
- [Phase 08-01]: DualTimestamp Deserialize sets mono to Instant::now() since monotonic clock has no meaningful serialized value
- [Phase 08-02]: Liquidity factor = min(prediction fill_ratio, options ba_proxy) where ba_proxy = max(0.1, 1.0 - ba_spread * 5.0)
- [Phase 08-02]: Options fee estimate = deribit_taker_fee_rate * underlying_price * |delta| (USD-scale approximate taker fee)
- [Phase 08-02]: Both ArbDirection variants computed per event update, all logged to JSONL regardless of threshold status
- [Phase 08-02]: 3-way fan-out: SpreadEngine (blocking) + PricingEngine (try_send) + CrossAssetEngine (try_send)
- [Phase 09-01]: axum 0.8 with http1 feature required for axum::serve (json+tokio alone insufficient)
- [Phase 09-01]: VenueHealth created in pipeline per venue (supervisors don't accept health trackers yet)
- [Phase 09-01]: PipelineHandles struct returns snapshot_rx + venue_health from run_multi_venue_pipeline
- [Phase 09-01]: TradeEvent made pub for offline tooling access from integration tests
- [Phase 09-01]: Schema documentation as inline doc comments (JSONL Schema v1.0) on all 4 output types
- [Phase 09-02]: ReplaySource enum (File vs Records) avoids temp file overhead for multi-venue replay
- [Phase 09-02]: Recorded local_ts used for DualTimestamp wall clock (mono=Instant::now(), no meaningful replay value)
- [Phase 09-02]: replay_mode bypasses all wall-clock staleness gates (simplest approach per research)
- [Phase 09-02]: forward_snapshots made pub for reuse from replay module
- [Phase 09-02]: DataMode::Replay routes to run_replay_pipeline (multi-venue) not run_pipeline (single-file)
- [Phase 09-03]: VenueHealth passed as Arc to supervisor constructors; forward_snapshots uses Option<Arc<VenueHealth>> for replay/mock compatibility
- [Phase 10-01]: forward_snapshots annotates event_id via EventRegistry lookup before fan-in send
- [Phase 10-01]: borrow_and_update() in config watch subscriber to properly mark value as seen
- [Phase 10-01]: Config hot-reload subscriber only spawned in live mode (replay must be deterministic)
- [Phase 10-01]: ArbSignal consumer uses metrics::counter! with direction label for Prometheus

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 7]: statrs 0.18 confirmed working on Rust 1.92 (MSRV 1.65) -- resolved
- [Phase 7]: Risk premium calibration needs 2-4 weeks of parallel data collection before signals are meaningful

## Session Continuity

Last session: 2026-02-24
Stopped at: Completed 10-01-PLAN.md (Critical pipeline wiring - OBSV-04, SGNL-05, OBSV-01 gap closure)
Resume file: .planning/phases/10-critical-pipeline-wiring/10-01-SUMMARY.md
