# Domain Pitfalls

**Domain:** CLI-based statistical analysis tooling (spread analytics + signal scoring) for an existing Rust cross-venue arbitrage signal generator (v1.4)
**Researched:** 2026-02-28
**Confidence:** HIGH (statistical methodology, codebase analysis), MEDIUM (JSONL scaling under soak test volumes, Sharpe annualization for binary events)

---

## Critical Pitfalls

Mistakes that produce misleading go/no-go conclusions, cause incorrect capital allocation decisions, or require metric recomputation from scratch.

### Pitfall 1: Sharpe Ratio Annualization Invalid for Binary Event Outcomes

**What goes wrong:**
The standard Sharpe ratio formula annualizes using `sqrt(N)` where N is the number of return periods per year. This assumes returns are IID (independent and identically distributed) with roughly normal distribution. Binary prediction market arb outcomes violate both assumptions:

1. **Returns are bimodal, not normal.** Each position resolves to either a win (event settled correctly) or a loss (event settled incorrectly or costs exceeded edge). The return distribution has two peaks separated by the cost basis, not a smooth bell curve. Standard deviation of a bimodal distribution overstates the "risk" relative to what `sqrt(N)` annualization expects.

2. **Returns are NOT IID.** Positions on the same event are correlated (all BTC-100K-June positions resolve together). Positions clustered around the same expiry date resolve simultaneously, creating return clustering that violates independence. The `sqrt(N)` annualization presumes each period's return is independent of every other period's return.

3. **N is ambiguous.** Trading frequency is irregular -- signals fire when spreads exceed thresholds, not on a fixed schedule. There is no natural "period" to annualize from. Using daily returns when there are days with zero signals produces zero-return periods that artificially compress the mean and inflate the denominator.

4. **Small sample amplification.** With BTC binary events expiring weekly to monthly, the soak test period (weeks to low-single-digit months) produces perhaps 10-50 settled positions. Annualizing a Sharpe ratio from 30 observations using `sqrt(252/T)` creates false precision.

**Why it happens:**
Sharpe ratio is the default "how good is this strategy" metric. Every quant tutorial teaches `mean(returns) / std(returns) * sqrt(annualization_factor)`. It is easy to compute and easy to compare. But the standard formulation assumes continuous, normally distributed, IID returns -- all of which are violated by binary event arb.

**Consequences:**
- Annualized Sharpe of 2.5 from 30 binary outcomes creates false confidence in strategy quality
- A "go" decision for v2 execution based on a statistically meaningless metric
- Comparing this system's Sharpe against continuous trading strategies (where Sharpe 2+ is exceptional) misframes the risk profile

**Prevention:**
1. **Report the realized (non-annualized) Sharpe ratio** as the primary metric. Compute `mean(per-trade-returns) / std(per-trade-returns)` without any `sqrt(N)` scaling. Label it "per-trade Sharpe" or "realized Sharpe" to avoid confusion.
2. **Accompany Sharpe with a Probabilistic Sharpe Ratio (PSR)** that accounts for estimation error. PSR answers "what is the probability that the true Sharpe exceeds a benchmark?" given the sample size, skewness, and kurtosis. The `statrs` crate (already a dependency) provides the normal CDF needed for PSR.
3. **If annualized Sharpe is computed at all**, use the actual number of settled trades per year (not `sqrt(252)` or `sqrt(365)`). Document the annualization factor explicitly: "annualized using 47 trades/year equivalent."
4. **Report Sortino ratio alongside Sharpe.** Sortino uses downside deviation only, which is more meaningful for bimodal returns where upside "volatility" is actually a feature.
5. **Display the raw return histogram** in CLI output so the operator can visually assess whether the distribution is remotely normal.

**Detection:**
- Annualized Sharpe > 3.0 from fewer than 100 trades is almost certainly a spurious artifact
- The CLI should emit a warning if annualization factor exceeds `2 * actual_trade_count`
- Check if removing the top 2 winning trades drops the Sharpe by more than 50% (indicates extreme sensitivity to outliers)

**Phase to address:** Must be handled in the signal scoring CLI design. The choice of metrics and their computation methodology should be finalized before implementation begins.

---

### Pitfall 2: Survivorship Bias From Analyzing Only Settled Positions

**What goes wrong:**
The signal scoring CLI naturally analyzes positions that have settled (resolved) because those have definitive outcomes. But this systematically excludes:

1. **Positions still open at analysis time.** Long-dated events (monthly expiries) that have not yet resolved are excluded, even if they have been running at a loss for weeks. The analysis only sees the short-dated events that have already resolved, which may have systematically different characteristics (closer to expiry = less time for convergence = different edge profile).

2. **Positions that timed out.** The settlement monitor has a `PollingTier::TimedOut` state for events that did not resolve within the configured window (168 hours / 7 days). These represent unresolved positions -- potentially large losses -- that are excluded from "settled" analysis.

