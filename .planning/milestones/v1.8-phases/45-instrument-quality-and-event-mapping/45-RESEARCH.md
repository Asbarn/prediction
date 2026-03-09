# Phase 45: Instrument Quality and Event Mapping - Research

**Researched:** 2026-03-09
**Domain:** Near-the-money BTC instrument selection, cross-venue match validation, Polymarket bid-ask spread filtering
**Confidence:** HIGH

## Summary

Phase 45 populates the empty `events.toml` with quality instrument mappings and adds tooling to validate and filter them. The system currently has `events = []` in production -- no active mappings exist. Historical signals came from deep-OTM strikes ($105K when BTC was at $85K) where prediction market prices and options-implied probabilities measure fundamentally different economic bets.

BTC spot is currently around $68-69K (March 9, 2026). Polymarket's March 2026 slug (`what-price-will-bitcoin-hit-in-march-2026`) has contracts at strikes from $60K to $150K, with the near-the-money contracts being $65K (dip, 69% prob), $75K (reach, 42.5% prob), and $80K (reach, 21% prob). Deep OTM contracts like $100K+ show bestBid of $0.006-$0.01 with tiny spreads that represent phantom liquidity. Deribit has BTC options at all standard strikes ($65K, $70K, $75K, $80K) across weekly, monthly, and quarterly expiries.

The phase requires three deliverables: (1) populate events.toml with at least 3 near-the-money BTC instrument mappings via the existing discovery pipeline, (2) build a `match-audit` CLI that validates strike/expiry/direction alignment across venues, and (3) add bid-ask spread filtering to the Polymarket discovery pipeline so deep OTM phantom liquidity contracts are skipped.

**Primary recommendation:** Add `bestBid`/`bestAsk`/`spread` fields to `PolymarketMarketInfo`, add a configurable `max_polymarket_spread` threshold to `DiscoveryConfig`, filter in `discover_polymarket_structured`, then build the `match-audit` CLI following the established binary pattern, and finally manually populate or approve near-the-money mappings in `events.toml`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INST-01 | Production events.toml contains active near-the-money BTC instrument mappings with real liquidity | Live Gamma API confirms Polymarket has contracts at $65K/$75K/$80K strikes with BTC at ~$68K. Deribit has matching strikes. Discovery pipeline (`find_cross_venue_candidates_fuzzy`) already does cross-venue matching including Polymarket. Need to run discovery, review candidates, and approve 3+ mappings with strikes within 10% of spot. |
| INST-02 | Instrument match-audit CLI validates that paired contracts represent the same economic bet (strike, expiry, direction alignment) | New `bin/match-audit.rs` CLI reads `events.toml`, loads all active mappings, and validates: (a) strike prices match across venues, (b) expiry dates within configured tolerance, (c) direction (above/below) consistent, (d) settlement metadata compatible. Follows exact pattern of existing `spread-analytics` and `signal-scoring` CLIs (clap derive, table/JSON output). |
| INST-03 | Discovery pipeline filters out deep OTM contracts where Polymarket bid-ask spread exceeds configurable threshold | Gamma API returns `bestBid`, `bestAsk`, and `spread` fields per market. `PolymarketMarketInfo` struct currently does NOT parse these fields. Add them, add `max_polymarket_spread` config to `DiscoveryConfig` (default 0.10 = 10 cents), filter in `discover_polymarket_structured` before returning instruments. Contracts at $100K+ with 0.5-1% prices and $0.001 spreads still pass -- the issue is that deep OTM contracts have prices near $0.01 meaning the "spread" is small in absolute terms but the market is illiquid. The better filter is a **minimum price threshold** (e.g., bestBid >= $0.02) combined with spread threshold. |
</phase_requirements>

## Standard Stack

### Core

No new dependencies. All work uses existing crate APIs.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5 | CLI argument parsing for match-audit | Already used by spread-analytics, signal-scoring |
| comfy-table | 7.x | Table output for match-audit | Already used by spread-analytics, signal-scoring |
| serde/serde_json | existing | JSON output mode + TOML parsing | Already used throughout |
| toml/toml_edit | existing | Reading events.toml for validation | Already used by config loader, toml_writer |
| rust_decimal | 1.40 | Strike price comparison | Already used by discovery, pricing |
| chrono | existing | Expiry date arithmetic | Already used by discovery, lifecycle |
| reqwest | existing | HTTP client for Gamma API (spread fields) | Already used by discovery |

