# Pitfalls Research

**Domain:** Signal quality validation and cost model tuning for cross-venue crypto arbitrage
**Researched:** 2026-03-09
**Confidence:** HIGH (based on direct codebase analysis of cost_model.rs, engine.rs, signal/engine.rs, probability.rs, book_walker.rs, and domain expertise in quantitative trading systems)

## Critical Pitfalls

### Pitfall 1: Instrument Mismatch Between Prediction Markets and Options (Binary vs Continuous Payoff)

**What goes wrong:**
The system compares options-implied probabilities (P(S > K) derived via Black-76 call spread replication) with prediction market binary contract prices. Both are "probabilities" in the range [0, 1], but they measure different economic events with different settlement mechanics. The system currently shows options-implied probability of 0.26% vs Polymarket probability of 50% -- a 192x gap that screams "these are not the same instrument" rather than "arbitrage opportunity." A Polymarket contract "BTC above $105,000 by March 28" has a specific UMA oracle resolution source and 2-hour dispute window. A Deribit BTC-105000-28MAR option settles at 08:00 UTC using Deribit's own BTC index with continuous payoff above strike. These are fundamentally different instruments despite fuzzy matching on asset/strike/direction.

**Why it happens:**
The FuzzyMatchKey (asset/strike/direction) correctly matches instruments by surface-level attributes. But it cannot detect when the matched instruments are answering fundamentally different questions. With BTC at ~$85,000, a $105,000 strike is 24% out-of-the-money. Options correctly price this as ~0.3% probability for a near-term March expiry. The Polymarket contract at 50% must be asking a qualitatively different question, have a much longer time horizon, or reflect a structurally different market (retail vs institutional).

**How to avoid:**
- Add an "instrument coherence score" that compares the probability gap magnitude. A probability ratio exceeding 10x should auto-flag as likely mismatch and suppress signal generation.
- Validate moneyness alignment: if options-implied prob < 5% and prediction market prob > 30%, the instruments almost certainly represent different events. Gate signal generation on probability coherence.
- Cross-check time horizons: the call spread replication uses `time_to_expiry` from the options chain. If the Polymarket contract's endDateIso implies a significantly different duration, the probabilities are not comparable.
- Log and alert on probability ratio per instrument pair. Instrument pairs with ratio > 5x should be quarantined, not traded.

**Warning signs:**
- Probability ratio between venues exceeds 10x consistently across all computations
- All signals on a pair are one-directional (all "buy prediction market" or all "sell prediction market")
- Net spread before costs is enormous (>20%) -- this signals invalid comparison, not profitable opportunity
- Options-implied probability near 0% or 100% while prediction market price is mid-range

**Phase to address:**
Phase 1 (Instrument Matching Quality Audit) -- must be resolved before any cost model tuning is meaningful. Invalid instrument pairs make all downstream analysis garbage-in-garbage-out.

---

### Pitfall 2: Overfitting the Cost Model to Make Signals Appear Profitable

**What goes wrong:**
When all 19,844 daily signals show negative edge (~-19.5 after costs), the temptation is to reduce cost parameters until signals turn positive. The system has 8+ tunable cost knobs: `annualized_rate` (default 5%), `reference_holding_days` (30), `fee_rate` (0.25), `exponent` (2), `taker_coefficient` (0.07), `basis_risk_scale` (0.01), `flat_rate_override` (None), `liquidity_penalty_scale` (0.02). Each has a defensible range, but tuning all toward lower costs simultaneously is multi-parameter overfitting. The TOML-driven config makes this dangerously easy -- just edit numbers and re-run analysis.

**Why it happens:**
The system generates the same signal on every computation cycle for each instrument pair. With ~5 ops/s and 4 spread patterns, the 19,844 signals/day are not 19,844 independent opportunities -- they are the same handful of instrument pairs recomputed thousands of times. If the underlying arb doesn't exist (because instruments are mismatched per Pitfall 1), no amount of cost reduction creates real edge. But the analysis tools (`signal-scoring` CLI) will dutifully report improving edge metrics as costs are lowered, creating a feedback loop of delusion.

