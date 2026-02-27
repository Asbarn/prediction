# Phase 19: Polymarket Discovery and Cross-Venue Matching - Research

**Researched:** 2026-02-27
**Domain:** Polymarket Gamma API parsing, cross-venue instrument matching with expiry tolerance, confidence scoring
**Confidence:** HIGH

## Summary

Phase 19 extends the existing discovery infrastructure (hardened in Phase 18) with two major capabilities: (1) Polymarket structured field extraction from Gamma API market `question` text using regex-free string parsing, and (2) cross-venue matching with configurable expiry date tolerance replacing the current exact-expiry-only matching.

The Polymarket Gamma API returns crypto price markets as events containing multiple markets. Each market has a `question` field with predictable text patterns ("Will Bitcoin reach $150,000 by December 31, 2025?" for upward targets, "Will Bitcoin dip to $75,000 by February 28, 2025?" for downward targets). The `groupItemTitle` field contains abbreviated labels like "$150,000" or "^100,000". The `endDateIso` field provides the expiry date directly. The `conditionId` and `clobTokenIds` fields provide the blockchain identifiers needed for the existing `PolymarketMapping` config type.

The current `MatchKey` uses exact four-field matching including exact `NaiveDate` expiry, which prevents Deribit (Friday expiries), Kalshi (end-of-month expiries), and Polymarket (end-of-month expiries) from matching for the same economic event. The fix is to replace expiry in the match key with a tolerance-based grouping: match on exact asset/strike/direction, then cluster instruments whose expiry dates fall within a configurable window (default 7 days). Each cluster produces a candidate proposal with an `ExpiryConfidence` score based on the maximum date spread within the group.

**Primary recommendation:** Add `discover_polymarket_structured()` that parses `question` text with simple string operations (no regex crate needed), returns `Vec<DiscoveredInstrument>`, and replace exact-expiry `MatchKey` with `FuzzyMatchKey` (asset/strike/direction only) plus an expiry-tolerance grouping pass that produces confidence-scored candidates.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DISC-01 | System polls Polymarket Gamma API with crypto category filtering and extracts structured fields (asset, strike, direction, expiry) from groupItemTitle patterns | Gamma API `/events` endpoint with slug or tag filtering; `question` field contains "Will Bitcoin reach/dip to $X by Date?" patterns; `endDateIso` provides direct expiry date; `groupItemTitle` provides price label; `conditionId` + `clobTokenIds[0]` provide venue identifiers. Parse `question` text for direction (reach=Above, dip=Below), asset (Bitcoin=BTC), strike (dollar amount), and use `endDateIso` for expiry. |
| DISC-03 | System matches cross-venue instruments using exact asset/strike/direction with configurable expiry date tolerance window (default 7 days) | Replace exact `MatchKey` (includes expiry) with `FuzzyMatchKey` (asset/strike/direction only). Group instruments by FuzzyMatchKey, then within each group check that all expiry dates fall within the configurable tolerance window. Deribit Friday expiry and Kalshi/Polymarket end-of-month expiry for the same target period will match when within 7 days. |
| DISC-04 | System generates cross-venue candidate proposals including instruments from all matched venues with expiry confidence scoring (HIGH/MEDIUM/LOW based on date difference) | ExpiryConfidence enum: HIGH (all expiries within 2 days), MEDIUM (within 7 days), LOW (within tolerance but >7 days -- only if tolerance configured >7). Include confidence in CandidateMapping and log output. |
| INTG-02 | Polymarket discovery returns Vec<DiscoveredInstrument> (same type as Deribit/Kalshi) for unified cross-venue matching | `discover_polymarket_structured()` returns `Vec<DiscoveredInstrument>` with venue=Polymarket, instrument_id=conditionId, and all structured fields extracted from `question`/`endDateIso`. Integrates directly into existing `find_cross_venue_candidates()` pipeline after MatchKey update. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| reqwest | 0.12 | HTTP client for Gamma API polling | Already in Cargo.toml; used by existing discovery and settlement |
| chrono | 0.4 | Date parsing and comparison for expiry tolerance | Already in Cargo.toml; NaiveDate arithmetic for date difference |
| rust_decimal | 1.40 | Strike price parsing from dollar amounts | Already in Cargo.toml; used throughout codebase |
| serde/serde_json | 1.0 | JSON deserialization of Gamma API responses | Already in Cargo.toml; used by existing Polymarket types |
| toml_edit | 0.22 | Format-preserving TOML mutation for candidates | Already in Cargo.toml; batch mutation pattern from Phase 18 |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 | Structured logging for parse failures and confidence scores | Already in Cargo.toml; log unparseable questions at warn level |
| metrics | 0.24 | Prometheus counters for parsed/unparseable markets | Already in Cargo.toml; track parse success rate |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| String parsing for question text | `regex` crate | regex is NOT in dependency tree; string operations (contains, split, trim, parse) are sufficient for the 2-3 predictable Polymarket question patterns; adding regex would be a new dependency |
| Date tolerance in matching | strsim fuzzy string matching | Wrong abstraction; expiry tolerance is a numeric date comparison, not string similarity; strsim (v0.11, transitive dep) is for text comparison |
| Single tolerance window | Per-venue tolerance pairs | Over-engineering; a single configurable days window handles all observed venue expiry patterns (Deribit Friday, Kalshi end-of-month, Polymarket end-of-month) |