3. **Filtered signals.** The `FilteredSignalTracker` in `analyzer.rs` tracks signals with `ThresholdStatus::PassedStaticOnly` or `ThresholdStatus::Filtered`. These are signals that WOULD have been trades but were filtered by the dynamic threshold. Analyzing only `PassedBoth` signals ignores potentially profitable signals that were filtered out (or confirms that filtering works, if the filtered signals would have been losers). The `FilteredSignalTracker` already tracks this, but the CLI must use it.

4. **Events where one venue leg settled but the other did not.** The `PaperPosition` tracks per-leg settlement. If Deribit settled but Polymarket has not yet (due to dispute/delay), the position is partially settled. Counting it as unsettled excludes it; counting it as settled with only one leg misrepresents the outcome.

**Why it happens:**
Only settled positions have a definitive P&L. It is natural to compute hit rate and edge from positions with known outcomes. But the set of settled positions is not a random sample of all signals -- it is biased toward shorter-duration events, events without disputes, and events on venues with faster settlement.

**Consequences:**
- Hit rate computed from fast-settling events overstates actual system performance
- Missing timeout positions understates worst-case drawdown
- Filtered signal analysis (the most valuable part of the CLI) is skipped entirely if not explicitly included
- The go/no-go decision is based on a cherry-picked subset of outcomes

**Prevention:**
1. **Report "settled" and "total" separately.** The CLI output should show: "42 settled out of 67 total positions. 3 timed out. 22 still open."
2. **Include timed-out positions as losses.** A position that timed out after 7 days of polling likely represents a problematic event. Count it as a loss equal to the entry cost (fees + slippage), which is the minimum guaranteed loss on any position.
3. **Report filtered signal outcomes.** Using the `FilteredSignalTracker` data, show: "Of 150 filtered signals, X% would have been profitable, Y% would have been losers." This directly measures threshold effectiveness.
4. **Compute "worst case" metrics** that count all open positions as maximum-loss outcomes. This provides a floor for the strategy's quality.
5. **Require a minimum settlement ratio** before trusting metrics. E.g., "at least 80% of positions must be settled before metrics are actionable."

**Detection:**
- If `total_positions - settled_positions > 0.2 * total_positions`, emit a prominent warning
- If `timeout_count > 0`, always display it -- even one timed-out position is significant
- Compare filtered signal hit rate against passed signal hit rate. If they are similar, the threshold is not adding value

**Phase to address:** Must be addressed in the signal scoring CLI's data loading phase. The data model must include all position states, not just settled.

---

### Pitfall 3: Confidence Intervals Misleading With Small Samples (n < 30)

**What goes wrong:**
The system's BTC binary events expire on Deribit weekly or monthly. A soak test of 4-8 weeks might produce 10-30 settled positions per venue pair. Computing confidence intervals on proportions (hit rate) and means (average edge) with these sample sizes produces intervals so wide they are useless, OR uses methods that produce misleadingly narrow intervals:

1. **Normal approximation for hit rate CI.** The Wald interval `p +/- z * sqrt(p*(1-p)/n)` fails for small n, especially near p=0 or p=1. With n=15 and p=0.80 (12/15 wins), the Wald 95% CI is [0.60, 1.00] -- which overshoots 1.0 and must be clamped. Even clamped, it conveys false precision about a rate that could easily be 0.50 with a different sample.

2. **Student's t-distribution for edge CI.** With n=15, using `t(14)` for a CI on mean edge is correct in theory but the interval will be approximately 2x wider than a normal interval, often spanning zero. An edge of 0.02 +/- 0.03 (CI includes zero) is genuinely inconclusive, but operators may interpret the point estimate (0.02) as meaningful.

3. **Bootstrap CIs require n >= 20 minimum.** With n=10, bootstrapping resamples from 10 observations. The bootstrap distribution itself has high variance and the resulting CI is unreliable.

**Why it happens:**
Small samples are inherent to the domain. BTC binary events are not high-frequency -- each event maps to one position, and events expire weekly or monthly. The only way to get large n is to run the soak test for months.

**Consequences:**
- Confidence intervals that include zero for every metric, making the entire analysis "inconclusive"
- Alternatively, using normal approximation that produces artificially narrow intervals
- Operator loses trust in the analysis tool ("it always says inconclusive") and relies on point estimates instead

**Prevention:**
1. **Use Wilson score intervals for proportions (hit rate, false positive rate).** Wilson score is accurate for n >= 10 and does not overshoot [0, 1]. The formula is well-defined even at p=0 or p=1. Implement directly -- it requires only basic arithmetic and the z-score from `statrs::distribution::Normal`.
2. **Use bootstrap percentile intervals for means (edge, P&L) when n >= 20.** For n < 20, use the Student's t interval but label it prominently: "Warning: n=15, CI is unreliable."
3. **Display sample size prominently next to every CI.** The CLI output should always show `hit_rate = 0.80 [0.55, 0.93] (n=15)` so the operator immediately sees the sample limitation.
4. **Include a "minimum sample size" warning.** If n < 20, prepend the output with: "WARNING: Fewer than 20 settled positions. All metrics are preliminary."
5. **Report the exact count needed for a given CI width.** E.g., "To achieve a 95% CI width of +/- 0.10 on hit rate, need approximately 96 settled positions."

