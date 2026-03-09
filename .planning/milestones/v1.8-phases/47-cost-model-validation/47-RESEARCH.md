# Phase 47: Cost Model Validation - Research

**Researched:** 2026-03-09
**Domain:** Exchange fee validation, cost parameter sensitivity analysis, on-chain cost estimation
**Confidence:** HIGH

## Summary

Phase 47 validates that every cost parameter in the system is justified by external evidence rather than assumptions. The current codebase has two cost pipelines: (1) the `SpreadEngine` for Polymarket-vs-Kalshi prediction market spreads, and (2) the `CrossAssetEngine` (signal engine) for Polymarket-vs-options (Deribit/Derive) arbitrage. Both share the same fee config structs from `spread::config` but the signal engine adds a `deribit_taker_fee_rate` field for options fees.

The research identifies three concrete gaps: (a) the `deribit_taker_fee_rate` default of 0.0003 (0.03%) is correct for base tier but doesn't account for the 12.5% cap on option premium -- this matters for cheap options; (b) there is no Derive-specific fee rate -- the system uses Deribit's rate for all options venues; (c) there are zero on-chain cost estimates for Polymarket execution (gas, approval transactions, bridging).

**Primary recommendation:** Build a `cost-validation` CLI that (1) documents each parameter with its exchange source, (2) runs perturbation-based sensitivity analysis on existing signal logs, and (3) adds on-chain cost fields to the Polymarket cost model config.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| COST-01 | Cost model parameters validated against exchange fee documentation (Deribit, Derive, Polymarket) | Deribit fees verified at 0.03% taker (cap 12.5% of premium), 0.015% delivery. Derive taker fee is 0.04% (not 0.03%). Polymarket crypto formula confirmed: fee_rate=0.25, exponent=2. See Standard Stack section for all values. |
| COST-02 | Parameter sensitivity analysis shows which cost components have largest impact on net edge | Existing `cost_audit` module already ranks components by magnitude. Extend with perturbation analysis: vary each parameter +/-50% and measure delta on mean net_edge. No new dependencies needed. |
| COST-03 | On-chain execution costs (gas, bridging) estimated and included in Polymarket leg cost model | Polygon gas ~$0.005-0.01 per trade. Bridging from Ethereum ~$5-20 (amortized). New config fields needed in PolymarketFeeConfig. See Architecture Patterns section. |
</phase_requirements>

## Standard Stack

### Core (No New Dependencies)

This phase requires zero new crate dependencies. All work uses existing infrastructure:

| Component | Current | Purpose |
|-----------|---------|---------|
| `spread::config` | `PolymarketFeeConfig`, `KalshiFeeConfig`, `CarryConfig` | Fee parameter structs with serde defaults |
| `signal::config` | `SignalGenerationConfig` | Cross-asset fee params including `deribit_taker_fee_rate` |
| `analysis::cost_audit` | `compute_cost_audit()` | Existing component ranking by magnitude |
| `analysis::stats` | `mean_f64`, `stddev_f64`, etc. | Statistical helpers for sensitivity analysis |
| `analysis::io` | Signal JSONL loading | Shared I/O for CLI tools |

### Exchange Fee Reference Values (Validated)

#### Deribit BTC Options (HIGH confidence)
| Parameter | Value | Source |
|-----------|-------|--------|
| Taker fee | 0.03% of underlying (0.0003) | Deribit support docs, multiple review sites |
| Maker fee | 0.03% of underlying (0.0003) | Same (flat, no maker/taker split for base tier) |
| Fee cap | 12.5% of option premium | Deribit docs |
| Delivery/exercise fee | 0.015% of contract value | Deribit settlement docs |
| Daily options delivery | Exempt (0%) | Deribit settlement docs |
| VIP discounts | 16.66% (VIP1) to 66.66% (VIP6) off base rate | Deribit insights |

**Current code default:** `deribit_taker_fee_rate = 0.0003` -- CORRECT for base tier.
**Missing:** Fee cap at 12.5% of premium is NOT implemented. For deep OTM options with low premiums, this cap matters.
**Missing:** Delivery fee of 0.015% is not modeled (relevant for held-to-expiry scenarios).

#### Derive Options (MEDIUM confidence)
| Parameter | Value | Source |
|-----------|-------|--------|
| Taker fee | 0.04% of notional + $0.50 base | Derive help center |
| Maker fee | 0.03% of notional (rebate possible at top tiers) | Derive help center |
| Fee cap | 12.5% of option value | Derive help center |
| Top-tier maker | -0.005% (rebate) | Derive insights blog |
| Top-tier taker | 0.0075% | Derive insights blog |

