use std::collections::VecDeque;

/// Windowed rolling statistics using sum-based computation.
///
/// Maintains a time-based sliding window of (value, timestamp) pairs.
/// Old samples are evicted when they fall outside the window duration.
/// Supports mean, sample standard deviation, and percentile queries.
pub struct RollingStats {
    /// Buffered (value, timestamp_ms) pairs in insertion order.
    window: VecDeque<(f64, i64)>,
    /// Window duration in milliseconds.
    window_duration_ms: i64,
    /// Running sum of values in the window.
    sum: f64,
    /// Running sum of squared values in the window.
    sum_sq: f64,
}

impl RollingStats {
    /// Create a new RollingStats with the given window duration in seconds.
    pub fn new(window_duration_secs: u64) -> Self {
        Self {
            window: VecDeque::new(),
            window_duration_ms: (window_duration_secs as i64) * 1000,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    /// Push a new value with its timestamp. Evicts expired entries first.
    pub fn push(&mut self, value: f64, timestamp_ms: i64) {
        // Evict expired entries
        let cutoff = timestamp_ms - self.window_duration_ms;
        while let Some(&(old_val, old_ts)) = self.window.front() {
            if old_ts < cutoff {
                self.sum -= old_val;
                self.sum_sq -= old_val * old_val;
                self.window.pop_front();
            } else {
                break;
            }
        }

        self.window.push_back((value, timestamp_ms));
        self.sum += value;
        self.sum_sq += value * value;
    }

    /// Number of samples currently in the window.
    pub fn count(&self) -> usize {
        self.window.len()
    }

    /// Arithmetic mean of values in the window. Returns 0.0 if empty.
    pub fn mean(&self) -> f64 {
        let n = self.window.len() as f64;
        if n == 0.0 {
            return 0.0;
        }
        self.sum / n
    }

    /// Sample standard deviation. Returns 0.0 if fewer than 2 samples.
    pub fn stddev(&self) -> f64 {
        let n = self.window.len() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let variance = (self.sum_sq - self.sum * self.sum / n) / (n - 1.0);
        variance.max(0.0).sqrt()
    }

    /// Compute percentile (0-100) using linear interpolation.
    /// Returns 0.0 if empty. For p=50 this is the median.
    pub fn percentile(&self, p: f64) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }

        let mut values: Vec<f64> = self.window.iter().map(|(v, _)| *v).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = values.len();
        if n == 1 {
            return values[0];
        }

        // Linear interpolation
        let rank = (p / 100.0) * (n - 1) as f64;
        let lower = rank.floor() as usize;
        let upper = rank.ceil() as usize;
        let frac = rank - lower as f64;

        if lower == upper || upper >= n {
            values[lower.min(n - 1)]
        } else {
            values[lower] * (1.0 - frac) + values[upper] * frac
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stats_return_zero() {
        let stats = RollingStats::new(3600);
        assert_eq!(stats.count(), 0);
        assert_eq!(stats.mean(), 0.0);
        assert_eq!(stats.stddev(), 0.0);
        assert_eq!(stats.percentile(50.0), 0.0);
    }

    #[test]
    fn single_value_has_correct_mean_and_zero_stddev() {
        let mut stats = RollingStats::new(3600);
        stats.push(42.0, 1000);
        assert_eq!(stats.count(), 1);
        assert_eq!(stats.mean(), 42.0);
        assert_eq!(stats.stddev(), 0.0); // n < 2
        assert_eq!(stats.percentile(50.0), 42.0);
    }

    #[test]
    fn known_sequence_statistics() {
        let mut stats = RollingStats::new(3600);
        for (i, v) in [1.0, 2.0, 3.0, 4.0, 5.0].iter().enumerate() {
            stats.push(*v, (i as i64 + 1) * 1000);
        }
        assert_eq!(stats.count(), 5);
        assert!((stats.mean() - 3.0).abs() < 1e-10);
        // Sample stddev of [1,2,3,4,5] = sqrt(2.5) ~= 1.5811
        assert!((stats.stddev() - (2.5_f64).sqrt()).abs() < 1e-10);
        // Median (50th percentile) of [1,2,3,4,5] = 3.0
        assert!((stats.percentile(50.0) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn window_eviction_works() {
        let mut stats = RollingStats::new(10); // 10-second window

        // Push values at time 0s, 5s, 10s
        stats.push(10.0, 0);
        stats.push(20.0, 5_000);
        stats.push(30.0, 10_000);
        assert_eq!(stats.count(), 3);

        // Push at 15s -- evicts the value at 0s (cutoff = 15000 - 10000 = 5000)
        stats.push(40.0, 15_000);
        assert_eq!(stats.count(), 3); // [20@5000, 30@10000, 40@15000]
        assert!((stats.mean() - 30.0).abs() < 1e-10);
    }

    #[test]
    fn large_window_holds_all_values() {
        let mut stats = RollingStats::new(86400); // 24-hour window
        for i in 0..100 {
            stats.push(i as f64, i * 1000);
        }
        assert_eq!(stats.count(), 100);
    }

    #[test]
    fn percentile_boundary_values() {
        let mut stats = RollingStats::new(3600);
        for (i, v) in [10.0, 20.0, 30.0, 40.0, 50.0].iter().enumerate() {
            stats.push(*v, (i as i64 + 1) * 1000);
        }
        // 0th percentile = min
        assert!((stats.percentile(0.0) - 10.0).abs() < 1e-10);
        // 100th percentile = max
        assert!((stats.percentile(100.0) - 50.0).abs() < 1e-10);
        // 25th percentile of [10,20,30,40,50]: rank = 0.25*4 = 1.0 -> values[1] = 20
        assert!((stats.percentile(25.0) - 20.0).abs() < 1e-10);
    }
}
