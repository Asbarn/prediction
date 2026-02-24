# Phase 6: Prediction Market Spreads - Context

**Gathered:** 2026-02-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Cross-platform prediction market arbitrage detection between Polymarket and Kalshi. Computes fee-adjusted net spreads for all 4 directional patterns, logs every spread computation for distribution analysis, tracks hypothetical paper trade P&L, and exports key metrics to Prometheus. This phase delivers the first actionable trading signals.

</domain>

<decisions>
## Implementation Decisions

### Signal thresholds
- Static floor + dynamic component: `max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty`
- Spread distribution is the primary dynamic signal (statistical unusualness)
- Liquidity depth acts as a cost adjustment — thin books raise the threshold via an inverse-depth penalty
- All parameters (static_floor, k, penalty scaling) configurable in TOML
- Log all threshold components (static floor, rolling mean, rolling stddev, k*sigma, liquidity penalty, final threshold) for post-hoc evaluation of which factor drives useful signals
- Rolling window: configurable, default 4 hours — short enough for regime adaptation (FOMC, ETF news, weekend/weekday shifts), long enough for meaningful sample size
- Start with single window; design allows adding multiple windows (1h/4h/24h) later
- No cooldown or deduplication — fire every threshold crossing, deduplication is a downstream concern

### Cost model approach
- Walk the book for a configurable fixed notional size (e.g., $500) to compute average fill price — not top-of-book + flat penalty
- Polymarket fees: implement exact dynamic fee formula from their docs + TOML override to swap in flat rate for comparison
- Kalshi fees: 7% profit fee (from their current structure)
- Include carry cost: configurable annualized rate prorated by expected holding period, penalizing longer-dated positions
- Basis risk is a SEPARATE concern — not folded into the cost model. BasisRiskScore from Phase 5 is metadata/filter on the signal, not a cost component
- Both legs must pass staleness gate before spread computation proceeds

### Paper trade rules
- Configurable fixed notional per trade (TOML), leave room for Kelly/edge-proportional sizing later
- Entry: fill at next tick after signal fires (not at signal-time quote) — captures some adverse selection
- Track both hold-to-settlement AND mark-to-market over time, so both settlement P&L and early-exit (spread reversion) strategies can be analyzed post-hoc
- P&L aggregation: per-signal individual trade P&L + daily rollups. Weekly can be derived offline
- Log mark-to-market values over position lifetime for later strategy comparison

### Claude's Discretion
- Logging and metrics design (what goes to file vs Prometheus vs stdout)
- Prometheus metric naming and label conventions
- Exact data structures for spread computation pipeline
- Aggregate statistics implementation (mean, stddev, percentiles)

</decisions>

<specifics>
## Specific Ideas

- Threshold formula: `max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty` — user specified this exact structure
- "Log the components so you can later evaluate which factor was doing the useful work" — observability into threshold decision-making is important
- 4-hour default rolling window chosen specifically because "a spread distribution from overnight low-vol hours shouldn't be setting your thresholds during a daytime liquidation event"
- Next-tick fill for paper trades (not instant) to capture adverse selection reality
- Dual outcome tracking (settlement + mark-to-market) enables offline comparison of hold vs reversion strategies

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 06-prediction-market-spreads*
*Context gathered: 2026-02-23*
