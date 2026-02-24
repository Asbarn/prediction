# Project Research Summary

**Project:** v1.1 Paper Trading Validation
**Domain:** Settlement outcome tracking, signal analysis, failure alerting, file-based state persistence — built atop an existing 22,751 LOC async Rust cross-venue prediction market arbitrage system
**Researched:** 2026-02-24
**Confidence:** HIGH

## Executive Summary

The v1.1 milestone adds four capabilities to an already-working paper trading system: settlement outcome tracking (so signal predictions can be verified against actual event resolutions), signal analysis tooling (hit rate, edge measurement, false positive rate, time-to-convergence), failure alerting (detecting silent degradation before it corrupts the validation data), and file-based state persistence (so multi-week paper trading sessions survive restarts). The central goal is answering a single question with statistical confidence: "Are the cross-venue arbitrage signals generating real alpha, or are they artifacts of threshold misconfiguration and structural basis?"

The recommended approach is additive extension with zero new dependencies. Every capability the four features require is already present in the existing dependency tree: `reqwest` handles settlement API polling and webhook alerting, `serde_json` handles state persistence, `tokio::time::interval` handles periodic tasks, and the existing JSONL logging infrastructure provides the event-log substrate for analysis. This is a deliberate architectural constraint — the system is a solo-operator single-binary Linux service and the < 200KB total state volume makes a database unjustifiable. The build order is critical: settlement outcome tracking is the hard dependency that must exist before any signal analysis is meaningful. Failure alerting and file persistence can be built in parallel as independent tracks.

The key risks are: (1) settlement APIs are more heterogeneous than expected — Polymarket has no clean resolution endpoint and Deribit settlement prices must be compared against strikes to derive binary outcomes; (2) hit rate measurement is trivially wrong if timing windows, fill-vs-signal timing, and cost-adjusted P&L are not designed correctly from the start; (3) file persistence on Windows has non-atomic `rename()` semantics that differ from Linux POSIX behavior; and (4) alerting that monitors only connectivity misses the most dangerous class of failures — silent degradation where the system is connected but producing stale or missing output. All four risks have concrete mitigations already documented in the research.

---

## Key Findings

### Recommended Stack

No new crate dependencies are required for v1.1. This is the highest-confidence finding from STACK.md, supported by feature-by-feature analysis. The existing dependency set (`tokio`, `reqwest`, `serde_json`, `chrono`, `rust_decimal`, `statrs`, `tracing`, `metrics`, `axum`, `clap`, `uuid`, `thiserror`, `anyhow`) covers every need. Crates that were evaluated and explicitly rejected include: `sled` (abandoned 2022, data corruption issues), `bincode` (RUSTSEC-2025-0141, v3 does not compile), `rusqlite` (massive overkill for <200KB state), `lettre` (SMTP complexity vs webhook simplicity), `tokio-cron-scheduler` (overkill over `tokio::time::interval`), `tempfile` (three lines of `std::fs` suffice for the atomic write pattern), and any ORM crate.

**Core technologies for v1.1 work:**
- `reqwest 0.12`: Settlement API polling (Deribit REST, Kalshi REST, Polymarket Gamma API) and webhook alerting — already in tree
- `serde + serde_json`: State checkpoint serialization and JSONL signal/trade log reading — already in tree
- `tokio::time::interval`: Periodic polling loops for settlement tracker, alert monitor, checkpoint writer — already in tree
- `std::fs::write` + `std::fs::rename`: Atomic state file writes (write-to-tmp-then-rename, same pattern as existing `ContractLifecycleManager::atomic_write()`) — stdlib, no new crate
- `statrs 0.18`: Available for Sharpe ratio computation; most signal metrics (hit rate, false positive rate) are simple ratios requiring no statistical library — already in tree
- `clap 4.5`: Analysis CLI subcommand for running post-hoc reports — already in tree

### Expected Features

Full details in `.planning/research/FEATURES.md`.

**Must have (table stakes — all four are required to answer "are signals real?"):**
- Settlement outcome tracking per venue (Deribit delivery price vs strike, Kalshi `result` field, Polymarket Gamma API resolution) — without this, hit rate is impossible
- Signal analysis core metrics: hit rate, average edge (cost-adjusted), false positive rate, time-to-convergence — the analytical payoff of the whole milestone
- File-based state persistence with atomic writes and startup recovery — prevents losing weeks of paper trade data on restart
- Failure alerting with liveness checks (feed silence, partial coverage, no-signal detection) — without this, operator cannot trust data integrity during the validation period

