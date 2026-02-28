# Feature Research: v1.4 Analysis Tooling (Spread Analytics CLI + Signal Scoring CLI)

**Domain:** CLI-based statistical analysis tooling for cross-venue prediction market arbitrage
**Researched:** 2026-02-28
**Confidence:** HIGH (statistical methods well-established; data formats already defined in codebase; no external API dependencies)

**Scope note:** This research covers ONLY the new features for v1.4: two CLI tools that analyze soak test data offline. These tools READ existing JSONL log files and checkpoint state -- they do NOT modify the live system, connect to exchanges, or require new external dependencies beyond terminal table formatting.

**Existing infrastructure this builds on:**
- `spread_logs/{YYYY-MM-DD}.jsonl` -- SpreadResult JSONL files with net_spread, gross_spread, pattern, venue pair, timestamp_ms, threshold data, exchange timestamps, fill ratios, and cost breakdowns
- `signal_logs/{YYYY-MM-DD}.jsonl` -- ArbSignal JSONL files with net_edge, raw_spread, confidence, cost_breakdown, threshold_status, prediction/options legs, IV spread, skew adjustment
- `state/checkpoint.json` -- PaperPosition lifecycle state with settlement P&L, settled legs, divergence annotations, adverse selection, MTM history
- `AccumulatorBucket` -- existing runtime hit rate, edge, convergence, false positive rate tracking (keyed by venue_pair + event_id + threshold_status)
- `DailyRollup` -- existing per-day trade count, P&L totals, win/loss counts
- `clap` 4.5 already in Cargo.toml with `derive` feature and existing subcommand infrastructure (`Commands` enum)
- `statrs` 0.18 already in Cargo.toml -- provides Normal distribution with inverse CDF needed for Wilson score intervals
- `chrono` 0.4 already in Cargo.toml -- timestamp parsing and hour-of-day extraction
- `rust_decimal` 1.40 already in Cargo.toml -- all arithmetic

---

## Table Stakes

Features the CLI tools must have to answer the "go/no-go" question for v2 execution. Without these, the soak test data cannot be evaluated with statistical rigor.

### TS-1: Spread Distribution Summary Statistics

| Attribute | Detail |
|-----------|--------|
| Why Expected | The first question any trader asks about spread data is "what does the distribution look like?" -- mean, median, standard deviation, min, max, percentiles (p5, p25, p75, p95). Without this, the data is just a wall of JSONL. |
| Complexity | LOW |
| Dependencies | Reads `spread_logs/*.jsonl`, deserializes `SpreadResult` |

**What it is:** For a given date range, compute summary statistics over net_spread and gross_spread values. Output as a formatted terminal table showing count, mean, median, stddev, min, max, p5/p25/p75/p95 percentiles.

**Why it matters for go/no-go:** Establishes the baseline spread distribution. If mean net spread is consistently negative or near zero, there is no edge to exploit. If the distribution is highly skewed, it tells the trader whether rare large opportunities compensate for frequent small losses.

**Implementation note:** Sorting a `Vec<Decimal>` of all spreads to extract percentiles is O(n log n) and perfectly fine for days-to-weeks of data at ~1 record per second throughput (~86K records/day max). No streaming percentile algorithms needed.


### TS-2: Hourly Time-Bucket Spread Analysis

| Attribute | Detail |
|-----------|--------|
| Why Expected | Spread opportunities in cross-venue arbitrage are heavily time-of-day dependent. Crypto options markets (Deribit) have different activity patterns than prediction markets (Polymarket, Kalshi). Knowing which hours produce the best spreads is essential for timing execution in v2. |
| Complexity | LOW-MEDIUM |
| Dependencies | TS-1 (summary statistics computation), `timestamp_ms` field in SpreadResult |

**What it is:** Group spread records into 24 hourly buckets (UTC). For each bucket, compute: record count, mean net_spread, median net_spread, stddev, percentage of positive spreads, and best/worst spread. Present as a 24-row table.

**Why it matters for go/no-go:** If actionable spreads cluster in specific hours (e.g., overlapping US/EU session), v2 execution can be scheduled for those windows only, reducing operational complexity and capital requirements. If spreads are uniformly distributed, the system needs to run 24/7.

**Implementation:** Extract hour from `timestamp_ms` via `chrono::DateTime::from_timestamp_millis()`. Bucket into `[0..24]` array of `Vec<Decimal>`. Compute per-bucket stats using the same functions as TS-1.