**Detection:**
- Any CI that spans more than 50% of the metric's possible range (e.g., hit rate CI from 0.30 to 0.90)
- A point estimate where the CI includes both "strategy is profitable" and "strategy is unprofitable" values
- Bootstrap CIs that change significantly when re-run (indicates insufficient resamples for the given n)

**Phase to address:** Must be resolved during metric computation implementation. Wilson score should be the default interval method for proportions.

---

### Pitfall 4: Look-Ahead Bias in Threshold Effectiveness Analysis

**What goes wrong:**
The CLI analyzes whether the dynamic threshold (`ThresholdConfig.static_floor`, `k * rolling_stddev`, etc.) was effective at filtering bad signals. The temptation is to compute: "at what threshold setting would we have maximized profit?" This is classic look-ahead bias -- using future settlement outcomes to optimize a parameter that was set before the outcome was known.

Specifically, the existing `ThresholdComponents` struct logs the threshold value at computation time. If the CLI scans all signals and finds "signals with net_edge > 0.03 were 90% profitable while signals with net_edge between 0.01 and 0.03 were only 40% profitable," the conclusion "set threshold to 0.03" is overfitted to this sample. The next sample may have a completely different optimal threshold.

A subtler form: the `RollingStats` used for dynamic thresholding already uses past data (rolling mean + k*stddev). If the CLI's analysis period includes the warm-up period where rolling stats were in cold start mode (threshold = static_floor * cold_start_multiplier), those observations are under a different regime than the later observations where rolling stats are fully warmed. Mixing them contaminates the analysis.

**Why it happens:**
The whole point of the CLI is to evaluate threshold effectiveness. The natural approach is "look at all the data and find the best threshold." But this conflates in-sample optimization with out-of-sample prediction.

**Consequences:**
- Operator adjusts threshold based on overfitted analysis, then the next soak test period has worse results
- Regime-mixing (cold start + warm periods) produces threshold recommendations that are averages of two incompatible distributions
- False confidence that a specific threshold setting will be optimal going forward

**Prevention:**
1. **Exclude cold-start observations from threshold analysis.** Filter out all `SpreadResult` records where `threshold_components.is_cold_start == true`. The CLI should report: "Excluded N cold-start observations."
2. **Split the data into train/test halves chronologically.** Show threshold effectiveness on the first half and verify it holds on the second half. This is walk-forward validation at the simplest level.
3. **Report threshold effectiveness as a distribution, not an optimum.** Show a table: "at threshold 0.01: hit rate X%, edge Y%. At threshold 0.02: hit rate X%, edge Y%." Let the operator see the tradeoff curve rather than a single "optimal" value.
4. **Never auto-recommend a specific threshold value.** The CLI presents data; the operator decides. The output should say "here is how different thresholds performed in this sample" not "set your threshold to 0.027."
5. **If the soak test period is long enough (>60 observations post-cold-start), compute time-stability** of the optimal threshold. Does the "best" threshold shift significantly between weeks 1-2 and weeks 3-4?

**Detection:**
- The recommended threshold from the CLI matches the sample's mean net_edge suspiciously closely
- Threshold analysis includes observations where `is_cold_start == true`
- The operator changes the threshold and the next period's results are significantly worse

**Phase to address:** Threshold analysis should be a separate CLI subcommand or section, not mixed into the primary signal scoring output. The CLI design phase should specify that threshold analysis always includes the train/test split.

---

## Moderate Pitfalls

Mistakes that cause degraded analysis quality or misleading secondary metrics but do not invalidate the primary go/no-go assessment.

### Pitfall 5: JSONL Files Growing Unbounded During Long Soak Tests

**What goes wrong:**
The spread logger writes every computation to JSONL. With 3 venue pairs, each computing 4 spread patterns per event, on a 1-second tick rate, the spread log produces approximately:

- 3 pairs * 4 patterns * 1/sec = 12 records/second (when all pairs are active)
- Each `SpreadResult` JSON record is approximately 600-800 bytes (based on the sample data with threshold_components)
- Per day: 12 * 86400 * 700 bytes = ~725 MB per day

Over a 30-day soak test, that is ~21 GB of spread JSONL data split across 30 daily files. The signal log is much smaller (only signals above threshold), but the spread log is the primary input for the spread analytics CLI.

The CLI must process these files. If it reads entire files into memory (using `std::fs::read_to_string()` + line-by-line `serde_json::from_str()`), a single day's file (725 MB) requires at minimum 725 MB of memory for the raw string, plus the deserialized structures (approximately 2x for owned String fields in SpreadResult). A 30-day analysis requires either sequential processing (slow) or all-at-once loading (2-4 GB RAM).

Additionally, the `serde_json::from_str()` approach allocates per-line. With ~1 million lines per day, heap allocation pressure is significant.

**Why it happens:**
The spread logger was designed for observability (grep/tail for debugging), not for analytical consumption. Writing every computation is correct for the service's monitoring needs but produces volumes that are challenging for batch analysis.

**Consequences:**
- CLI analysis of 30 days of data takes minutes instead of seconds
- Memory exhaustion on machines with limited RAM
- Operator avoids running full-period analysis due to wait time, misses trends

