# Architecture Patterns

**Domain:** Polymarket connectivity fix and spread/signal engine generalization
**Researched:** 2026-03-09
**Confidence:** HIGH (direct source code analysis, no external dependencies)

## Current Architecture (Baseline)

```text
[DeribitSupervisor]      --RawMessage-->  [DeribitProcessor]      --+
[PolymarketSupervisor]   --RawMessage-->  [PolymarketProcessor]  --+--> fan-in mpsc --> [SnapshotFanOut]
[KalshiSupervisor]       --RawMessage-->  [KalshiProcessor]      --+         |
[DeriveSupervisor]       --RawMessage-->  [DeriveProcessor]      --+         |
                                                                             |
                    +--------------------------------------------------------+
                    |                          |                              |
              [SpreadEngine]            [PricingEngine]              [CrossAssetEngine]
          (Poly+Kalshi pairs)        (Deribit+Derive opts)       (ImpliedProb + PredMkt)
                    |                          |                              |
              [PaperTradeTracker]     --ImpliedProbability-->          [ArbSignal consumer]
```

The fan-out task in main.rs (line 385) clones each MarketSnapshot to three receivers:
1. `spread_snap_tx` -> SpreadEngine (blocking send, primary pipeline)
2. `pricing_snap_tx` -> PricingEngine (try_send, best-effort)
3. `signal_pred_snap_tx` -> CrossAssetEngine (try_send, best-effort)

PricingEngine filters to Deribit+Derive snapshots (line 146), processes options pricing, and emits ImpliedProbability to CrossAssetEngine via a separate probability channel.

### Key Hardcoding Points (What Must Change)

**1. SpreadEngine (`spread/engine.rs` line 228):**
```rust
if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() {
    return; // Deribit-only or single-venue -- skip
}
```
Requires BOTH Polymarket AND Kalshi. A mapping with only Polymarket + Deribit/Derive (no Kalshi) is silently skipped. The SpreadEngine computes prediction-vs-prediction spreads only (Poly vs Kalshi), not prediction-vs-options.

**2. SpreadEngine patterns (`spread/patterns.rs`):**
`SpreadPattern` enum has 4 variants all hardcoded to Polymarket-vs-Kalshi pairs. `compute_gross_spread()` takes `poly: &MarketSnapshot` and `kalshi: &MarketSnapshot` as named parameters.

**3. CrossAssetEngine (`signal/engine.rs` line 251):**
```rust
let mapping = match reg.lookup_by_instrument(
    Venue::Deribit,
    &prob.instrument_id.to_string(),
)
```
Hardcodes `Venue::Deribit` for probability instrument lookup. Will not find Derive-sourced instruments.

**4. CrossAssetEngine (`signal/engine.rs` line 543):**
```rust
options_leg: LegInfo {
    venue: Venue::Deribit,
```
Hardcodes Deribit as the options venue in ArbSignal output, even when probability came from Derive.

**5. ImpliedProbability has no source venue field:**
The struct carries `instrument_id` but no `venue` field. CrossAssetEngine cannot determine whether the probability came from Deribit or Derive without parsing the instrument name.

**6. CrossAssetEngine prediction venue filter (`signal/engine.rs` line 292):**
```rust
if snap.venue != Venue::Polymarket && snap.venue != Venue::Kalshi {
    return;
}
```
This is correct behavior -- only prediction market snapshots should be processed here. No change needed.

## Recommended Architecture Changes

### Change 1: Add `source_venue` to ImpliedProbability

**Component:** `pricing/types.rs`
**Type:** Data model addition

Add `pub source_venue: Venue` to `ImpliedProbability`. PricingEngine already knows the venue from the incoming MarketSnapshot (line 146 filters by venue). Pass it through to the output struct.

```rust
pub struct ImpliedProbability {
    pub source_venue: Venue,  // NEW: which options venue produced this
    pub instrument_id: InstrumentId,
    // ... rest unchanged
}
```

**Impact radius:** PricingEngine emission site, CrossAssetEngine consumption site, test helpers that construct ImpliedProbability (3 in signal/engine.rs tests). No serialization concern -- ImpliedProbability is not persisted to JSONL (ArbSignal is, and it already has `prediction_venue`).

### Change 2: Generalize CrossAssetEngine venue lookup

**Component:** `signal/engine.rs`
**Type:** 2-line fix + comment updates

Replace hardcoded `Venue::Deribit` with `prob.source_venue` at two sites:

