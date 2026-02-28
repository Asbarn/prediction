# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-02-28
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.4 Requirements

Requirements for v1.4 Analysis Tooling milestone. Two CLI binaries for offline statistical analysis of soak test data.

### Infrastructure

- [ ] **INFRA-01**: User can run `spread-analytics` and `signal-scoring` as separate CLI binaries with `--from YYYY-MM-DD`, `--to YYYY-MM-DD`, and `--last N` date-range filtering
- [ ] **INFRA-02**: User sees analysis output as formatted terminal tables with aligned numeric columns and section headers
- [ ] **INFRA-03**: User can pass `--output json` to get machine-readable JSON output instead of terminal tables
- [ ] **INFRA-04**: User can pass `--by-event` to see all analyses broken down by event_id in addition to aggregate view

### Spread Analytics

- [ ] **SPREAD-01**: User can view spread distribution summary statistics (count, mean, median, stddev, min, max, p5/p25/p75/p95) for net and gross spreads over a date range
- [ ] **SPREAD-02**: User can view hourly time-bucket analysis showing per-hour spread statistics across 24 UTC hours to identify when opportunities cluster
- [ ] **SPREAD-03**: User can view venue-pair breakdown showing spread statistics grouped by venue pair (Polymarket-Kalshi, Deribit-Polymarket, Deribit-Kalshi) with directional detail

### Signal Scoring

- [ ] **SIGNAL-01**: User can view hit rate (gross and net) with Wilson score confidence intervals at 95% and 99% levels, with sample size reported alongside
- [ ] **SIGNAL-02**: User can view cost-adjusted mean edge with one-sample t-test significance (t-statistic, p-value, 95% CI) to determine if edge is distinguishable from zero
- [ ] **SIGNAL-03**: User can view per-trade Sharpe ratio and frequency-adjusted annualized Sharpe ratio computed from settled position P&L series
- [ ] **SIGNAL-04**: User can view Probabilistic Sharpe Ratio (PSR) showing the probability that true Sharpe exceeds zero, accounting for skewness and kurtosis of returns
- [ ] **SIGNAL-05**: User can view maximum drawdown in absolute and percentage terms with drawdown start date, trough date, recovery date (or ongoing), and current drawdown state

## Future Requirements

### Threshold Optimization

- **THRESH-01**: User can view threshold effectiveness breakdown (count, hit rate, mean edge) per threshold_status category (PassedBoth, PassedStaticOnly, Filtered)
- **THRESH-02**: User can run optimal threshold backtest to find the threshold that would have maximized net P&L

### Extended Analytics

- **EXT-01**: User can view spread autocorrelation (lag-1 through lag-N) to assess opportunity persistence
- **EXT-02**: User can compare two date ranges side by side with delta columns to detect regime changes

## Out of Scope

| Feature | Reason |
|---------|--------|
| Real-time TUI dashboard | Prometheus + Grafana already covers live monitoring; analysis tools are for offline post-hoc evaluation |
| Database backend (SQLite/DuckDB) | JSONL sufficient at current scale; in-memory Vec<T> faster than SQLite for expected volumes |
| Full backtesting engine | Requires simulating fills, latency, partial fills; actual settled data is stronger evidence than backtests |
| Charting / plotting | JSON output + external tools (matplotlib, gnuplot) preferred; terminal charts are low-fidelity |
| Automated go/no-go decision | Human judgment weighs statistics alongside risk tolerance, capital, market conditions; no algorithm captures all factors |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 26 | Pending |
| INFRA-02 | Phase 26 | Pending |
| INFRA-03 | Phase 26 | Pending |
| INFRA-04 | Phase 26 | Pending |
| SPREAD-01 | Phase 27 | Pending |
| SPREAD-02 | Phase 27 | Pending |
| SPREAD-03 | Phase 27 | Pending |
| SIGNAL-01 | Phase 28 | Pending |
| SIGNAL-02 | Phase 28 | Pending |
| SIGNAL-03 | Phase 28 | Pending |
| SIGNAL-04 | Phase 28 | Pending |
| SIGNAL-05 | Phase 28 | Pending |

**Coverage:**
- v1.4 requirements: 12 total
- Mapped to phases: 12
- Unmapped: 0

---
*Requirements defined: 2026-02-28*
*Last updated: 2026-02-28 after roadmap creation*