### Supporting

No new supporting libraries needed.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| CLI binary for match-audit | Subcommand in main binary | Separate binary is the established pattern (spread-analytics, signal-scoring). Avoids bloating the main binary. |
| Absolute spread filter | Relative spread (spread/midpoint) filter | Absolute spread is simpler and more intuitive for prediction market contracts where prices are 0-1. A $0.10 absolute spread threshold works well. |

**Installation:**
```bash
# No new dependencies to install
```

## Architecture Patterns

### Affected Files

```
src/
  events/
    discovery.rs       # INST-03: Add spread fields to PolymarketMarketInfo,
                       #          filter in discover_polymarket_structured
  config/
    events.rs          # INST-03: Add max_polymarket_spread + min_polymarket_price
                       #          to DiscoveryConfig
  bin/
    match_audit.rs     # INST-02: New CLI binary
config/
  events.toml          # INST-01: Populate with near-the-money BTC mappings
Cargo.toml             # INST-02: Add [[bin]] entry for match-audit
```

### Pattern 1: Polymarket Bid-Ask Spread Filtering (INST-03)

**What:** Parse `bestBid`, `bestAsk`, `spread` from Gamma API response. Filter out contracts where spread exceeds threshold or price is below minimum.

**When to use:** During `discover_polymarket_structured` before adding to instrument list.

**Implementation approach:**

1. Add fields to `PolymarketMarketInfo`:
```rust
pub struct PolymarketMarketInfo {
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    pub question: String,
    #[serde(rename = "endDateIso")]
    pub end_date_iso: Option<String>,
    pub active: bool,
    pub closed: bool,
    #[serde(default)]
    pub tokens: Vec<PolymarketToken>,
    pub category: Option<String>,
    // NEW: price quality fields from Gamma API
    #[serde(rename = "bestBid", default)]
    pub best_bid: Option<f64>,
    #[serde(rename = "bestAsk", default)]
    pub best_ask: Option<f64>,
    #[serde(default)]
    pub spread: Option<f64>,
}
```

2. Add config fields to `DiscoveryConfig`:
```rust
/// Maximum Polymarket bid-ask spread for discovery inclusion.
/// Markets with spread exceeding this threshold are filtered out.
/// Default 0.10 (10 cents on a $0-$1 contract).
#[serde(default = "default_max_polymarket_spread")]
pub max_polymarket_spread: f64,

/// Minimum Polymarket best bid price for discovery inclusion.
/// Markets with best bid below this are filtered out as illiquid.
/// Default 0.02 ($0.02 = 2%).
#[serde(default = "default_min_polymarket_price")]
pub min_polymarket_price: f64,
```

3. Filter in `discover_polymarket_structured` loop:
```rust
// Filter: skip markets with excessive spread or illiquid prices
if let (Some(bid), Some(ask)) = (market.best_bid, market.best_ask) {
    if bid < min_polymarket_price {
        tracing::debug!(
            condition_id = %market.condition_id,
            best_bid = bid,
            threshold = min_polymarket_price,
            "skipping Polymarket market: best bid below minimum price"
        );
        metrics::counter!("polymarket_filtered_low_price").increment(1);
        continue;
    }
    if let Some(spread) = market.spread {
        if spread > max_polymarket_spread {
            tracing::debug!(
                condition_id = %market.condition_id,
                spread = spread,
                threshold = max_polymarket_spread,
                "skipping Polymarket market: spread exceeds threshold"
            );
            metrics::counter!("polymarket_filtered_wide_spread").increment(1);
            continue;
        }
    }
}
```

**Key insight:** The `spread` field from Gamma API is the absolute bid-ask spread. For March 2026 BTC contracts: near-the-money contracts ($65K-$80K) have spreads of $0.005-$0.02, while deep OTM contracts ($100K+) have spreads of $0.001-$0.002 but prices near $0.005-$0.01. The minimum price filter (`min_polymarket_price`) is more effective than spread alone at filtering phantom liquidity, because deep OTM contracts paradoxically have TIGHT spreads (both sides are near zero). The combined filter (price >= $0.02 AND spread <= $0.10) correctly passes near-the-money contracts while rejecting deep OTM.