**Installation:** No new crate dependencies. Zero additions to `Cargo.toml`.

## Architecture Patterns

### Recommended Project Structure
```
src/
+-- events/
|   +-- discovery.rs        # MODIFIED: add discover_polymarket_structured(), FuzzyMatchKey,
|   |                        #           tolerance-based matching, ExpiryConfidence, update
|   |                        #           find_cross_venue_candidates
|   +-- toml_writer.rs      # MODIFIED: add expiry_confidence field to CandidateMapping
|   +-- lifecycle.rs        # MODIFIED: wire Polymarket structured discovery into poll_cycle,
|   |                        #           pass rate limiter, include Polymarket in cross-venue matching
|   +-- registry.rs         # UNCHANGED
|   +-- risk.rs             # UNCHANGED
|   +-- mod.rs              # UNCHANGED
+-- config/
|   +-- events.rs           # MODIFIED: add expiry_tolerance_days to DiscoveryConfig,
|   |                        #           add polymarket_slugs config, add expiry_confidence to EventMapping
+-- main.rs                  # UNCHANGED (rate limiters already passed)
```

### Pattern 1: Polymarket Question Text Parsing (String Operations)
**What:** Parse the `question` field from Polymarket Gamma API markets using simple string operations to extract asset, strike, direction, and expiry. Use `endDateIso` for the authoritative expiry date.
**When to use:** For all Polymarket crypto price binary markets.
**Why not regex:** The `regex` crate is not in the dependency tree and the question patterns are sufficiently predictable that string operations work reliably.

**Observed Polymarket question patterns (from live Gamma API, 2026-02-27):**

| Pattern | Example | Direction |
|---------|---------|-----------|
| "Will {Asset} reach ${Strike} by {Date}?" | "Will Bitcoin reach $150,000 by December 31, 2025?" | Above |
| "Will {Asset} dip to ${Strike} by {Date}?" | "Will Bitcoin dip to $75,000 by February 28, 2025?" | Below |
| "Will {Asset} hit ${Strike} by {Date}?" | "Will Bitcoin hit $100,000 by December 31, 2025?" | Above |

**groupItemTitle patterns:**
- Upward: "$150,000" or "^100,000" or "^$150,000"
- Downward: "$75,000"
- These are display labels, NOT authoritative for parsing. Use `question` text + `endDateIso` instead.

```rust
// Source: codebase pattern + Polymarket API analysis

/// Asset name mapping from Polymarket question text to normalized ticker.
fn normalize_polymarket_asset(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "bitcoin" => Some("BTC"),
        "ethereum" | "ether" => Some("ETH"),
        "solana" => Some("SOL"),
        _ => None,
    }
}

/// Parse a Polymarket question into structured fields.
///
/// Supports patterns:
///   "Will {Asset} reach ${Strike} by {Date}?" -> Above
///   "Will {Asset} dip to ${Strike} by {Date}?" -> Below
///   "Will {Asset} hit ${Strike} by {Date}?" -> Above (treat "hit" as upward)
///
/// Returns None for unparseable questions (logged at warn level).
fn parse_polymarket_question(question: &str) -> Option<(String, Decimal, Direction)> {
    // Strip leading "Will " and trailing "?"
    let q = question.strip_prefix("Will ")?.strip_suffix('?')?.trim();

    // Find the asset name (first word after "Will ")
    let space_idx = q.find(' ')?;
    let asset_name = &q[..space_idx];
    let asset = normalize_polymarket_asset(asset_name)?.to_string();
    let rest = &q[space_idx + 1..];

    // Determine direction from verb
    let (direction, after_verb) = if let Some(r) = rest.strip_prefix("reach $") {
        (Direction::Above, r)
    } else if let Some(r) = rest.strip_prefix("hit $") {
        (Direction::Above, r)
    } else if let Some(r) = rest.strip_prefix("dip to $") {
        (Direction::Below, r)
    } else {
        return None;
    };

    // Extract strike: everything before " by "
    let by_idx = after_verb.find(" by ")?;
    let strike_str = &after_verb[..by_idx];
    // Remove commas from strike (e.g., "150,000" -> "150000")
    let strike_clean: String = strike_str.chars().filter(|c| *c != ',').collect();
    let strike = Decimal::from_str_exact(&strike_clean).ok()?;

    Some((asset, strike, direction))
}
```

