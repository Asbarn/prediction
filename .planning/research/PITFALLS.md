# Pitfalls Research

**Domain:** Adding settlement outcome tracking, signal analysis, failure alerting, and file-based persistence to an existing async Rust trading system (v1.1)
**Researched:** 2026-02-24
**Confidence:** HIGH (integration pitfalls based on codebase analysis), MEDIUM (venue-specific settlement API behavior)

---

## Critical Pitfalls

### Pitfall 1: Settlement Outcome Data Is Harder to Get Than Expected

**What goes wrong:**
Developers assume each venue has a clean "get settlement result" API endpoint returning a simple yes/no outcome and settlement price. In reality:

- **Deribit** has `public/get_settlement_history_by_instrument` which returns settlement/delivery events, but options settlement uses a 30-minute TWAP of the Deribit Index (07:30-08:00 UTC) as the delivery price. The actual settlement result (ITM/OTM) must be computed by comparing this delivery price against the strike. The API may not return data immediately at 08:00 UTC -- there can be a processing delay.

- **Polymarket** has no dedicated "get resolution result" REST endpoint as of early 2025. Market resolution status must be inferred from the Gamma Markets API (`active: false`, `closed: true` fields) or by monitoring on-chain resolution events. The py-clob-client GitHub issue #117 confirms this gap. Price history for resolved markets is only available at 12+ hour granularity.

- **Kalshi** has a `GET /markets/{ticker}` endpoint that includes a `result` field ("yes"/"no"/null), plus a `GET /portfolio/settlements` endpoint for position-level settlement data. However, Kalshi's authentication (RSA-signed JWTs) adds complexity and their API semantics differ from the other venues.

The three venues have fundamentally different resolution semantics, timing, and data availability patterns.

**Why it happens:**
Settlement tracking is often designed assuming a uniform API shape across venues. The happy path is coded first, edge cases are discovered in production.

**How to avoid:**
- Design the settlement tracker with a per-venue adapter trait that returns a normalized `SettlementOutcome { event_id, venue, outcome: Yes/No/Unknown/Disputed, settlement_price: Option<Decimal>, settled_at: DateTime, raw_data: Value }`. Each venue implements its own polling/detection logic.
- For Deribit: poll `get_settlement_history_by_instrument` for each tracked instrument after the expiry time. Compute ITM/OTM from delivery_price vs strike. Do NOT assume the result is available immediately -- poll with exponential backoff starting from expiry + 5 minutes.
- For Polymarket: poll the Gamma Markets API for the `closed` and `active` flags. As a fallback, check if the final price locks to exactly 0 or 1 (resolution). Consider on-chain event monitoring as a secondary signal.
- For Kalshi: poll `GET /markets/{ticker}` and check the `result` field. Requires authenticated requests (RSA JWT signing already implemented in `feed::kalshi::auth`).
- Implement a `SettlementStatus::Pending` state for outcomes not yet available, with configurable retry/timeout.

**Warning signs:**
- Settlement tracker reports 0% of outcomes resolved after expected expiry times.
- `Unknown` or `Pending` outcomes persist for days without transitioning.
- Deribit options show as "unsettled" because the instrument ID changes after expiry (delisted from active instruments).

**Phase to address:**
Phase 1 (Settlement Outcome Tracking) -- this is the foundational data source all downstream analysis depends on.

---

### Pitfall 2: Comparing Signals Against Outcomes With Wrong Timing Windows

**What goes wrong:**
Signal analysis computes hit rate by asking "did the signal predict the outcome correctly?" but gets the timing relationship wrong. Common mistakes:

1. **Using the signal at entry time instead of at fill time.** The existing `PaperTradeTracker` correctly models adverse selection by filling at next-tick prices, but signal analysis might accidentally compare the signal's net_spread at signal_time against the final outcome, ignoring that the fill price was different.

2. **Comparing against the wrong settlement window.** A signal fired on Monday for an event expiring Friday may have been a "correct" signal at the time but the market moved by Friday. The analysis must distinguish: (a) was the signal directionally correct at settlement? (b) was there a profitable exit window before settlement? (c) was the entry-to-settlement P&L positive?