**Prevention:**
1. **Stream-process JSONL files.** Use `BufReader::new(File::open(path))` with `lines()` iterator. Parse and accumulate statistics per-line without holding all records in memory. This is the standard Rust pattern and should be the default.
2. **Pre-filter by timestamp or event_id.** The CLI should accept `--from` and `--to` date arguments. Since files are date-stamped (`YYYY-MM-DD.jsonl`), the CLI only opens files within the range. Within each file, records have `timestamp_ms` for further filtering.
3. **Accumulate aggregates, not records.** For hourly time-bucket analysis, maintain a `HashMap<(hour, venue_pair), BucketStats>` where `BucketStats` is a small struct with count/sum/sum_sq/min/max. This requires O(hours * venue_pairs) memory, not O(records).
4. **Consider pre-computed daily summary files.** The existing `DailyAggregator` computes daily rollups. A spread-analytics summary file per day (written at rotation time by the SpreadLogger or a post-processing step) would reduce the CLI's work to reading 30 small summary files instead of 30 large JSONL files.
5. **Use `simd-json` or `sonic-rs` for parsing performance** if standard `serde_json` is a bottleneck. However, `serde_json` with streaming should be fast enough for this volume. Profile before adding dependencies.

**Detection:**
- CLI takes more than 10 seconds for a single day's analysis -- likely loading entire file into memory
- `peak_rss` reported by the OS during CLI execution exceeds 1 GB
- The CLI panics with "out of memory" on long soak test data

**Phase to address:** The CLI's data loading layer should be stream-based from the start. This is an architectural decision, not a late optimization.

---

### Pitfall 6: Max Drawdown Calculation Edge Cases

**What goes wrong:**
Max drawdown (peak-to-trough decline) has several implementation edge cases that produce wrong results in a binary-event trading context:

1. **No meaningful equity curve.** Standard max drawdown tracks a portfolio equity curve over time. But binary arb positions do not have continuous mark-to-market -- they are entered at a cost, remain open at uncertain value, and then resolve to a definitive outcome. The "equity curve" is a step function that jumps at each settlement. Between settlements, the portfolio value is the sum of unrealized positions whose value is unknown (the mark-to-market in `PaperPosition` uses current mid-price, which is a noisy estimate for a binary outcome).

2. **Settlement clustering creates artificial drawdown.** If 5 positions all expire on the same date and 3 lose, the equity curve drops sharply. This looks like a large drawdown, but it is really one event (3 correlated losses). The max drawdown metric does not distinguish between "3 independent losses over 3 weeks" (genuinely concerning) and "3 correlated losses from the same expiry" (one bad outcome).

3. **Empty periods inflate drawdown timing.** If there are no settlements for 2 weeks (waiting for monthly expiries), the equity curve is flat. Then a cluster of losses occurs. The drawdown "duration" includes the flat period, making it look like the system was underwater for weeks when in reality it was simply idle.

4. **Starting balance matters.** Drawdown as a percentage requires a denominator. If using a notional starting balance (e.g., $10,000 paper trading), the percentage drawdown depends on an arbitrary constant. If using cumulative P&L, the drawdown percentage is undefined when starting from zero (division by zero at P&L=0).

**Why it happens:**
Max drawdown is designed for continuously-traded portfolios with daily or intra-day mark-to-market. Binary event arb with weekly/monthly settlements does not produce the kind of equity curve that drawdown was designed to measure.

**Consequences:**
- Reported max drawdown is dominated by settlement clustering, not independent risk
- Drawdown duration is inflated by idle periods
- Percentage drawdown depends on arbitrary starting balance

**Prevention:**
1. **Report drawdown in absolute terms (dollar P&L decline), not percentage.** Since there is no meaningful "portfolio value" (it is paper trading with notional sizing), absolute drawdown is more interpretable.
2. **Report "consecutive losing trades" as the primary risk metric** instead of or alongside max drawdown. This counts the longest streak of net-negative settled positions and is unambiguous.
3. **If percentage drawdown is computed, use peak cumulative P&L as the denominator**, not starting balance. This means drawdown is undefined until the first profitable trade (no peak to draw down from). Handle this edge case: "drawdown not computable until first profitable settlement."
4. **Annotate drawdown with the number of settlements involved.** "Max drawdown: -$450 over 3 settlements on 2026-03-01" is much more informative than "max drawdown: -$450."
5. **Separate correlated losses.** If multiple positions resolve on the same date for the same event, count them as a single "event loss" for drawdown purposes. Report both "per-position drawdown" and "per-event drawdown."

**Detection:**
- Max drawdown occurs entirely within a single settlement date (settlement clustering)
- Drawdown duration exceeds 2x the average settlement interval (idle period inflation)
- Percentage drawdown calculated from a starting balance of zero (division error or NaN)

**Phase to address:** Drawdown implementation in the signal scoring CLI. The specification should define which drawdown variant to compute before implementation.

---

### Pitfall 7: Timestamp Inconsistency Between Spread Logs and Signal Logs

**What goes wrong:**
The system uses multiple timestamp sources that are not perfectly aligned:

