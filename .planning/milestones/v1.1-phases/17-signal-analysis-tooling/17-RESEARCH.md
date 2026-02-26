# Phase 17: Signal Analysis Tooling - Research

**Researched:** 2026-02-26
**Domain:** Post-settlement statistical analysis of arbitrage signal quality
**Confidence:** HIGH

## Summary

Phase 17 adds statistical analysis of settled paper trade positions to answer "are the arbitrage signals generating real alpha?" The system already has all prerequisite infrastructure: Phase 16 delivers `SettlementOutcome` via mpsc channel to `PaperTradeTracker`, which creates `SettlementRecord` entries with per-leg P&L breakdown (raw, net, fees, slippage) and logs them to JSONL. Phase 13's paper trade tracker already tracks position lifecycle from signal through settlement. Phase 15 checkpoints accumulator state. The primary engineering work is (1) building accumulator data structures that compute hit rate, cost-adjusted edge, false positive rate, and time-to-convergence from settled positions, (2) exposing those accumulators as Prometheus gauges with venue-pair and event-id labels, (3) enriching JSONL settlement records with the computed analysis metrics, and (4) tracking filtered signals alongside settlement outcomes for threshold effectiveness analysis.

The project has a strict zero-new-dependencies policy for v1.1. All required functionality (Prometheus metrics via `metrics` crate, JSONL logging via `serde_json` + `BufWriter`, `Decimal` arithmetic via `rust_decimal`) is already available in the dependency tree. The `metrics` crate's `gauge!` macro with label key-value pairs directly supports the required Prometheus label dimensions (venue_pair, event_id). The `DailyAggregator` pattern in `src/paper_trade/aggregator.rs` provides the template for accumulator design and checkpoint integration.

**Primary recommendation:** Build a `SignalAnalyzer` struct that consumes settled positions (from `handle_settlement` in the tracker) and filtered signal events (from the signal engine), accumulates lifetime statistics keyed by (venue_pair, event_id, threshold_status), and exposes computed metrics via Prometheus gauges and per-settlement JSONL enrichment. Integrate into the existing `PaperTradeTracker` event loop. Persist accumulator state in `CheckpointState` (version bump to 3).

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Cost modeling & edge calculation:**
- Per-venue fee schedule: each venue (Polymarket, Kalshi, Deribit) has its own maker/taker fee tier configured
- Slippage estimated from order-book depth at signal time (not a flat bps assumption)
- Report both gross hit rate (price moved in right direction) and net hit rate (profitable after fees + slippage) so operator can see cost impact
- Adverse selection captured naturally by filling each leg at its own next tick using real market data -- no synthetic penalty
- Log inter-leg time gap as metadata for each paper trade
- Add `max_leg_fill_gap` threshold (e.g., 2s) to mark paper trades as "stale fill" when second leg's tick arrives too late -- keeps signal quality stats clean
- Measure empirical adverse selection over time rather than guessing at a decay parameter

**Analysis granularity:**
- Primary dimensions (Prometheus labels): venue pair (Polymarket<>Deribit, Kalshi<>Deribit, Polymarket<>Kalshi), event ID (canonical event), and time period
- Per individual event as the Prometheus label -- finest grain, aggregate at query time in Grafana
- Event characteristics (strike distance, expiry alignment, basis risk) already exist as metadata in the event registry -- use PromQL grouping / label joins in Grafana to slice by characteristics rather than building application-level buckets
- Daily rollups as the primary time aggregation unit; hourly/weekly derived in Grafana from raw data
- Lifetime accumulators only (no application-level rolling windows) -- rolling windows done at Grafana query time
- Cardinality is manageable: BTC-only v1 with three venues means single digits to low dozens of active events