**Should have (differentiators, build if time allows):**
- Settlement prediction scheduling (pre-schedule API polls based on known expiry dates from `EventMapping`, avoiding unnecessary polling)
- Threshold effectiveness analysis: compare `ThresholdStatus::PassedBoth` vs `PassedStaticOnly` vs `Filtered` against final settlement outcomes — directly answers "should I tighten/loosen thresholds?"
- Pattern-specific performance breakdown by `SpreadPattern` and `ArbDirection`
- Cross-venue settlement discrepancy detection (flags when Polymarket and Kalshi resolve the same underlying event differently)

**Defer to v2+ (anti-features for v1.1):**
- Full database (SQLite/PostgreSQL) — unjustifiable for <200KB of state
- Real-time dashboarding of signal analytics — statistics require settlement data that arrives hours/days later; real-time display of incomplete stats is misleading
- Automated threshold adjustment — premature optimization on sparse data will oscillate; surface metrics clearly and let operator adjust TOML manually
- Email/SMS/PagerDuty integration — emit to Prometheus and let Alertmanager handle routing if needed
- Historical data backfill from venue APIs — separate data engineering task; v1.1 accumulates data going forward

### Architecture Approach

The architecture is strictly additive: four new tokio tasks plugged into the existing pipeline as consumers of existing data, with no changes to the hot path (fan-out, SpreadEngine, PricingEngine, CrossAssetEngine). The existing patterns — tokio tasks with `CancellationToken`, bounded `mpsc` channels (1024), `tokio::select! biased`, JSONL daily-rotation logging with `BufWriter`, `Arc<RwLock<T>>` for shared state with `try_read` on hot paths, and `#[serde(default)]` config structs — are all reused verbatim. The architecture research is based on direct analysis of 22,751 LOC of existing source code, giving this section uniquely high confidence.

**Four new modules, all additive:**

1. `src/settlement/` (`SettlementTracker`) — Periodic tokio task; watches `EventRegistry` for expired events; polls per-venue REST APIs via `reqwest`; emits `SettlementOutcome` via mpsc channel to `SignalAnalyzer` and `PaperTradeTracker`; writes settlement JSONL. Build order: second (after alerting scaffold, before analysis).

2. `src/analysis/` (`SignalAnalyzer`) — Hybrid online/batch accumulator; receives `ArbSignal` from a new fan-out tap on the existing arb_signal channel and `SettlementOutcome` from settlement tracker; computes hit rate, edge accuracy, false positive rate, time-to-convergence; writes analysis JSONL and Prometheus gauges. Build order: third (depends on settlement outcomes).

3. `src/alert/` (`AlertMonitor`) — Periodic sweep (every 30s); reads `VenueHealth` atomics (non-blocking); checks feed silence, partial coverage, no-signal gap; emits `tracing::warn!`, Prometheus metrics, shares `Arc<RwLock<HashMap<AlertKey, ActiveAlert>>>` with health endpoint via `try_read`. Build order: first (no new dependencies).

4. `src/persistence/` (`StatePersistence`) — Checkpoint-based (not WAL); periodic serialize of `PaperTradeTracker::snapshot()` and `SignalAnalyzer::snapshot()` to `state/checkpoint.json` via atomic write-then-rename; startup recovery loads checkpoint then replays JSONL events after checkpoint timestamp. Build order: fourth (needs stable snapshot APIs from other components).

**Modifications to existing modules:**
- `src/paper_trade/tracker.rs`: Add `snapshot()`/`restore()` methods; wire `SettlementOutcome` channel for position settlement
- `src/paper_trade/aggregator.rs`: Add `Deserialize` derive to `DailyRollup` (currently only `Serialize`)
- `src/health/mod.rs`: Add `alerts: Vec<AlertSummary>` field to `HealthResponse`
- `src/config/system.rs`: Add `[settlement]`, `[alerting]`, `[persistence]` config sections with `#[serde(default)]`

### Critical Pitfalls

Full details in `.planning/research/PITFALLS.md`.

1. **Settlement data is heterogeneous across venues** — Deribit requires delivery-price-vs-strike comparison (not a raw yes/no field); Polymarket has no dedicated resolution endpoint as of early 2025 (must infer from `closed: true` Gamma API flag, or lock to 0/1 price); Kalshi requires RSA JWT auth for the `result` field. Mitigation: per-venue adapter with normalized `SettlementOutcome` type; poll with exponential backoff starting expiry+5 min; implement `SettlementStatus::Pending` for outcomes not yet available; handle off-line-period backfill on startup by scanning open positions with expired events.

