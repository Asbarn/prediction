# Phase 11: BasisRiskScore Downstream Consumption - Research

**Researched:** 2026-02-24
**Domain:** Internal wiring -- connecting existing BasisRiskScore computation to spread/signal cost models
**Confidence:** HIGH

## Summary

Phase 11 closes four audit gaps (EVNT-02, EVNT-03, EVNT-05, SGNL-02) by wiring the already-computed `BasisRiskScore` from the `EventRegistry`/`ContractLifecycleManager` into the `SpreadEngine` and `CrossAssetEngine` cost models. The `BasisRiskScore` struct and all computation logic already exist in `src/events/risk.rs` (Phase 5), and the `ContractLifecycleManager` already computes and logs near-expiry inflation. The gap is purely **downstream consumption**: no engine ever reads these scores to adjust costs or thresholds.

This is a wiring-and-integration phase, not a new-feature phase. All the math exists. The work is: (1) make BasisRiskScore accessible to engines at runtime, (2) add a settlement basis risk premium term to both cost models, (3) expose near-expiry flags from ContractLifecycleManager to signal threshold adjustment, and (4) feed expiry temporal mismatch data into CrossAssetEngine spread calculations.

**Primary recommendation:** Add a `BasisRiskCache` (HashMap<event_id, BasisRiskScore>) populated by the ContractLifecycleManager lifecycle poll, shared with engines via `Arc<RwLock<>>`. Both SpreadEngine and CrossAssetEngine look up the score during spread computation and add `composite * configurable_scale_factor` as a settlement basis risk premium cost term.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EVNT-02 | Settlement basis analyzer produces basis_risk_score | Already computed in `events/risk.rs`. Gap: not consumed by downstream engines. Wire `BasisRiskScore.composite` into SpreadEngine and CrossAssetEngine cost models as a settlement basis risk premium. |
| EVNT-03 | Expiry alignment validation quantifies temporal mismatch as basis risk | Already computed in `compute_risk_for_mapping()` which derives `settlement_time_risk` from Deribit-vs-prediction temporal gap. Gap: this temporal mismatch data is not used in CrossAssetEngine spread calculations. Wire temporal mismatch from BasisRiskScore into CrossAssetEngine. |
| EVNT-05 | Near-expiry contracts receive special handling flags (pricing character change, liquidity warnings, elevated settlement risk) | Already computed in `check_expiry_warning()` and `inflate_risk_score()` in ContractLifecycleManager. Gap: flags are logged but not exposed to signal threshold adjustment. Expose `ExpiryWarning.flags` and inflation factor to threshold computation. |
| SGNL-02 | Spread calculation adjusts for settlement basis risk premium | Fees, slippage, carry cost all implemented. Gap: "settlement basis risk premium" term is missing from both SpreadEngine and CrossAssetEngine cost models. Add basis_risk_premium derived from BasisRiskScore.composite to the total_cost calculation. |
</phase_requirements>

## Standard Stack

### Core

This phase uses no new external libraries. All work is internal wiring using existing Rust standard library and project types.

| Component | Location | Purpose | Status |
|-----------|----------|---------|--------|
| `BasisRiskScore` | `src/events/risk.rs` | Composite risk score with settlement_time, source, criteria components | EXISTS - needs consumption |
| `compute_risk_for_mapping()` | `src/events/risk.rs` | Computes BasisRiskScore from EventMapping metadata | EXISTS - needs runtime call |
| `check_expiry_warning()` | `src/events/risk.rs` | Returns ExpiryWarning with tier/flags/inflation for near-expiry | EXISTS - needs exposure |
| `inflate_risk_score()` | `src/events/risk.rs` | Inflates settlement_time_risk by expiry tier factor | EXISTS - needs exposure |
| `EventRegistry` | `src/events/registry.rs` | In-memory registry with lookup by event_id and instrument | EXISTS - needs BasisRiskScore storage |
| `ContractLifecycleManager` | `src/events/lifecycle.rs` | Periodic poll that computes risk scores and warnings | EXISTS - needs to publish scores |
| `SpreadEngine` | `src/spread/engine.rs` | Prediction market spread computation | EXISTS - needs basis risk premium |
| `CrossAssetEngine` | `src/signal/engine.rs` | Cross-asset arbitrage signal generation | EXISTS - needs basis risk premium + temporal mismatch |

