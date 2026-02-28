# Phase 25: Tech Debt Sweep - Research

**Researched:** 2026-02-27
**Domain:** Rust code fixes -- IV spread propagation, config-driven book depth, Kalshi staleness computation
**Confidence:** HIGH

## Summary

Phase 25 addresses three behavior-changing tech debt items identified during v1.0 and carried forward through v1.1, v1.2, and v1.3. All three are isolated, well-scoped code fixes that require zero new crate dependencies. Each fix touches a different subsystem (signal engine, Deribit channel construction, Kalshi normalizer), making them independent and safe to implement in any order.

The codebase already contains the data needed for each fix -- it just is not being propagated to the right place. FIX-01 requires passing IV spread from `ImpliedProbability` (which already has `solver_meta` with bid/ask IV) to `ArbSignal.iv_spread`. FIX-02 requires reading a new `book_depth_levels` config field from the `[deribit]` config section and using it in `build_subscription_channels()` instead of the hardcoded `20`, plus populating the options leg's `book_depth_levels` from the Deribit snapshot's depth vector length instead of `0`. FIX-03 requires computing staleness from `exchange_timestamp` in the Kalshi normalizer, mirroring the pattern already used by the Polymarket normalizer.

**Primary recommendation:** Implement all three fixes as separate atomic changes. Each is 5-20 lines of code change. Follow the Polymarket normalizer pattern for FIX-03. Thread IV spread through `ImpliedProbability` for FIX-01. Add a config field for FIX-02.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FIX-01 | `iv_spread` field populated from IV solver metadata instead of always 0.0 | The `ImpliedProbability` struct does not carry an `iv_spread` field, but the IV solver already computes `bid_iv` and `ask_iv`. The `CrossAssetEngine.compute_and_emit()` at line 499-512 of `signal/engine.rs` hardcodes `iv_spread = 0.0`. Fix: add an `iv_spread` field to `ImpliedProbability`, populate it in `PricingEngine.process_snapshot()` (where `iv_spread = ask_iv - bid_iv` is already computed at line 282), and read it in `CrossAssetEngine`. |
| FIX-02 | Options `book_depth_levels` read from config instead of hardcoded 0 | Two sub-issues: (1) `build_subscription_channels()` in `channels.rs:121` hardcodes `20` in the book channel format `book.{inst}.none.20.100ms`. This should come from `DeribitConfig`. (2) `CrossAssetEngine` at `signal/engine.rs:560` hardcodes `book_depth_levels: 0` for the options leg. This should use the actual depth from the snapshot (available via the cached `ImpliedProbability`'s source snapshot depth, or stored alongside in the cache). |
| FIX-03 | Kalshi `is_stale` computed from exchange_timestamp instead of always false | `KalshiProcessor.produce_snapshot()` at `normalize.rs:272` hardcodes `let is_stale = false`. The `exchange_ts_ms` variable is already computed at line 255-270. Fix: mirror the Polymarket normalizer pattern -- check `is_exchange_data_stale(exchange_ts_ms, staleness_threshold_ms)`. |
</phase_requirements>

## Standard Stack

### Core

No new libraries needed. All fixes use existing project infrastructure.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| N/A | N/A | All fixes are code-level changes | Zero new dependencies (project convention) |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde | existing | Serialize new `iv_spread` field on `ImpliedProbability` | FIX-01 serialization |
| toml/serde | existing | Deserialize new `book_depth_levels` config field | FIX-02 config |
| chrono | existing | Staleness computation from exchange timestamps | FIX-03 staleness |

### Alternatives Considered

None. All three fixes are straightforward code corrections with no library choices.

**Installation:**
```bash
# No new dependencies
```

## Architecture Patterns

### Recommended Project Structure

No new files needed. All changes are to existing modules:

```
src/
├── pricing/
│   ├── engine.rs          # FIX-01: Populate iv_spread on ImpliedProbability
│   └── types.rs           # FIX-01: Add iv_spread field to ImpliedProbability
├── signal/
│   └── engine.rs          # FIX-01: Read iv_spread from ImpliedProbability
│                          # FIX-02: Read snapshot depth for options leg
├── feed/
│   ├── deribit/
│   │   ├── channels.rs    # FIX-02: Parameterize book depth in channel name
│   │   └── normalize.rs   # (unchanged -- depth already flows through)
│   └── kalshi/
│       └── normalize.rs   # FIX-03: Compute is_stale from exchange_timestamp
├── config/
│   └── venues.rs          # FIX-02: Add book_depth_levels field to DeribitConfig
└── tests/
    └── schema_golden_test.rs  # Update iv_spread value in golden test
```

### Pattern 1: Data Propagation (FIX-01)

**What:** Pass computed IV spread from PricingEngine through ImpliedProbability to CrossAssetEngine.
**When to use:** When a computed value in an upstream stage is needed downstream but is not currently carried by the intermediate data type.

**Current broken flow (signal/engine.rs:499-512):**
```rust
// BUG: This always produces 0.0
let iv_spread = match (prob.prob_bid, prob.prob_ask) {
    (Some(_bid), Some(_ask)) => {
        prob.solver_meta
            .as_ref()
            .map(|_| {
                // IV spread derived from the probability extraction bid/ask IVs
                // The actual IV spread is in the pricing engine, here we use skew_adjustment as proxy
                0.0_f64  // <-- ALWAYS 0.0
            })
            .unwrap_or(0.0)
    }
    _ => 0.0,
};
```

**Fixed flow:**
```rust
// Step 1: Add iv_spread to ImpliedProbability (pricing/types.rs)
pub struct ImpliedProbability {
    // ... existing fields ...
    /// IV bid-ask spread (ask_iv - bid_iv), populated from IV solver.
    pub iv_spread: f64,
}

// Step 2: Populate in PricingEngine (pricing/engine.rs, around line 282)
let iv_spread = ask_iv - bid_iv;
// ... later when building ImpliedProbability:
let implied_prob = ImpliedProbability {
    // ... existing fields ...
    iv_spread: iv_spread.max(0.0),
};

// Step 3: Read in CrossAssetEngine (signal/engine.rs)
let iv_spread = prob.iv_spread;
```

### Pattern 2: Config-Driven Constants (FIX-02)

**What:** Replace hardcoded values with config-driven parameters.
**When to use:** When a constant was hardcoded during initial development and should be operator-configurable.

**Two sub-issues in FIX-02:**

**Sub-issue A: Channel depth parameter**
```rust
// Current (channels.rs:121): hardcoded 20
channels.push(format!("book.{}.none.20.100ms", inst));

// Fixed: parameterized
channels.push(format!("book.{}.none.{}.100ms", inst, depth_levels));

// Config (venues.rs): add field to DeribitConfig
pub struct DeribitConfig {
    // ... existing fields ...
    /// Number of book depth levels to subscribe to (Deribit grouped book).
    /// Valid values: 1, 10, 20. Default: 20.
    #[serde(default = "default_book_depth_levels")]
    pub book_depth_levels: u32,
}

fn default_book_depth_levels() -> u32 {
    20
}
```

**Sub-issue B: Options leg book_depth_levels in ArbSignal**
```rust
// Current (signal/engine.rs:560): hardcoded 0
options_leg: LegInfo {
    // ...
    book_depth_levels: 0, // options don't have depth in our model
    // ...
},

// Fixed: use actual depth from the snapshot that generated the probability.
// The MarketSnapshot has depth_bids and depth_asks vectors.
// Problem: CrossAssetEngine caches ImpliedProbability, not MarketSnapshot.
// Solution: Add a book_depth_levels field to ImpliedProbability populated from
// the snapshot's depth vector length in PricingEngine.
```

### Pattern 3: Staleness Computation (FIX-03)

**What:** Compute `is_stale` from exchange_timestamp using the same pattern as Polymarket and Deribit normalizers.
**When to use:** Every venue processor that has exchange timestamps should compute staleness.

**Reference implementation (Polymarket normalizer, polymarket/normalize.rs:134-139):**
```rust
let exchange_data_stale = exchange_ts
    .map(|ts| is_exchange_data_stale(ts, self.staleness_threshold_ms))
    .unwrap_or(false);
let is_stale = exchange_data_stale;
```

**Kalshi fix (kalshi/normalize.rs:272):**
```rust
// Current: let is_stale = false;
// Fixed:
let is_stale = exchange_ts_ms
    .map(|ts| {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let age_ms = (now_ms - ts).max(0) as u64;
        age_ms > self.staleness_threshold_ms
    })
    .unwrap_or(false);
```

### Anti-Patterns to Avoid

- **Mixing tech debt fixes with feature changes in the same commit:** Each FIX should be its own commit for clean bisectability (per v1.3 decision: "Tech debt sweep in separate final phase for clean bisectability").
- **Breaking existing test expectations without updating tests:** The golden test (`tests/schema_golden_test.rs`) checks for `iv_spread` field presence. Any change to `ImpliedProbability` struct must update all tests that construct it.
- **Adding config fields without `#[serde(default)]`:** Breaking TOML backward compatibility. All new config fields must have defaults.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Staleness computation | Custom age check | Copy Polymarket's `is_exchange_data_stale()` pattern | Consistent with existing codebase |
| Config field defaults | Manual if-else | `#[serde(default = "fn_name")]` | Rust/serde idiomatic |
| IV spread propagation | Recompute in signal engine | Add field to ImpliedProbability, set in PricingEngine | Single source of truth |

**Key insight:** All three fixes involve wiring existing data to existing outputs. No new computation logic is needed.

## Common Pitfalls

### Pitfall 1: Breaking ImpliedProbability Construction Sites

**What goes wrong:** Adding a field to `ImpliedProbability` causes compilation errors everywhere it is constructed.
**Why it happens:** `ImpliedProbability` is constructed in `pricing/engine.rs` (2 places: normal path and near-expiry path), plus test files (`pricing/engine.rs` tests, `signal/engine.rs` tests, `signal/logger.rs` tests, `schema_golden_test.rs`).
**How to avoid:** Search for all construction sites before adding the field. Set the new field to `0.0` in the near-expiry path (where no IV solver runs) and test constructors.
**Warning signs:** Compiler errors listing many files after adding the field.

### Pitfall 2: Kalshi is_stale Affects SpreadEngine and CrossAssetEngine

**What goes wrong:** Once Kalshi `is_stale` is correctly computed, the `SpreadEngine.passes_staleness_gate()` will reject Kalshi snapshots that were previously accepted.
**Why it happens:** `SpreadEngine` checks `kalshi.is_stale` at line 424 of `spread/engine.rs`. Currently this is always `false`, so it never triggers. Once FIX-03 is applied, stale Kalshi data will be correctly rejected, potentially reducing spread computation volume.
**How to avoid:** This is desired behavior. Document the expected behavior change. Verify that the staleness threshold is reasonable (currently 5000ms in venues.toml).
**Warning signs:** Spread computation volume drops after deploying FIX-03 (expected if Kalshi data was occasionally stale).

### Pitfall 3: Book Depth Channel Change Requires Reconnect

**What goes wrong:** Changing `book_depth_levels` in config does not take effect until the Deribit connection reconnects, because the channel name is baked into the subscription at connect time.
**Why it happens:** `build_subscription_channels()` is called during subscription setup. It constructs channel names like `book.{inst}.none.{depth}.100ms`. The depth is fixed for the connection lifetime.
**How to avoid:** This is expected behavior. The config is read at startup / reconnect. Document that changing depth requires restart or config reload triggering reconnect.
**Warning signs:** None -- this is by design.

### Pitfall 4: iv_spread vs ConfidenceComponents.iv_spread Name Collision

**What goes wrong:** Confusion between `ArbSignal.iv_spread` (IV bid-ask spread value) and `ConfidenceComponents.iv_spread` (normalized 0-1 score derived from IV spread).
**Why it happens:** Both use the same field name for different semantics.
**How to avoid:** In the fix, `ArbSignal.iv_spread` should carry the raw IV spread (ask_iv - bid_iv) in vol points, same as `SmilePoint.iv_spread`. The confidence component remains a normalized score.
**Warning signs:** Using the confidence score (0.0-1.0) where the raw spread (e.g., 0.05 vol points) is expected or vice versa.

### Pitfall 5: Near-Expiry Path Missing iv_spread

**What goes wrong:** The near-expiry code path in `PricingEngine.process_near_expiry()` does not run the IV solver, so there is no bid_iv/ask_iv to compute iv_spread from.
**Why it happens:** Near-expiry uses intrinsic pricing, bypassing the IV solver entirely.
**How to avoid:** Set `iv_spread = 0.0` in the near-expiry path (same as other solver-dependent fields like `solver_meta: None`).
**Warning signs:** N/A -- `0.0` is correct for near-expiry (no IV spread exists).

## Code Examples

### FIX-01: IV Spread Propagation

**Step 1: Add field to ImpliedProbability (src/pricing/types.rs)**
```rust
pub struct ImpliedProbability {
    // ... existing fields (after near_expiry) ...
    /// IV bid-ask spread (ask_iv - bid_iv) from IV solver.
    /// Zero for near-expiry intrinsic pricing (no IV solver runs).
    pub iv_spread: f64,
}
```

**Step 2: Populate in normal pricing path (src/pricing/engine.rs, ~line 378)**
```rust
let implied_prob = ImpliedProbability {
    // ... existing fields ...
    near_expiry: false,
    iv_spread: iv_spread.max(0.0),  // iv_spread already computed at line 282
};
```

**Step 3: Populate in near-expiry path (src/pricing/engine.rs, ~line 481)**
```rust
let implied_prob = ImpliedProbability {
    // ... existing fields ...
    near_expiry: true,
    iv_spread: 0.0,  // No IV solver in near-expiry path
};
```

**Step 4: Read in CrossAssetEngine (src/signal/engine.rs, ~line 499)**
```rust
// Replace the entire iv_spread block with:
let iv_spread = prob.iv_spread;
```

### FIX-02: Book Depth from Config

**Step 1: Add field to DeribitConfig (src/config/venues.rs)**
```rust
pub struct DeribitConfig {
    // ... existing fields ...
    /// Number of book depth levels for grouped book subscription.
    /// Valid Deribit values: 1, 10, 20. Default: 20.
    #[serde(default = "default_book_depth_levels")]
    pub book_depth_levels: u32,
}

fn default_book_depth_levels() -> u32 {
    20
}
```

**Step 2: Parameterize channel construction (src/feed/deribit/channels.rs)**
```rust
pub fn build_subscription_channels(instruments: &[String], book_depth_levels: u32) -> Vec<String> {
    let mut channels = Vec::with_capacity(instruments.len() * 3 + 1);
    for inst in instruments {
        channels.push(format!("book.{}.none.{}.100ms", inst, book_depth_levels));
        channels.push(format!("ticker.{}.raw", inst));
        channels.push(format!("trades.{}.raw", inst));
    }
    channels.push("deribit_price_index.btc_usd".to_string());
    channels
}
```

**Step 3: Options leg depth in ArbSignal (src/signal/engine.rs)**

Option A (simpler): Use the cached `ImpliedProbability` to derive depth. This requires adding a `book_depth_levels` field to `ImpliedProbability` (populated from `snapshot.depth_bids.len() + snapshot.depth_asks.len()` in PricingEngine, or just the bid side count).

Option B (direct config): Pass `book_depth_levels` from config into `CrossAssetEngine` and use it directly.

Recommendation: Option A -- use the actual Deribit snapshot's depth vector length. This is the real number of levels the engine received. Add `options_book_depth: usize` to `ImpliedProbability`, populate from `snapshot.depth_bids.len()` in PricingEngine, read it in CrossAssetEngine.

### FIX-03: Kalshi Staleness

**Replace line 272 in src/feed/kalshi/normalize.rs:**
```rust
// Before:
let is_stale = false;

// After:
let is_stale = exchange_ts_ms
    .map(|ts| {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let age_ms = (now_ms - ts).max(0) as u64;
        age_ms > self.staleness_threshold_ms
    })
    .unwrap_or(false);

if is_stale {
    tracing::warn!(
        market = %market_ticker,
        "Kalshi exchange data stale -- marking is_stale=true"
    );
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| iv_spread always 0.0 | Propagate from IV solver | This phase | Confidence scoring and signal metadata reflect actual IV liquidity |
| book_depth_levels hardcoded 20/0 | Config-driven + actual depth | This phase | Operators can tune Deribit book depth; signal metadata shows real depth |
| Kalshi is_stale always false | Computed from exchange_timestamp | This phase | SpreadEngine correctly rejects stale Kalshi data |

**Deprecated/outdated:**
- None. These are fixing existing bugs, not replacing deprecated APIs.

## Open Questions

1. **FIX-02 Sub-issue B: How to propagate options book depth to ArbSignal?**
   - What we know: The options leg `book_depth_levels` is hardcoded to `0` in CrossAssetEngine. The actual depth is available in the MarketSnapshot that PricingEngine processes, but only `ImpliedProbability` (not the full snapshot) reaches CrossAssetEngine.
   - What's unclear: Whether to add a field to `ImpliedProbability` (adds to struct size for every probability) or to pass the depth count separately.
   - Recommendation: Add `options_book_depth: usize` to `ImpliedProbability`. The struct already has 16+ fields; one more `usize` is negligible. This keeps the data collocated with the probability it describes.

2. **FIX-02: Should `build_subscription_channels` signature change?**
   - What we know: The function currently takes only `&[String]`. Adding `book_depth_levels` parameter changes the API.
   - What's unclear: How many call sites use this function.
   - Recommendation: Add the parameter. All callers have access to `DeribitConfig`. Search for `build_subscription_channels` callers and update them.

## Sources

### Primary (HIGH confidence)

All findings are from direct source code analysis of the project codebase:

- `src/signal/engine.rs:499-512` -- iv_spread hardcoded to 0.0 (FIX-01 bug location)
- `src/pricing/engine.rs:282` -- iv_spread already computed as `ask_iv - bid_iv` (FIX-01 data source)
- `src/pricing/types.rs` -- `ImpliedProbability` struct (FIX-01 carrier type)
- `src/feed/deribit/channels.rs:121` -- book depth hardcoded to 20 (FIX-02a)
- `src/signal/engine.rs:560` -- options leg book_depth hardcoded to 0 (FIX-02b)
- `src/config/venues.rs` -- `DeribitConfig` struct (FIX-02 config location)
- `src/feed/kalshi/normalize.rs:272` -- `is_stale = false` (FIX-03 bug location)
- `src/feed/kalshi/normalize.rs:255-270` -- exchange_ts_ms already computed (FIX-03 data source)
- `src/feed/polymarket/normalize.rs:134-139` -- reference staleness pattern (FIX-03 template)
- `src/feed/deribit/normalize.rs:497-514` -- Deribit staleness pattern (FIX-03 template)

### Secondary (MEDIUM confidence)

- `.planning/research/FEATURES.md` -- Tech debt inventory with 15 items
- `.planning/research/PITFALLS.md` -- Pitfall 8: Tech debt cleanup risks
- `.planning/research/STACK.md` -- Confirms zero new dependencies needed

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- No new dependencies, all code changes in existing modules
- Architecture: HIGH -- All three fixes follow existing patterns already present in the codebase
- Pitfalls: HIGH -- Comprehensive analysis of construction sites, downstream effects, and test impacts

**Research date:** 2026-02-27
**Valid until:** Indefinite (internal codebase fixes, not dependent on external library versions)