**How to avoid:**
- Establish cost model bounds from empirical data BEFORE tuning. For Polymarket: verify fee formula against on-chain transaction records. For Deribit: verify taker fee from exchange documentation (currently 0.03%, which is correct for options). For carry: benchmark against actual USDC lending rates.
- The 30-day `reference_holding_days` is almost certainly wrong for near-expiry instruments. A BTC option expiring in 3 days has carry cost of $0.21, not $2.05. Make carry dynamic based on actual time-to-expiry.
- Never adjust more than one cost parameter at a time. Document the external evidence for each change.
- Add a "cost model sanity check" that computes the minimum trade notional needed for positive expected value given current costs. If minimum viable notional exceeds available liquidity, the trade is not viable regardless of signal quality.

**Warning signs:**
- Cost parameters adjusted downward without citing external market data
- After tuning, signals suddenly flip from all-negative to positive
- Ratio of cost components shifts dramatically (e.g., carry drops from 40% to 5% of total cost)
- Signal edge distribution shifts uniformly rather than for specific instrument pairs

**Phase to address:**
Phase 2 (Cost Model Validation) -- validate each cost component independently against real market data before drawing conclusions.

---

### Pitfall 3: Prediction Market Liquidity Illusion (Displayed Depth != Executable Depth)

**What goes wrong:**
The system uses `walk_the_book` (in `spread/book_walker.rs`) to compute fill prices from order book depth. But Polymarket CLOB books at extreme prices ($0.001/$0.999) consist almost entirely of resting limit orders that exist as cheap options positions, not as executable liquidity. The $500 notional fill computed by walk_the_book is fictional because:
1. Orders at $0.001 may be placed by market makers who cancel upon aggression (ghosting)
2. Polygon gas costs ($0.01-$5.00 per transaction) are not in the fee model
3. At extreme prices, the displayed depth has negligible economic value -- maker's capital commitment is near zero
4. The walk_the_book function treats all depth levels equally: `fill_at_level = remaining.min(size.into_inner())` with no quality filtering

**Why it happens:**
The book_walker was designed for competitive two-sided markets (Polymarket vs Kalshi) where both books have meaningful depth near mid-market. When used on deep-OTM instruments where one side quotes at $0.001, the walker produces a fill_ratio of 1.0 and an avg_fill_price of $0.001, which looks like "we can buy this entire position very cheaply." But there is no rational counterparty to take the other side of a clearly mispriced contract.

**How to avoid:**
- Add minimum price and maximum price filters: ignore depth levels below $0.02 or above $0.98 where liquidity is structural noise.
- Add a "book quality score" based on depth concentration, bid-ask spread width, and number of price levels. A book with a $0.001/$0.999 spread has zero information content.
- Add Polygon gas cost as a fixed per-trade cost component to the Polymarket leg. At $500 notional with $0.001 price, the entire position value is $0.50 -- gas alone exceeds this.
- Track actual trade history via Polymarket Gamma API. If no fills occurred in 24 hours on a contract, the depth is decorative.
- Cross-validate CLOB depth against REST /midpoint (already implemented in SourceCoordinator). Large divergence between REST midpoint and CLOB mid indicates phantom depth.

**Warning signs:**
- Polymarket bid/ask spread exceeds $0.10 (10% of contract value)
- Prices at $0.001 or $0.999 -- "nobody is actively trading this"
- fill_ratio returns 1.0 on extreme-price contracts (trivially consumed phantom depth)
- avg_fill_price equals best bid/ask exactly (no meaningful depth consumed beyond top-of-book)

**Phase to address:**
Phase 3 (Polymarket Liquidity Analysis) -- quantify real vs phantom liquidity per instrument before trusting signal quality numbers.

---

### Pitfall 4: Survivorship Bias in Instrument Selection

**What goes wrong:**
Analyzing the 19,844 daily signals while ignoring the larger universe of potential instrument pairs that were never monitored. If the system only tracks BTC-105000 (far OTM at ~$85K spot) and this pair generates awful signals, concluding "cross-venue arbitrage doesn't work" ignores that near-the-money strikes (BTC-85000, BTC-90000) might have genuine opportunities but were never instrumented. The events.toml is currently empty -- all approved events have expired or been archived. The signal data being analyzed comes from a non-representative sample of instruments.

