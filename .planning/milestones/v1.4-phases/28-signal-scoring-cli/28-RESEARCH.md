# Phase 28: Signal Scoring CLI - Research

**Researched:** 2026-02-28
**Domain:** Statistical signal scoring (hit rates, hypothesis testing, Sharpe ratios, drawdown analysis) for a Rust CLI consuming JSONL settlement data
**Confidence:** HIGH

## Summary

Phase 28 implements the statistical scoring engine for the `signal-scoring` CLI binary, replacing the Phase 26 loading-summary placeholder with five distinct analysis sections: hit rate with Wilson CIs (SIGNAL-01), edge t-test (SIGNAL-02), Sharpe ratios (SIGNAL-03), Probabilistic Sharpe Ratio (SIGNAL-04), and max drawdown (SIGNAL-05). The Phase 26 infrastructure -- CLI arg parsing, date-range file loading, dual output format (table + JSON), and `--by-event` flag -- is already in place. The only new work is the computation layer and rendering.

The primary data source is `settlement_logs/settlements-{YYYY-MM-DD}.jsonl`, which contains `AnalysisSettlementRecord` entries. This type currently only derives `Serialize` -- adding `Deserialize` is a prerequisite for this phase. Each record provides `gross_hit` (bool), `net_hit` (bool), `total_net_pnl` (String-encoded Decimal), `total_raw_pnl`, `total_fees`, `total_slippage`, `event_id`, and `settled_at_ms`. This is sufficient to compute all five required metrics: hit rates count booleans, edge uses net_pnl parsed to Decimal/f64, Sharpe uses the P&L series, PSR extends Sharpe with skew/kurtosis, and drawdown walks the cumulative P&L curve.

The `statrs` crate (0.18, already in Cargo.toml) provides `Normal::standard().cdf()` for PSR and Wilson z-scores, and `StudentsT::new(0.0, 1.0, df).unwrap().cdf()` for t-test p-values. The existing `analysis::stats` module provides `wilson_ci()`, `mean_f64()`, and `stddev_f64()`. New functions needed: `skewness_f64()`, `kurtosis_f64()`, and the five scoring computations themselves. No new crate dependencies required.

**Primary recommendation:** Add `Deserialize` to `AnalysisSettlementRecord`, build a `scoring` submodule in `src/analysis/` with five pure computation functions, and wire them into the existing `signal_scoring` binary through a `ScoringResult` struct that renders as both table and JSON.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SIGNAL-01 | Hit rate (gross and net) with Wilson score CIs at 95% and 99%, with sample size (n=X) | `AnalysisSettlementRecord.gross_hit`/`net_hit` booleans provide counts; `wilson_ci()` already in `analysis::stats`; z=1.96 (95%) and z=2.576 (99%); display `n=X` alongside each interval |
| SIGNAL-02 | Cost-adjusted mean edge with t-statistic, p-value, 95% CI answering "is edge distinguishable from zero?" | Parse `total_net_pnl` strings to f64; one-sample t-test: t = mean / (stddev / sqrt(n)); p-value via `statrs::distribution::StudentsT` CDF; CI = mean +/- t_crit * SE |
| SIGNAL-03 | Per-trade Sharpe ratio (primary, no annualization) and frequency-adjusted annualized Sharpe, with PSR showing probability true Sharpe > 0 | Per-trade Sharpe = mean(pnl) / stddev(pnl); annualized = per-trade * sqrt(trades_per_year); PSR per SIGNAL-04 |
| SIGNAL-04 | Probabilistic Sharpe Ratio (PSR): probability that true Sharpe exceeds zero, accounting for skewness and kurtosis | PSR(0) = Phi((SR * sqrt(n-1)) / sqrt(1 - skew*SR + (kurt-1)/4 * SR^2)); requires `skewness_f64()` and `kurtosis_f64()` functions |
| SIGNAL-05 | Maximum drawdown in absolute and percentage terms with start date, trough date, recovery date (or "ongoing") | Walk cumulative P&L series; track running peak; record max drawdown span with timestamps; convert settled_at_ms to dates for display |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| statrs | 0.18 (existing) | Student's t CDF for p-values, Normal CDF for PSR, Normal inverse CDF for z-scores | Already in Cargo.toml; provides `StudentsT::new(0.0, 1.0, df).cdf(t)` and `Normal::standard().cdf(z)` |
| rust_decimal | 1.40 (existing) | Parsing P&L strings from JSONL back to Decimal for precision | `Decimal::from_str()` for P&L values; `to_f64()` for statistical computation |
| chrono | 0.4 (existing) | Converting `settled_at_ms` (i64) to dates for drawdown date display | `DateTime::from_timestamp_millis(ms).date_naive()` for human-readable dates |
| serde/serde_json | 1.0 (existing) | JSONL deserialization and JSON output | Need to add `Deserialize` to `AnalysisSettlementRecord` |
| comfy-table | 7 (existing) | Terminal table rendering for scoring results | Already used in `analysis::output`; five-section table layout |
| clap | 4.5 (existing) | CLI argument parsing | Already in signal_scoring binary |
| anyhow | 1.0 (existing) | Error handling | Already in signal_scoring binary |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| analysis::stats | (project) | `wilson_ci()`, `mean_f64()`, `stddev_f64()`, `mean_decimal()` | Hit rate CIs, edge computation, Sharpe computation |
| analysis::output | (project) | `render_output()`, `new_table()`, `set_numeric_columns()`, `section_header()` | All table/JSON rendering |
| analysis::io | (project) | `load_jsonl()`, `DateRange`, `files_in_dir_prefixed()` | Loading settlement JSONL files |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| statrs for t-distribution CDF | Hand-rolled incomplete beta function | Numerically fragile, already have statrs |
| Parsing AnalysisSettlementRecord | Creating a separate lightweight struct | More types to maintain, same data |
| Settlement logs as data source | Joining signal_logs + paper_trade logs | AnalysisSettlementRecord already has all needed fields (hit booleans, P&L, event_id, timestamp) |