### Pattern 2: Match-Audit CLI (INST-02)

**What:** CLI binary that loads `events.toml` and validates all active mappings.

**When to use:** Before approving new discovery candidates, or as ongoing validation.

**Structure follows established pattern:**
```rust
// src/bin/match_audit.rs
#[derive(Parser)]
#[command(name = "match-audit")]
#[command(about = "Validate instrument quality and cross-venue alignment")]
struct Cli {
    /// Config directory containing events.toml
    #[arg(long, default_value = "config")]
    config_dir: PathBuf,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Only show mappings with issues
    #[arg(long)]
    issues_only: bool,

    /// Expiry tolerance in days (default from discovery config)
    #[arg(long)]
    expiry_tolerance: Option<i64>,
}
```

**Validation checks per mapping:**
1. **Strike alignment:** All venues reference the same strike price (exact match required after normalization)
2. **Expiry alignment:** Expiry dates across venues within tolerance (default 7 days)
3. **Direction consistency:** All venues agree on above/below
4. **Venue coverage:** At least 2 venues present (Polymarket + options venue)
5. **Moneyness assessment:** Flag if strike is >10% from current spot (requires spot price input or fetch)
6. **Settlement metadata:** Check if settlement sources are configured

**Output format:**
```
Match Audit Report
==================
Events checked: 5
  Passed: 3
  Issues: 2

ID                      Strike  Dir    Venues  Expiry Spread  Issues
BTC-75000-2026-03-31    75000   above  PM+DB   0 days         OK
BTC-80000-2026-03-31    80000   above  PM+DB   0 days         OK
BTC-65000-2026-03-31    65000   below  PM+DB   0 days         OK
BTC-100000-2026-03-31   100000  above  PM+DB   0 days         WARN: >10% OTM
BTC-150000-2026-03-31   150000  above  PM+DB   0 days         WARN: >50% OTM
```

### Pattern 3: Event Mapping Population (INST-01)

**What:** Populate events.toml with near-the-money BTC mappings.

**Approach:** The discovery pipeline already runs in production and proposes candidates with `approved = false`. The task is to:
1. Ensure discovery runs and generates candidates for current near-the-money strikes
2. Review candidates (ideally with match-audit CLI)
3. Approve quality mappings by setting `approved = true`

**Example mapping for BTC-75000 (above):**
```toml
[[events]]
id = "BTC-75000-2026-03-31"
asset = "BTC"
strike = "75000"
direction = "above"
expiry = "2026-03-31"
approved = true
status = "active"

[events.venues.deribit]
instrument = "BTC-28MAR26-75000-C"

[events.venues.polymarket]
condition_id = "0xd32f73b7..."
token_id = "53871780..."

[events.venues.derive]
instrument = "BTC-20260328-75000-C"
```

**Target mappings (BTC at ~$68K, strikes within 10% = $61K-$75K):**

| Strike | Direction | Polymarket Prob | Deribit Strike | Quality |
|--------|-----------|----------------|----------------|---------|
| $65000 | below (dip) | 69% | BTC-*-65000-P | HIGH - mid-range probability, liquid |
| $75000 | above (reach) | 42.5% | BTC-*-75000-C | HIGH - mid-range probability, liquid |
| $80000 | above (reach) | 21% | BTC-*-80000-C | MEDIUM - slightly OTM but reasonable |

**Note on expiry alignment:** Polymarket March contracts end 2026-04-01. Deribit has 28MAR26 expiry (Friday settlement). This is a 4-day gap -- within the default 7-day tolerance but should be flagged by match-audit with MEDIUM confidence.

### Anti-Patterns to Avoid

