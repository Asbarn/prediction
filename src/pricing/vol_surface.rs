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
}
