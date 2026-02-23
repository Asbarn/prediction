//! Dynamic threshold computation for spread signal detection.
//!
//! Implements the formula:
//! `max(static_floor, rolling_mean + k * rolling_stddev) + liquidity_penalty`
//!
//! Features cold start mode (elevated floor when insufficient samples),
//! configurable parameters, and full component logging for post-hoc analysis.

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

use crate::spread::config::ThresholdConfig;
use crate::spread::patterns::ThresholdComponents;
use crate::spread::rolling_stats::RollingStats;

/// Compute the dynamic threshold and return both the final value and
/// the breakdown of all components.
///
/// Logic:
/// 1. Static floor from config
/// 2. Dynamic component (mean + k * stddev, or cold-start elevated floor)
/// 3. Base = max(floor, dynamic)
/// 4. Liquidity penalty based on fill ratio
/// 5. Final = base + penalty
pub fn compute_threshold(
    stats: &RollingStats,
    config: &ThresholdConfig,
    buy_fill_ratio: Decimal,
    sell_fill_ratio: Decimal,
) -> (Decimal, ThresholdComponents) {
    let static_floor = config.static_floor;
    let is_cold_start = stats.count() < config.min_samples_for_dynamic();
    let mean = stats.mean();
    let stddev = stats.stddev();

    // Dynamic component
    let dynamic = if !is_cold_start {
        let mean_dec = Decimal::from_f64(mean).unwrap_or(Decimal::ZERO);
        let stddev_dec = Decimal::from_f64(stddev).unwrap_or(Decimal::ZERO);
        mean_dec + config.k * stddev_dec
    } else {
        // Cold start: elevated static floor
        static_floor * config.cold_start_multiplier
    };

    // Base = max(floor, dynamic)
    let base = static_floor.max(dynamic);

    // Liquidity penalty
    let avg_fill_ratio = (buy_fill_ratio + sell_fill_ratio) / Decimal::new(2, 0);
    let liquidity_penalty = if avg_fill_ratio >= Decimal::ONE {
        Decimal::ZERO
    } else {
        config.liquidity_penalty_scale * (Decimal::ONE - avg_fill_ratio)
    };

    // Final threshold
    let final_threshold = base + liquidity_penalty;

    let k_sigma_f64 = config
        .k
        .to_string()
        .parse::<f64>()
        .unwrap_or(2.0)
        * stddev;

    let components = ThresholdComponents {
        static_floor,
        rolling_mean: mean,
        rolling_stddev: stddev,
        k_sigma: k_sigma_f64,
        liquidity_penalty,
        final_threshold,
        is_cold_start,
    };

    (final_threshold, components)
}