1. **SpreadResult.timestamp_ms** is `i64` representing local milliseconds from `Utc::now()`. This is wall-clock time.
2. **SpreadResult.poly_exchange_ts** and **SpreadResult.kalshi_exchange_ts** are exchange-provided timestamps (milliseconds). These are the exchange's clock, which may differ from the local clock.
3. **ArbSignal.timestamp** is a `DualTimestamp` which serializes as `DateTime<Utc>` (ISO 8601). This is wall-clock time.
4. **Daily file rotation** uses `Utc::now().date_naive()`. A spread computation at 23:59:59.999 UTC could be written to today's file while the corresponding signal (generated milliseconds later) is written to tomorrow's file.

When the CLI performs hourly time-bucket analysis on spread data, it must decide which timestamp to use. Using `timestamp_ms` (local wall clock) groups by when the system computed the spread. Using `poly_exchange_ts` groups by when the venue produced the data. These can differ by seconds to minutes if the system is under load or the venue has clock drift.

Furthermore, when correlating spread results with signal results (e.g., "what was the spread distribution in the hour before this signal fired?"), the CLI must join across two JSONL file sets using timestamps. Misaligned timestamps produce incorrect correlations.

**Why it happens:**
The logging system was designed for independent observability (grep one file at a time). Cross-file correlation was not a design requirement. Each logger uses a convenient timestamp for its domain.

**Consequences:**
- Hourly buckets contain records from slightly different actual times depending on which timestamp is used
- Cross-file joins (spread data + signal data) miss records near file boundaries
- Analysis that spans midnight UTC splits observations across two files unpredictably

**Prevention:**
1. **Choose ONE canonical timestamp for all CLI bucketing: `timestamp_ms` from SpreadResult.** This is the system's computation time, is always present (not Optional), and is monotonically increasing (barring NTP adjustments). Document this choice.
2. **For cross-file correlation, use a time window tolerance.** When finding spread records near a signal, use `signal.timestamp +/- 5_seconds` rather than exact match. This handles both clock drift and logging latency.
3. **Handle file boundary edge cases explicitly.** When analyzing hour 23:00-00:00, load both today's file and tomorrow's file, then filter by timestamp. The daily file is a storage boundary, not a logical boundary.
4. **Display exchange timestamps separately** when available, as "venue latency" (difference between exchange_ts and local timestamp_ms). This is a useful diagnostic but should not be the primary bucketing key.
5. **In the SpreadResult and ArbSignal JSONL schema, all timestamps are in UTC.** The CLI should never attempt timezone conversion. All times are UTC, period.

**Detection:**
- Records with `poly_exchange_ts` more than 60 seconds different from `timestamp_ms` (suggests clock drift or processing delay)
- Hourly bucket counts that are suspiciously uneven (e.g., hour 23 has 2x the records of hour 22) -- may indicate a bucketing boundary issue
- Signal-spread correlation that produces "no matching spread data" for signals that clearly should have spread data

**Phase to address:** Timestamp handling should be specified in the CLI design document. The "canonical timestamp for bucketing" decision must be made before implementation.

---

### Pitfall 8: Cost Model Drift Between Live System and Analysis CLI

**What goes wrong:**
The signal scoring CLI computes "cost-adjusted edge" and uses the `CostBreakdown` from logged signals. But the cost model parameters may have changed between when the data was logged and when it is analyzed:

1. **Fee model updates.** If Polymarket or Kalshi changes their fee structure during the soak test, signals logged before the change have old fees. The CLI cannot distinguish "fee model v1" signals from "fee model v2" signals unless it checks the `fee_model_version` field on `SettledLeg`.

2. **Config changes during soak test.** The `SpreadConfig` fields (target_notional, fee rates, carry rate, basis_risk_scale) are config-driven. If the operator tunes these during the soak test (e.g., changes `annualized_rate` from 0.05 to 0.08), signals before and after the change have different cost bases. Aggregating them produces a meaningless average.

3. **Carry cost is path-dependent.** The carry cost in `SpreadResult` uses `reference_holding_days` from config. But the ACTUAL holding period (entry to settlement) may differ. A position entered 14 days before expiry has different carry than one entered 2 days before expiry, but both use the same `reference_holding_days` from config. The CLI should compute actual holding period from `PaperPosition.filled_at` to `PaperPosition.settled_at`, not rely on the logged carry cost.

**Why it happens:**
The live system logs costs at computation time using current config. There is no mechanism to retroactively adjust costs when config changes or when actual holding periods differ from reference periods.

**Consequences:**
- Aggregated cost-adjusted edge mixes signals with different cost assumptions
- Carry cost error systematically overstates or understates true cost depending on whether reference_holding_days is longer or shorter than actual holding
- Fee model changes create discontinuities in the time series that the CLI interprets as edge changes