**Why it happens:**
The discovery pipeline matches instruments across venues by asset/strike/direction, but instrument approval is manual (`approved = false` by default). If the approved set was selected during a period when BTC was near $100K, all those $105K strikes are now 24% OTM and have no informational content for near-term arbitrage. The system's instrument selection reflects historical market conditions, not current tradeable opportunities.

**How to avoid:**
- Before analyzing signal quality, map the full instrument universe: enumerate all Deribit strikes, all Polymarket BTC contracts, all Kalshi KXBTC events. Identify which pairs have structural arbitrage potential (similar moneyness, aligned expiry, adequate liquidity on both sides).
- Separate "the system's signal quality is poor" from "the system is looking at the wrong instruments." The former requires algorithm changes; the latter requires better instrument selection.
- Include coverage metrics: what percentage of the tradeable universe is the system monitoring? If 2 pairs out of 50 possible, the sample is meaninglessly small.
- Focus instrument selection on near-the-money strikes (delta 0.30-0.70) where both options liquidity and prediction market activity are highest.

**Warning signs:**
- Analysis conclusions drawn from fewer than 5 distinct instrument pairs
- All analyzed instruments have similar moneyness (all deep OTM or all deep ITM)
- No near-the-money instruments despite highest theoretical arb potential there
- Signal analysis covers less than one full options expiry cycle

**Phase to address:**
Phase 1 (Instrument Matching Quality Audit) and Phase 4 (Near-the-Money Strike Coverage).

---

### Pitfall 5: Confusing Prediction Market Prices with Probability Estimates

**What goes wrong:**
Treating Polymarket contract prices as unbiased probability estimates when they embed: (1) risk premium -- buyers of tail risk demand compensation, (2) liquidity premium -- illiquid contracts trade at discounts, (3) favorite-longshot bias -- extreme probabilities are systematically mispriced in prediction markets, (4) capital lockup premium -- USDC collateral locked from trade to settlement has opportunity cost, (5) venue-specific friction -- gas costs, withdrawal delays, KYC differences. The system computes `gross_spread = prediction_market_prob - options_implied_prob` and treats this as "edge." But a prediction market price of $0.50 doesn't mean 50% probability -- it means the marginal buyer and seller cleared at $0.50, which could reflect 40% true probability plus 10% structural premium.

**Why it happens:**
Options pricing under risk-neutral measure produces risk-neutral probabilities. Prediction market prices look like probabilities (range 0-1, binary payoff). The mathematical similarity hides the economic difference: options markets have market makers, leverage, hedging, and institutional participation. Prediction markets are retail-dominated with structural frictions that create systematic bias.

**How to avoid:**
- Add a "prediction market premium model" that estimates the non-probability component. At minimum, adjust for capital lockup cost: $500 locked at 5% for 30 days = $2.05 opportunity cost, which is built into the market price but not the probability.
- Compare prediction market prices across venues (Polymarket vs Kalshi on the same event) to estimate venue-specific premium. Price differences between venues for identical events are not "edge" but venue friction.
- Add favorite-longshot bias correction: prediction markets systematically overprice low-probability events and underprice high-probability events. This is well-documented in academic literature (Wolfers & Zitzewitz 2004, Manski 2006).
- Use the `method_disagreement` field from ProbabilityExtraction as a quality gate. High disagreement between call spread and N(d2) methods suggests the options-implied probability itself is unreliable.

**Warning signs:**
- Gross spread consistently positive in one direction across all instruments
- Spread magnitude correlates with contract illiquidity
- Prediction market prices are round numbers ($0.50, $0.25, $0.75) suggesting thin markets
- Cross-venue prediction market prices disagree by >5%

**Phase to address:**
Phase 2 (Cost Model Validation) -- model prediction market premium as a cost component.

---

### Pitfall 6: Missing On-Chain and Fixed Execution Costs in the Cost Model