### Supporting

| Type | Location | Purpose |
|------|----------|---------|
| `ExpiryWarning` | `src/events/risk.rs` | Near-expiry warning with tier_name, flags, hours_to_expiry, risk_inflation_factor |
| `RiskWeightsConfig` | `src/config/events.rs` | Configurable weights for composite risk scoring |
| `SettlementMetadata` | `src/config/events.rs` | Per-mapping settlement time/source metadata |
| `ThresholdConfig` | `src/spread/config.rs` | Dynamic threshold parameters (static_floor, k, liquidity_penalty, cold_start) |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Shared `Arc<RwLock<HashMap>>` cache | Store BasisRiskScore directly on EventMapping | Would require mutating config structs and EventRegistry at runtime -- violates current immutable-config pattern. Cache is cleaner. |
| Per-computation `compute_risk_for_mapping()` | Pre-computed cache refreshed by lifecycle manager | Per-computation is simpler but adds latency to every spread calc. Cache is refreshed on lifecycle poll (every 5-10min) -- risk scores change slowly. |
| Scale factor in cost model | Additive fixed premium | Scale factor is more principled: high-risk mappings get proportionally larger premiums while zero-risk mappings get zero premium. |

## Architecture Patterns

### Recommended Data Flow

```
ContractLifecycleManager (periodic poll)
    |
    |-- compute_risk_for_mapping() per active mapping
    |-- check_expiry_warning() per active mapping
    |-- inflate_risk_score() if near-expiry
    |
    v
BasisRiskCache: Arc<RwLock<HashMap<String, CachedRiskInfo>>>
    |
    +--------+--------+
    |                  |
    v                  v
SpreadEngine     CrossAssetEngine
    |                  |
    |-- lookup by      |-- lookup by event_id
    |   event_id       |-- add basis_risk_premium to total_cost
    |-- add basis_     |-- use temporal_mismatch_hours in spread
    |   risk_premium   |-- adjust threshold if near_expiry flags
    |   to total_cost  |
    v                  v
SpreadResult     ArbSignal (with basis_risk_premium in CostBreakdown)
```

### Pattern 1: BasisRiskCache Shared State

**What:** A cache struct holding per-event BasisRiskScore, ExpiryWarning, and the (optionally inflated) composite score. Updated by ContractLifecycleManager on each poll cycle, read by SpreadEngine and CrossAssetEngine.

**When to use:** Always -- this is the primary pattern for this phase.

**Structure:**
```rust
/// Cached basis risk info per event mapping, updated by lifecycle manager.
pub struct CachedRiskInfo {
    /// Base BasisRiskScore (not inflated).
    pub base_score: BasisRiskScore,
    /// Near-expiry warning (if within any threshold).
    pub expiry_warning: Option<ExpiryWarning>,
    /// Effective composite score (inflated if near-expiry, else base).
    pub effective_composite: f64,
    /// Settlement time difference in hours (for temporal mismatch).
    pub temporal_mismatch_hours: f64,
    /// Last updated timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Thread-safe cache shared between lifecycle manager and engines.
pub type BasisRiskCache = Arc<RwLock<HashMap<String, CachedRiskInfo>>>;
```

### Pattern 2: Settlement Basis Risk Premium in Cost Model

**What:** A new cost term `basis_risk_premium = effective_composite * basis_risk_scale_factor` added to total_cost in both SpreadEngine and CrossAssetEngine.

**When to use:** Every spread computation that has a corresponding event_id in the BasisRiskCache.

**Integration points:**

For **SpreadEngine** (`src/spread/engine.rs`, `process_snapshot` method around line 194-195):
```rust
// Current:
let total_cost = buy_fee + sell_fee + carry;

// After Phase 11:
let basis_risk_premium = self.lookup_basis_risk_premium(&event_id);
let total_cost = buy_fee + sell_fee + carry + basis_risk_premium;
```

