# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-24)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.1 Paper Trading Validation -- Phase 16 (Settlement Outcome Tracking)

## Current Position

Phase: 17 of 17 (Signal Analysis Tooling)
Plan: 2 of 3 complete
Status: Executing Phase 17
Last activity: 2026-02-26 -- Completed 17-02 (Pipeline Integration)

Progress: [############################..] 93% (v1.0: 36/36 plans | v1.1: 11/TBD plans)

## Performance Metrics

**v1.0 Summary:**
- Total plans completed: 36
- Total phases: 13
- Lines of Rust: 22,751
- Tests: 417+
- Timeline: 4 days (2026-02-21 to 2026-02-24)

**v1.1:**
- Plans completed: 11
- Phases: 4 (14-17)

| Phase | Plan | Duration | Tasks | Files |
|-------|------|----------|-------|-------|
| 14-failure-alerting | 01 | 7min | 3 | 6 |
| 14-failure-alerting | 02 | 14min | 3 | 8 |
| 15-state-persistence | 01 | 7min | 2 | 7 |
| 15-state-persistence | 02 | 8min | 2 | 4 |
| 16-settlement-outcome-tracking | 01 | 8min | 2 | 8 |
| 16-settlement-outcome-tracking | 02 | 7min | 1 | 3 |
| 16-settlement-outcome-tracking | 03 | 17min | 2 | 10 |
| 16-settlement-outcome-tracking | 04 | 2min | 1 | 1 |
| 17-signal-analysis-tooling | 01 | 12min | 2 | 15 |
| 17-signal-analysis-tooling | 02 | 11min | 2 | 5 |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Full decision history preserved in .planning/milestones/v1.0-ROADMAP.md

Recent decisions:
- v1.1: Zero new crate dependencies -- all features built on existing dependency tree
- v1.1: Alerting first build order -- monitors during rest of v1.1 development
- v1.1: Phase 16 (Settlement) flagged for /gsd:research-phase due to venue API heterogeneity
- 14-01: PipelineLiveness uses AtomicI64 (epoch millis) not Mutex<DateTime> for lock-free reads
- 14-01: Severity thresholds: PartialCoverage Critical at <50% venues, SignalGap Critical at >2x threshold
- 14-02: AlertMonitor collects conditions into Vec before processing for clean cleanup separation
- 14-02: Liveness recording at end of computation loop (not per-pattern) captures full evaluation cycles
- 14-02: Startup grace period for signal gap avoids false alarms during pipeline warmup
- 15-01: Deserialize added to DailyRollup in Task 1 (pulled forward) to unblock CheckpointState compilation
- 15-01: CheckpointState version is u32 (not semver string) for compact schema evolution
- 15-02: Checkpoint tick uses Duration::from_secs(u64::MAX) when persistence disabled to avoid overhead
- 15-02: Final checkpoint after trade_logger.flush() ensures JSONL completeness up to checkpoint timestamp
- 15-02: Recovery errors degrade gracefully with warnings, never block startup
- 16-01: VenueChecker enum dispatch instead of async-trait crate -- zero new dependencies
- 16-01: CheckContext struct passes expiry/strike/direction to venue checkers alongside event_id/instrument
- 16-01: Kalshi scalar detection checks settlement_value_dollars for non-binary values even on yes/no results
- 16-01: Polymarket outcome_prices parsed as Vec<String> (JSON-in-JSON) with configurable threshold
- 16-02: Free function check_trigger() avoids borrow checker conflict in two-phase poll_cycle
- 16-02: TrackedEvent.is_backfill with serde(default) for backward-compatible serialization
- 16-02: Backfill timeouts stored in drain vec; caller sends after initialization complete
- 16-03: Arc<RwLock<HashMap>> shared state between SettlementMonitor and PaperTradeTracker for checkpoint inclusion
- 16-03: Net P&L (fee-adjusted) as headline settlement_pnl per CONTEXT.md decision
- 16-03: Timeout positions evicted immediately from recently_settled per CONTEXT.md
- 16-03: Rate limiters created for Polymarket/Kalshi in pipeline at 5 req/s default for settlement REST calls
- 16-03: Settlement channel kept open when monitor disabled via _settlement_tx_hold variable
- 16-03: CheckpointState version bumped to 2 with backward-compatible serde(default) on settlement_tracking
- 16-04: Deribit REST URL derived from ws_url config (replace wss->https, truncate at /ws/) rather than adding a new config field
- 16-04: Kalshi settlement checker replicates private key loading inline (load_kalshi_key_from_file is private to pipeline.rs)
- 16-04: Fallback rate limiters created at 5 req/s for any venue missing from pipeline_handles.venue_rate_limiters
- 17-01: Hash derive added to ThresholdStatus for AccumulatorKey HashMap usage
- 17-01: ThresholdStatus computed from static_floor comparison in SpreadEngine (PassedBoth vs PassedStaticOnly vs Filtered)
- 17-01: False positive rate = (gross_hits - net_hits) / total_settled (fees ate the edge)
- 17-02: analysis_accumulators stored as Vec<(K,V)> in CheckpointState for JSON key compatibility
- 17-02: SettlementLogger::log_record made generic over impl Serialize for both record types
- 17-02: Prometheus gauges emitted once after settlement batch (not per-position) for efficiency
- 17-02: Stale fill detection applied at signal time matching SpreadResult exchange timestamps

### Pending Todos

None.

### Blockers/Concerns

- Risk premium calibration needs 2-4 weeks of parallel data collection
- Expired test instrument BTC-27JUN25-100000-C in events.toml
- Kalshi market_tickers = [] in default config
- Polymarket has no clean resolution endpoint -- must infer from Gamma API closed flag + price lock
- Windows rename() is not atomic when target exists -- RESOLVED: atomic_write() uses remove-then-rename fallback

## Session Continuity

Last session: 2026-02-26
Stopped at: Completed 17-02-PLAN.md (Pipeline Integration)
Next action: Execute 17-03-PLAN.md
