# Phase 33: Discovery and Matching - Research

**Researched:** 2026-03-06
**Domain:** Derive REST API instrument discovery, cross-venue matching, ContractLifecycleManager integration
**Confidence:** HIGH

## Summary

Phase 33 adds Derive venue discovery and cross-venue matching to the existing ContractLifecycleManager pipeline. The codebase already has a well-established pattern: `discover_deribit()`, `discover_kalshi()`, and `discover_polymarket_structured()` each fetch instruments from REST APIs and return `Vec<DiscoveredInstrument>`. Cross-venue matching uses `FuzzyMatchKey` (asset/strike/direction) with configurable expiry tolerance. The `CandidateVenues` struct already has `derive: Option<String>` and `build_candidate_table` in `toml_writer.rs` already writes Derive venue entries.

The key work is: (1) implementing `discover_derive()` following the exact Deribit discovery pattern, (2) updating `filter_new_candidates_fuzzy()` to populate `CandidateVenues.derive` instead of ignoring `Venue::Derive`, (3) adding `derive_poll_interval_secs` to `DiscoveryConfig`, and (4) wiring Derive polling into the lifecycle manager's `poll_cycle()`.

**Primary recommendation:** Follow the existing Deribit discovery pattern exactly. The Derive REST API uses POST to `https://api.lyra.finance/public/get_instruments` with JSON body (not GET with query params like Deribit). Response structure has `option_details.strike` (string), `option_details.expiry` (epoch seconds), `option_details.option_type` ("C"/"P"), and `instrument_name` in `BTC-YYYYMMDD-STRIKE-C/P` format.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DISC-01 | Derive REST-based instrument listing via `public/get_instruments` endpoint | Derive REST API is POST-based at `https://api.lyra.finance/public/get_instruments` with JSON body `{instrument_type: "option", currency: "BTC", expired: false}`. Response has `result` array with `instrument_name`, `is_active`, `option_details.strike`, `option_details.expiry`, `option_details.option_type`. Existing `discover_deribit()` pattern provides exact template. |
| DISC-02 | Cross-venue matching between Derive BTC options and Deribit/Polymarket instruments using existing FuzzyMatchKey | `FuzzyMatchKey` (asset/strike/direction) and `find_cross_venue_candidates_fuzzy()` already exist. Two `Venue::Derive => {}` stubs in `filter_new_candidates_fuzzy()` and `filter_new_candidates()` need to be updated to populate `derive` field. Derive uses YYYYMMDD dates vs Deribit DDMMMYY, but both normalize to `NaiveDate` so matching works automatically. |
| DISC-03 | Proposal writing for discovered Derive matches to events.toml (approved = false) | `CandidateVenues.derive: Option<String>` already exists. `build_candidate_table()` in `toml_writer.rs` already writes `[events.venues.derive] instrument = "..."`. No changes needed to TOML writer. |
| DISC-04 | Discovery integrated into ContractLifecycleManager periodic background pipeline | Lifecycle manager needs: `derive_poll_interval_secs` config field, `last_derive_poll` tracker in `poll_cycle()`, Derive discovery block following Deribit pattern (rate limiter, suspect detection, absence tracking), and Derive absence checking in step 1b. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| reqwest | (existing) | HTTP POST to Derive REST API | Already used for Deribit/Kalshi/Polymarket discovery |
| serde/serde_json | (existing) | Deserialize Derive REST response | Standard JSON handling |
| rust_decimal | (existing) | Parse string strikes to Decimal | Matches existing DiscoveredInstrument.strike type |
| chrono | (existing) | Parse epoch expiry to NaiveDate | Matches existing pattern |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | (existing) | Structured logging for discovery events | WARN for new proposals, INFO for counts |
| metrics | (existing) | Prometheus counters/gauges | lifecycle_discovery_polls, lifecycle_candidates_discovered |

No new dependencies needed. All libraries are already in use.

