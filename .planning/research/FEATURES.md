# Feature Landscape

**Domain:** Cross-venue crypto arbitrage signal quality validation -- diagnosing negative edge, validating instrument matching, tuning cost models, assessing liquidity, optimizing strike/expiry selection
**Researched:** 2026-03-09
**Confidence:** HIGH (codebase fully examined; root cause of negative edge identified in code; market structure researched)

**Scope note:** This research covers ONLY v1.8 signal quality validation features. The system is fully deployed at 42,732 LOC Rust with 4-venue feeds, cross-asset and prediction-market spread engines, paper trading, and production infrastructure. All 19,844 signals show ~-19.5 net edge. The goal is to understand why and fix it.

**Existing code this builds on:**

| Asset | Location | Status | v1.8 Relevance |
|-------|----------|--------|----------------|
| CrossAssetEngine | `src/signal/engine.rs` | Produces ArbSignal with cost breakdown | **Root cause**: cost model mixes probability-space spreads with notional-space costs |
| SpreadEngine | `src/spread/engine.rs` | Pairs Polymarket+Kalshi only | Not producing output (spread_logs empty) -- needs fixing |
| cost_model.rs | `src/spread/cost_model.rs` | polymarket_fee, kalshi_taker_fee, carry_cost | Computes fees on `filled_notional` ($500), producing ~$26 total cost vs ~0.08 probability-space spread |
| signal_scoring CLI | `src/bin/signal_scoring.rs` | Hit rate, edge t-test, Sharpe, PSR, drawdown | Needs data to work with; currently all signals filtered |
| spread_analytics CLI | `src/bin/spread_analytics.rs` | Distribution, hourly, venue-pair analysis | Needs spread_logs to be populated first |
| SpreadLogger | `src/spread/logger.rs` | JSONL logging for spread computations | **Known bug**: not producing output |
| EventRegistry | `src/events/registry.rs` | Maps cross-venue instruments via TOML | events.toml has `events = []` -- no active mappings |
| probability.rs | `src/pricing/probability.rs` | Call spread replication + N(d2) extraction | Working correctly; produces probabilities in 0-1 space |

---

## Table Stakes

Features without which v1.8 goals cannot be met. Missing = cannot determine if arb opportunities exist.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| **Cost model unit fix** | Root cause of -19.5 net edge: `net_edge = (raw_spread - total_cost) * liquidity_factor` where `raw_spread` is in probability space (0.08 = 8%) but `total_cost` is in notional dollars ($26.33). Fees computed on `filled_notional` ($500) produce dollar amounts subtracted from probability-space values. Must normalize everything to same unit space. | Medium | CrossAssetEngine, SpreadEngine | Options: (1) normalize costs to probability space by dividing by target_notional, (2) convert spread to dollar terms by multiplying by notional. Option 1 is cleaner since downstream (threshold, rolling stats) works in probability space. E.g., $26.33 / $500 = 0.0527 (5.27% cost), vs 0.08 (8%) raw spread = 0.0273 net edge -- actually plausible. |
| **Spread logger fix** | spread_logs directory is empty. SpreadEngine.log() is called but producing no output. Cannot run spread-analytics CLI without data. Must diagnose and fix. | Low | SpreadLogger | Likely a path or initialization issue. The `log_dir` defaults to "spread_logs" relative path. May be a cwd issue in Docker or the async logger may be silently failing. |
| **Signal data diagnostic CLI** | Need tooling to decompose the -19.5 edge into components: how much is fees, carry, slippage, basis risk? What are the raw spreads before costs? What instruments are generating signals? Current signal_scoring CLI operates on settlement data only, not signal logs. | Medium | Signal log JSONL schema | New CLI or extension: parse signal_logs, break down cost components, show per-event-id statistics, histogram of raw spreads vs costs. Data already logged in JSONL with full CostBreakdown. |
| **Event mapping population** | events.toml has `events = []`. No approved cross-venue mappings exist. Discovery pipeline runs but either no candidates match or none have been approved. Must understand what the discovery pipeline is finding and populate mappings. | Low-Med | Discovery pipeline, events.toml | Review discovery logs. Manually inspect Polymarket Gamma API + Deribit options chain for matching BTC strike/expiry pairs. Approve at least 3-5 mappings to generate meaningful signal data. |
| **Cost breakdown logging enhancement** | Current CostBreakdown in ArbSignal logs dollar amounts for each component. Need to also log the probability-space-normalized cost so analysis can verify the fix worked. | Low | Cost model unit fix | Add `total_cost_normalized` and per-component normalized fields. Or restructure to always work in one unit space. |

