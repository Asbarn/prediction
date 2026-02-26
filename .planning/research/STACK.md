# Stack Research: v1.2 Automated Event Management

**Domain:** Venue market discovery, cross-venue event matching, TOML proposal writing
**Researched:** 2026-02-26
**Confidence:** HIGH

## Scope

This document covers ONLY the stack additions needed for v1.2 Automated Event Management. The existing v1.0/v1.1 stack is validated and unchanged. See prior STACK.md versions for those decisions.

## Existing Stack Already Covering v1.2 Needs

Most v1.2 capabilities are **already covered** by the existing dependency tree. Critical finding: discovery.rs and toml_writer.rs already exist with working implementations.

| Technology | Version (resolved) | v1.2 Usage | Already Used By |
|------------|-------------------|------------|-----------------|
| reqwest | 0.12 | Venue discovery API polling | Settlement checkers, Gamma API |
| serde + serde_json | 1.0 | Deserialize API responses | Everywhere |
| toml | 0.8.23 | Config deserialization | SystemConfig, EventsConfig |
| toml_edit | 0.22.27 | Format-preserving TOML writes | `events::toml_writer` (already implemented) |
| governor | 0.8.1 | Rate limiting discovery polls | `feed::reliability::rate_limiter` |
| chrono | 0.4 | Expiry dates, timestamps | Throughout |
| rust_decimal | 1.40 | Strike price comparison | Pricing pipeline |
| tokio | 1.x (full) | Async polling loops, channels | Runtime |
| tracing | 0.1 | Log new proposals | Everywhere |
| rsa + sha2 + base64 | 0.9 / 0.10 / 0.22 | Kalshi RSA-PSS auth for discovery | Kalshi feed auth |

## New Dependencies Required

### One new direct dependency: `strsim`

| Technology | Version | Purpose | Why This One |
|------------|---------|---------|-------------|
| strsim | 0.11 | String similarity metrics for fuzzy event name matching | Already in dependency tree transitively via clap; provides Jaro-Winkler and normalized Levenshtein -- the two algorithms needed for event title comparison |

**Why `strsim` and not alternatives:**

| Crate | What It Does | Why Not (or Why Yes) |
|-------|-------------|---------------------|
| **strsim 0.11** | String similarity metrics (Jaro-Winkler, Levenshtein, Sorensen-Dice) | **USE THIS.** Already transitively compiled (via clap). Provides normalized 0.0-1.0 similarity scores. Zero additional binary size cost. |
| nucleo | Interactive fuzzy finder (fzf-like) | Wrong tool. Designed for user-facing search-as-you-type, not batch comparison of event titles. |
| fuzzy-matcher | Smith-Waterman based fuzzy matching | Wrong tool. Designed for ranking search results, not pairwise similarity scoring between known strings. |
| rapidfuzz | Python-first fuzzy matching with Rust bindings | Heavier than needed. strsim covers our use case in a fraction of the API surface. |
| regex | Pattern matching | Over-engineered for "are these two event titles about the same thing?" questions. |

### Why strsim Is the Right Choice

The cross-venue matching problem is: given `"Will BTC be above $100K on June 27?"` (Polymarket) and `"BTC-27JUN25-100000-C"` (Deribit), determine if they describe the same event.

For **Deribit and Kalshi**, matching is already exact (per discovery.rs): both venues expose structured fields (asset, strike, expiry, direction) that can be compared directly. The existing `MatchKey` four-field exact match handles this.

Fuzzy matching is needed specifically for:
1. **Polymarket question text parsing** -- extracting structured fields from free-form text like "Will Bitcoin be above $100,000 on June 27, 2025?" and matching against known event patterns
2. **Confidence scoring** -- computing a similarity score between a normalized Polymarket question and a canonical event description to set a confidence threshold for auto-proposals
3. **Future multi-asset expansion** -- when event naming conventions vary across venues

The relevant `strsim` functions:
- `jaro_winkler(a, b) -> f64` -- Returns 0.0-1.0 similarity, boosting common prefixes. Best for event titles that start with the same asset name.
- `normalized_levenshtein(a, b) -> f64` -- Returns 0.0-1.0 similarity based on edit distance. Good for catching rewordings.
- `sorensen_dice(a, b) -> f64` -- Bigram-based similarity. Useful as a tiebreaker.

**Confidence:** HIGH. `strsim 0.11.1` is already compiled as a transitive dependency of `clap_builder 4.5.60 -> strsim 0.11.1`. Adding it as a direct dependency reuses the same compiled artifact with zero additional build cost.