## Architecture Patterns

### Recommended Structure
No new files needed. Changes go into existing files:

```
src/
  events/
    discovery.rs       # Add discover_derive() + response structs
    lifecycle.rs       # Add Derive polling to poll_cycle()
  config/
    events.rs          # Add derive_poll_interval_secs to DiscoveryConfig
```

### Pattern 1: Venue Discovery Function
**What:** Each venue has a `discover_{venue}()` async function that takes an HTTP client, base URL, and rate limiter, returns `Vec<DiscoveredInstrument>`.
**When to use:** Always -- this is the established pattern for all 3 existing venues.
**Example (from discover_deribit):**
```rust
pub async fn discover_derive(
    client: &reqwest::Client,
    base_url: &str,       // "https://api.lyra.finance"
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    if let Some(limiter) = rate_limiter {
        limiter.wait().await;
    }
    let resp = client
        .post(format!("{}/public/get_instruments", base_url))
        .json(&serde_json::json!({
            "instrument_type": "option",
            "currency": "BTC",
            "expired": false,
        }))
        .send()
        .await?;
    let body: DeriveInstrumentsResponse = resp.json().await?;
    // ... parse result array into DiscoveredInstrument
}
```

### Pattern 2: Lifecycle Manager Venue Polling
**What:** Each venue gets its own `last_{venue}_poll` tracker, interval check, suspect detection, and absence tracking block in `poll_cycle()`.
**When to use:** Wiring Derive into the lifecycle manager.
**Key elements:**
- `last_derive_poll` Instant tracker (initialized to trigger immediately)
- `derive_polled` and `derive_suspect` boolean flags
- Identical structure to Deribit block (metrics counter, rate limiter lookup, suspect detection, previous poll count update)

### Pattern 3: FuzzyMatchKey Integration
**What:** `Venue::Derive` match arms in `filter_new_candidates_fuzzy()` and `filter_new_candidates()` must populate `derive` field instead of being no-ops.
**When to use:** Updating the two stub match arms.
**Example:**
```rust
Venue::Derive => derive = Some(inst.instrument_id.clone()),
```

### Anti-Patterns to Avoid
- **Separate Derive matching path:** Do NOT create a separate matching function for Derive. Use the existing `find_cross_venue_candidates_fuzzy()` -- Derive instruments go into the same `all_discovered` pool as all other venues.
- **GET request for Derive API:** Derive uses POST with JSON body, not GET with query params like Deribit.
- **Parsing instrument name for discovery fields:** Use structured API response fields (`option_details.strike`, `option_details.expiry`, `option_details.option_type`), not instrument name parsing. The instrument name parser already exists in `pricing/instrument.rs` but that is for downstream pricing, not discovery.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cross-venue matching | Custom Derive-specific matching | Existing `find_cross_venue_candidates_fuzzy()` | Already handles multi-venue grouping with expiry tolerance |
| TOML writing | Custom Derive TOML entries | Existing `build_candidate_table()` | Already handles `venues.derive` field |
| Instrument name format | Custom parser for discovery | Structured API fields from REST response | `option_details.strike`, `option_details.expiry`, `option_details.option_type` are authoritative |
| Rate limiting | Custom rate limiter | Existing `VenueRateLimiter` with `venue_rate_limiters.get(&Venue::Derive)` | Already configured per-venue |
| Absence tracking | Custom Derive expiry detection | Existing `AbsenceTracker` | Already handles all venues generically |

**Key insight:** Nearly all infrastructure exists. This phase is about removing `Venue::Derive => {}` stubs and adding a single discovery function + lifecycle wiring.

## Common Pitfalls

