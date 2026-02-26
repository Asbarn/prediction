# Phase 18: Discovery Infrastructure Hardening - Research

**Researched:** 2026-02-26
**Domain:** Rate limiting, API polling resilience, atomic file operations, state tracking
**Confidence:** HIGH

## Summary

Phase 18 hardens the existing `ContractLifecycleManager` discovery polling infrastructure with three targeted improvements: (1) shared rate limiters so discovery polls and feed/settlement components share the same venue rate budget, (2) consecutive-absence expiry guards that prevent a single missing API response from falsely expiring an instrument, and (3) batched TOML writes that accumulate all modifications within a single poll cycle into one atomic write instead of one write per candidate/expiry.

All three changes are refactoring and hardening of existing code -- no new external dependencies are needed. The `governor` crate (already at v0.8 in Cargo.toml) provides `VenueRateLimiter`. The `toml_edit` crate (already at v0.22) supports accumulating multiple mutations on a single `DocumentMut` before serializing. The consecutive-absence tracking requires a new `HashMap<(Venue, String), u32>` counter in `ContractLifecycleManager` with a configurable threshold in `DiscoveryConfig`.

**Primary recommendation:** Refactor `ContractLifecycleManager` to accept shared `VenueRateLimiter` instances from the pipeline, accumulate all TOML mutations on a single `DocumentMut` per cycle, and track per-instrument absence counts with a configurable consecutive-absence threshold (default 3).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DISC-02 | System polls Deribit and Kalshi APIs for new instruments with shared rate limiters and consecutive-absence expiry guards | Shared VenueRateLimiter pattern (already used by settlement checkers via pipeline_handles.venue_rate_limiters); consecutive-absence HashMap counter with configurable threshold; partial-response detection via instrument count comparison |
| LIFE-04 | System requires N consecutive absence polls before marking an instrument as expired (prevents false expirations from partial API responses) | AbsenceTracker HashMap<(Venue, String), u32> with configurable `consecutive_absence_threshold` (default 3) in DiscoveryConfig; partial API response detection (>20% count drop) skips expiry evaluation entirely |
| INTG-03 | All TOML writes use existing VenueRateLimiter and batch writes per poll cycle (not per-candidate) | Refactor poll_cycle to parse events.toml once into DocumentMut, apply all append_candidate and mark_expired mutations on that document, then write once via atomic_write at end of cycle |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| governor | 0.8 | Rate limiting via `VenueRateLimiter` | Already in Cargo.toml; wraps governor::RateLimiter with Arc for thread-safe sharing |
| toml_edit | 0.22 | Format-preserving TOML mutation | Already in Cargo.toml; `DocumentMut` supports multiple in-place mutations before serialization |
| tokio | 1 | Async runtime, intervals, RwLock | Already in Cargo.toml; all background tasks use tokio |
| reqwest | 0.12 | HTTP client for venue REST APIs | Already in Cargo.toml; used by discovery and settlement |
| metrics | 0.24 | Prometheus counters/gauges | Already in Cargo.toml; lifecycle metrics already exist |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | 0.1 | Structured logging for suspect responses | Already in Cargo.toml; warn-level logs for partial API responses |
| anyhow | 1.0 | Error propagation in discovery functions | Already in Cargo.toml |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Sharing existing VenueRateLimiter | New separate discovery rate limiters | Would exceed venue rate budgets when feeds + settlement + discovery all hit the same API |
| In-memory absence HashMap | Persisted absence counters in TOML | Unnecessary complexity; counters reset on restart is acceptable (fresh poll will re-check) |
| Partial-response heuristic (>20% drop) | Exact expected count from config | Venue instrument counts fluctuate legitimately; percentage-based heuristic is more robust |