**Prevention:**
1. **Group analysis by config regime.** If config changed during the soak test, the CLI should detect cost parameter changes between records (compare `total_cost` patterns or, if available, a config generation marker) and segment the analysis accordingly.
2. **Recompute actual carry cost in the CLI** using `(settled_at - filled_at).num_days()` * `annualized_rate * notional / 365`. This replaces the logged carry cost with the realized carry cost. The CLI has access to position timestamps.
3. **The `fee_model_version` field on SettledLeg** (already exists) should be used to group and segregate analysis by fee model version. Report per-version metrics.
4. **Add a "cost sensitivity" analysis.** Show how edge changes under +/- 20% cost variations. If the strategy is only profitable under the most optimistic cost assumptions, that is a red flag.
5. **Log a config snapshot hash with each SpreadResult.** This is a future improvement -- for now, the CLI should warn if spread cost distributions are bimodal (suggesting a config change mid-test).

**Detection:**
- Bimodal distribution of `total_cost` values (two clusters suggesting config change)
- `carry_cost` values that are identical across positions with very different holding periods
- Fee amounts that change discontinuously at a specific date

**Phase to address:** The CLI should accept a `--config` flag to load the current config for reference, but should primarily use the logged cost data. Cost recomputation (carry) is a feature for the signal scoring CLI's metric computation layer.

---

### Pitfall 9: CLI Output Too Verbose or Too Terse for Actionable Decisions

**What goes wrong:**
Analysis CLI tools commonly fail at the UX level in two opposite ways:

1. **Too verbose.** Dumping every metric for every venue pair, every event, every threshold bucket in a wall of text. The operator cannot find the go/no-go signal in the noise. Example of anti-pattern output:
```
kalshi_polymarket BuyPolyYesSellKalshiYes: n=12, hit_rate=0.667, edge=0.021, ci=[0.35,0.90]
kalshi_polymarket SellPolyYesBuyKalshiYes: n=8, hit_rate=0.750, edge=0.018, ci=[0.35,0.97]
kalshi_polymarket BuyPolyNoSellKalshiNo: n=12, hit_rate=0.333, edge=-0.021, ci=[0.10,0.65]
kalshi_polymarket SellPolyNoBuyKalshiNo: n=8, hit_rate=0.250, edge=-0.018, ci=[0.03,0.65]
deribit_polymarket BuyPredictionSellOptions: ...
[30 more lines]
```
The NO patterns are algebraically equivalent to the YES patterns (as noted in `patterns.rs` tests). Showing all 4 patterns doubles the output with no new information.

2. **Too terse.** A single-number output ("Sharpe: 1.8") without context, confidence interval, sample size, or time period is worse than useless because it invites misinterpretation.

3. **Unclear defaults.** If `--from` and `--to` are not specified, does the CLI analyze all available data? Only today? The last 30 days? An undocumented default leads to inconsistent analysis.

**Why it happens:**
The developer implements the metric computation correctly but does not think carefully about what the operator needs to see vs. what is computable. The temptation is to show everything that was computed.

**Consequences:**
- Operator ignores verbose output and makes gut-feel decisions instead of data-driven ones
- Terse output is misinterpreted, leading to overconfidence
- Different analysis runs produce different results because of unclear defaults, eroding trust in the tool

**Prevention:**
1. **Tiered output: summary first, detail on request.** Default output shows 5-7 key metrics with CIs and sample sizes. A `--verbose` flag shows per-venue-pair and per-event breakdowns. A `--json` flag produces machine-readable output.
2. **Collapse algebraically equivalent patterns.** BuyPolyYesSellKalshiYes and SellPolyNoBuyKalshiNo produce negated spreads. Only show the positive-direction pattern (net buyer) and note the complement exists.
3. **Explicit defaults documented in `--help`.** "Default date range: all available data. Use --from YYYY-MM-DD --to YYYY-MM-DD to restrict."
4. **Machine-parseable output mode.** `--output json` for piping into other tools. `--output table` (default) for human reading. Never mix prose and data in the default output.
5. **Include an explicit "GO / NO-GO / INCONCLUSIVE" recommendation** based on configurable thresholds (e.g., `--min-trades 30 --min-hit-rate 0.55 --max-drawdown 500`). This forces the operator to define their criteria upfront.

**Detection:**
- Default CLI output exceeds 40 lines (too verbose for a terminal screen)
- CLI output has no sample sizes next to metrics
- Two runs on the same data produce different results (unclear defaults)

**Phase to address:** CLI design phase. The output format should be specified before metric implementation. `clap` (already a dependency) supports subcommands and argument defaults cleanly.

---

### Pitfall 10: Mixing Venue-Pair Types in Aggregate Metrics

**What goes wrong:**
The system operates on two fundamentally different types of venue pairs:

1. **Prediction-vs-prediction** (Polymarket vs Kalshi): Both sides are binary contracts. The spread is in probability space. Cost models are symmetric (both sides pay binary contract fees). The `SpreadPattern` enum handles these 4 patterns.

2. **Prediction-vs-options** (Polymarket/Kalshi vs Deribit): One side is a binary contract, the other is an options position. The spread is in probability space but the cost models are asymmetric (binary contract fee vs. options taker fee + carry). The `ArbSignal` type handles these.

If the CLI aggregates hit rate and edge across BOTH types, the metrics are confounded. Prediction-vs-prediction arbs have different edge distributions, cost structures, and risk profiles than prediction-vs-options arbs. A 60% hit rate that is 80% from prediction-vs-prediction and 40% from prediction-vs-options masks a failing options strategy.

