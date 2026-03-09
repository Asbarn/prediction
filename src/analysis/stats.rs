use rust_decimal::Decimal;
use serde::Serialize;
use statrs::distribution::{ContinuousCDF, StudentsT};

/// Arithmetic mean of Decimal values using full-precision Decimal arithmetic.
/// Returns None if the slice is empty.
pub fn mean_decimal(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    let sum: Decimal = values.iter().copied().sum();
    Some(sum / Decimal::from(values.len()))
}

/// Arithmetic mean of f64 values.
/// Returns None if the slice is empty.
pub fn mean_f64(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let n = values.len() as f64;
    Some(values.iter().sum::<f64>() / n)
}

/// Sample standard deviation (Bessel's correction, n-1 denominator).
/// Returns None if fewer than 2 values.
pub fn stddev_f64(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values
        .iter()
        .map(|v| {
            let d = v - mean;
            d * d
        })
        .sum::<f64>()
        / (n - 1.0);
    Some(variance.sqrt())
}

/// Percentile using linear interpolation. Caller must pre-sort the slice.
/// `p` is 0-100. Returns None if slice is empty.
///
/// Follows the rank = p/100 * (n-1), floor/ceil/frac interpolation pattern
/// from `src/spread/rolling_stats.rs`.
pub fn percentile_f64(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }

    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let frac = rank - lower as f64;

    if lower == upper || upper >= sorted.len() {
        Some(sorted[lower.min(sorted.len() - 1)])
    } else {
        Some(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
    }
}

/// Median (convenience wrapper for the 50th percentile).
/// Caller must pre-sort the slice. Returns None if empty.
pub fn median_f64(sorted: &[f64]) -> Option<f64> {
    percentile_f64(sorted, 50.0)
}