**Installation:** No new crate dependencies. Zero additions to `Cargo.toml`.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── events/
│   ├── lifecycle.rs        # MODIFIED: accept shared rate limiters, add absence tracking, batch TOML writes
│   ├── discovery.rs        # MODIFIED: accept VenueRateLimiter parameter in discover_deribit/discover_kalshi
│   ├── toml_writer.rs      # MODIFIED: add batch mutation functions (append_candidates_to_toml, mark_expired_batch)
│   ├── registry.rs         # UNCHANGED
│   ├── risk.rs             # UNCHANGED
│   └── mod.rs              # UNCHANGED
├── config/
│   └── events.rs           # MODIFIED: add consecutive_absence_threshold and partial_response_threshold to DiscoveryConfig
├── feed/
│   └── reliability/
│       └── rate_limiter.rs  # UNCHANGED (VenueRateLimiter already Clone + Arc-wrapped)
└── main.rs                  # MODIFIED: pass pipeline rate limiters to ContractLifecycleManager
```

### Pattern 1: Shared Rate Limiter Injection
**What:** Pass existing `VenueRateLimiter` instances from `PipelineHandles.venue_rate_limiters` into `ContractLifecycleManager`, so discovery polls share the same rate budget as WebSocket feeds and settlement checkers.
**When to use:** Any component that makes HTTP/API calls to a venue that already has a rate limiter in the pipeline.
**Example:**
```rust
// In ContractLifecycleManager::new(), accept optional shared limiters
pub struct ContractLifecycleManager {
    // ... existing fields ...
    venue_rate_limiters: HashMap<Venue, VenueRateLimiter>,
}

// In discover_deribit, call limiter.wait() before each HTTP request
pub async fn discover_deribit(
    client: &reqwest::Client,
    base_url: &str,
    currencies: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all = Vec::new();
    for currency in currencies {
        if let Some(limiter) = rate_limiter {
            limiter.wait().await;
        }
        // ... existing HTTP request logic ...
    }
    Ok(all)
}
```
**Precedent in codebase:** Settlement checkers (DeribitResolutionChecker, KalshiResolutionChecker, PolymarketResolutionChecker) all accept and use shared VenueRateLimiter instances from `pipeline_handles.venue_rate_limiters`. See `src/main.rs:546-550` and `src/settlement/deribit.rs:69`.

### Pattern 2: Consecutive-Absence Tracking
**What:** Maintain a `HashMap<(Venue, String), u32>` counter that tracks how many consecutive poll cycles each known instrument has been absent from the API response. Only trigger expiry when the count reaches the configurable threshold (default 3).
**When to use:** Any scenario where a single missing data point should not trigger an irreversible state transition.
**Example:**
```rust
/// Tracks consecutive absence counts per (venue, instrument_id).
/// Reset to 0 when instrument appears in a poll response.
/// Incremented when instrument is absent from a poll response.
struct AbsenceTracker {
    counts: HashMap<(Venue, String), u32>,
    threshold: u32,  // default 3
}

impl AbsenceTracker {
    fn record_present(&mut self, venue: Venue, instrument_id: &str) {
        self.counts.remove(&(venue, instrument_id.to_string()));
    }

    fn record_absent(&mut self, venue: Venue, instrument_id: &str) -> bool {
        let count = self.counts
            .entry((venue, instrument_id.to_string()))
            .or_insert(0);
        *count += 1;
        *count >= self.threshold
    }

    fn is_expired(&self, venue: Venue, instrument_id: &str) -> bool {
        self.counts
            .get(&(venue, instrument_id.to_string()))
            .map_or(false, |&c| c >= self.threshold)
    }
}
```
**Precedent in codebase:** The `ActiveAlert.count` field in `src/alert/types.rs:185` tracks consecutive evaluations where a condition was true, following the same count-then-trigger pattern.

### Pattern 3: Batched TOML Mutations
**What:** Parse `events.toml` into a single `DocumentMut` at the start of the poll cycle, apply all candidate appends and expiry status changes to that one document, then serialize and write atomically once at the end.
**When to use:** Whenever a single logical operation produces multiple TOML modifications.
**Example:**
```rust
// In poll_cycle: read TOML once, mutate N times, write once
async fn poll_cycle(&self, ...) {
    // ... discovery logic produces candidates_to_append and events_to_expire ...

    let needs_write = !candidates_to_append.is_empty() || !events_to_expire.is_empty();
    if needs_write {
        let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
        let mut doc: DocumentMut = content.parse()?;

        // Apply all appends
        for candidate in &candidates_to_append {
            append_candidate_to_doc(&mut doc, candidate)?;
        }

        // Apply all expirations
        for event_id in &events_to_expire {
            mark_expired_in_doc(&mut doc, event_id)?;
        }

        // Single atomic write
        self.atomic_write(&doc.to_string()).await?;
    }
}
```
**Key insight:** The current code calls `append_candidate()` and `mark_expired()` inside loops, each of which reads the file, parses it, modifies it, and writes it back. With N candidates, this is N file reads + N file writes + N parses. The batched approach is 1 read + 1 parse + 1 write regardless of N.

### Pattern 4: Partial Response Detection
**What:** Track the instrument count from the previous successful poll for each venue. If the current response contains >20% fewer instruments, log it as suspect and skip expiry evaluation for that venue's instruments.
**When to use:** API responses that could return partial data due to pagination bugs, transient server issues, or rate limiting.
**Example:**
```rust
struct PreviousPollCounts {
    counts: HashMap<Venue, usize>,
}

