# Phase 44: Critical Bug Fixes and Data Pipeline Repair - Research

**Researched:** 2026-03-09
**Domain:** Cost model arithmetic fixes, fee rounding correction, spread logger venue gating
**Confidence:** HIGH

## Summary

Phase 44 addresses three confirmed bugs identified by direct code inspection during v1.8 milestone research. These bugs collectively explain why all production signals show approximately -19.5 net edge and why spread_logs are empty.

The three bugs are: (1) a unit mismatch in `signal/engine.rs` where probability-space raw spreads (~0.08) are subtracted by dollar-denominated cost totals (~$19+), producing impossible net_edge values; (2) a Kalshi fee ceiling rounding error in `spread/cost_model.rs` where `Decimal::ceil()` rounds to the nearest integer instead of the nearest cent, overstating fees by up to 57x; and (3) a venue gate in `spread/engine.rs` that requires both Polymarket AND Kalshi presence in an event mapping before processing, which blocks all spread computation since Kalshi is disabled/geo-blocked.

All three fixes are pure code changes with zero new dependencies, zero new async channels, and zero infrastructure modifications. The fixes are fully specified by the source code -- no external research needed.

**Primary recommendation:** Fix all three bugs in a single phase, write unit tests for each fix confirming correct behavior, then verify end-to-end that signal_logs show plausible net_edge values and spread_logs contain SpreadResult entries.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FIX-01 | Cost model subtracts fees in the same unit space as raw spread (probability-space, not dollar-space) | Confirmed: `signal/engine.rs` line 471 subtracts dollar costs (~$19) from probability spread (~0.08). Fix: divide dollar-denominated costs by target_notional (500) to normalize to probability space. |
| FIX-02 | Kalshi taker fee calculation rounds to cents (not integers) via correct Decimal rounding | Confirmed: `cost_model.rs` line 57 uses `Decimal::ceil()` which rounds to integer ceiling. `0.0175.ceil() = 1` instead of `0.02`. Fix: use `(raw * Decimal::ONE_HUNDRED).ceil() / Decimal::ONE_HUNDRED`. |
| FIX-03 | Spread logger produces SpreadResult JSONL entries for active Polymarket-vs-options pairs (not gated on Kalshi presence) | Confirmed: `spread/engine.rs` line 228 requires both `polymarket` AND `kalshi` venues. Since Kalshi is disabled, no events pass. Fix: change gate to require Polymarket + any options venue (Deribit/Derive), or Polymarket + Kalshi. |
</phase_requirements>

## Standard Stack

### Core

No new dependencies. All fixes use existing crate APIs.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rust_decimal | 1.40 | All cost arithmetic | Already used throughout cost_model.rs, engine.rs |
| serde/serde_json | existing | SpreadResult serialization | Already used by SpreadLogger |
| tokio | existing | Async runtime | Already used by SpreadEngine::run |

### Supporting

No supporting libraries needed. This is a pure bug-fix phase.

### Alternatives Considered

None. The fixes are arithmetic corrections, not design choices.

## Architecture Patterns

### Affected Files

```
src/
  signal/
    engine.rs          # FIX-01: normalize costs to probability space
  spread/
    cost_model.rs      # FIX-02: Kalshi ceiling rounding to cents
    engine.rs          # FIX-03: relax venue gate to not require Kalshi
```

### Pattern 1: Cost Normalization (FIX-01)

**What:** All dollar-denominated cost components must be divided by `target_notional` before subtracting from `raw_spread`, which is in probability space (0-1 range).

**Why:** `raw_spread` is computed as `options_prob - pred_ask` (e.g., 0.55 - 0.47 = 0.08). But `prediction_fee` comes from `polymarket_fee(walk.filled_notional, ...)` which returns dollars (e.g., `500 * 0.25 * 0.0625 = 7.8125`). Similarly, `carry_cost(500, ...)` returns ~2.05. The total_cost ends up ~$19 in dollar space, subtracted from ~0.08 in probability space, yielding net_edge ~ -19.5.

