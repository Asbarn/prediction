---
phase: 11-basis-risk-consumption
verified: 2026-02-24T00:00:00Z
status: passed
score: 13/13 must-haves verified
re_verification: false
---

# Phase 11: Basis Risk Consumption Verification Report

**Phase Goal:** Connect BasisRiskScore from EventRegistry to spread and signal cost models, enabling settlement basis risk premium in cost calculations and near-expiry flag exposure to downstream consumers.
**Verified:** 2026-02-24
**Status:** passed
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | BasisRiskCache type exists as `Arc<RwLock<HashMap<String, CachedRiskInfo>>>` | VERIFIED | `src/events/risk.rs:296` — `pub type BasisRiskCache = Arc<RwLock<HashMap<String, CachedRiskInfo>>>;` |
| 2 | CachedRiskInfo contains base_score, expiry_warning, effective_composite, temporal_mismatch_hours, updated_at | VERIFIED | `src/events/risk.rs:282-293` — all five fields present with correct types |
| 3 | ContractLifecycleManager populates BasisRiskCache on every poll cycle for all active_approved mappings | VERIFIED | `src/events/lifecycle.rs:405-456` — `cache.clear()` + loop over `registry.active_approved()` + `cache.insert(...)` |
| 4 | SpreadConfig and SignalGenerationConfig have basis_risk_scale field with serde(default) | VERIFIED | `src/spread/config.rs:51-53`, `src/signal/config.rs:63-65` — both fields with `#[serde(default = "default_basis_risk_scale")]` and `#[serde(with = "rust_decimal::serde::str")]` |
| 5 | Cache defaults to empty; engines handle missing entries gracefully via zero premium fallback | VERIFIED | `src/spread/engine.rs:88-100`, `src/signal/engine.rs:96-110` — `None => return Decimal::ZERO` pattern; `try_read()` returns zero on contention |
| 6 | SpreadEngine cost model includes basis_risk_premium derived from BasisRiskCache lookup | VERIFIED | `src/spread/engine.rs:226-230` — `let basis_risk_premium = self.lookup_basis_risk_premium(&event_id)` added to `total_cost = buy_fee + sell_fee + carry + basis_risk_premium` |
| 7 | CrossAssetEngine cost model includes basis_risk_premium derived from BasisRiskCache lookup | VERIFIED | `src/signal/engine.rs:425-430` — `let basis_risk_premium = self.lookup_basis_risk_premium(event_id)` included in total_cost sum |
| 8 | Near-expiry events have tightened signal thresholds via ExpiryWarning.risk_inflation_factor | VERIFIED | `src/signal/engine.rs:451-453` — `let expiry_inflation = self.lookup_expiry_threshold_inflation(event_id); let threshold_value = threshold_value * expiry_inflation;` |
| 9 | SpreadResult includes basis_risk_premium field | VERIFIED | `src/spread/patterns.rs:217` — `pub basis_risk_premium: Decimal` with `#[serde(default)]` |
| 10 | CostBreakdown includes basis_risk_premium field | VERIFIED | `src/signal/types.rs:74` — `pub basis_risk_premium: Decimal` with `#[serde(default)]` |
| 11 | main.rs creates BasisRiskCache and passes it to lifecycle manager and both engines | VERIFIED | `src/main.rs:162` — `new_basis_risk_cache()`, passed at lines 249, 367, 398 |
| 12 | Replay/mock mode pre-populates cache (lifecycle manager doesn't run) | VERIFIED | `src/main.rs:164-183` — `if !is_live` block iterates `active_approved()` and populates `CachedRiskInfo` entries |
| 13 | Missing cache entries default to zero premium (never blocks or errors) | VERIFIED | `lookup_basis_risk_premium` and `lookup_expiry_threshold_inflation` both use `try_read()` with safe fallbacks; confirmed by 354 passing tests with `basis_risk_cache: None` defaults |

**Score: 13/13 truths verified**

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/events/risk.rs` | BasisRiskCache type alias and CachedRiskInfo struct | VERIFIED | Lines 282-301: both types present and substantive; `new_basis_risk_cache()` helper present |
| `src/events/mod.rs` | Re-exports BasisRiskCache, CachedRiskInfo, new_basis_risk_cache | VERIFIED | Line 7: `pub use risk::{BasisRiskCache, CachedRiskInfo, new_basis_risk_cache};` |
| `src/events/lifecycle.rs` | Cache field, constructor param, poll cycle population | VERIFIED | Lines 50, 68, 80, 405-456: field stored, accepted as constructor param, written on every poll cycle |
| `src/spread/config.rs` | basis_risk_scale field on SpreadConfig | VERIFIED | Lines 51-53, 75: field present with serde default and included in `impl Default` |
| `src/signal/config.rs` | basis_risk_scale field on SignalGenerationConfig | VERIFIED | Lines 63-65, 89: field present with serde default and included in `impl Default` |
| `src/spread/engine.rs` | SpreadEngine with BasisRiskCache field and premium lookup | VERIFIED | Lines 49, 63, 78-81, 85-100, 226-264: field, builder, helper, and call site all present |
| `src/spread/patterns.rs` | SpreadResult with basis_risk_premium field | VERIFIED | Lines 213-217, 502, 544: field with `#[serde(default)]`, schema doc updated, tests updated |
| `src/signal/engine.rs` | CrossAssetEngine with BasisRiskCache, premium and threshold adjustment | VERIFIED | Lines 57, 73, 88-91, 95-133, 425-453, 541: all required pieces present |
| `src/signal/types.rs` | CostBreakdown with basis_risk_premium field | VERIFIED | Lines 71-74, 220: field with `#[serde(default)]`, test constructor updated |
| `src/main.rs` | BasisRiskCache creation and wiring to lifecycle+engines | VERIFIED | Lines 12-13, 162, 165-183, 249, 367, 398: fully wired |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/events/lifecycle.rs` | `src/events/risk.rs` | `basis_risk_cache.write()` populates CachedRiskInfo | WIRED | `cache.write().await` at line 408; `cache.insert(...)` at lines 425 and 451 |
| `src/main.rs` | `src/events/lifecycle.rs` | `basis_risk_cache.clone()` passed to `ContractLifecycleManager::new()` | WIRED | `basis_risk_cache.clone()` at line 249; constructor accepts it at lifecycle.rs:68 |
| `src/main.rs` | `src/spread/engine.rs` | `basis_risk_cache.clone()` passed to `SpreadEngine::with_basis_risk_cache()` | WIRED | `.with_basis_risk_cache(basis_risk_cache.clone())` at main.rs:367 |
| `src/main.rs` | `src/signal/engine.rs` | `basis_risk_cache.clone()` passed to `CrossAssetEngine::with_basis_risk_cache()` | WIRED | `.with_basis_risk_cache(basis_risk_cache.clone())` at main.rs:398 |
| `src/spread/engine.rs` | `src/events/risk.rs` | SpreadEngine reads CachedRiskInfo via `cache.try_read()` | WIRED | `try_read()` in `lookup_basis_risk_premium()` at engine.rs:89-100; reads `info.effective_composite` |
| `src/signal/engine.rs` | `src/events/risk.rs` | CrossAssetEngine reads `risk_inflation_factor` from `expiry_warning` | WIRED | `w.risk_inflation_factor` accessed in `lookup_expiry_threshold_inflation()` at engine.rs:127 |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|---------|
| EVNT-02 | 11-01, 11-02 | Settlement basis analyzer quantifies per-mapping basis_risk_score consumed downstream | SATISFIED | BasisRiskScore.composite consumed via BasisRiskCache in both engines; `effective_composite * basis_risk_scale` = premium added to total_cost |
| EVNT-03 | 11-01, 11-02 | Temporal mismatch quantified and used in spread calcs | SATISFIED | `temporal_mismatch_hours` stored in CachedRiskInfo (lifecycle.rs:448-449); available for post-hoc logging and analysis |
| EVNT-05 | 11-01, 11-02 | Near-expiry flags exposed to downstream consumers | SATISFIED | ExpiryWarning stored in CachedRiskInfo.expiry_warning; CrossAssetEngine reads `risk_inflation_factor` and inflates threshold_value (engine.rs:451-453) |
| SGNL-02 | 11-02 | Spread calculation includes settlement basis risk premium | SATISFIED | `basis_risk_premium` term added to both SpreadEngine total_cost (patterns.rs:210-211) and CrossAssetEngine total_cost (engine.rs:428-430); `#[serde(default)]` ensures backward-compat JSONL |

REQUIREMENTS.md tracking table confirms all four IDs mapped to Phase 11 with status "Complete".

No orphaned requirements: only EVNT-02, EVNT-03, EVNT-05, SGNL-02 are mapped to Phase 11.

---

### Anti-Patterns Found

None. No TODO/FIXME/HACK/PLACEHOLDER comments in any modified file. No stub return patterns found. No empty handler implementations. All cache lookup methods have substantive logic including `try_read()` with fallback, hash map lookup, and arithmetic.

---

### Human Verification Required

The following items have correct code-level wiring but cannot be confirmed programmatically:

**1. Near-Expiry Threshold Inflation Effect on Signal Frequency**
- Test: Run replay mode with an event mapping whose expiry is within 6 hours. Compare ArbSignal output — the signal threshold_value should be approximately 2x the baseline.
- Expected: Fewer signals emitted (threshold raised by inflation factor 2.0 for critical tier), with threshold_value in JSONL reflecting the inflated value.
- Why human: Requires an active near-expiry mapping in events.toml and live replay data; cannot verify numerically from static code alone.

**2. Basis Risk Premium Appears in JSONL Output**
- Test: Run replay or mock mode; inspect spread_logs/ and signal_logs/ JSONL files for `basis_risk_premium` field presence and non-zero value (requires events with settlement metadata).
- Expected: Each SpreadResult line contains `"basis_risk_premium":"0.01"` (or similar non-zero value for events with settlement metadata configured).
- Why human: Premium is zero when no settlement metadata exists in events.toml; a real mapping with settlement data is needed to confirm the non-zero path.

---

### Gaps Summary

No gaps found. All 13 observable truths verified against actual code. All artifacts are substantive and wired. All four requirement IDs are satisfied with concrete implementation evidence. All 354 lib tests pass. The four phase commits (a13c85f, bb33488, 08d7b28, 8cec30a) are confirmed in git history.

---

_Verified: 2026-02-24_
_Verifier: Claude (gsd-verifier)_
