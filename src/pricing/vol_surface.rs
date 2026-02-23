//! Per-expiry implied volatility surface construction and interpolation.
//!
//! Constructs a `VolSmile` from raw (strike, IV) observations with quality
//! filtering, linear interpolation between observed points, and flat
//! extrapolation beyond boundary strikes. Quality tiers (Good/Minimum/
//! Degraded/Empty) reflect surface reliability for downstream confidence.

use chrono::NaiveDate;
use serde::Serialize;

use super::config::VolSurfaceConfig;

// ---------------------------------------------------------------------------
// SmilePoint
// ---------------------------------------------------------------------------

/// A single observed point on the vol smile.
#[derive(Debug, Clone, Serialize)]
pub struct SmilePoint {
    /// Strike price (USD).
    pub strike: f64,
    /// Mid implied volatility (annualized).
    pub iv: f64,
    /// Bid-side implied volatility.
    pub bid_iv: f64,
    /// Ask-side implied volatility.
    pub ask_iv: f64,
    /// IV bid-ask spread (ask_iv - bid_iv).
    pub iv_spread: f64,
}

// ---------------------------------------------------------------------------
// SmileQuality
// ---------------------------------------------------------------------------

/// Quality tier reflecting vol smile reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SmileQuality {
    /// 5+ usable strikes -- reliable interpolation.
    Good,
    /// 3-4 usable strikes -- minimum for interpolation.
    Minimum,
    /// Fewer than min_usable_strikes -- falls back to flat ATM vol.
    Degraded,
    /// 0 usable strikes -- no data.
    Empty,
}

// ---------------------------------------------------------------------------
// VolSmile
// ---------------------------------------------------------------------------

/// Per-expiry implied volatility smile with quality filtering.
///
/// Points are always sorted by strike ascending. Quality filtering excludes
/// strikes with excessive IV bid-ask spread or non-positive IV.
#[derive(Debug, Clone, Serialize)]
pub struct VolSmile {
    /// Expiry date for this smile.
    pub expiry: NaiveDate,
    /// Usable smile points, sorted by strike ascending.
    pub points: Vec<SmilePoint>,
    /// Excluded strikes with reasons (strike, reason).
    pub excluded: Vec<(f64, String)>,
    /// Quality tier based on remaining point count.
    pub quality: SmileQuality,
    /// ATM implied volatility (nearest to forward price).
    pub atm_iv: Option<f64>,
}

impl VolSmile {
    /// Construct a vol smile from raw observations with quality filtering.
    ///
    /// Filters out points with excessive IV spread or non-positive IV,
    /// sorts remaining by strike, determines quality tier, and identifies
    /// ATM IV as the point nearest to the forward price.
    pub fn new(
        expiry: NaiveDate,
        raw_points: Vec<SmilePoint>,
        config: &VolSurfaceConfig,
        forward_price: f64,
    ) -> Self {
        let mut points = Vec::with_capacity(raw_points.len());
        let mut excluded = Vec::new();

        for p in raw_points {
            // Filter: non-positive IV
            if p.iv <= 0.0 {
                excluded.push((p.strike, "non-positive IV".to_string()));
                continue;
            }

            // Filter: excessive IV bid-ask spread
            if p.iv_spread > config.max_iv_spread_filter {
                excluded.push((
                    p.strike,
                    format!(
                        "iv_spread={:.2} exceeds max {:.2}",
                        p.iv_spread, config.max_iv_spread_filter
                    ),
                ));
                continue;
            }

            points.push(p);
        }

        // Sort by strike ascending
        points.sort_by(|a, b| a.strike.partial_cmp(&b.strike).unwrap_or(std::cmp::Ordering::Equal));

        // Find ATM IV: point with strike closest to forward price
        let atm_iv = if points.is_empty() {
            None
        } else {
            let mut closest = &points[0];
            let mut min_dist = (closest.strike - forward_price).abs();
            for p in &points[1..] {
                let dist = (p.strike - forward_price).abs();
                if dist < min_dist {
                    min_dist = dist;
                    closest = p;
                }
            }
            Some(closest.iv)
        };

        // Determine quality tier
        let count = points.len();
        let quality = if count == 0 {
            SmileQuality::Empty
        } else if count < config.min_usable_strikes {
            SmileQuality::Degraded
        } else if count >= config.good_strike_count {
            SmileQuality::Good
        } else {
            SmileQuality::Minimum
        };

        Self {
            expiry,
            points,
            excluded,
            quality,
            atm_iv,
        }
    }