- **Approving deep OTM instruments:** Strikes at $100K+ (47%+ OTM) produce the 192x probability gap identified in research. The match-audit CLI should flag these.
- **Relying on spread filter alone:** Deep OTM contracts have paradoxically tight absolute spreads (both sides near $0). The minimum price filter is essential.
- **Hard-coding mappings without discovery:** Manually writing events.toml entries is fragile. Let discovery propose candidates, then approve them. This ensures instrument IDs (condition_id, token_id) are correct.
- **Fetching spot price from external API during match-audit:** Keep the CLI simple and offline. Accept spot price as a `--spot` argument or read from the latest Deribit index ticker in market data.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cross-venue matching | Custom matching logic in match-audit | Reuse existing `MatchKey`/`FuzzyMatchKey` from `discovery.rs` | Already proven with extensive tests, handles expiry tolerance, sliding windows |
| TOML manipulation | String concatenation for events.toml | `toml_edit::DocumentMut` via existing `toml_writer.rs` | Preserves comments, formatting; battle-tested in lifecycle manager |
| CLI argument parsing | Manual arg parsing | `clap` derive macros | Established pattern in 3 existing binaries |
| Table output | Manual formatting | `comfy-table` | Established pattern in existing CLIs |
| Polymarket question parsing | Regex for structured extraction | Existing `parse_polymarket_question` | Already handles "reach", "hit", "dip to" patterns with tests |

## Common Pitfalls

### Pitfall 1: Deep OTM Phantom Liquidity

**What goes wrong:** Contracts at $0.005 bestBid show tight $0.001 spreads, passing naive spread filters. These represent phantom liquidity where no real trading occurs.
**Why it happens:** At extreme prices, both sides of the book are near zero. The absolute spread is tiny but the market is illiquid -- nobody is actively trading "Will BTC reach $150K by end of March" when spot is $68K.
**How to avoid:** Use minimum price threshold (`min_polymarket_price >= 0.02`) as the primary filter, spread threshold as secondary. Log filtered-out instruments with metrics counter for monitoring.
**Warning signs:** Polymarket bestBid < $0.02, probability ratio between venues > 10x.

### Pitfall 2: Expiry Mismatch Between Polymarket and Deribit

**What goes wrong:** Polymarket March contracts expire April 1 (end of month). Deribit monthly options expire last Friday of the month (March 28, 2026 at 08:00 UTC). This 4-day gap means the instruments settle at different times on different settlement sources.
**Why it happens:** Polymarket uses calendar month boundaries; Deribit uses exchange-standard Friday settlement. The system's fuzzy matching (7-day tolerance) correctly matches these, but the settlement time difference introduces basis risk.
**How to avoid:** Match-audit CLI should report expiry gap in days per mapping. Settlement metadata should note the difference. Mappings with >3 day gaps get MEDIUM confidence, >7 days get LOW.
**Warning signs:** ExpiryConfidence reported as LOW/MEDIUM in discovery logs.

### Pitfall 3: Stale Polymarket bestBid/bestAsk Data