### Pattern 2: Fuzzy Match Key (Expiry Tolerance)
**What:** Replace exact four-field MatchKey with a three-field FuzzyMatchKey (asset/strike/direction only) for the initial grouping pass, then apply expiry tolerance within each group to form candidate proposals.
**When to use:** Cross-venue matching where venues have different expiry conventions.

```rust
/// Three-field match key for cross-venue matching with expiry tolerance.
/// Exact on asset, strike, and direction. Expiry is matched within a
/// configurable tolerance window in a second pass.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FuzzyMatchKey {
    pub asset: String,
    pub strike: Decimal,
    pub direction: Direction,
}

impl FuzzyMatchKey {
    pub fn from_discovered(d: &DiscoveredInstrument) -> Self {
        Self {
            asset: d.asset.to_uppercase(),
            strike: d.strike,
            direction: d.direction.clone(),
        }
    }
}

/// Confidence level for expiry alignment between matched venues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpiryConfidence {
    /// All venue expiries within 2 days of each other.
    High,
    /// All venue expiries within 7 days of each other.
    Medium,
    /// All venue expiries within the configured tolerance (>7 days).
    Low,
}

impl std::fmt::Display for ExpiryConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpiryConfidence::High => write!(f, "HIGH"),
            ExpiryConfidence::Medium => write!(f, "MEDIUM"),
            ExpiryConfidence::Low => write!(f, "LOW"),
        }
    }
}

/// Compute expiry confidence from the maximum date spread in a group.
fn compute_expiry_confidence(expiries: &[NaiveDate]) -> ExpiryConfidence {
    if expiries.len() <= 1 {
        return ExpiryConfidence::High;
    }
    let min = expiries.iter().min().unwrap();
    let max = expiries.iter().max().unwrap();
    let spread_days = (*max - *min).num_days();

    if spread_days <= 2 {
        ExpiryConfidence::High
    } else if spread_days <= 7 {
        ExpiryConfidence::Medium
    } else {
        ExpiryConfidence::Low
    }
}
```

### Pattern 3: Two-Pass Cross-Venue Matching
**What:** First pass groups by FuzzyMatchKey (asset/strike/direction). Second pass checks expiry tolerance within each group, computing confidence and selecting the representative expiry date.
**When to use:** Replaces the current single-pass `find_cross_venue_candidates()`.

```rust
/// Group discovered instruments by asset/strike/direction, then filter
/// by expiry tolerance, computing confidence for each candidate group.
///
/// Returns groups with instruments from 2+ different venues where all
/// expiry dates fall within the configured tolerance window.
pub fn find_cross_venue_candidates_fuzzy(
    instruments: &[DiscoveredInstrument],
    expiry_tolerance_days: i64,
) -> Vec<(FuzzyMatchKey, Vec<&DiscoveredInstrument>, ExpiryConfidence)> {
    // Pass 1: Group by asset/strike/direction (no expiry)
    let mut groups: HashMap<FuzzyMatchKey, Vec<&DiscoveredInstrument>> = HashMap::new();
    for inst in instruments {
        let key = FuzzyMatchKey::from_discovered(inst);
        groups.entry(key).or_default().push(inst);
    }

    // Pass 2: Filter, check expiry tolerance, compute confidence
    let mut results = Vec::new();
    for (key, insts) in groups {
        // Must have 2+ different venues
        let venues: HashSet<Venue> = insts.iter().map(|i| i.venue).collect();
        if venues.len() < 2 {
            continue;
        }

        // Check expiry tolerance: all dates must be within window
        let expiries: Vec<NaiveDate> = insts.iter().map(|i| i.expiry).collect();
        let min_expiry = expiries.iter().min().unwrap();
        let max_expiry = expiries.iter().max().unwrap();
        let spread = (*max_expiry - *min_expiry).num_days();

        if spread > expiry_tolerance_days {
            continue; // Expiry spread exceeds tolerance
        }

        let confidence = compute_expiry_confidence(&expiries);
        results.push((key, insts, confidence));
    }

    results
}
```

