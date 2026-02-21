# Pitfalls Research

**Domain:** Cross-venue crypto prediction market / options arbitrage
**Researched:** 2026-02-21
**Confidence:** HIGH (core pricing and settlement pitfalls), MEDIUM (API/operational pitfalls)

---

## Critical Pitfalls

### Pitfall 1: Risk-Neutral vs Real-World Probability Conflation

**What goes wrong:**
The system treats options-implied probabilities and prediction market prices as directly comparable, but they measure fundamentally different things. Options-implied probabilities from Deribit are risk-neutral (Q-measure) probabilities that embed a risk premium. Prediction market prices are also interpretable as risk-neutral probabilities, but under a completely different numeraire and risk structure. The systematic wedge between them is not an arbitrage opportunity -- it is the risk premium itself.

In crypto specifically, the volatility risk premium is large and time-varying. BTC options consistently price tail events higher than their real-world frequency (negative skew inflates OTM put prices, equivalently inflating the implied probability of downward moves). A naive system would see "Deribit says 35% chance BTC below $X, Polymarket says 25%" and call it a 10% edge. Much of that 10% is the volatility/jump risk premium that options sellers demand, not a mispricing.

**Why it happens:**
The mapping from options prices to event probabilities requires choosing between Q-measure (risk-neutral) and P-measure (real-world) probabilities. Most implementations extract N(d2) or a call-spread-implied probability and compare it directly to prediction market prices without adjusting for the risk premium wedge. Academic literature confirms these cannot be disentangled without a general equilibrium model or assumptions about the stochastic discount factor.

**How to avoid:**
- Never treat the raw options-implied probability as "the market's true belief." It is the risk-neutral probability, which systematically overstates bad-state probabilities and understates good-state probabilities.
- Frame the system as detecting deviations from the *historical* relationship between options-implied and prediction-market probabilities, not absolute mispricings. Build a running baseline of the typical wedge for each event type and flag only when the wedge deviates significantly from its own history.
- Consider using realized vol / historical event frequencies to estimate the risk premium component and subtract it from the raw options-implied probability before comparison.
- For v1 paper trading: log both raw and adjusted probabilities. Track whether raw signals are systematically biased in one direction (they will be).

**Warning signs:**
- Signals consistently favor one direction (e.g., always "buy protection" / always showing options-implied probability higher than prediction market price for downside events).
- Backtests show positive expected value before transaction costs but the "edge" is suspiciously stable across all market conditions -- this is the risk premium, not alpha.
- The signal magnitude correlates with VIX/DVOL levels rather than with actual mispricings.

**Phase to address:**
Phase 1 (Core Pricing Engine). This is foundational -- if the probability comparison is wrong, every downstream signal is garbage. Must be addressed before any signal generation.

---

### Pitfall 2: Settlement Basis Risk -- Different Venues Resolve "The Same Event" Differently

**What goes wrong:**
The system assumes that a Polymarket contract like "BTC above $100K on March 31" and a Deribit BTC option expiring on March 31 are economically equivalent. They are not. The settlement mechanisms differ in at least four critical dimensions:

1. **Settlement price calculation:** Deribit uses a 30-minute TWAP of 450 index samples (every 4 seconds) ending at 08:00 UTC. Prediction markets may resolve based on a specific spot price at a specific moment, "end of day" in an ambiguous timezone, or a "consensus of credible reporting."
2. **Index composition:** Deribit's BTC index is an equally-weighted average of mid-prices from selected exchanges, with outlier trimming (+/-0.5% from median). Polymarket's BTC price markets may reference a different set of exchanges or a specific price feed.
3. **Resolution ambiguity:** The Cardi B Super Bowl halftime case (Feb 2026) is the canonical example -- Kalshi settled at last-traded price ($0.26 YES) while Polymarket resolved YES at $1.00 for the same real-world event. For price-based events, ambiguity arises when BTC hovers near the strike at settlement time.
4. **Timing:** Deribit settles at 08:00 UTC. Prediction markets may resolve at end-of-day in US Eastern time, or whenever the resolution source publishes.

A $2 flash crash during Deribit's 30-min TWAP window could push the settlement price below a strike while the prediction market (resolving on a different price feed or time) shows BTC above the strike. Both sides of the "arbitrage" lose.

**Why it happens:**
Cross-venue comparison requires assuming settlement equivalence. The resolution details are buried in fine print that differs across platforms. Developers build for the happy path where both venues agree on the outcome.

