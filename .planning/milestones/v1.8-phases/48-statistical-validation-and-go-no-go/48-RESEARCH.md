# Phase 48: Statistical Validation and Go/No-Go - Research

**Researched:** 2026-03-09
**Domain:** Time-series statistics, autocorrelation correction, train/test splitting, go/no-go reporting
**Confidence:** HIGH

## Summary

Phase 48 is the capstone of the v1.8 milestone. It requires three specific capabilities: (1) autocorrelation-corrected effective sample sizes for all statistical tests, (2) explicit train/test data splitting so evaluation uses out-of-sample data, and (3) a final go/no-go CLI report with confidence intervals, effective sample size, and a clear recommendation.

The project already has a rich analysis infrastructure: `stats.rs` (mean, stddev, percentile, Wilson CI, Pearson, KS test), `scoring.rs` (hit rates, t-test, Sharpe, PSR, drawdown), `sensitivity.rs` (cost perturbation), and mature CLI patterns (clap, DateRange, JSONL loading, table/JSON output). The main gaps are: (a) no autocorrelation estimation or effective sample size correction, (b) no train/test split mechanism, and (c) no unified go/no-go report that synthesizes all metrics into a recommendation.

**Primary recommendation:** Add autocorrelation estimation (lag-1 ACF) and effective sample size functions to `stats.rs`, add a train/test split utility to `io.rs`, create a new `go_no_go.rs` analysis module and `go-no-go` CLI binary that ties everything together with a clear PROCEED/DO NOT PROCEED recommendation.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| STAT-01 | Signal analysis accounts for autocorrelation (effective sample size, not raw count) | Add `autocorrelation_lag1()` and `effective_sample_size()` to `stats.rs`; apply correction factor in edge t-test and hit rate CI computations |
| STAT-02 | Out-of-sample validation separates training/tuning data from evaluation data | Add chronological train/test split to `io.rs`; split spread_logs and signal_logs by date; report which date ranges are train vs test |
| STAT-03 | Final go/no-go report with confidence intervals on expected edge after all fixes applied | New `go_no_go.rs` module + `go-no-go` CLI binary; synthesizes edge CI, effective n, hit rate, Sharpe into PROCEED/DO NOT PROCEED |
</phase_requirements>

## Standard Stack

### Core (already in project)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| statrs | 0.18 | Student's t, Normal CDF for p-values | Already used in scoring.rs |
| clap | 4.5 | CLI argument parsing | Already used in all CLI binaries |
| comfy-table | 7 | Terminal table rendering | Already used via output.rs |
| serde/serde_json | 1.0 | JSONL serialization | Already used everywhere |
| chrono | 0.4 | Date handling for train/test splits | Already used in io.rs |
| rust_decimal | 1.40 | Decimal arithmetic | Already used everywhere |

### No New Dependencies Needed
All statistical computations (autocorrelation, effective sample size, corrected t-tests) can be implemented with basic arithmetic and the existing `statrs` crate. No new crates required.

## Architecture Patterns

### Recommended Project Structure
```
src/
  analysis/
    stats.rs          # ADD: autocorrelation_lag1(), effective_sample_size()
    scoring.rs         # MODIFY: add corrected_edge_test() using effective n
    go_no_go.rs        # NEW: go/no-go computation and report generation
    io.rs              # ADD: train_test_split() for chronological splitting
    mod.rs             # ADD: pub mod go_no_go;
  bin/
    go_no_go.rs        # NEW: go-no-go CLI binary
```

### Pattern 1: Autocorrelation Estimation (Lag-1 ACF)
**What:** Compute lag-1 autocorrelation coefficient for a time series
**When to use:** Before any statistical test on spread/signal data
**Formula:**
```
ACF(1) = sum((x_t - mean) * (x_{t+1} - mean)) / sum((x_t - mean)^2)
```
**Example:**
```rust
/// Lag-1 autocorrelation coefficient.
/// Returns None if fewer than 3 observations or zero variance.
pub fn autocorrelation_lag1(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 3 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let var: f64 = values.iter().map(|x| (x - mean).powi(2)).sum();
    if var == 0.0 {
        return None;
    }
    let cov: f64 = values.windows(2)
        .map(|w| (w[0] - mean) * (w[1] - mean))
        .sum();
    Some(cov / var)
}
```

### Pattern 2: Effective Sample Size
**What:** Correct raw sample size for serial correlation
**When to use:** Replace raw `n` in all statistical tests (t-test, CI width)
**Formula:**
```
n_eff = n * (1 - rho) / (1 + rho)
```
Where `rho` is the lag-1 autocorrelation. When `rho <= 0`, `n_eff = n` (no correction needed).
**Example:**
```rust
/// Effective sample size correcting for lag-1 autocorrelation.
/// Returns raw n if autocorrelation is zero or negative (no correction needed).
/// Minimum return value is 2 (to avoid degenerate statistics).
pub fn effective_sample_size(n: usize, rho: f64) -> usize {
    if rho <= 0.0 || n < 3 {
        return n;
    }
    let n_eff = (n as f64) * (1.0 - rho) / (1.0 + rho);
    (n_eff.round() as usize).max(2)
}
```