**What goes wrong:**
The Polymarket fee model (`polymarket_fee` in cost_model.rs) computes `shares * fee_rate * (p * (1-p))^exponent` which captures the platform fee but completely misses: (1) Polygon gas costs per transaction ($0.01-$5.00), (2) USDC bridging costs from Ethereum mainnet ($5-50), (3) ERC-20 approval transaction costs, (4) withdrawal/off-ramp fees, (5) settlement collection transaction costs. For a round-trip $500 arb, these total $10-50 in fixed costs. The system's current cost breakdown shows carry (~$2.05) + venue fees (~$1-2) = ~$4 total cost, missing ~$20+ in execution costs.

**Why it happens:**
The system was designed for signal generation (v1.0), not execution. The cost model was built to be "directionally correct" for paper trading. But for v1.8 signal quality validation, an incomplete cost model produces misleadingly optimistic edges. Even if instrument matching is fixed and real arb opportunities exist, the missing $20+ in fixed costs per round trip means the minimum viable edge is ~4% on $500 notional -- much higher than most prediction market arb spreads.

**How to avoid:**
- Add a fixed per-trade cost component to the Polymarket leg: conservative estimate $2-5 per side for gas.
- Add a fixed per-trade cost for Deribit/Derive: minimum order fees and API interaction costs.
- Compute minimum viable notional: at what trade size does the fixed cost drop below 1% of notional? Answer: ~$500 per side for $5 gas. Below this, fixed costs dominate.
- Factor round-trip costs: entry + exit/settlement = at least 2 transactions per leg, 4 total for the arb.
- The carry cost default (5% annualized, 30-day hold) produces $2.05 on $500. But gas cost for 4 transactions at $2 each = $8. Gas is 4x the modeled carry but invisible in the current model.

**Warning signs:**
- Total computed cost per trade is < $5 on $500 notional (unrealistically low for on-chain execution)
- Carry cost dominates the cost breakdown (should be dwarfed by execution costs at $500 notional)
- Signal edge is positive but less than $10 per trade (likely consumed by unmodeled costs)

**Phase to address:**
Phase 2 (Cost Model Validation) -- add fixed per-trade cost components before any go/no-go conclusions.

---

### Pitfall 7: Statistical Analysis Errors in Signal Quality Evaluation

**What goes wrong:**
Multiple statistical mistakes compound when evaluating signal quality with the existing `signal-scoring` CLI:

1. **Non-independent observations:** The system computes ~5 signals/second on the same book state. 19,844 daily signals are not independent -- they are the same handful of instrument pairs recomputed thousands of times. The t-test in `EdgeTestResult` assumes independence, massively overstating statistical power.

2. **Multiple comparison problem:** Testing 4 spread patterns x N instrument pairs x 2 engines (Spread + CrossAsset) generates many hypotheses. Without Bonferroni or FDR correction, spurious significant results are expected.

3. **Look-ahead bias in threshold tuning:** Adjusting `static_floor` (default 1%), `k` (default 2.0), and `cold_start_multiplier` (default 2.0) based on historical signal performance, then evaluating on the same data, produces optimistically biased results.

4. **Wrong null hypothesis:** The edge t-test tests H0: mean_edge = 0. The relevant question is H0: mean_edge > total_execution_cost. Testing against zero inflates apparent significance.

5. **Sharpe ratio inflation:** The `SharpeResult` annualizes using 365.25-day year (correct for 24/7 markets) but if the daily "returns" are autocorrelated (same signal repeating), the annualized Sharpe is inflated by sqrt(autocorrelation factor).

**Why it happens:**
The signal-scoring CLI implements the math correctly (Wilson CIs, t-tests, Sharpe/PSR, drawdown). The code is sound. But the application context creates pitfalls: a single book update triggers recomputation of all 4 patterns, producing 4 non-independent "observations" from one market event.

**How to avoid:**
- Compute effective sample size: deduplicate signals sharing the same market snapshot timestamps. Independent observations = unique market state changes, not raw signal count.
- Apply Bonferroni correction when testing across multiple instrument pairs.
- Use out-of-sample validation: split signal history into training (tune thresholds) and test (evaluate performance) periods. Never tune and evaluate on the same data.
- Change null hypothesis to H0: mean_edge <= minimum_viable_edge (including all execution costs from Pitfall 6).
- Use block bootstrap (not IID bootstrap) for confidence intervals that respect temporal dependence.