**How to avoid:**
- Build a formal settlement specification for every tracked event pair. Document: resolution source, timing, price methodology, dispute mechanism, and edge-case handling for each venue.
- Classify event pairs by settlement basis risk: LOW (both use same price feed and time), MEDIUM (different methodology but same approximate time), HIGH (different time, different source, or ambiguous resolution).
- For price-based events: compute the probability that BTC is within the "danger zone" (close enough to the strike that settlement methodology differences matter). When this probability is high, widen the required spread before signaling.
- Never assume "close enough" -- a 1-hour timing difference during FOMC announcements can mean a 5%+ BTC price difference.

**Warning signs:**
- Backtests show occasional large losses on individual trades that looked like clear arbitrage opportunities.
- Events near the strike at expiry show inconsistent P&L despite correct directional calls.
- System generates signals on events where the prediction market resolution criteria are vague or use terms like "consensus of credible reporting."

**Phase to address:**
Phase 1 (Event Mapping) and Phase 2 (Signal Generation). Event mapping must capture settlement specs from day one. Signal generation must incorporate basis risk into position sizing.

---

### Pitfall 3: Naive Digital Option Pricing (N(d2) Under Skew)

**What goes wrong:**
The Black-Scholes formula gives the risk-neutral probability of a binary (cash-or-nothing) call finishing in-the-money as N(d2). This is only correct when implied volatility is constant across strikes. In reality, BTC options on Deribit exhibit significant volatility skew (negative skew in most market conditions, with OTM puts trading at higher implied vol than OTM calls).

Under skew, N(d2) computed at the ATM vol is systematically biased. The correct price of a digital option under skew includes a correction term proportional to the vega times the slope of the implied volatility smile (d(sigma)/dK). For negative skew, this means:
- Digital calls are MORE expensive than N(d2) suggests (the skew correction adds value)
- Digital puts are LESS expensive than N(d2) suggests

The magnitude of this error can be 2-5% in probability terms for strikes 1-2 standard deviations from ATM, which is larger than most arbitrage spreads you would be trading.

**Why it happens:**
N(d2) is the formula everyone learns first. Call spread replication (the correct approach) requires choosing a strike width, interpolating the vol surface between strikes, and handling the discretization error. It is tempting to skip this complexity in v1.

**How to avoid:**
- Use tight call spread replication: Price = [C(K - dK) - C(K + dK)] / (2 * dK), where each call is priced at its own interpolated implied volatility from the vol surface. Use dK = 1-2 strike spacings on Deribit (typically $125 for daily options, $250-$500 for weeklies).
- Build a proper vol surface interpolation. Deribit's strike grid is sparse, especially in the wings. Use SVI (Stochastic Volatility Inspired) parameterization or cubic spline interpolation on the (delta, vol) surface. Do not linearly interpolate between strikes in vol space -- this introduces butterfly arbitrage.
- Quantify the skew correction explicitly: compute N(d2) AND the call-spread price, log the difference, and monitor whether signals flip when using the corrected price. If they do, those signals were driven by pricing error, not real mispricing.
- Account for the discretization error in the call spread: for finite dK, the call spread underreplicates the digital near the strike (pays less than $1 when spot finishes between K-dK and K+dK). Apply the known correction or use 2-3 call spreads at different widths and average.

**Warning signs:**
- Implied probabilities extracted from options are systematically different from prediction market prices in the same direction for all events.
- The system shows more "opportunities" on far-OTM events (where skew is steepest and N(d2) error is largest).
- Backtest P&L is worse on events that settle near the strike (where the digital payoff discontinuity and vol smile curvature matter most).

**Phase to address:**
Phase 1 (Core Pricing Engine). This must be correct before signal generation. The vol surface construction and call spread replication should be the very first components built and validated.

---

### Pitfall 4: Stale Data Generating False Arbitrage Signals

**What goes wrong:**
During fast-moving events (FOMC announcements, ETF decisions, regulatory news), prediction markets can move 10-30% in seconds while options markets lag because:
1. Deribit options market makers widen spreads or pull quotes entirely during news events, leaving stale last-traded prices or wide bid-ask spreads.
2. Prediction market CLOB prices update instantly with aggressive taker orders while options book depth evaporates.
3. The system compares a fresh prediction market price against a stale options mid-price and sees a "huge" mispricing that does not actually exist.