### What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| Any NLP/ML crate (rust-bert, tokenizers, etc.) | Massive overkill. Event matching is pattern extraction + string similarity, not natural language understanding. | `strsim` + regex patterns + structured field comparison |
| fuzzy-matcher | Interactive search tool, not batch similarity scorer | `strsim` |
| nucleo / nucleo-matcher | fzf-like fuzzy finder for UIs, wrong problem domain | `strsim` |
| skim | Terminal fuzzy finder, not a library for programmatic matching | `strsim` |
| levenshtein (standalone crate) | Only provides Levenshtein. strsim provides Levenshtein + 7 other algorithms in one crate that is already compiled. | `strsim` |
| Any database (SQLite, sled) | Discovery state is tiny (list of known instruments, last poll time). Fits in memory + existing checkpoint pattern. | In-memory HashSet + JSON checkpoint |
| reqwest-middleware | Rate limiting is already handled by `governor`. Adding middleware layers is unnecessary abstraction. | `governor::RateLimiter` (existing) |
| tower-http rate limiting | Already have `governor` for per-venue rate limiting. tower-http is for server-side middleware. | `governor::RateLimiter` (existing) |

---

## Venue Discovery API Details

### Deribit: `public/get_instruments`

**Already implemented in:** `src/events/discovery.rs::discover_deribit()`

| Property | Value |
|----------|-------|
| Endpoint | `GET /api/v2/public/get_instruments?currency={}&kind=option` |
| Auth | None (public endpoint) |
| Pagination | None -- returns all instruments for currency/kind in one response |
| Rate limit | 1 req/s sustained (official docs). Current config polls every 300s, well under limit. |
| Response size | ~200-500 instruments per currency for options. Single response, no pagination needed. |
| Filtering | By currency (required), kind (optional), expired flag |

**No changes needed.** The existing implementation is correct and complete.

### Kalshi: `GET /trade-api/v2/markets`

**Already implemented in:** `src/events/discovery.rs::discover_kalshi()`

| Property | Value |
|----------|-------|
| Endpoint | `GET /trade-api/v2/markets?series_ticker={}&status=open&limit=200` |
| Auth | RSA-PSS signed headers (reuses existing `sign_kalshi_request`) |
| Pagination | Cursor-based. Response includes `cursor` field; pass as query param for next page. Empty/null cursor = last page. |
| Rate limit | Not explicitly documented for this endpoint. Current config polls every 600s. |
| Filtering | By series_ticker, status, event_ticker. Supports up to limit=1000. |
| Response size | Paginated, default limit=100, max 1000. KXBTC series typically has 20-50 open markets. |

**No changes needed.** Cursor-based pagination is already correctly implemented in the loop.

### Polymarket: `GET /markets` (Gamma API)

**Already implemented in:** `src/events/discovery.rs::discover_polymarket()`

| Property | Value |
|----------|-------|
| Endpoint | `GET {gamma_api_url}/markets?active=true&limit=100&offset={}` |
| Auth | None required |
| Pagination | Offset-based. Increment offset by limit until response count < limit. |
| Rate limit | Not officially documented. Current config polls every 600s. |
| Filtering | By active, closed, tag_id, slug, order |
| Response size | Polymarket has thousands of markets. Crypto subset is smaller but still needs pagination. |

**No changes needed.** Offset-based pagination is already correctly implemented.

**Note:** The existing implementation correctly marks Polymarket as "deactivation monitoring only in v1" -- structured field extraction from free-form questions is the v1.2 addition that needs `strsim`.

---

## Rate Limiting Strategy for Discovery Polling

The existing `governor` crate is sufficient. No new rate limiting infrastructure needed.

### Current Rate Limiting Architecture

```
VenueRateLimiter (governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>)
  |-- Per-venue instance with configurable req/s quota
  |-- .wait().await blocks until rate allows
  |-- Already used in: WebSocket message sending, settlement checking
```

### Discovery Polling Approach

Discovery runs on long intervals (300-600 seconds per venue), far below any rate limit. The rate limiter is a safety net, not the primary throttle.

| Venue | Poll interval (config) | API rate limit | Safety margin |
|-------|----------------------|----------------|---------------|
| Deribit | 300s (5 min) | 1 req/s sustained | 300x headroom |
| Kalshi | 600s (10 min) | Conservative (undocumented) | Safe at 10-min intervals |
| Polymarket | 600s (10 min) | Undocumented | Safe at 10-min intervals |