**Warning signs:**
- P-values are extremely small (< 0.001) despite economically meaningless effect sizes
- Adding more data always improves statistical significance even though edge remains constant
- Results sensitive to the time window chosen for analysis
- Sharpe ratio exceeds 3.0 on daily data (extremely rare in any real strategy)

**Phase to address:**
Phase 5 (Statistical Analysis Framework) -- fix methodology before drawing conclusions.

---

### Pitfall 8: Kalshi Fee Ceiling Rounding May Be Grossly Overstating Costs

**What goes wrong:**
The `kalshi_taker_fee` function uses `Decimal::ceil()` when `use_ceiling = true` (the default). `Decimal::ceil()` rounds to the nearest integer ceiling. For a per-contract raw fee of $0.0175, `ceil(0.0175) = 1` (rounds to $1.00). This means 10 contracts cost $10.00 instead of the expected $0.175 -- a 57x overstatement. If Kalshi's actual rounding convention is "round up to nearest cent" ($0.02), the correct result would be $0.20 for 10 contracts.

**Why it happens:**
`Decimal::ceil()` in the rust_decimal library rounds to the nearest integer, not to the nearest cent. The test comment says "Kalshi rounds per contract" and the test expects ceil(0.0175) = 1, which is mathematically correct for integer ceiling. But this likely does not match Kalshi's actual fee schedule, where the minimum fee per contract is likely $0.01 (one cent), not $1.00 (one dollar).

**How to avoid:**
- Verify Kalshi's actual fee rounding convention from their exchange documentation.
- If rounding is to cents (likely), change to: `(per_contract_raw * Decimal::new(100, 0)).ceil() / Decimal::new(100, 0)` to round up to 2 decimal places.
- This single fix could reduce computed Kalshi costs by 50x for typical trades, significantly changing the signal quality picture for Polymarket-Kalshi spreads.
- Add a test case: 10 contracts at p=0.50 with coefficient 0.07 should produce fee of $0.20 (if cent-rounding), not $10.00 (if integer-rounding).

**Warning signs:**
- Kalshi fees dominate the cost breakdown when Kalshi is the buy/sell side
- Spreads appear profitable on Polymarket-Deribit pairs but never on Polymarket-Kalshi pairs
- Per-trade Kalshi fee exceeds the contract notional value