For **CrossAssetEngine** (`src/signal/engine.rs`, `compute_and_emit` method around line 372-373):
```rust
// Current:
let total_cost = prediction_fee + options_fee_estimate + carry
    + prediction_slippage + options_spread_cost;

// After Phase 11:
let basis_risk_premium = self.lookup_basis_risk_premium(event_id);
let total_cost = prediction_fee + options_fee_estimate + carry
    + prediction_slippage + options_spread_cost + basis_risk_premium;
```

### Pattern 3: Near-Expiry Threshold Adjustment

**What:** When an event has active `ExpiryWarning` flags (especially "elevated_settlement_risk" or "pricing_character_change"), the signal threshold for that event is tightened (raised) proportionally.

**When to use:** In CrossAssetEngine's threshold evaluation, when basis risk cache contains an ExpiryWarning for the event.

**Design:** Apply the risk_inflation_factor from ExpiryWarning as a threshold multiplier:
```rust
let mut effective_threshold = threshold_value;
if let Some(risk_info) = cache.get(event_id) {
    if let Some(ref warning) = risk_info.expiry_warning {
        // Tighten threshold for near-expiry events
        effective_threshold = effective_threshold * warning.risk_inflation_factor;
    }
}
```

### Anti-Patterns to Avoid

- **Recomputing BasisRiskScore on every snapshot:** `compute_risk_for_mapping()` parses dates and does string operations. Do this once per lifecycle poll (5-10min), not per snapshot (100ms). The cache amortizes this.
- **Blocking on cache lock in hot path:** Use `try_read()` or ensure `read().await` is non-contentious. The lifecycle manager writes rarely (every 5-10 minutes); engines read frequently. RwLock is the right choice here.
- **Making basis risk premium mandatory:** If cache has no entry for an event_id (e.g., no settlement metadata configured), default to zero premium. Never block or error on missing risk data.
- **Coupling ExpiryWarning flags to hard-coded behavior:** Use the configurable risk_inflation_factor from the expiry threshold config, not magic numbers.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| BasisRiskScore computation | Custom risk math | `events::risk::compute_risk_for_mapping()` | Already tested, handles edge cases (missing metadata, unknown source pairs) |
| Expiry warning detection | Custom date math | `events::risk::check_expiry_warning()` | Already tested, picks tightest tier, handles edge cases |
| Risk inflation | Custom inflation math | `events::risk::inflate_risk_score()` | Recalculates composite correctly with default weights |
| Thread-safe shared cache | Custom sync primitives | `Arc<RwLock<HashMap>>` | Standard tokio pattern, already used for EventRegistry |

**Key insight:** All the risk computation functions exist and are well-tested. The only new code is the cache structure, the lookup integration in engines, and the config for the scale factor.

## Common Pitfalls

### Pitfall 1: Cache Stale After Config Hot-Reload
**What goes wrong:** Config hot-reload refreshes EventRegistry (config_rx subscriber in main.rs), but BasisRiskCache is not refreshed. New mappings have no risk data until next lifecycle poll.
**Why it happens:** BasisRiskCache is updated by ContractLifecycleManager, which runs on a separate polling schedule from config reload.
**How to avoid:** Either (a) trigger a cache refresh when config changes (add a watch subscriber for BasisRiskCache), or (b) accept that cache lags up to one poll interval after config change -- this is acceptable since risk scores change slowly and the lifecycle manager runs frequently.
**Warning signs:** New mappings showing zero basis_risk_premium in spread logs despite having settlement metadata.
**Recommendation:** Accept lag (option b) -- simpler, and one poll cycle is 5-10 minutes which is fine for risk parameter updates.

### Pitfall 2: Decimal vs f64 Mismatch
**What goes wrong:** BasisRiskScore uses `f64` for all fields. SpreadEngine/CrossAssetEngine cost models use `Decimal`. Mixing without conversion causes type errors.
**Why it happens:** BasisRiskScore was designed for logging/annotation (f64 is fine), but cost models need Decimal precision.
**How to avoid:** Convert f64 composite to Decimal at the cache lookup boundary using `Decimal::from_f64(composite).unwrap_or(Decimal::ZERO)`. This pattern is already used throughout the codebase (e.g., `Decimal::from_f64(prob.underlying_price)` in signal/engine.rs:342).
**Warning signs:** Compilation errors on type mismatch.