3. **Survivorship bias in hit rate.** If the system generates 100 signals but only 60 get filled (40 remain Pending and expire), reporting hit rate on the 60 filled trades overstates accuracy. The 40 unfilled signals were likely in fast-moving markets where the opportunity evaporated -- exactly the hard cases.

**Why it happens:**
Signal analysis is implemented as a post-hoc computation over trade logs without carefully tracing the full lifecycle: signal_time -> fill_time -> mtm_updates -> settlement_time. Each stage has different prices.

**How to avoid:**
- Define hit rate as: `filled_and_profitable_at_settlement / total_filled`. Report separately: `fill_rate = filled / total_signals` and `signal_accuracy = correct_direction_at_settlement / total_settled`.
- Track time-to-convergence: how long after signal generation does the spread move in the predicted direction? This requires correlating `SpreadResult.timestamp_ms` with subsequent MTM updates from `MtmSnapshot` entries.
- Never compute hit rate on unsettled trades. Use `PositionStatus::Settled` as the gate. Report separately how many trades are still `Open` (not yet settled).
- Include adverse selection in the P&L computation: `realized_pnl = settlement_pnl - adverse_selection_cost`.

**Warning signs:**
- Hit rate looks suspiciously high (>70%) -- likely measuring something other than true settlement P&L.
- Time-to-convergence is negative or undefined for many trades -- signals may be stale by the time they fill.
- Large gap between "directionally correct" and "profitable after costs" rates.

**Phase to address:**
Phase 2 (Signal Analysis Tooling) -- must be designed correctly from the start since retroactive correction requires re-processing all historical data.

---

### Pitfall 3: File Persistence That Corrupts State on Crash or Power Loss

**What goes wrong:**
The system adds file-based persistence for paper P&L and signal history by serializing state to a JSON file. On crash, the file contains a partial write: truncated JSON that fails to parse on restart, losing all accumulated state. Or worse: the file is empty (opened for write, OS crash before flush).

This is especially dangerous because the system currently operates entirely in-memory (`PaperTradeTracker.pending: HashMap`, `PaperTradeTracker.open: Vec<PaperPosition>`, `DailyAggregator.daily_pnl: HashMap`). The transition from "pure in-memory" to "persisted" is where corruption bugs hide.

**Why it happens:**
Standard `File::create()` + `serde_json::to_writer()` is not atomic. On any OS, a crash between open and complete write leaves a corrupted file. Even with `BufWriter::flush()`, the OS may not have synced to disk. On Windows specifically, `rename()` is not atomic if the target already exists (unlike POSIX).

**How to avoid:**
- Use the write-to-temp-then-rename pattern that `ContractLifecycleManager` already uses for events.toml (see `atomic_write()` in `events/lifecycle.rs` line ~487). Replicate this exact pattern for P&L state files.
- On Windows, use `tokio::fs::remove_file()` then `tokio::fs::rename()` since Windows `rename()` fails if the target exists. Or use the `tempfile` crate which handles cross-platform atomicity.
- Use JSONL (append-only) for the signal history log rather than overwriting a single JSON file. The existing `TradeLogger` in `paper_trade/tracker.rs` already does this correctly -- extend it rather than replacing it.
- For the aggregate P&L state (daily rollups, open positions), serialize as a single JSON checkpoint file written atomically at regular intervals (e.g., every 60 seconds and on shutdown).
- On startup, if the primary state file is corrupted: fall back to the temp file (which is the in-flight write), then fall back to replaying from the JSONL trade log to reconstruct state.

**Warning signs:**
- State file is empty (0 bytes) after a restart.
- `serde_json::from_str()` fails with "unexpected EOF" on startup.
- Paper P&L shows zero after a restart despite days of accumulated data.
- The `.tmp` file exists alongside the primary file (indicates incomplete write).

**Phase to address:**
Phase 4 (File-Based Persistence) -- but the design must be settled during Phase 1 since settlement outcomes also need persistence.

---

### Pitfall 4: Alerting That Monitors the Wrong Thing (Detecting Noise, Missing Silence)