### Pattern 3: Chronological Train/Test Split
**What:** Split JSONL data files by date into non-overlapping train and test sets
**When to use:** STAT-02 requires this for out-of-sample validation
**Key constraint:** Split must be chronological (not random) because data is time-series
**Example:**
```rust
/// Split a DateRange into train and test ranges.
/// Default: first 70% of days for training, last 30% for testing.
/// Returns (train_range, test_range).
pub fn train_test_split(range: &DateRange, test_fraction: f64) -> (DateRange, DateRange) {
    let total_days = (range.to - range.from).num_days();
    let train_days = ((total_days as f64) * (1.0 - test_fraction)).floor() as i64;
    let split_date = range.from + chrono::Duration::days(train_days);
    let train = DateRange { from: range.from, to: split_date - chrono::Duration::days(1) };
    let test = DateRange { from: split_date, to: range.to };
    (train, test)
}
```

### Pattern 4: Go/No-Go Report Structure
**What:** Unified report combining all statistical metrics into a decision
**Decision logic:**
```
PROCEED if ALL of:
  1. n_eff >= 30 (minimum statistical power)
  2. Mean net edge > 0 (positive expected value)
  3. Lower bound of 95% CI on edge > 0 (statistically significant positive edge)
  4. PSR > 0.95 (95%+ probability Sharpe > 0)

DO NOT PROCEED if ANY of:
  1. n_eff < 30 (insufficient data)
  2. 95% CI includes zero (edge not statistically distinguishable from zero)
  3. Mean net edge <= 0 (negative expected value)

BORDERLINE (manual review needed):
  - Mean edge > 0 but CI includes zero
  - n_eff between 10 and 30
```

### Anti-Patterns to Avoid
- **Using raw n in statistical tests on time-series data:** Financial time series exhibit autocorrelation. Using raw n overstates significance (narrower CIs, lower p-values than warranted). Always use n_eff.
- **Random train/test split on time series:** Leaks future information into training set. Must use chronological split.
- **Reporting p-values without effective sample size:** A p-value of 0.01 with n=1000 but n_eff=15 is misleading. Always report n_eff alongside.
- **Binary go/no-go without confidence intervals:** "Edge is positive" is useless without knowing the uncertainty range.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Student's t CDF | Custom implementation | `statrs::distribution::StudentsT` | Already used in scoring.rs, handles all edge cases |
| Normal CDF | Custom Phi(z) | `statrs::distribution::Normal` | Already used for PSR |
| Table rendering | Manual string formatting | `comfy-table` via `output.rs` helpers | Project-wide pattern |
| JSONL loading | Custom file parsing | `io::load_jsonl()` + `DateRange` | Already mature, handles errors |

## Common Pitfalls

### Pitfall 1: Effective Sample Size Can Be Very Small
**What goes wrong:** With high autocorrelation (rho=0.9), n_eff = n * 0.1/1.9 = ~5% of raw n. An apparent sample of 200 signals could have n_eff of only 10.
**Why it happens:** Spread signals from the same event/instrument cluster temporally.
**How to avoid:** Always compute and display both raw n and n_eff. If n_eff < 30, explicitly warn about low statistical power. The go/no-go report must gate on n_eff, not raw n.
**Warning signs:** Large gap between n and n_eff, autocorrelation > 0.5.

### Pitfall 2: Corrected T-Test Degrees of Freedom
**What goes wrong:** Using `n - 1` degrees of freedom with effective sample size.
**Why it happens:** When using n_eff in the standard error calculation, the degrees of freedom for the t-distribution should also use `n_eff - 1`, not `n - 1`.
**How to avoid:** Use `n_eff - 1` for df in t-distribution, `n_eff` for standard error divisor.

### Pitfall 3: Train/Test Split With Too Little Test Data
**What goes wrong:** 70/30 split on 5 days of data gives 1-2 days test, which is meaningless.
**Why it happens:** System may not have much historical data yet.
**How to avoid:** Report the actual number of test days and test records. If test set has fewer than 10 records, warn that results are unreliable. Consider requiring a minimum test period.

### Pitfall 4: Wilson CI With Effective Sample Size
**What goes wrong:** Wilson CI for hit rates uses raw n, overstating precision.
**Why it happens:** `wilson_ci()` in stats.rs takes usize total.
**How to avoid:** When reporting hit rate CIs in the go/no-go report, pass n_eff as the total parameter (with adjusted successes count scaled proportionally), or widen the CI by the correction factor.

### Pitfall 5: Multiple Testing Without Correction
**What goes wrong:** Reporting p-values for edge, hit rate, Sharpe separately inflates false positive risk.
**Why it happens:** Each test has its own alpha threshold.
**How to avoid:** The go/no-go report should use a single primary metric (edge CI) for the decision. Other metrics are supporting evidence, not independent tests. No Bonferroni needed if there's one primary hypothesis.

## Code Examples