**Phase to address:**
Phase 2 (Cost Model Validation) -- verify against Kalshi exchange documentation. This may be a significant source of the -19.5 negative edge.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Using default cost parameters without market validation | Ships faster, directional estimates | All edge calculations unreliable, false go/no-go conclusions | Only during v1.0 development. Must validate before v1.8 go/no-go decisions |
| Static 30-day carry period | Simple, conservative | Overstates carry for short-dated instruments (1-3 day expiry has $0.07 carry, not $2.05), understates for long-dated | Replace with dynamic carry based on time-to-expiry in Phase 2 |
| Treating fill_ratio=1.0 as "fully executable" | Avoids building execution simulation | At extreme prices, 100% fill is meaningless because entire displayed depth is phantom | Never acceptable for signal quality validation. Phase 3 must add depth quality scoring |
| Rolling window stats (4-hour default) without regime detection | Responsive to changing conditions | During BTC moves >5%, rolling window mixes pre-move and post-move observations, producing meaningless statistics | Acceptable for initial deployment; needs regime detection for v1.8 |
| Autocorrelated signal counting as independent observations | More data points, smaller CIs | Overstates statistical power by 10-100x, leading to false conclusions of significance | Never acceptable for signal quality validation |
| No on-chain cost modeling | Simpler cost model | $20+ unmodeled costs per round-trip make all edge calculations optimistic by $20 | Only during paper trading phase; must add before any capital deployment decision |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Polymarket CLOB depth at extreme prices | Treating orders at $0.001/$0.999 as executable liquidity | Filter depth levels outside $0.02-$0.98. Add depth quality threshold. Cross-check against REST /midpoint |
| Deribit options for far-OTM strikes | Using call spread replication with large epsilon, producing unstable probabilities | Check `method_disagreement`. When epsilon > 10% of strike, N(d2) is more reliable but still noisy. Far-OTM probabilities inherently imprecise |
| Kalshi fee ceiling rounding | `Decimal::ceil()` rounds 0.0175 to 1 (integer), not to 0.02 (cent). 57x cost overstatement | Verify Kalshi's actual rounding convention. Likely needs cent-level ceiling, not integer-level |
| Cross-venue timestamp comparison | Treating Polymarket block timestamps, Kalshi REST poll timestamps, and Deribit WebSocket timestamps as synchronized | Add clock skew tolerance. Polymarket blocks: 2s cadence. Kalshi REST: up to 10s poll lag. Staleness gate may incorrectly reject valid Kalshi data |
| Prediction market settlement resolution | Assuming Polymarket and Kalshi resolve identically. Polymarket uses UMA oracle with 2-hour dispute window. Kalshi uses CFTC-regulated same-day settlement | Model settlement timing difference as cost/risk, not just basis_risk_premium. Different resolution sources can disagree on outcome |
| Deribit options settlement time | Options settle at 08:00 UTC using Deribit index. Prediction markets may resolve at midnight UTC or end-of-day | Time-zone and settlement-hour mismatches create carry cost asymmetry not captured by flat carry model |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Computing all 4 spread patterns for clearly invalid instrument pairs | 19,844 signals/day all with -19.5 edge | Add instrument quality gate before spread computation. Skip pairs with probability ratio > 10x | Already broken -- 100% of signal compute wasted on invalid pairs |
| Rolling stats accumulating on permanently negative-edge instruments | Dynamic threshold adapts to "everything is bad" and loses discriminative power | Reset rolling stats when instrument pair is recalibrated. Gate rolling stats on minimum edge quality | Broken at steady state |
| JSONL signal logs growing unbounded | ~50MB/day disk usage, filling EBS within months | Add log rotation. Consider sampling: 100% of threshold-passing, 10% of filtered signals | ~30 days without rotation on default 8GB EBS |
| Spread logger producing no output (known bug) | spread_logs directory is empty, spread-analytics CLI has no data to analyze | Fix spread logger before running spread analysis. Verify file creation and write-through | Currently broken |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Tuning cost model to show profitability without external validation, then using for capital deployment go/no-go | Financial loss from delusional cost assumptions | Require every cost parameter change to cite external evidence. Version control config changes with justification |
| Trusting prediction market displayed depth for position sizing | Opening positions based on phantom liquidity, unable to exit at expected prices | Never size positions based on displayed depth alone. Assume 50% depth degradation minimum |
| Drawing statistical conclusions from autocorrelated data and deploying capital | False confidence in strategy viability, actual Sharpe could be 5-10x lower than computed | Compute effective sample size before any significance test. Use block bootstrap for CIs |

## "Looks Done But Isn't" Checklist