**No new dependencies needed.** Everything required is already in Cargo.toml.

## Architecture Patterns

### Recommended Project Structure

```
src/
  analysis/
    mod.rs                # Add: pub mod scoring;
    stats.rs              # Add: skewness_f64(), kurtosis_f64()
    scoring.rs            # NEW: HitRateResult, EdgeTestResult, SharpeResult, DrawdownResult, ScoringResult
    io.rs                 # Existing (no changes needed)
    output.rs             # Existing (no changes needed)
  bin/
    signal_scoring.rs     # Replace placeholder with scoring computation + rendering
  paper_trade/
    analyzer.rs           # Add Deserialize derive to AnalysisSettlementRecord
```

### Pattern 1: Pure Computation Functions Taking Slices

**What:** Each scoring metric is a pure function taking `&[AnalysisSettlementRecord]` (or pre-extracted `&[f64]`) and returning a result struct.

**When to use:** All five scoring computations.

**Why:** Testable without JSONL files, composable for `--by-event` grouping, same function serves both aggregate and per-event views.

**Example:**
```rust
// src/analysis/scoring.rs
use crate::analysis::stats::{wilson_ci, mean_f64, stddev_f64};

#[derive(Debug, Clone, Serialize)]
pub struct HitRateResult {
    pub gross_hits: usize,
    pub net_hits: usize,
    pub total: usize,
    pub gross_rate: f64,
    pub net_rate: f64,
    pub gross_ci_95: (f64, f64),
    pub gross_ci_99: (f64, f64),
    pub net_ci_95: (f64, f64),
    pub net_ci_99: (f64, f64),
}

pub fn compute_hit_rates(records: &[AnalysisSettlementRecord]) -> Option<HitRateResult> {
    let total = records.len();
    if total == 0 { return None; }
    let gross_hits = records.iter().filter(|r| r.gross_hit).count();
    let net_hits = records.iter().filter(|r| r.net_hit).count();
    Some(HitRateResult {
        gross_hits,
        net_hits,
        total,
        gross_rate: gross_hits as f64 / total as f64,
        net_rate: net_hits as f64 / total as f64,
        gross_ci_95: wilson_ci(gross_hits, total, 1.96)?,
        gross_ci_99: wilson_ci(gross_hits, total, 2.576)?,
        net_ci_95: wilson_ci(net_hits, total, 1.96)?,
        net_ci_99: wilson_ci(net_hits, total, 2.576)?,
    })
}
```

