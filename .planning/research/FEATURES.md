# Feature Landscape: v1.1 Paper Trading Validation

**Domain:** Settlement outcome tracking, signal analysis tooling, failure alerting, file-based state persistence
**Researched:** 2026-02-24
**Confidence:** HIGH (settlement tracking, persistence) / MEDIUM (signal analysis metrics, failure alerting patterns)

**Scope note:** This research covers ONLY the four new features for v1.1. Existing v1.0 features (feeds, pricing, spread engines, paper trading) are already built and validated. Dependencies on existing code are identified throughout.

---

## Table Stakes

Features that a paper trading validation system must have. Without these, the system cannot answer "are my signals real?" -- the entire goal of v1.1.

### TS-1: Settlement Outcome Tracking

The system generates signals about whether an event will resolve YES or NO. To measure if those signals are correct, it must know *how each event actually resolved*. Without settlement tracking, hit rate and P&L calculations are impossible.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Deribit delivery price ingestion | Options settle at 30-min TWAP delivery price (07:30-08:00 UTC). Must fetch this post-expiry to compute whether options-implied signals were correct. | LOW | Existing `reqwest::Client` in `ContractLifecycleManager`. Deribit `public/get_delivery_prices` REST endpoint (no auth required). Existing `EventRegistry` tracks expiry dates. |
| Kalshi market result polling | Kalshi markets resolve with explicit `result: "yes"/"no"` via `GetMarket` REST endpoint. Must poll resolved markets to know binary outcome. | LOW | Existing Kalshi auth (`kalshi::auth`) and REST infrastructure. `GetMarket` endpoint returns `result` and `status` fields. Existing lifecycle manager already polls Kalshi REST. |
| Polymarket resolution status polling | Polymarket markets resolve via UMA Optimistic Oracle. Gamma API returns `closed: true` plus resolution data. Must poll to know binary outcome. | LOW | Existing Polymarket Gamma API client in `discovery::discover_polymarket`. Gamma API `closed` and resolution fields on market response. |
| Settlement outcome storage per event | Store the actual outcome (YES/NO/UNRESOLVED) and settlement prices per event_id, linked to existing paper trade positions for P&L computation. | LOW | Existing `PaperPosition` has `settle()` method and `PositionStatus::Settled` state. Existing `TradeEvent::Settlement` variant in JSONL schema. Existing `EventMapping` with expiry dates. |
| Automated signal-vs-outcome comparison | When an event settles, automatically compute realized P&L for all paper positions linked to that event, comparing signal direction against actual outcome. | MEDIUM | Existing `PaperTradeTracker` manages `open: Vec<PaperPosition>`. Existing `DailyAggregator` accepts `record_trade()`. New: must connect settlement poll results to position settlement. |

**Venue-specific settlement mechanics (critical domain knowledge):**

- **Deribit:** Options expire at 08:00 UTC. Delivery price = 30-min TWAP of Deribit Index (07:30-08:00 UTC), snapshot every 4 seconds. Cash-settled European-style. REST endpoint `public/get_delivery_prices` returns historical delivery prices by currency (no auth required). HIGH confidence -- verified against Deribit support docs.
- **Kalshi:** REST `GetMarket` endpoint returns `result` field with values `"yes"`, `"no"`, or null (unresolved). `status` field indicates lifecycle stage. Resolution typically completes within hours of event conclusion. Kalshi uses centralized staff oracle referencing official sources. MEDIUM confidence -- inferred from API docs search results.
- **Polymarket:** Gamma API `GET /markets` returns `closed` boolean and resolution data. UMA Optimistic Oracle resolves with 2-hour challenge window (or 48-hour if disputed). On-chain settlement: winning shares = $1, losing = $0. MEDIUM confidence -- verified against Polymarket docs and UMA documentation.

### TS-2: Signal Analysis Tooling