```rust
// Line 251: registry lookup
let mapping = match reg.lookup_by_instrument(
    prob.source_venue,  // was: Venue::Deribit
    &prob.instrument_id.to_string(),
)

// Line 543: options leg construction
options_leg: LegInfo {
    venue: prob.source_venue,  // was: Venue::Deribit
```

Also update the `deribit_taker_fee_rate` config field. Currently it is applied generically to all options venues. Two options:
- (A) Rename to `options_taker_fee_rate` (config rename, backward-incompatible with existing TOML)
- (B) Add a separate `derive_taker_fee_rate` and dispatch on venue
- **Recommendation:** (A) Rename. Derive and Deribit have the same fee structure for this use case. A single `options_taker_fee_rate` config field is cleaner. Accept the TOML migration.

### Change 3: SpreadEngine gate relaxation

**Component:** `spread/engine.rs`
**Type:** Logic change at line 228

The SpreadEngine's purpose is prediction-market-vs-prediction-market arbitrage. It should process events that have at least two prediction market venues, not specifically Polymarket AND Kalshi.

**Before:**
```rust
if mapping.venues.polymarket.is_none() || mapping.venues.kalshi.is_none() {
    return;
}
```

**After:**
```rust
let pred_venue_count = [
    mapping.venues.polymarket.is_some(),
    mapping.venues.kalshi.is_some(),
].iter().filter(|&&v| v).count();
if pred_venue_count < 2 {
    return; // Need at least 2 prediction markets for cross-venue spread
}
```

This is currently a no-op (Polymarket and Kalshi are the only prediction markets), but it makes the intent clear and is forward-compatible if a third prediction market is added.

**Important clarification:** The SpreadEngine does NOT need to support single-prediction-market-vs-options. That is CrossAssetEngine's job, and CrossAssetEngine already handles it correctly. The v1.7 goal of "generalize spread engine beyond Polymarket+Kalshi hardcoding to support single prediction market vs options-implied probability" is actually about ensuring the system produces cross-asset signals when only ONE prediction market has data. That functionality lives in CrossAssetEngine, not SpreadEngine. SpreadEngine only needs the gate relaxation.

### Change 4: Polymarket WebSocket connectivity fix

**Component:** `feed/polymarket/client.rs`
**Type:** Investigation + likely config/code changes

**Current URL:** `wss://ws-subscriptions-clob.polymarket.com/ws/market`