### Pattern 2: Extract-Then-Compute for P&L Series

**What:** Extract `total_net_pnl` strings to `Vec<f64>` once, then pass the slice to all P&L-dependent functions (edge, Sharpe, PSR, drawdown).

**When to use:** Edge test, Sharpe, PSR, and drawdown all need the P&L series.

**Why:** Avoids re-parsing strings in each function. Single extraction point handles parse errors consistently.

**Example:**
```rust
fn extract_pnl_series(records: &[AnalysisSettlementRecord]) -> Vec<f64> {
    records.iter()
        .filter_map(|r| r.total_net_pnl.parse::<f64>().ok())
        .collect()
}

// Then pass &pnl[..] to edge_test(), sharpe_ratio(), psr(), max_drawdown()
```

### Pattern 3: ScoringResult Composite Struct

**What:** A single `ScoringResult` struct aggregating all five sub-results, rendered as one table (with section headers) or one JSON object.

**When to use:** Both aggregate and per-event output.

**Why:** The `render_output()` function takes a single `T: Serialize`. One struct = one render call. Section headers in the table separate the five areas visually.

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ScoringResult {
    pub hit_rates: Option<HitRateResult>,
    pub edge_test: Option<EdgeTestResult>,
    pub sharpe: Option<SharpeResult>,
    pub drawdown: Option<DrawdownResult>,
}
```

### Pattern 4: By-Event Grouping via HashMap

**What:** Group records by `event_id`, compute ScoringResult for each group, render per-event sections.

**When to use:** When `--by-event` flag is set.

**Why:** Same computation functions, different input slices. No code duplication.

```rust
fn group_by_event(records: &[AnalysisSettlementRecord]) -> HashMap<String, Vec<&AnalysisSettlementRecord>> {
    let mut groups: HashMap<String, Vec<&AnalysisSettlementRecord>> = HashMap::new();
    for record in records {
        groups.entry(record.event_id.clone()).or_default().push(record);
    }
    groups
}
```

### Pattern 5: Sorted Chronological P&L for Drawdown

**What:** Sort records by `settled_at_ms` before computing drawdown (and Sharpe, since Sharpe assumes time-ordered returns).

**When to use:** Drawdown requires chronological order. Sharpe annualization requires time-ordered trades.

**Why:** JSONL files may have out-of-order settlements (e.g., two events settle on the same day but one is logged first despite settling second).

### Anti-Patterns to Avoid

- **Computing hit rate from P&L signs instead of the boolean fields:** `AnalysisSettlementRecord` has `gross_hit` and `net_hit` booleans that were computed at settlement time with the full cost model. Re-deriving from P&L sign could differ due to rounding or zero-P&L edge cases. Use the booleans.

- **Annualizing Sharpe without a frequency estimate:** The success criteria require "frequency-adjusted annualized Sharpe." You need trades_per_year = (trades / observation_days) * 365. Do not assume 252 trading days -- prediction markets trade 24/7/365.

- **Using population variance (n) instead of sample variance (n-1):** All existing stats use Bessel's correction (n-1 denominator). The t-test, Sharpe ratio, and PSR all require sample statistics. Be consistent with `stddev_f64()` which already uses n-1.

- **Treating PSR formula kurtosis as excess kurtosis:** The Bailey/Lopez de Prado PSR formula uses **excess kurtosis** (gamma_4 - 3 for the "excess" part, or the "gamma" parameter in the formula is already excess kurtosis depending on source). The formula as presented in the canonical reference uses gamma_4 as "kurtosis" where gamma_4 of a normal distribution is 3, and the formula has `(gamma_4 - 1)/4` which accounts for this. Be precise about which kurtosis definition you use. See Code Examples section.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Student's t CDF | Numerical integration or series approximation | `statrs::distribution::StudentsT::new(0.0, 1.0, df).cdf(t_stat)` | Regularized incomplete beta function is numerically delicate; statrs is battle-tested |
| Normal CDF | Hand-coded erf approximation | `statrs::distribution::Normal::standard().cdf(z)` | Already used in pricing module; precision matters for PSR |
| Wilson score CI | Wald interval (p +/- z*SE) | `analysis::stats::wilson_ci()` | Wald overshoots [0,1] at small n; Wilson already implemented and tested |
| Terminal table rendering | Manual format strings | `analysis::output::{new_table, set_numeric_columns, section_header, render_output}` | Already built in Phase 26; handles alignment, section headers, JSON dual-output |
| Date range file loading | Manual file enumeration | `DateRange::files_in_dir_prefixed()` | Handles date iteration, file existence checks, prefix patterns |

**Key insight:** The statistical formulas themselves are straightforward arithmetic -- the value is in getting the edge cases right (n=0, n=1, zero variance, negative Sharpe) and using proper distributions for p-values.

## Common Pitfalls

### Pitfall 1: AnalysisSettlementRecord Missing Deserialize

**What goes wrong:** `load_jsonl::<AnalysisSettlementRecord>()` fails to compile because the type only derives `Serialize`.

**Why it happens:** The type was created for write-only logging in the runtime; Phase 28 is the first consumer that reads it back.

**How to avoid:** Add `Deserialize` to the derive macro: `#[derive(Debug, Clone, Serialize, Deserialize)]`. The `total_raw_pnl`, `total_net_pnl`, `total_fees`, `total_slippage` fields are `String` (not Decimal), so standard String deserialization works. `ThresholdStatus` already derives Deserialize.