Paper trading is useless without statistical analysis of signal quality. The operator needs to know: "Are my signals profitable? How often are they right? How fast do they converge?"

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Hit rate computation (win/loss count) | The most fundamental metric: what fraction of signals were correct? Needs to be computed per-event, per-pattern, per-venue-pair, and overall. | LOW | Existing `DailyAggregator` already tracks `winning_trades` and `losing_trades`. Existing `PaperPosition` has `settlement_pnl`. New: aggregate across longer windows (weekly, lifetime). |
| Edge measurement (average realized P&L) | Average P&L per signal tells you if the edge is real and how large it is. Must separate gross edge from cost drag. | LOW | Existing `DailyRollup` has `avg_pnl` and `total_pnl`. Existing `PaperPosition` stores `signal_spread` vs actual outcome. New: decompose P&L into signal edge vs cost components. |
| False positive rate | Fraction of signals that appeared profitable at signal time but resulted in a loss at settlement. Critical for threshold tuning. | MEDIUM | Existing signals have `ThresholdStatus::PassedBoth` in `ArbSignal`. Existing positions track `signal_spread` and `settlement_pnl`. New: correlate threshold-passing signals with ultimate outcomes. |
| Time-to-convergence measurement | How long does a spread take to close after signal generation? Shorter = more reliable signal. Spreads that never converge are false signals from structural basis. | MEDIUM | Existing `MtmSnapshot` history tracks spread over time per position. New: analyze when unrealized P&L peaks and when spread crosses zero, measuring time from signal to convergence. |
| Adverse selection measurement | How much worse is the fill price vs the signal price? High adverse selection means signals are late -- the market already moved. | LOW | Existing `PaperPosition.adverse_selection` field already computed on fill. New: aggregate statistics (mean, median, p95) across all positions. |
| Signal confidence vs outcome correlation | Do high-confidence signals (from the pricing engine) actually perform better? If not, confidence scoring needs recalibration. | MEDIUM | Existing `ArbSignal.confidence` field (0.0-1.0). Existing `ConfidenceComponents` breakdown. New: bucket signals by confidence quartile and compare hit rates. |
| Per-event-type breakdown | Different events may have different signal characteristics. BTC $100k signals may be systematically different from $80k signals. | LOW | Existing `event_id` links signals to events. Existing `EventMapping` has `asset`, `strike`, `direction`. New: group statistics by these fields. |
| Periodic analysis report emission | Structured summary emitted at configurable intervals (daily, weekly) via tracing logs and optionally to a dedicated JSONL analysis file. | LOW | Existing `DailyAggregator.emit_daily_summary()` pattern. Existing daily JSONL rotation in `TradeLogger` and `SignalLogger`. New: richer report content, longer windows. |

**Standard signal quality metrics from quantitative trading (MEDIUM confidence, from research):**

| Metric | Definition | Interpretation |
|--------|-----------|----------------|
| Hit rate | winning_trades / total_trades | Above 50% for binary signals suggests real edge. Context-dependent -- a 40% hit rate with 3:1 reward/risk is fine. |
| Average edge (bps) | mean(realized_pnl) / notional | Positive = real alpha. Compare against total cost to ensure edge exceeds friction. |
| Sharpe ratio | mean(daily_pnl) / stddev(daily_pnl) * sqrt(252) | Above 1.0 suggests meaningful risk-adjusted returns. |
| False positive rate | false_signals / total_threshold_passing_signals | Below 30% is good; above 50% means thresholds need tightening. |
| Time-to-convergence | median time from signal to spread crossing zero | Minutes = healthy arb. Hours = structural basis or slow market. Never = false signal. |
| Adverse selection ratio | mean(adverse_selection) / mean(signal_spread) | Below 20% is good. Above 50% means signals fire too late. |
| Signal decay (half-life) | Time for signal edge to halve from initial value | From alpha decay research: initial signal deterioration typically shows 60% decay in first few periods. |

### TS-3: Failure Alerting

