//! Instrument name parsers for options venues.
//!
//! Supports:
//! - Deribit: "BTC-27JUN25-100000-C" (DDMMMYY format)
//! - Derive: "BTC-20260305-69500-C" (YYYYMMDD format)

use chrono::NaiveDate;

use super::types::{OptionType, ParsedInstrument};

/// Parse a Deribit instrument name into its components.
///
/// Deribit option instrument names follow the pattern:
/// `{ASSET}-{DDMMMYY}-{STRIKE}-{C|P}`
///
/// Examples:
/// - "BTC-27JUN25-100000-C" -> BTC, 2025-06-27, 100000.0, Call
/// - "ETH-28MAR25-4000-P"  -> ETH, 2025-03-28, 4000.0, Put
///
/// Returns `None` for non-option instruments (futures, perpetuals, etc.)
/// which have fewer than 4 dash-separated parts.
pub fn parse_deribit_instrument(name: &str) -> Option<ParsedInstrument> {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 4 {
        return None;
    }

    let asset = parts[0].to_string();
    let expiry_str = parts[1];
    let strike_str = parts[2];
    let type_str = parts[3];

    // Parse expiry: DDMMMYY format (e.g., "27JUN25")
    let expiry = parse_expiry(expiry_str)?;

    // Parse strike as f64
    let strike: f64 = strike_str.parse().ok()?;

    // Parse option type
    let option_type = match type_str {
        "C" => OptionType::Call,
        "P" => OptionType::Put,
        _ => return None,
    };

    Some(ParsedInstrument {
        asset,
        expiry,
        strike,
        option_type,
    })
}

/// Parse a Derive instrument name into its components.
///
/// Derive option instrument names follow the pattern:
/// `{ASSET}-{YYYYMMDD}-{STRIKE}-{C|P}`
///
/// Examples:
/// - "BTC-20260305-69500-C" -> BTC, 2026-03-05, 69500.0, Call
/// - "BTC-20260627-100000-P" -> BTC, 2026-06-27, 100000.0, Put
///
/// Returns `None` for non-option instruments or malformed names.
/// Naturally rejects Deribit's DDMMMYY format because "27JUN25" is not 8 digits.
pub fn parse_derive_instrument(name: &str) -> Option<ParsedInstrument> {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 4 {
        return None;
    }

    let asset = parts[0].to_string();
    let date_str = parts[1];
    let strike_str = parts[2];
    let type_str = parts[3];

    // Parse expiry: YYYYMMDD format (exactly 8 chars, all digits)
    if date_str.len() != 8 || !date_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let expiry = NaiveDate::parse_from_str(date_str, "%Y%m%d").ok()?;

    // Parse strike as f64
    let strike: f64 = strike_str.parse().ok()?;

    // Parse option type
    let option_type = match type_str {
        "C" => OptionType::Call,
        "P" => OptionType::Put,
        _ => return None,
    };

    Some(ParsedInstrument {
        asset,
        expiry,
        strike,
        option_type,
    })
}