**Current code (signal/engine.rs line 467-471):**
```rust
// Total cost (excluding liquidity factor, which multiplies edge)
let total_cost =
    prediction_fee + options_fee_estimate + carry + prediction_slippage + options_spread_cost + basis_risk_premium;

// Net edge = (raw_spread - total_cost) * liquidity_factor
let net_edge = (raw_spread - total_cost) * liquidity_factor;
```

**Fix:** Normalize each dollar-denominated cost by dividing by `target_notional`:
```rust
// Normalize dollar costs to probability space (same units as raw_spread)
let target = self.config.target_notional;
let prediction_fee_norm = prediction_fee / target;
let options_fee_norm = options_fee_estimate / target;
let carry_norm = carry / target;

// prediction_slippage is already in probability space (|avg_fill - top_of_book|)
// options_spread_cost is already in probability space (half of bid-ask spread)
// basis_risk_premium: scale factor applied to composite score, already small

let total_cost =
    prediction_fee_norm + options_fee_norm + carry_norm
    + prediction_slippage + options_spread_cost + basis_risk_premium;

let net_edge = (raw_spread - total_cost) * liquidity_factor;
```

**Which costs need normalization:**
| Cost Component | Current Units | Needs Division | Rationale |
|---------------|---------------|----------------|-----------|
| `prediction_fee` | USD (from `polymarket_fee(filled_notional, ...)`) | YES | Input is shares * fee_rate * ... |
| `options_fee_estimate` | USD (taker_rate * underlying_price * delta) | YES | Deribit fee is per-contract in USD |
| `carry` | USD (notional * rate * days/365) | YES | `carry_cost(target_notional, ...)` |
| `prediction_slippage` | probability | NO | `(avg_fill - top_of_book).abs()` -- both are probabilities |
| `options_spread_cost` | probability | NO | `(ask_prob - bid_prob) / 2` -- already in prob space |
| `basis_risk_premium` | dimensionless (score * 0.01) | NO | Already scaled small |

**Impact:** With default config (target_notional=500, p=0.50), prediction_fee ~ 1.5625. Normalized: 1.5625/500 = 0.003125. Options fee ~ 0.0003 * 85000 * 0.5 = 12.75. Normalized: 12.75/500 = 0.0255. Carry ~ 2.05. Normalized: 2.05/500 = 0.0041. Total normalized cost ~ 0.033, vs raw_spread ~ 0.08. Net edge ~ 0.047 (plausible) instead of -19.5.

### Pattern 2: Cents-Precision Ceiling Rounding (FIX-02)

**What:** Replace `Decimal::ceil()` with cents-precision ceiling: `(raw * 100).ceil() / 100`.

**Current code (cost_model.rs line 52-61):**
```rust
let per_contract_raw =
    config.taker_coefficient * price_probability * (Decimal::ONE - price_probability);

if config.use_ceiling {
    let per_contract_ceil = per_contract_raw.ceil();
    per_contract_ceil * contracts
}
```

**Bug:** `Decimal::ceil()` returns the smallest integer >= the value. So `Decimal::from_str("0.0175").unwrap().ceil()` returns `1`, not `0.02`. The existing test on line 162-176 even validates this wrong behavior.

**Fix:**
```rust
if config.use_ceiling {
    // Kalshi rounds up to the nearest cent (2 decimal places)
    let scaled = per_contract_raw * Decimal::ONE_HUNDRED;
    let per_contract_ceil = scaled.ceil() / Decimal::ONE_HUNDRED;
    per_contract_ceil * contracts
}
```

**Verification:** For p=0.25: `0.07 * 0.25 * 0.75 = 0.013125`. Cents ceiling: `ceil(1.3125) / 100 = 2 / 100 = 0.02`. For 1 contract: fee = $0.02 (correct, per success criteria). Old behavior: `ceil(0.013125) = 1`, fee = $1.00 (57x overstatement).