**Recommendation:** Reuse the existing `VenueRateLimiter` pattern. Create one rate limiter per venue for discovery operations, sharing the same `governor` infrastructure. The `tokio::time::interval` loop already provides coarse-grained throttling; the rate limiter prevents bursts during pagination (Kalshi may need 2-3 requests per poll cycle due to cursor pagination).

---

## TOML Writing Strategy

**Already implemented in:** `src/events/toml_writer.rs`

The `toml_edit 0.22` crate is already integrated with working functions:
- `append_candidate_to_toml()` -- Appends new `[[events]]` entry with `approved = false`
- `mark_expired_in_toml()` -- Updates status field to "expired"

Both functions parse the existing TOML content, modify the AST, and serialize back -- preserving all comments, formatting, and manual edits.

**Why toml_edit and not toml for writing:**
- `toml` (the serde-based crate) would serialize from structs, destroying comments, custom formatting, and field ordering
- `toml_edit` operates on the document AST, preserving every byte that is not modified
- This is critical because `events.toml` is human-curated (operator reviews and approves proposals)

**No version upgrade needed.** `toml_edit 0.22.27` is current and compatible with `toml 0.8.23` (they share `toml_datetime 0.6.11`). While 0.23.x exists, upgrading would require also upgrading `toml` to maintain compatibility, which is unnecessary churn for zero functional benefit.

---

## Cargo.toml Changes

**One line added:**

```toml
# Fuzzy string matching for cross-venue event name comparison
strsim = "0.11"
```

That is the entirety of the Cargo.toml change for v1.2.

Full context of what stays unchanged:

```toml
# EXISTING -- already covers v1.2 discovery needs
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }  # API polling
serde = { version = "1.0", features = ["derive"] }                 # Response deserialization
serde_json = "1.0"                                                  # JSON parsing
toml = "0.8"                                                        # Config reading
toml_edit = "0.22"                                                  # Format-preserving TOML writing
governor = "0.8"                                                    # Rate limiting
chrono = { version = "0.4", features = ["serde"] }                  # Dates and timestamps
rust_decimal = { version = "1.40", features = ["maths", "serde-with-str"] }  # Strike comparison
rsa = { version = "0.9", features = ["sha2"] }                     # Kalshi auth
tokio = { version = "1", features = ["full"] }                     # Async runtime
tracing = "0.1"                                                     # Logging

# NEW -- one addition
strsim = "0.11"                                                     # String similarity (Jaro-Winkler, Levenshtein)
```

## Integration Points with Existing Architecture

### What Already Exists (discovery.rs)

The `src/events/discovery.rs` module already contains:
- `discover_deribit()` -- Polls Deribit instruments API, returns `Vec<DiscoveredInstrument>`
- `discover_kalshi()` -- Polls Kalshi markets API with cursor pagination, returns `Vec<DiscoveredInstrument>`
- `discover_polymarket()` -- Polls Polymarket Gamma API with offset pagination, returns `Vec<PolymarketMarketInfo>`
- `find_cross_venue_candidates()` -- Groups instruments by exact `MatchKey` (asset + strike + expiry + direction)
- `filter_new_candidates()` -- Filters out already-registered events
- `flag_novel_instruments()` -- Identifies single-venue instruments for operator attention

### What v1.2 Adds Using `strsim`

The missing piece is Polymarket-to-structured-field matching. Currently Polymarket is "deactivation monitoring only" because its data is free-form text. With `strsim`:

```rust
use strsim::jaro_winkler;

/// Compute confidence that a Polymarket question matches a known event.
fn match_confidence(
    polymarket_question: &str,
    canonical_description: &str,
) -> f64 {
    let normalized_question = normalize_event_text(polymarket_question);
    let normalized_canonical = normalize_event_text(canonical_description);
    jaro_winkler(&normalized_question, &normalized_canonical)
}

fn normalize_event_text(text: &str) -> String {
    text.to_lowercase()
        .replace("bitcoin", "btc")
        .replace("$", "")
        .replace(",", "")
        // ... further normalization
}
```

This enables:
1. Given a Deribit+Kalshi candidate match (found via exact fields), search Polymarket for a market about the same event
2. Score Polymarket questions against the canonical event description
3. If score exceeds threshold (e.g., 0.85), include Polymarket in the proposed mapping

### What toml_writer.rs Already Handles