### TS-3: Venue-Pair Spread Breakdown

| Attribute | Detail |
|-----------|--------|
| Why Expected | The system tracks 4 directional spread patterns across venue pairs (Polymarket-Kalshi, Deribit-Polymarket, Deribit-Kalshi). Aggregating across all pairs hides which venue combination actually produces edge. A per-pair breakdown is necessary to decide which venue pairs to execute on. |
| Complexity | LOW |
| Dependencies | TS-1 (summary statistics), `pattern` field in SpreadResult (has `venue_pair_label()` method) |

**What it is:** Group spread records by venue pair label. For each pair, compute the same summary statistics as TS-1 plus directional breakdown (by SpreadPattern variant). Present as a multi-section table.

**Why it matters for go/no-go:** If only kalshi_polymarket spreads show positive mean net_spread but deribit_polymarket does not, v2 execution should focus on the Kalshi-Polymarket pair. This directly informs capital allocation strategy.

**Implementation:** Group by `SpreadResult.pattern.venue_pair_label()`. For each group, compute stats. Also break down by `SpreadPattern` variant within each group to show directional skew.


### TS-4: Hit Rate with Confidence Intervals

| Attribute | Detail |
|-----------|--------|
| Why Expected | Raw hit rate (e.g., "60% of signals were profitable") is meaningless without a confidence interval. With small sample sizes (dozens to low hundreds of settled positions in early soak testing), the true hit rate could easily be 40-80%. Statistical significance determines whether the observed edge is real or noise. |
| Complexity | MEDIUM |
| Dependencies | Reads `state/checkpoint.json` (settled PaperPositions) or recomputes from signal/spread logs |

**What it is:** Compute gross hit rate and net hit rate (post-fee) with Wilson score confidence intervals at 95% and 99% levels. Wilson score is preferred over Wald (normal approximation) because it performs well with small samples and proportions near 0 or 1 -- both common in early soak testing.

**Formula (Wilson score interval):**
```
p_hat = successes / n
z = normal_inverse_cdf(1 - alpha/2)   # 1.96 for 95%, 2.576 for 99%
denominator = 1 + z^2/n
center = (p_hat + z^2/(2*n)) / denominator
margin = (z * sqrt(p_hat*(1-p_hat)/n + z^2/(4*n^2))) / denominator
CI = [center - margin, center + margin]
```

**Why it matters for go/no-go:** If the 95% CI lower bound for net hit rate is below 50%, the trader cannot conclude with confidence that the strategy is profitable. This is the single most important statistical test for the go/no-go decision.

**Implementation:** `statrs::distribution::Normal::new(0.0, 1.0).inverse_cdf(0.975)` gives z=1.96. All arithmetic in `rust_decimal` for precision. Report sample size alongside CI to highlight when more data is needed.


### TS-5: Cost-Adjusted Edge with Statistical Significance

| Attribute | Detail |
|-----------|--------|
| Why Expected | Hit rate alone does not capture profitability. A 90% hit rate with tiny wins and occasional large losses can be worse than a 40% hit rate with large wins. The mean net edge (profit per trade after all costs) and its statistical significance (is it distinguishable from zero?) are required. |
| Complexity | MEDIUM |
| Dependencies | Settled positions from checkpoint or signal logs, cost_breakdown data |

**What it is:** Compute mean net edge across settled positions with a one-sample t-test against H0: mean_edge = 0. Report:
- Mean net edge (per trade)
- Standard error of the mean
- t-statistic = mean / (stddev / sqrt(n))
- p-value (two-tailed)
- 95% confidence interval for mean edge
- Cost breakdown: mean total cost, mean fee impact, mean slippage

**Why it matters for go/no-go:** Even if hit rate is above 50%, if the mean net edge is not statistically significantly different from zero (p > 0.05), the trader cannot conclude the strategy generates profit after costs. This catches strategies that appear profitable due to random variation.

**Implementation:** t-distribution not in `statrs`? The `statrs` crate includes `StudentsT` distribution. Use `StudentsT::new(0.0, 1.0, df).cdf(t_stat)` for p-value computation. All values from `PaperPosition.settlement_pnl` and cost data from the originating signals.


### TS-6: Sharpe Ratio Calculation

