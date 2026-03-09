# Phase 41: Signal Engine Generalization - Research

**Researched:** 2026-03-09
**Domain:** Rust signal engine refactoring (CrossAssetEngine venue hardcoding removal)
**Confidence:** HIGH

## Summary

Phase 41 removes hardcoded venue references from CrossAssetEngine so it works with any options venue (Deribit or Derive) paired with any single prediction market (Polymarket alone, without requiring Kalshi). The changes are surgical -- three specific locations in `src/signal/engine.rs` plus adding a `source_venue` field to `ImpliedProbability` in `src/pricing/types.rs`.

The core issue is that `ImpliedProbability` (produced by PricingEngine) does not carry which venue it came from, even though PricingEngine already processes both Deribit and Derive snapshots. CrossAssetEngine then assumes all probabilities are from Deribit when looking up event mappings and building signal output. Additionally, the prediction market snapshot handler hardcodes the list of acceptable prediction venues.

**Primary recommendation:** Add a `source_venue: Venue` field to `ImpliedProbability`, populate it in PricingEngine from the incoming snapshot's venue, and use it in CrossAssetEngine for registry lookup and signal attribution. For SIG-03, make the prediction venue iteration dynamic based on what data exists in the cache.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SIG-01 | ImpliedProbability struct includes source venue field (Deribit or Derive) instead of hardcoded Deribit | Add `source_venue: Venue` to ImpliedProbability, populate in PricingEngine (2 construction sites), use in CrossAssetEngine |
| SIG-02 | CrossAssetEngine generates ArbSignals using implied probabilities from any options venue (not just Deribit) | Replace `Venue::Deribit` at line 251 with `prob.source_venue`, replace `Venue::Deribit` at line 543 with `prob.source_venue` |
| SIG-03 | CrossAssetEngine generates signals with a single prediction market venue (Polymarket alone, without requiring Kalshi) | Change line 273 loop to dynamically iterate only prediction venues that have cached data, keeping the venue filter at line 292 |
</phase_requirements>

## Architecture Patterns

### Current Data Flow

```
MarketSnapshot (Deribit/Derive) -> PricingEngine -> ImpliedProbability (no venue!) -> CrossAssetEngine
MarketSnapshot (Polymarket/Kalshi) -> fan-out -> CrossAssetEngine
```

### Target Data Flow

```
MarketSnapshot (Deribit/Derive) -> PricingEngine -> ImpliedProbability (source_venue=Deribit|Derive) -> CrossAssetEngine
MarketSnapshot (Polymarket/Kalshi) -> fan-out -> CrossAssetEngine
```

### Change Locations

All changes are in 3 files:

**1. `src/pricing/types.rs` -- ImpliedProbability struct (SIG-01)**

Add field:
```rust
/// Source options venue that produced this probability.
pub source_venue: Venue,
```

This requires importing `Venue` in `src/pricing/types.rs` (add `use crate::types::Venue;`).

**2. `src/pricing/engine.rs` -- PricingEngine construction sites (SIG-01)**

Two places where `ImpliedProbability { ... }` is constructed:
- Line ~389 (normal pricing path): add `source_venue: snapshot.venue,`
- Line ~494 (near-expiry path): add `source_venue: snapshot.venue,`

The `snapshot` variable is the incoming `MarketSnapshot` which already carries `.venue` (Deribit or Derive).

**3. `src/signal/engine.rs` -- CrossAssetEngine (SIG-02 + SIG-03)**

Three specific changes:

| Line | Current Code | New Code | Requirement |
|------|-------------|----------|-------------|
| 251 | `Venue::Deribit` | `prob.source_venue` | SIG-02 |
| 256-258 | `"unmapped Deribit instrument"` | `"unmapped options instrument"` (log text) | SIG-02 |
| 543 | `venue: Venue::Deribit` | `venue: prob.source_venue` | SIG-02 |
| 273 | `for venue in [Venue::Polymarket, Venue::Kalshi]` | iterate over prediction venues that have cached data | SIG-03 |

For SIG-03, the change at line 273 needs to iterate only over venues for which `latest_pred` has data for this event_id. The current code tries both Polymarket and Kalshi unconditionally. A clean approach:

```rust
// 4. Try spread computation against each cached prediction market venue
let pred_venues: Vec<Venue> = self.latest_pred.keys()
    .filter(|(eid, _)| eid == &event_id)
    .map(|(_, v)| *v)
    .collect();
for venue in pred_venues {
    self.compute_and_emit(&event_id, venue, signal_tx).await;
}
```

This is both more general (works with any prediction venue) and solves SIG-03 (no longer requires both Polymarket AND Kalshi to have data -- works with whichever venues have data).

The filter at line 292 (`if snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi`) should remain as-is -- it correctly gates which incoming snapshots are treated as prediction market data. This is a categorization concern (which venues ARE prediction markets), not a "require all" concern.

### Config Consideration: `deribit_taker_fee_rate`

The config field `deribit_taker_fee_rate` at line 430 is used for options fee estimation. Currently there is no `derive_taker_fee_rate` config field. Two options:

- **Option A (minimal):** Rename to `options_taker_fee_rate` (Derive has similar fee structure). This is a config breaking change but only affects TOML files.
- **Option B (keep as-is):** Leave `deribit_taker_fee_rate` as the fee rate for ALL options venues since Derive fees are comparable. Document that this applies to all options venues.