The reverse also happens: a whale moves a thin prediction market book 5% in one trade, creating a temporary dislocation that reverts in seconds. The system signals an opportunity against stable options prices, but the prediction market price was the stale/manipulated one.

**Why it happens:**
Most implementations use a simple "compare latest prices from both venues" approach without modeling quote freshness, book depth, or the information content of price changes. Timestamps from different venues may have different latencies. A REST API poll hitting Deribit every 1 second and Polymarket every 500ms will systematically compare prices from different moments in time.

**How to avoid:**
- Attach timestamps and staleness scores to every data point. Define "stale" as: bid-ask spread > X% (options), or last trade > Y seconds ago, or book depth < Z (prediction market).
- Implement a staleness gate: never generate signals when either venue's data is classified as stale. Log these rejected signals separately -- they are valuable for understanding the data quality regime.
- Use Deribit WebSocket subscriptions (not REST polling) for real-time mark prices and book updates. Deribit mark prices update continuously even when the book is thin.
- For prediction markets: subscribe to the CLOB WebSocket for real-time book updates, not just last trade price.
- During known high-volatility windows (FOMC, CPI releases, ETF decisions), automatically widen the minimum spread threshold or suppress signals entirely for a configurable window (e.g., +/- 5 minutes around scheduled announcements).
- Cross-validate: if the prediction market moves 10% and options have not moved at all, the signal is staleness, not arbitrage. Require corroborating movement on both sides.

**Warning signs:**
- Signal frequency spikes dramatically during news events (these are almost all false positives).
- Signals cluster in the first 1-2 seconds after an event and then disappear (the "stale" side catches up).
- Backtest shows signals that "worked" only because the backtester used last-traded prices instead of executable bid/ask at time of signal.

**Phase to address:**
Phase 1 (Data Pipeline). Staleness detection must be built into the data layer before signals are generated. This is not a signal-quality filter bolted on later -- it is a core property of every market data point.

---

### Pitfall 5: Ignoring Transaction Costs and Liquidity Realities in Signal Evaluation

**What goes wrong:**
A 3% spread between venues looks like a clear signal. But after accounting for:
- Polymarket: dynamic taker fees up to 1.56% on crypto markets at 50/50 odds, plus gas fees on Polygon (~$0.01-0.05/trade)
- Deribit: 0.03% options taker fee (capped at 12.5% of option price for cheap options)
- Bid-ask spread on Deribit options: typically 5-15% of option price for OTM options, much wider in the wings
- Bid-ask spread on prediction markets: 1-5% for liquid events, potentially 10%+ for illiquid ones
- Slippage: executing size moves the price, especially on thin prediction market books with only $5K-$15K per side

...the 3% "edge" becomes negative. The system reports 100 "opportunities" per day, none of which would be profitable to execute.

**Why it happens:**
Building the signal detector is the fun part. Modeling execution costs is tedious but essential. Most implementations start with mid-price comparison and plan to "add costs later," but the costs fundamentally change which signals are real.

**How to avoid:**
- Model all-in execution cost from day one, even for paper trading. The signal should report: gross edge, estimated cost, and net edge. Only signals with net edge > minimum threshold (suggest 1% for v1) should be surfaced.
- For options: use the actual bid or ask price (not mid) depending on trade direction. A binary probability derived from call-spread bid prices will be very different from one derived from ask prices.
- For prediction markets: use book depth to estimate fill price at target size. A $1,000 position filled across 5 price levels is not the same as the best-bid price.
- Track Polymarket's fee curve: fees peak at 50/50 odds (1.56% max on crypto markets) and decrease toward extremes. This means the most "interesting" prediction market prices (near 50%) are also the most expensive to trade.
- Build a cost model that updates in real-time with current book state, not a static fee assumption.

**Warning signs:**
- Signal count drops by 90%+ when transaction costs are added (normal -- this is what should happen).
- The remaining signals cluster on highly liquid events with tight spreads (good -- these are the real opportunities).
- Backtest with mid-prices shows great returns; backtest with bid-ask shows losses (the entire "edge" was the spread).

**Phase to address:**
Phase 2 (Signal Generation). Costs must be integrated into signal evaluation, not treated as a separate concern. But the cost models depend on Phase 1 data quality.

---

### Pitfall 6: Options Expiry / Event Mismatch (No Hedgeable Instrument Exists)

