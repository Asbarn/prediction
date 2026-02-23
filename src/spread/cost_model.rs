use std::str::FromStr;

use rust_decimal::Decimal;

use super::config::{CarryConfig, KalshiFeeConfig, PolymarketFeeConfig};

/// Compute Polymarket dynamic fee for a trade.
///
/// Formula: `shares * fee_rate * (price * (1 - price))^exponent`
///
/// If `flat_rate_override` is set, uses `shares * flat_rate` instead.
///
/// Supports exponent=1 (sports markets, max ~0.44% at p=0.50) and
/// exponent=2 (crypto markets, max ~1.56% at p=0.50).
///
/// Returns the absolute fee amount (always non-negative).
pub fn polymarket_fee(shares: Decimal, price: Decimal, config: &PolymarketFeeConfig) -> Decimal {
    // Flat rate override path
    if let Some(flat_rate) = config.flat_rate_override {
        return shares * flat_rate;
    }

    // Dynamic formula: shares * fee_rate * (p * (1-p))^exponent
    let base = price * (Decimal::ONE - price);
    let scaled = match config.exponent {
        1 => base,
        2 => base * base,
        n => {
            // General case (unlikely but safe)
            let mut result = Decimal::ONE;
            for _ in 0..n {
                result *= base;
            }
            result
        }
    };

    shares * config.fee_rate * scaled
}

/// Compute Kalshi taker fee for a trade.
///
/// Formula: `coefficient * contracts * P * (1 - P)`
///
/// If `use_ceiling` is true, the per-contract fee is ceiling-rounded before
/// multiplying by the number of contracts (Kalshi rounds per contract).
///
/// Returns the absolute fee amount (always non-negative).
pub fn kalshi_taker_fee(
    contracts: Decimal,
    price_probability: Decimal,
    config: &KalshiFeeConfig,
) -> Decimal {
    let per_contract_raw =
        config.taker_coefficient * price_probability * (Decimal::ONE - price_probability);

    if config.use_ceiling {
        // Kalshi rounds up per contract -- ceil to 2 decimal places (cents)
        let per_contract_ceil = per_contract_raw.ceil();
        per_contract_ceil * contracts
    } else {
        per_contract_raw * contracts
    }
}

/// Compute carry cost for holding a position.
///
/// Formula: `notional * annualized_rate * reference_holding_days / 365`
///
/// Models the opportunity cost of capital locked in the position.
/// Returns the absolute carry cost.
pub fn carry_cost(notional: Decimal, config: &CarryConfig) -> Decimal {
    let days = Decimal::from(config.reference_holding_days);
    let year = Decimal::new(365, 0);

    notional * config.annualized_rate * days / year
}