### Pattern 4: Gamma API Event-Based Polling with Slug Filtering
**What:** Poll the Gamma API `/events` endpoint with known event slugs (e.g., "what-price-will-bitcoin-hit-in-february") to get crypto price markets, rather than attempting to filter all markets by category tag.
**When to use:** For discovering new Polymarket crypto price prediction markets.
**Why slug-based:** The Gamma API's tag filtering returns stale/irrelevant results in testing. Slug-based discovery with known event patterns is more reliable and targeted. The event slugs follow predictable patterns that can be configured via TOML.

```rust
/// Polymarket event slug patterns for crypto price markets.
/// Configured in DiscoveryConfig.polymarket_event_slugs.
///
/// Examples:
///   "what-price-will-bitcoin-hit-in-{month}"
///   "what-price-will-bitcoin-hit-in-{year}"
///
/// The lifecycle manager generates current slugs based on the current date.
fn generate_polymarket_slugs(base_patterns: &[String]) -> Vec<String> {
    let now = chrono::Utc::now();
    let month_name = now.format("%B").to_string().to_lowercase();
    let year = now.format("%Y").to_string();

    base_patterns.iter().map(|pattern| {
        pattern
            .replace("{month}", &month_name)
            .replace("{year}", &year)
    }).collect()
}
```

### Anti-Patterns to Avoid
- **Parsing `groupItemTitle` for structured data:** The `groupItemTitle` field is a display label (e.g., "$150,000"), NOT an authoritative data source. It lacks direction information and has inconsistent formatting. Use the `question` field + `endDateIso` instead.
- **Using `regex` crate for 2-3 simple patterns:** Adding a new dependency for predictable string patterns that `str::strip_prefix()`, `str::contains()`, and `str::find()` handle perfectly. The project has zero regex dependency currently.
- **Treating Polymarket tag/category filtering as reliable:** Testing showed the Gamma API's tag filtering returns old/irrelevant data. Event slug-based discovery is more reliable and targeted.
- **Exact expiry matching for cross-venue candidates:** Deribit options expire on Fridays, Kalshi on month-end, Polymarket on month-end. The same economic event (e.g., "BTC above $100K in June 2025") has different specific dates across venues. Exact matching misses valid cross-venue opportunities.
- **Using condition_id as the primary Polymarket instrument ID for matching:** Multiple markets within the same event have different condition IDs. The condition_id is the correct instrument identifier for the PolymarketMapping in events.toml, but the `question` text provides the matching fields (asset/strike/direction).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Date arithmetic for expiry tolerance | Manual day counting | `chrono::NaiveDate` subtraction (`date1 - date2 = Duration`) | Handles month boundaries, leap years correctly |
| Dollar amount parsing with commas | Custom numeric parser | `Decimal::from_str_exact()` after removing commas | Already validated in Deribit/Kalshi parsing |
| HTTP pagination for Gamma API | Custom pagination state machine | Offset-based loop (existing `discover_polymarket()` pattern) | Already implemented and working |
| TOML writing with confidence field | Manual string building | `toml_edit::Table` with `build_candidate_table()` extension | Existing helper from Phase 18 handles all formatting |
| Rate-limited API polling | Manual sleep/timer | Existing `VenueRateLimiter` from feed pipeline | Shared rate budget already wired in Phase 18 |

**Key insight:** The Polymarket structured discovery is primarily a parsing problem (question text to structured fields) plus a matching algorithm change (exact expiry to tolerance-based). All HTTP infrastructure, TOML writing, rate limiting, and lifecycle management already exist from Phases 5, 16, and 18.

## Common Pitfalls