### Pitfall 1: Derive REST API is POST, not GET
**What goes wrong:** Using GET with query params like the Deribit pattern.
**Why it happens:** Deribit uses `GET /api/v2/public/get_instruments?currency=BTC&kind=option`, so copy-paste would use GET.
**How to avoid:** The probe test at `tests/derive_api_probe.rs:602` confirms: POST to `https://api.lyra.finance/public/get_instruments` with JSON body `{"instrument_type": "option", "currency": "BTC", "expired": false}`.
**Warning signs:** 405 Method Not Allowed or unexpected response.

### Pitfall 2: Derive strike is a string in option_details, not a float
**What goes wrong:** Using `as_f64()` to parse strike.
**Why it happens:** Deribit returns `strike: Option<f64>`, but Derive returns `option_details.strike` as a string (e.g., `"100000"`).
**How to avoid:** Use `Decimal::from_str()` for Derive strikes (consistent with the project decision from Phase 31: "DeriveBook uses Decimal::from_str, not f64").
**Warning signs:** Deserialization errors or precision loss.

### Pitfall 3: Derive expiry is seconds (not milliseconds)
**What goes wrong:** Using `DateTime::from_timestamp_millis()` like Deribit.
**Why it happens:** Deribit `expiration_timestamp` is milliseconds. Derive `option_details.expiry` appears to be epoch seconds based on probe test field name and values.
**How to avoid:** Verify in deserialization struct. Use `DateTime::from_timestamp()` for seconds or check if value > 10^12 to detect milliseconds.
**Warning signs:** Dates in year 52000+ (if seconds treated as millis) or 1970 (if millis treated as seconds).

### Pitfall 4: Base URL construction differs from Deribit
**What goes wrong:** Constructing REST URL from WebSocket URL like Deribit does.
**Why it happens:** Deribit lifecycle code strips `wss://` and `/ws/` from ws_url to build REST URL. Derive's REST API is at `https://api.lyra.finance`, same host as WebSocket `wss://api.lyra.finance/ws`.
**How to avoid:** Strip the `/ws` path from ws_url: `ws_url.trim_start_matches("wss://").trim_start_matches("ws://").split("/ws").next()` gives `api.lyra.finance`, then prepend `https://`.
**Warning signs:** 404 errors from incorrect URL construction.

### Pitfall 5: Forgetting to update min_poll_interval_secs
**What goes wrong:** Derive poll interval not included in minimum calculation, causing lifecycle tick to miss Derive polls.
**Why it happens:** `DiscoveryConfig::min_poll_interval_secs()` only chains `.min()` for existing three venues.
**How to avoid:** Add `.min(self.derive_poll_interval_secs)` to the chain.

### Pitfall 6: Missing Derive absence checking in step 1b
**What goes wrong:** Approved mappings with Derive instruments not checked against discovery data.
**Why it happens:** Step 1b checks Deribit/Kalshi/Polymarket instruments but has no Derive block.
**How to avoid:** Add a `mapping.venues.derive` check block identical to the existing three.

## Code Examples

### Derive REST Response Deserialization
```rust
// Source: tests/derive_api_probe.rs (confirmed against live API)
#[derive(Debug, Deserialize)]
struct DeriveInstrumentsResponse {
    result: Vec<DeriveInstrumentInfo>,
}

#[derive(Debug, Deserialize)]
struct DeriveInstrumentInfo {
    instrument_name: String,
    is_active: bool,
    #[allow(dead_code)]
    quote_currency: Option<String>,  // "USDC"
    option_details: Option<DeriveOptionDetails>,
}

#[derive(Debug, Deserialize)]
struct DeriveOptionDetails {
    /// Strike price as string (e.g., "100000")
    strike: String,
    /// Expiry as epoch timestamp (seconds or millis -- verify)
    expiry: u64,
    /// Option type: "C" or "P"
    option_type: String,
}
```

### Updating filter_new_candidates_fuzzy
```rust
// In filter_new_candidates_fuzzy(), replace:
//   Venue::Derive => {} // Derive matching deferred to v1.5 Phase 31
// With:
Venue::Derive => derive = Some(inst.instrument_id.clone()),

// And in the CandidateVenues construction, replace:
//   derive: None, // Derive matching deferred to v1.5 Phase 31
// With:
derive,

// Same pattern applies to filter_new_candidates() (the non-fuzzy version)
```