The TOML writing is fully implemented. The `append_candidate_to_toml()` function:
- Parses existing `events.toml` preserving comments and formatting
- Appends a new `[[events]]` entry with all venue-specific sub-tables
- Sets `approved = false` and `discovered_at` timestamp
- Returns the modified TOML as a string

The orchestrator (discovery manager) will:
1. Read current `events.toml` content
2. Call `append_candidate_to_toml()` with the new candidate
3. Write the result atomically (write-to-temp + rename, existing pattern)
4. Emit a structured log for operator notification
5. The existing `ConfigReloader` + SIGHUP mechanism picks up new approved entries

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| String matching | strsim 0.11 | Hand-rolled Levenshtein | strsim is already compiled (via clap). Reimplementing Jaro-Winkler correctly is non-trivial and error-prone. |
| String matching | strsim 0.11 | NLP/ML (rust-bert) | Enormous dependency (PyTorch runtime). Event matching is string comparison, not language understanding. |
| TOML writing | toml_edit 0.22 (existing) | toml 0.8 serde serialization | Destroys comments and formatting in human-curated config files. |
| TOML writing | toml_edit 0.22 (existing) | toml_edit 0.23 upgrade | Would require coordinated toml 0.9 upgrade. No functional benefit for our use case. |
| Rate limiting | governor 0.8 (existing) | reqwest-middleware | Already have per-venue rate limiters. Adding middleware is unnecessary abstraction. |
| Rate limiting | governor 0.8 (existing) | Manual tokio::time::sleep | governor provides GCRA token bucket. Manual sleep does not handle burst correctly. |
| Discovery state | In-memory + JSON checkpoint | SQLite | Discovery state is a set of known instrument IDs. Fits in a HashSet. |
| Pagination | Per-venue implementation (existing) | Generic paginator crate | No good Rust crate exists. Each venue has different pagination (none, cursor, offset). Three implementations already exist and work. |

## Version Compatibility

| Crate | Pinned | Resolved | Rust Edition | Notes |
|-------|--------|----------|-------------|-------|
| strsim | 0.11 | 0.11.1 | 2015+ compatible | Already compiled as transitive dep of clap |
| toml_edit | 0.22 | 0.22.27 | 2021+ | Paired with toml 0.8, do not upgrade independently |
| toml | 0.8 | 0.8.23 | 2021+ | Paired with toml_edit 0.22 |
| governor | 0.8 | 0.8.1 | 2021+ | Stable, actively maintained |

Key constraint: Rust 2024 edition (1.85+) is specified in `Cargo.toml`. All crates (existing and new) support this.

## Sources

- [strsim crate on crates.io](https://crates.io/crates/strsim) -- v0.11.1, provides Jaro-Winkler, Levenshtein, Sorensen-Dice and others (HIGH confidence)
- [strsim-rs GitHub](https://github.com/rapidfuzz/strsim-rs) -- Maintained by rapidfuzz organization, last updated Nov 2025 (HIGH confidence)
- [strsim 0.11.1 API docs](https://docs.rs/strsim/0.11.1/strsim/) -- Full function listing confirmed (HIGH confidence)
- [Deribit API docs](https://docs.deribit.com/) -- `public/get_instruments` endpoint: no pagination, no auth, 1 req/s sustained limit (HIGH confidence)
- [Kalshi API docs: Get Markets](https://docs.kalshi.com/api-reference/market/get-markets) -- Cursor-based pagination, limit 1-1000, series_ticker filtering (HIGH confidence)
- [Kalshi API docs: Pagination](https://docs.kalshi.com/getting_started/pagination) -- Cursor mechanism: null cursor = last page (HIGH confidence)
- [Polymarket Gamma API: Get Events](https://docs.polymarket.com/developers/gamma-markets-api/get-events) -- Offset-based pagination, active/closed filtering (HIGH confidence)
- [toml_edit on crates.io](https://crates.io/crates/toml_edit) -- v0.23.7 latest, v0.22.27 in use, no breaking changes needed (MEDIUM confidence)
- [governor on crates.io](https://crates.io/crates/governor) -- v0.8.x, GCRA-based rate limiting (HIGH confidence)
- Existing codebase analysis: `src/events/discovery.rs`, `src/events/toml_writer.rs`, `src/feed/reliability/rate_limiter.rs` -- All three discovery functions, TOML writer, and rate limiter already implemented (HIGH confidence)
- Cargo dependency tree: `cargo tree -p strsim -i` confirms strsim 0.11.1 is already compiled via clap_builder (HIGH confidence)

---
*Stack research for: v1.2 Automated Event Management*
*Researched: 2026-02-26*