/// Extension trait to access min_samples from ThresholdConfig.
///
/// Uses the rolling_min_samples from SpreadConfig, but since ThresholdConfig
/// doesn't directly hold it, we add a default method.
impl ThresholdConfig {
    /// Minimum samples before dynamic threshold activates.
    ///
    /// Returns a reasonable default of 30 if not configured.
    pub fn min_samples_for_dynamic(&self) -> usize {
        // This is stored at SpreadConfig level; ThresholdConfig provides
        // a default that matches SpreadConfig::default().rolling_min_samples.
        30
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn default_config() -> ThresholdConfig {
        ThresholdConfig::default()
    }

    fn make_warm_stats() -> RollingStats {
        let mut stats = RollingStats::new(3600);
        // Push 50 samples to exceed min_samples (30)
        for i in 0..50 {
            stats.push(0.02 + (i as f64) * 0.0001, (i as i64) * 1000);
        }
        stats
    }

    fn make_cold_stats() -> RollingStats {
        let mut stats = RollingStats::new(3600);
        // Push 5 samples -- below min_samples (30)
        for i in 0..5 {
            stats.push(0.02, (i as i64) * 1000);
        }
        stats
    }

    #[test]
    fn cold_start_uses_elevated_floor() {
        let config = default_config();
        let stats = make_cold_stats();

        let (threshold, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,
            Decimal::ONE,
        );

        assert!(components.is_cold_start);
        // Cold start: floor * multiplier = 0.01 * 2.0 = 0.02
        // No liquidity penalty (full fill)
        assert_eq!(threshold, dec("0.02"));
    }

    #[test]
    fn cold_start_zero_samples() {
        let config = default_config();
        let stats = RollingStats::new(3600);

        let (threshold, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,
            Decimal::ONE,
        );

        assert!(components.is_cold_start);
        assert_eq!(threshold, dec("0.02")); // 0.01 * 2.0
    }

    #[test]
    fn warm_state_uses_rolling_stats() {
        let config = default_config();
        let stats = make_warm_stats();

        let (threshold, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,
            Decimal::ONE,
        );

        assert!(!components.is_cold_start);
        // Dynamic = mean + k * stddev
        // If dynamic > static_floor: threshold = dynamic
        // static_floor = 0.01, mean ~ 0.0224, stddev ~ 0.0015
        // dynamic ~ 0.0224 + 2 * 0.0015 ~ 0.0254 > 0.01
        assert!(threshold > config.static_floor);
    }

    #[test]
    fn static_floor_wins_when_dynamic_is_lower() {
        // Set a very high static floor
        let config = ThresholdConfig {
            static_floor: dec("0.50"),
            k: dec("2"),
            liquidity_penalty_scale: dec("0.02"),
            cold_start_multiplier: dec("2"),
        };
        let stats = make_warm_stats();

        let (threshold, _) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,
            Decimal::ONE,
        );

        // mean + k*stddev ~ 0.025 < 0.50, so floor wins
        assert_eq!(threshold, dec("0.50"));
    }

    #[test]
    fn full_fill_no_liquidity_penalty() {
        let config = default_config();
        let stats = make_cold_stats();

        let (threshold_full, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,  // full buy fill
            Decimal::ONE,  // full sell fill
        );

        assert_eq!(components.liquidity_penalty, Decimal::ZERO);

        // Compare with partial fill
        let (threshold_partial, _) = compute_threshold(
            &stats,
            &config,
            dec("0.5"),
            dec("0.5"),
        );

        assert!(threshold_partial > threshold_full);
    }

    #[test]
    fn partial_fill_adds_liquidity_penalty() {
        let config = default_config();
        let stats = make_cold_stats();

        let (_, components) = compute_threshold(
            &stats,
            &config,
            dec("0.5"),  // 50% fill
            dec("0.5"),  // 50% fill
        );

        // avg_fill_ratio = 0.5, penalty = 0.02 * (1 - 0.5) = 0.01
        assert_eq!(components.liquidity_penalty, dec("0.01"));
    }

    #[test]
    fn empty_book_max_liquidity_penalty() {
        let config = default_config();
        let stats = make_cold_stats();

        let (_, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ZERO,  // no fill
            Decimal::ZERO,  // no fill
        );

        // avg_fill_ratio = 0, penalty = 0.02 * 1.0 = 0.02
        assert_eq!(components.liquidity_penalty, dec("0.02"));
    }

    #[test]
    fn components_capture_all_fields() {
        let config = default_config();
        let stats = make_warm_stats();

        let (_, components) = compute_threshold(
            &stats,
            &config,
            dec("0.8"),
            dec("0.9"),
        );

        // All fields should be populated
        assert_eq!(components.static_floor, dec("0.01"));
        assert!(!components.is_cold_start);
        assert!(components.rolling_mean > 0.0);
        assert!(components.rolling_stddev >= 0.0);
        assert!(components.k_sigma >= 0.0);
        assert!(components.final_threshold > Decimal::ZERO);
    }

    #[test]
    fn asymmetric_fill_ratios() {
        let config = default_config();
        let stats = make_cold_stats();

        let (_, components) = compute_threshold(
            &stats,
            &config,
            Decimal::ONE,   // buy fully filled
            Decimal::ZERO,  // sell empty book
        );

        // avg_fill_ratio = (1.0 + 0.0) / 2 = 0.5
        // penalty = 0.02 * 0.5 = 0.01
        assert_eq!(components.liquidity_penalty, dec("0.01"));
    }
}
