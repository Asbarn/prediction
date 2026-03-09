---
phase: 47-cost-model-validation
verified: 2026-03-09T22:05:12Z
status: passed
score: 7/7 must-haves verified
---

# Phase 47: Cost Model Validation Verification Report

**Phase Goal:** Every cost parameter is justified by external evidence (exchange docs or on-chain data), not by what makes signals look profitable
**Verified:** 2026-03-09T22:05:12Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Derive options fees use 0.04% taker rate + $0.50 base fee, not Deribit's 0.03% rate | VERIFIED | `src/signal/config.rs:72-79` defines `derive_taker_fee_rate` (default 0.0004) and `derive_base_fee_usd` (default 0.50); `src/signal/engine.rs:444-449` dispatches via `Venue::Derive` match arm |
| 2 | Polymarket cost model includes gas and bridging cost fields | VERIFIED | `src/spread/config.rs:140-149` defines `gas_cost_usd` (default 0.01) and `bridge_cost_amortized_usd` (default 0.0); `src/signal/engine.rs:491-492` uses both in total cost calc |
| 3 | cost-validate CLI prints a validation report comparing config values to documented exchange fee schedules | VERIFIED | `src/bin/cost_validate.rs` is 199 lines with clap CLI, table/JSON output, config loading; `src/analysis/cost_validate.rs` validates 9 parameters with source citations |
| 4 | Each config parameter in the report has a cited source | VERIFIED | All 9 entries in `validate_signal_config()` have explicit `source` strings citing Deribit, Derive help center, Polymarket docs, PolygonScan, or internal documentation |
| 5 | Sensitivity analysis output ranks cost components by their impact on net edge | VERIFIED | `src/analysis/sensitivity.rs:150-156` sorts by `slope.abs()` descending, assigns 1-based ranks; unit test `dominant_component_ranks_first` confirms |
| 6 | Perturbation analysis shows how net edge changes when each parameter varies by +/-50% | VERIFIED | `DEFAULT_FACTORS = [0.5, 0.75, 1.0, 1.25, 1.5]` covers +/-50%; `component_sensitivity()` computes adjusted net_edge per factor; slope via finite difference |
| 7 | Operator can see which cost parameters matter most for profitability | VERIFIED | `src/bin/cost_validate.rs:31` exposes `--sensitivity` flag; CLI renders ranked table with Rank, Component, Slope, At 0.5x/1.0x/1.5x columns |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/spread/config.rs` | PolymarketFeeConfig with gas_cost_usd and bridge_cost_amortized_usd | VERIFIED | Fields at lines 140-149, defaults and serde attrs present, test at line 246 |
| `src/signal/config.rs` | SignalGenerationConfig with derive_taker_fee_rate and derive_base_fee_usd | VERIFIED | Fields at lines 72-79, defaults 0.0004 and 0.50, test at line 138 |
| `src/signal/engine.rs` | Venue-aware options fee calculation | VERIFIED | Match on `Venue::Derive` at line 445, on-chain cost at line 491, included in normalization at line 500 |
| `src/analysis/cost_validate.rs` | Validation logic with source citations | VERIFIED | 337 lines, exports `validate_signal_config`, `ValidationEntry`, `ValidationStatus`, 5 unit tests |
| `src/bin/cost_validate.rs` | cost-validate CLI binary | VERIFIED | 199 lines, clap parser, table/JSON output, --sensitivity/--log-dir/--from/--to/--last flags |
| `src/analysis/sensitivity.rs` | Perturbation-based sensitivity analysis | VERIFIED | 433 lines, exports `component_sensitivity`, `sensitivity_analysis`, `SensitivityResult`, `SensitivityReport`, 6 unit tests |
| `Cargo.toml` | [[bin]] entry for cost-validate | VERIFIED | Lines 110-112: `name = "cost-validate"`, `path = "src/bin/cost_validate.rs"` |
| `src/analysis/mod.rs` | Modules registered | VERIFIED | `pub mod cost_validate;` and `pub mod sensitivity;` present |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/signal/engine.rs` | `src/signal/config.rs` | `derive_taker_fee_rate` used in fee calc | WIRED | Line 446 reads `self.config.derive_taker_fee_rate`, line 449 reads `self.config.derive_base_fee_usd` |
| `src/signal/engine.rs` | `src/spread/config.rs` | `gas_cost_usd` in total cost | WIRED | Line 491 reads `self.config.polymarket_fees.gas_cost_usd`, line 500 includes in normalization |
| `src/analysis/cost_validate.rs` | `src/signal/config.rs` | Reads config values to compare | WIRED | Line 11 imports `SignalGenerationConfig`, line 66 accepts `&SignalGenerationConfig` |
| `src/analysis/sensitivity.rs` | `src/signal/types.rs` | Reads CostBreakdown fields | WIRED | Line 11 imports `CostBreakdown`, line 54 pattern-matches on 6 named fields |
| `src/bin/cost_validate.rs` | `src/analysis/sensitivity.rs` | CLI calls sensitivity_analysis | WIRED | Line 12 imports `sensitivity_analysis`, `sensitivity_table`; line 148 calls `sensitivity_analysis(&signals)` |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| COST-01 | 47-01 | Cost model parameters validated against exchange fee documentation | SATISFIED | `validate_signal_config()` checks 9 parameters with citations to Deribit, Derive, Polymarket, PolygonScan docs |
| COST-02 | 47-02 | Parameter sensitivity analysis shows which cost components have largest impact | SATISFIED | `sensitivity_analysis()` perturbs 6 cost components at 5 factors, ranks by |slope|; CLI --sensitivity flag |
| COST-03 | 47-01 | On-chain execution costs estimated and included in Polymarket leg cost model | SATISFIED | `gas_cost_usd` ($0.01 default from PolygonScan data), `bridge_cost_amortized_usd` (operator-defined); both included in total_cost normalization |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | - | - | - | No anti-patterns detected |

No TODOs, FIXMEs, placeholders, or empty implementations found in any phase-47 artifacts.

### Human Verification Required

### 1. CLI Output Readability

**Test:** Run `cargo run --bin cost-validate` and inspect the table formatting
**Expected:** Clean table with 9 rows, all showing MATCH except bridge_cost (UNDOCUMENTED), summary row at bottom
**Why human:** Table formatting/alignment is visual

### 2. Sensitivity Analysis with Real Data

**Test:** Run `cargo run --bin cost-validate -- --sensitivity --last 30`
**Expected:** Validation table followed by sensitivity table ranking 6 components by impact, with options_fee_estimate likely dominant
**Why human:** Verifying the ranking makes economic sense requires domain knowledge

### 3. JSON Output Validity

**Test:** Run `cargo run --bin cost-validate -- --sensitivity --output json --last 30`
**Expected:** Valid JSON with `validation` and `sensitivity` top-level keys
**Why human:** Quick manual check that JSON is well-formed and complete

---

_Verified: 2026-03-09T22:05:12Z_
_Verifier: Claude (gsd-verifier)_