2. **Hit rate measurement is trivially wrong without careful timing design** — must use fill price (not signal price), must compute cost-adjusted P&L (including adverse selection, fees, carry), must report fill rate and hit rate separately, must gate on `PositionStatus::Settled` not `PositionStatus::Open`. Survivorship bias: unfilled signals (typically in fast-moving markets) must be counted in the denominator. Mitigation: define all metrics precisely before implementation; test with a synthetic known-outcome trade.

3. **File persistence corruption on crash** — Standard `File::create()` + `serde_json::to_writer()` is not atomic; crash between open and flush leaves truncated JSON. Windows `rename()` fails if target exists (unlike POSIX). Mitigation: always use write-to-tmp-then-rename (already exists as `ContractLifecycleManager::atomic_write()`); on Windows, `remove_file` before `rename`; keep JSONL trade logs as source of truth for replay recovery.

4. **Alerting that monitors connectivity instead of output liveness** — The most dangerous failures are silent: feed connected but not streaming, config drift causing zero event mappings, all thresholds too tight producing no signals. Mitigation: add dead-man's-switch liveness timestamps per pipeline stage (`last_spread_computed_at`, `last_signal_evaluated_at`); alert on absence of expected events, not just presence of errors.

5. **Blocking tokio runtime with synchronous file I/O** — `std::fs::write()` inside an `async fn` blocks a tokio worker thread, stalling all other tasks sharing that thread. Mitigation: use `tokio::task::spawn_blocking()` or `tokio::fs` for checkpoint writes; serialize to `Vec<u8>` in async context, then hand bytes to `spawn_blocking` for the actual write+rename; never put file I/O in the message-processing path of the `tokio::select!` loop.

---

## Implications for Roadmap

Based on combined research, the natural phase structure is dependency-driven with two parallel tracks in Phase 1. All research files converge on the same build order.

### Phase 1A: Failure Alerting
**Rationale:** No dependencies on any other new v1.1 component. Reads existing `VenueHealth` atomics. Delivers immediate operational value for unattended paper trading runs. Simplest of the four features. Should be built first so it is monitoring throughout the entire v1.1 build process.
**Delivers:** `AlertMonitor` task, alert deduplication with cooldown, `tracing::warn!` + Prometheus alert metrics, health endpoint extension with active alert summary, webhook POST via `reqwest` for operator notifications.
**Addresses:** TS-3 (Failure Alerting) from FEATURES.md
**Avoids:** Pitfall 4 (monitor output liveness, not just connectivity) — specifically implement dead-man's-switch checks at each pipeline stage, not just `VenueHealth.is_available()`
**Research flag:** Standard patterns — follows existing `ContractLifecycleManager` interval task pattern exactly. Skip `/gsd:research-phase`.

### Phase 1B: File-Based State Persistence (parallel with 1A)
**Rationale:** No dependencies on settlement tracking or signal analysis. Depends only on existing `PaperPosition` (already `Serialize + Deserialize`) and the `atomic_write()` pattern already in `ContractLifecycleManager`. Enables the multi-week paper trading sessions that v1.1 requires without data loss on restart.
**Delivers:** `StatePersistence` module with checkpoint writer and startup recovery, `PaperTradeTracker::snapshot()`/`restore()` methods, `DailyRollup: Deserialize`, atomic JSON checkpoint files in `state/` directory, JSONL event replay for recovery after checkpoint timestamp.
**Addresses:** TS-4 (File-Based Persistence) from FEATURES.md
**Avoids:** Pitfall 3 (file corruption) and Pitfall 5 (blocking tokio runtime) — use `tokio::task::spawn_blocking` for checkpoint writes; design startup recovery path before any other code touches `PaperTradeTracker::new()`
**Research flag:** Standard patterns — write-then-rename is established in codebase, `serde` is already complete. Windows-specific `rename()` behavior needs a kill-test verification. Skip `/gsd:research-phase` but include kill-test in acceptance criteria.