**Current code:** Uses `deribit_taker_fee_rate` (0.0003) for ALL options venues. Derive's base taker rate is 0.0004 (0.04%) + $0.50 flat -- higher than Deribit.
**Action needed:** Add `derive_taker_fee_rate` or venue-aware options fee config.

#### Polymarket Crypto Markets (HIGH confidence)
| Parameter | Value | Source |
|-----------|-------|--------|
| Fee formula | `C * p * fee_rate * (p * (1-p))^exponent` | Polymarket docs (confirmed March 2026) |
| fee_rate (crypto) | 0.25 | Polymarket docs |
| exponent (crypto) | 2 | Polymarket docs |
| Max effective rate | 1.56% at p=0.50 | Polymarket docs |
| Maker rebate | 20% of taker fee | Polymarket docs |
| Min fee | 0.0001 USDC | Polymarket docs |

**Current code defaults:** `fee_rate = 0.25`, `exponent = 2` -- CORRECT for crypto markets.
**Note:** Fee formula in code matches official docs exactly.

#### On-Chain Costs for Polymarket (MEDIUM confidence)
| Cost Component | Estimate | Source |
|----------------|----------|--------|
| Polygon gas per trade | $0.005 - $0.01 | PolygonScan avg tx fee chart |
| ERC20 approve tx | ~$0.005 | Standard Polygon gas estimate |
| Bridging (Ethereum to Polygon) | $5-20 per bridge tx | Multiple bridge comparison sites |
| Bridging (L2/exchange direct) | $0.50-2.00 | Exchange withdrawal fees |

**Current code:** Zero on-chain costs modeled. The `PolymarketFeeConfig` only has the exchange fee formula.

## Architecture Patterns

### Pattern 1: Cost Validation Report CLI

**What:** A `cost-validate` binary that loads config, compares each parameter to documented exchange values, and outputs a validation report.
**When to use:** COST-01 requirement -- document and verify all parameters.

```
src/
  bin/cost_validate.rs     # CLI entry point
  analysis/
    cost_validate.rs       # Core validation logic
    cost_audit.rs          # (existing) component ranking
    sensitivity.rs         # (new) perturbation analysis
```

The validation module should define a `ValidationEntry` struct:

```rust
struct ValidationEntry {
    parameter: String,        // e.g., "deribit_taker_fee_rate"
    config_value: String,     // e.g., "0.0003"
    expected_value: String,   // e.g., "0.0003"
    source: String,           // e.g., "Deribit support docs: 0.03% of underlying"
    status: ValidationStatus, // Match / Mismatch / Missing
}
```

### Pattern 2: Perturbation-Based Sensitivity Analysis

**What:** For each cost parameter, perturb it by +/-50% (or a configurable range), recompute net_edge across all signals, and report the delta.
**When to use:** COST-02 requirement -- rank cost components by impact on net edge.

The existing `compute_cost_audit` function processes signals after costs are baked in. Sensitivity analysis needs to re-derive costs from raw spread minus perturbed cost components. The `CostBreakdown` on each `ArbSignal` already stores individual components, so:

```
For each cost component C:
  For each perturbation factor f in [0.5, 0.75, 1.0, 1.25, 1.50]:
    adjusted_total_cost = total_cost - C_original + C_original * f
    adjusted_net_edge = raw_spread - adjusted_total_cost
  Report: delta_net_edge per unit change in C
```

Output: Table sorted by |delta_net_edge / delta_parameter|, showing which parameters matter most.

### Pattern 3: On-Chain Cost Config Extension

**What:** Add gas and bridging cost fields to `PolymarketFeeConfig`.
**When to use:** COST-03 requirement.

```rust
// In spread/config.rs, extend PolymarketFeeConfig:
pub struct PolymarketFeeConfig {
    // ... existing fields ...

    /// Estimated gas cost per trade on Polygon (USD).
    /// Default: 0.01 (conservative estimate for CTF exchange interaction).
    #[serde(default = "default_gas_cost_usd")]
    #[serde(with = "rust_decimal::serde::str")]
    pub gas_cost_usd: Decimal,

    /// Amortized bridging cost per trade (USD).
    /// Assumes periodic bridging; amortized over expected trades per bridge.
    /// Default: 0.0 (operator must set based on their bridging pattern).
    #[serde(default)]
    #[serde(with = "rust_decimal::serde::str")]
    pub bridge_cost_amortized_usd: Decimal,
}
```