**Warning signs:** Compile error: "the trait bound `AnalysisSettlementRecord: Deserialize<'_>` is not satisfied."

### Pitfall 2: Settlement Log File Prefix

**What goes wrong:** Using `files_in_dir()` (no prefix) instead of `files_in_dir_prefixed("settlements-")` for settlement logs.

**Why it happens:** Signal logs use no prefix (`{YYYY-MM-DD}.jsonl`) but settlement logs use `settlements-{YYYY-MM-DD}.jsonl`.

**How to avoid:** The signal_scoring binary needs TWO data sources: signal logs (no prefix, for counting total signals if needed) and settlement logs (prefixed, for the actual scoring). Primary data source is settlement logs. Use `range.files_in_dir_prefixed(&settlement_dir, "settlements-")`.

**Warning signs:** Zero records loaded despite settlement data existing on disk.

### Pitfall 3: Division by Zero in Statistics

**What goes wrong:** Sharpe ratio with n=0 or n=1 records, t-test with n=1 (stddev undefined), PSR with SR=0 and denominator=0.

**Why it happens:** Small sample sizes in per-event breakdowns or short date ranges.

**How to avoid:** Every computation function must return `Option<T>` and handle degenerate cases:
- Hit rate: needs n >= 1
- Edge t-test: needs n >= 2 (stddev requires 2+ values)
- Sharpe: needs n >= 2
- PSR: needs n >= 2 and SR != 0 (denominator involves SR)
- Drawdown: needs n >= 1

**Warning signs:** NaN or Inf in output, panics on division.

### Pitfall 4: Kurtosis Definition Confusion in PSR

**What goes wrong:** Using excess kurtosis (normal = 0) vs. raw kurtosis (normal = 3) inconsistently in the PSR formula.

**Why it happens:** Different sources define kurtosis differently. The PSR paper by Bailey & Lopez de Prado uses a formula where kurtosis gamma_4 has normal=3 (raw kurtosis), and the formula contains `(gamma_4 - 1)/4 * SR^2`. Some implementations use excess kurtosis (gamma_4 - 3) which changes the formula.

**How to avoid:** Implement `kurtosis_f64()` as **excess kurtosis** (normal distribution = 0). Then in the PSR formula, use `(kurtosis + 3 - 1)/4 = (kurtosis + 2)/4` for the kurtosis term. Alternatively, implement raw kurtosis and use `(kurtosis - 1)/4` directly. Document which convention is used. The canonical PSR formula from Bailey & Lopez de Prado (2012):

```
PSR(0) = Phi( (SR * sqrt(n-1)) / sqrt(1 - skew*SR + (kurt-1)/4 * SR^2) )
```

where `kurt` is raw kurtosis (normal = 3). If you compute excess kurtosis (normal = 0), substitute `(excess_kurt + 2)/4`.

