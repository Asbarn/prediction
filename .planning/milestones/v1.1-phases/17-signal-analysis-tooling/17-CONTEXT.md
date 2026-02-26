# Phase 17: Signal Analysis Tooling - Context

**Gathered:** 2026-02-26
**Status:** Ready for planning

<domain>
## Phase Boundary

Compute statistical evidence from settled paper trade positions to answer "are the arbitrage signals generating real alpha?" Metrics include hit rate, cost-adjusted edge, false positive rate, and time-to-convergence. This phase builds on Phase 16's settlement outcome tracking. It does NOT add new signal generation, trading logic, or strategy optimization — just measurement and reporting of what exists.

</domain>

<decisions>
## Implementation Decisions

### Cost modeling & edge calculation
- Per-venue fee schedule: each venue (Polymarket, Kalshi, Deribit) has its own maker/taker fee tier configured
- Slippage estimated from order-book depth at signal time (not a flat bps assumption)
- Report both gross hit rate (price moved in right direction) and net hit rate (profitable after fees + slippage) so operator can see cost impact
- Adverse selection captured naturally by filling each leg at its own next tick using real market data — no synthetic penalty
- Log inter-leg time gap as metadata for each paper trade
- Add `max_leg_fill_gap` threshold (e.g., 2s) to mark paper trades as "stale fill" when second leg's tick arrives too late — keeps signal quality stats clean
- Measure empirical adverse selection over time rather than guessing at a decay parameter

### Analysis granularity
- Primary dimensions (Prometheus labels): venue pair (Polymarket↔Deribit, Kalshi↔Deribit, Polymarket↔Kalshi), event ID (canonical event), and time period
- Per individual event as the Prometheus label — finest grain, aggregate at query time in Grafana
- Event characteristics (strike distance, expiry alignment, basis risk) already exist as metadata in the event registry — use PromQL grouping / label joins in Grafana to slice by characteristics rather than building application-level buckets
- Daily rollups as the primary time aggregation unit; hourly/weekly derived in Grafana from raw data
- Lifetime accumulators only (no application-level rolling windows) — rolling windows done at Grafana query time
- Cardinality is manageable: BTC-only v1 with three venues means single digits to low dozens of active events

### Threshold effectiveness
- Side-by-side hit rates: show hit rate, avg edge, count for each ThresholdStatus category (PassedBoth, PassedStaticOnly, Filtered)
- Log filtered signals too (signals that didn't become paper trades) with their eventual settlement outcomes — enables "did I filter out winners?" retrospective analysis
- Numbers only, no heuristic recommendations — operator interprets and decides
- Threshold effectiveness broken down by same dimensions (venue pair, event, time period), not just aggregate

### Operator workflow
- Grafana dashboards for live monitoring, JSONL for deeper post-hoc analysis — both equally important
- Per-settlement JSONL records only (one line per settled position with all computed metrics) — no periodic summary records in JSONL
- Human-readable log line on each settlement in addition to structured JSONL (e.g., "SETTLED: BTC>100K Poly↔Deribit +2.3% edge (net), hit")
- Daily log summary: once per day, emit a summary log entry with the day's hit rate, total settled, avg edge, etc.

### Claude's Discretion
- Exact Prometheus metric names and label structure
- JSONL record schema (what fields, naming conventions)
- Human-readable log line format
- Daily summary trigger mechanism (timer vs settlement count)
- How filtered signals are tracked alongside settlement outcomes
- Internal accumulator data structures

</decisions>

<specifics>
## Specific Ideas

- System is prediction market arb (Polymarket/Kalshi vs Deribit options-implied probabilities), NOT generic crypto arb — terminology and dimensions should reflect this domain
- Emit finest-grain data from application, aggregate at query time — don't bake analytical assumptions into the application layer where they're hard to change
- Fill each leg independently at its own next tick — this naturally captures adverse selection with real market data, better than synthetic penalties
- Prometheus label cardinality is fine for v1 (BTC-only, 3 venues, low event count) — if expanding to multi-asset with hundreds of events, would move to proper analytics DB (ClickHouse, TimescaleDB)
- Define all Prometheus labels from the start — much harder to add labels retroactively than to ignore ones you don't need yet

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 17-signal-analysis-tooling*
*Context gathered: 2026-02-26*