**What goes wrong:**
Failure alerting is added to detect degraded states, but it monitors symptoms (e.g., reconnection events, error rates) instead of the absence of expected events. The most dangerous failures are **silent**: a venue feed connects successfully but stops sending data, the pricing engine computes probabilities but a config change means no events map anymore, or the spread engine runs but the BasisRiskCache is stale because the lifecycle manager silently stopped polling.

The existing system has `VenueHealth` (feed/health.rs) tracking connection state and `last_message_at`, plus metrics for staleness rejections. But these are binary -- they detect "is the feed up?" not "is the system producing useful output end-to-end?"

**Why it happens:**
Alerting is usually built bottom-up: instrument each component, alert on errors. The systemic failures that actually cost money are the ones where no individual component errors but the end-to-end pipeline stops producing correct output. This requires top-down monitoring: "when was the last valid signal?" "when was the last spread computation?" "are all expected event pairs producing spreads?"

**How to avoid:**
- Implement **liveness checks** at each pipeline stage, not just connectivity checks:
  - Feed layer: `last_message_at` (already exists in VenueHealth) + **message rate check** (messages/minute should be within expected range)
  - Spread engine: `last_spread_computed_at` per event_id + **computation rate check**
  - Signal engine: `last_signal_emitted_at` or `last_signal_evaluated_at`
  - Paper trade: `last_position_update_at`
- Alert on **absence**, not just **presence** of errors:
  - "No spread computed for event X in 10 minutes" is more valuable than "5 staleness rejections"
  - "Paper trade tracker received 0 snapshots in 5 minutes" catches the silent pipe disconnect
- Use the **dead man's switch** pattern: each component must positively assert it is alive within a configurable interval. If it doesn't, the monitor fires.
- Start simple: a single periodic task (every 60 seconds) that checks timestamps across all pipeline stages and emits a structured log/metric if any stage is stale. Do NOT build a complex event-driven alerting framework.

**Warning signs:**
- All venue feeds show "healthy" but no signals are being generated (config drift, stale registry).
- Alerts fire constantly for expected transient issues (reconnections) creating alert fatigue.
- A venue silently disconnects and nobody notices for hours because VenueHealth still shows the last successful connection.

**Phase to address:**
Phase 3 (Failure Alerting) -- but the liveness timestamp infrastructure should be added to each engine as those engines are touched in Phases 1-2.

---

### Pitfall 5: Blocking the Tokio Runtime With Synchronous File I/O

**What goes wrong:**
Adding file-based persistence introduces synchronous filesystem calls (`std::fs::write`, `serde_json::to_writer`) into async task contexts. This blocks a tokio worker thread, stalling all other tasks on that thread. In a system with ~10 concurrent async tasks (3 venue feeds, fan-out, spread engine, pricing engine, signal engine, paper tracker, lifecycle manager, health server), blocking even one worker thread for 10ms can cause cascading latency spikes and channel backpressure.

The existing `TradeLogger` in `paper_trade/tracker.rs` already does synchronous file I/O (`std::fs`, `std::io::Write`) inside the async `PaperTradeTracker::run()` method. This has been acceptable because writes are infrequent and fast, but adding heavier persistence (checkpoint files, state recovery reads) amplifies the problem.

**Why it happens:**
Rust's type system does not distinguish "blocking" from "non-blocking" at the async boundary. A developer adds `std::fs::write()` inside an `async fn` and the compiler is happy. The performance impact only shows under load. Tokio's documentation explicitly warns against this but it is easy to forget.

**How to avoid:**
- For writes: use `tokio::task::spawn_blocking()` for any file I/O that might exceed 1ms. This moves the work to a dedicated blocking thread pool. OR use `tokio::fs` which wraps operations in `spawn_blocking` internally.
- For the checkpoint pattern: serialize to a `Vec<u8>` in the async context (CPU work, fast), then pass the bytes to `spawn_blocking` for the actual file write + rename.
- For reads on startup: perform all file reads before entering the main `tokio::select!` loop, or use `spawn_blocking`.
- Do NOT wrap the existing `TradeLogger` in `spawn_blocking` per-write -- the overhead of thread handoff is worse than the occasional 0.1ms write. Instead, keep the existing `BufWriter` with periodic flush, but move the periodic flush to `spawn_blocking`.
- Monitor: add a `tokio::runtime::metrics` check for worker thread blocking time if using the `tokio_unstable` feature flag. At minimum, log if any checkpoint write exceeds 5ms.