---

## Differentiators

Features that deepen the signal quality analysis beyond the minimum fix. Not required for basic validation but high value.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| **Instrument matching quality audit CLI** | Validates that cross-venue pairs actually represent the same economic bet. Compares: (1) strike prices match, (2) expiry dates align within tolerance, (3) settlement basis is compatible (index vs oracle), (4) contract direction (above/below) is consistent. Reports mismatches. | Medium | EventRegistry, events.toml with populated mappings | Critical for deep OTM problem: a Polymarket contract "BTC above $100K" paired with Deribit BTC-27JUN25-100000-C might look right but have different settlement mechanisms (Polymarket resolves at specific UTC time vs Deribit settles at 08:00 UTC index). Output: per-mapping confidence score and mismatch flags. |
| **Polymarket book depth analyzer** | Assesses real-world liquidity per instrument. Current data shows $0.001/$0.999 spreads on deep OTM contracts -- these are essentially illiquid markets where no real trading occurs. Need to identify which instruments have meaningful two-sided liquidity (tight spreads, depth > $100 at multiple levels). | Medium | Polymarket WS/REST data, new CLI or extension to spread-analytics | Metrics per instrument: best bid-ask spread, depth at top 3 levels, volume, update frequency. Instruments with >5 cent spreads or <$50 depth are not tradeable. Filter these from signal generation to eliminate noise. |
| **Near-the-money strike selector** | Deep OTM options have wide bid-ask spreads and low liquidity on both Deribit and prediction markets. ATM and near-the-money strikes have the tightest spreads and most liquidity. System should prefer strikes where BTC spot is within 10-20% of the strike. | Medium | Deribit market data, discovery pipeline | Current discovery matches ANY strike. $100K strike when BTC is at $85K is 18% OTM -- marginal. $200K strike when BTC is at $85K is 135% OTM -- untradeable. Add moneyness filter to discovery pipeline: only match instruments where moneyness (spot/strike ratio) is within configurable range (e.g., 0.8 to 1.2). |
| **Cost model sensitivity analyzer** | Tool that sweeps cost model parameters (fee rates, carry rate, holding days, slippage assumptions) and shows how net edge distribution changes. Identifies which parameter most impacts profitability. | Medium | Cost model unit fix, signal data | Parameterized re-computation: for each historical spread, recompute net edge with varied cost assumptions. Output: sensitivity table showing breakeven fee rate, breakeven carry, etc. |
| **Options fee model calibration** | Current options fee estimate: `taker_fee_rate * underlying_price * |delta|`. This computes the BTC-margined fee (Deribit charges in BTC). For a $85K BTC with delta=0.1, fee = 0.0003 * 85000 * 0.1 = $2.55 per contract. But the prediction market leg is $500 notional in USDC. The fee spaces are incompatible. Need calibration against actual Deribit fee schedule. | Medium | Deribit fee documentation | Deribit options taker fee: 0.03% of underlying or 12.5% of option price (whichever is lower), capped at that. Current formula ignores the cap and the "percentage of option price" alternative. |
| **Time-of-day and expiry-proximity analysis** | Research shows prediction market mispricings cluster during volatility events and near-expiry convergence. Analyze signal quality by hour-of-day and days-to-expiry to find windows where profitable signals concentrate. | Low-Med | Signal data with timestamps, event expiry dates | Extension to signal_scoring or spread_analytics: bucket edge by hour, by days-to-expiry. If edge concentrates at 2-6 hours before expiry, that is an actionable finding for position timing. |

---

## Anti-Features