### Pitfall 3: SpreadResult/ArbSignal Missing basis_risk_premium Field
**What goes wrong:** Adding a new cost term to total_cost but not logging it separately means operators cannot distinguish basis risk premium from other costs in post-hoc analysis.
**Why it happens:** Temptation to just add to total_cost without surfacing the component.
**How to avoid:** Add `basis_risk_premium` field to both SpreadResult and CostBreakdown structs. Add `#[serde(default)]` for backward-compatible deserialization of old JSONL records.
**Warning signs:** Spread logs showing total_cost higher than sum of known components.

### Pitfall 4: ExpiryWarning Flags Not Available in Engine Context
**What goes wrong:** ExpiryWarning flags (e.g., "pricing_character_change") are only logged by lifecycle manager but never propagated to engines.
**Why it happens:** Currently the lifecycle manager computes warnings and logs them but doesn't store them in a shared data structure.
**How to avoid:** Include ExpiryWarning in CachedRiskInfo so engines can read flags and inflation factor.

### Pitfall 5: Replay Mode and BasisRiskCache
**What goes wrong:** In replay mode, ContractLifecycleManager doesn't run (per main.rs line 185: `if is_live`). BasisRiskCache will be empty, so no basis risk premium is applied during replay.
**Why it happens:** Lifecycle manager requires live REST API access.
**How to avoid:** In replay mode, pre-populate the cache from EventMapping settlement metadata at startup (one-time computation). This ensures replayed signals include basis risk adjustments. Use a simple loop: for each active mapping, compute_risk_for_mapping() and populate cache.
**Warning signs:** Replay spread logs showing zero basis_risk_premium for all events.

## Code Examples

### Cache Population (ContractLifecycleManager)

```rust
// In lifecycle.rs poll_cycle(), after step 6 (expiry warnings):
// Populate the shared BasisRiskCache.
{
    let registry = self.registry.read().await;
    let mut cache = self.basis_risk_cache.write().await;
    let now = Utc::now();

    for mapping in registry.active_approved() {
        let base_score = match compute_risk_for_mapping(mapping, &self.risk_weights) {
            Some(s) => s,
            None => continue, // no settlement metadata
        };

        // Parse expiry for warning check
        let expiry_warning = parse_expiry_datetime(&mapping.expiry)
            .and_then(|dt| check_expiry_warning(&dt, &now, &self.expiry_thresholds));

        let effective_composite = match &expiry_warning {
            Some(w) => inflate_risk_score(&base_score, w.risk_inflation_factor).composite,
            None => base_score.composite,
        };

        // Extract temporal mismatch hours
        let temporal_mismatch_hours = base_score.settlement_time_risk
            / self.risk_weights.time_per_hour.max(0.001); // reverse the scaling

        cache.insert(mapping.id.clone(), CachedRiskInfo {
            base_score,
            expiry_warning,
            effective_composite,
            temporal_mismatch_hours,
            updated_at: now,
        });
    }
}
```

### Basis Risk Premium Lookup (Engine Pattern)

```rust
// Shared pattern for both SpreadEngine and CrossAssetEngine
fn lookup_basis_risk_premium(
    cache: &BasisRiskCache,
    event_id: &str,
    scale_factor: Decimal,
) -> Decimal {
    // Non-blocking read -- if lock is contended, return zero (rare case)
    let guard = match cache.try_read() {
        Ok(g) => g,
        Err(_) => return Decimal::ZERO,
    };
    match guard.get(event_id) {
        Some(info) => {
            Decimal::from_f64(info.effective_composite)
                .unwrap_or(Decimal::ZERO)
                * scale_factor
        }
        None => Decimal::ZERO,
    }
}
```

### Near-Expiry Threshold Adjustment