These fields flow into `polymarket_fee()` as additive USD costs, then get normalized to probability space via `/ target_notional` in the engine.

### Pattern 4: Derive Fee Differentiation

**What:** Add a `derive_taker_fee_rate` to `SignalGenerationConfig` so options fee calculation can vary by venue.
**When to use:** When the signal engine processes Derive-sourced implied probabilities.

Currently line 443-445 of signal/engine.rs uses `self.config.deribit_taker_fee_rate` for ALL options. This should become venue-aware:

```rust
let fee_rate = match prob.source_venue {
    Venue::Deribit => self.config.deribit_taker_fee_rate,
    Venue::Derive => self.config.derive_taker_fee_rate,
    _ => self.config.deribit_taker_fee_rate, // fallback
};
```

### Anti-Patterns to Avoid

- **Hardcoding fee values in Rust code:** All fee parameters must remain in config.toml. The validation CLI compares config values against documented expectations, but doesn't hardcode "correct" values.
- **Recomputing signals from scratch:** Sensitivity analysis should work on existing signal log data, perturbing stored cost components, NOT re-running the full pricing pipeline.
- **Modeling gas costs as percentage fees:** Polygon gas is a fixed-dollar cost per transaction, not proportional to trade size. Model as additive USD, not multiplicative.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Statistical analysis | Custom stat functions | Existing `analysis::stats` module | Already has mean, stddev, median, percentile, Pearson, KS |
| Table rendering | Manual formatting | Existing `analysis::output` module | Consistent with cost-audit and book-depth CLIs |
| Signal loading | Custom file parsing | Existing `analysis::io` module | Handles JSONL, time filtering, glob patterns |
| CLI argument parsing | Manual arg handling | `clap` (already in deps) | Consistent with other CLI tools |

## Common Pitfalls

### Pitfall 1: Fee Cap Not Modeled
**What goes wrong:** Deep OTM options have very low premiums. Without the 12.5% cap, the fee formula `0.03% * underlying_price * delta` can exceed the option value.
**Why it happens:** The cap is exchange-specific behavior not documented in simplified fee descriptions.
**How to avoid:** Implement `min(fee_rate * underlying * delta, 0.125 * option_premium)` for Deribit/Derive.
**Warning signs:** options_fee_estimate > option premium in signal logs.

### Pitfall 2: Dollar vs Probability Space Confusion
**What goes wrong:** Gas costs are in USD. Prediction fees are computed in USD then divided by target_notional. Mixing units silently produces wrong net_edge.
**Why it happens:** The system has TWO normalization paths (see engine.rs line 281 and signal/engine.rs line 482).
**How to avoid:** Always explicitly comment unit space. Gas/bridge costs are USD, normalized to probability space via `/ target_notional`.
**Warning signs:** This was already fixed once in Phase 44 (FIX-01). Follow the same pattern.

### Pitfall 3: Sensitivity Analysis on Empty/Sparse Data
**What goes wrong:** If signal_logs have few entries, sensitivity results are meaningless noise.
**Why it happens:** Production may not have accumulated enough signals yet.
**How to avoid:** Require minimum sample count (e.g., 20 signals) before producing sensitivity output. Print warning otherwise.
**Warning signs:** Sensitivity report shows high variance with < 10 signals.

### Pitfall 4: Derive Base Fee ($0.50) as Fixed Cost
**What goes wrong:** Derive's $0.50 base fee per trade is fixed, not proportional. For a $500 notional trade, that's 0.1% -- significant. Easy to miss because Deribit has no base fee.
**Why it happens:** Assuming all exchanges have purely percentage-based fees.
**How to avoid:** Model Derive fee as `$0.50 + 0.04% * notional`, converted to probability space.

## Code Examples

### Sensitivity Analysis Core Logic

```rust
/// Compute sensitivity of net_edge to a single cost component.
///
/// Perturbs the component by each factor, recomputes net_edge,
/// and returns the slope (delta_net_edge / delta_factor).
pub fn component_sensitivity(
    signals: &[ArbSignal],
    component_name: &str,
    factors: &[f64],  // e.g., [0.5, 0.75, 1.0, 1.25, 1.5]
) -> SensitivityResult {
    // For each factor, compute mean adjusted net_edge
    let mut factor_vs_edge: Vec<(f64, f64)> = Vec::new();

    for &factor in factors {
        let adjusted_edges: Vec<f64> = signals.iter().map(|s| {
            let cb = &s.cost_breakdown;
            let original = get_component(cb, component_name);
            let delta = original * Decimal::from_f64(factor - 1.0).unwrap();
            // net_edge = raw_spread - total_cost => adjusted = net_edge + original - original*factor
            //                                               = net_edge - delta
            let adjusted = s.net_edge - delta;
            adjusted.to_f64().unwrap_or(0.0)
        }).collect();

        let mean = mean_f64(&adjusted_edges).unwrap_or(0.0);
        factor_vs_edge.push((factor, mean));
    }

    // Linear slope from factor=0.5 to factor=1.5
    // ...
}
```