### Lifecycle Manager Derive Polling Block
```rust
// In poll_cycle(), add after Polymarket block:
// --- Derive ---
let mut derive_polled = false;
let mut derive_suspect = false;
if last_derive_poll.elapsed()
    >= Duration::from_secs(self.discovery_config.derive_poll_interval_secs)
{
    *last_derive_poll = Instant::now();
    metrics::counter!("lifecycle_discovery_polls", "venue" => "derive").increment(1);
    let derive_limiter = self.venue_rate_limiters.get(&Venue::Derive);
    let derive_rest_url = format!(
        "https://{}",
        self.venues_config.derive.ws_url
            .trim_start_matches("wss://")
            .trim_start_matches("ws://")
            .split("/ws")
            .next()
            .unwrap_or("api.lyra.finance")
    );
    match discover_derive(&self.http_client, &derive_rest_url, derive_limiter).await {
        Ok(instruments) => {
            let count = instruments.len();
            tracing::info!(venue = "derive", count = count, "discovered instruments");
            // ... suspect detection, previous poll count update ...
            derive_polled = true;
            all_discovered.extend(instruments);
        }
        Err(e) => {
            tracing::warn!(venue = "derive", error = %e, "Derive discovery failed, continuing");
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `Venue::Derive => {}` stubs | Active Derive matching | Phase 33 | Derive instruments participate in cross-venue candidate matching |
| 3-venue discovery | 4-venue discovery | Phase 33 | ContractLifecycleManager polls Derive alongside Deribit/Kalshi/Polymarket |
| No Derive config | `derive_poll_interval_secs` in DiscoveryConfig | Phase 33 | Independent Derive poll interval control |

## Open Questions

1. **Derive expiry timestamp unit (seconds vs milliseconds)**
   - What we know: The probe test accesses `option_details/expiry` as `u64`. Deribit uses milliseconds.
   - What's unclear: Whether Derive uses seconds or milliseconds. The probe test does not assert the unit.
   - Recommendation: Check if the value is > 10^12 (milliseconds) or < 10^12 (seconds) at runtime, or hardcode based on live API testing. Use `DateTime::from_timestamp(expiry as i64, 0)` for seconds or `DateTime::from_timestamp_millis(expiry as i64)` for millis.

2. **Whether Derive option_type is "C"/"P" or "call"/"put"**
   - What we know: Deribit API returns `"call"`/`"put"` in structured fields. Derive instrument names use `C`/`P`. The probe test checks `option_details/option_type` as string.
   - What's unclear: Exact string value returned by REST API.
   - Recommendation: Handle both: match on `"C" | "call"` for Above, `"P" | "put"` for Below.

## Sources

### Primary (HIGH confidence)
- `src/events/discovery.rs` -- Existing discover_deribit(), FuzzyMatchKey, filter_new_candidates_fuzzy() patterns
- `src/events/lifecycle.rs` -- ContractLifecycleManager poll_cycle() with venue polling pattern
- `src/events/toml_writer.rs` -- CandidateVenues.derive already implemented
- `src/config/events.rs` -- DiscoveryConfig structure needing derive_poll_interval_secs
- `tests/derive_api_probe.rs` -- Live API probe confirming POST endpoint, response structure, instrument format

### Secondary (MEDIUM confidence)
- `src/config/venues.rs` -- DeriveConfig with ws_url for REST URL construction

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in use, no new dependencies
- Architecture: HIGH -- follows exact established pattern from 3 existing venues
- Pitfalls: HIGH -- confirmed via probe tests and codebase analysis
- API details (expiry unit, option_type format): MEDIUM -- probe test confirms structure but not all field semantics

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable internal codebase patterns)