**Test update required:** The existing test `kalshi_fee_at_p50_with_ceiling` asserts `ceil(0.0175) = 1`. This must be updated to assert `ceil to cents(0.0175) = 0.02`, so fee = 10 * 0.02 = 0.20.

### Pattern 3: Relaxed Venue Gate for Spread Logger (FIX-03)

**What:** Change the SpreadEngine's venue gate so it can compute spreads for Polymarket-vs-options pairs, not just Polymarket-vs-Kalshi.

**Current code (spread/engine.rs line 227-229):**
```rust
// 2. Only process if mapping has both Polymarket AND Kalshi venue entries
if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() {
    return; // Deribit-only or single-venue -- skip (Phase 8)
}
```

**Problem:** Kalshi is disabled/geo-blocked from Poland. All credentials are PLACEHOLDER. No events will ever have Kalshi venue entries in practice. This gate blocks ALL spread computation.

**Design decision:** The SpreadEngine currently pairs Polymarket vs Kalshi (two prediction markets). The CrossAssetEngine separately pairs options-implied probabilities vs prediction markets. There are two possible approaches:

**Option A (Recommended): Fix spread engine to pair Polymarket vs options venues.**
The SpreadEngine should produce SpreadResult entries for Polymarket-vs-Deribit/Derive pairs, since those are the actual cross-venue pairs the system trades. This aligns it with what CrossAssetEngine already does and ensures spread_logs contain the data downstream phases need.

This requires:
1. Changing the venue gate to check for Polymarket + any options venue (Deribit or Derive)
2. Adapting the snapshot pairing to use options snapshots instead of Kalshi snapshots
3. Reusing the options-implied probability as the "other side" price

**Option B (Simpler): Make spread engine work for Polymarket + Kalshi OR Polymarket + options.**
Keep the existing Polymarket-vs-Kalshi path but add a second pairing path for Polymarket-vs-options. More code but less refactoring risk.

**Option C (Simplest): Remove the Kalshi gate entirely, let it process any pair with Polymarket.**
Change the gate to only require Polymarket presence. If Kalshi data arrives, pair with it; otherwise pair with whatever other venue data is available (currently: options-implied data comes through CrossAssetEngine, not SpreadEngine).

**Important context:** The SpreadEngine and CrossAssetEngine serve different purposes:
- SpreadEngine: pairs two prediction market venues (Polymarket vs Kalshi) -- spread_logs
- CrossAssetEngine: pairs options-implied prob vs prediction market -- signal_logs

Since Kalshi is permanently disabled, the SpreadEngine needs rethinking. The success criteria says "spread_logs JSONL files containing SpreadResult entries for active Polymarket-vs-options pairs." This means the planner should route Polymarket-vs-options pairs through the SpreadEngine (or a new log path), producing spread_logs.

**The simplest approach that satisfies the success criteria:** Have the CrossAssetEngine ALSO write to a spread-style JSONL log (or convert its ArbSignal to SpreadResult format), OR refactor SpreadEngine to accept ImpliedProbability + MarketSnapshot pairs. The planner should choose.

### Anti-Patterns to Avoid

- **Partial unit fix:** Do NOT fix only one cost component. All dollar-denominated costs must be normalized in one change, with a single test verifying the net_edge magnitude.
- **Fixing tests to match bugs:** The existing `kalshi_fee_at_p50_with_ceiling` test validates the WRONG behavior. Update the test to assert correct behavior, not the other way around.
- **Changing SpreadEngine without considering CrossAssetEngine:** Both engines have similar cost computation paths. The unit mismatch fix (FIX-01) must be applied to BOTH engines. Check `spread/engine.rs` line 280 for the same pattern.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Decimal ceiling to N places | Custom rounding math | `(value * 10^N).ceil() / 10^N` | Standard pattern; `rust_decimal` has no `ceil_dp(n)` but this idiom is idiomatic |
| Unit normalization | New cost types | Simple division by target_notional | Adding wrapper types is over-engineering for a one-line fix |

## Common Pitfalls