impl PreviousPollCounts {
    fn is_suspect(&self, venue: Venue, current_count: usize) -> bool {
        if let Some(&prev) = self.counts.get(&venue) {
            if prev > 0 && current_count < prev * 80 / 100 {
                return true;  // >20% drop
            }
        }
        false
    }

    fn update(&mut self, venue: Venue, count: usize) {
        self.counts.insert(venue, count);
    }
}
```

### Anti-Patterns to Avoid
- **Per-component rate limiters for the same venue:** Creating a separate `VenueRateLimiter::new("deribit_discovery", 5)` when the feed pipeline already has a Deribit limiter. The venue's API does not distinguish between your components -- all requests count against the same global limit.
- **Single-absence expiry:** Marking an instrument expired because it was missing from ONE API response. API responses can be partial due to pagination bugs, transient errors, or server-side caching delays.
- **Write-per-mutation in a loop:** Calling `atomic_write()` inside a `for candidate in &candidates` loop. Each write triggers a file system event that the config watcher debouncer may process, causing unnecessary registry refreshes and potential race conditions.
- **Unbounded absence counter growth:** Never cleaning up absence counters for instruments that have been definitively expired. The HashMap should remove entries when the expiry is committed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rate limiting | Custom token bucket / sleep-based throttle | `governor::RateLimiter` via existing `VenueRateLimiter` | Governor handles burst, refill, and clock edge cases; battle-tested |
| Atomic file writes | Direct `fs::write` then hope for no crash | Existing `persistence::atomic::atomic_write()` or the async equivalent in lifecycle.rs | Write-to-temp + fsync + rename is the only crash-safe pattern; Windows fallback already handled |
| TOML format preservation | Manual string concatenation / regex | `toml_edit::DocumentMut` in-place mutation | Preserves comments, whitespace, formatting; handles escaping edge cases |
| Consecutive-count tracking | Manual if/else chains | Simple `HashMap<K, u32>` with threshold check | Clean, testable, O(1) lookup |

**Key insight:** All infrastructure needed for this phase already exists in the codebase. The work is wiring shared instances and adding a thin state-tracking layer, not building new infrastructure.

## Common Pitfalls

### Pitfall 1: Rate Limiter Not Actually Shared
**What goes wrong:** Discovery creates its own `VenueRateLimiter::new("deribit", 20)` even though `pipeline_handles.venue_rate_limiters` already has a Deribit limiter. Both limiters allow 20 req/s independently, so the venue sees up to 40 req/s and rate-limits or bans the client.
**Why it happens:** The `ContractLifecycleManager` is created before `PipelineHandles` is available (in `main.rs` the lifecycle manager is spawned at line 264, before the pipeline might be fully initialized). Or developer forgets that `VenueRateLimiter::clone()` clones the `Arc`, not the underlying limiter.
**How to avoid:** Pass `pipeline_handles.venue_rate_limiters` HashMap to `ContractLifecycleManager::new()`. The existing pattern from settlement checkers (main.rs:546-550) shows the correct approach: `.get(&Venue::Deribit).cloned()` to get a clone of the Arc-wrapped limiter. If pipeline hasn't started yet, wait for it or create the limiters first and share them.
**Warning signs:** HTTP 429 responses from Deribit/Kalshi in logs despite configured rate limits; multiple "deribit" rate limiter venues in debug output.

### Pitfall 2: Partial API Response Causing Mass False Expirations
**What goes wrong:** Kalshi returns a paginated response but a transient error on page 2 causes the function to return only page 1 results. Hundreds of instruments from later pages are "missing" and get marked expired.
**Why it happens:** Current `discover_kalshi()` breaks out of the pagination loop on cursor exhaustion but doesn't distinguish "no more pages" from "error fetching next page." If the HTTP request for a subsequent page fails, the error propagates up and the partial results are discarded (the `?` operator on line 218 of discovery.rs).
**How to avoid:** Two defenses: (1) consecutive-absence threshold means one bad response doesn't trigger expiry, and (2) the partial-response heuristic (>20% instrument count drop vs. previous poll) flags the response as suspect and skips expiry evaluation entirely for that venue.
**Warning signs:** Sudden spike in expired instruments; lifecycle_discovery_polls counter incrementing but instrument count drops sharply.

### Pitfall 3: TOML Write Race with Config Watcher
**What goes wrong:** Multiple `atomic_write()` calls within one poll cycle each produce a file rename event. The `notify-debouncer-mini` config watcher (debounce window typically 200-500ms) may fire mid-cycle, causing the registry to refresh with a partially-updated TOML file.
**Why it happens:** Current code calls `self.append_candidate()` (which does read-modify-write) inside a `for candidate in &new_candidates` loop. With 5 new candidates, that is 5 writes in rapid succession.
**How to avoid:** Batch all mutations into one `DocumentMut` and write once. The registry refresh at the end of `poll_cycle()` (line 467-469) already handles the single-write case correctly.
**Warning signs:** Registry refresh logs appearing mid-cycle; intermittent "event not found" errors right after discovery; config watcher "changed" events outnumbering poll cycles.

### Pitfall 4: Windows Atomic Rename Behavior
**What goes wrong:** On Windows, `std::fs::rename()` over an existing file can fail (unlike POSIX). The existing `persistence::atomic::atomic_write()` handles this with a remove-then-rename fallback, but the async version in `lifecycle.rs` (`tokio::fs::rename`) may not handle it consistently.
**Why it happens:** Windows file system semantics differ from POSIX. The async lifecycle.rs `atomic_write` method (line 487-492) does not have the Windows fallback that `persistence::atomic::atomic_write` has.
**How to avoid:** Either use the synchronous `persistence::atomic::atomic_write` via `tokio::task::spawn_blocking`, or add the same Windows fallback to the async version.
**Warning signs:** Intermittent "Access denied" or "file in use" errors on Windows during TOML writes.

### Pitfall 5: Absence Counter Memory Leak
**What goes wrong:** The absence counter HashMap grows unboundedly as instruments expire and new ones appear over weeks/months of operation.
**Why it happens:** Entries are added for each tracked instrument but never removed after expiry is committed.
**How to avoid:** When an instrument is definitively marked expired (absence count reached threshold), remove its entry from the absence tracker. Also clear entries for instruments that reappear (count reset to 0 means entry can be removed).
**Warning signs:** Slowly growing memory usage over long uptimes; absence tracker HashMap growing monotonically.

## Code Examples

### Example 1: Refactored ContractLifecycleManager with Shared Rate Limiters
```rust
// Source: codebase pattern from src/main.rs:546-550 (settlement checker wiring)
use std::collections::HashMap;
use crate::feed::reliability::VenueRateLimiter;
use crate::types::Venue;