**What goes wrong:**
The system identifies a prediction market event ("Will BTC be above $100K on April 15?") but there is no Deribit option expiring on April 15. The nearest options expire on April 11 (daily) and April 18 (weekly). The system either:
1. Uses the April 18 option as a proxy, introducing 3 days of additional price uncertainty that overwhelms the signal.
2. Attempts to interpolate between expiries, producing a synthetic probability that is model-dependent and unreliable.
3. Uses the April 11 option, which expires before the event and is useless.

For daily Deribit options, the strike spacing is $125, meaning the nearest available strike may be $60+ away from the prediction market's binary threshold. This discretization introduces pricing error.

**Why it happens:**
Prediction markets create events with arbitrary dates and thresholds. Options markets have fixed expiry calendars and strike grids. The overlap is imperfect. Developers assume "close enough" temporal and strike matching without quantifying the residual risk.

**How to avoid:**
- Build an explicit event-matching layer that pairs prediction market events with the best available options instruments. Score each pairing by: temporal gap (days between event resolution and nearest option expiry), strike gap (distance between event threshold and nearest available strike), and liquidity of the matched option.
- Define hard cutoffs: reject pairings where temporal gap > 1 day or strike gap > 2 strike spacings. These are not tradeable.
- For temporal mismatches: quantify the additional uncertainty using BTC's historical daily returns. A 3-day gap at 60% annualized vol means ~3.5% daily move, which translates to substantial probability uncertainty for near-ATM events.
- Maintain a live catalog of Deribit expiries and strike grids. New daily options appear with ~$125 spacing in a ~5% range around ATM. The catalog must update daily.

**Warning signs:**
- The system generates signals on events with no close option expiry match (these are phantom opportunities).
- Backtest assumes perfect expiry matching that does not exist in live markets.
- Signal quality degrades as you look at events further from standard options expiry dates.