### Pitfall 1: Forgetting to fix BOTH engines
**What goes wrong:** FIX-01 is applied to `signal/engine.rs` but the same unit mismatch exists in `spread/engine.rs` line 280 (`sell_walk.avg_fill_price - buy_walk.avg_fill_price - total_cost`). In the spread engine, the book walker prices and fees are in the same space (both based on prediction market prices), so this may not have the same bug -- but must be verified.
**How to avoid:** Audit both engines for unit consistency. In the spread engine, `buy_fee` and `sell_fee` come from `polymarket_fee(filled_notional, ...)` which returns dollars, while `sell_walk.avg_fill_price - buy_walk.avg_fill_price` is in probability space. The same bug exists there too.

### Pitfall 2: Breaking the existing SpreadEngine tests
**What goes wrong:** The spread engine has integration tests with hard-coded expected values. Fixing the cost model changes those values.
**How to avoid:** Update ALL existing test assertions to match corrected math. Do not skip tests.

### Pitfall 3: Kalshi fee test is the bug specification
**What goes wrong:** The test `kalshi_fee_at_p50_with_ceiling` on cost_model.rs line 162-176 explicitly validates the buggy behavior (`ceil(0.0175) = 1`). A developer might think the test is correct and the requirement is wrong.
**How to avoid:** The test IS wrong. Update it. The correct behavior per Kalshi exchange rules: ceiling-round to cents, not to integers.

### Pitfall 4: Division by zero in cost normalization
**What goes wrong:** If `target_notional` is zero (misconfigured), division by zero panics.
**How to avoid:** The default is 500. Add a debug_assert or validation that target_notional > 0. The config validation module (`config/validation.rs`) could enforce this.

### Pitfall 5: SpreadEngine FIX-03 scope creep
**What goes wrong:** Trying to fully refactor SpreadEngine to pair options-vs-prediction-markets turns into a large refactor touching snapshot pairing, staleness gates, book walking, and fee computation.
**How to avoid:** The minimal fix is to make the existing spread logging happen for Polymarket-vs-options pairs. If the CrossAssetEngine already produces the right data into signal_logs, consider whether spread_logs need to duplicate it, or whether a simpler adapter (writing SpreadResult from ArbSignal) suffices.

## Code Examples

### FIX-01: Cost normalization in signal engine
```rust
// Source: signal/engine.rs, around line 467
// BEFORE (buggy):
let total_cost = prediction_fee + options_fee_estimate + carry
    + prediction_slippage + options_spread_cost + basis_risk_premium;

// AFTER (fixed):
let target = self.config.target_notional;
// Normalize dollar-denominated costs to probability space
let total_cost = (prediction_fee + options_fee_estimate + carry) / target
    + prediction_slippage + options_spread_cost + basis_risk_premium;
```

### FIX-02: Kalshi fee ceiling to cents
```rust
// Source: spread/cost_model.rs, around line 55
// BEFORE (buggy):
let per_contract_ceil = per_contract_raw.ceil(); // rounds $0.0175 to $1.00

// AFTER (fixed):
let per_contract_ceil = (per_contract_raw * Decimal::ONE_HUNDRED).ceil()
    / Decimal::ONE_HUNDRED; // rounds $0.0175 to $0.02
```

### FIX-02: Updated test assertion
```rust
#[test]
fn kalshi_fee_at_p50_with_ceiling() {
    let config = KalshiFeeConfig {
        taker_coefficient: dec("0.07"),
        use_ceiling: true,
    };
    let fee = kalshi_taker_fee(dec("10"), dec("0.50"), &config);
    // Per contract raw = 0.0175, ceil to cents = 0.02, * 10 = 0.20
    assert_eq!(fee, dec("0.20"));
}

#[test]
fn kalshi_fee_at_p25_with_ceiling() {
    // Success criteria: Kalshi taker fee on a $0.25 contract = $0.02
    let config = KalshiFeeConfig {
        taker_coefficient: dec("0.07"),
        use_ceiling: true,
    };
    let fee = kalshi_taker_fee(dec("1"), dec("0.25"), &config);
    // 0.07 * 0.25 * 0.75 = 0.013125, ceil to cents = 0.02
    assert_eq!(fee, dec("0.02"));
}
```