**What goes wrong:** Gamma API prices may be cached or stale. A contract that looked liquid (bestBid $0.42) may have been drained.
**Why it happens:** Known issue (GitHub py-clob-client #180). Gamma API is not a real-time feed -- it's a snapshot API with caching.
**How to avoid:** Treat Gamma API spread data as a coarse filter, not a precision measurement. Conservative thresholds (min_price 0.02, max_spread 0.10) accommodate staleness. Real-time liquidity assessment happens downstream via the CLOB WebSocket feed.
**Warning signs:** Large divergence between Gamma API outcomePrices and CLOB WebSocket best bid/ask.

### Pitfall 4: Polymarket Token ID Misassignment

**What goes wrong:** Each Polymarket market has multiple tokens (Yes and No outcomes). Using the wrong token_id means subscribing to the wrong side of the contract.
**Why it happens:** The tokens array may have different ordering across markets. Current code takes the "Yes" outcome token, which is correct for "reach" (above) contracts but needs care for "dip to" (below) contracts.
**How to avoid:** Match-audit should verify token_id corresponds to the correct outcome for the mapped direction. For "above" direction, the Yes token is "BTC reaches $X" (correct). For "below" direction, the Yes token is "BTC dips to $X" (also correct, since the contract is "will it dip").

## Code Examples

### Loading events.toml for match-audit
```rust
// Reuse existing config loader
use prediction::config::load_events_config;

let events_path = config_dir.join("events.toml");
let events_config = load_events_config(&events_path)?;

for mapping in &events_config.events {
    if !mapping.approved || mapping.status != LifecycleStatus::Active {
        continue;
    }
    // Validate mapping...
}
```

### Moneyness calculation
```rust
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

fn moneyness_pct(strike: &str, spot: f64) -> Option<f64> {
    let strike_dec = Decimal::from_str_exact(strike).ok()?;
    let strike_f64 = strike_dec.to_f64()?;
    Some(((strike_f64 - spot) / spot).abs() * 100.0)
}

// BTC at $68K, strike $75K -> 10.3% OTM
// BTC at $68K, strike $100K -> 47% OTM
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Exact four-field matching only | Fuzzy matching with expiry tolerance | v1.2 (Phase 19) | Enables Polymarket (calendar month) to match Deribit (Friday expiry) |
| Polymarket excluded from auto-matching | Polymarket included in fuzzy cross-venue matching | v1.5 (Phase 33) | All three venues participate in candidate generation |
| No OTM filtering | None yet (this phase adds it) | v1.8 (Phase 45) | Will eliminate phantom liquidity noise |

## Open Questions

1. **Spot price source for moneyness check in match-audit**
   - What we know: Deribit index price is the most authoritative BTC spot source we have
   - What's unclear: Should match-audit fetch live price, or accept `--spot` argument?
   - Recommendation: Accept `--spot` argument for simplicity (offline tool). If not provided, warn that moneyness checks are skipped. Future enhancement could fetch from Deribit REST API.

2. **Polymarket "before" contracts vs monthly contracts**
   - What we know: Polymarket has both monthly slugs (`what-price-will-bitcoin-hit-in-march-2026`) and annual slugs (`what-price-will-bitcoin-hit-before-2027`). Annual contracts have much longer time horizons.
   - What's unclear: Are annual contracts matchable to Deribit quarterly options?
   - Recommendation: Stick with monthly Polymarket contracts for v1.8. The time horizon alignment is much better (weeks, not months).

3. **Derive availability for near-the-money strikes**
   - What we know: Derive discovery is implemented and runs in production
   - What's unclear: Whether Derive has the same strikes as Deribit at near-the-money levels
   - Recommendation: Let discovery handle it. If Derive has matching instruments, they will be included as venue enrichments. Not critical for INST-01 (Polymarket + Deribit is sufficient for 2+ venue requirement).

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis: `src/events/discovery.rs` (PolymarketMarketInfo struct, discover_polymarket_structured, find_cross_venue_candidates_fuzzy), `src/config/events.rs` (DiscoveryConfig), `src/events/lifecycle.rs` (poll_cycle), `src/events/toml_writer.rs` (CandidateMapping)
- Live Gamma API response: `https://gamma-api.polymarket.com/events?slug=what-price-will-bitcoin-hit-in-march-2026` -- confirmed fields: bestBid, bestAsk, spread, outcomePrices, tokens with token_id+outcome
- Live Deribit API: `https://www.deribit.com/api/v2/public/get_instruments?currency=BTC&kind=option` -- confirmed BTC options at $65K-$80K strikes
- v1.8 research archive: `.planning/research/SUMMARY.md`, `FEATURES.md`, `PITFALLS.md` -- instrument mismatch pitfall, near-the-money recommendation

### Secondary (MEDIUM confidence)
- [Polymarket Gamma API Overview](https://docs.polymarket.com/developers/gamma-markets-api/overview) -- confirms outcomePrices represent implied probabilities
- [Polymarket /book stale data issue #180](https://github.com/Polymarket/py-clob-client/issues/180) -- known Gamma API staleness
- BTC spot price ~$68-69K on March 9, 2026 -- from multiple news sources

### Tertiary (LOW confidence)
- Derive strike availability at near-the-money levels -- not verified via API, assumed similar to Deribit

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing patterns
- Architecture: HIGH - match-audit follows proven CLI pattern, Polymarket field additions are pure struct extension
- Pitfalls: HIGH - deep OTM filtering strategy verified against live Gamma API data, expiry mismatch documented with specific dates

**Research date:** 2026-03-09
**Valid until:** 2026-03-16 (Polymarket contract availability changes as expiry approaches; BTC spot price affects moneyness calculations)