pub struct ContractLifecycleManager {
    // ... existing fields ...
    /// Shared rate limiters from the feed pipeline.
    /// Discovery polls call limiter.wait() before each HTTP request.
    venue_rate_limiters: HashMap<Venue, VenueRateLimiter>,
    /// Tracks consecutive absences per (venue, instrument_id).
    absence_tracker: AbsenceTracker,
    /// Previous successful poll instrument counts for partial-response detection.
    previous_poll_counts: HashMap<Venue, usize>,
}

impl ContractLifecycleManager {
    pub fn new(
        // ... existing params ...
        venue_rate_limiters: HashMap<Venue, VenueRateLimiter>,
    ) -> Self {
        let consecutive_absence_threshold = discovery_config
            .consecutive_absence_threshold
            .unwrap_or(3);
        Self {
            // ... existing fields ...
            venue_rate_limiters,
            absence_tracker: AbsenceTracker::new(consecutive_absence_threshold),
            previous_poll_counts: HashMap::new(),
        }
    }
}
```

### Example 2: Rate-Limited Discovery Call
```rust
// Source: pattern from src/settlement/deribit.rs:69 (limiter.wait() before HTTP)
pub async fn discover_deribit(
    client: &reqwest::Client,
    base_url: &str,
    currencies: &[String],
    rate_limiter: Option<&VenueRateLimiter>,
) -> anyhow::Result<Vec<DiscoveredInstrument>> {
    let mut all = Vec::new();
    for currency in currencies {
        // Respect shared venue rate limit before HTTP call
        if let Some(limiter) = rate_limiter {
            limiter.wait().await;
        }
        let url = format!("{}/api/v2/public/get_instruments", base_url);
        let resp = client
            .get(&url)
            .query(&[("currency", currency.as_str()), ("kind", "option")])
            .send()
            .await?;
        // ... existing parsing logic ...
    }
    Ok(all)
}
```

### Example 3: Batched TOML Write Functions
```rust
// Source: pattern from src/events/toml_writer.rs (existing append/mark functions)
use toml_edit::{value, DocumentMut, Table};