| Attribute | Detail |
|-----------|--------|
| Why Expected | The Sharpe ratio is the universal metric for risk-adjusted return in quantitative trading. Any trader evaluating a strategy will compute it. A Sharpe below 1.0 (annualized) is generally insufficient to justify live capital deployment. |
| Complexity | LOW-MEDIUM |
| Dependencies | Time series of per-trade P&L (settled positions) |

**What it is:** Compute the Sharpe ratio from the sequence of per-trade returns:
```
Sharpe = mean(returns) / stddev(returns) * sqrt(annualization_factor)
```

For binary event arbitrage with irregular trade timing, the annualization factor must be derived from the actual trading frequency, not a fixed 252 (trading days) assumption.

**Key consideration for this system:** Trades are not daily -- they correspond to binary event settlements that may be days or weeks apart. The annualization factor should be computed as: `sqrt(365.25 * 24 * 3600 / avg_trade_interval_seconds)` to normalize by actual trading frequency.

**Output:** Report raw (non-annualized) Sharpe, annualized Sharpe, number of trades, average trade interval, and a note on the annualization methodology.

**Why it matters for go/no-go:** Industry consensus is that annualized Sharpe > 1.0 is acceptable for retail algorithmic trading, > 2.0 is strong. Below 1.0 after costs suggests the strategy does not compensate for the risk taken.


### TS-7: Maximum Drawdown Computation

| Attribute | Detail |
|-----------|--------|
| Why Expected | Max drawdown measures the worst peak-to-trough decline. Even a profitable strategy with high Sharpe can have a drawdown that exceeds the trader's risk tolerance or available capital. |
| Complexity | LOW |
| Dependencies | Time-ordered series of cumulative P&L (from settled positions) |

**What it is:** From the cumulative P&L curve of settled trades:
1. Compute running peak (high-water mark)
2. At each point, compute drawdown = (current - peak) / peak (or absolute drawdown in dollar terms)
3. Report maximum drawdown in both absolute terms and as percentage of peak equity
4. Report drawdown duration (time from peak to recovery, or ongoing if still in drawdown)

**Output:** Max drawdown amount, max drawdown percentage, drawdown start date, trough date, recovery date (or "ongoing"), current drawdown.

**Why it matters for go/no-go:** If max drawdown exceeds the trader's risk budget (e.g., 20% of capital), the strategy cannot be deployed even if its expected return is positive. Max drawdown directly informs position sizing and capital requirements for v2.


### TS-8: Date-Range Filtering and Multi-Day Aggregation

| Attribute | Detail |
|-----------|--------|
| Why Expected | Soak testing produces data over days or weeks. The trader needs to analyze specific date ranges (e.g., last 7 days, last 3 days, specific date) and compare different periods. Without date filtering, every analysis covers all available data with no ability to isolate trends or regime changes. |
| Complexity | LOW |
| Dependencies | All other features (this is a filter applied before analysis) |

**What it is:** CLI flags for `--from YYYY-MM-DD`, `--to YYYY-MM-DD`, and `--last N` (last N days). Applied at the JSONL file loading stage -- only load files within the date range, then filter records by timestamp within those files.

**Implementation:** Since files are already named `{YYYY-MM-DD}.jsonl`, date filtering at the file level is trivial. Combine with per-record timestamp filtering for intra-day boundaries.


### TS-9: Terminal Table Output

| Attribute | Detail |
|-----------|--------|
| Why Expected | Raw numbers dumped to stdout are unusable. Formatted tables with aligned columns, headers, and section separators are the minimum for a CLI analysis tool. The trader reads this output in a terminal to make decisions. |
| Complexity | LOW |
| Dependencies | All other features (this is the presentation layer) |

**What it is:** Use a terminal table library to format all output. Requirements:
- Aligned numeric columns (right-justified)
- Decimal precision appropriate to the values (spreads to 4dp, percentages to 1dp, counts as integers)
- Section headers separating different analysis views
- Color highlighting for key values (green for positive, red for negative) -- optional but standard