The system must detect and surface operational failures that silently degrade signal quality. A stale feed that is not detected produces phantom signals. A partial feed outage that is not alerted means the operator does not know the system is running impaired.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Stale data alerting (beyond reconnection) | Existing staleness detection rejects stale data from spread computation. But: the operator is not explicitly alerted when a feed becomes persistently stale. A venue could be "connected" (WebSocket open) but sending no updates. | LOW | Existing `VenueHealth.last_message_at()` timestamp. Existing `StalenessConfig.threshold_ms`. New: periodic check comparing last_message_at against configurable alert threshold. Emit tracing::warn and increment alert counter. |
| Partial feed detection | If Deribit is streaming but Polymarket is down, cross-asset signals still work but prediction-market-only spreads do not. The system should clearly surface which signal types are operational. | LOW | Existing `VenueHealth.is_available()` per venue. Existing `/health` endpoint reports per-feed status. New: derive operational signal-type status from feed availability matrix. |
| Silent failure detection (no-signal alerting) | If the system is receiving data from all feeds but producing zero signals for an extended period, something may be wrong (all events expired, threshold misconfiguration, bug). Absence of output is harder to detect than presence of errors. | MEDIUM | Existing `signal_count` and `filtered_count` in `CrossAssetEngine`. Existing Prometheus counter `arb_signals_emitted_total`. New: track time since last signal emission; alert if exceeding configurable threshold (e.g., 1 hour with zero signals while feeds are active). |
| Data quality degradation alerting | Beyond simple up/down: detect when feed quality degrades (increased gap between ticks, reduced book depth, higher staleness rate) without full disconnection. | MEDIUM | Existing `feed_latency_ms` histogram. Existing sequence numbers in `MarketSnapshot`. New: rolling statistics on inter-tick gaps, book depth, and staleness rejection rate. Alert when metrics deviate beyond configurable thresholds from baseline. |
| Configuration validation alerting | Detect configuration errors that would silently reduce system effectiveness: expired events still active, empty market lists, unreachable thresholds. | LOW | Existing `config::validation` module. Existing event lifecycle (expired status). New: runtime validation checks on config reload and periodic schedule. |
| Alert deduplication and cooldown | Prevent alert storms: if Polymarket is down, emit one alert, not one per second. Configurable cooldown per alert type. | LOW | New: simple in-memory cooldown tracker per alert key. Standard pattern -- HashMap of (alert_key -> last_alerted_at). |
| Prometheus alert metrics | All alerts emit corresponding Prometheus counters/gauges so Grafana/Alertmanager can build dashboards and escalation rules. | LOW | Existing `metrics::counter!` and `metrics::gauge!` pattern throughout codebase. New: dedicated alert metric names (e.g., `alert_stale_feed_total`, `alert_silent_failure_active`). |

**Key insight from research:** 41% of critical model degradations in production systems went undetected for over a week when relying only on traditional monitoring (error logs, up/down checks). Silent failures -- where the system appears operational but produces degraded output -- are the most dangerous category. The v1.1 failure alerting must specifically target this class of failure.

### TS-4: File-Based State Persistence

The existing paper trade tracker is in-memory only. If the system restarts, all open positions and accumulated statistics are lost. For a paper trading session spanning 2-4 weeks, this is unacceptable.