### Phase 2: Settlement Outcome Tracking
**Rationale:** The critical path item. Every downstream analysis feature (hit rate, edge measurement, threshold effectiveness) is mathematically impossible without knowing how events actually resolved. Must be built after alerting (so degradation is visible during integration) and persistence (so settlement outcomes are durable). This is also the most technically uncertain phase due to heterogeneous venue APIs.
**Delivers:** `SettlementTracker` task with per-venue adapters (Deribit delivery price, Kalshi result field, Polymarket Gamma API), `SettlementOutcome` mpsc channel to downstream consumers, settlement outcomes JSONL, `PositionStatus::Settled` transitions on `PaperTradeTracker`, `SettlementStatus::Pending` for outcomes not yet available, backfill on startup for events that expired while system was offline.
**Addresses:** TS-1 (Settlement Outcome Tracking) and differentiator D-1 (Settlement Timing Intelligence) from FEATURES.md
**Avoids:** Pitfall 1 (venue API heterogeneity) — implement per-venue adapter trait, poll with exponential backoff from expiry+5min, handle Polymarket's lack of a clean resolution endpoint via `closed` flag + price-locks-to-0-or-1 fallback
**Research flag:** NEEDS `/gsd:research-phase` during planning. Venue settlement APIs have gaps and ambiguities (especially Polymarket). Integration test with a real expired instrument from each venue before considering phase complete.

### Phase 3: Signal Analysis Tooling
**Rationale:** The analytical payoff of v1.1. Depends on Phase 2 (settlement outcomes must flow in before hit rate is meaningful). The online accumulation infrastructure (counters, grouping by event/direction/pattern) can be scaffolded early, but metrics only become meaningful once real settlement data arrives. This phase transforms raw data into the answer: "Are signals real?"
**Delivers:** `SignalAnalyzer` task, `SignalAnalysisReport` with hit rate, cost-adjusted average edge, false positive rate, time-to-convergence, per-event breakdown, per-direction breakdown, `ArbSignal` fan-out tap from existing arb_signal channel, periodic structured log reports, Prometheus gauges for rolling metrics, threshold effectiveness analysis (correlate `ThresholdStatus` with settlement outcomes), pattern-specific performance by `SpreadPattern` and `ArbDirection`.
**Addresses:** TS-2 (Signal Analysis Tooling) and differentiators D-2 (Advanced Signal Analytics) from FEATURES.md
**Avoids:** Pitfall 2 (wrong timing windows) — define all metric denominators precisely: hit rate = `filled_and_profitable_at_settlement / total_filled`; report fill rate and signal accuracy separately; include adverse selection in P&L; gate all rate computations on `PositionStatus::Settled`
**Research flag:** Standard patterns for metric accumulation. The analysis methodology (timing windows, cost-adjusted P&L) needs careful specification in the phase plan. Skip `/gsd:research-phase` but require precision spec for each metric formula before implementation begins.

### Phase Ordering Rationale

- **Alerting first** because the build process itself benefits from monitoring. If settlement API integration breaks something subtle (e.g., an unexpected API error silently starves the settlement tracker channel), alerting will surface it during development.
- **Persistence parallel to alerting** because they are fully independent and both are prerequisites for meaningful multi-week paper trading. Persistence needs `PaperTradeTracker` modifications that can be done before settlement integration.
- **Settlement before analysis** because the dependency is absolute — signal analysis without settlement outcomes produces nonsense. There is no way to defer this ordering.
- **Analysis last** because it requires both settlement data flowing AND stable interfaces from all three upstream components. It is also the highest-value deliverable, so arriving at it via a stable foundation is correct.

### Research Flags

Phases needing deeper research during planning:
- **Phase 2 (Settlement Outcome Tracking):** Venue API heterogeneity is documented but integration details need hands-on verification. Specifically: Polymarket resolution detection (no clean endpoint), Deribit settlement history instrument ID behavior post-expiry (instruments are delisted), Kalshi auth requirements for settlement data. Recommend integration test with one real expired instrument per venue before Phase 2 is considered complete.

Phases with standard patterns (skip `/gsd:research-phase`):
- **Phase 1A (Failure Alerting):** Follows `ContractLifecycleManager` pattern exactly. VenueHealth atomics are already the right substrate. Only design decision is the dead-man's-switch liveness check implementation.
- **Phase 1B (File-Based Persistence):** Atomic write pattern already exists in codebase. The kill-test verification is an acceptance criterion, not a research question.
- **Phase 3 (Signal Analysis):** Signal quality metrics are standard quant domain knowledge. Implementation is arithmetic on JSONL data. The research gap is methodology specification (what exactly counts in each denominator), not technology.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new dependencies; every claim verified against existing `Cargo.toml`. Rejected crates evaluated explicitly (sled, bincode, rusqlite, lettre). Rust 2024 edition (1.85+) already specified — no version concerns. |
| Features | HIGH (table stakes) / MEDIUM (differentiators) | Table stakes features are directly grounded in existing code (PaperPosition, DailyRollup, VenueHealth are already the substrate). Differentiator value estimates are based on quant research literature with MEDIUM confidence. |
| Architecture | HIGH | Based on direct source code analysis of 22,751 LOC, not inference. Every integration point is traced to specific files and line numbers. The "extend, don't restructure" principle is confirmed by codebase structure. |
| Pitfalls | HIGH (integration, architecture) / MEDIUM (venue API behavior) | Integration pitfalls are based on direct codebase analysis (e.g., existing `atomic_write()` pattern, Windows rename semantics, tokio blocking behavior). Venue API pitfalls are based on official documentation plus GitHub issues — real but may evolve. |