**Warning signs:** PSR > 1.0 or PSR < 0.0 (should be a probability in [0, 1]), or PSR values that seem unreasonable for the observed Sharpe.

### Pitfall 5: Drawdown Dates From Millisecond Timestamps

**What goes wrong:** Displaying raw `settled_at_ms` values instead of human-readable dates.

**Why it happens:** `settled_at_ms` is `i64` milliseconds since epoch. The success criteria require "drawdown start date, trough date, and recovery date."

**How to avoid:** Convert with `DateTime::from_timestamp_millis(ms).map(|dt| dt.date_naive())` or `DateTime::from_timestamp(ms / 1000, 0)`. Handle the case where `from_timestamp_millis` returns `None` for invalid timestamps.

**Warning signs:** Dates displayed as large integers.

### Pitfall 6: Frequency Estimation for Annualized Sharpe

**What goes wrong:** Annualizing Sharpe by multiplying by sqrt(252) -- the stock market convention.

**Why it happens:** Most Sharpe ratio tutorials assume daily stock returns with 252 trading days.

**How to avoid:** Prediction markets trade 24/7/365. Compute actual trading frequency: `trades_per_year = total_trades / observation_period_in_years`. Observation period = (last_settlement_ms - first_settlement_ms) in years. Then annualized Sharpe = per_trade_sharpe * sqrt(trades_per_year). If observation period is zero (all trades on same day), annualized Sharpe is undefined (return None).

**Warning signs:** Absurdly large annualized Sharpe (> 10) from short observation periods.

## Code Examples

Verified patterns from codebase analysis and official documentation:

### One-Sample T-Test (Edge Significance)

```rust
// src/analysis/scoring.rs
use statrs::distribution::{ContinuousCDF, StudentsT};

#[derive(Debug, Clone, Serialize)]
pub struct EdgeTestResult {
    pub mean_edge: f64,
    pub std_error: f64,
    pub t_statistic: f64,
    pub p_value: f64,       // two-tailed
    pub ci_95: (f64, f64),  // 95% CI for mean edge
    pub n: usize,
}

pub fn compute_edge_test(pnl: &[f64]) -> Option<EdgeTestResult> {
    if pnl.len() < 2 { return None; }
    let n = pnl.len();
    let mean = mean_f64(pnl)?;
    let sd = stddev_f64(pnl)?;
    if sd == 0.0 { return None; }
    let se = sd / (n as f64).sqrt();
    let t_stat = mean / se;  // H0: mean = 0
    let df = (n - 1) as f64;

    // Two-tailed p-value via Student's t CDF
    let t_dist = StudentsT::new(0.0, 1.0, df).ok()?;
    let p_value = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));

    // 95% CI: mean +/- t_crit * SE
    let t_crit = t_dist.inverse_cdf(0.975);  // two-tailed 95%
    let ci_95 = (mean - t_crit * se, mean + t_crit * se);

    Some(EdgeTestResult { mean_edge: mean, std_error: se, t_statistic: t_stat, p_value, ci_95, n })
}
```

### Sharpe Ratio (Per-Trade and Annualized)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SharpeResult {
    pub per_trade_sharpe: f64,
    pub annualized_sharpe: Option<f64>,  // None if observation period is zero
    pub trades_per_year: Option<f64>,
    pub psr: Option<f64>,                // Probabilistic Sharpe Ratio
    pub n: usize,
}

pub fn compute_sharpe(pnl: &[f64], first_ms: i64, last_ms: i64) -> Option<SharpeResult> {
    if pnl.len() < 2 { return None; }
    let mean = mean_f64(pnl)?;
    let sd = stddev_f64(pnl)?;
    if sd == 0.0 { return None; }

    let per_trade_sharpe = mean / sd;

    // Frequency-adjusted annualization
    let obs_years = (last_ms - first_ms) as f64 / (365.25 * 24.0 * 3600.0 * 1000.0);
    let (annualized, trades_per_year) = if obs_years > 0.0 {
        let tpy = pnl.len() as f64 / obs_years;
        (Some(per_trade_sharpe * tpy.sqrt()), Some(tpy))
    } else {
        (None, None)
    };

    // PSR: probability true Sharpe > 0
    let psr = compute_psr(pnl, per_trade_sharpe);

    Some(SharpeResult {
        per_trade_sharpe,
        annualized_sharpe: annualized,
        trades_per_year,
        psr,
        n: pnl.len(),
    })
}
```

### Probabilistic Sharpe Ratio (PSR)

```rust
use statrs::distribution::{ContinuousCDF, Normal};