| Feature | Why Expected | Complexity | Dependencies on Existing Code |
|---------|--------------|------------|-------------------------------|
| Paper P&L state persistence (positions + aggregates) | Open positions, pending fills, and daily aggregates must survive restarts. Without this, a crash erases weeks of paper trading data. | MEDIUM | Existing `PaperPosition` derives `Serialize, Deserialize`. Existing `DailyRollup` derives `Serialize`. Existing `TradeLogger` writes JSONL (but append-only events, not restorable state). New: periodic state snapshot to JSON file with atomic writes. |
| Signal history persistence | Historical signal data (all emitted `ArbSignal`s with outcomes) must be loadable for analysis tooling. Existing JSONL signal logs provide raw data but are not indexed. | LOW | Existing `SignalLogger` writes all signals to JSONL. Existing `TradeEvent` JSONL captures trade lifecycle. New: signal history is already persisted via JSONL; add a summary index file linking signal_id to outcome for fast lookup. |
| Startup state recovery | On restart, load last-known state: open positions, pending fills, daily aggregates, last settlement check times. Resume paper trading without data loss. | MEDIUM | Existing `PaperTradeTracker::new()` starts fresh. New: add `PaperTradeTracker::from_state()` that loads snapshot file. Must handle partial/corrupt files gracefully (fallback to empty state with warning). |
| Atomic writes with corruption safety | State files must never be half-written. A crash during write must not corrupt the existing state. | LOW | Existing `ContractLifecycleManager.atomic_write()` uses write-to-tmp-then-rename pattern. Reuse same pattern for state files. |
| Configurable persistence directory and interval | Operator chooses where state files go and how often they are written. More frequent = less data loss on crash, more I/O. | LOW | Existing `PaperTradeConfig` has `log_dir`. New: add `state_dir` (or reuse `log_dir`) and `state_save_interval_secs` fields. |

**Implementation approach -- JSONL event log + periodic state snapshot:**

The system already writes append-only JSONL trade events (signal, entry, mtm, settlement). This is the "write-ahead log." The new state persistence adds a periodic state snapshot (full serialization of all open positions and aggregates to a JSON file using atomic write). On startup:

1. Load the latest state snapshot (positions, aggregates, settlement tracker state)
2. Replay any JSONL events that occurred after the snapshot timestamp
3. Resume normal operation

This is a standard "snapshot + WAL replay" pattern used in databases and trading systems. The existing JSONL files serve as the WAL; the new snapshot file provides fast startup.

---

## Differentiators

Features that go beyond basic paper trading validation and provide deeper insight into signal quality. Not expected, but significantly increase the value of the paper trading period.

### D-1: Settlement Timing Intelligence

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Settlement prediction scheduling | Pre-schedule settlement checks based on known expiry dates from `EventMapping`. Do not poll every minute -- poll at the right time (e.g., 08:05 UTC for Deribit expirations). | LOW | Existing `EventMapping.expiry` field. Existing `ExpiryThreshold` tiered warnings. New: schedule settlement data fetches based on expiry + settlement_delay. |
| Cross-venue settlement discrepancy detection | When the same underlying event settles differently on Polymarket vs Kalshi (it happens -- see the Cardi B and Zelensky suit incidents), flag the discrepancy for the operator and mark affected positions as basis-risk-loss. | MEDIUM | Existing `SettlementMetadata` tracks per-venue resolution sources. Existing basis risk scoring. New: compare actual outcomes across venues for the same event. |
| Deribit settlement index value capture | Capture the actual Deribit Index value at settlement (the 30-min TWAP) and compare against the strike to determine in-the-money/out-of-the-money outcome for options positions. | LOW | Existing Deribit REST infrastructure. `public/get_delivery_prices` returns delivery_price and date. New: compare delivery_price against EventMapping.strike to determine binary outcome. |

### D-2: Advanced Signal Analytics

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Threshold effectiveness analysis | For each ThresholdStatus (PassedBoth, PassedStaticOnly, Filtered), compute win rate and average P&L. This directly answers: "Should I tighten or loosen my thresholds?" | MEDIUM | Existing `ThresholdStatus` on all signals (logged to JSONL even when filtered). Existing threshold_components breakdown. New: post-hoc analysis correlating threshold status with settlement outcomes. |
| Cost model validation | Compare predicted costs (fees, slippage, carry) against realized costs at settlement. If the cost model is systematically under-estimating, signals appear more profitable than they are. | MEDIUM | Existing `CostBreakdown` on every `ArbSignal`. Existing `PaperPosition.adverse_selection` (one component of realized cost). New: compute realized total cost at settlement and compare against predicted. |
| Pattern-specific performance | Track P&L by `SpreadPattern` (BuyPolyYesSellKalshiYes, etc.) and by `ArbDirection` (BuyPredictionSellOptions, SellPredictionBuyOptions). Some patterns may systematically outperform others. | LOW | Existing `SpreadPattern` and `ArbDirection` on all signals/positions. New: group settlement outcomes by these fields. |
| Signal correlation analysis | Detect whether signals cluster in time (many signals within minutes may be the same opportunity, not independent signals) to avoid over-counting and inflating hit rate. | MEDIUM | Existing signal timestamps and event_ids. New: sliding window analysis of signal emission rate. Cluster correlated signals and count as single opportunity. |