**Why it happens:**
Both types produce settled positions with P&L. It is natural to compute aggregate metrics. The `AccumulatorKey` in `analyzer.rs` keys by `venue_pair` (e.g., "kalshi_polymarket" vs "deribit_polymarket"), but the CLI must propagate this grouping.

**Consequences:**
- Aggregate metrics hide per-type performance differences
- A strong prediction-vs-prediction performance masks weak prediction-vs-options performance
- The go/no-go decision for v2 execution (which is primarily about prediction-vs-options) is based on metrics contaminated by prediction-vs-prediction data

**Prevention:**
1. **Always report metrics by venue-pair type.** The default CLI output should separate prediction-vs-prediction and prediction-vs-options results, even in the summary.
2. **Use the `venue_pair_label()` from SpreadPattern for prediction-vs-prediction** and `prediction_venue` from ArbSignal for prediction-vs-options to group results.
3. **If aggregate metrics are shown, label them explicitly** as "ALL PAIRS (aggregate)" and show per-pair metrics directly below.
4. **The go/no-go recommendation should be per-pair-type**, not aggregate. "Prediction-vs-prediction: GO. Prediction-vs-options: INCONCLUSIVE (n=8)."

**Detection:**
- CLI output shows a single "hit rate" without venue-pair breakdown
- Aggregate hit rate is between the per-pair hit rates (weighted average masking divergent performance)

**Phase to address:** Data model design. The CLI's internal aggregation structures must key by venue-pair type from the start.

---

## Minor Pitfalls

Issues that cause inconvenience, suboptimal UX, or minor correctness issues but are self-correcting or low-impact.

### Pitfall 11: `DualTimestamp` Deserialization Loses Monotonic Clock

**What goes wrong:**
`DualTimestamp` deserializes the `mono` field as `Instant::now()` (timestamp.rs line 48) because monotonic instants are not serializable across process boundaries. This means any CLI analysis that deserializes `ArbSignal` records from JSONL and then calls `.elapsed()` on the timestamp gets "time since deserialization" not "time since signal generation." This is correct behavior (it is how DualTimestamp was designed), but the CLI developer might accidentally use `signal.timestamp.elapsed()` instead of computing duration from `signal.timestamp.wall`.

**Prevention:**
In the CLI code, always use `signal.timestamp.wall` for time-based analysis. Never call `.elapsed()` or `.mono` on deserialized timestamps. Add a code comment at the deserialization site.

**Phase to address:** Implementation review.

---

### Pitfall 12: Decimal String Serialization Complicates Numeric Analysis

**What goes wrong:**
All `Decimal` fields in SpreadResult and ArbSignal are serialized as JSON strings (via `#[serde(with = "rust_decimal::serde::str")]`). This means the CLI must deserialize them back into `Decimal` for computation. This is already handled by the existing serde annotations (deserialization works correctly), but it means external tools (jq, Python scripts) cannot directly do arithmetic on these fields without parsing them as strings first.

More subtle: `rust_decimal`'s `Decimal` type preserves trailing zeros. The value "0.0100" serializes as `"0.0100"`, not `"0.01"`. If the CLI formats output by converting Decimal to f64 for display, precision may be lost. If it displays the Decimal string directly, formatting is inconsistent (some values have 2 decimal places, others have 16).

**Prevention:**
1. Use `Decimal::normalize()` before display to strip trailing zeros.
2. Define a display format: 4 decimal places for probabilities and edges, 2 for dollar amounts, 6 for fees.
3. For the `--json` output mode, keep the string-encoded Decimals as-is for round-trip compatibility.

**Phase to address:** Output formatting implementation.

---

### Pitfall 13: Clap Subcommand Structure Becomes Unwieldy

**What goes wrong:**
The project already uses `clap` (v4.5) for CLI argument parsing. Adding two new CLI tools (spread-analytics, signal-scoring) to the existing binary raises a design question: should these be subcommands of the main binary (`prediction spread-analytics ...`, `prediction signal-score ...`) or separate binaries?

If implemented as subcommands, the main binary's `main.rs` (which currently runs the live trading pipeline) must be refactored to handle subcommands. This touches a critical code path (the live system startup) for a non-live feature (offline analysis). If the subcommand parsing has a bug, it could prevent the live system from starting.

If implemented as separate binaries, they share no code with the main binary. Shared types (SpreadResult, ArbSignal) must be importable from a library crate. The current `src/lib.rs` exports the full crate, which is fine.

**Prevention:**
1. **Use `[[bin]]` entries in Cargo.toml** for separate binaries that import from the library crate. This keeps the live system's entry point untouched.
2. Alternatively, use `clap` subcommands but put the live system behind a `run` subcommand (breaking change) or make no-subcommand default to the live system (backward compatible).
3. **Recommendation: separate binaries.** `src/bin/spread_analytics.rs` and `src/bin/signal_score.rs`. These import types from `prediction::spread::patterns::SpreadResult`, etc. No change to `src/main.rs`.