```rust
// In CrossAssetEngine compute_and_emit(), after threshold computation:
let mut effective_threshold = threshold_value;
if let Some(ref cache) = self.basis_risk_cache {
    if let Ok(guard) = cache.try_read() {
        if let Some(risk_info) = guard.get(event_id) {
            // Add basis risk premium to CostBreakdown
            let basis_premium = Decimal::from_f64(risk_info.effective_composite)
                .unwrap_or(Decimal::ZERO) * self.config.basis_risk_scale;

            // Tighten threshold for near-expiry events
            if let Some(ref warning) = risk_info.expiry_warning {
                let inflation = Decimal::from_f64(warning.risk_inflation_factor)
                    .unwrap_or(Decimal::ONE);
                effective_threshold = effective_threshold * inflation;
            }
        }
    }
}
```

## State of the Art

| Old Approach (Current) | New Approach (Phase 11) | Impact |
|------------------------|------------------------|--------|
| BasisRiskScore computed, only logged | BasisRiskScore consumed in cost models | Settlement risk reflected in spread calculations |
| ExpiryWarning logged by lifecycle manager | ExpiryWarning exposed to threshold adjustment | Near-expiry signals require higher edge to pass |
| Temporal mismatch computed but unused | Temporal mismatch feeds into CrossAssetEngine | Expiry alignment gap quantified in signal metadata |
| Cost model: fees + slippage + carry | Cost model: fees + slippage + carry + basis_risk_premium | More accurate edge estimation |

## Open Questions

1. **Basis risk scale factor default value**
   - What we know: BasisRiskScore.composite ranges 0.0-1.0+ (typical values 0.2-0.7 for real mappings). Scale factor converts this to a cost term in probability space.
   - What's unclear: What scale factor produces sensible premiums? If composite=0.5 and scale=0.01, premium=0.005 (0.5%). Is that reasonable?
   - Recommendation: Default scale_factor=0.01 (1% of composite). This means a mapping with composite=0.5 pays 50bps extra. Make configurable in SpreadConfig and SignalGenerationConfig with `#[serde(default)]` for backward compatibility. Calibrate with 2-4 weeks of paper trading data.

2. **Should SpreadResult include the full CachedRiskInfo or just the premium?**
   - What we know: SpreadResult currently has no risk-related fields. Adding full CachedRiskInfo adds struct size.
   - What's unclear: How much risk metadata is useful in post-hoc analysis?
   - Recommendation: Add only `basis_risk_premium: Decimal` to SpreadResult and CostBreakdown. Log the full CachedRiskInfo separately via tracing at debug level. This keeps the JSONL output compact while preserving auditability.

3. **Cache lifetime and stale entries**
   - What we know: Lifecycle manager polls every 5-10 minutes. Mappings can expire between polls.
   - What's unclear: Should expired mappings be evicted from the cache?
   - Recommendation: Evict entries for expired mappings when the cache is refreshed. This is the natural behavior since the lifecycle manager iterates `active_approved()` only.

## Sources

### Primary (HIGH confidence)
- `src/events/risk.rs` -- BasisRiskScore struct and all computation functions (verified by reading source)
- `src/events/lifecycle.rs` -- ContractLifecycleManager poll cycle showing where risk is computed but only logged (verified by reading source)
- `src/spread/engine.rs` -- SpreadEngine cost model showing total_cost = buy_fee + sell_fee + carry (verified by reading source)
- `src/signal/engine.rs` -- CrossAssetEngine cost model showing total_cost without basis risk premium (verified by reading source)
- `src/signal/types.rs` -- CostBreakdown struct showing current cost fields (verified by reading source)
- `.planning/v1.0-MILESTONE-AUDIT.md` -- Gap documentation for EVNT-02, EVNT-03, EVNT-05, SGNL-02 (verified by reading source)

### Secondary (MEDIUM confidence)
- `src/events/registry.rs` -- EventRegistry lookup patterns and Arc<RwLock> sharing pattern (verified by reading source and main.rs wiring)
- `src/main.rs` -- Pipeline wiring showing how EventRegistry is shared and how lifecycle manager is spawned (verified by reading source)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all components exist, pure internal wiring
- Architecture: HIGH -- follows existing patterns (Arc<RwLock<>> sharing, cache population by background task)
- Pitfalls: HIGH -- identified from reading actual codebase code paths and gaps documented in audit

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable -- internal wiring, no external dependencies)