/// Wilson score confidence interval for a proportion.
///
/// Returns `(lower, upper)` bounds for the estimated proportion
/// `successes / total` at the given z-score confidence level
/// (e.g., z = 1.96 for 95% CI).
///
/// Returns None if `total == 0`.
pub fn wilson_ci(successes: usize, total: usize, z: f64) -> Option<(f64, f64)> {
    if total == 0 {
        return None;
    }
    let n = total as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = (z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / denom;
    Some((center - margin, center + margin))
}

/// Fisher's bias-corrected sample skewness.
/// Returns None if n < 3 or variance is zero.
pub fn skewness_f64(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 3 {
        return None;
    }
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let m2: f64 = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / nf;
    if m2 == 0.0 {
        return None;
    }
    let m3: f64 = values.iter().map(|x| (x - mean).powi(3)).sum::<f64>() / nf;
    let g1 = m3 / m2.powf(1.5);
    let correction = (nf * (nf - 1.0)).sqrt() / (nf - 2.0);
    Some(g1 * correction)
}

/// Fisher's excess kurtosis (normal distribution = 0).
/// Returns None if n < 4 or variance is zero.
pub fn kurtosis_f64(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 4 {
        return None;
    }
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let m2: f64 = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / nf;
    if m2 == 0.0 {
        return None;
    }
    let m4: f64 = values.iter().map(|x| (x - mean).powi(4)).sum::<f64>() / nf;
    let raw_kurt = m4 / (m2 * m2);
    let excess = ((nf - 1.0) / ((nf - 2.0) * (nf - 3.0)))
        * ((nf + 1.0) * raw_kurt - 3.0 * (nf - 1.0));
    Some(excess)
}

/// Pearson product-moment correlation coefficient between two samples.
///
/// Returns `None` if fewer than 2 paired observations, or if either sample
/// has zero variance (correlation undefined).
pub fn pearson_correlation(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len().min(y.len());
    if n < 2 {
        return None;
    }

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_yy = 0.0;
    let mut sum_xy = 0.0;

    for i in 0..n {
        sum_x += x[i];
        sum_y += y[i];
        sum_xx += x[i] * x[i];
        sum_yy += y[i] * y[i];
        sum_xy += x[i] * y[i];
    }

    let nf = n as f64;
    let var_x = sum_xx - sum_x * sum_x / nf;
    let var_y = sum_yy - sum_y * sum_y / nf;

    if var_x <= 0.0 || var_y <= 0.0 {
        return None;
    }

    let cov = sum_xy - sum_x * sum_y / nf;
    Some(cov / (var_x.sqrt() * var_y.sqrt()))
}

/// Result of a two-sample Kolmogorov-Smirnov test.
#[derive(Debug, Clone, Serialize)]
pub struct KsTestResult {
    /// Maximum absolute difference between the two ECDFs.
    pub statistic: f64,
    /// Asymptotic p-value for the KS statistic.
    pub p_value: f64,
    /// Size of the first sample.
    pub n1: usize,
    /// Size of the second sample.
    pub n2: usize,
}

/// Two-sample Kolmogorov-Smirnov test.
///
/// Returns `None` if either sample is empty. Computes the KS statistic
/// (maximum ECDF difference) and an asymptotic p-value using
/// `p = 2 * exp(-2 * n_eff * D^2)` where `n_eff = n1*n2 / (n1+n2)`.
pub fn ks_test_two_sample(sample1: &[f64], sample2: &[f64]) -> Option<KsTestResult> {
    if sample1.is_empty() || sample2.is_empty() {
        return None;
    }

    let n1 = sample1.len();
    let n2 = sample2.len();

    let mut sorted1 = sample1.to_vec();
    let mut sorted2 = sample2.to_vec();
    sorted1.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted2.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut i = 0usize;
    let mut j = 0usize;
    let mut d_max: f64 = 0.0;

    while i < n1 || j < n2 {
        let v1 = if i < n1 { sorted1[i] } else { f64::INFINITY };
        let v2 = if j < n2 { sorted2[j] } else { f64::INFINITY };

        if v1 <= v2 {
            i += 1;
        }
        if v2 <= v1 {
            j += 1;
        }

        let ecdf1 = i as f64 / n1 as f64;
        let ecdf2 = j as f64 / n2 as f64;
        let d = (ecdf1 - ecdf2).abs();
        if d > d_max {
            d_max = d;
        }
    }

    let n_eff = (n1 as f64 * n2 as f64) / (n1 + n2) as f64;
    let p_value = (2.0 * (-2.0 * n_eff * d_max * d_max).exp()).clamp(0.0, 1.0);

    Some(KsTestResult {
        statistic: d_max,
        p_value,
        n1,
        n2,
    })
}

/// Lag-1 autocorrelation coefficient for an f64 time series.
///
/// Returns None if fewer than 3 observations or if the series has zero variance.
/// Formula: ACF(1) = sum((x_t - mean) * (x_{t+1} - mean)) / sum((x_t - mean)^2)
pub fn autocorrelation_lag1(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;

    let denom: f64 = values.iter().map(|x| (x - mean).powi(2)).sum();
    if denom == 0.0 {
        return None;
    }

    let numer: f64 = values
        .windows(2)
        .map(|w| (w[0] - mean) * (w[1] - mean))
        .sum();

    Some(numer / denom)
}

/// Effective sample size corrected for serial correlation.
///
/// Formula: n_eff = n * (1 - rho) / (1 + rho).
/// When rho <= 0 or n < 3, returns raw n. Minimum return value is 2.
pub fn effective_sample_size(n: usize, rho: f64) -> usize {
    if rho <= 0.0 || n < 3 {
        return n;
    }
    let n_eff = (n as f64) * (1.0 - rho) / (1.0 + rho);
    (n_eff.round() as usize).max(2)
}

/// Result of an autocorrelation-corrected one-sample t-test (H0: mean = 0).
#[derive(Debug, Clone, Serialize)]
pub struct CorrectedEdgeTestResult {
    pub mean_edge: f64,
    pub std_error: f64,
    pub t_statistic: f64,
    pub p_value: f64,
    pub ci_95: (f64, f64),
    pub raw_n: usize,
    pub effective_n: usize,
    pub autocorrelation: f64,
}

/// Autocorrelation-corrected one-sample t-test (H0: mean = 0).
///
/// Uses effective sample size for standard error divisor and degrees of freedom.
/// Returns None if fewer than 3 values or zero standard deviation.
pub fn compute_corrected_edge_test(values: &[f64]) -> Option<CorrectedEdgeTestResult> {
    if values.len() < 3 {
        return None;
    }
    let raw_n = values.len();
    let mean = mean_f64(values)?;
    let sd = stddev_f64(values)?;
    if sd == 0.0 {
        return None;
    }

    let rho = autocorrelation_lag1(values).unwrap_or(0.0);
    let n_eff = effective_sample_size(raw_n, rho);
    let n_eff_f = n_eff as f64;

    let std_error = sd / n_eff_f.sqrt();
    let t_stat = mean / std_error;
    let df = n_eff_f - 1.0;

    let t_dist = StudentsT::new(0.0, 1.0, df).ok()?;
    let p_value = 2.0 * (1.0 - t_dist.cdf(t_stat.abs()));

    // 95% CI using t critical value
    let t_crit = t_dist.inverse_cdf(0.975);
    let ci_95 = (mean - t_crit * std_error, mean + t_crit * std_error);

    Some(CorrectedEdgeTestResult {
        mean_edge: mean,
        std_error,
        t_statistic: t_stat,
        p_value,
        ci_95,
        raw_n,
        effective_n: n_eff,
        autocorrelation: rho,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(mean_decimal(&[]), None);
        assert_eq!(mean_f64(&[]), None);
        assert_eq!(stddev_f64(&[]), None);
        assert_eq!(percentile_f64(&[], 50.0), None);
        assert_eq!(median_f64(&[]), None);
        assert_eq!(wilson_ci(0, 0, 1.96), None);
    }

    #[test]
    fn single_value_mean_returns_value() {
        assert_eq!(mean_f64(&[42.0]), Some(42.0));
        assert_eq!(mean_decimal(&[dec!(42)]), Some(dec!(42)));
    }

    #[test]
    fn single_value_stddev_returns_none() {
        // n < 2 for sample stddev
        assert_eq!(stddev_f64(&[42.0]), None);
    }

    #[test]
    fn single_value_percentile_returns_value() {
        assert_eq!(percentile_f64(&[42.0], 0.0), Some(42.0));
        assert_eq!(percentile_f64(&[42.0], 50.0), Some(42.0));
        assert_eq!(percentile_f64(&[42.0], 100.0), Some(42.0));
    }

    #[test]
    fn known_sequence_mean() {
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((mean_f64(&vals).unwrap() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn known_sequence_stddev() {
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0];
        let expected = (2.5_f64).sqrt(); // sample stddev = sqrt(2.5)
        assert!((stddev_f64(&vals).unwrap() - expected).abs() < 1e-10);
    }

    #[test]
    fn known_sequence_median() {
        let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((median_f64(&sorted).unwrap() - 3.0).abs() < 1e-10);
    }

    #[test]
    fn known_sequence_percentiles() {
        let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
        // p25: rank = 0.25 * 4 = 1.0 -> values[1] = 2.0
        assert!((percentile_f64(&sorted, 25.0).unwrap() - 2.0).abs() < 1e-10);
        // p75: rank = 0.75 * 4 = 3.0 -> values[3] = 4.0
        assert!((percentile_f64(&sorted, 75.0).unwrap() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn wilson_ci_known_values() {
        // wilson_ci(7, 10, 1.96) should produce bounds approximately (0.3968, 0.8922)
        let (lower, upper) = wilson_ci(7, 10, 1.96).unwrap();
        assert!(
            (lower - 0.3968).abs() < 0.001,
            "Wilson CI lower bound: expected ~0.3968, got {lower}"
        );
        assert!(
            (upper - 0.8922).abs() < 0.001,
            "Wilson CI upper bound: expected ~0.8922, got {upper}"
        );
    }

    #[test]
    fn wilson_ci_zero_total_returns_none() {
        assert_eq!(wilson_ci(0, 0, 1.96), None);
    }

    #[test]
    fn mean_decimal_known_values() {
        let vals = [dec!(10), dec!(20), dec!(30)];
        assert_eq!(mean_decimal(&vals), Some(dec!(20)));
    }

    #[test]
    fn mean_decimal_precision() {
        // Verify Decimal arithmetic preserves precision
        let vals = [dec!(1), dec!(2), dec!(3)];
        // 6 / 3 = 2 exactly
        assert_eq!(mean_decimal(&vals), Some(dec!(2)));
    }

    #[test]
    fn skewness_symmetric_is_zero() {
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0];
        let skew = skewness_f64(&vals).unwrap();
        assert!(
            skew.abs() < 1e-10,
            "Symmetric distribution should have skewness ~0, got {skew}"
        );
    }

    #[test]
    fn skewness_right_skewed() {
        let vals = [1.0, 1.0, 1.0, 1.0, 10.0];
        let skew = skewness_f64(&vals).unwrap();
        assert!(
            skew > 0.0,
            "Right-skewed distribution should have positive skewness, got {skew}"
        );
    }

    #[test]
    fn skewness_too_few_returns_none() {
        assert_eq!(skewness_f64(&[]), None);
        assert_eq!(skewness_f64(&[1.0]), None);
        assert_eq!(skewness_f64(&[1.0, 2.0]), None);
    }

    #[test]
    fn kurtosis_normal_like() {
        // A uniform-ish distribution should have negative excess kurtosis (platykurtic)
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let kurt = kurtosis_f64(&vals).unwrap();
        // Uniform excess kurtosis is -1.2; discrete uniform of 10 is close
        assert!(
            kurt > -3.0 && kurt < 3.0,
            "Reasonable kurtosis expected, got {kurt}"
        );
    }

    #[test]
    fn kurtosis_too_few_returns_none() {
        assert_eq!(kurtosis_f64(&[]), None);
        assert_eq!(kurtosis_f64(&[1.0]), None);
        assert_eq!(kurtosis_f64(&[1.0, 2.0]), None);
        assert_eq!(kurtosis_f64(&[1.0, 2.0, 3.0]), None);
    }

    #[test]
    fn kurtosis_zero_variance_returns_none() {
        let vals = [5.0, 5.0, 5.0, 5.0, 5.0];
        assert_eq!(kurtosis_f64(&vals), None);
    }

    // -- Pearson correlation tests --

    #[test]
    fn pearson_perfect_positive() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 4.0, 6.0, 8.0, 10.0];
        let r = pearson_correlation(&x, &y).unwrap();
        assert!(
            (r - 1.0).abs() < 1e-10,
            "Expected r = 1.0, got {r}"
        );
    }

    #[test]
    fn pearson_perfect_negative() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [10.0, 8.0, 6.0, 4.0, 2.0];
        let r = pearson_correlation(&x, &y).unwrap();
        assert!(
            (r - (-1.0)).abs() < 1e-10,
            "Expected r = -1.0, got {r}"
        );
    }

    #[test]
    fn pearson_uncorrelated_returns_near_zero() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let y = [2.0, 5.0, 1.0, 4.0, 3.0];
        let r = pearson_correlation(&x, &y).unwrap();
        assert!(
            r.abs() < 0.5,
            "Expected near-zero correlation, got {r}"
        );
    }

    #[test]
    fn pearson_too_few_returns_none() {
        assert_eq!(pearson_correlation(&[], &[]), None);
        assert_eq!(pearson_correlation(&[1.0], &[2.0]), None);
    }

    #[test]
    fn pearson_zero_variance_returns_none() {
        let x = [5.0, 5.0, 5.0];
        let y = [1.0, 2.0, 3.0];
        assert_eq!(pearson_correlation(&x, &y), None);
    }

    // -- KS test tests --

    #[test]
    fn ks_identical_samples() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let result = ks_test_two_sample(&data, &data).unwrap();
        assert!(
            result.statistic < 1e-10,
            "Identical samples should have D near 0, got {}",
            result.statistic
        );
        assert!(
            result.p_value > 0.9,
            "Identical samples should have high p-value, got {}",
            result.p_value
        );
    }

    #[test]
    fn ks_completely_different() {
        let a = [0.0, 0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0, 1.0, 1.0];
        let result = ks_test_two_sample(&a, &b).unwrap();
        assert!(
            (result.statistic - 1.0).abs() < 1e-10,
            "Completely different samples should have D = 1.0, got {}",
            result.statistic
        );
    }

    #[test]
    fn ks_empty_returns_none() {
        assert!(ks_test_two_sample(&[], &[1.0, 2.0]).is_none());
        assert!(ks_test_two_sample(&[1.0, 2.0], &[]).is_none());
    }

    // -- Autocorrelation tests --

    #[test]
    fn acf_constant_series_returns_none() {
        let vals = [5.0, 5.0, 5.0, 5.0, 5.0];
        assert_eq!(autocorrelation_lag1(&vals), None);
    }

    #[test]
    fn acf_alternating_series_negative() {
        let vals = [1.0, -1.0, 1.0, -1.0, 1.0];
        let rho = autocorrelation_lag1(&vals).unwrap();
        assert!(
            rho < 0.0,
            "Alternating series should have negative autocorrelation, got {rho}"
        );
    }

    #[test]
    fn acf_trending_series_positive() {
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0];
        let rho = autocorrelation_lag1(&vals).unwrap();
        assert!(
            rho > 0.3,
            "Trending series should have positive rho > 0.3, got {rho}"
        );
    }

    #[test]
    fn acf_too_few_returns_none() {
        assert_eq!(autocorrelation_lag1(&[]), None);
        assert_eq!(autocorrelation_lag1(&[1.0]), None);
        assert_eq!(autocorrelation_lag1(&[1.0, 2.0]), None);
    }

    // -- Effective sample size tests --

    #[test]
    fn neff_with_positive_autocorrelation() {
        let n_eff = effective_sample_size(100, 0.5);
        // n_eff = 100 * 0.5 / 1.5 = 33.33 -> 33
        assert!(
            (n_eff as i64 - 33).abs() <= 1,
            "Expected ~33, got {n_eff}"
        );
    }

    #[test]
    fn neff_with_zero_autocorrelation() {
        let n_eff = effective_sample_size(100, 0.0);
        assert_eq!(n_eff, 100);
    }

    #[test]
    fn neff_with_negative_autocorrelation() {
        let n_eff = effective_sample_size(100, -0.3);
        assert_eq!(n_eff, 100, "Negative rho should return raw n");
    }

    // -- Corrected edge test tests --

    #[test]
    fn corrected_edge_test_autocorrelated_series() {
        // A positively autocorrelated series with positive mean
        let vals: Vec<f64> = (1..=20).map(|i| 0.5 + 0.01 * i as f64).collect();
        let result = compute_corrected_edge_test(&vals).unwrap();
        assert!(result.mean_edge > 0.0, "Mean should be positive");
        assert!(
            result.effective_n < result.raw_n,
            "n_eff ({}) should be less than raw_n ({}) for autocorrelated data",
            result.effective_n,
            result.raw_n
        );
        assert!(result.autocorrelation > 0.0, "Should detect positive autocorrelation");
    }

    #[test]
    fn corrected_edge_test_too_few_returns_none() {
        assert!(compute_corrected_edge_test(&[1.0, 2.0]).is_none());
    }
}