**Warning signs:**
- Spread computation latency increases from <1ms to 10-50ms sporadically (correlates with checkpoint writes).
- Channel buffer utilization spikes (visible via `metrics::gauge!("paper_trades_open")` and similar).
- `try_send` failures increase on the secondary engine channels (pricing, signal fan-out) because fan-out is stalled waiting for a blocked spread engine channel.

**Phase to address:**
Phase 4 (File-Based Persistence) -- but must be considered in Phase 1 if settlement outcomes are persisted.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Store all state in a single JSON file | Simple implementation, single read/write | Grows unbounded, slow to parse at scale, all-or-nothing corruption risk | Never for growing data (signals, trades). OK for small fixed-size config-like state |
| Use `Utc::now()` for settlement time comparison | Simple, no extra data needed | Breaks deterministic replay; settlement logic cannot be tested with historical data | Only in live-mode code paths; replay must use event timestamps |
| Skip fsync after atomic write | Faster writes | State loss on power failure (OS crash, not process crash) | Acceptable for paper trading (data is recoverable from logs). Unacceptable for real trading |
| Hardcode venue polling intervals | Quick implementation | Different venues have different rate limits and data freshness. Deribit settlement data is available faster than Polymarket resolution | Never -- use per-venue config (already established pattern in `DiscoveryConfig`) |
| Alert via log messages only | No external dependencies | Logs must be actively monitored; silent failures go unnoticed if nobody watches | Acceptable for v1.1 solo trader. Must evolve to push notifications before v2 |
| Compute signal analysis at query time over raw logs | No pre-computation needed | O(n) over all historical trades per query; becomes unusable after weeks of data | Only for initial implementation. Must add incremental rollup within 2-4 weeks |

## Integration Gotchas

Common mistakes when connecting new features to the existing system.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Settlement tracker + EventRegistry | Querying settlement for instruments that have already been rolled (expired in registry, replaced by new expiry) | Track settlement by **original instrument ID at signal time**, not current registry state. Store instrument_id in `PaperPosition` (already done: `event_id` is stored, but need the specific venue instrument IDs too) |
| Signal analysis + SpreadEngine | Reading from the spread JSONL log which contains ALL computations (both above and below threshold) and counting them as "signals" | Only analyze trades that entered `PaperPosition` with `PositionStatus::Open` or `Settled`. The spread log is for debugging, not analysis |
| File persistence + PaperTradeTracker | Adding persistence inside the `tokio::select!` loop, making every tick slower | Checkpoint on a timer (every 60s) or on day boundary, not on every snapshot/signal |
| Failure alerting + VenueHealth | Duplicating the existing `VenueHealth` state tracking instead of extending it | Add new fields/methods to `VenueHealth` (e.g., `last_spread_at`, `computation_rate`) rather than building a parallel monitoring system |
| Settlement tracker + BasisRiskCache | Attempting to read settlement data from the risk cache, which only stores risk scores not settlement outcomes | Settlement outcomes are new data -- they need their own storage. The risk cache provides context (what was the expected risk) but not outcomes |
| Signal analysis + existing DailyAggregator | Trying to add hit rate/edge metrics to DailyAggregator, which tracks P&L not signal accuracy | Create a separate `SignalAnalyzer` that consumes settled positions and computes signal-quality metrics. DailyAggregator stays focused on P&L |
| File persistence + graceful shutdown | Writing state on `cancel.cancelled()` but the state is already partially consumed -- channels are closed, final trades not included | Flush state BEFORE dropping channel receivers. The existing shutdown order in `PaperTradeTracker::run()` does this correctly (emits daily summary, flushes logger). Persistence must happen in the same shutdown block |

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Unbounded `Vec<PaperPosition>` in `self.open` | Memory grows linearly with trade count; iteration for MTM update slows linearly | Cap open positions or evict stale ones; use `HashMap<event_id, Vec<Position>>` for O(1) lookup by event | After ~1000 open positions (unlikely in paper trading, but possible if settlement tracker never settles them) |
| Unbounded `mtm_history: Vec<MtmSnapshot>` per position | Each snapshot adds ~48 bytes per open position per tick. 3 venues at 1 snapshot/sec = ~150 entries/min/position | Cap MTM history length (keep last N or downsample to 1/minute) | After 24 hours: ~8640 entries * 48 bytes * N positions |
| Full state serialization on every checkpoint | Checkpoint time grows linearly with accumulated state | Use incremental checkpointing: only write changed positions since last checkpoint. Or use append-only JSONL for incremental writes + periodic full checkpoint | After ~10,000 historical trades in the state file |
| `EventRegistry.read().await` in forward_snapshots hot path | Already present in pipeline.rs line ~343. RwLock contention increases if settlement tracker also reads the registry frequently | Settlement tracker should cache the mappings it needs rather than reading the registry on every poll. The existing `BasisRiskCache` pattern is the model to follow | Only if settlement polling is frequent (>1/sec), which it should not be |

