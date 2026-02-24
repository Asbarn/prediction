# Phase 5: Event Mapping - Research

**Researched:** 2026-02-22
**Domain:** Cross-venue instrument registry, basis risk quantification, contract lifecycle discovery, TOML-driven config with auto-discovery
**Confidence:** HIGH (architecture and patterns well-understood; venue APIs documented; existing codebase provides clear integration points)

## Summary

Phase 5 builds a config-driven event registry that maps equivalent instruments across Polymarket, Kalshi, and Deribit, with each mapping carrying a quantified basis risk score and lifecycle status. The existing codebase already has a minimal `EventsConfig` (in `config/events.rs`) with `EventMapping` structs and an `events.toml` file -- Phase 5 significantly extends this schema to support: approval status (approved/pending), basis risk breakdown per factor, lifecycle states (active/expiring/expired), and discovery metadata. The system also adds a contract lifecycle manager that periodically polls each venue's REST API to discover new instruments and detect expired ones.

The primary architectural challenge is the hybrid auto-discovery pattern: the discovery module polls venue APIs, proposes candidate matches by comparing structured fields (asset + strike + expiry + direction), and appends them to `events.toml` with `approved = false`. The user reviews and flips to `approved = true`. This requires a TOML write-back capability that the current `toml` crate (used for read-only deserialization) does not cleanly support for preserving formatting. The `toml_edit` crate (format-preserving TOML parser/editor) is the standard solution for programmatic TOML modification.