/// Compute PSR(0): probability that true Sharpe > 0.
///
/// Formula: PSR(0) = Phi( SR * sqrt(n-1) / sqrt(1 - skew*SR + (kurt-1)/4 * SR^2) )
/// where kurt is RAW kurtosis (normal=3).
///
/// Source: Bailey & Lopez de Prado, "The Sharpe Ratio Efficient Frontier" (2012)
pub fn compute_psr(pnl: &[f64], sharpe: f64) -> Option<f64> {
    let n = pnl.len();
    if n < 2 { return None; }

    let skew = skewness_f64(pnl)?;
    let excess_kurt = kurtosis_f64(pnl)?;  // excess kurtosis, normal = 0
    let raw_kurt = excess_kurt + 3.0;       // raw kurtosis, normal = 3

    let numerator = sharpe * ((n - 1) as f64).sqrt();
    let denominator_sq = 1.0 - skew * sharpe + (raw_kurt - 1.0) / 4.0 * sharpe * sharpe;
    if denominator_sq <= 0.0 { return None; }  // degenerate case

    let z = numerator / denominator_sq.sqrt();
    let norm = Normal::standard();
    Some(norm.cdf(z))
}
```

### Skewness and Kurtosis (New Stats Functions)

```rust
// src/analysis/stats.rs -- additions

/// Sample skewness (Fisher's definition, bias-corrected).
/// Returns None if fewer than 3 values.
pub fn skewness_f64(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 3 { return None; }
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let m2: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / nf;
    let m3: f64 = values.iter().map(|v| (v - mean).powi(3)).sum::<f64>() / nf;
    if m2 == 0.0 { return None; }
    let g1 = m3 / m2.powf(1.5);
    // Bias correction factor: sqrt(n*(n-1)) / (n-2)
    let correction = ((nf * (nf - 1.0)).sqrt()) / (nf - 2.0);
    Some(g1 * correction)
}

/// Sample excess kurtosis (Fisher's definition, normal = 0).
/// Returns None if fewer than 4 values.
pub fn kurtosis_f64(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 4 { return None; }
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let m2: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / nf;
    let m4: f64 = values.iter().map(|v| (v - mean).powi(4)).sum::<f64>() / nf;
    if m2 == 0.0 { return None; }
    let raw_kurt = m4 / (m2 * m2);
    // Bias correction: (n-1)/((n-2)*(n-3)) * ((n+1)*raw_kurt - 3*(n-1)) + 3
    // Then subtract 3 for excess kurtosis
    let excess = ((nf - 1.0) / ((nf - 2.0) * (nf - 3.0)))
        * ((nf + 1.0) * raw_kurt - 3.0 * (nf - 1.0));
    Some(excess)
}
```

### Maximum Drawdown

```rust
#[derive(Debug, Clone, Serialize)]
pub struct DrawdownResult {
    pub max_drawdown_abs: f64,
    pub max_drawdown_pct: Option<f64>,  // None if peak is zero
    pub peak_date: String,              // YYYY-MM-DD
    pub trough_date: String,            // YYYY-MM-DD
    pub recovery_date: Option<String>,  // None = "ongoing"
    pub current_drawdown_abs: f64,
    pub current_drawdown_pct: Option<f64>,
}