### Corrected Edge T-Test
```rust
/// Edge t-test corrected for autocorrelation.
/// Uses effective sample size for standard error and degrees of freedom.
pub fn compute_corrected_edge_test(values: &[f64]) -> Option<CorrectedEdgeTestResult> {
    let n = values.len();
    if n < 3 {
        return None;
    }

    let mean = mean_f64(values)?;
    let sd = stddev_f64(values)?;
    if sd == 0.0 {
        return None;
    }

    let rho = autocorrelation_lag1(values).unwrap_or(0.0);
    let n_eff = effective_sample_size(n, rho);

    let se = sd / (n_eff as f64).sqrt();
    let t_stat = mean / se;
    let df = (n_eff - 1) as f64;

    let t_dist = StudentsT::new(0.0, 1.0, df).ok()?;
    let p_value = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));
    let t_crit = t_dist.inverse_cdf(0.975);
    let ci_95 = (mean - t_crit * se, mean + t_crit * se);

    Some(CorrectedEdgeTestResult {
        mean_edge: mean,
        std_error: se,
        t_statistic: t_stat,
        p_value,
        ci_95,
        raw_n: n,
        effective_n: n_eff,
        autocorrelation: rho,
    })
}
```

### Go/No-Go Report Decision Logic
```rust
pub enum GoNoGoDecision {
    Proceed,
    DoNotProceed,
    InsufficientData,
}

pub fn make_decision(
    edge_test: &CorrectedEdgeTestResult,
    min_effective_n: usize,
) -> GoNoGoDecision {
    if edge_test.effective_n < min_effective_n {
        return GoNoGoDecision::InsufficientData;
    }
    // Primary criterion: 95% CI lower bound > 0
    if edge_test.ci_95.0 > 0.0 {
        GoNoGoDecision::Proceed
    } else {
        GoNoGoDecision::DoNotProceed
    }
}
```

### CLI Binary Pattern (matches existing project conventions)
```rust
#[derive(Parser)]
#[command(name = "go-no-go")]
#[command(about = "Statistical validation and go/no-go assessment for arbitrage signals")]
struct Cli {
    #[arg(long)]
    from: Option<NaiveDate>,
    #[arg(long)]
    to: Option<NaiveDate>,
    #[arg(long)]
    last: Option<u32>,
    /// Fraction of data to hold out for testing (0.0-1.0)
    #[arg(long, default_value = "0.3")]
    test_fraction: f64,
    /// Minimum effective sample size for go decision
    #[arg(long, default_value = "30")]
    min_effective_n: usize,
    #[arg(long, default_value = "table")]
    output: OutputFormat,
    #[arg(long, default_value = "spread_logs")]
    spread_dir: PathBuf,
    #[arg(long, default_value = "signal_logs")]
    signal_dir: PathBuf,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw sample count | Effective sample size (ACF correction) | Standard since 1990s | Prevents overstating significance of autocorrelated data |
| Random train/test split | Chronological split for time series | Standard practice | Prevents look-ahead bias |
| Point estimate of edge | Edge with confidence intervals | Standard | Quantifies uncertainty around expected profit |

## Open Questions

1. **How much data is actually available?**
   - What we know: signal_logs has 5 days (Mar 4-9), spread_logs has 1 day (Mar 9)
   - What's unclear: Whether there's enough data for meaningful statistical inference
   - Recommendation: The CLI should handle small-data gracefully and clearly report when n_eff is too low for reliable conclusions. This is informational, not blocking.

2. **Should the go/no-go use spread data or signal data?**
   - What we know: SpreadResult (spread_logs) has net_spread; ArbSignal (signal_logs) has net_edge with cost breakdown
   - What's unclear: Which is the better basis for the go/no-go decision
   - Recommendation: Use signal_logs (ArbSignal) as the primary data source since net_edge includes the validated cost model from Phase 47. Spread data is supplementary. The CLI should accept both data directories.

3. **What threshold for autocorrelation warrants concern?**
   - What we know: ACF > 0.3 is generally considered moderate; > 0.7 is high
   - Recommendation: Report the measured autocorrelation; flag as WARNING if > 0.5

## Sources

### Primary (HIGH confidence)
- Project codebase: `src/analysis/stats.rs`, `scoring.rs`, `sensitivity.rs`, `io.rs`, `output.rs` - verified existing infrastructure
- Project codebase: `src/bin/signal_scoring.rs`, `Cargo.toml` - verified CLI patterns and dependencies
- `statrs` 0.18 already in use for Student's t and Normal CDF

### Secondary (HIGH confidence)
- Effective sample size formula: standard textbook result (Bayley & Hammersley 1946, widely cited)
- Lag-1 ACF formula: standard time series analysis (Box-Jenkins methodology)
- Chronological train/test split: standard practice for time series validation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all patterns verified in codebase
- Architecture: HIGH - follows established project patterns exactly
- Pitfalls: HIGH - well-known statistical pitfalls with clear mitigations
- Autocorrelation formulas: HIGH - textbook statistics, trivial to implement

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable statistical methods, not framework-dependent)