    /// Number of usable points in the smile.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// True if no usable points remain.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    // -----------------------------------------------------------------------
    // Interpolation
    // -----------------------------------------------------------------------

    /// Interpolate implied volatility at an arbitrary strike.
    ///
    /// - **Empty** quality: returns `None`.
    /// - **Degraded** quality with ATM IV: returns flat ATM vol for any strike.
    /// - **Single point**: returns that point's IV for any strike.
    /// - **Below minimum strike**: flat extrapolation (first point's IV).
    /// - **Above maximum strike**: flat extrapolation (last point's IV).
    /// - **Between two points**: linear interpolation.
    pub fn interpolate(&self, strike: f64) -> Option<f64> {
        if self.quality == SmileQuality::Empty {
            return None;
        }

        // Degraded quality: flat ATM vol fallback
        if self.quality == SmileQuality::Degraded {
            if let Some(atm) = self.atm_iv {
                return Some(atm);
            }
            // Degraded with no ATM IV shouldn't happen (we set ATM if any
            // points exist), but handle gracefully.
            return self.points.first().map(|p| p.iv);
        }

        // Single point: return its IV for any strike
        if self.points.len() == 1 {
            return Some(self.points[0].iv);
        }

        let first = &self.points[0];
        let last = &self.points[self.points.len() - 1];

        // Flat extrapolation below minimum strike
        if strike <= first.strike {
            return Some(first.iv);
        }

        // Flat extrapolation above maximum strike
        if strike >= last.strike {
            return Some(last.iv);
        }

        // Binary search for the two surrounding points
        // partition_point returns the first index where strike <= points[i].strike
        let idx = self
            .points
            .partition_point(|p| p.strike < strike);

        // idx should be in [1, len-1] at this point since we handled boundary cases
        let upper = &self.points[idx];
        let lower = &self.points[idx - 1];

        // Exact match on an observed strike
        if (upper.strike - strike).abs() < f64::EPSILON {
            return Some(upper.iv);
        }
        if (lower.strike - strike).abs() < f64::EPSILON {
            return Some(lower.iv);
        }

        // Linear interpolation
        let t = (strike - lower.strike) / (upper.strike - lower.strike);
        let iv = lower.iv + (upper.iv - lower.iv) * t;
        Some(iv)
    }

    // -----------------------------------------------------------------------
    // Bracket finding
    // -----------------------------------------------------------------------

    /// Find the nearest observed strikes bracketing the target strike.
    ///
    /// Returns `(k_lower, k_upper)` where `k_lower < target < k_upper`.
    /// If the target is exactly on an observed strike, uses the adjacent
    /// strikes on both sides.
    ///
    /// Returns `None` if the target is below all or above all observed
    /// strikes (cannot bracket), or if fewer than 2 points exist.
    pub fn nearest_bracket(&self, target_strike: f64) -> Option<(f64, f64)> {
        if self.points.len() < 2 {
            return None;
        }

        let first = self.points[0].strike;
        let last = self.points[self.points.len() - 1].strike;

        // Cannot bracket if target is outside observed range
        if target_strike <= first || target_strike >= last {
            return None;
        }

        let idx = self.points.partition_point(|p| p.strike < target_strike);

        // If target lands exactly on an observed strike, use adjacent strikes
        if idx < self.points.len() && (self.points[idx].strike - target_strike).abs() < f64::EPSILON
        {
            // Need both sides to exist
            if idx == 0 || idx >= self.points.len() - 1 {
                return None;
            }
            return Some((self.points[idx - 1].strike, self.points[idx + 1].strike));
        }

        // idx is the first point >= target; idx-1 is the last point < target
        if idx == 0 || idx >= self.points.len() {
            return None;
        }

        Some((self.points[idx - 1].strike, self.points[idx].strike))
    }