/// Compute maximum drawdown from chronologically sorted P&L series.
/// `timestamps_ms` must be same length as `pnl` and sorted ascending.
pub fn compute_max_drawdown(pnl: &[f64], timestamps_ms: &[i64]) -> Option<DrawdownResult> {
    if pnl.is_empty() { return None; }

    // Build cumulative P&L curve
    let mut cumulative = Vec::with_capacity(pnl.len());
    let mut running = 0.0_f64;
    for &p in pnl {
        running += p;
        cumulative.push(running);
    }

    // Walk the curve tracking peak and max drawdown
    let mut peak = cumulative[0];
    let mut peak_idx = 0;
    let mut max_dd = 0.0_f64;
    let mut max_dd_peak_idx = 0;
    let mut max_dd_trough_idx = 0;

    for (i, &val) in cumulative.iter().enumerate() {
        if val > peak {
            peak = val;
            peak_idx = i;
        }
        let dd = peak - val;
        if dd > max_dd {
            max_dd = dd;
            max_dd_peak_idx = peak_idx;
            max_dd_trough_idx = i;
        }
    }

    // Find recovery: first index after trough where cumulative >= peak at max_dd_peak_idx
    let peak_at_dd = cumulative[max_dd_peak_idx];
    let recovery_idx = cumulative[max_dd_trough_idx..]
        .iter()
        .position(|&v| v >= peak_at_dd)
        .map(|offset| max_dd_trough_idx + offset);

    // Current drawdown
    let current_peak = cumulative.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let current_val = *cumulative.last().unwrap();
    let current_dd = current_peak - current_val;

    // Convert indices to dates
    let to_date = |idx: usize| -> String {
        chrono::DateTime::from_timestamp_millis(timestamps_ms[idx])
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    let max_dd_pct = if peak_at_dd != 0.0 { Some(max_dd / peak_at_dd.abs() * 100.0) } else { None };
    let current_dd_pct = if current_peak != 0.0 { Some(current_dd / current_peak.abs() * 100.0) } else { None };

    Some(DrawdownResult {
        max_drawdown_abs: max_dd,
        max_drawdown_pct: max_dd_pct,
        peak_date: to_date(max_dd_peak_idx),
        trough_date: to_date(max_dd_trough_idx),
        recovery_date: recovery_idx.map(|idx| to_date(idx)),
        current_drawdown_abs: current_dd,
        current_drawdown_pct: current_dd_pct,
    })
}
```

### Table Rendering with Section Headers

```rust
use crate::analysis::output::{new_table, set_numeric_columns, section_header, render_output};

fn scoring_table(result: &ScoringResult) -> Table {
    let mut table = new_table(&["Metric", "Value"]);
    set_numeric_columns(&mut table, &[1]);

    // Section 1: Hit Rates
    if let Some(ref hr) = result.hit_rates {
        section_header(&mut table, "=== HIT RATES ===", 2);
        table.add_row(vec![
            format!("Gross Hit Rate (n={})", hr.total),
            format!("{:.1}%", hr.gross_rate * 100.0),
        ]);
        table.add_row(vec![
            "  95% CI".to_string(),
            format!("[{:.1}%, {:.1}%]", hr.gross_ci_95.0 * 100.0, hr.gross_ci_95.1 * 100.0),
        ]);
        // ... more rows
    }

    // Section 2: Edge Test
    // Section 3: Sharpe
    // Section 4: Drawdown

    table
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Wald interval for proportions | Wilson score interval | Standard since 1990s rediscovery | Wilson correct at all sample sizes; Wald overshoots [0,1] |
| Sharpe ratio alone | Sharpe + PSR | Bailey & Lopez de Prado (2012) | PSR accounts for skew/kurtosis, gives probability not point estimate |
| Annualized Sharpe * sqrt(252) | Frequency-adjusted annualization | Common correction | 252 assumes stock trading days; prediction markets are 365 |
| Simple max drawdown | Max drawdown with dates and recovery tracking | Always best practice | Dates make drawdowns actionable (e.g., correlate with market events) |

**Deprecated/outdated:**
- Wald confidence interval for proportions: should never be used; Wilson score is strictly better at small n and equivalent at large n
- Assuming normal returns for Sharpe: PSR explicitly incorporates non-normality through skewness/kurtosis

## Open Questions

1. **Settlement log data availability**
   - What we know: `settlement_logs/` directory does not yet exist on disk (no settled positions during soak test so far). The system creates `settlements-{YYYY-MM-DD}.jsonl` files when positions settle.
   - What's unclear: Whether there will be settlement data by the time this phase is tested.
   - Recommendation: Build the scoring code to handle n=0 gracefully (display "Insufficient data: 0 settled positions"). Unit tests should use synthetic data. Integration tests with `tempfile` should write test JSONL files.

2. **Signal-to-settlement join for SIGNAL-01 denominator**
   - What we know: The success criteria say "hit rate" which implies successes/total. `AnalysisSettlementRecord` only has settled positions. The "total" for hit rate is the number of settled positions, not total signals fired.
   - What's unclear: Whether the user wants hit rate as "settled_hits / settled_total" (only settled positions) or "settled_hits / all_signals_fired" (including unsettled). The STATE.md blocker mentions "Settlement correlation join logic" needs investigation.
   - Recommendation: Use settled positions as the denominator (settled_hits / settled_total). This is the statistically sound denominator -- unsettled signals have unknown outcomes and cannot contribute to a binary success/failure metric. The `n=X` display makes the sample size explicit.

3. **Drawdown percentage base**
   - What we know: Success criteria say "absolute and percentage terms." Percentage drawdown typically means peak-to-trough / peak.
   - What's unclear: Whether "peak" means peak cumulative P&L (which could be negative early on) or initial capital base.
   - Recommendation: Use peak cumulative P&L as the base. If peak is zero or negative, percentage is undefined (display "N/A"). This is the standard drawdown convention for a P&L curve starting from zero.

## Sources

### Primary (HIGH confidence)
- Direct source analysis: `src/paper_trade/analyzer.rs` -- `AnalysisSettlementRecord` struct (line 84), only derives `Serialize` not `Deserialize`; all fields verified
- Direct source analysis: `src/analysis/stats.rs` -- existing `wilson_ci()`, `mean_f64()`, `stddev_f64()`, `percentile_f64()` functions
- Direct source analysis: `src/analysis/output.rs` -- `render_output()`, `new_table()`, `set_numeric_columns()`, `section_header()` helpers
- Direct source analysis: `src/analysis/io.rs` -- `load_jsonl()`, `DateRange`, `files_in_dir_prefixed()` method
- Direct source analysis: `src/bin/signal_scoring.rs` -- Phase 26 placeholder binary, ready for scoring integration
- Direct source analysis: `src/paper_trade/tracker.rs` -- Settlement logger writes `settlements-{YYYY-MM-DD}.jsonl` files (line 200)
- Direct source analysis: `Cargo.toml` -- `statrs = "0.18"` already present, no new dependencies needed
- [statrs StudentsT docs](https://docs.rs/statrs/0.18.0/statrs/distribution/struct.StudentsT.html) -- verified constructor `StudentsT::new(location, scale, freedom)`, `cdf()`, `inverse_cdf()` methods

### Secondary (MEDIUM confidence)
- [Bailey & Lopez de Prado PSR formula](https://portfoliooptimizer.io/blog/the-probabilistic-sharpe-ratio-bias-adjustment-confidence-intervals-hypothesis-testing-and-minimum-track-record-length/) -- PSR formula with standard error incorporating skewness and kurtosis; cross-verified with [quantdare.com/probabilistic-sharpe-ratio/](https://quantdare.com/probabilistic-sharpe-ratio/) and [QuantConnect research](https://www.quantconnect.com/research/17112/probabilistic-sharpe-ratio/)
- [Wilson score interval](https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval) -- formula verified against existing `wilson_ci()` implementation in codebase

### Tertiary (LOW confidence)
- None. All findings verified from primary sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already in Cargo.toml; no new crates; `statrs` StudentsT verified from docs.rs
- Architecture: HIGH -- builds directly on Phase 26 infrastructure; pattern follows existing spread_analytics binary structure
- Pitfalls: HIGH -- AnalysisSettlementRecord Deserialize gap verified from source; file prefix naming verified from tracker.rs; PSR formula cross-verified across 3 sources
- Statistical formulas: HIGH -- Wilson CI already implemented and tested in codebase; t-test is standard; PSR formula verified across multiple academic/professional sources
- Data model: HIGH -- AnalysisSettlementRecord fields verified; settlement log file naming convention verified from tracker.rs line 200

**Research date:** 2026-02-28
**Valid until:** 2026-03-28 (stable -- statistical formulas don't change; infrastructure from Phase 26 is frozen)