### Pitfall 1: Polymarket Question Format Changes
**What goes wrong:** Polymarket updates the wording of crypto price market questions (e.g., from "Will Bitcoin reach $X" to "Will BTC exceed $X"), and the parser silently fails to extract structured fields.
**Why it happens:** Polymarket markets are created by different market makers and the question format is not contractually fixed. The format has been stable for crypto price markets but could change without notice.
**How to avoid:** (1) Log all unparseable questions at WARN level with the full question text. (2) Track a `polymarket_parse_failures` Prometheus counter. (3) If the parse failure rate exceeds a threshold, emit an alert. (4) Keep the parser modular so new patterns can be added without restructuring.
**Warning signs:** `polymarket_parse_failures` counter increasing; WARN logs with unfamiliar question text; drop in discovered Polymarket instruments.

### Pitfall 2: Expiry Tolerance Window Too Wide
**What goes wrong:** A 7-day tolerance window matches instruments that are economically distinct events (e.g., a weekly Kalshi market for the week of June 23-27 with a monthly Polymarket market expiring June 30 -- they target different time periods even though dates overlap).
**Why it happens:** Date proximity does not always imply economic equivalence. Two instruments can expire within 7 days but measure different time windows.
**How to avoid:** (1) Make tolerance configurable (default 7 days). (2) The ExpiryConfidence score gives the operator visibility into date spread -- LOW confidence proposals get extra scrutiny. (3) The `approved = false` gate means no false match costs real money. (4) Start with 7 days as the default; the operator can tighten after observing proposal quality.
**Warning signs:** LOW confidence proposals for instruments that are clearly different economic events; multiple proposals for the same strike/asset/direction with overlapping but distinct expiry clusters.

### Pitfall 3: Missing clobTokenIds (Token ID Not in First Position)
**What goes wrong:** The code assumes `clobTokenIds[0]` is the "Yes" token for the binary market, but the token order could be outcome-dependent.
**Why it happens:** Polymarket binary markets have two tokens (Yes and No). The `outcomes` array determines which `clobTokenIds` entry maps to which outcome. If outcomes are `["Yes", "No"]`, then `clobTokenIds[0]` is the Yes token. But this mapping is not guaranteed by API documentation.
**How to avoid:** Always pair `clobTokenIds` with the `outcomes` array position. Find the "Yes" outcome index and use the corresponding `clobTokenIds` entry. The existing `PolymarketMapping` uses `token_id` which should be the "Yes" token (for the "above" direction) or appropriately selected.
**Warning signs:** Order book prices not matching expected probability range (e.g., a "70% likely" market showing 30% price because the wrong token was selected).

### Pitfall 4: Duplicate Candidates from Overlapping Event Slugs
**What goes wrong:** Both "what-price-will-bitcoin-hit-in-march" and "what-price-will-bitcoin-hit-in-2025" contain BTC-100000-Above markets that expire in March 2025. The parser creates duplicate candidates.
**Why it happens:** The same economic market can appear in multiple Polymarket events (monthly and yearly).
**How to avoid:** Deduplicate by `conditionId` before creating `DiscoveredInstrument` entries. Each unique conditionId should appear once in the discovered instruments, regardless of which event slug it came from.
**Warning signs:** Multiple identical candidates in logs; duplicate instrument_ids in the same poll cycle.

### Pitfall 5: Gamma API Rate Limiting on Burst Polling
**What goes wrong:** Polling multiple event slugs in rapid succession triggers Gamma API rate limiting (HTTP 429 or silent throttling).
**Why it happens:** Each slug generates a separate HTTP request. With 12+ monthly slug patterns, that is 12+ requests in quick succession.
**How to avoid:** Use the shared `VenueRateLimiter` for Polymarket (already wired in Phase 18). Call `limiter.wait().await` before each Gamma API request.
**Warning signs:** HTTP 429 responses; empty or truncated API responses; `polymarket_discovery_errors` counter increasing.

## Code Examples