### D-3: Operational Intelligence

| Feature | Value Proposition | Complexity | Dependencies on Existing Code |
|---------|-------------------|------------|-------------------------------|
| Feed quality scoring over time | Track per-venue quality metrics (uptime, message rate, latency percentiles) over days/weeks. Surfaces seasonal patterns and venue reliability trends. | LOW | Existing `VenueHealth` tracks per-message timestamps. Existing Prometheus histograms. New: periodic snapshot of quality metrics to analysis file. |
| Degradation impact analysis | When a degradation event occurs (feed outage, high staleness period), measure the impact on signal generation: how many signals were missed or degraded during the window? | MEDIUM | Existing `arb_staleness_rejections` counter. Existing `feed_available` gauge per venue. New: correlate degradation windows with signal emission gaps. |

---

## Anti-Features

Features that seem relevant to v1.1 but should be explicitly avoided.

| Anti-Feature | Why It Seems Relevant | Why Avoid | What to Do Instead |
|--------------|----------------------|-----------|-------------------|
| Full database (SQLite/PostgreSQL) for state persistence | "Proper" persistence should use a real database | Adds a dependency, schema migrations, and operational complexity for a solo-trader single-binary system. The data volume (hundreds of positions, not millions) does not justify a database. | JSONL event logs + periodic JSON state snapshots. Already matches the JSONL patterns used throughout the codebase. Zero new dependencies. |
| Real-time dashboarding of signal analytics | "Need to see hit rate updating live" | Adds frontend complexity. Signal analysis is inherently retrospective -- you need settlement outcomes, which arrive hours/days after signals. Real-time display of incomplete statistics is misleading. | Periodic analysis reports to tracing logs + dedicated JSONL analysis file. Parse with existing tooling (jq, Grafana with JSONL plugin, or simple Python scripts). |
| Automated threshold adjustment based on analytics | "If hit rate is low, auto-tighten thresholds" | Premature optimization. Need 2-4 weeks of data before statistics are meaningful. Auto-adjustment on sparse data will oscillate and overfit. Automated parameter changes in a system managing paper trades are unnecessary complexity. | Surface threshold effectiveness metrics clearly. Let the operator make informed manual adjustments via TOML config. |
| Email/SMS/PagerDuty integration for alerts | "Enterprise alerting for failures" | Over-engineering for a solo-trader system. The operator monitors via terminal, Grafana, and Prometheus/Alertmanager. Adding direct notification integrations is unnecessary when Prometheus Alertmanager already handles routing. | Emit all alerts as Prometheus metrics + tracing logs. If the operator wants PagerDuty, configure Alertmanager routing rules (external to this system). |
| Historical data backfill from venue APIs | "Fetch all historical settlements to bootstrap analysis" | Each venue has different historical data APIs with different rate limits and formats. Backfilling is a separate data engineering task, not a core runtime feature. | The system generates its own data going forward. Settlement tracking starts fresh and accumulates. JSONL replay of recorded feeds handles the backtesting case. |
| Multi-file state with versioned migrations | "State schema will change, need migrations" | Over-engineering for v1.1. The state format is a direct serialization of existing structs that already derive Serialize/Deserialize. If the format changes, the operator can restart fresh (it is paper trading, not real capital). | Single JSON snapshot file. If deserialization fails on startup, log a warning and start fresh. Add a schema_version field for future-proofing, but do not build a migration system. |