Features to explicitly NOT build for v1.8.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Execution engine / order placement | v1.8 is diagnostic -- understand if opportunities exist before building execution. Building execution on unfixed signals wastes effort. | Fix cost model, validate signal quality, then v2 can add execution. |
| ML/AI signal prediction | The problem is a unit mismatch bug and instrument selection, not signal prediction accuracy. ML cannot fix a math error. | Fix the cost model arithmetic. Statistical analysis sufficient for go/no-go. |
| Real-time Grafana dashboard for diagnostics | Diagnostic analysis is inherently offline/batch. Adding real-time dashboards for a one-time investigation is overbuilt. | CLI tools producing JSON output. Pipe to jq or load into spreadsheet for ad-hoc analysis. |
| Database backend for analysis | Signal logs are JSONL files with ~20K records. Vec<T> in-memory analysis is faster and simpler than SQLite/DuckDB at this scale. | Continue JSONL + in-memory analysis pattern from v1.4. |
| New venue integration | Adding venues before understanding why current signals are unprofitable multiplies the diagnostic surface area. | Fix current pipeline first. New venues are v2+. |
| Automated parameter tuning / optimization | Premature optimization. Must first understand if ANY parameter set produces positive edge. If the entire market structure is unfavorable, no tuning will help. | Manual parameter sweeps via CLI tools. Human judgment on whether the market structure supports arb. |
| Polymarket on-chain analysis | Gas costs, MEV, and on-chain execution details are execution-phase concerns. v1.8 is signal validation only. | Defer to v2 execution planning. |

---

## Feature Dependencies

```
Cost Model Unit Fix (CRITICAL -- unblocks everything)
  |
  +-> Signal Data Diagnostic CLI (needs correct cost computation to be useful)
  |     |
  |     +-> Cost Model Sensitivity Analyzer (sweeps parameters on corrected model)
  |     +-> Time-of-Day/Expiry-Proximity Analysis (operates on correct edge values)
  |
  +-> Cost Breakdown Logging Enhancement (logs normalized costs)
  |
  +-> Options Fee Model Calibration (refines fee computation after unit fix)

Spread Logger Fix (independent of cost model, but critical for analytics)
  |
  +-> Spread Analytics CLI becomes usable (already built, needs data)

Event Mapping Population (independent -- enables real signal generation)
  |
  +-> Instrument Matching Quality Audit (validates populated mappings)
  |
  +-> Near-the-Money Strike Selector (filters discovery to viable instruments)
  |
  +-> Polymarket Book Depth Analyzer (assesses liquidity of matched instruments)
```

---

## Root Cause Analysis: The -19.5 Net Edge

**Confirmed by code inspection (HIGH confidence):**

In `src/signal/engine.rs` lines 396-471:

1. `raw_spread = options_prob - pred_ask` -- this is in probability space (e.g., 0.08 = 8 percentage points)
2. `prediction_fee = polymarket_fee(walk.filled_notional, ...)` -- `walk.filled_notional` is $500 (target_notional). Fee computed on $500: `500 * 0.25 * (0.47 * 0.53)^2 = 7.76`
3. `options_fee_estimate = 0.0003 * 85000 * 0.2 = 5.10` (or higher with larger delta)
4. `carry = 500 * 0.05 * 30/365 = 2.05`
5. `total_cost = 7.76 + 5.10 + 2.05 + 0 + 0.02 + 0 = 14.93` (in dollars)
6. `net_edge = (0.08 - 14.93) * 0.80 = -11.88` (or worse, explaining the ~-19.5 average)

**The fix:** Normalize all costs to probability space by dividing dollar costs by target_notional. `14.93 / 500 = 0.0299`, making `net_edge = (0.08 - 0.0299) * 0.80 = 0.0401` -- a plausible 4% edge before deeper analysis.

**Secondary issues (after the unit fix):**
- Empty events.toml: no approved instrument mappings means signals come only from test fixtures
- Signal log data shows `event_id: "test-both-directions"` -- these are test events, not live market data
- Spread logger not producing output -- prevents spread-level analysis
- Deep OTM instruments have negligible real-world liquidity

---

## MVP Recommendation