**Phase to address:**
Phase 1 (Event Mapping). The expiry/strike matching logic must exist before signal generation. This determines the universe of tradeable events.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Use N(d2) instead of call spread replication | Simpler pricing code, one formula | 2-5% probability error under skew, systematic signal bias | Never -- the error is larger than typical edges |
| REST polling instead of WebSocket subscriptions | Simpler implementation, no connection management | Stale data, higher latency, more API rate limit consumption | Only during initial prototyping (<1 week), must migrate |
| Mid-price comparison without bid-ask modeling | More signals to analyze | False positives that would lose money on execution | Paper trading phase only, must add before any real trading |
| Static fee model (e.g., "assume 0.5% each side") | No need to integrate fee APIs | Misses dynamic fees (Polymarket peaks at 1.56%), misses spread variation | First week of development, then replace with live fee model |
| Single-threaded data collection | Simpler architecture | Cannot keep up with multiple venues' WebSocket feeds during high-vol events | Never for production -- use async from the start (Rust tokio) |
| Hardcoded event-option mappings | Quick to get first signal running | Breaks every time new options list or prediction markets change events | Initial prototype only, must build dynamic matching |
| Ignoring Deribit's 30-min TWAP settlement | Simpler settlement model | Wrong P&L calculation, wrong signal evaluation near expiry | Never -- the TWAP vs spot difference matters most when it matters most (near-strike settlements) |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Deribit WebSocket | Opening 32+ connections (hitting the per-IP limit) and getting silently disconnected | Use a single authenticated WebSocket connection with multiplexed subscriptions (up to 500 channels). Authenticate even for public data to get higher rate limits. |
| Deribit Rate Limits | Treating all endpoints equally at 20 req/s | Matching engine endpoints (order placement/cancel) are limited to ~5 req/s. Non-matching engine endpoints allow 20 req/s burst (100 request burst capacity, 10,000 credits/s refill). Different methods cost different credits. |
| Deribit Vol Surface | Using last-traded price for OTM options that haven't traded in hours | Use mark price (Deribit's model-based estimate) for illiquid options. Mark price updates continuously. Fall back hierarchy: mark price > NBBO mid > last trade. Deribit discards options with delta < 5% from its own DVOL calculation -- you should too. |
| Polymarket CLOB API | Sending market orders that cross the spread aggressively | Use limit orders (post-only where possible to earn maker rebates). Polymarket's batch order endpoint supports up to 15 orders per call. |
| Polymarket Fee Curve | Ignoring the probability-dependent fee structure | Fees use formula: `fee = C * feeRate * (p * (1-p))^exponent`. At 50/50 odds on crypto markets, taker fee is ~1.56%. At 90/10 odds, fee drops to ~0.14%. Factor this into signal evaluation. |
| Polymarket Settlement (International) | Assuming deterministic resolution | International Polymarket uses UMA Optimistic Oracle: proposal ($750 bond) -> 2hr challenge window -> possible DVM token vote (48-96 hrs). 98.5% resolve at first layer, but the 1.5% that don't can lock capital for days. |
| Kalshi API | Assuming same resolution as Polymarket for "same" events | Kalshi uses CFTC-registered source agencies (BLS for econ data, CF Benchmarks for crypto prices, leagues for sports). Resolution criteria are legally distinct from Polymarket's. The same real-world event can resolve differently. |
| Kalshi Settlement Edge Cases | Assuming binary YES/NO resolution | Kalshi Rule 6.3(c) allows settlement at last-traded price for ambiguous outcomes. This means your "YES at $1" payoff may become "YES at $0.26." |
| Polygon Gas | Assuming zero gas costs on Polygon | Gas costs are low (~$0.01-0.05/trade) but nonzero. During Polygon congestion (rare but possible), gas can spike. Approval transactions (6 one-time approvals per wallet) cost ~0.01 POL each. Must hold POL/ETH for gas. |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Polling REST APIs for price updates | Signals lag real-time by 1-5 seconds, rate limits consumed quickly | Use WebSocket subscriptions for both Deribit and Polymarket CLOB. REST only for infrequent metadata/catalog queries. | Immediately -- any latency-sensitive comparison fails with polling |
| Rebuilding full vol surface on every option price update | CPU spikes, pricing delays during fast markets, cascading staleness | Incremental vol surface updates: only refit the affected part of the surface when a single option updates. Cache the SVI parameters and refit locally. | When monitoring >20 options simultaneously (typical for covering multiple strikes/expiries) |
| Storing all tick data in memory without rotation | Memory grows unbounded during long-running sessions | Ring buffer for recent ticks (last N minutes), periodic flush to disk/database for historical analysis. Rust's bounded channels or VecDeque with capacity. | After 4-8 hours of continuous operation, depending on number of instruments |
| Synchronous cross-venue comparison | Blocks on the slower venue, misses fast-moving opportunities on the faster one | Async architecture with independent update streams per venue. Comparison triggers on ANY update from either side, using latest-known state from the other. | Immediately in volatile markets |
| Not rate-limiting outbound requests during reconnection storms | Deribit/Polymarket temporarily bans IP after reconnect attempts blow through rate limits | Exponential backoff on reconnection. Track credit budget. Queue requests during rate-limit recovery. | First network hiccup or exchange maintenance window |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Storing Deribit API keys in code or config files checked into git | Full account access (trading, withdrawal if enabled) | Use environment variables or a secrets manager. Deribit API keys support IP whitelisting -- enable it. Create read-only keys for data collection (separate from trading keys). |
| Using a single Polymarket wallet for all operations | Complete fund exposure if private key is compromised | Use separate wallets: one for deposits/holding, one for active trading with limited balance. Polymarket wallet is an EOA -- private key compromise means total loss. |
| Running on a US IP address for Polymarket trading | Account freeze, fund forfeiture | Polymarket restricts US users. Using VPNs violates ToS and risks account termination with forfeited balances. For v1 paper trading (signal-only), reading public market data should be fine from any jurisdiction, but verify current ToS. |
| Not validating WebSocket message integrity | Spoofed price data leading to bad signals | Verify message sequence numbers, check for gaps, validate price reasonableness against recent history. Deribit WebSocket supports heartbeat -- implement it. |
| Exposing the signal dashboard to the public internet | Competitors see your signals; operational information leaked | Bind to localhost only. If remote access needed, use SSH tunnel or VPN, not a public-facing web server. |

## "Looks Done But Isn't" Checklist