    // -----------------------------------------------------------------------
    // Skew
    // -----------------------------------------------------------------------

    /// Compute skew at a given strike: `strike_iv - atm_iv`.
    ///
    /// Returns `None` if ATM IV is unavailable or interpolation fails.
    pub fn skew_at(&self, strike: f64) -> Option<f64> {
        let atm = self.atm_iv?;
        let strike_iv = self.interpolate(strike)?;
        Some(strike_iv - atm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> VolSurfaceConfig {
        VolSurfaceConfig {
            min_usable_strikes: 3,
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        }
    }

    fn make_point(strike: f64, iv: f64, spread: f64) -> SmilePoint {
        SmilePoint {
            strike,
            iv,
            bid_iv: iv - spread / 2.0,
            ask_iv: iv + spread / 2.0,
            iv_spread: spread,
        }
    }

    /// Test a: 5 good points -> SmileQuality::Good, all points retained.
    #[test]
    fn construction_good_quality() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Good);
        assert_eq!(smile.points.len(), 5);
        assert!(smile.excluded.is_empty());
        // ATM IV should be from strike nearest to forward (100000)
        assert!((smile.atm_iv.unwrap() - 0.50).abs() < f64::EPSILON);
    }

    /// Test b: 2 wide-spread points excluded -> remaining count determines quality.
    #[test]
    fn construction_excludes_wide_spread() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.80),  // excluded: spread > 0.50
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.70), // excluded: spread > 0.50
            make_point(110000.0, 0.58, 0.06),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        assert_eq!(smile.points.len(), 3);
        assert_eq!(smile.excluded.len(), 2);
        assert_eq!(smile.quality, SmileQuality::Minimum);

        // Check exclusion reasons
        assert!(smile.excluded[0].1.contains("iv_spread="));
        assert!(smile.excluded[0].1.contains("exceeds max"));
    }

    /// Test c: < min_usable_strikes -> SmileQuality::Degraded.
    #[test]
    fn construction_degraded_quality() {
        let config = default_config();
        let points = vec![
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Degraded);
        assert_eq!(smile.points.len(), 2);
        assert!(smile.atm_iv.is_some());
    }

    /// Test d: Points are sorted by strike regardless of input order.
    #[test]
    fn construction_sorted_by_strike() {
        let config = default_config();
        let points = vec![
            make_point(110000.0, 0.58, 0.06),
            make_point(90000.0, 0.60, 0.05),
            make_point(105000.0, 0.52, 0.04),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        let strikes: Vec<f64> = smile.points.iter().map(|p| p.strike).collect();
        assert_eq!(strikes, vec![90000.0, 95000.0, 100000.0, 105000.0, 110000.0]);
    }

    /// Test e: Empty input -> SmileQuality::Empty with no ATM IV.
    #[test]
    fn construction_empty() {
        let config = default_config();
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, Vec::new(), &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Empty);
        assert!(smile.atm_iv.is_none());
        assert!(smile.is_empty());
    }

    /// Test f: Non-positive IV excluded with reason.
    #[test]
    fn construction_excludes_non_positive_iv() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.0, 0.04),     // excluded: iv = 0
            make_point(100000.0, -0.10, 0.03),   // excluded: negative iv
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        assert_eq!(smile.points.len(), 3);
        assert_eq!(smile.excluded.len(), 2);
        assert!(smile.excluded.iter().all(|(_, reason)| reason == "non-positive IV"));
        assert_eq!(smile.quality, SmileQuality::Minimum);
    }

    // =======================================================================
    // Interpolation tests
    // =======================================================================

    fn make_good_smile() -> VolSmile {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        VolSmile::new(expiry, points, &config, 100000.0)
    }

    /// Test g: Interpolation at exact observed strike returns that IV.
    #[test]
    fn interpolate_exact_strike() {
        let smile = make_good_smile();
        let iv = smile.interpolate(100000.0).unwrap();
        assert!(
            (iv - 0.50).abs() < 1e-10,
            "expected 0.50 at exact strike, got {iv}"
        );

        let iv_low = smile.interpolate(90000.0).unwrap();
        assert!(
            (iv_low - 0.60).abs() < 1e-10,
            "expected 0.60 at lowest strike, got {iv_low}"
        );
    }

    /// Test h: Interpolation between two strikes returns linear blend.
    #[test]
    fn interpolate_between_strikes() {
        let smile = make_good_smile();
        // Between 90000 (0.60) and 95000 (0.55) at midpoint 92500
        let iv = smile.interpolate(92500.0).unwrap();
        let expected = 0.60 + (0.55 - 0.60) * (92500.0 - 90000.0) / (95000.0 - 90000.0);
        assert!(
            (iv - expected).abs() < 1e-10,
            "expected {expected}, got {iv}"
        );
        // Verify it's strictly between the two surrounding IVs
        assert!(iv > 0.55 && iv < 0.60, "IV should be between 0.55 and 0.60, got {iv}");
    }

    /// Test i: Extrapolation below minimum strike returns first IV (flat).
    #[test]
    fn extrapolate_below() {
        let smile = make_good_smile();
        let iv = smile.interpolate(80000.0).unwrap();
        assert!(
            (iv - 0.60).abs() < 1e-10,
            "expected flat extrapolation = 0.60, got {iv}"
        );
    }

    /// Test j: Extrapolation above maximum strike returns last IV (flat).
    #[test]
    fn extrapolate_above() {
        let smile = make_good_smile();
        let iv = smile.interpolate(120000.0).unwrap();
        assert!(
            (iv - 0.58).abs() < 1e-10,
            "expected flat extrapolation = 0.58, got {iv}"
        );
    }

    /// Test k: nearest_bracket returns correct surrounding strikes.
    #[test]
    fn nearest_bracket_between() {
        let smile = make_good_smile();
        let (lower, upper) = smile.nearest_bracket(97000.0).unwrap();
        assert!(
            (lower - 95000.0).abs() < f64::EPSILON,
            "expected lower=95000, got {lower}"
        );
        assert!(
            (upper - 100000.0).abs() < f64::EPSILON,
            "expected upper=100000, got {upper}"
        );
    }

    /// Test l: nearest_bracket returns None for out-of-range strikes.
    #[test]
    fn nearest_bracket_out_of_range() {
        let smile = make_good_smile();
        assert!(smile.nearest_bracket(80000.0).is_none(), "below all strikes");
        assert!(smile.nearest_bracket(120000.0).is_none(), "above all strikes");
        // Exactly on boundary
        assert!(smile.nearest_bracket(90000.0).is_none(), "on first strike");
        assert!(smile.nearest_bracket(110000.0).is_none(), "on last strike");
    }

    /// Test m: nearest_bracket on exact observed strike uses adjacent strikes.
    #[test]
    fn nearest_bracket_exact_strike() {
        let smile = make_good_smile();
        // Target = 100000 (exact observed strike), expect (95000, 105000)
        let (lower, upper) = smile.nearest_bracket(100000.0).unwrap();
        assert!(
            (lower - 95000.0).abs() < f64::EPSILON,
            "expected lower=95000, got {lower}"
        );
        assert!(
            (upper - 105000.0).abs() < f64::EPSILON,
            "expected upper=105000, got {upper}"
        );
    }

    /// Test n: skew_at returns correct skew relative to ATM.
    #[test]
    fn skew_at_various_strikes() {
        let smile = make_good_smile();
        // ATM IV = 0.50 (at strike 100000)
        // At ATM: skew should be 0
        let skew_atm = smile.skew_at(100000.0).unwrap();
        assert!(
            skew_atm.abs() < 1e-10,
            "ATM skew should be ~0, got {skew_atm}"
        );

        // At 90000: IV = 0.60, skew = 0.60 - 0.50 = 0.10
        let skew_low = smile.skew_at(90000.0).unwrap();
        assert!(
            (skew_low - 0.10).abs() < 1e-10,
            "expected skew=0.10, got {skew_low}"
        );

        // At 110000: IV = 0.58, skew = 0.58 - 0.50 = 0.08
        let skew_high = smile.skew_at(110000.0).unwrap();
        assert!(
            (skew_high - 0.08).abs() < 1e-10,
            "expected skew=0.08, got {skew_high}"
        );
    }

    /// Test o: Degraded quality returns flat ATM vol for any strike.
    #[test]
    fn degraded_returns_flat_atm() {
        let config = default_config();
        let points = vec![
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
        ];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Degraded);

        // Should return ATM IV (0.50) for any strike
        let iv_low = smile.interpolate(80000.0).unwrap();
        let iv_mid = smile.interpolate(100000.0).unwrap();
        let iv_high = smile.interpolate(120000.0).unwrap();
        assert!((iv_low - 0.50).abs() < 1e-10, "degraded should return ATM IV, got {iv_low}");
        assert!((iv_mid - 0.50).abs() < 1e-10, "degraded should return ATM IV, got {iv_mid}");
        assert!((iv_high - 0.50).abs() < 1e-10, "degraded should return ATM IV, got {iv_high}");
    }

    /// Test p: Empty quality returns None for interpolation.
    #[test]
    fn empty_returns_none() {
        let config = default_config();
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, Vec::new(), &config, 100000.0);

        assert!(smile.interpolate(100000.0).is_none());
        assert!(smile.nearest_bracket(100000.0).is_none());
        assert!(smile.skew_at(100000.0).is_none());
    }

    /// Test q: Single point returns its IV for any strike.
    #[test]
    fn single_point_returns_its_iv() {
        let config = VolSurfaceConfig {
            min_usable_strikes: 1, // set to 1 so single point is not Degraded
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        };
        let points = vec![make_point(100000.0, 0.50, 0.03)];
        let expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let smile = VolSmile::new(expiry, points, &config, 100000.0);

        let iv = smile.interpolate(80000.0).unwrap();
        assert!((iv - 0.50).abs() < 1e-10);
        let iv = smile.interpolate(120000.0).unwrap();
        assert!((iv - 0.50).abs() < 1e-10);
    }

    /// Test r: Interpolation monotonicity check -- interpolated value is strictly
    /// between the two surrounding IVs (for cases where IVs differ).
    #[test]
    fn interpolation_monotonicity() {
        let smile = make_good_smile();
        // Check many points between each pair of observed strikes
        let strikes = &smile.points;
        for w in strikes.windows(2) {
            let (k_lo, iv_lo) = (w[0].strike, w[0].iv);
            let (k_hi, iv_hi) = (w[1].strike, w[1].iv);
            let lo_iv = iv_lo.min(iv_hi);
            let hi_iv = iv_lo.max(iv_hi);

            // Test 10 interior points
            for i in 1..10 {
                let frac = i as f64 / 10.0;
                let k = k_lo + (k_hi - k_lo) * frac;
                let iv = smile.interpolate(k).unwrap();
                assert!(
                    iv >= lo_iv - 1e-10 && iv <= hi_iv + 1e-10,
                    "interpolated IV {iv} outside [{lo_iv}, {hi_iv}] at strike {k}"
                );
            }
        }
    }
}