### FIX-03: Venue gate relaxation (minimal approach)
```rust
// Source: spread/engine.rs, around line 227
// BEFORE:
if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() {
    return;
}

// AFTER: require Polymarket + at least one other venue
let has_polymarket = mapping.venues.polymarket.is_some();
let has_other_venue = mapping.venues.kalshi.is_some()
    || mapping.venues.deribit.is_some()
    || mapping.venues.derive.is_some();
if !has_polymarket || !has_other_venue {
    return;
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `Decimal::ceil()` for Kalshi fees | `(val * 100).ceil() / 100` for cents precision | This phase | 57x fee overstatement eliminated |
| Dollar costs subtracted from probability spreads | All costs normalized to probability space | This phase | net_edge goes from -19.5 to ~0.04 |
| SpreadEngine requires Kalshi | SpreadEngine works with any pairing | This phase | spread_logs populated for first time |

## Open Questions

1. **SpreadEngine refactor scope for FIX-03**
   - What we know: The SpreadEngine pairs Polymarket vs Kalshi, but Kalshi is disabled. The CrossAssetEngine already pairs options vs Polymarket and produces signal_logs.
   - What's unclear: Should SpreadEngine be refactored to pair options vs Polymarket (duplicating CrossAssetEngine logic), or should spread_logs be generated from CrossAssetEngine output?
   - Recommendation: The planner should choose the minimal approach. If downstream phases (46-48) only need signal_logs, skip the SpreadEngine refactor and just ensure signal_logs have the right data. If spread_logs are specifically needed, add a SpreadResult writer to CrossAssetEngine.

2. **Spread engine cost normalization (FIX-01 in spread engine)**
   - What we know: `spread/engine.rs` line 280 has `net_spread = sell_walk.avg_fill_price - buy_walk.avg_fill_price - total_cost`. The fill prices are probabilities, but fees are dollars.
   - What's unclear: If FIX-03 changes what the spread engine computes (e.g., options-vs-Polymarket), the cost computation path may also change.
   - Recommendation: Fix the unit mismatch in both engines. The spread engine has the same bug pattern.

3. **`Decimal::ONE_HUNDRED` availability**
   - What we know: Need `Decimal::new(100, 0)` or similar constant.
   - What's unclear: Whether `rust_decimal` provides `ONE_HUNDRED` as a constant.
   - Recommendation: Use `Decimal::new(100, 0)` or define a local constant. Minor detail.

## Sources

### Primary (HIGH confidence)
- `src/signal/engine.rs` lines 396-471 -- confirmed unit mismatch: `raw_spread` (probability ~0.08) minus `total_cost` (dollars ~$19)
- `src/spread/cost_model.rs` lines 52-61 -- confirmed `Decimal::ceil()` rounds to integer, not to cents
- `src/spread/engine.rs` line 228 -- confirmed gate requires both Polymarket AND Kalshi
- `src/spread/config.rs` -- default `target_notional = 500`, default `taker_coefficient = 0.07`
- `.planning/research/SUMMARY.md` -- v1.8 milestone research confirming all three bugs

### Secondary (MEDIUM confidence)
- Kalshi fee documentation -- taker coefficient 0.07 with per-contract ceiling rounding to cents (from code analysis, needs exchange doc verification in Phase 47)
- `rust_decimal` crate documentation -- `ceil()` behavior confirmed by existing test assertions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, pure code fixes
- Architecture: HIGH -- all affected code paths identified by direct inspection
- Pitfalls: HIGH -- bugs confirmed by code, test corrections straightforward
- FIX-03 approach: MEDIUM -- multiple valid approaches, planner must choose scope

**Research date:** 2026-03-09
**Valid until:** indefinite (bug fixes don't go stale)