/// Parse Deribit expiry string "DDMMMYY" into a NaiveDate.
///
/// Examples: "27JUN25" -> 2025-06-27, "28MAR25" -> 2025-03-28
fn parse_expiry(s: &str) -> Option<NaiveDate> {
    if s.len() < 5 {
        return None;
    }

    // Extract day (first 1-2 digits), month (3 chars), year (last 2 digits)
    // Find where the month letters start
    let day_end = s.find(|c: char| c.is_ascii_alphabetic())?;
    if day_end == 0 {
        return None;
    }

    let day: u32 = s[..day_end].parse().ok()?;

    let month_start = day_end;
    let month_end = month_start + 3;
    if month_end > s.len() {
        return None;
    }

    let month_str = &s[month_start..month_end];
    let month = match month_str {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        _ => return None,
    };

    let year_str = &s[month_end..];
    let year_2digit: i32 = year_str.parse().ok()?;
    let year = 2000 + year_2digit;

    NaiveDate::from_ymd_opt(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_call() {
        let inst = parse_deribit_instrument("BTC-27JUN25-100000-C").unwrap();
        assert_eq!(inst.asset, "BTC");
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2025, 6, 27).unwrap());
        assert!((inst.strike - 100000.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Call);
    }

    #[test]
    fn parse_valid_put() {
        let inst = parse_deribit_instrument("ETH-28MAR25-4000-P").unwrap();
        assert_eq!(inst.asset, "ETH");
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2025, 3, 28).unwrap());
        assert!((inst.strike - 4000.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Put);
    }

    #[test]
    fn parse_futures_returns_none() {
        // Futures have fewer parts: "BTC-27JUN25"
        assert!(parse_deribit_instrument("BTC-27JUN25").is_none());
    }

    #[test]
    fn parse_perpetual_returns_none() {
        assert!(parse_deribit_instrument("BTC-PERPETUAL").is_none());
    }

    #[test]
    fn parse_malformed_returns_none() {
        assert!(parse_deribit_instrument("").is_none());
        assert!(parse_deribit_instrument("not-an-instrument").is_none());
        assert!(parse_deribit_instrument("BTC-27JUN25-100000-X").is_none());
        assert!(parse_deribit_instrument("BTC-INVALID-100000-C").is_none());
    }

    #[test]
    fn parse_december_expiry() {
        let inst = parse_deribit_instrument("BTC-26DEC25-200000-C").unwrap();
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2025, 12, 26).unwrap());
        assert!((inst.strike - 200000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_single_digit_day() {
        let inst = parse_deribit_instrument("BTC-3JAN26-50000-P").unwrap();
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2026, 1, 3).unwrap());
        assert!((inst.strike - 50000.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Put);
    }

    // ---- Derive instrument parser tests ----

    #[test]
    fn derive_parse_valid_call() {
        let inst = parse_derive_instrument("BTC-20260305-69500-C").unwrap();
        assert_eq!(inst.asset, "BTC");
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2026, 3, 5).unwrap());
        assert!((inst.strike - 69500.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Call);
    }

    #[test]
    fn derive_parse_valid_put() {
        let inst = parse_derive_instrument("BTC-20260627-100000-P").unwrap();
        assert_eq!(inst.asset, "BTC");
        assert_eq!(inst.expiry, NaiveDate::from_ymd_opt(2026, 6, 27).unwrap());
        assert!((inst.strike - 100000.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Put);
    }

    #[test]
    fn derive_rejects_deribit_format() {
        // Deribit's DDMMMYY format should be rejected (not 8 digits)
        assert!(parse_derive_instrument("BTC-27JUN25-100000-C").is_none());
    }

    #[test]
    fn deribit_rejects_derive_format() {
        // Derive's YYYYMMDD format should be rejected by the Deribit parser
        assert!(parse_deribit_instrument("BTC-20260305-69500-P").is_none());
    }

    #[test]
    fn derive_parse_malformed_returns_none() {
        assert!(parse_derive_instrument("").is_none());
        assert!(parse_derive_instrument("BTC").is_none());
        assert!(parse_derive_instrument("BTC-20260305").is_none());
        assert!(parse_derive_instrument("BTC-20260305-69500").is_none());
        assert!(parse_derive_instrument("BTC-2026030-69500-C").is_none()); // 7 digits
        assert!(parse_derive_instrument("BTC-20260305-abc-C").is_none()); // invalid strike
        assert!(parse_derive_instrument("BTC-20260305-69500-X").is_none()); // invalid type
    }

    #[test]
    fn derive_parse_single_digit_strike() {
        let inst = parse_derive_instrument("BTC-20260305-500-P").unwrap();
        assert!((inst.strike - 500.0).abs() < f64::EPSILON);
        assert_eq!(inst.option_type, OptionType::Put);
    }

    #[test]
    fn derive_invalid_date_returns_none() {
        // Month 13 does not exist
        assert!(parse_derive_instrument("BTC-20261301-69500-C").is_none());
    }
}