/// Compute total one-way cost for a leg of a spread trade.
///
/// Sum of venue fee + carry cost. Each leg has its own fee model.
pub fn total_cost(fee: Decimal, carry: Decimal) -> Decimal {
    fee + carry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spread::config::{CarryConfig, KalshiFeeConfig, PolymarketFeeConfig};

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    // ---- Polymarket fee tests ----

    #[test]
    fn polymarket_fee_exponent_2_at_p50() {
        // At p=0.50: base = 0.50 * 0.50 = 0.25, base^2 = 0.0625
        // fee = 100 * 0.25 * 0.0625 = 1.5625 (1.5625% of notional)
        let config = PolymarketFeeConfig {
            fee_rate: dec("0.25"),
            exponent: 2,
            flat_rate_override: None,
        };
        let fee = polymarket_fee(dec("100"), dec("0.50"), &config);
        assert_eq!(fee, dec("1.5625"));
    }

    #[test]
    fn polymarket_fee_exponent_1_at_p50() {
        // At p=0.50: base = 0.25
        // fee = 100 * 0.25 * 0.25 = 6.25
        // But with sports fee_rate of 0.0175: 100 * 0.0175 * 0.25 = 0.4375
        let config = PolymarketFeeConfig {
            fee_rate: dec("0.0175"),
            exponent: 1,
            flat_rate_override: None,
        };
        let fee = polymarket_fee(dec("100"), dec("0.50"), &config);
        assert_eq!(fee, dec("0.4375"));
    }

    #[test]
    fn polymarket_fee_at_p0_is_zero() {
        let config = PolymarketFeeConfig::default();
        let fee = polymarket_fee(dec("100"), Decimal::ZERO, &config);
        assert_eq!(fee, Decimal::ZERO);
    }

    #[test]
    fn polymarket_fee_at_p1_is_zero() {
        let config = PolymarketFeeConfig::default();
        let fee = polymarket_fee(dec("100"), Decimal::ONE, &config);
        assert_eq!(fee, Decimal::ZERO);
    }

    #[test]
    fn polymarket_fee_flat_rate_override() {
        let config = PolymarketFeeConfig {
            fee_rate: dec("0.25"),
            exponent: 2,
            flat_rate_override: Some(dec("0.01")),
        };
        let fee = polymarket_fee(dec("100"), dec("0.50"), &config);
        // flat rate: 100 * 0.01 = 1.00
        assert_eq!(fee, dec("1.00"));
    }

    #[test]
    fn polymarket_fee_asymmetric_price() {
        // p=0.80: base = 0.80 * 0.20 = 0.16, base^2 = 0.0256
        // fee = 100 * 0.25 * 0.0256 = 0.64
        let config = PolymarketFeeConfig::default();
        let fee = polymarket_fee(dec("100"), dec("0.80"), &config);
        assert_eq!(fee, dec("0.64"));
    }

    // ---- Kalshi fee tests ----

    #[test]
    fn kalshi_fee_at_p50_with_ceiling() {
        // Per contract: 0.07 * 0.50 * 0.50 = 0.0175
        // ceil(0.0175) = 1 (ceiling rounds 0.0175 to 1 -- integer cents)
        // For 10 contracts: 10 * 1 = 10
        //
        // Note: Decimal::ceil() rounds to the nearest integer ceiling.
        // 0.0175.ceil() = 1
        let config = KalshiFeeConfig {
            taker_coefficient: dec("0.07"),
            use_ceiling: true,
        };
        let fee = kalshi_taker_fee(dec("10"), dec("0.50"), &config);
        // Per contract raw = 0.0175, ceil(0.0175) = 1, * 10 = 10
        assert_eq!(fee, dec("10"));
    }

    #[test]
    fn kalshi_fee_at_p50_without_ceiling() {
        // Without ceiling: 10 * 0.07 * 0.50 * 0.50 = 10 * 0.0175 = 0.175
        let config = KalshiFeeConfig {
            taker_coefficient: dec("0.07"),
            use_ceiling: false,
        };
        let fee = kalshi_taker_fee(dec("10"), dec("0.50"), &config);
        assert_eq!(fee, dec("0.175"));
    }

    #[test]
    fn kalshi_fee_single_contract_at_p50() {
        // 1 * 0.07 * 0.50 * 0.50 = 0.0175
        let config = KalshiFeeConfig {
            taker_coefficient: dec("0.07"),
            use_ceiling: false,
        };
        let fee = kalshi_taker_fee(dec("1"), dec("0.50"), &config);
        assert_eq!(fee, dec("0.0175"));
    }

    #[test]
    fn kalshi_fee_at_p0_is_zero() {
        let config = KalshiFeeConfig::default();
        let fee = kalshi_taker_fee(dec("10"), Decimal::ZERO, &config);
        assert_eq!(fee, Decimal::ZERO);
    }

    #[test]
    fn kalshi_fee_at_p1_is_zero() {
        let config = KalshiFeeConfig::default();
        let fee = kalshi_taker_fee(dec("10"), Decimal::ONE, &config);
        assert_eq!(fee, Decimal::ZERO);
    }

    // ---- Carry cost tests ----

    #[test]
    fn carry_cost_30_days() {
        // 500 * 0.05 * 30/365 = 500 * 0.05 * 0.082191... = 2.05479...
        let config = CarryConfig {
            annualized_rate: dec("0.05"),
            reference_holding_days: 30,
        };
        let cost = carry_cost(dec("500"), &config);
        // 500 * 0.05 * 30 / 365 = 750/365 = 2.054794520547945205...
        // Decimal arithmetic: 500 * 0.05 = 25, * 30 = 750, / 365
        let expected = dec("750") / dec("365");
        assert_eq!(cost, expected);
    }

    #[test]
    fn carry_cost_zero_notional() {
        let config = CarryConfig::default();
        let cost = carry_cost(Decimal::ZERO, &config);
        assert_eq!(cost, Decimal::ZERO);
    }

    #[test]
    fn carry_cost_zero_rate() {
        let config = CarryConfig {
            annualized_rate: Decimal::ZERO,
            reference_holding_days: 30,
        };
        let cost = carry_cost(dec("500"), &config);
        assert_eq!(cost, Decimal::ZERO);
    }

    // ---- Total cost tests ----

    #[test]
    fn total_cost_sums_fee_and_carry() {
        let fee = dec("1.50");
        let carry = dec("2.00");
        assert_eq!(total_cost(fee, carry), dec("3.50"));
    }
}