**Overall confidence:** HIGH

### Gaps to Address

- **Polymarket resolution detection:** No dedicated REST endpoint for querying market resolution results as of early 2025. The recommended approach (`closed: true` + price-lock-to-0-or-1 fallback) is an inference from documented behavior, not an officially documented resolution query pattern. Address during Phase 2 with a hands-on integration test against a known-resolved Polymarket market.

- **Deribit post-expiry instrument behavior:** After an options instrument expires, it may be delisted from the active instrument list. The settlement tracker must query by instrument_id (stored at signal time) rather than looking up current instruments. This is documented as a pitfall but the exact API behavior needs verification during Phase 2 integration.

- **Windows kill-test for atomic writes:** The system targets Linux as primary deployment, but development occurs on Windows (evident from file paths in codebase). The `std::fs::rename` POSIX atomicity guarantee does not apply on Windows when the target file exists. The `ContractLifecycleManager::atomic_write()` implementation should be inspected for Windows compatibility before Phase 1B acceptance.

- **MTM history memory growth bound:** `MtmSnapshot` history accumulates in memory per open position at ~150 entries/min/position (3 venues, 1 snapshot/sec). Over 24 hours this is ~8,640 entries per position. The PITFALLS.md recommends capping or downsampling. The exact cap or downsample policy needs a decision during Phase 1B (when persistence design is locked in) since checkpoint size depends on it.

- **Signal analysis methodology precision:** Hit rate, false positive rate, and time-to-convergence each have multiple valid definitions. The specific denominator choices (filled trades vs all signals, settled vs all filled) need to be locked in specification before Phase 3 implementation starts, since retroactive correction requires reprocessing all historical data.

---

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis, `D:/Programming/Rust/prediction/src/` (22,751 LOC) — architecture patterns, integration points, existing data types
- [Deribit Settlement Documentation](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) — TWAP delivery price methodology, 08:00 UTC expiry
- [Deribit API: get_delivery_prices](https://docs.deribit.com/) — public endpoint, no auth required
- [Kalshi API: Get Market](https://docs.kalshi.com/api-reference/market/get-market) — `result` and `status` fields
- [Kalshi API: Get Settlements](https://docs.kalshi.com/api-reference/portfolio/get-settlements) — portfolio settlement history
- [Tokio async filesystem docs](https://docs.rs/tokio/latest/tokio/fs/index.html) — `spawn_blocking` for file I/O

### Secondary (MEDIUM confidence)
- [Polymarket: How Markets Resolve](https://docs.polymarket.com/polymarket-learn/markets/how-are-markets-resolved) — UMA Optimistic Oracle resolution process, 2-hour challenge window
- [Polymarket Gamma API](https://docs.polymarket.com/developers/gamma-markets-api/gamma-structure) — `closed`, `active` fields for resolution inference
- [Signal quality metrics — Macrosynergy](https://macrosynergy.com/research/how-to-measure-the-quality-of-a-trading-signal/) — hit rate, Sharpe, false positive rate definitions
- [Silent failure detection](https://www.vincentlakatos.com/blog/building-a-monitoring-system-that-catches-silent-failures/) — dead-man's-switch monitoring patterns
- [Alpha decay research — MicroAlphas](https://microalphas.com/signal-decay-patterns/) — 60% signal decay in first periods (context for time-to-convergence interpretation)

### Tertiary (LOW confidence — needs validation)
- [Polymarket py-clob-client GitHub issue #117](https://github.com/Polymarket/py-clob-client/issues/117) — confirms no dedicated resolution query endpoint as of 2025; API may have evolved
- [Polymarket py-clob-client GitHub issue #216](https://github.com/Polymarket/py-clob-client/issues/216) — price history limitation for resolved markets; indirectly confirms resolution API gap

---
*Research completed: 2026-02-24*
*Ready for roadmap: yes*