### Phase 1: Fix the Fundamentals (unblocks everything)

Prioritize:
1. **Cost model unit fix** -- normalize all cost components to probability space (divide by target_notional) in both CrossAssetEngine and SpreadEngine. This is the single most impactful fix.
2. **Spread logger fix** -- diagnose why spread_logs is empty; fix so spread-analytics CLI works.
3. **Event mapping population** -- review discovery pipeline output, manually approve 3-5 BTC strike/expiry mappings that have near-the-money strikes and visible liquidity.

Rationale: Until costs and spreads are in the same unit space, all signal analysis produces garbage. Spread logger and event mappings are prerequisites for generating real analysis data.

### Phase 2: Diagnostic Tooling

Prioritize:
1. **Signal data diagnostic CLI** -- decompose cost breakdown per event, per direction, per cost component. Answer "where does the edge go?"
2. **Instrument matching quality audit** -- validate that approved mappings represent the same economic bet (strike, direction, expiry alignment).
3. **Polymarket book depth analyzer** -- identify which instruments have real liquidity vs ghost $0.001/$0.999 books.

Rationale: After the cost model is fixed and real data flows, these tools answer the go/no-go question: "Do profitable arbitrage opportunities exist in current market structure?"

### Phase 3: Optimization (conditional on Phase 2 finding positive edge)

Prioritize:
1. **Near-the-money strike selector** -- filter discovery to strikes where liquidity actually lives.
2. **Options fee model calibration** -- refine Deribit fee computation with actual fee schedule (cap rules).
3. **Cost model sensitivity analyzer** -- find optimal parameter settings.

Rationale: Only worth building if Phase 2 confirms that positive edge exists after cost model fix. If all signals remain negative after the unit fix, the conclusion is that the market structure does not support this arb strategy at current spreads/costs, and optimization is moot.

### Defer:
- Time-of-day/expiry-proximity analysis (nice-to-have after go/no-go is answered)
- Cost breakdown logging enhancement (low priority if diagnostic CLI can parse existing logs)
- Execution engine (v2, contingent on v1.8 finding positive edge)

---

## Sources

- **Codebase inspection** (HIGH confidence): `src/signal/engine.rs` lines 396-471 (cost model), `src/spread/cost_model.rs` (fee formulas), `src/signal/config.rs` (default parameters), signal_logs/2026-03-09.jsonl (actual signal data)
- [Prediction Market Arbitrage Strategies: Cross-Platform Trading](https://ahasignals.com/research/prediction-market-arbitrage-strategies/) -- Cross-venue arb mechanics between Kalshi and Polymarket
- [Systematic Edges in Prediction Markets - QuantPedia](https://quantpedia.com/systematic-edges-in-prediction-markets/) -- Market structure and edge persistence
- [Arbitrage in Prediction Markets (IMDEA)](https://arxiv.org/abs/2508.03474) -- Academic analysis of $40M+ arb profits extracted from Polymarket
- [Capitalizing on Prediction Markets 2026](https://www.ainvest.com/news/capitalizing-prediction-markets-2026-institutional-grade-strategies-market-making-arbitrage-2601/) -- Institutional-grade arb strategy patterns
- [Polymarket CLOB Introduction](https://docs.polymarket.com/developers/CLOB/introduction) -- Order book architecture, unified YES/NO book
- [Polymarket /book stale data issue #180](https://github.com/Polymarket/py-clob-client/issues/180) -- Known stale order book data problem
- [Deribit Options Metrics](https://www.deribit.com/statistics/BTC/metrics/options) -- Strike-level open interest and liquidity data
- [Bitcoin Deep OTM Options Activity](https://www.coindesk.com/markets/2025/12/08/bitcoin-traders-target-usd20k-bitcoin-strike-as-deep-out-of-the-money-options-gain-traction) -- Deep OTM used for vol trading, not arb
- [Arbitrage Opportunities in Crypto Derivatives (ScienceDirect)](https://www.sciencedirect.com/science/article/pii/S138641812400048X) -- Cost model validation in crypto options

---
*Feature research for: v1.8 Signal Quality Validation*
*Researched: 2026-03-09*