/// Append multiple candidates to a DocumentMut in-place (no file I/O).
pub fn append_candidates_to_doc(
    doc: &mut DocumentMut,
    candidates: &[CandidateMapping],
) -> anyhow::Result<()> {
    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    for candidate in candidates {
        let mut entry = Table::new();
        entry["id"] = value(&candidate.id);
        entry["asset"] = value(&candidate.asset);
        // ... same field population as existing append_candidate_to_toml ...
        entry["approved"] = value(false);
        entry["status"] = value("active");
        entry["discovered_at"] = value(chrono::Utc::now().to_rfc3339());
        // ... venue sub-tables ...
        events.push(entry);
    }
    Ok(())
}

/// Mark multiple events as expired in a DocumentMut in-place (no file I/O).
pub fn mark_expired_batch_in_doc(
    doc: &mut DocumentMut,
    event_ids: &[String],
) -> anyhow::Result<()> {
    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    for target_id in event_ids {
        for i in 0..events.len() {
            if let Some(table) = events.get_mut(i) {
                if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                    if id == target_id {
                        table["status"] = value("expired");
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
```

### Example 4: DiscoveryConfig Extensions
```rust
// Source: extends existing src/config/events.rs DiscoveryConfig
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
    // ... existing fields ...

    /// Number of consecutive polls where an instrument must be absent
    /// before it is marked expired. Default 3.
    #[serde(default = "default_consecutive_absence_threshold")]
    pub consecutive_absence_threshold: u32,

    /// Percentage drop in instrument count (vs previous poll) that triggers
    /// a "suspect partial response" warning. Default 0.2 (20%).
    #[serde(default = "default_partial_response_threshold")]
    pub partial_response_threshold: f64,
}

fn default_consecutive_absence_threshold() -> u32 { 3 }
fn default_partial_response_threshold() -> f64 { 0.2 }
```

### Example 5: Poll Cycle with Batched Writes and Absence Tracking
```rust
// Source: refactored from src/events/lifecycle.rs poll_cycle()
async fn poll_cycle(&self, ...) {
    let mut all_discovered: Vec<DiscoveredInstrument> = Vec::new();
    let mut candidates_to_append: Vec<CandidateMapping> = Vec::new();
    let mut events_to_expire: Vec<String> = Vec::new();

    // 1. Discover instruments (with rate limiting)
    if should_poll_deribit {
        let limiter = self.venue_rate_limiters.get(&Venue::Deribit);
        match discover_deribit(&self.http_client, &base_url, &currencies, limiter).await {
            Ok(instruments) => {
                let count = instruments.len();
                // Partial-response check
                if self.previous_poll_counts.is_suspect(Venue::Deribit, count) {
                    tracing::warn!(
                        venue = "deribit",
                        previous = ?self.previous_poll_counts.get(&Venue::Deribit),
                        current = count,
                        "suspect partial API response -- skipping expiry evaluation for Deribit"
                    );
                    // Still use instruments for candidate discovery, but skip expiry
                } else {
                    self.previous_poll_counts.update(Venue::Deribit, count);
                    // Process absences for expiry (see step 4)
                }
                all_discovered.extend(instruments);
            }
            Err(e) => { /* existing error handling */ }
        }
    }

    // 2-3. Cross-venue matching (unchanged logic)
    // ... find_cross_venue_candidates, filter_new_candidates ...
    // Collect candidates_to_append instead of writing immediately

    // 4. Expiry detection with consecutive-absence tracking
    for mapping in &all_mappings {
        if mapping.status == LifecycleStatus::Expired { continue; }
        // For each venue instrument in the mapping...
        if let Some(ref deribit) = mapping.venues.deribit {
            if deribit_was_polled && !deribit_is_suspect {
                if discovered_ids.contains(&(Venue::Deribit, deribit.instrument.as_str())) {
                    self.absence_tracker.record_present(Venue::Deribit, &deribit.instrument);
                } else {
                    let should_expire = self.absence_tracker.record_absent(
                        Venue::Deribit, &deribit.instrument
                    );
                    if should_expire {
                        events_to_expire.push(mapping.id.clone());
                    }
                }
            }
        }
    }

    // 5. Single batched TOML write
    if !candidates_to_append.is_empty() || !events_to_expire.is_empty() {
        match self.batched_toml_write(&candidates_to_append, &events_to_expire).await {
            Ok(()) => { /* log success */ }
            Err(e) => { tracing::error!(error = %e, "batched TOML write failed"); }
        }
        self.refresh_registry().await;
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Per-component rate limiters | Shared VenueRateLimiter via Arc | v1.0 Phase 3 (feeds) + v1.1 Phase 16 (settlement) | Discovery is the last component not yet sharing; this phase fixes that |
| Single-absence expiry | Consecutive-absence with configurable threshold | This phase (v1.2 Phase 18) | Eliminates false expirations from transient API issues |
| Write-per-mutation TOML | Batched single write per poll cycle | This phase (v1.2 Phase 18) | Eliminates write races with config watcher; reduces I/O by factor of N |

**Deprecated/outdated:**
- The current expiry detection in `lifecycle.rs` lines 311-357 uses single-absence logic -- an instrument missing from one poll immediately triggers expiry. This will be replaced by the consecutive-absence tracker.
- The current per-candidate `self.append_candidate()` calls in the loop at lines 274-294 will be replaced by batched collection followed by a single write.
- The async `atomic_write` in lifecycle.rs (lines 487-492) lacks the Windows fallback present in `persistence::atomic::atomic_write`. Should be unified.

## Open Questions

1. **Rate limiter creation ordering -- RESOLVED**
   - What we know: `pipeline_handles` is created at main.rs:207-215 via `.await?`, which completes before the lifecycle manager creation at main.rs:264. The `venue_rate_limiters` HashMap is populated by the pipeline and available on `pipeline_handles`.
   - Resolution: The ordering is correct. `pipeline_handles.venue_rate_limiters` can be cloned (or moved) into the lifecycle manager constructor. The settlement checkers already demonstrate this pattern at main.rs:546-550 using `.get(&Venue::Deribit).cloned()`. For the lifecycle manager, pass the entire HashMap (or clone it) since discovery needs all venues.

2. **Absence tracker persistence across restarts**
   - What we know: The absence tracker is in-memory (HashMap). On restart, all counters reset to 0.
   - What's unclear: Whether this is acceptable -- after restart, the first poll will show all instruments as "present" (resetting counters) or instruments that expired during downtime won't be in the response (starting fresh at count=1).
   - Recommendation: In-memory is acceptable. After restart, the system needs N consecutive absences before expiring, which is the correct defensive behavior. No persistence needed.

3. **Partial-response threshold tuning**
   - What we know: 20% drop is the success criteria threshold.
   - What's unclear: Whether Deribit or Kalshi instrument counts fluctuate by >20% legitimately (e.g., batch expiry of monthly options).
   - Recommendation: Make the threshold configurable via `partial_response_threshold` in DiscoveryConfig. Use 20% as default but allow operators to tune. Also, the threshold should only apply when `previous_count > 0` (skip on first poll).

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/feed/reliability/rate_limiter.rs` -- VenueRateLimiter implementation with Arc<GovernorLimiter>
- Codebase analysis: `src/events/lifecycle.rs` -- current poll_cycle with per-mutation writes and single-absence expiry
- Codebase analysis: `src/events/toml_writer.rs` -- toml_edit DocumentMut mutation patterns
- Codebase analysis: `src/events/discovery.rs` -- discover_deribit and discover_kalshi functions (no rate limiter param)
- Codebase analysis: `src/main.rs:546-550` -- settlement checker shared rate limiter wiring pattern
- Codebase analysis: `src/feed/pipeline.rs:120-136` -- rate limiter creation and HashMap storage
- Codebase analysis: `src/persistence/atomic.rs` -- synchronous atomic_write with Windows fallback
- Codebase analysis: `src/config/events.rs` -- DiscoveryConfig struct (needs new fields)
- Codebase analysis: `src/alert/types.rs:184-185` -- consecutive count pattern precedent

### Secondary (MEDIUM confidence)
- governor crate v0.8: `RateLimiter::direct(quota)` with `until_ready().await` -- verified via existing usage in codebase
- toml_edit v0.22: `DocumentMut` supports multiple `as_array_of_tables_mut()` mutations before `to_string()` -- verified via existing `append_candidate_to_toml` and `mark_expired_in_toml` patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all dependencies already in Cargo.toml and actively used
- Architecture: HIGH - all patterns are extensions of existing codebase patterns (shared rate limiters from settlement, consecutive counting from alerts, TOML mutation from toml_writer)
- Pitfalls: HIGH - identified from direct analysis of current code deficiencies (single-absence expiry, per-mutation writes, Windows rename fallback gap)

**Research date:** 2026-02-26
**Valid until:** 2026-03-28 (stable domain, no external API changes expected)