### Validation Entry Generation

```rust
/// Compare config values against documented exchange fee schedules.
pub fn validate_deribit_fees(config: &SignalGenerationConfig) -> Vec<ValidationEntry> {
    vec![
        ValidationEntry {
            parameter: "deribit_taker_fee_rate".into(),
            config_value: config.deribit_taker_fee_rate.to_string(),
            expected_value: "0.0003".into(),
            source: "Deribit: 0.03% of underlying (base tier)".into(),
            status: if config.deribit_taker_fee_rate == dec("0.0003") {
                ValidationStatus::Match
            } else {
                ValidationStatus::Mismatch
            },
        },
        // ... more entries
    ]
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Integer ceiling for Kalshi fees | Cents-precision ceiling | Phase 44 (FIX-02) | Up to 57x overstatement fixed |
| Fees subtracted in dollar space | Fees normalized to probability space | Phase 44 (FIX-01) | Fixed -19.5 net_edge bug |
| No cost audit tooling | cost-audit CLI with component ranking | Phase 46 (DIAG-01) | Can now identify dominant cost drivers |
| Single options fee rate | Still single rate (gap) | -- | Need venue-aware fee rate for Derive |

## Open Questions

1. **Deribit VIP tier applicability**
   - What we know: VIP tiers exist (16.66% to 66.66% discount), require 100k+ USDC equity
   - What's unclear: Whether the project's trading account qualifies for any VIP tier
   - Recommendation: Use base tier (0.03%) as conservative default. Add `deribit_vip_discount` config field for future use.

2. **Polymarket fee-enabled status per market**
   - What we know: Not all markets have fees enabled. The `feesEnabled` flag varies per market.
   - What's unclear: Whether the specific crypto markets we trade always have fees enabled
   - Recommendation: Query `GET /fee-rate?token_id={id}` at runtime, but for cost model validation, assume fees are always enabled (conservative).

3. **Bridging frequency assumption**
   - What we know: Bridge cost per transaction is $5-20 from Ethereum, <$2 from exchanges
   - What's unclear: How to amortize bridging cost -- depends on trading frequency and rebalancing needs
   - Recommendation: Make `bridge_cost_amortized_usd` a config parameter defaulting to 0, with documentation explaining how to calculate it based on trading patterns.

## Sources

### Primary (HIGH confidence)
- Polymarket docs (https://docs.polymarket.com/trading/fees) - Fee formula, fee_rate, exponent values confirmed March 2026
- Deribit support (https://support.deribit.com/hc/en-us/articles/25944746248989-Fees) - Base fee rates
- Deribit settlement docs (https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) - Delivery fee 0.015%

### Secondary (MEDIUM confidence)
- Derive help center (https://help.derive.xyz/en/articles/8691534-what-are-the-fees) - Maker 0.03%, taker 0.04% + $0.50 base
- Derive insights blog (https://insights.derive.xyz/derives-new-options-fee-schedule-tighter-spreads-cheaper-trading/) - Updated fee tiers
- PolygonScan gas tracker (https://polygonscan.com/gastracker) - Current gas prices
- PolygonScan avg tx fee (https://polygonscan.com/chart/avg-txfee-usd) - Historical gas cost data

### Tertiary (LOW confidence)
- Multiple review sites for Deribit fee cross-verification (milkroad.com, jeangalea.com)
- Bridge cost estimates from general crypto guides (approximate, varies with congestion)

## Metadata

**Confidence breakdown:**
- Polymarket fee formula: HIGH - Confirmed from official docs, matches code exactly
- Deribit fee rates: HIGH - Multiple sources agree on 0.03% taker, 12.5% cap, 0.015% delivery
- Derive fee rates: MEDIUM - Help center confirmed 0.04% taker + $0.50, but full tier structure not verified
- On-chain costs: MEDIUM - Polygon gas is well-documented at <$0.01, bridging costs are approximate
- Sensitivity analysis approach: HIGH - Standard perturbation method, no external dependencies needed

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (fee schedules change infrequently; gas costs are volatile but order-of-magnitude stable)