**Threshold effectiveness:**
- Side-by-side hit rates: show hit rate, avg edge, count for each ThresholdStatus category (PassedBoth, PassedStaticOnly, Filtered)
- Log filtered signals too (signals that didn't become paper trades) with their eventual settlement outcomes -- enables "did I filter out winners?" retrospective analysis
- Numbers only, no heuristic recommendations -- operator interprets and decides
- Threshold effectiveness broken down by same dimensions (venue pair, event, time period), not just aggregate

**Operator workflow:**
- Grafana dashboards for live monitoring, JSONL for deeper post-hoc analysis -- both equally important
- Per-settlement JSONL records only (one line per settled position with all computed metrics) -- no periodic summary records in JSONL
- Human-readable log line on each settlement in addition to structured JSONL (e.g., "SETTLED: BTC>100K Poly<>Deribit +2.3% edge (net), hit")
- Daily log summary: once per day, emit a summary log entry with the day's hit rate, total settled, avg edge, etc.

### Claude's Discretion
- Exact Prometheus metric names and label structure
- JSONL record schema (what fields, naming conventions)
- Human-readable log line format
- Daily summary trigger mechanism (timer vs settlement count)
- How filtered signals are tracked alongside settlement outcomes
- Internal accumulator data structures

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ANLZ-01 | System computes hit rate (profitable-at-settlement / total-settled positions) | `SignalAnalyzer` accumulator tracks `gross_hits` (raw_pnl > 0) and `net_hits` (net_pnl > 0) counters alongside `total_settled`; computes both gross and net hit rates. Exposed as `signal_analysis_hit_rate{venue_pair, event_id, kind="gross"}` and `kind="net"` Prometheus gauges. |
| ANLZ-02 | System computes cost-adjusted average edge per settled position | `SignalAnalyzer` accumulates `sum_net_pnl` (total net P&L across all settled) and divides by `total_settled`. Also accumulates `sum_gross_pnl`, `sum_fees`, `sum_slippage` for decomposition. Exposed as `signal_analysis_avg_edge{venue_pair, event_id}` gauge. |
| ANLZ-03 | System computes false positive rate (signals resulting in loss at settlement) | False positive = net_pnl <= 0 at settlement. `false_positive_count / total_settled`. This is `1.0 - net_hit_rate` by definition, but tracked as a distinct counter for clarity. Exposed as `signal_analysis_false_positive_rate{venue_pair, event_id}` gauge. |
| ANLZ-04 | System computes time-to-convergence (signal generation to price convergence duration) | `signal_timestamp_ms` is on `PaperPosition`; `settled_at_ms` is set by `finalize_settlement()`. Difference = time-to-convergence in seconds. Accumulated as `sum_convergence_secs` and `convergence_count` for average, plus exposed per-settlement. Prometheus gauge `signal_analysis_avg_convergence_secs{venue_pair, event_id}`. |
| ANLZ-05 | System correlates threshold status with settlement outcomes | Requires propagating `ThresholdStatus` from signal/spread layer onto `PaperPosition`. The `SpreadResult` does not carry `ThresholdStatus` (only `threshold` value), but `ArbSignal` does. The spread patterns already have `threshold` and `threshold_components` fields. Need to add a `ThresholdStatus` field to `PaperPosition` and propagate from the signal evaluation context. Accumulators keyed by `(venue_pair, event_id, threshold_status)`. For filtered signals: track separately via a `FilteredSignalTracker` that records signal event_id + threshold_status and later correlates with settlement outcomes. |
| ANLZ-06 | Analysis metrics are exposed as Prometheus gauges | All accumulators emit `metrics::gauge!()` calls with venue_pair, event_id labels. Updated on each settlement. Reuses existing `metrics-exporter-prometheus` infrastructure (Phase 6). |
| ANLZ-07 | Analysis results are logged to structured JSONL | Enrich existing `SettlementRecord` with analysis fields (hit_rate_at_settlement, running_avg_edge, convergence_secs, threshold_status). Write to existing settlement JSONL logger. One record per settlement (per CONTEXT.md decision). |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `metrics` | 0.24 | Prometheus gauge/counter/histogram facade | Already in Cargo.toml; all existing metrics use this |
| `metrics-exporter-prometheus` | 0.18 | Prometheus HTTP scrape endpoint | Already installed in `setup_prometheus()` |
| `rust_decimal` | 1.40 | Exact decimal arithmetic for financial calculations | Already used throughout; prevents floating-point P&L drift |
| `serde` / `serde_json` | 1.0 | JSONL serialization for analysis records | Already used for all JSONL output |
| `chrono` | 0.4 | Timestamps for convergence measurement | Already used for settlement timestamps |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tracing` | 0.1 | Structured logging for human-readable settlement lines and daily summaries | Already used for all operator-visible log output |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| In-app rolling windows | Grafana `rate()` / `increase()` over raw counters | CONTEXT.md explicitly decided: lifetime accumulators only, rolling windows in Grafana |
| Per-dimension HashMap accumulators | Separate Prometheus labels only (no app-side tracking) | Need app-side accumulators for JSONL enrichment and daily log summaries; Prometheus alone insufficient |
| ClickHouse / TimescaleDB | File-based JSONL | Out of Scope per REQUIREMENTS.md; cardinality is low enough for file-based approach in v1 |

**Installation:**
No new dependencies needed. Zero-new-dependency constraint for v1.1 is maintained.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── paper_trade/
│   ├── tracker.rs       # Existing -- add SignalAnalyzer integration point in handle_settlement
│   ├── position.rs      # Existing -- add threshold_status field, stale_fill flag, inter_leg_gap_ms
│   ├── aggregator.rs    # Existing -- extend emit_daily_summary with analysis metrics
│   └── analyzer.rs      # NEW -- SignalAnalyzer struct, AccumulatorBucket, FilteredSignalTracker
├── persistence/
│   └── checkpoint.rs    # Existing -- add AnalysisAccumulatorState to CheckpointState (v3)
└── settlement/
    └── types.rs         # Existing -- enrich SettlementRecord with analysis fields
```

### Pattern 1: Keyed Accumulator Buckets
**What:** A `HashMap<AccumulatorKey, AccumulatorBucket>` where the key is `(venue_pair, event_id, threshold_status)` and the bucket holds running counters (total_settled, gross_hits, net_hits, sum_net_pnl, sum_gross_pnl, sum_fees, sum_slippage, sum_convergence_secs, etc.).
**When to use:** For all ANLZ metrics that must be broken down by multiple dimensions.
**Why:** Mirrors the `DailyAggregator` pattern already in the codebase (`HashMap<String, DailyRollup>`). Keyed accumulators are the standard approach when you need both app-side computation (for JSONL enrichment) and Prometheus export (for live dashboards). Lifetime accumulators (never reset) per CONTEXT.md decision.

```rust
/// Key for analysis accumulator buckets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccumulatorKey {
    /// Venue pair label (e.g., "polymarket_kalshi", "polymarket_deribit").
    pub venue_pair: String,
    /// Canonical event ID (e.g., "BTC-100K-2025-06-30").
    pub event_id: String,
    /// Threshold status category.
    pub threshold_status: ThresholdStatus,
}

/// Running statistics for a single accumulator bucket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccumulatorBucket {
    pub total_settled: u64,
    pub gross_hits: u64,       // raw_pnl > 0
    pub net_hits: u64,         // net_pnl > 0
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_gross_pnl: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_net_pnl: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_fees: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub sum_slippage: Decimal,
    pub sum_convergence_secs: f64,
    pub stale_fill_count: u64,
}
```

### Pattern 2: Venue Pair Derivation from SpreadPattern
**What:** Derive the venue pair label from the `SpreadPattern` enum on `PaperPosition`.
**When to use:** Every time a position is analyzed -- the pattern tells us which venues are involved.
**Why:** `SpreadPattern` already encodes buy/sell venue information via `buy_venue()` / `sell_venue()`. The venue pair is the sorted pair of these two venues (alphabetical for consistent labeling).

```rust
impl SpreadPattern {
    /// Derive the venue pair label for analysis dimensions.
    /// Returns a consistent alphabetically-sorted pair label.
    pub fn venue_pair_label(&self) -> &'static str {
        match (self.buy_venue(), self.sell_venue()) {
            (Venue::Kalshi, Venue::Polymarket) | (Venue::Polymarket, Venue::Kalshi) => "kalshi_polymarket",
            (Venue::Deribit, Venue::Polymarket) | (Venue::Polymarket, Venue::Deribit) => "deribit_polymarket",
            (Venue::Deribit, Venue::Kalshi) | (Venue::Kalshi, Venue::Deribit) => "deribit_kalshi",
            _ => "unknown",
        }
    }
}
```

### Pattern 3: Filtered Signal Tracking for Threshold Effectiveness
**What:** A lightweight tracker that records filtered signals (ThresholdStatus::Filtered and PassedStaticOnly) with their event_id so that when settlement occurs, the system can correlate what would have happened had the signal not been filtered.
**When to use:** ANLZ-05 threshold effectiveness analysis -- "did I filter out winners?"
**Why:** CONTEXT.md explicitly requires logging filtered signals with their eventual settlement outcomes. The ArbSignal carries ThresholdStatus. Currently, only PassedBoth signals flow to PaperTradeTracker via SpreadEngine. Filtered/PassedStaticOnly signals are logged to signal JSONL but not tracked for settlement correlation.

**Implementation approach:** In the CrossAssetEngine (or a new channel from it), emit all signal evaluations (including filtered ones) to a lightweight accumulator. When settlement arrives for an event_id, look up whether any filtered signals existed for that event and record their hypothetical outcome. This requires:
1. A `HashMap<String, Vec<FilteredSignalEntry>>` keyed by event_id storing filtered signal metadata (net_edge, threshold_status, timestamp_ms).
2. On settlement: look up event_id in filtered signals, compute hypothetical outcome, update threshold effectiveness counters.

### Pattern 4: Enriched Settlement JSONL Record
**What:** Extend the existing `SettlementRecord` struct with analysis fields computed at settlement time.
**When to use:** ANLZ-07 -- every settlement record logged to JSONL includes running analysis metrics.
**Why:** Per CONTEXT.md: "Per-settlement JSONL records only (one line per settled position with all computed metrics)." The existing `SettlementRecord` in `settlement/types.rs` already has the P&L breakdown. Add analysis-specific fields.

```rust
// Additional fields on SettlementRecord (or a wrapper AnalysisSettlementRecord):
pub struct AnalysisSettlementRecord {
    // ... all existing SettlementRecord fields ...

    /// Venue pair label for this position.
    pub venue_pair: String,
    /// ThresholdStatus of the signal that created this position.
    pub threshold_status: ThresholdStatus,
    /// Time-to-convergence in seconds (signal_timestamp to settlement).
    pub convergence_secs: f64,
    /// Whether this was a gross hit (raw_pnl > 0).
    pub gross_hit: bool,
    /// Whether this was a net hit (net_pnl > 0, after fees+slippage).
    pub net_hit: bool,
    /// Running gross hit rate at time of this settlement.
    pub running_gross_hit_rate: f64,
    /// Running net hit rate at time of this settlement.
    pub running_net_hit_rate: f64,
    /// Running average net edge at time of this settlement.
    pub running_avg_net_edge: f64,
    /// Running false positive rate at time of this settlement.
    pub running_false_positive_rate: f64,
    /// Running average convergence time in seconds.
    pub running_avg_convergence_secs: f64,
    /// Inter-leg fill time gap in milliseconds (if multi-leg).
    pub inter_leg_gap_ms: Option<i64>,
    /// Whether marked as stale fill (gap > max_leg_fill_gap).
    pub stale_fill: bool,
}
```

### Pattern 5: Daily Summary via Existing Timer
**What:** Emit analysis summary in the existing daily tick handler in `PaperTradeTracker::run()`.
**When to use:** CONTEXT.md requires daily log summary with hit rate, total settled, avg edge.
**Why:** The tracker already has a `daily_tick` interval timer at line 376 that calls `emit_daily_summary`. Extend this to include analysis metrics. Timer-based (not settlement-count-based) is simpler and consistent with existing pattern.

### Anti-Patterns to Avoid
- **Application-level rolling windows:** CONTEXT.md explicitly says lifetime accumulators only. Rolling windows are done at Grafana query time. Do NOT build sliding window buffers.
- **Separate analysis service/task:** Do NOT spawn a separate tokio task for analysis. The analysis is triggered synchronously inside `handle_settlement` in the tracker -- fast O(1) accumulator updates, no async needed.
- **Storing full position history in accumulators:** Accumulators store running sums and counts only, never raw data. The JSONL log IS the raw data store for post-hoc analysis.
- **Building application-level event characteristic buckets:** CONTEXT.md says event characteristics (strike distance, expiry alignment, basis risk) should be sliced via PromQL label joins in Grafana, not application-level buckets.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Prometheus metric emission | Custom HTTP endpoint | `metrics::gauge!()` with labels | Already set up in Phase 6; adding gauges with labels is zero-config |
| JSONL file rotation | Custom rotation logic | Existing `SettlementLogger` pattern | Already handles daily rotation, buffered writes, flush |
| Decimal arithmetic | f64 for P&L | `rust_decimal::Decimal` | Already used everywhere; prevents accumulation drift |
| Checkpoint serialization | Custom binary format | `serde_json` with `CheckpointState` | Already proven in Phase 15; just add new fields with `#[serde(default)]` |

**Key insight:** Phase 17 is almost entirely about wiring existing infrastructure together with new accumulator logic. There are no new external dependencies, no new I/O patterns, no new async coordination patterns. The complexity is in getting the math right and the label dimensions correct.

## Common Pitfalls

### Pitfall 1: ThresholdStatus Not on PaperPosition
**What goes wrong:** `PaperPosition` currently has no `threshold_status` field. `SpreadResult` has `threshold` (the value) and `threshold_components` but not `ThresholdStatus`. `ArbSignal` has `ThresholdStatus` but flows to a separate consumer (the ArbSignal consumer in main.rs), not to PaperTradeTracker.
**Why it happens:** The signal pipeline has two paths: SpreadEngine -> PaperTradeTracker (via SpreadResult) and CrossAssetEngine -> ArbSignal consumer. ThresholdStatus evaluation happens in CrossAssetEngine, but paper trades come from SpreadEngine.
**How to avoid:** The SpreadResult already carries `threshold` and `threshold_components` (including `static_floor` and `final_threshold`). The threshold status can be re-derived in the paper trade tracker from these fields: if `threshold_components` is Some and `net_spread >= final_threshold`, it's PassedBoth; if `net_spread >= static_floor` but `< final_threshold`, it's PassedStaticOnly. Alternatively, add `ThresholdStatus` directly to `SpreadResult`. The latter is cleaner.
**Warning signs:** If ANLZ-05 tests show all positions as "unknown" threshold status.

### Pitfall 2: Prometheus Label Cardinality Explosion
**What goes wrong:** Using fine-grained labels (event_id) on counters/gauges can cause cardinality issues if event count grows.
**Why it happens:** Each unique label combination creates a new time series in Prometheus.
**How to avoid:** Per CONTEXT.md: "Cardinality is manageable: BTC-only v1 with three venues means single digits to low dozens of active events." This is safe for v1. If expanding to multi-asset, move to ClickHouse/TimescaleDB. For now, 3 venue pairs x ~10 events x 3 threshold statuses = ~90 time series max. Well within safe limits.
**Warning signs:** Prometheus scrape latency increasing, memory usage growing.

### Pitfall 3: Stale Fill Detection Requires Inter-Leg Timing Data
**What goes wrong:** The `max_leg_fill_gap` feature requires knowing when each leg was filled, but currently `PaperPosition` only has a single `entry_timestamp_ms`.
**Why it happens:** In the current paper trade flow, fill happens on a single next-tick snapshot. Both legs are filled simultaneously from the same event's snapshot.
**How to avoid:** For Poly-Kalshi spreads (current patterns), both legs come from the same event mapping, so fill happens from the same event's snapshots. But the exchange timestamps (`poly_exchange_ts`, `kalshi_exchange_ts`) on `SpreadResult` capture the actual venue-side timing. Store these on PaperPosition and compute inter-leg gap from the exchange timestamps. If gap exceeds `max_leg_fill_gap`, flag as stale fill.
**Warning signs:** All positions showing zero inter-leg gap because only local timestamps are used.

### Pitfall 4: Checkpoint Version Compatibility
**What goes wrong:** Adding `analysis_accumulators` to `CheckpointState` breaks backward compatibility with existing v2 checkpoints.
**Why it happens:** New field without `#[serde(default)]` causes deserialization failure.
**How to avoid:** Follow the exact pattern from Phase 16 (v1 -> v2 upgrade): add `#[serde(default)]` on the new field, bump version to 3. The existing `v1_checkpoint_backward_compatibility` test in `checkpoint.rs` shows the proven pattern.
**Warning signs:** Startup crash after upgrade with "missing field" deserialization error.

### Pitfall 5: Filtered Signal Correlation with Settlement
**What goes wrong:** Filtered signals are never logged with settlement outcomes because they never become paper trades, so there is no position to settle.
**Why it happens:** Only signals that pass threshold become PaperPositions. Filtered signals are logged to signal JSONL only.
**How to avoid:** Maintain a separate `FilteredSignalTracker` that stores `(event_id, threshold_status, net_edge, timestamp_ms)` for filtered/PassedStaticOnly signals. When settlement arrives for an event_id, check if any filtered signals existed for that event and compute what the outcome would have been (based on whether the event settled Yes/No vs the signal's direction). This is approximate (no actual fill prices) but sufficient for threshold tuning.
**Warning signs:** Threshold effectiveness report shows data only for PassedBoth, nothing for Filtered/PassedStaticOnly.

### Pitfall 6: Division by Zero in Rate Calculations
**What goes wrong:** Computing `gross_hits / total_settled` when `total_settled == 0`.
**Why it happens:** Early in system lifetime, no positions have settled yet.
**How to avoid:** Guard all rate computations with `if total_settled > 0` checks. Prometheus gauges should not be emitted (or emit 0.0 / NaN sentinel) when denominator is zero.
**Warning signs:** NaN or panic in Prometheus scrape output.

## Code Examples

### Accumulator Update on Settlement
```rust
// Called from PaperTradeTracker::handle_settlement after finalize_settlement()
fn record_settlement_analysis(&mut self, pos: &PaperPosition) {
    let venue_pair = pos.pattern.venue_pair_label().to_string();
    let threshold_status = pos.threshold_status.unwrap_or(ThresholdStatus::PassedBoth);

    let key = AccumulatorKey {
        venue_pair: venue_pair.clone(),
        event_id: pos.event_id.clone(),
        threshold_status,
    };

    let bucket = self.accumulators.entry(key).or_default();

    let total_net_pnl: Decimal = pos.settled_legs.iter().map(|l| l.net_pnl).sum();
    let total_raw_pnl: Decimal = pos.settled_legs.iter().map(|l| l.raw_pnl).sum();
    let total_fees: Decimal = pos.settled_legs.iter().map(|l| l.entry_fee + l.exit_fee).sum();
    let total_slippage: Decimal = pos.settled_legs.iter().map(|l| l.slippage_estimate).sum();

    bucket.total_settled += 1;
    if total_raw_pnl > Decimal::ZERO { bucket.gross_hits += 1; }
    if total_net_pnl > Decimal::ZERO { bucket.net_hits += 1; }
    bucket.sum_gross_pnl += total_raw_pnl;
    bucket.sum_net_pnl += total_net_pnl;
    bucket.sum_fees += total_fees;
    bucket.sum_slippage += total_slippage;

    // Time-to-convergence
    if let Some(settled_at_ms) = pos.settled_at_ms {
        let convergence_secs = (settled_at_ms - pos.signal_timestamp_ms) as f64 / 1000.0;
        bucket.sum_convergence_secs += convergence_secs;
    }

    // Stale fill check
    if pos.stale_fill {
        bucket.stale_fill_count += 1;
    }
}
```

### Prometheus Gauge Emission
```rust
fn emit_prometheus_gauges(&self) {
    for (key, bucket) in &self.accumulators {
        if bucket.total_settled == 0 { continue; }

        let vp = key.venue_pair.as_str();
        let eid = key.event_id.as_str();
        let ts = format!("{:?}", key.threshold_status);

        let gross_hit_rate = bucket.gross_hits as f64 / bucket.total_settled as f64;
        let net_hit_rate = bucket.net_hits as f64 / bucket.total_settled as f64;
        let false_positive_rate = 1.0 - net_hit_rate;
        let avg_net_edge = bucket.sum_net_pnl.to_f64().unwrap_or(0.0) / bucket.total_settled as f64;
        let avg_convergence = bucket.sum_convergence_secs / bucket.total_settled as f64;

        metrics::gauge!("signal_analysis_gross_hit_rate",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(gross_hit_rate);

        metrics::gauge!("signal_analysis_net_hit_rate",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(net_hit_rate);

        metrics::gauge!("signal_analysis_false_positive_rate",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(false_positive_rate);

        metrics::gauge!("signal_analysis_avg_net_edge",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(avg_net_edge);

        metrics::gauge!("signal_analysis_avg_convergence_secs",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(avg_convergence);

        metrics::gauge!("signal_analysis_total_settled",
            "venue_pair" => vp.to_string(), "event_id" => eid.to_string(), "threshold_status" => ts.clone()
        ).set(bucket.total_settled as f64);
    }
}
```

### Human-Readable Settlement Log Line
```rust
// Per CONTEXT.md: "SETTLED: BTC>100K Poly↔Deribit +2.3% edge (net), hit"
tracing::info!(
    event_id = %pos.event_id,
    venue_pair = %venue_pair,
    net_edge_pct = format!("{:+.1}%", net_edge_pct),
    outcome = if net_hit { "hit" } else { "miss" },
    convergence_secs = convergence_secs,
    threshold_status = ?threshold_status,
    "SETTLED: {} {} {:+.1}% edge (net), {}",
    pos.event_id, venue_pair, net_edge_pct, if net_hit { "hit" } else { "miss" }
);
```

### Daily Summary Extension
```rust
// Extend existing emit_daily_summary to include analysis metrics
tracing::info!(
    date = date,
    total_settled = total_settled_today,
    gross_hit_rate = format!("{:.1}%", gross_hit_rate * 100.0),
    net_hit_rate = format!("{:.1}%", net_hit_rate * 100.0),
    avg_net_edge = format!("{:.2}", avg_net_edge),
    false_positive_rate = format!("{:.1}%", false_positive_rate * 100.0),
    avg_convergence_secs = format!("{:.0}", avg_convergence_secs),
    stale_fills = stale_fill_count,
    "DAILY ANALYSIS SUMMARY"
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| P&L tracking only (Phase 13) | Settlement with per-leg P&L (Phase 16) | Phase 16 (2026-02-26) | Foundation for analysis metrics |
| DailyAggregator for basic rollups | Phase 17 adds keyed accumulators by dimension | Phase 17 | Multi-dimensional analysis |
| ThresholdStatus on ArbSignal only | Propagate to SpreadResult/PaperPosition | Phase 17 | Enables threshold effectiveness |

**Key codebase patterns to follow:**
- `DailyAggregator` (aggregator.rs): HashMap-based accumulator with export/import for checkpoint
- `SettlementLogger` (tracker.rs): JSONL logging with daily rotation and buffered writes
- `CheckpointState` v1->v2 upgrade (checkpoint.rs): `#[serde(default)]` for backward compatibility
- `handle_settlement` (tracker.rs): Settlement processing pipeline with metrics emission

## Open Questions

1. **ThresholdStatus propagation path**
   - What we know: `SpreadResult` carries `threshold` (Option<Decimal>) and `threshold_components` (Option<ThresholdComponents>). `ArbSignal` carries `ThresholdStatus`. PaperTradeTracker receives `SpreadResult`, not `ArbSignal`.
   - What's unclear: Whether to add `ThresholdStatus` to `SpreadResult` (changing the SpreadEngine interface) or re-derive it in the tracker from threshold_components fields.
   - Recommendation: Add `threshold_status: Option<ThresholdStatus>` to `SpreadResult` with `#[serde(default)]`. This is cleaner than re-deriving, and the SpreadEngine already evaluates the threshold to decide whether to emit. Minimal change, consistent with existing pattern.

2. **Filtered signal tracking scope**
   - What we know: CrossAssetEngine evaluates all signals and sets ThresholdStatus. Only PassedBoth signals flow to SpreadEngine -> PaperTradeTracker. Filtered signals go to signal JSONL.
   - What's unclear: How to efficiently route filtered signal metadata to the analysis accumulator without adding a new channel or coupling CrossAssetEngine to PaperTradeTracker.
   - Recommendation: Add an mpsc channel from CrossAssetEngine to PaperTradeTracker carrying a lightweight `FilteredSignalEvent { event_id, threshold_status, net_edge, timestamp_ms }`. The tracker stores these in a `HashMap<String, Vec<FilteredSignalEvent>>` keyed by event_id. On settlement, correlate. Bounded buffer with per-event cap to prevent unbounded growth. Alternatively, consume from signal JSONL logs post-hoc for truly offline analysis -- simpler but loses live Prometheus visibility.

3. **Inter-leg fill gap measurement**
   - What we know: `SpreadResult` has `poly_exchange_ts` and `kalshi_exchange_ts` (both `Option<i64>` millisecond timestamps from the venue). These represent when the venue produced the tick used for the fill.
   - What's unclear: Whether these timestamps are reliably populated for all venue combinations.
   - Recommendation: Store both exchange timestamps on `PaperPosition`. Compute inter-leg gap as `abs(poly_exchange_ts - kalshi_exchange_ts)` when both are present. Fall back to 0 (no gap information) when either is None. Add `max_leg_fill_gap_ms` config field (default 2000ms per CONTEXT.md).

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/paper_trade/tracker.rs` -- settlement handling, JSONL logging, Prometheus metrics pattern
- Codebase analysis: `src/paper_trade/aggregator.rs` -- DailyAggregator accumulator pattern, export/import for checkpointing
- Codebase analysis: `src/paper_trade/position.rs` -- PaperPosition lifecycle, settlement P&L computation
- Codebase analysis: `src/settlement/types.rs` -- SettlementRecord, SettledLeg, OutcomeKind types
- Codebase analysis: `src/signal/types.rs` -- ThresholdStatus enum, ArbSignal structure
- Codebase analysis: `src/spread/patterns.rs` -- SpreadPattern, SpreadResult, ThresholdComponents
- Codebase analysis: `src/persistence/checkpoint.rs` -- CheckpointState versioning with serde(default)
- Codebase analysis: `src/metrics_export/mod.rs` -- Prometheus setup, existing histogram bucket configuration
- Codebase analysis: `Cargo.toml` -- dependency inventory confirming all needed crates present

### Secondary (MEDIUM confidence)
- `metrics` crate usage patterns verified across entire codebase (50+ existing gauge/counter/histogram calls)
- DailyAggregator checkpoint integration verified via Phase 15 implementation

### Tertiary (LOW confidence)
- None -- all findings verified against codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- zero new dependencies, all libraries already in use with verified patterns
- Architecture: HIGH -- direct extension of existing patterns (DailyAggregator, SettlementLogger, CheckpointState)
- Pitfalls: HIGH -- identified from concrete codebase analysis (ThresholdStatus gap, filtered signal tracking)
- Requirements mapping: HIGH -- each ANLZ requirement has a clear implementation path with existing infrastructure

**Research date:** 2026-02-26
**Valid until:** 2026-03-26 (stable domain, no external dependency changes)