### Example 1: Polymarket Structured Discovery Function
```rust
// Source: extends existing discover_polymarket() in src/events/discovery.rs

/// Gamma API event response (multi-market events).
#[derive(Debug, Clone, Deserialize)]
struct GammaEventResponse {
    title: Option<String>,
    markets: Vec<PolymarketMarketInfo>,
}

/// Discover structured Polymarket instruments from Gamma API events.
///
/// Polls each configured event slug, parses market questions for structured
/// fields, and returns DiscoveredInstrument entries for cross-venue matching.
pub async fn discover_polymarket_structured(
    client: &reqwest::Client,
    gamma_api_url: &str,
    event_slugs: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all = Vec::new();
    let mut seen_conditions: HashSet<String> = HashSet::new();

    for slug in event_slugs {
        if let Some(limiter) = rate_limiter {
            limiter.wait().await;
        }

        let resp: Vec<GammaEventResponse> = client
            .get(format!("{}/events", gamma_api_url))
            .query(&[("slug", slug.as_str())])
            .send()
            .await?
            .json()
            .await?;

        for event in &resp {
            for market in &event.markets {
                // Deduplicate by conditionId
                if seen_conditions.contains(&market.condition_id) {
                    continue;
                }
                seen_conditions.insert(market.condition_id.clone());

                // Skip inactive/closed markets
                if !market.active || market.closed {
                    continue;
                }

                // Parse question for structured fields
                let (asset, strike, direction) = match parse_polymarket_question(&market.question) {
                    Some(fields) => fields,
                    None => {
                        tracing::warn!(
                            condition_id = %market.condition_id,
                            question = %market.question,
                            "unparseable Polymarket question, skipping"
                        );
                        metrics::counter!("polymarket_parse_failures").increment(1);
                        continue;
                    }
                };

                // Use endDateIso for expiry (authoritative)
                let expiry = match &market.end_date_iso {
                    Some(d) => match NaiveDate::parse_from_str(d, "%Y-%m-%d") {
                        Ok(date) => date,
                        Err(_) => continue,
                    },
                    None => continue,
                };

                // Get token_id (first token, typically "Yes" outcome)
                let token_id = market.tokens.first()
                    .map(|t| t.token_id.clone())
                    .unwrap_or_default();

                all.push(DiscoveredInstrument {
                    venue: Venue::Polymarket,
                    instrument_id: market.condition_id.clone(),
                    asset,
                    strike,
                    expiry,
                    direction,
                    is_active: market.active && !market.closed,
                    raw_expiry_timestamp: 0, // Polymarket uses date, not timestamp
                });
            }
        }
    }

    tracing::info!(
        venue = "polymarket",
        slugs_polled = event_slugs.len(),
        instruments_discovered = all.len(),
        "Polymarket structured discovery complete"
    );

    Ok(all)
}
```

### Example 2: DiscoveryConfig Extensions
```rust
// Source: extends existing src/config/events.rs DiscoveryConfig

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
    // ... existing fields from Phase 18 ...

    /// Expiry tolerance in days for cross-venue matching.
    /// Instruments with the same asset/strike/direction but different
    /// expiry dates within this window are considered matches.
    /// Default 7 days (covers Deribit Friday to Kalshi/Polymarket end-of-month).
    #[serde(default = "default_expiry_tolerance_days")]
    pub expiry_tolerance_days: i64,

    /// Polymarket event slug patterns for crypto price market discovery.
    /// Supports {month} and {year} placeholders that are expanded at runtime.
    /// Default: ["what-price-will-bitcoin-hit-in-{month}",
    ///           "what-price-will-bitcoin-hit-in-{year}"]
    #[serde(default = "default_polymarket_event_slugs")]
    pub polymarket_event_slugs: Vec<String>,
}

fn default_expiry_tolerance_days() -> i64 { 7 }
fn default_polymarket_event_slugs() -> Vec<String> {
    vec![
        "what-price-will-bitcoin-hit-in-{month}".to_string(),
        "what-price-will-bitcoin-hit-in-{year}".to_string(),
    ]
}
```

### Example 3: CandidateMapping with Confidence
```rust
// Source: extends existing CandidateMapping in src/events/toml_writer.rs

pub struct CandidateMapping {
    pub id: String,
    pub asset: String,
    pub strike: String,
    pub direction: Direction,
    pub expiry: String,
    pub venues: CandidateVenues,
    /// Confidence score for expiry alignment between matched venues.
    pub expiry_confidence: ExpiryConfidence,
}

// In build_candidate_table, add confidence field:
fn build_candidate_table(candidate: &CandidateMapping) -> Table {
    // ... existing fields ...
    entry["expiry_confidence"] = value(candidate.expiry_confidence.to_string());
    // ... rest of function ...
}
```

### Example 4: Updated CandidateVenues with Polymarket (condition_id, token_id)
```rust
// Source: CandidateVenues already supports polymarket: Option<(String, String)>
// No structural change needed -- just wire it up

// In filter_new_candidates, add Polymarket venue extraction:
for inst in instruments {
    match inst.venue {
        Venue::Deribit => deribit = Some(inst.instrument_id.clone()),
        Venue::Kalshi => kalshi = Some(inst.instrument_id.clone()),
        Venue::Polymarket => {
            // For Polymarket, instrument_id is condition_id
            // token_id comes from the discovered instrument's metadata
            polymarket = Some((inst.instrument_id.clone(), token_id.clone()));
        }
    }
}
```