**Recommendation:** Option B (keep as-is). Derive's taker fee is similar to Deribit's. Renaming config fields is unnecessary complexity for v1.7. The field name is cosmetic -- the math works for both venues. If venue-specific options fees are needed later, that's a future requirement.

### Test Updates

Tests in `src/signal/engine.rs` and `src/signal/types.rs` construct `ImpliedProbability` directly. These need the new `source_venue` field added:

- `make_implied_probability()` helper in engine tests: add `source_venue: Venue::Deribit`
- `make_signal()` helper in types tests: already uses `Venue::Deribit` for options_leg, no change needed

New test cases needed:
1. **Derive-sourced probability produces correctly attributed signal** -- ImpliedProbability with `source_venue: Venue::Derive` should produce ArbSignal with `options_leg.venue == Venue::Derive`
2. **Single prediction venue (Polymarket only)** -- Engine should produce signals when only Polymarket data exists, no Kalshi data
3. **Registry lookup uses source venue** -- Derive instrument lookups use `Venue::Derive`, not `Venue::Deribit`

### Anti-Patterns to Avoid

- **Adding venue-specific branches in CrossAssetEngine:** The goal is REMOVING venue specificity, not adding more `match venue { ... }` arms. The engine should be venue-agnostic for options.
- **Changing the prediction venue filter logic:** Line 292's filter is correct categorization -- it identifies which venues are prediction markets. Don't remove it; it prevents options snapshots from being processed as prediction data.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Venue categorization (options vs prediction) | Complex trait system | Keep existing if-check at line 292 | Simple and correct; only 4 venues total |
| Per-venue fee lookup | HashMap<Venue, FeeConfig> | Existing `deribit_taker_fee_rate` for all options | Derive fees are similar; unnecessary complexity |

## Common Pitfalls

### Pitfall 1: Breaking Serialization
**What goes wrong:** Adding `source_venue` to ImpliedProbability could break deserialization of existing JSONL logs.
**How to avoid:** ImpliedProbability is only serialized in signal logs (via ArbSignal, not directly). The ArbSignal already has venue info in `options_leg.venue` and `prediction_venue`. ImpliedProbability itself is only sent over in-memory channels, never deserialized from disk. No serialization concern.

### Pitfall 2: Forgetting Test Helpers
**What goes wrong:** Adding a field to ImpliedProbability causes compilation failures in every test that constructs one.
**How to avoid:** Search for all `ImpliedProbability {` construction sites. There are exactly 4: 2 in pricing/engine.rs (production), 1 in signal/engine.rs tests, and potentially others in integration tests.

### Pitfall 3: Registry Lookup Miss for Derive
**What goes wrong:** Using `prob.source_venue` for registry lookup but EventMapping doesn't have a Derive instrument configured.
**Why it happens:** Not all event mappings have Derive instruments in the TOML config yet.
**How to avoid:** The existing `lookup_by_instrument` already returns `None` for missing venue instruments, and the existing `None` handling at line 253-263 correctly skips unmapped instruments. No code change needed -- this is already handled.

### Pitfall 4: Stale `latest_prob` Cache Key
**What goes wrong:** `latest_prob` is keyed by `event_id: String` (line 41). If both Deribit and Derive produce probabilities for the same event, they overwrite each other.
**Impact:** This is actually fine for v1.7 -- an event should only have one active options source at a time. The latest probability wins. If multi-options-venue support is needed later (OPT-01), the key would need to become `(event_id, options_venue)`.
**How to avoid:** Document this as a known limitation. No code change needed for v1.7.

## Code Examples

### SIG-01: ImpliedProbability with source_venue

```rust
// src/pricing/types.rs - add after existing fields
/// Source options venue that produced this probability.
pub source_venue: Venue,
```

### SIG-02: CrossAssetEngine handle_probability fix

```rust
// src/signal/engine.rs line ~250 - use source venue from probability
let mapping = match reg.lookup_by_instrument(
    prob.source_venue,  // was: Venue::Deribit
    &prob.instrument_id.to_string(),
) {
```

```rust
// src/signal/engine.rs line ~543 - use source venue in signal
options_leg: LegInfo {
    venue: prob.source_venue,  // was: Venue::Deribit
    instrument_id: prob.instrument_id.to_string(),
```

### SIG-03: Dynamic prediction venue iteration

```rust
// src/signal/engine.rs line ~273 - iterate cached prediction venues
let pred_venues: Vec<Venue> = self.latest_pred.keys()
    .filter(|(eid, _)| eid == &event_id)
    .map(|(_, v)| *v)
    .collect();
for venue in pred_venues {
    self.compute_and_emit(&event_id, venue, signal_tx).await;
}
```

## Open Questions

1. **Should `deribit_taker_fee_rate` be renamed to `options_taker_fee_rate`?**
   - What we know: Derive has similar fee structure (~0.03% taker). The field is only used in CrossAssetEngine.
   - Recommendation: Keep as-is for v1.7. Cosmetic rename can happen later if per-venue fees diverge.

## Sources

### Primary (HIGH confidence)
- Direct source code inspection: `src/signal/engine.rs`, `src/pricing/types.rs`, `src/pricing/engine.rs`, `src/events/registry.rs`, `src/config/events.rs`
- All findings verified by reading the actual codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - pure Rust refactoring, no new dependencies
- Architecture: HIGH - surgical changes to 3 files, clear data flow
- Pitfalls: HIGH - all identified via source code analysis

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable; internal refactoring, no external dependencies)