---

## Feature Dependencies

```
[Settlement Outcome Tracking (TS-1)]
    |
    +--> Requires: EventRegistry (expiry dates, venue instruments)
    +--> Requires: ContractLifecycleManager (REST polling infrastructure)
    +--> Requires: PaperTradeTracker (position settlement method)
    |
    v
[Signal Analysis Tooling (TS-2)]
    |
    +--> Requires: Settlement Outcome Tracking (TS-1) -- cannot compute hit rate without outcomes
    +--> Requires: PaperPosition (trade data)
    +--> Requires: ArbSignal (signal metadata for correlation analysis)
    |
    v
[Threshold Effectiveness Analysis (D-2)] -- most valuable differentiator
    |
    +--> Requires: Signal Analysis Tooling (TS-2)
    +--> Requires: Signal JSONL logs (all signals including filtered ones)

[Failure Alerting (TS-3)]
    |
    +--> Requires: VenueHealth (feed status)
    +--> Requires: Prometheus metrics (alert emission)
    +--> Independent of: Settlement tracking -- can be built first
    |
    v
[Operational Intelligence (D-3)] -- nice to have

[File-Based Persistence (TS-4)]
    |
    +--> Requires: PaperPosition Serialize/Deserialize (already implemented)
    +--> Requires: Atomic write pattern (already implemented)
    +--> Independent of: Settlement tracking, analysis tooling
    +--> Enables: Multi-week paper trading sessions without data loss
```

### Dependency Notes

- **Settlement Outcome Tracking is the critical dependency.** Signal analysis (hit rate, edge measurement, false positive rate) is mathematically impossible without knowing how events actually resolved. This must be built first.
- **Failure Alerting is independent.** It depends only on existing feed health infrastructure and can be built in parallel with settlement tracking.
- **File-Based Persistence is independent.** It depends only on existing serialization traits and can be built in parallel.
- **Signal Analysis depends on settlement tracking.** The analysis tooling consumes settlement outcomes. However, the aggregation infrastructure (counters, grouping) can be scaffolded before settlement data flows in.
- **Threshold effectiveness analysis is the highest-value differentiator** but requires the full chain: settlement outcomes -> signal analysis -> threshold correlation.

### Build Order Recommendation

```
Phase 1 (parallel tracks, no dependencies on each other):
  Track A: File-Based Persistence (TS-4) -- enables long paper trading runs
  Track B: Failure Alerting (TS-3) -- immediate operational value

Phase 2 (depends on nothing new, but gates Phase 3):
  Settlement Outcome Tracking (TS-1) -- the critical path item

Phase 3 (depends on TS-1):
  Signal Analysis Tooling (TS-2) -- the payoff
  Differentiators (D-1, D-2, D-3) -- if time allows
```

---

## MVP Recommendation

### Must Build (answers "are my signals real?")

1. **Settlement Outcome Tracking (TS-1)** -- Without this, the entire v1.1 milestone is pointless. Poll each venue's REST API for settlement data. Match outcomes to paper positions. Compute realized P&L.
2. **Signal Analysis Tooling (TS-2)** -- Core metrics: hit rate, average edge, false positive rate, time-to-convergence. Emit as periodic structured log reports and Prometheus metrics.
3. **File-Based Persistence (TS-4)** -- Periodic state snapshots with atomic writes. Paper trading sessions must survive restarts over the 2-4 week validation period.
4. **Failure Alerting (TS-3)** -- Stale data alerting, silent failure detection, partial feed detection. Without this, the operator cannot trust that the system was running correctly during the validation period, which undermines signal analysis conclusions.

### Defer