## Security Mistakes

Domain-specific security issues relevant to this system.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Storing Kalshi RSA private key in the persistence state file | Key exposure if state file is leaked or committed to git | Never include credentials in persisted state. The existing `Credentials` struct loads from env vars -- maintain this separation |
| Logging settlement API responses with auth tokens | Token exposure in JSONL logs and tracing output | Redact auth headers before logging. The existing Kalshi auth already handles JWT generation per-request, but settlement polling must strip tokens from error messages |
| Persisting paper trade state with real instrument IDs to a shared location | Reveals trading strategy and targeted instruments | Keep state files in a gitignored directory. The existing `recordings/` pattern is already gitignored |
| Settlement outcome polling without rate limiting | API ban from Deribit (20 req/s limit) or Kalshi | Reuse the existing `VenueRateLimiter` infrastructure for settlement API calls. Poll at most once per minute per expired instrument |

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Settlement tracking:** Often missing the "disputed/ambiguous" outcome state -- verify the tracker handles cases where Polymarket and the actual outcome disagree (UMA dispute process)
- [ ] **Settlement tracking:** Often missing instruments that expired while the system was offline -- verify the tracker discovers and backfills missed settlements on startup
- [ ] **Signal analysis:** Often missing the denominator -- verify hit rate reports total filled trades, not just settled ones, and explicitly reports how many are still pending settlement
- [ ] **Signal analysis:** Often missing cost-adjusted P&L -- verify the edge calculation includes adverse selection, fees, and carry, not just raw spread at settlement
- [ ] **Failure alerting:** Often missing the "everything looks fine but output is wrong" case -- verify at least one end-to-end check (e.g., "time since last threshold-passing signal evaluation" not just "time since last feed message")
- [ ] **File persistence:** Often missing the "startup recovery" path -- verify the system loads persisted state on restart AND validates its consistency (e.g., no duplicate trade IDs, no positions with status transitions that skip states)
- [ ] **File persistence:** Often missing the "state migration" story -- verify that adding new fields to `PaperPosition` or `DailyRollup` still parses old state files (use `#[serde(default)]` on all new fields)
- [ ] **File persistence:** Often missing Windows-specific atomic rename -- verify `rename()` works when target file exists on Windows (it does not by default; need `remove_file` first or use `tempfile` crate)

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Corrupted state file on crash | LOW | Replay from JSONL trade logs to reconstruct positions and P&L. The trade log (already written by `TradeLogger`) is append-only and much more resilient than a checkpoint file. This is why the JSONL log is the source of truth, not the checkpoint |
| Wrong settlement outcomes recorded | MEDIUM | Add a "recompute settlement" CLI command that re-polls venue APIs and overwrites previous outcomes. Settlement outcomes should be overridable manually (TOML or JSON override file) for disputed cases |
| Blocking I/O stalls pipeline | LOW | Move to `spawn_blocking` or `tokio::fs`. No data loss, just performance degradation during the fix |
| Alert fatigue from noisy alerts | LOW | Add progressive throttling: first occurrence logs at WARN, subsequent repeats within a cooldown window log at DEBUG. Only re-escalate to WARN when the condition clears and recurs |
| Missing settlements for offline period | MEDIUM | On startup, scan all `PaperPosition` with `PositionStatus::Open` and check if their event's expiry has passed. If so, queue them for settlement outcome resolution. This requires persisting the `expiry` date in each position |
| Signal analysis shows misleading hit rate | LOW | Always report alongside: fill rate, adverse selection distribution, and a "theoretical vs actual" comparison. If hit rate and fill rate diverge significantly, the analysis methodology is suspect |

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Settlement data harder than expected (Pitfall 1) | Phase 1: Settlement Tracking | Unit test each venue adapter with mocked API responses. Integration test with recorded real settlement data from at least one expired instrument per venue |
| Wrong timing windows for analysis (Pitfall 2) | Phase 2: Signal Analysis | Test with a known synthetic trade: signal at t=0, fill at t=1 with known adverse selection, settlement at t=100 with known outcome. Verify all metrics match hand-calculated values |
| File corruption on crash (Pitfall 3) | Phase 4: File Persistence | Kill-test: run system, `kill -9` mid-operation, restart, verify state recovery. Run this on both Linux and Windows |
| Alerting monitors wrong thing (Pitfall 4) | Phase 3: Failure Alerting | Simulate silent failure: disconnect a venue at the network level (not via cancel token), verify alert fires within configured timeout. Simulate config drift: remove all event mappings, verify alert fires for "no spread computations" |
| Blocking tokio runtime (Pitfall 5) | Phase 4: File Persistence | Add timing instrumentation to checkpoint writes. Verify 99th percentile checkpoint time is under 5ms. Run concurrent load test with all 3 venues during checkpoint |
| Unbounded MTM history growth | Phase 4: File Persistence | After 24 hours of paper trading, verify memory usage is stable (not linearly growing). Cap MTM history or downsample |
| Settlement outcome for rolled instruments | Phase 1: Settlement Tracking | Test scenario: instrument expires, lifecycle manager rolls it, settlement tracker still resolves the expired instrument's outcome |
| Startup state recovery | Phase 4: File Persistence | Test scenario: accumulate 50 trades, kill process, restart, verify all 50 trades are present with correct P&L |