**Investigation approach (Phase 1 of build):**
1. SSH into EC2, test connectivity: `curl -v https://ws-subscriptions-clob.polymarket.com`
2. Test WebSocket with a tool: `websocat wss://ws-subscriptions-clob.polymarket.com/ws/market`
3. Check the connection failure layer:
   - TCP level (can't connect) -> firewall/geo-block
   - TLS level (handshake fails) -> certificate/SNI issue
   - HTTP upgrade level (403/429) -> Cloudflare blocking datacenter IPs
   - Application level (connects but no data) -> subscription issue

**Likely causes (ordered by probability):**
1. **Cloudflare datacenter IP blocking:** Polymarket uses Cloudflare. Datacenter IPs (AWS) are often fingerprinted and treated differently than residential IPs. May require specific headers (User-Agent, Origin).
2. **WebSocket URL change:** Polymarket has changed their CLOB API URLs before. The `ws-subscriptions-clob` subdomain may have migrated.
3. **Token ID staleness:** If token IDs in events.toml have changed or expired, subscriptions return no data even though the connection succeeds.
4. **Security group outbound:** Less likely since other venues work over WSS/443.

**Possible fixes:**
- Add Cloudflare-friendly headers (User-Agent, Origin) to the WS handshake
- Update WebSocket URL if it has changed
- Add a SOCKS proxy or residential proxy if datacenter IPs are blocked (complexity increase)
- Use `tokio-tungstenite` `connect_async_with_config` to set custom headers

### Change 5: No changes needed to PricingEngine (beyond Change 1)

PricingEngine already handles both Deribit and Derive correctly:
- Line 146: Filters to `Venue::Deribit` and `Venue::Derive` only
- Line 163-165: Venue-specific instrument parsing (`parse_deribit_instrument` vs `parse_derive_instrument`)
- Line 237: Venue-gated price conversion (BTC-inverse for Deribit, USD pass-through for Derive)

The only addition is populating the new `source_venue` field on ImpliedProbability output.

## Component Boundaries

| Component | Responsibility | Change Type | Change Description |
|-----------|---------------|-------------|-------------------|
| `ImpliedProbability` | Data transfer struct | **MODIFIED** | Add `source_venue: Venue` field |
| `PricingEngine` | IV solving, probability extraction | **MODIFIED** | Populate `source_venue` from snapshot |
| `CrossAssetEngine` | Cross-asset signal generation | **MODIFIED** | Use `prob.source_venue` instead of hardcoded `Venue::Deribit` (2 sites) |
| `SpreadEngine` | Prediction-vs-prediction spreads | **MODIFIED** | Relax Poly+Kalshi gate to "2+ prediction markets" |
| `PolymarketClient` | WS connection to Polymarket CLOB | **POSSIBLY MODIFIED** | May need headers/URL changes for EC2 connectivity |
| `PolymarketSupervisor` | Reconnection with backoff | **POSSIBLY MODIFIED** | May need enhanced error logging |
| `SignalGenerationConfig` | Signal engine TOML config | **MODIFIED** | Rename `deribit_taker_fee_rate` to `options_taker_fee_rate` |
| Pipeline fan-out | Snapshot distribution | **NO CHANGE** | Already sends all snapshots to all engines |
| `EventRegistry` | Cross-venue instrument mapping | **NO CHANGE** | Already supports all 4 venues |
| `ArbSignal` | Signal output struct | **NO CHANGE** | Already has `prediction_venue` field |
| `SpreadPattern` | Spread direction enum | **NO CHANGE** | Poly+Kalshi patterns are correct for prediction-vs-prediction |

## Data Flow (After Changes)

```text
[4 venue feeds] --> [fan-in] --> [SnapshotFanOut]
                                       |
     +-------- SpreadEngine -----------+----------- PricingEngine ----------+
     |   (Poly+Kalshi prediction       |        (Deribit+Derive options     |
     |    pairs, unchanged logic)      |         -> ImpliedProbability      |
     |                                 |         now with source_venue)     |
     |                                 |                |                   |
     |                                 |        [probability_tx]            |
     |                                 |                |                   |
     |                                 +---> CrossAssetEngine <-------------+
     |                                    (prob from ANY options venue       |
     |                                     + snap from ANY prediction venue) |
     |                                              |
     v                                              v
[PaperTradeTracker]                        [ArbSignal consumer]
```

Key change: CrossAssetEngine now correctly identifies Derive-sourced probabilities and pairs them with prediction market snapshots, producing ArbSignals with accurate `options_leg.venue` attribution.

## Patterns to Follow

### Pattern 1: Venue-agnostic registry lookup
**What:** Derive the venue from the data source rather than hardcoding.
**When:** Any component that looks up event mappings by instrument.
**Example:**
```rust
// Correct: venue comes from the data
let mapping = reg.lookup_by_instrument(prob.source_venue, &prob.instrument_id.to_string());

// Wrong: hardcoded venue assumption
let mapping = reg.lookup_by_instrument(Venue::Deribit, &prob.instrument_id.to_string());
```

### Pattern 2: Venue-gated fee computation
**What:** The CrossAssetEngine already handles venue-specific fees via `match pred_venue`. Extend this pattern for options fees if venue-specific rates are needed.
**When:** Computing costs for a trade on a specific venue.
**Example:** Already implemented for prediction venues (lines 415-427 of signal/engine.rs).

### Pattern 3: Additive struct fields with serde(default)
**What:** When adding fields to structs that may be deserialized from older data, use `#[serde(default)]` for backward compatibility.
**When:** ImpliedProbability is not serialized to JSONL, so this is not needed for Change 1. But if it were, `source_venue` should default to `Venue::Deribit`.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Full SpreadEngine generalization
**What:** Refactoring SpreadEngine to support N prediction venues with N-choose-2 pairings, renaming SpreadPattern variants to generic venue_a/venue_b.
**Why bad:** Over-engineering. Only Polymarket and Kalshi are prediction markets. The SpreadPattern enum (BuyPolyYesSellKalshiYes etc.) is domain-specific and readable. Generic patterns would obscure intent.
**Instead:** Keep SpreadEngine focused on Poly-vs-Kalshi. Cross-asset signals are CrossAssetEngine's job.

### Anti-Pattern 2: Merging SpreadEngine and CrossAssetEngine
**What:** Combining prediction-vs-prediction and prediction-vs-options into one engine.
**Why bad:** Different pairing logic (2 snapshots vs snapshot+probability), different cost models, different output types (SpreadResult vs ArbSignal). Merging creates a god object.
**Instead:** Keep them separate. They share utilities (book_walker, cost_model, rolling_stats, threshold) but have distinct responsibilities.

### Anti-Pattern 3: Proxy infrastructure for Polymarket
**What:** Setting up a residential proxy relay or VPN for Polymarket WebSocket access.
**Why bad:** Adds infrastructure cost, latency, single point of failure, and operational complexity. Should only be considered as a last resort after simpler fixes (headers, URL update) are exhausted.
**Instead:** First investigate the root cause. Most Cloudflare blocks can be bypassed with correct User-Agent/Origin headers.

## Suggested Build Order

### Phase 1: Polymarket connectivity diagnosis (no code changes)
- SSH into EC2, test WebSocket connectivity with diagnostic tools
- Determine if issue is geo-blocking, Cloudflare, headers, or network
- Independent of all other work; may reveal a blocker requiring a workaround

### Phase 2: Add `source_venue` to ImpliedProbability
- Add field to `ImpliedProbability` struct (1 line)
- Update `PricingEngine` to populate from incoming snapshot venue (1 line)
- Update test helpers that construct `ImpliedProbability` (3 sites)
- Pure additive change, no behavior change, all existing tests pass
- **No dependency on other phases**

### Phase 3: Generalize CrossAssetEngine
- Replace `Venue::Deribit` with `prob.source_venue` (2 sites in signal/engine.rs)
- Rename `deribit_taker_fee_rate` to `options_taker_fee_rate` in config
- Update TOML config key
- Write tests: Derive-sourced probability pairs with Polymarket snapshot
- **Depends on Phase 2** (needs `source_venue` field)

### Phase 4: SpreadEngine gate relaxation
- Replace Poly+Kalshi check with "2+ prediction markets" check
- Update comment from "Phase 8" to reflect current purpose
- No new tests needed (existing 4-pattern tests still pass)
- **Independent of Phases 2-3**

### Phase 5: Apply Polymarket connectivity fix
- Implement fix from Phase 1 diagnosis (headers, URL, proxy, etc.)
- May be config-only or may require client.rs changes
- **Depends on Phase 1 diagnosis**

### Phase 6: End-to-end production verification
- Deploy to EC2
- Verify Polymarket data flows through fan-in
- Verify CrossAssetEngine produces ArbSignals with Polymarket data
- Verify ArbSignals appear in JSONL logs with correct `prediction_venue` and `options_leg.venue`
- Verify Prometheus metrics: `arb_signals_emitted_total`, `arb_computations_total`
- Verify SpreadEngine produces SpreadResults when both Poly+Kalshi have data
- **Depends on all prior phases**

### Build Order Rationale
- Phase 1 is independent and may be a blocker (needs investigation time)
- Phase 2 is a pure data model change with no behavior change -- lowest risk
- Phase 3 is the core value delivery (Derive probabilities + any prediction market = signals)
- Phase 4 is defensive cleanup, can run in parallel with Phase 3
- Phase 5 may be trivial (config) or complex (proxy) -- isolating allows parallel work
- Phase 6 is integration verification, naturally last

## Sources

- Direct source code analysis:
  - `src/spread/engine.rs` -- SpreadEngine with Poly+Kalshi hardcoding (line 228)
  - `src/signal/engine.rs` -- CrossAssetEngine with Deribit hardcoding (lines 251, 292, 543)
  - `src/pricing/engine.rs` -- PricingEngine venue handling (lines 146, 163-165, 237)
  - `src/pricing/types.rs` -- ImpliedProbability struct (no venue field)
  - `src/spread/patterns.rs` -- SpreadPattern enum, compute_gross_spread
  - `src/signal/types.rs` -- ArbSignal, ArbDirection, CostBreakdown, LegInfo
  - `src/feed/pipeline.rs` -- pipeline wiring, fan-out task, forward_snapshots
  - `src/feed/polymarket/client.rs` -- WebSocket client, subscription message
  - `src/feed/polymarket/supervisor.rs` -- reconnection supervisor with backoff
  - `src/config/events.rs` -- EventMapping, EventVenues (supports all 4 venues)
  - `src/types/venue.rs` -- Venue enum (Deribit, Polymarket, Kalshi, Derive)
  - `src/main.rs` -- engine spawning (lines 363-799), channel wiring, fan-out
  - `config/venues.toml` -- Polymarket WS URL

---
*Architecture research for: v1.7 Prediction Market Signal Pipeline*
*Researched: 2026-03-09*