The basis risk scoring system quantifies three independent factors: settlement time risk (linear with temporal mismatch between Deribit's Friday 08:00 UTC settlement and prediction market resolution), settlement source risk (categorical weights for index-vs-oracle pairs, predefined in config), and resolution criteria risk (qualitative differences in settlement methodology). These produce a structured breakdown plus a composite score.

**Primary recommendation:** Extend the existing `EventsConfig` with approval/lifecycle/risk fields, add `toml_edit` for format-preserving write-back to events.toml, build venue discovery modules using `reqwest` (already in Cargo.toml) to poll REST instrument-list APIs, and implement a periodic `ContractLifecycleManager` task that runs on a configurable interval.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Hybrid approach: auto-discovery proposes candidate matches, user reviews and sets `approved = true` in events.toml
- Match fields: asset + strike + expiry + direction (all four must align)
- Strike matching is exact after normalization -- no fuzzy tolerance band
- Unapproved candidates are visible to downstream but carry a `pending` flag -- useful for monitoring potential opportunities before committing
- Discovery writes candidates to events.toml with `approved = false` plus a structured log entry
- Structured breakdown: separate scores per factor (settlement_time_risk, source_risk, criteria_risk) plus a composite score
- Settlement time risk: hours matter -- score linearly with time difference (even a few hours is meaningful)
- Settlement source risk: categorical weights per source-pair (index-index = 0, index-oracle = 0.5, oracle-oracle = 0.2, etc.) -- predefined in config
- Risk is annotation only -- all approved mappings generate signals regardless of risk level. No automatic suppression.
- Periodic REST polling of each venue's instrument list API (no feed-driven detection)
- Poll interval configurable in TOML per venue -- user tunes based on experience
- Auto-append candidates to events.toml with `approved = false` plus log entry
- Flag novel/unmatched instruments separately so user can spot new opportunity types (new assets, event types)
- Configurable warning thresholds in TOML: multiple tiers (e.g., 'caution' at 48h, 'warning' at 24h, 'critical' at 6h) each with different flags
- Deribit expiry rolls create a new candidate mapping with `approved = false` -- user reviews before it goes live (approved status does NOT carry over)
- Expired mappings archived in events.toml with `status = 'expired'` -- kept for historical reference, excluded from runtime queries
- Near-expiry warnings both annotate the mapping AND inflate the settlement_time_risk component -- downstream gets both the flag and a quantitative signal

### Claude's Discretion
- Exact TOML schema design for events.toml
- REST polling implementation details per venue
- Composite risk score aggregation formula
- Internal data structures for the runtime registry

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EVNT-01 | Config-driven event registry (TOML) maps equivalent instruments across Polymarket, Kalshi, and Deribit using structured fields (asset, strike, expiry, direction) | Extended EventsConfig schema with all four structured fields, approval status, and venue-specific instrument IDs. toml_edit for write-back. Existing events.toml already has basic structure to build on. |
| EVNT-02 | Settlement basis analyzer quantifies per-mapping: expiry/settlement time differences, settlement source differences, resolution criteria differences, producing a basis_risk_score | BasisRiskScore struct with three component scores + composite. Linear time-difference scoring, categorical source-pair weights from config, qualitative criteria scoring. All annotated on each mapping. |
| EVNT-03 | Expiry alignment validation quantifies temporal mismatch between options expiry (Deribit Friday 08:00 UTC) and prediction market resolution as basis risk | Deribit settlement at Friday 08:00 UTC via 30-min TWAP (07:30-08:00). Prediction markets resolve at various times. settlement_time_risk computed as hours of mismatch, scored linearly. |
| EVNT-04 | Contract lifecycle manager continuously discovers new contracts, detects expiring/expired ones, and handles Deribit expiry rolls -- not just at startup | Periodic REST polling via reqwest to Deribit `public/get_instruments`, Kalshi `GET /markets`, Polymarket Gamma API `GET /markets`. Configurable per-venue poll interval. Discovery proposes candidates, expiry detection archives. Deribit rolls create new candidates. |
| EVNT-05 | Contracts approaching expiry receive special handling flags (pricing character change, liquidity warnings, elevated settlement risk) | Multi-tier warning thresholds (caution/warning/critical) configurable in TOML. ExpiryWarning flags on mappings. settlement_time_risk inflation near expiry. Downstream consumers read both flags and inflated risk score. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| toml_edit | 0.22+ | Format-preserving TOML read/write for events.toml auto-discovery write-back | Preserves comments, formatting, ordering when appending new entries; standard for cargo-edit and similar tools |
| toml | 0.8 (existing) | TOML deserialization for config loading (read path) | Already in Cargo.toml; used for initial config load |
| reqwest | 0.12 (existing) | HTTP REST client for venue instrument-list API polling | Already in Cargo.toml with json + rustls-tls features |
| chrono | 0.4 (existing) | Timestamp arithmetic for expiry calculations and settlement time comparisons | Already in Cargo.toml; DateTime/Duration operations |
| serde | 1.0 (existing) | Serialization/deserialization of config and runtime structures | Already in Cargo.toml |
| rust_decimal | 1.40 (existing) | Precise strike price representation and comparison | Already in Cargo.toml; exact decimal matching for strikes |
| tokio | 1 (existing) | Async runtime for periodic polling tasks | Already in Cargo.toml |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| metrics | 0.24 (existing) | Lifecycle metrics (discovery count, expiry warnings, poll latency) | Instrument counters and gauges for observability |
| tracing | 0.1 (existing) | Structured logging for discovery events and lifecycle transitions | All discovery/expiry/warning events |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| toml_edit for write-back | Serialize via toml crate (full rewrite) | Loses comments, formatting, manual edits; unacceptable for user-edited config file |
| toml_edit for write-back | Append raw text to file | Fragile; no TOML validation on write; breaks easily |
| Per-venue REST polling | WebSocket subscription to instrument listing changes | No venue provides real-time instrument listing changes via WS; REST is the only option |
| reqwest for REST calls | Custom HTTP client | reqwest already in Cargo.toml, battle-tested, async, JSON support built-in |

**Installation:**
```toml
# Add to Cargo.toml [dependencies]
toml_edit = "0.22"
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── config/
│   ├── events.rs          # MODIFY -- Extend EventMapping with approval, lifecycle, risk fields
│   ├── validation.rs      # MODIFY -- Add event mapping validation (expiry date parsing, venue count)
│   └── mod.rs             # MODIFY -- Re-export new types
├── events/                # NEW -- Event mapping module
│   ├── mod.rs             # Module root, re-exports
│   ├── registry.rs        # Runtime event registry (in-memory index, queryable)
│   ├── discovery.rs       # Auto-discovery: poll venues, propose candidates
│   ├── risk.rs            # Basis risk scoring (settlement time, source, criteria)
│   ├── lifecycle.rs       # ContractLifecycleManager: periodic poll, expiry detection, roll handling
│   └── toml_writer.rs     # Format-preserving TOML write-back for candidate appending
├── feed/
│   └── pipeline.rs        # MODIFY -- Wire event registry into pipeline (event_id annotation on MarketSnapshot)
└── types/
    └── ids.rs             # Existing EventId -- no changes needed
```

### Pattern 1: Runtime Event Registry (In-Memory Index)
**What:** A queryable in-memory registry built from events.toml at startup, refreshed on config reload and discovery. Provides O(1) lookups by (venue, instrument_id) -> EventMapping and by event_id -> all venue legs.
**When to use:** Every time a MarketSnapshot arrives and needs its event_id annotated, or when downstream needs to find all legs of an event for spread calculation.

```rust
// In-memory registry with dual-index lookup
pub struct EventRegistry {
    /// All mappings (including pending and expired for reference)
    mappings: Vec<EventMapping>,
    /// Index: (Venue, InstrumentId) -> index into mappings vec
    instrument_index: HashMap<(Venue, InstrumentId), usize>,
    /// Index: EventId -> index into mappings vec
    event_index: HashMap<EventId, usize>,
}

impl EventRegistry {
    /// Build from loaded EventsConfig
    pub fn from_config(config: &EventsConfig) -> Self { /* ... */ }

    /// Lookup event by venue-specific instrument ID
    /// Used in pipeline to annotate MarketSnapshot.event_id
    pub fn lookup_by_instrument(&self, venue: Venue, instrument_id: &InstrumentId) -> Option<&EventMapping> {
        self.instrument_index.get(&(venue, instrument_id.clone()))
            .map(|&idx| &self.mappings[idx])
    }

    /// Get all active, approved mappings (excludes expired)
    pub fn active_approved(&self) -> impl Iterator<Item = &EventMapping> {
        self.mappings.iter()
            .filter(|m| m.approved && m.status == LifecycleStatus::Active)
    }

    /// Refresh from updated config (after discovery appends or config reload)
    pub fn refresh(&mut self, config: &EventsConfig) { /* rebuild indexes */ }
}
```

### Pattern 2: Venue Discovery Module (Per-Venue REST Polling)
**What:** Each venue has a discovery function that calls the REST API to list instruments, parses the response, and returns normalized instrument descriptors. A central coordinator calls each venue's discoverer on a configurable interval.
**When to use:** The ContractLifecycleManager calls this on each polling cycle.

```rust
// Normalized instrument descriptor from any venue
pub struct DiscoveredInstrument {
    pub venue: Venue,
    pub instrument_id: String,
    pub asset: String,        // "BTC", "ETH"
    pub strike: Decimal,      // Normalized strike price
    pub expiry: NaiveDate,    // Expiry date
    pub direction: Direction, // Above/Below (Call/Put mapped to direction)
    pub is_active: bool,
    pub raw_expiry_timestamp: i64, // Original millisecond timestamp for precise comparison
}

// Deribit discovery: GET /api/v2/public/get_instruments?currency=BTC&kind=option
async fn discover_deribit(client: &reqwest::Client, base_url: &str) -> Result<Vec<DiscoveredInstrument>> {
    let resp = client.get(format!("{}/api/v2/public/get_instruments", base_url))
        .query(&[("currency", "BTC"), ("kind", "option")])
        .send().await?
        .json::<DeribitInstrumentsResponse>().await?;
    // Map each instrument to DiscoveredInstrument
    // option_type "call" -> Direction::Above, "put" -> Direction::Below
    // Parse instrument_name for strike and expiry, or use response fields directly
}

// Kalshi discovery: GET /trade-api/v2/markets?status=open&series_ticker=KXBTC
async fn discover_kalshi(client: &reqwest::Client, config: &KalshiConfig) -> Result<Vec<DiscoveredInstrument>> {
    // Paginate through results using cursor
    // Parse ticker, floor_strike/cap_strike, close_time
}

// Polymarket discovery: GET https://gamma-api.polymarket.com/markets
async fn discover_polymarket(client: &reqwest::Client) -> Result<Vec<DiscoveredInstrument>> {
    // Paginate using offset/limit
    // Filter for crypto/BTC-related markets
    // Parse conditionId, tokens, end_date_iso
}
```

### Pattern 3: Candidate Matching Algorithm
**What:** Compare discovered instruments across venues using structured fields. All four fields (asset, strike, expiry, direction) must match exactly after normalization for a candidate match.
**When to use:** After each discovery poll, when processing newly discovered instruments.

```rust
// Match key: the four structured fields that must align
#[derive(Hash, Eq, PartialEq, Clone)]
pub struct MatchKey {
    pub asset: String,
    pub strike: Decimal,    // Exact after normalization
    pub expiry: NaiveDate,
    pub direction: Direction,
}

impl MatchKey {
    pub fn from_discovered(d: &DiscoveredInstrument) -> Self {
        MatchKey {
            asset: d.asset.to_uppercase(),
            strike: d.strike,
            expiry: d.expiry,
            direction: d.direction,
        }
    }
}

// Group discovered instruments by match key across venues
fn find_cross_venue_candidates(
    instruments: &[DiscoveredInstrument],
) -> HashMap<MatchKey, Vec<&DiscoveredInstrument>> {
    let mut groups: HashMap<MatchKey, Vec<&DiscoveredInstrument>> = HashMap::new();
    for inst in instruments {
        let key = MatchKey::from_discovered(inst);
        groups.entry(key).or_default().push(inst);
    }
    // Only return groups with instruments from 2+ venues
    groups.retain(|_, v| {
        let venues: HashSet<_> = v.iter().map(|i| i.venue).collect();
        venues.len() >= 2
    });
    groups
}
```

### Pattern 4: Format-Preserving TOML Append
**What:** Use `toml_edit` to append new event mapping entries to events.toml without destroying existing formatting, comments, or manual edits.
**When to use:** When auto-discovery proposes new candidate mappings.

```rust
use toml_edit::{DocumentMut, value, Array, Item, Table};

fn append_candidate_to_toml(
    toml_content: &str,
    candidate: &EventMapping,
) -> Result<String> {
    let mut doc = toml_content.parse::<DocumentMut>()?;

    // Get or create the [[events]] array
    let events = doc["events"].as_array_of_tables_mut()
        .ok_or_else(|| anyhow::anyhow!("events.toml missing [[events]] array"))?;

    // Build new table entry
    let mut entry = toml_edit::Table::new();
    entry["id"] = value(&candidate.id);
    entry["asset"] = value(&candidate.asset);
    entry["strike"] = value(&candidate.strike);
    entry["direction"] = value(&candidate.direction);
    entry["expiry"] = value(&candidate.expiry);
    entry["approved"] = value(false);
    entry["status"] = value("active");
    entry["discovered_at"] = value(chrono::Utc::now().to_rfc3339());
    // ... add venue-specific sub-tables ...

    events.push(entry);
    Ok(doc.to_string())
}
```

### Pattern 5: Basis Risk Scoring
**What:** Compute a structured risk breakdown for each mapping with three independent component scores plus a composite.
**When to use:** When building or refreshing the event registry, and when near-expiry inflation applies.

```rust
pub struct BasisRiskScore {
    /// Hours of settlement time difference, scored linearly
    pub settlement_time_risk: f64,
    /// Categorical weight for source-pair mismatch
    pub source_risk: f64,
    /// Qualitative assessment of criteria differences
    pub criteria_risk: f64,
    /// Composite: weighted sum of components
    pub composite: f64,
}

impl BasisRiskScore {
    pub fn compute(
        deribit_expiry: DateTime<Utc>,    // Friday 08:00 UTC
        prediction_resolution: DateTime<Utc>, // Varies per market
        source_pair: SourcePair,          // e.g., IndexOracle
        criteria_diff: f64,               // 0.0 = identical, 1.0 = very different
        weights: &RiskWeights,            // From config
    ) -> Self {
        let time_diff_hours = (deribit_expiry - prediction_resolution)
            .num_minutes().abs() as f64 / 60.0;
        let settlement_time_risk = time_diff_hours * weights.time_per_hour;

        let source_risk = match source_pair {
            SourcePair::IndexIndex => 0.0,
            SourcePair::IndexOracle => 0.5,
            SourcePair::OracleOracle => 0.2,
            // ... other pairs from config
        };

        let composite = weights.time_weight * settlement_time_risk
            + weights.source_weight * source_risk
            + weights.criteria_weight * criteria_diff;

        BasisRiskScore { settlement_time_risk, source_risk, criteria_risk: criteria_diff, composite }
    }

    /// Inflate settlement_time_risk near expiry (EVNT-05)
    pub fn with_expiry_inflation(&self, hours_to_expiry: f64, thresholds: &ExpiryThresholds) -> Self {
        let inflation = thresholds.inflation_factor(hours_to_expiry);
        BasisRiskScore {
            settlement_time_risk: self.settlement_time_risk * inflation,
            composite: self.composite * inflation, // Re-compute would be more precise
            ..*self
        }
    }
}
```

### Pattern 6: Contract Lifecycle Manager
**What:** A tokio task that runs on a configurable interval, polls venue APIs, discovers new instruments, detects expiry transitions, and updates events.toml and the runtime registry.
**When to use:** Runs continuously from startup.

```rust
pub struct ContractLifecycleManager {
    registry: Arc<RwLock<EventRegistry>>,
    http_client: reqwest::Client,
    config: LifecycleConfig,
    events_toml_path: PathBuf,
    cancel: CancellationToken,
}

impl ContractLifecycleManager {
    pub async fn run(self) {
        let mut interval = tokio::time::interval(
            Duration::from_secs(self.config.poll_interval_secs)
        );

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(e) = self.poll_cycle().await {
                        tracing::error!(error = %e, "lifecycle poll cycle failed");
                    }
                }
            }
        }
    }

    async fn poll_cycle(&self) -> Result<()> {
        // 1. Discover instruments from each venue
        // 2. Find new cross-venue candidates
        // 3. Detect expired instruments
        // 4. Handle Deribit expiry rolls (new contract with approved=false)
        // 5. Update events.toml via toml_edit
        // 6. Refresh runtime registry
        // 7. Apply expiry warnings to near-expiry mappings
    }
}
```

### Anti-Patterns to Avoid
- **Rewriting events.toml with `toml::to_string`:** Destroys comments, formatting, and manual edits. Use `toml_edit` for write-back.
- **Fuzzy strike matching:** User explicitly decided on exact matching after normalization. Do not add tolerance bands.
- **Automatic approval of discovered candidates:** Always write with `approved = false`. User must review.
- **Suppressing signals based on risk score:** Risk is annotation-only. All approved mappings generate signals regardless of risk level.
- **Single poll interval for all venues:** Each venue should have an independently configurable poll interval.
- **Blocking the main pipeline during discovery:** Discovery polling runs in its own async task, never blocking the snapshot processing pipeline.
- **Carrying over approval on Deribit rolls:** When a Deribit contract rolls to a new expiry, the new mapping is a fresh candidate with `approved = false`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML format-preserving edit | String manipulation or full rewrite | `toml_edit` crate | Preserves comments, formatting, ordering; handles edge cases in TOML spec |
| HTTP REST client | Raw TCP/hyper | `reqwest` (already in Cargo.toml) | Async, JSON, TLS, connection pooling built-in |
| Decimal strike comparison | f64 equality | `rust_decimal` (already in Cargo.toml) | Exact decimal arithmetic; no floating-point comparison pitfalls |
| Date/time arithmetic | Manual day counting | `chrono` (already in Cargo.toml) | Handles timezones, DST, leap years; Duration arithmetic |
| Periodic task scheduling | Manual sleep loops | `tokio::time::interval` | Handles drift correctly; integrates with select! |
| Deribit instrument name parsing | Regex | Structured split on `-` separator | Instrument names follow strict `{ASSET}-{DDMMMYY}-{STRIKE}-{C|P}` format; split is simpler and faster |

**Key insight:** The complexity in this phase is in the domain logic (matching, risk scoring, lifecycle state transitions), not in infrastructure. Every infrastructure need (HTTP, TOML, time, decimals) is already solved by existing dependencies.

## Common Pitfalls

### Pitfall 1: TOML Write-Back Destroys User Comments
**What goes wrong:** Using `toml::to_string` to serialize the full EventsConfig back to file destroys all comments and manual formatting the user added.
**Why it happens:** The `toml` crate's serializer produces minimal output with no comments.
**How to avoid:** Use `toml_edit::DocumentMut` to parse the existing file, append new entries, and write back. This preserves all existing content.
**Warning signs:** User's carefully annotated events.toml loses all comments after first auto-discovery cycle.

### Pitfall 2: Deribit Instrument Name Date Parsing
**What goes wrong:** Failing to parse the `DDMMMYY` date format in Deribit instrument names (e.g., "27JUN25" = June 27, 2025).
**Why it happens:** The format uses 2-digit year and 3-letter month abbreviation, which is non-standard for most date parsing libraries.
**How to avoid:** Use `chrono::NaiveDate::parse_from_str` with format `%d%b%y` (e.g., "27JUN25" parses as 2025-06-27). Alternatively, use the `expiration_timestamp` field from the API response directly (milliseconds since epoch).
**Warning signs:** Expiry date mismatches causing failed cross-venue matching.

### Pitfall 3: Kalshi Strike Price Extraction
**What goes wrong:** Kalshi market tickers (e.g., "KXBTCD-25JUN30-T100000") encode strike price differently than Deribit, and extracting it requires understanding the ticker format.
**Why it happens:** Kalshi uses series-specific ticker formats; crypto price range markets encode the strike in the ticker, but the API also provides `floor_strike` and `cap_strike` fields.
**How to avoid:** Use the structured `floor_strike`/`cap_strike` fields from the API response, not string parsing of the ticker. For binary yes/no markets, the relevant strike is in the market description or event metadata.
**Warning signs:** Strike mismatches preventing cross-venue matching.

### Pitfall 4: Polymarket Markets Are Not Structured Like Options
**What goes wrong:** Assuming Polymarket markets have explicit strike prices and expiry dates in structured fields.
**Why it happens:** Polymarket markets are free-form prediction markets; the "strike" equivalent is embedded in the market question (e.g., "Will BTC be above $100,000 on June 30?").
**How to avoid:** For Polymarket, the structured fields (asset, strike, expiry, direction) must be extracted from the market question/description or configured manually in events.toml. Auto-discovery for Polymarket may need to match based on keywords in the question or rely on user-configured condition IDs.
**Warning signs:** Auto-discovery fails to match Polymarket markets to Deribit options because fields are not directly comparable.

### Pitfall 5: Settlement Time Mismatch Is Not Just Date-Based
**What goes wrong:** Comparing only expiry dates (NaiveDate) and concluding risk is zero when dates match.
**Why it happens:** Deribit settles at exactly Friday 08:00 UTC using a 30-minute TWAP. Prediction markets may resolve at different times on the same day, or at varying times based on event outcome.
**How to avoid:** Use full DateTime<Utc> for settlement time comparison, not just NaiveDate. Store Deribit's exact settlement timestamp (from `expiration_timestamp` API field). For prediction markets, use the configured or discovered resolution time. Compute risk as hours difference, not days.
**Warning signs:** Two mappings with same date show zero settlement time risk despite 8+ hours of actual difference.

### Pitfall 6: Discovery Poll Overwhelming Venue Rate Limits
**What goes wrong:** Polling all instruments too frequently (e.g., every 30 seconds) hits rate limits, causing auth failures or temporary bans.
**Why it happens:** Instrument list APIs return large payloads (Deribit has hundreds of options per currency); frequent polling is unnecessary because new instruments are listed infrequently.
**How to avoid:** Default poll interval should be conservative (e.g., 300 seconds / 5 minutes). Per-venue configurable. Deribit `public/get_instruments` is a public endpoint (no auth, generous rate limit). Kalshi requires auth and has tighter limits. Polymarket Gamma API has moderate limits.
**Warning signs:** 429 status codes or connection throttling during discovery polls.

### Pitfall 7: Concurrent File Writes on Discovery
**What goes wrong:** Multiple discovery tasks or config reloader writing to events.toml simultaneously, causing corruption.
**Why it happens:** Discovery runs periodically, config reloader watches for changes, and both may touch events.toml.
**How to avoid:** Use a single writer task that serializes all events.toml modifications through a channel. Config reloader should detect discovery-triggered changes (file watcher fires) but not interfere. Use file locking or a mutex on the write path.
**Warning signs:** events.toml becomes corrupted or has partial entries after concurrent operations.

### Pitfall 8: Deribit Expiry Roll Detection
**What goes wrong:** When a Deribit monthly option expires and the next month's is listed, the system either misses the new contract or incorrectly carries over the old mapping's approval.
**Why it happens:** The old instrument becomes inactive and a new one appears; the system needs to detect the relationship between them.
**How to avoid:** When a mapped Deribit instrument changes from active to expired, search for a new instrument with the same asset/strike/direction but a later expiry. Create this as a new candidate mapping with `approved = false`. Mark the old mapping as `status = 'expired'`.
**Warning signs:** Gaps in coverage after monthly expiry; old expired instruments still appearing in active queries.

## Code Examples

### Extended events.toml Schema
```toml
# Event mapping configuration
# Discovery appends new candidates with approved = false
# User reviews and sets approved = true to activate

# Settlement source risk weights (predefined)
[risk_weights]
time_per_hour = 0.05       # Risk per hour of settlement time difference
time_weight = 0.4          # Weight of time risk in composite
source_weight = 0.4        # Weight of source risk in composite
criteria_weight = 0.2      # Weight of criteria risk in composite

# Source pair risk categories
[risk_weights.source_pairs]
index_index = 0.0
index_oracle = 0.5
oracle_oracle = 0.2

# Discovery configuration
[discovery]
deribit_poll_interval_secs = 300     # Poll Deribit every 5 minutes
kalshi_poll_interval_secs = 600      # Poll Kalshi every 10 minutes
polymarket_poll_interval_secs = 600  # Poll Polymarket every 10 minutes
deribit_currencies = ["BTC"]         # Currencies to discover
kalshi_series_tickers = ["KXBTC"]    # Kalshi series to monitor

# Expiry warning thresholds
[[expiry_thresholds]]
name = "caution"
hours_before_expiry = 48
flags = ["pricing_character_change"]
risk_inflation_factor = 1.2

[[expiry_thresholds]]
name = "warning"
hours_before_expiry = 24
flags = ["pricing_character_change", "liquidity_warning"]
risk_inflation_factor = 1.5

[[expiry_thresholds]]
name = "critical"
hours_before_expiry = 6
flags = ["pricing_character_change", "liquidity_warning", "elevated_settlement_risk"]
risk_inflation_factor = 2.0

# --- Event Mappings ---

[[events]]
id = "BTC-100K-2025-06-27"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-06-27"
approved = true
status = "active"    # active | expiring | expired

# Settlement metadata for risk scoring
[events.settlement]
deribit_settlement_time = "2025-06-27T08:00:00Z"  # Friday 08:00 UTC TWAP
deribit_settlement_source = "deribit_index"         # 30-min TWAP of Deribit BTC Index
polymarket_resolution_source = "oracle"             # UMA optimistic oracle
kalshi_resolution_source = "index"                  # Kalshi's reference price

[events.venues.deribit]
instrument = "BTC-27JUN25-100000-C"

[events.venues.polymarket]
condition_id = "0xabc..."
token_id = "12345"

[events.venues.kalshi]
ticker = "KXBTCD-25JUN30-T100000"

# Auto-discovered candidate (not yet approved)
[[events]]
id = "BTC-120K-2025-07-25"
asset = "BTC"
strike = "120000"
direction = "above"
expiry = "2025-07-25"
approved = false
status = "active"
discovered_at = "2026-02-22T14:30:00Z"

[events.venues.deribit]
instrument = "BTC-25JUL25-120000-C"

# No Polymarket or Kalshi match found yet -- venues with no match are omitted
```

### Deribit Instrument Discovery via REST API
```rust
// Source: Deribit API docs (https://docs.deribit.com/api-reference/market-data/public-get_instruments)
#[derive(Debug, Deserialize)]
struct DeribitInstrumentsResponse {
    result: Vec<DeribitInstrumentInfo>,
}

#[derive(Debug, Deserialize)]
struct DeribitInstrumentInfo {
    instrument_name: String,
    kind: String,                   // "option", "future", etc.
    base_currency: String,          // "BTC", "ETH"
    strike: Option<f64>,            // Strike price (options only)
    option_type: Option<String>,    // "call" or "put" (options only)
    expiration_timestamp: i64,      // Milliseconds since epoch
    is_active: bool,
    settlement_period: String,      // "week", "month", "perpetual"
    creation_timestamp: i64,
}

async fn discover_deribit_instruments(
    client: &reqwest::Client,
    base_url: &str,
    currency: &str,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    // Public endpoint, no auth needed
    let url = format!("{}/api/v2/public/get_instruments", base_url);
    let resp: DeribitInstrumentsResponse = client
        .get(&url)
        .query(&[("currency", currency), ("kind", "option")])
        .send().await?
        .json().await?;

    let instruments = resp.result.into_iter()
        .filter(|i| i.is_active)
        .filter_map(|i| {
            let strike = Decimal::from_f64_retain(i.strike?)?;
            let expiry_ts = i.expiration_timestamp;
            let expiry_dt = DateTime::from_timestamp_millis(expiry_ts)?;
            let expiry_date = expiry_dt.date_naive();
            let direction = match i.option_type.as_deref()? {
                "call" => Direction::Above,
                "put" => Direction::Below,
                _ => return None,
            };
            Some(DiscoveredInstrument {
                venue: Venue::Deribit,
                instrument_id: i.instrument_name,
                asset: i.base_currency,
                strike,
                expiry: expiry_date,
                direction,
                is_active: i.is_active,
                raw_expiry_timestamp: expiry_ts,
            })
        })
        .collect();

    Ok(instruments)
}
```

### Kalshi Market Discovery via REST API
```rust
// Source: Kalshi API docs (https://docs.kalshi.com/api-reference/market/get-markets)
#[derive(Debug, Deserialize)]
struct KalshiMarketsResponse {
    markets: Vec<KalshiMarketInfo>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KalshiMarketInfo {
    ticker: String,
    event_ticker: String,
    title: String,
    subtitle: Option<String>,
    status: String,           // "open", "closed", "settled"
    close_time: String,       // ISO 8601 datetime
    floor_strike: Option<f64>,
    cap_strike: Option<f64>,
}

async fn discover_kalshi_markets(
    client: &reqwest::Client,
    config: &KalshiConfig,
    api_key_id: &str,
    private_key: &RsaPrivateKey,
    series_tickers: &[String],
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all_instruments = Vec::new();

    for series in series_tickers {
        let mut cursor = None;
        loop {
            let timestamp_ms = chrono::Utc::now().timestamp_millis();
            let path = "/trade-api/v2/markets";
            let signature = sign_kalshi_request(private_key, timestamp_ms, "GET", path)?;

            let mut req = client.get(format!("{}{}", config.rest_url, path))
                .header("KALSHI-ACCESS-KEY", api_key_id)
                .header("KALSHI-ACCESS-SIGNATURE", &signature)
                .header("KALSHI-ACCESS-TIMESTAMP", timestamp_ms.to_string())
                .query(&[("series_ticker", series.as_str()), ("status", "open"), ("limit", "200")]);

            if let Some(ref c) = cursor {
                req = req.query(&[("cursor", c.as_str())]);
            }

            let resp: KalshiMarketsResponse = req.send().await?.json().await?;

            for market in &resp.markets {
                if let Some(inst) = parse_kalshi_to_discovered(market) {
                    all_instruments.push(inst);
                }
            }

            match resp.cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }
    }

    Ok(all_instruments)
}
```

### Polymarket Market Discovery via Gamma API
```rust
// Source: Polymarket Gamma API docs (https://docs.polymarket.com/developers/gamma-markets-api/get-markets)
#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(rename = "conditionId")]
    condition_id: String,
    question: String,
    #[serde(rename = "endDateIso")]
    end_date_iso: Option<String>,
    active: bool,
    closed: bool,
    tokens: Vec<GammaToken>,
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GammaToken {
    token_id: String,
    outcome: String,  // "Yes" or "No"
}

async fn discover_polymarket_markets(
    client: &reqwest::Client,
    gamma_api_url: &str,
) -> anyhow::Result<Vec<GammaMarket>> {
    let mut all_markets = Vec::new();
    let mut offset = 0;
    let limit = 100;

    loop {
        let resp: Vec<GammaMarket> = client
            .get(format!("{}/markets", gamma_api_url))
            .query(&[("limit", &limit.to_string()), ("offset", &offset.to_string())])
            .query(&[("active", "true")])
            .send().await?
            .json().await?;

        let count = resp.len();
        all_markets.extend(resp);

        if count < limit {
            break;
        }
        offset += limit;
    }

    Ok(all_markets)
}
```

### MarketSnapshot Event ID Annotation
```rust
// In the pipeline, after a MarketSnapshot is produced by a processor,
// annotate it with the event_id from the registry before sending downstream.
fn annotate_snapshot(
    mut snapshot: MarketSnapshot,
    registry: &EventRegistry,
) -> MarketSnapshot {
    snapshot.event_id = registry
        .lookup_by_instrument(snapshot.venue, &snapshot.instrument_id)
        .filter(|m| m.status != LifecycleStatus::Expired)
        .map(|m| EventId::new(&m.id));
    snapshot
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Static config files only | Auto-discovery with user approval | Recent pattern in crypto infra | Reduces manual config burden; ensures new contracts are not missed |
| toml crate for read/write | toml_edit for format-preserving write-back | toml_edit mature since 2023 | Critical for maintaining user comments and formatting in config files |
| Deribit integer-based prices | Unchanged (Deribit API stable) | N/A | Deribit option instrument API response format is stable |
| Kalshi integer prices | Kalshi migrating to `_dollars`/`_fp` fields | Jan 2026 deadline, legacy deprecated Feb 2026 | Discovery response parsing should use `_dollars` fields where available |

**Deprecated/outdated:**
- Kalshi integer-based price fields in REST responses: Deprecated as of Feb 2026. Use `*_dollars` and `*_fp` fields.
- Manual-only instrument mapping: The auto-discovery pattern (poll + propose + approve) is the modern approach for systems tracking multiple venues.

## Open Questions

1. **Polymarket Structured Field Extraction**
   - What we know: Polymarket markets are free-form prediction markets; asset, strike, expiry, and direction are not provided as structured API fields.
   - What's unclear: How to reliably extract these from the market question/description text. For example, "Will BTC be above $100,000 on June 30, 2025?" contains all four fields but requires NLP-like parsing.
   - Recommendation: For v1, auto-discovery for Polymarket focuses on matching condition_ids that the user has pre-configured or semi-configured. Full text-based extraction is deferred. Polymarket entries in events.toml are primarily user-authored, with discovery used only to detect deactivation/resolution of existing markets.

2. **Kalshi Crypto Market Structure**
   - What we know: Kalshi has crypto price markets under series like "KXBTC" and "KXBTCMAXY". Market tickers encode date and sometimes price.
   - What's unclear: Whether all crypto price markets have `floor_strike`/`cap_strike` fields, or whether some are yes/no binaries without explicit strikes.
   - Recommendation: Use the `floor_strike`/`cap_strike` fields when available. For yes/no binary markets, extract the strike from the market title/subtitle using simple pattern matching. Test with live API during implementation.

3. **Discovery Rate Limiting Across Venues**
   - What we know: Each venue has different rate limits. Deribit public endpoints are generous. Kalshi requires auth for all endpoints. Polymarket Gamma API has moderate limits.
   - What's unclear: Exact rate limits for Gamma API `GET /markets` and Kalshi `GET /markets` when polling frequently.
   - Recommendation: Start with conservative intervals (5-10 minutes). The `governor` rate limiter (already in Cargo.toml) can be used for REST polling. Monitor 429 responses and adjust.

4. **TOML File Write Atomicity**
   - What we know: `toml_edit` produces a string; we write it to disk. If the process crashes mid-write, the file could be corrupted.
   - What's unclear: Whether this is a real risk given the file is typically small (<10KB).
   - Recommendation: Write to a temporary file, then atomically rename. This is a standard pattern (`tempfile` crate or manual rename). The `toml_edit` output is validated before write.

## Sources

### Primary (HIGH confidence)
- [Deribit public/get_instruments](https://docs.deribit.com/api-reference/market-data/public-get_instruments) -- Full endpoint docs: parameters (currency, kind, expired), response fields (instrument_name, strike, expiration_timestamp, option_type, kind, is_active, settlement_period)
- [Deribit Settlement docs](https://support.deribit.com/hc/en-us/articles/29734325712413-Settlement) -- Settlement at 08:00 UTC, 30-minute TWAP from 07:30-08:00
- [Kalshi Get Markets](https://docs.kalshi.com/api-reference/market/get-markets) -- Full endpoint docs: parameters (event_ticker, series_ticker, status, cursor), response fields (ticker, close_time, floor_strike, cap_strike, status)
- [Kalshi Get Series List](https://docs.kalshi.com/api-reference/market/get-series-list) -- Series structure, settlement_sources, frequency
- [Polymarket Gamma API Get Markets](https://docs.polymarket.com/developers/gamma-markets-api/get-markets) -- Market fields: conditionId, tokens, endDateIso, active, closed
- [Polymarket Gamma Structure](https://docs.polymarket.com/developers/gamma-markets-api/gamma-structure) -- Market/event data model, conditionId, questionId, token IDs
- [toml_edit crate](https://crates.io/crates/toml_edit) -- Format-preserving TOML parser/editor, version 0.22+

### Secondary (MEDIUM confidence)
- [Deribit Contract Introduction Policy](https://support.deribit.com/hc/en-us/articles/25944688876957-Contract-Introduction-Policy) -- When new options series are listed
- [Kalshi crypto markets](https://kalshi.com/category/crypto/btc) -- Live market examples showing ticker format and market types
- [toml_edit docs.rs](https://docs.rs/toml_edit) -- DocumentMut API, Table manipulation, Array of Tables
- [toml_edit GitHub](https://github.com/toml-rs/toml/tree/main/crates/toml_edit) -- Source and examples

### Tertiary (LOW confidence)
- Polymarket structured field extraction from question text -- no official API support for this; requires custom parsing logic
- Kalshi `floor_strike`/`cap_strike` availability on crypto markets -- confirmed in API schema but not verified on live crypto series specifically

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- only new dependency is toml_edit; all other libraries already in Cargo.toml
- Architecture (registry, lifecycle manager): HIGH -- clear patterns from existing codebase; per-venue polling is straightforward reqwest usage
- Basis risk scoring: HIGH -- domain logic is well-defined by user decisions; scoring formula is arithmetic, no external dependencies
- Deribit discovery: HIGH -- public/get_instruments is well-documented with structured response fields
- Kalshi discovery: MEDIUM -- API documented but crypto-specific market structure (floor_strike/cap_strike) needs live verification
- Polymarket discovery: MEDIUM -- Gamma API documented but structured field extraction from free-form markets is inherently limited
- TOML write-back: HIGH -- toml_edit is mature and widely used for this exact purpose
- Pitfalls: HIGH -- identified from API docs, existing codebase patterns, and domain knowledge

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (30 days -- venue APIs are stable; Kalshi dollar-field migration is the main time-sensitive item)