## Sources

- Deribit API settlement documentation: https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement
- Deribit settlement price TWAP methodology: https://docs.deribit.com/
- Polymarket resolution process: https://docs.polymarket.com/polymarket-learn/markets/how-are-markets-resolved
- Polymarket CLOB API gap for resolved markets: https://github.com/Polymarket/py-clob-client/issues/117
- Polymarket price history limitation for resolved markets: https://github.com/Polymarket/py-clob-client/issues/216
- Kalshi settlement API: https://docs.kalshi.com/fix/market-settlement
- Kalshi market result endpoint: https://docs.kalshi.com/api-reference/market/get-market
- Kalshi portfolio settlements: https://docs.kalshi.com/api-reference/portfolio/get-settlements
- Tokio async filesystem operations: https://docs.rs/tokio/latest/tokio/fs/index.html
- Silent failure detection patterns: https://www.vincentlakatos.com/blog/building-a-monitoring-system-that-catches-silent-failures/
- Prediction market settlement disputes: https://defirate.com/prediction-markets/how-contracts-settle/
- UMA dispute resolution for prediction markets: https://blog.uma.xyz/articles/what-is-a-prediction-market-dispute
- Atomic file write pattern in Rust: https://users.rust-lang.org/t/mvdb-atomic-easy-to-use-file-backed-storage-using-serde/12219
- Codebase analysis: existing `atomic_write()` in `events/lifecycle.rs`, `TradeLogger` in `paper_trade/tracker.rs`, `VenueHealth` in `feed/health.rs`, pipeline fan-out in `main.rs`

---
*Pitfalls research for: v1.1 Paper Trading Validation milestone*
*Researched: 2026-02-24*