**Phase to address:** Architecture decision before implementation begins. This is a 5-minute decision with lasting consequences.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Metric selection and design | Pitfall 1: Sharpe annualization invalid for binary events | Use per-trade Sharpe; add PSR; report Sortino; never sqrt(252) |
| Data loading layer | Pitfall 5: JSONL files too large for memory | Stream-process with BufReader; accumulate aggregates, not records |
| Data loading layer | Pitfall 2: only settled positions analyzed | Load all position states; report settlement ratio; count timeouts as losses |
| Confidence interval implementation | Pitfall 3: misleading CIs with small n | Wilson score for proportions; Student's t with n-warning for means |
| Threshold effectiveness analysis | Pitfall 4: look-ahead bias in threshold optimization | Exclude cold-start; split train/test; never auto-recommend threshold |
| Drawdown computation | Pitfall 6: binary settlement edge cases | Report absolute drawdown; count consecutive losses; annotate clustering |
| Timestamp handling | Pitfall 7: inconsistent timestamps across log files | Use timestamp_ms as canonical; window tolerance for joins |
| Cost-adjusted edge computation | Pitfall 8: cost model drift during soak test | Group by fee_model_version; recompute carry from actual holding period |
| CLI output design | Pitfall 9: too verbose or too terse | Tiered output (summary/verbose/json); collapse equivalent patterns |
| Aggregation by venue pair | Pitfall 10: mixing prediction-vs-prediction with prediction-vs-options | Always report by pair type; separate go/no-go per type |
| Binary layout | Pitfall 13: subcommand vs separate binary | Use separate binaries via [[bin]] entries; do not modify main.rs |
| Output formatting | Pitfall 12: Decimal display inconsistency | Decimal::normalize(); fixed decimal places per field type |

---

## Integration-Specific Warnings

These pitfalls arise specifically from integrating analysis tooling into the EXISTING system.

### The CLI Must Not Import Async Runtime Dependencies Unnecessarily

The analysis CLI tools are offline batch processors. They should NOT depend on `tokio` runtime features beyond what `DualTimestamp::deserialize` requires (it calls `tokio::time::Instant::now()` during deserialization). If the CLI binary has `#[tokio::main]`, it pulls in the full async runtime for no reason. Consider whether `DualTimestamp` deserialization can use a non-tokio Instant, or simply accept the minimal tokio dependency.

### Existing Types Use `rust_decimal` Throughout -- The CLI Must Too

The CLI must use `rust_decimal::Decimal` for ALL financial computations (spreads, edges, costs, P&L). Using `f64` for intermediate calculations introduces floating-point error that compounds across aggregation. The `statrs` crate uses `f64` for statistical functions (CDF, PDF), so conversions between Decimal and f64 are necessary at the statistics boundary -- but raw P&L and spread values must stay in Decimal until the final display step.

### The Signal Log Contains ALL Signals, Not Just PassedBoth

Looking at the actual signal log data, all threshold statuses are logged (Filtered, PassedStaticOnly, PassedBoth). The signal scoring CLI must filter by `threshold_status` appropriately. The "go/no-go" analysis should focus on `PassedBoth` signals (which would have been traded), while the threshold effectiveness analysis examines all statuses.

### SpreadResult Timestamp Is Milliseconds, ArbSignal Timestamp Is ISO 8601

These two log file types use different timestamp formats. The CLI code that processes both must handle the conversion. When joining spread data with signal data by time, convert both to milliseconds-since-epoch for comparison.

---

## Sources

- Sharpe ratio estimation and confidence intervals: [Two Sigma Research Paper](https://www.twosigma.com/wp-content/uploads/sharpe-tr-1.pdf)
- Probabilistic Sharpe Ratio: [QuantConnect Research](https://www.quantconnect.com/research/17112/probabilistic-sharpe-ratio/)
- Sharpe ratio for algorithmic trading: [QuantStart](https://www.quantstart.com/articles/Sharpe-Ratio-for-Algorithmic-Trading-Performance-Measurement/)
- Wilson score confidence interval: [Econometrics Blog](https://www.econometrics.blog/post/the-wilson-confidence-interval-for-a-proportion/)
- Wilson CI vs alternatives comparison: [arXiv 2508.10223](https://arxiv.org/html/2508.10223v1)
- Survivorship bias in backtesting: [Quantified Strategies](https://www.quantifiedstrategies.com/survivorship-bias-in-backtesting/)
- Seven sins of quantitative investing: [Portfolio Optimization Book](https://bookdown.org/palomar/portfoliooptimizationbook/8.2-seven-sins.html)
- Backtesting biases: [AlgoTrading101](https://algotrading101.com/wiki/backtesting-biases-and-risks/)
- Maximum drawdown calculation: [LuxAlgo](https://www.luxalgo.com/blog/maximum-drawdown-metric-calculation-and-use-cases/)
- Streaming JSON in Rust: [Rust Forum Discussion](https://users.rust-lang.org/t/reading-json-sequentially/57708)
- Large JSON optimization: [SuperJSON Blog](https://superjson.ai/blog/2025-09-07-optimizing-large-json-files-production/)
- Codebase analysis: All source file references cite structures and patterns from the prediction project codebase as read during research