- **Threshold effectiveness analysis (D-2):** Requires accumulated settlement data. Build the data collection in v1.1; add the analysis in a future pass or as a post-hoc script.
- **Cross-venue settlement discrepancy detection (D-1):** Valuable but rare -- maybe 1-2 events per year. Manual review of settlement data is sufficient for v1.1.
- **Signal correlation analysis (D-2):** Nice-to-have for avoiding inflated hit rates. Can be computed post-hoc from JSONL logs.
- **All operational intelligence features (D-3):** Prometheus metrics provide most of this already. Defer dedicated analysis tooling.

---

## Sources

### Venue Settlement Documentation (HIGH confidence)
- [Deribit Settlement](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) -- delivery price TWAP calculation, 08:00 UTC expiry
- [Deribit API: get_expirations](https://docs.deribit.com/api-reference/market-data/public-get_expirations) -- REST endpoint for expiration data
- [Deribit API: get_delivery_prices](https://docs.deribit.com/) -- REST endpoint for delivery prices (no auth required)
- [Kalshi API: Get Market](https://docs.kalshi.com/api-reference/market/get-market) -- market result and status fields
- [Kalshi API: Get Settlements](https://docs.kalshi.com/api-reference/portfolio/get-settlements) -- settlement data with market_result
- [Kalshi Market Settlement (FIX)](https://docs.kalshi.com/fix/market-settlement) -- FIX protocol settlement
- [Polymarket: How Markets Resolve](https://docs.polymarket.com/polymarket-learn/markets/how-are-markets-resolved) -- UMA Optimistic Oracle resolution process
- [Polymarket Gamma API Structure](https://docs.polymarket.com/developers/gamma-markets-api/gamma-structure) -- market response fields

### Signal Quality and Trading Analysis (MEDIUM confidence)
- [How to Measure Signal Quality -- Macrosynergy](https://macrosynergy.com/research/how-to-measure-the-quality-of-a-trading-signal/) -- signal quality metrics framework
- [Signal Decay Analysis -- MicroAlphas](https://microalphas.com/signal-decay-patterns/) -- alpha decay rates, 60% initial decay
- [Top 7 Backtesting Metrics -- LuxAlgo](https://www.luxalgo.com/blog/top-7-metrics-for-backtesting-results/) -- hit rate, Sharpe, drawdown
- [Walk-Forward Validation Framework -- arXiv](https://arxiv.org/html/2512.12924v1) -- rolling validation methodology
- [Top 7 Trading Signals -- ExtractAlpha](https://extractalpha.com/2025/07/01/top-7-trading-signals-every-quant-should-track/) -- quantitative signal metrics

### Failure Detection and Monitoring (MEDIUM confidence)
- [Silent Data Failures -- Datagaps](https://datagapsproducts.medium.com/data-observability-use-cases-preventing-silent-data-failures-across-industries-aa661048a743) -- 41% undetected degradation stat
- [Stale Data Detection -- Data Intellect](https://dataintellect.com/blog/stale-data-part-2-duplicate-detection-and-apis/) -- stale data monitoring patterns
- [Trade Infrastructure Monitoring 2025 -- A-Team](https://a-teaminsight.com/blog/the-top-seven-trade-infrastructure-monitoring-solutions-in-2025/) -- trading system monitoring approaches
- [Proactive Model Degradation Detection -- ECML-PKDD 2025](https://ecmlpkdd-storage.s3.eu-central-1.amazonaws.com/preprints/2025/ads/preprint_ecml_pkdd_2025_ads_315.pdf) -- PRODEM framework

### Persistence and Rust Ecosystem (MEDIUM confidence)
- [serde-jsonlines crate](https://docs.rs/serde-jsonlines) -- JSONL serialization for Rust
- [serde-rs/json](https://github.com/serde-rs/json) -- JSON serialization
- [Polymarket UMA CTF Adapter](https://github.com/Polymarket/uma-ctf-adapter) -- on-chain resolution mechanism

---

*Feature research for: v1.1 Paper Trading Validation*
*Researched: 2026-02-24*