- [ ] **Cost model validation:** Often missing on-chain execution costs (gas, bridging, approvals) -- verify by computing round-trip cost for a $500 arb including ALL transactions (should be $20+, not $4)
- [ ] **Instrument matching:** Often matching by surface attributes without verifying probability coherence -- verify that probability ratio between matched instruments is < 5x
- [ ] **Kalshi fee rounding:** Code uses `Decimal::ceil()` which rounds to integer, possibly 57x overstatement -- verify against actual Kalshi fee schedule
- [ ] **Statistical significance:** Often reported without correcting for autocorrelation -- verify by computing effective sample size (unique market state changes, not raw signal count of 19,844)
- [ ] **Liquidity analysis:** Based on displayed depth without testing executability -- verify by checking for actual fills on the contract in last 24 hours via Polymarket Gamma API
- [ ] **Spread logger output:** spread_logs directory currently empty (known bug) -- verify spread logger produces data before running spread-analytics CLI
- [ ] **Settlement data coverage:** Signal scoring requires settled outcomes -- verify sufficient settled signals exist before drawing statistical conclusions
- [ ] **Cross-asset vs prediction-market-only analysis:** SpreadEngine pairs prediction markets with each other; CrossAssetEngine pairs options with prediction markets. Ensure correct engine output is analyzed for cross-asset arbitrage thesis
- [ ] **Carry cost accuracy:** 30-day default may be 10x wrong for near-expiry instruments -- verify carry is computed from actual time-to-expiry

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Instrument mismatch deployed to production | LOW | Update events.toml with corrected mappings. System picks up config changes dynamically. No code change needed |
| Overfitted cost model used for go/no-go | MEDIUM | Re-run signal analysis with validated parameters. Compare old vs new edge distributions. Document calibration methodology |
| Capital deployed based on phantom liquidity | HIGH | Cannot recover lost spread from positions opened at displayed prices but unable to close. Prevention is the only strategy |
| Statistical conclusions from autocorrelated data | LOW | Re-run analysis with effective sample size correction. Block bootstrap for CIs. Results may change from "significant" to "inconclusive" |
| Kalshi fee rounding error (57x overstatement) | LOW | Fix ceiling rounding to cent-level. Re-run all Kalshi-involved signal analysis. May reveal viable opportunities previously hidden |
| Gas cost spike during live arb execution | MEDIUM | Add gas price oracle check before execution (v2). Set max gas threshold. Abort trade if gas exceeds edge |
| Missing on-chain costs discovered after capital deployment | HIGH | Cannot retroactively fix trades executed without proper cost accounting. All P&L lower than projected |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Instrument mismatch (binary vs continuous) | Phase 1: Instrument Matching Quality Audit | Probability ratio < 5x for all approved pairs; coherence score > 0.7 |
| Cost model overfitting | Phase 2: Cost Model Validation | Each parameter justified by external evidence; total cost validated against exchange fee schedules |
| Polymarket liquidity illusion | Phase 3: Polymarket Liquidity Analysis | Depth quality score per contract; phantom depth filtered; gas costs included in model |
| Survivorship bias in instrument selection | Phase 4: Near-the-Money Strike Coverage | At least 3 distinct moneyness buckets covered; coverage metrics tracked |
| Prediction market premium confusion | Phase 2: Cost Model Validation | Cross-venue premium estimated from Polymarket vs Kalshi comparison; favorite-longshot bias adjustment added |
| Missing on-chain/fixed costs | Phase 2: Cost Model Validation | Fixed per-trade cost component added; round-trip cost > $15 for on-chain execution |
| Statistical analysis errors | Phase 5: Statistical Analysis Framework | Effective sample size computed; multiple comparison correction applied; out-of-sample validation |
| Kalshi fee ceiling rounding | Phase 2: Cost Model Validation | Kalshi fee verified against exchange docs; test with expected cent-rounding result |

## Sources

- Direct codebase analysis: `src/spread/cost_model.rs` (fee formulas), `src/spread/engine.rs` (spread computation pipeline), `src/signal/engine.rs` (cross-asset signal generation), `src/spread/config.rs` (cost parameter defaults), `src/pricing/probability.rs` (options-implied probability extraction), `src/spread/book_walker.rs` (order book walking), `src/analysis/scoring.rs` (statistical analysis)
- Polymarket fee structure: dynamic formula with exponent=2 for crypto, fee_rate=0.25 (from code defaults, HIGH confidence)
- Kalshi fee structure: taker coefficient 0.07 with ceiling rounding (from code, needs verification against exchange docs, MEDIUM confidence)
- Deribit taker fee: 0.03% (0.0003) for options (from code default, matches Deribit public fee schedule, HIGH confidence)
- Polygon gas costs: $0.01-$5.00 per transaction (MEDIUM confidence, network-condition-dependent)
- Prediction market pricing biases: favorite-longshot bias, liquidity premium (Wolfers & Zitzewitz 2004, Manski 2006, HIGH confidence for existence of bias)
- Statistical methodology: multiple comparison corrections, effective sample size for autocorrelated series, block bootstrap (standard statistical practice, HIGH confidence)
- rust_decimal `Decimal::ceil()` behavior: rounds to nearest integer ceiling (verified from crate documentation, HIGH confidence)

---
*Pitfalls research for: Signal quality validation in cross-venue crypto arbitrage*
*Researched: 2026-03-09*