**Library recommendation:** `comfy-table` -- well-tested, no unsafe, handles dynamic-width content, already used in Rust CLI ecosystem. Single dependency addition (consistent with project's conservative dependency philosophy, but justified because terminal formatting is genuinely hard to do well from scratch).

**Alternative considered:** Manual `println!` with format strings. Rejected because alignment breaks with variable-width numbers and the code becomes a maintenance burden.

---

## Differentiators

Features that go beyond the stated v1.4 goals but would add significant value to the go/no-go decision. Not required for the milestone, but worth considering.

### DIFF-1: Probabilistic Sharpe Ratio (PSR)

| Attribute | Detail |
|-----------|--------|
| Value Proposition | The standard Sharpe ratio is a point estimate. With small samples, its distribution is wide. The Probabilistic Sharpe Ratio (developed by Marcos Lopez de Prado) answers: "What is the probability that the true Sharpe ratio exceeds a benchmark (e.g., 0)?" This directly quantifies confidence in the strategy's risk-adjusted performance. |
| Complexity | MEDIUM |
| Dependencies | TS-6 (Sharpe ratio), `statrs` for Normal CDF |

**Formula:**
```
PSR = Normal_CDF((SR_hat - SR_benchmark) * sqrt(n-1) / sqrt(1 - skew*SR_hat + (kurtosis-1)/4 * SR_hat^2))
```
Where `SR_hat` is observed Sharpe, `SR_benchmark` is the target (usually 0), `n` is number of trades, `skew` and `kurtosis` are of the return distribution.

**Why valuable:** With 50 trades, a Sharpe of 1.5 might have only 80% probability of being truly above 0 if returns are skewed. PSR makes this explicit.

**Recommendation:** Include if time permits. The additional computation is trivial once Sharpe is computed. High value for small-sample go/no-go decisions.


### DIFF-2: Threshold Effectiveness Analysis

| Attribute | Detail |
|-----------|--------|
| Value Proposition | The system already tracks signals that were filtered by the dynamic threshold (PassedStaticOnly, Filtered) and correlates them with settlement outcomes. The CLI should surface this data: "How many profitable trades did the threshold filter out? Is the threshold too aggressive or too permissive?" |
| Complexity | MEDIUM |
| Dependencies | `FilteredSignalTracker` data from runtime, threshold_status field on SpreadResult and ArbSignal |

**What it is:** For each threshold_status category (PassedBoth, PassedStaticOnly, Filtered):
- Count of signals
- Hit rate (for PassedBoth: from settlements; for filtered: hypothetical based on outcome)
- Mean edge
- Optimal threshold backtesting: what threshold would have maximized net P&L?

**Why valuable:** Directly informs threshold tuning before v2 deployment. If the threshold is filtering out 30% of would-be profitable trades, it needs adjustment.

**Recommendation:** Include the basic breakdown (count and hit rate per threshold status). Defer optimal threshold backtesting to a future milestone -- it requires replaying all spread data with different thresholds, which is a larger effort.


### DIFF-3: Per-Event Breakdown

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Aggregate statistics can hide that all profit comes from one event while other events lose money. Breaking down by event_id shows if the edge is concentrated or distributed. |
| Complexity | LOW |
| Dependencies | TS-1 through TS-7 (same computations, different grouping key) |

**What it is:** Run all spread and signal scoring analyses grouped by `event_id` in addition to the aggregate view. Present as a table sorted by net P&L per event.

**Why valuable:** If only 1 of 5 events shows positive edge, the strategy may not generalize. If all events show positive edge, the signal is robust.

**Recommendation:** Include. Very low marginal cost (add a group-by to existing computations).


### DIFF-4: Spread Autocorrelation

| Attribute | Detail |
|-----------|--------|
| Value Proposition | If spreads are autocorrelated (today's spread predicts tomorrow's), it means the arb opportunity is persistent and execution timing is less critical. If spreads are mean-reverting quickly, execution speed matters more. This informs v2 latency requirements. |
| Complexity | MEDIUM |
| Dependencies | TS-1 (spread time series) |

**What it is:** Compute lag-1 through lag-N autocorrelation of the net_spread time series. Report correlation coefficients and whether they are statistically significant.

**Recommendation:** Defer. Interesting but not essential for the go/no-go decision. The primary question is "is there edge?" not "how persistent is the edge?" Persistence analysis is a v2 optimization concern.


### DIFF-5: JSON Output Mode

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Machine-readable output for piping to other tools, storing analysis snapshots, or feeding into a future dashboard. |
| Complexity | LOW |
| Dependencies | All features (alternative serialization of the same data) |

**What it is:** `--output json` flag that outputs all analysis results as JSON instead of terminal tables. Uses the same `serde::Serialize` structs that back the table rendering.

**Recommendation:** Include. Trivial to implement (derive `Serialize` on all output structs, `serde_json::to_string_pretty`). Enables `jq` piping and analysis snapshots saved to files.


### DIFF-6: Comparative Period Analysis

| Attribute | Detail |
|-----------|--------|
| Value Proposition | Compare two time periods side by side (e.g., "last 7 days" vs "previous 7 days") to detect if spread patterns are stable or deteriorating. |
| Complexity | MEDIUM |
| Dependencies | TS-8 (date filtering), all analysis features |

**What it is:** `--compare-from YYYY-MM-DD --compare-to YYYY-MM-DD` flags that run the same analysis on two date ranges and present results side by side with delta columns.

**Recommendation:** Defer. The trader can run the tool twice with different date ranges and compare manually. The side-by-side presentation is nice-to-have but adds CLI complexity.

---

## Anti-Features

Features to explicitly NOT build in v1.4.

### AF-1: Real-Time Dashboard / TUI

| Anti-Feature | ncurses/ratatui-based live-updating terminal dashboard |
|--------------|-------------------------------------------------------|
| Why Requested | "It would be cool to watch stats update in real time" |
| Why Problematic | Massive scope increase (event loop, widget layout, state management). The analysis tools are for offline post-hoc analysis of soak test data, not live monitoring. Live monitoring is already covered by Prometheus metrics + Grafana (standard approach for this system). A TUI would duplicate existing capability with a worse interface. |
| Alternative | Run the CLI periodically during soak testing. Use `watch -n 60 prediction analyze-spreads --last 1` for pseudo-live updates. |


### AF-2: Database Backend for Analysis Data

| Anti-Feature | SQLite/DuckDB backend replacing JSONL files |
|--------------|----------------------------------------------|
| Why Requested | "SQL queries would make analysis more flexible" |
| Why Problematic | Violates the project's explicit "no database" philosophy (TOML/JSONL sufficient at current scale). Adds a major new dependency. JSONL files are human-readable, git-trackable, and `grep`-able. The analysis volume (thousands to tens of thousands of records) is trivially handled by in-memory Rust `Vec<T>`. Loading 100K SpreadResult records from JSONL into memory takes under 1 second in Rust. |
| Alternative | Load JSONL into `Vec<SpreadResult>`, filter/group/aggregate in-memory with iterators. This is faster than SQLite for the expected data volumes and has zero infrastructure requirements. |


### AF-3: Backtesting Engine

| Anti-Feature | Full replay-based backtesting with hypothetical execution simulation |
|--------------|---------------------------------------------------------------------|
| Why Requested | "What if we had used different thresholds?" |
| Why Problematic | A backtesting engine is a v2+ feature that requires simulating fill prices, order book state, latency, and partial fills. The v1.4 CLI tools analyze what actually happened, not what might have happened. Backtesting with look-ahead bias is worse than useless -- it creates false confidence. |
| Alternative | TS-4/TS-5 with Wilson score CIs on actual settled positions. If the actual data shows edge, that is stronger evidence than any backtest. |


### AF-4: Charting / Plotting

| Anti-Feature | In-terminal or image-based charts (histograms, scatter plots, time series) |
|--------------|----------------------------------------------------------------------------|
| Why Requested | "Visualize the spread distribution" |
| Why Problematic | Terminal plotting is low-fidelity and adds dependencies (plotters, textplots). Image generation requires a render backend. The trader can export JSON (DIFF-5) and use external tools (Python/matplotlib, gnuplot, Excel) for visualization if needed. Tables convey the same information more precisely for statistical analysis. |
| Alternative | JSON output + external plotting tools. Tables with percentile breakdowns serve the analytical purpose. |


### AF-5: Automated Go/No-Go Decision

| Anti-Feature | CLI outputs "GO" or "NO-GO" based on programmatic thresholds |
|--------------|--------------------------------------------------------------|
| Why Requested | "Automate the decision" |
| Why Problematic | The go/no-go decision is a human judgment that weighs statistical evidence alongside risk tolerance, capital availability, market conditions, and operational readiness. No algorithm can capture all these factors. Presenting false precision ("the system says GO") creates unjustified confidence. |
| Alternative | Present the statistics clearly with confidence intervals. Let the trader decide. Flag when sample sizes are insufficient for reliable conclusions (e.g., "n=12 settled trades -- confidence intervals are wide"). |

---

## Feature Dependencies

```
TS-8: Date-Range Filtering
    |
    +-- TS-1: Spread Distribution Stats (needs loaded, filtered data)
    |     |
    |     +-- TS-2: Hourly Time-Bucket Analysis (reuses stat functions)
    |     |
    |     +-- TS-3: Venue-Pair Breakdown (reuses stat functions)
    |
    +-- TS-4: Hit Rate with CIs (needs loaded position data)
    |     |
    |     +-- TS-5: Cost-Adjusted Edge (builds on position analysis)
    |     |
    |     +-- TS-6: Sharpe Ratio (builds on P&L series)
    |           |
    |           +-- TS-7: Max Drawdown (needs cumulative P&L series)
    |           |
    |           +-- DIFF-1: Probabilistic Sharpe (extends Sharpe)

TS-9: Terminal Table Output (parallel -- presentation layer for all features)

DIFF-5: JSON Output (parallel -- alternative presentation)

DIFF-2: Threshold Effectiveness (parallel -- independent data source)

DIFF-3: Per-Event Breakdown (parallel -- adds group-by to existing analyses)
```

### Dependency Notes

- **TS-8 (date filtering) is the foundation** -- every analysis needs to load and filter JSONL data. Build the data loading layer first.
- **TS-1 (spread stats) and TS-4 (hit rate) are independent branches** -- spread analytics CLI and signal scoring CLI can be built in parallel once the data loading layer exists.
- **TS-9 (table output) is needed by everything** -- but can be stubbed initially with `println!` and upgraded to `comfy-table` as a separate step.
- **DIFF-2 (threshold effectiveness) reads different data** (FilteredSignalTracker state) than the main analyses, so it is independent.
- **Two natural CLI subcommands emerge:** `analyze-spreads` (TS-1, TS-2, TS-3) and `score-signals` (TS-4, TS-5, TS-6, TS-7). Shared infrastructure: TS-8 (loading), TS-9/DIFF-5 (output).

---

## MVP Recommendation

### Must Have (v1.4 ship criteria)

- [x] **TS-8:** Date-range filtering -- foundation for all analysis
- [x] **TS-9:** Terminal table output -- presentation layer
- [x] **TS-1:** Spread distribution summary statistics -- "what does the data look like?"
- [x] **TS-2:** Hourly time-bucket analysis -- "when do opportunities appear?"
- [x] **TS-3:** Venue-pair breakdown -- "which venue pairs have edge?"
- [x] **TS-4:** Hit rate with Wilson score confidence intervals -- "is the win rate real?"
- [x] **TS-5:** Cost-adjusted edge with t-test significance -- "is the edge real after costs?"
- [x] **TS-6:** Sharpe ratio -- "is the risk-adjusted return acceptable?"
- [x] **TS-7:** Maximum drawdown -- "can I survive the worst case?"

### Should Have (include if time permits)

- [ ] **DIFF-1:** Probabilistic Sharpe Ratio -- small marginal cost, high value for small samples
- [ ] **DIFF-3:** Per-event breakdown -- low cost, reveals concentration risk
- [ ] **DIFF-5:** JSON output mode -- trivial to implement, enables external tooling

### Future Consideration (defer)

- [ ] **DIFF-2:** Threshold effectiveness analysis -- valuable but requires additional data pipeline work
- [ ] **DIFF-4:** Spread autocorrelation -- optimization concern, not go/no-go
- [ ] **DIFF-6:** Comparative period analysis -- manual comparison is sufficient initially

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| TS-8: Date filtering | HIGH | LOW | P1 |
| TS-9: Table output | HIGH | LOW | P1 |
| TS-1: Spread stats | HIGH | LOW | P1 |
| TS-2: Hourly buckets | HIGH | LOW | P1 |
| TS-3: Venue-pair breakdown | HIGH | LOW | P1 |
| TS-4: Hit rate + CIs | HIGH | MEDIUM | P1 |
| TS-5: Edge significance | HIGH | MEDIUM | P1 |
| TS-6: Sharpe ratio | HIGH | LOW-MEDIUM | P1 |
| TS-7: Max drawdown | MEDIUM | LOW | P1 |
| DIFF-1: PSR | MEDIUM | LOW | P2 |
| DIFF-3: Per-event | MEDIUM | LOW | P2 |
| DIFF-5: JSON output | MEDIUM | LOW | P2 |
| DIFF-2: Threshold analysis | MEDIUM | MEDIUM | P3 |
| DIFF-4: Autocorrelation | LOW | MEDIUM | P3 |
| DIFF-6: Period comparison | LOW | MEDIUM | P3 |

**Priority key:**
- P1: Must have for v1.4 -- required for go/no-go decision
- P2: Should have, add if time permits -- improves quality of analysis
- P3: Future consideration -- optimization and extended analysis

---

## Data Sources and Schemas

### Spread Analytics CLI (`analyze-spreads`) reads:

**Source:** `spread_logs/{YYYY-MM-DD}.jsonl`
**Schema:** `SpreadResult` struct with fields:
| Field | Type | Use in Analysis |
|-------|------|-----------------|
| `net_spread` | Decimal (string) | Primary metric for distribution analysis |
| `gross_spread` | Decimal (string) | Pre-cost spread for gross edge |
| `pattern` | SpreadPattern enum | Venue pair grouping, directional analysis |
| `event_id` | String | Per-event breakdown |
| `timestamp_ms` | i64 | Time-of-day bucketing, date filtering |
| `threshold` | Decimal or null | Threshold at computation time |
| `threshold_status` | ThresholdStatus or null | Threshold effectiveness |
| `total_cost` | Decimal (string) | Cost analysis |
| `buy_fill_ratio` / `sell_fill_ratio` | Decimal | Liquidity analysis |

### Signal Scoring CLI (`score-signals`) reads:

**Source 1:** `signal_logs/{YYYY-MM-DD}.jsonl`
**Schema:** `ArbSignal` struct with fields:
| Field | Type | Use in Analysis |
|-------|------|-----------------|
| `net_edge` | Decimal (string) | Edge after costs |
| `raw_spread` | Decimal (string) | Pre-cost edge |
| `confidence` | f64 | Confidence score analysis |
| `cost_breakdown` | CostBreakdown | Fee/slippage analysis |
| `threshold_status` | ThresholdStatus | Threshold effectiveness |
| `prediction_venue` | Venue | Venue breakdown |

**Source 2:** `state/checkpoint.json`
**Schema:** Checkpoint with `open` and `daily_rollups` fields, containing `PaperPosition` data with:
| Field | Type | Use in Analysis |
|-------|------|-----------------|
| `settlement_pnl` | Decimal or null | P&L for Sharpe, drawdown, hit rate |
| `status` | PositionStatus | Filter to "Settled" only |
| `settled_legs` | Vec<SettledLeg> | Per-venue P&L breakdown |
| `adverse_selection` | Decimal or null | Fill quality analysis |
| `threshold_status` | ThresholdStatus or null | Threshold analysis |

---

## Sources

- [Binomial proportion confidence interval (Wilson score)](https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval) -- Wilson score formula, comparison with Wald and Clopper-Pearson methods
- [statrs::distribution::Normal](https://docs.rs/statrs/latest/statrs/distribution/struct.Normal.html) -- Rust Normal distribution with inverse CDF for z-scores
- [statrs::distribution::Binomial](https://docs.rs/statrs/latest/statrs/distribution/struct.Binomial.html) -- Binomial distribution in statrs
- [Sharpe Ratio for Algorithmic Trading Performance Measurement](https://www.quantstart.com/articles/Sharpe-Ratio-for-Algorithmic-Trading-Performance-Measurement/) -- Annualization methodology, interpretation thresholds
- [Advanced Trading Metrics: Sharpe, Sortino, Calmar, SQN & K-Ratio](https://algostrategyanalyzer.com/en/blog/advanced-trading-metrics/) -- Comprehensive trading metric overview (2026)
- [How to measure the quality of a trading signal](https://macrosynergy.com/research/how-to-measure-the-quality-of-a-trading-signal/) -- Signal quality metrics, classification-based evaluation, Probabilistic Sharpe Ratio
- [comfy-table](https://github.com/Nukesor/comfy-table) -- Terminal table formatting library for Rust
- [Intraday Patterns in Bid/Ask Spreads](https://digitalcommons.memphis.edu/facpubs/11507/) -- Academic evidence for time-of-day spread patterns (McInish & Wood)
- [5 Key Metrics to Evaluate Trading Algorithms](https://www.utradealgos.com/blog/5-key-metrics-to-evaluate-the-performance-of-your-trading-algorithms/) -- Industry standard performance evaluation metrics (2025)

---
*Feature research for: v1.4 Analysis Tooling (Spread Analytics CLI + Signal Scoring CLI)*
*Researched: 2026-02-28*
