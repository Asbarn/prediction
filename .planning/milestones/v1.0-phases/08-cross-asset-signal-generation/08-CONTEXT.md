# Phase 8: Cross-Asset Signal Generation - Context

**Gathered:** 2026-02-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Compute spreads between options-implied probabilities (Phase 7 PricingEngine output) and prediction market prices (Phase 6 SpreadEngine data) for each mapped event. Generate ArbSignal outputs with full metadata. Apply configurable edge thresholds with static floor + dynamic adjustment. Complete the core arbitrage detection pipeline.

Does NOT include: execution, position sizing, risk management, or backtesting. Those are future phases.

</domain>

<decisions>
## Implementation Decisions

### Spread Computation
- Staleness gate: only compute spreads when both sides (options-implied prob and prediction market price) have data within a configurable freshness window
- Missing pairs: log at debug level and skip — no signal for unpaired instruments
- Directional with costs: compute both directions (buy prediction + sell options-implied, sell prediction + buy options-implied), subtract costs from each, report the profitable direction
- Confidence pass-through: compute spreads for ALL options-implied probabilities regardless of confidence score. Carry confidence into ArbSignal metadata. Let the threshold engine use confidence as one factor — do not gate on input

### Signal Output & Metadata
- Rich metadata: ArbSignal carries pricing method used, vol surface quality, solver convergence info, prediction market venue, book depth, IV spread — beyond the minimum required fields
- Channel + JSONL: emit on tokio mpsc channel for real-time consumers AND log to JSONL file for offline analysis (follows existing spread/trade logging pattern)
- Fixed configurable TTL: all signals get the same TTL from config (e.g., 30 seconds). Not dynamic for v1
- Full Prometheus metrics: counters for signals generated/filtered, histograms for edge size and confidence, gauge for active signal count

### Dynamic Thresholds
- Static floor + dynamic component: `max(static_floor, rolling_mean + k * rolling_stddev)` — already decided in project decisions doc
- Config: `min_edge_bps` (static floor, e.g., 100bps), `threshold_k` (multiplier), `rolling_window_seconds` (default 14400 = 4 hours)
- Liquidity penalty reduces effective edge (not threshold): `net_edge * liquidity_factor` where factor maps from book walker fill price vs top-of-book. Lives in cost_breakdown alongside fees and slippage — it's a measured quantity, not a tuning parameter
- Static floor during warmup: use only static floor until rolling window has sufficient history, then dynamic component kicks in
- No hysteresis: each cycle is independent. Signal emitted if edge > threshold at that moment. No state tracking of "active" signals

### Signal Lifecycle
- Emit new each time: every spread computation that passes threshold emits a fresh ArbSignal. Downstream handles dedup if needed
- Event-driven: recompute spread whenever either side updates (options prob or prediction market price). Immediate signal response, not periodic tick
- Same JSONL file with flag: all signals in one file with `threshold_status` field: "passed_both", "passed_static_only", "filtered". Simpler for analysis
- Periodic summary: log aggregate stats every N minutes (configurable) at info level — event coverage, signal rate, filter rate, mean edge

### Claude's Discretion
- Exact channel buffer sizes
- JSONL rotation/file naming conventions (follow existing patterns)
- Internal caching strategy for latest prices from each side
- Warmup threshold (how many data points before dynamic component activates)

</decisions>

<specifics>
## Specific Ideas

- Liquidity factor should map from the Phase 6 book walker's fill price computation: `(walked_fill_price - top_of_book) / top_of_book`. This is a measured quantity from existing infrastructure, not a new tuning parameter
- Paper trade P&L tracker (Phase 6) will consume signals — net_spread should reflect realizable edge so P&L comparison is straightforward
- Log signals that pass static floor even if they fail dynamic threshold. Annotate which threshold each signal passed. This gives Phase 9 replay data to evaluate threshold effectiveness
- The `arb_signal_net_edge_bps` Prometheus histogram should reflect capturable edge (after liquidity discount), not theoretical mid-price edge
- 90%+ of raw signals will likely vanish after costs (Pitfall 5 from research) — the static floor prevents noise from surfacing

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 08-cross-asset-signal-generation*
*Context gathered: 2026-02-23*