- [ ] **Vol surface construction:** Often missing butterfly/calendar arbitrage checks -- verify the interpolated surface does not admit negative butterfly spreads or negative forward variance
- [ ] **Call spread replication:** Often missing the dK choice validation -- verify the call spread width is narrow enough to capture the digital payoff accurately but wide enough that both strikes have liquid quotes
- [ ] **Event matching:** Often missing temporal basis check -- verify that the matched option actually expires within an acceptable window of the prediction market resolution date
- [ ] **Settlement model:** Often missing Deribit's TWAP methodology -- verify P&L calculations use 30-min TWAP settlement, not last-traded or spot price at 08:00 UTC
- [ ] **Staleness detection:** Often missing on the "slow" venue -- verify that signals are gated on BOTH venues having fresh data, not just the one that triggered the comparison
- [ ] **Cost model:** Often missing Polymarket's dynamic fee curve -- verify that fees are computed as `C * feeRate * (p*(1-p))^exponent`, not a flat percentage
- [ ] **Signal P&L attribution:** Often missing the distinction between "signal was correct" and "trade would have been profitable" -- verify that backtest tracks gross edge, costs, slippage, and net P&L separately
- [ ] **Prediction market resolution:** Often missing dispute/delay handling -- verify the system handles the case where a market takes 48-96 hours to resolve via UMA DVM escalation
- [ ] **Options data completeness:** Often missing OTM wing data -- verify that the vol surface handles missing/illiquid strikes gracefully (mark price fallback, not just dropping them)
- [ ] **Timezone handling:** Often missing UTC normalization -- verify all timestamps are in UTC internally, with explicit timezone conversion only at display/comparison points

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Risk-neutral/real-world conflation | MEDIUM | Retrofit a risk premium adjustment layer. Re-run backtests with adjusted probabilities. Historical signal accuracy data is still useful -- just reinterpret it. |
| Settlement basis risk (loss on ambiguous event) | HIGH | Cannot recover lost capital. Retroactively classify events by basis risk and filter future signals. Add the failing event type to a blocklist. |
| Naive N(d2) pricing | LOW | Replace pricing function with call spread replication. Vol surface may already exist. Re-run signals with corrected prices -- most infrastructure remains valid. |
| Stale data false positives | LOW | Add staleness gate to data pipeline. Existing signals can be retroactively filtered. No architecture change needed if data layer has timestamps. |
| Transaction cost blindness | LOW | Add cost model as a filter layer. Existing signals are re-evaluated with costs. The painful part is discovering that 90% of "signals" are not viable. |
| Expiry mismatch | MEDIUM | Build event-matching catalog. May require restructuring the event-to-instrument mapping. Existing signals for well-matched events remain valid. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Risk-neutral vs real-world conflation | Phase 1: Core Pricing | Run signal bias test: do signals systematically favor one direction? If yes, risk premium is leaking into signals. |
| Settlement basis risk | Phase 1: Event Mapping | For each event pair, document resolution specs for both venues. Score basis risk. Verify no HIGH-risk pairs are traded without widened thresholds. |
| Naive digital pricing (N(d2)) | Phase 1: Core Pricing | Compute N(d2) AND call-spread price for same event. If difference > 0.5%, the vol surface / replication is needed. |
| Stale data false signals | Phase 1: Data Pipeline | Inject synthetic staleness (delay one feed by 5 seconds) and verify system suppresses signals during the delay. |
| Transaction cost blindness | Phase 2: Signal Generation | Compare signal count with and without costs. Verify >80% reduction (if not, cost model is likely too generous). |
| Expiry/event mismatch | Phase 1: Event Mapping | Enumerate all prediction market events and their best option matches. Verify temporal gap, strike gap, and liquidity for each. |
| Polymarket dynamic fees | Phase 2: Signal Generation | Verify fee calculation matches Polymarket's published formula at p=0.5, p=0.1, p=0.9 reference points. |
| Deribit TWAP settlement | Phase 3: Backtesting/Validation | Back-compute Deribit settlement prices using historical index data and 30-min TWAP. Compare against using spot-at-expiry. Quantify the error. |

## Sources