### Example 5: Lifecycle Integration (poll_cycle changes)
```rust
// In lifecycle.rs poll_cycle(), add Polymarket structured discovery:

// --- Polymarket (structured discovery replacing deactivation-only monitoring) ---
if last_polymarket_poll.elapsed()
    >= Duration::from_secs(self.discovery_config.polymarket_poll_interval_secs)
{
    *last_polymarket_poll = Instant::now();
    metrics::counter!("lifecycle_discovery_polls", "venue" => "polymarket").increment(1);

    let polymarket_limiter = self.venue_rate_limiters.get(&Venue::Polymarket);
    let slugs = generate_polymarket_slugs(&self.discovery_config.polymarket_event_slugs);

    match discover_polymarket_structured(
        &self.http_client,
        &self.venues_config.polymarket.gamma_api_url,
        &slugs,
        polymarket_limiter,
    )
    .await
    {
        Ok(instruments) => {
            let count = instruments.len();
            tracing::info!(venue = "polymarket", count, "discovered structured instruments");
            // Partial-response detection
            if self.previous_poll_counts.is_suspect(
                Venue::Polymarket, count,
                self.discovery_config.partial_response_threshold,
            ) {
                tracing::warn!(venue = "polymarket", "suspect partial response");
                polymarket_suspect = true;
            } else {
                self.previous_poll_counts.update(Venue::Polymarket, count);
            }
            polymarket_polled = true;
            all_discovered.extend(instruments);
        }
        Err(e) => {
            tracing::warn!(venue = "polymarket", error = %e, "structured discovery failed");
        }
    }
}

// Then: cross-venue matching includes all three venues now
let candidates = find_cross_venue_candidates_fuzzy(
    &all_discovered,
    self.discovery_config.expiry_tolerance_days,
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Polymarket deactivation monitoring only | Full structured discovery with question parsing | This phase (Phase 19) | Polymarket instruments participate in cross-venue matching |
| Exact four-field MatchKey (including expiry date) | FuzzyMatchKey (asset/strike/direction) + expiry tolerance | This phase (Phase 19) | Cross-venue matches between Deribit Friday and Kalshi/Polymarket month-end expiries |
| Two-venue candidates (Deribit + Kalshi) | Three-venue candidates (Deribit + Kalshi + Polymarket) | This phase (Phase 19) | Higher-confidence arbitrage signals when three independent venues agree |
| No confidence scoring on candidates | ExpiryConfidence (HIGH/MEDIUM/LOW) based on date spread | This phase (Phase 19) | Operator can prioritize review of high-confidence proposals |

**Deprecated/outdated:**
- `find_cross_venue_candidates()` with exact MatchKey will be replaced by `find_cross_venue_candidates_fuzzy()` with FuzzyMatchKey + tolerance
- The deactivation-only `discover_polymarket()` will remain for backward compatibility but structured discovery via `discover_polymarket_structured()` is the primary path
- `CandidateMapping` without `expiry_confidence` field will be extended with the field (with a default for backward compatibility)

## Open Questions

1. **Polymarket token_id selection for PolymarketMapping**
   - What we know: Each Polymarket market has two tokens (Yes/No) in `clobTokenIds`. The `outcomes` array maps to `clobTokenIds` by position. The existing `PolymarketMapping` stores a single `token_id`.
   - What's unclear: Should the token_id be the "Yes" token for Above-direction markets and the "No" token for Below-direction markets? Or always the "Yes" token? The existing config uses token_id for CLOB WebSocket subscription.
   - Recommendation: Use the "Yes" token for Above-direction markets (buying "Yes" = betting price goes above), and the "Yes" token for Below-direction markets too (since "Will Bitcoin dip to $X" with "Yes" = the dip happened = Below). Verify by checking the existing PolymarketMapping entries in events.toml. The key is that the `token_id` used must match what the CLOB WebSocket expects for the correct outcome side. **MEDIUM confidence** -- needs validation against existing Polymarket feed code.

2. **Gamma API event slug pattern completeness**
   - What we know: Monthly events follow "what-price-will-bitcoin-hit-in-{month}" and yearly follow "what-price-will-bitcoin-hit-in-{year}". These are the observed patterns as of 2026-02-27.
   - What's unclear: Whether Polymarket will introduce new event slug patterns for crypto price markets (e.g., weekly, daily, different assets).
   - Recommendation: Make slug patterns configurable via `polymarket_event_slugs` in DiscoveryConfig. The default covers observed patterns. Operators can add new patterns without code changes. Log when a configured slug returns empty results (may indicate pattern change).

3. **Handling the "reach" vs "hit" vs "dip to" distinction for Direction**
   - What we know: "reach" and "hit" map to Above (upward target), "dip to" maps to Below (downward target). These are the only patterns observed.
   - What's unclear: Whether there are other verbs in use for other asset classes or newer markets.
   - Recommendation: Implement the three known verbs and log any unparseable patterns as WARN. The `polymarket_parse_failures` counter will surface new patterns. **HIGH confidence** for BTC markets.

4. **Representative expiry date for tolerance-matched candidates**
   - What we know: When Deribit expires June 27 and Kalshi expires June 30, the candidate needs a single "expiry" value for events.toml.
   - What's unclear: Should the candidate use the earliest, latest, or median expiry?
   - Recommendation: Use the **earliest** expiry date as the candidate's expiry. This is the most conservative choice -- the system treats the event as expiring at the earliest venue's expiry, ensuring near-expiry warnings trigger correctly. The per-venue actual expiry dates are preserved in the venue-specific mappings.

## Sources

### Primary (HIGH confidence)
- Polymarket Gamma API live response: `GET /events?slug=what-price-will-bitcoin-hit-in-2025` -- verified question field patterns, groupItemTitle values, endDateIso format, conditionId structure, clobTokenIds format, and outcomes array (2026-02-27)
- Polymarket Gamma API live response: `GET /events?slug=what-price-will-bitcoin-hit-in-february` -- verified monthly event slug pattern, question wording "reach" and "dip to" variants (2026-02-27)
- Codebase analysis: `src/events/discovery.rs` -- existing DiscoveredInstrument type, MatchKey, find_cross_venue_candidates(), discover_polymarket(), PolymarketMarketInfo struct with condition_id, question, end_date_iso, tokens fields
- Codebase analysis: `src/events/lifecycle.rs` -- poll_cycle structure, Polymarket polling section, batched_toml_write, absence tracker
- Codebase analysis: `src/events/toml_writer.rs` -- CandidateMapping, CandidateVenues (polymarket field is `Option<(String, String)>` for condition_id + token_id), build_candidate_table helper
- Codebase analysis: `src/config/events.rs` -- DiscoveryConfig (needs expiry_tolerance_days and polymarket_event_slugs), EventMapping, PolymarketMapping (condition_id + token_id)

### Secondary (MEDIUM confidence)
- Polymarket Gamma API documentation: `https://docs.polymarket.com/developers/gamma-markets-api/overview` -- confirmed endpoints /events and /markets, query parameters slug/active/limit/offset/tag_id
- Polymarket Gamma API documentation: `https://docs.polymarket.com/developers/gamma-markets-api/fetch-markets-guide` -- confirmed event-slug-based fetching strategy
- Polymarket Gamma API `/tags` endpoint -- confirmed tag ID 744 = "cryptocurrency" (but `forceHide: true` and filtering unreliable in testing)
- GitHub `CarlosIbCu/polymarket-kalshi-btc-arbitrage-bot` -- confirmed cross-venue BTC arbitrage pattern between Polymarket and Kalshi

### Tertiary (LOW confidence)
- Polymarket question text stability -- observed patterns "reach $X", "dip to $X", "hit $X" across two different event slugs, but no guarantee of format stability (permissionless market creation). **Needs ongoing monitoring via parse failure counter.**
- `groupItemTitle` field format -- observed "$150,000", "^100,000" patterns but inconsistent across events. **Do NOT rely on for parsing; use `question` field instead.**

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - zero new dependencies, all existing crates sufficient
- Architecture: HIGH - extends proven patterns (DiscoveredInstrument, cross-venue matching, CandidateMapping) with well-understood modifications (question parsing, FuzzyMatchKey, tolerance window)
- Pitfalls: HIGH - identified from live API testing (format variability, tag filtering unreliability, duplicate markets) and codebase analysis (token_id mapping, rate limiting)
- Polymarket API: MEDIUM - question format patterns verified from live data but stability not guaranteed by API contract

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (Polymarket question patterns may change; re-verify before implementation if delayed)