### Settlement and Resolution
- [How Kalshi and Polymarket Settle Markets (and Disputes)](https://defirate.com/prediction-markets/how-contracts-settle/) -- MEDIUM confidence (verified with multiple sources)
- [SettleRisk - Resolution Risk Scoring](https://settlerisk.com/) -- LOW confidence (single source, but useful framework)
- [Cardi B Halftime Settlement Dispute](https://www.gamblinginsider.com/news/110468/kalshi-polymarket-cardi-b-halftime-settlement-cftc-complaint) -- HIGH confidence (multiple news sources confirm)
- [Steptoe: Risks of Ambiguity in Prediction Markets](https://www.steptoe.com/en/news-publications/its-not-on-the-house-the-risks-of-ambiguity-in-prediction-markets.html) -- HIGH confidence (law firm analysis)

### Digital Options Pricing and Skew
- [Quant Next: Binary Options Pricing, Replication and Skew Sensitivity](https://quant-next.com/binary-options-pricing-replication-and-skew-sensitivity/) -- HIGH confidence (quantitative analysis with formulas)
- [Quant Next PDF: Binary Options Replication](https://quant-next.com/wp-content/uploads/2024/11/Binary-Options_-Replication-and-Skew-Sensitivity.pdf) -- HIGH confidence
- [Field Recordings: Digital Options Pricing by Replication](https://fieldrecordings.wordpress.com/2011/01/07/digital-options-pricing-by-replication/) -- MEDIUM confidence
- [OpenGamma: Digital Forex Options](https://quant.opengamma.io/Digital-Forex-Options-OpenGamma.pdf) -- HIGH confidence (institutional quant library)

### Risk-Neutral vs Real-World Probabilities
- [Toward Black-Scholes for Prediction Markets (arXiv)](https://arxiv.org/html/2510.15205v1) -- MEDIUM confidence (preprint, not peer-reviewed, but rigorous framework)
- [FactSet: Mind Your Ps and Qs](https://insight.factset.com/mind-your-ps-and-qs-real-world-vs.risk-neutral-probabilities) -- HIGH confidence
- [Bank of England: Implied Risk-Neutral Probability Density Functions](https://www.bankofengland.co.uk/working-paper/1997/implied-risk-neutral-probability-density-functions-from-option-prices) -- HIGH confidence

### Deribit API and Options Structure
- [Deribit Rate Limits](https://support.deribit.com/hc/en-us/articles/25944617523357-Rate-Limits) -- HIGH confidence (official docs, though specific numbers could not be fetched directly)
- [Deribit Market Data Collection Best Practices](https://support.deribit.com/hc/en-us/articles/29592500256669-Market-Data-Collection-Best-Practices) -- HIGH confidence (official docs)
- [Deribit Connection Management Best Practices](https://support.deribit.com/hc/en-us/articles/25944603459613-Connection-Management-Best-Practices) -- HIGH confidence
- [Deribit Settlement](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) -- HIGH confidence (30-min TWAP from 450 samples confirmed)
- [Deribit Daily Options Launch](https://insights.deribit.com/exchange-updates/launch-of-btc-daily-options-on-deribit/) -- HIGH confidence ($125 strike spacing, ~5% range around ATM)

### Polymarket API and Fees
- [Polymarket Trading Fees Documentation](https://docs.polymarket.com/polymarket-learn/trading/fees) -- HIGH confidence (official docs, fee formula verified)
- [Polymarket Dynamic Fees for Latency Arbitrage](https://www.financemagnates.com/cryptocurrency/polymarket-introduces-dynamic-fees-to-curb-latency-arbitrage-in-short-term-crypto-markets/) -- MEDIUM confidence (news, confirmed by multiple outlets)
- [Polymarket CLOB API Overview](https://docs.polymarket.com/developers/gamma-markets-api/overview) -- HIGH confidence (official docs)

### Arbitrage Risks and Pitfalls
- [AInvest: Algorithmic Arbitrage in Crypto Prediction Markets](https://www.ainvest.com/news/algorithmic-arbitrage-crypto-prediction-markets-exploiting-binary-mispricings-polymarket-2512/) -- LOW confidence (single source, but domain-specific)
- [Risks and Pitfalls in Crypto Arbitrage Trading](https://coincryptorank.com/blog/risks-crypto-arbitrage) -- LOW confidence (blog)
- [BeInCrypto: Arbitrage Bots Dominate Polymarket](https://beincrypto.com/polymarket-arbitrage-risk-free-profit/) -- MEDIUM confidence

### Volatility Surface
- [Bitcoin Implied Volatility Surface from Deribit (Medium)](https://medium.com/coinmonks/bitcoin-implied-volatility-surface-from-deribit-70fba845102a) -- LOW confidence (blog, but implementation-relevant)
- [PMC: Implied Volatility Estimation of Bitcoin Options](https://pmc.ncbi.nlm.nih.gov/articles/PMC8418903/) -- HIGH confidence (peer-reviewed)

---
*Pitfalls research for: Cross-venue crypto prediction market / options arbitrage*
*Researched: 2026-02-21*
