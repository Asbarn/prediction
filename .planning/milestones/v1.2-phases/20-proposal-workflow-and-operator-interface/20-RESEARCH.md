# Phase 20: Proposal Workflow and Operator Interface - Research

**Researched:** 2026-02-27
**Domain:** TOML config mutation, structured logging, Prometheus metrics, config reload validation
**Confidence:** HIGH

## Summary

Phase 20 bridges the discovery pipeline (Phases 18-19) to operator-visible output. The system already discovers cross-venue candidates and writes them via `append_candidates_to_doc` in batched TOML writes. Phase 20 must: (1) ensure the writing path sets `approved = false` with proper atomic writes (already done -- PROP-01 is largely implemented), (2) emit WARN-level structured tracing logs per new proposal (PROP-02), (3) expose Prometheus gauges/counters for proposal metrics (PROP-03), and (4) add validation logic that runs on config reload to reject approved mappings that fail safety checks (PROP-04).

The codebase already has the TOML writing infrastructure (`toml_edit::DocumentMut`, `append_candidates_to_doc`, `build_candidate_table`, `atomic_write`), the `metrics` crate wired to Prometheus, and the `tracing` structured logging stack with JSON file output. The config reload path uses `notify`-based file watching with a `watch::Sender<AppConfig>` channel, reloading via `load_config()` which calls `validate_config()`. Phase 20 extends `validate_config()` with new safety checks for approved mappings and adds proposal-specific logging and metrics to the lifecycle poll cycle.

**Primary recommendation:** Extend the existing lifecycle poll cycle with WARN-level proposal logging and proposal metrics, then add approved-mapping validation rules to `validate_config()` in `src/config/validation.rs`. No new crates or architectural changes needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PROP-01 | System writes candidate mappings to events.toml with approved = false via atomic TOML writes preserving formatting and comments | **Already implemented.** `build_candidate_table` in `toml_writer.rs` sets `approved = false`, `append_candidates_to_doc` appends to `DocumentMut`, and `batched_toml_write` in `lifecycle.rs` uses atomic write-to-tmp-then-rename. Verify and add any missing edge case handling. |
| PROP-02 | System emits structured tracing log with event_id, matched venues, instruments, expiry dates, and confidence when a new candidate is proposed | Current lifecycle.rs logs at INFO level with partial fields. Must upgrade to WARN level with all required fields: event_id, matched venue names, per-venue instrument identifiers, expiry dates, and ExpiryConfidence score. Use `tracing::warn!` with named fields for JSON-structured output. |
| PROP-03 | System exposes Prometheus gauges for pending proposal count and total proposals counter | Add `metrics::gauge!("proposals_pending")` that is set each poll cycle by counting unapproved active mappings in the registry. Add `metrics::counter!("proposals_total")` that increments for each new proposal written. The `metrics` crate + `metrics-exporter-prometheus` are already installed and configured. |
| PROP-04 | System validates approved mappings on config reload: at least 2 venue instruments, instruments still active, expiry not passed | Extend `validate_config()` in `validation.rs` with three new checks for approved mappings: (1) venue count >= 2, (2) instrument activity (requires discovery data -- see Architecture section for approach), (3) expiry date not in the past. Invalid mappings produce warning logs and are rejected (config reload fails, keeping previous config -- existing behavior). |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `toml_edit` | 0.22 | Format-preserving TOML document manipulation | Already in Cargo.toml; `DocumentMut` preserves comments and whitespace during mutations |
| `tracing` | 0.1 | Structured logging with field-based JSON output | Already configured with dual stdout (human) + file (JSON) layers; WARN level visible to operator |
| `metrics` | 0.24 | Prometheus-compatible gauge/counter emission | Already wired to `metrics-exporter-prometheus` 0.18 with HTTP scrape endpoint |
| `chrono` | 0.4 | Date parsing for expiry validation | Already used throughout for `NaiveDate::parse_from_str` |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `notify` | 8 | File-system watching for config reload | Already drives ConfigReloader; no changes needed |
| `tokio` | 1 | Async runtime for atomic file I/O | Already in use; `tokio::fs` for async TOML read/write |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `metrics::gauge!` for pending count | Manual atomic counter | `metrics` macros are zero-cost when recorder installed; hand-rolled adds complexity for no gain |
| `validate_config` extension | Separate validation pass | Extending existing function keeps all validation in one place; matches established pattern |

**No new dependencies required.** All libraries are already in Cargo.toml.

## Architecture Patterns

### Recommended Project Structure

No new files needed. Changes are modifications to existing files:

```
src/
  config/
    validation.rs    # Add approved-mapping validation checks (PROP-04)
  events/
    lifecycle.rs     # Add WARN-level proposal logging + metrics (PROP-02, PROP-03)
    toml_writer.rs   # Verify PROP-01 completeness (already implemented)
  events/
    registry.rs      # Add pending_count() helper method (PROP-03)
```

### Pattern 1: Structured WARN-Level Proposal Log

**What:** Each new proposal emits a single `tracing::warn!` with all required fields as named structured parameters.
**When to use:** When a candidate passes through `filter_new_candidates_fuzzy` and is about to be written to TOML.
**Example:**

```rust
// In lifecycle.rs poll_cycle, where candidates_to_append is populated
for candidate in &candidates_to_append {
    let venue_names: Vec<&str> = [
        candidate.venues.deribit.as_ref().map(|_| "deribit"),
        candidate.venues.polymarket.as_ref().map(|_| "polymarket"),
        candidate.venues.kalshi.as_ref().map(|_| "kalshi"),
    ]
    .into_iter()
    .flatten()
    .collect();

    tracing::warn!(
        event_id = %candidate.id,
        matched_venues = ?venue_names,
        deribit_instrument = ?candidate.venues.deribit,
        polymarket_instrument = ?candidate.venues.polymarket,
        kalshi_instrument = ?candidate.venues.kalshi,
        expiry = %candidate.expiry,
        confidence = %candidate.expiry_confidence,
        "new proposal: candidate mapping discovered"
    );

    metrics::counter!("proposals_total").increment(1);
}
```

This replaces the existing `tracing::info!` candidate log in lifecycle.rs (around line 442-455) which already logs partial fields. The upgrade is: (a) WARN level instead of INFO, (b) all required fields explicitly named, (c) all venue instrument identifiers included.

### Pattern 2: Prometheus Gauge for Pending Count

**What:** After each poll cycle, set a gauge to the count of unapproved active mappings.
**When to use:** At the end of the poll cycle, after registry refresh.
**Example:**

```rust
// After registry refresh in poll_cycle
let registry = self.registry.read().await;
let pending_count = registry.all_mappings()
    .iter()
    .filter(|m| !m.approved && m.status == LifecycleStatus::Active)
    .count();
metrics::gauge!("proposals_pending").set(pending_count as f64);
drop(registry);
```

### Pattern 3: Approved Mapping Validation on Config Reload

**What:** In `validate_config()`, check each approved mapping for safety: >= 2 venues, valid expiry, not expired.
**When to use:** During config reload (file watcher triggers `load_config()` which calls `validate_config()`).
**Design choice:** There are two approaches for handling invalid approved mappings:

**Option A (Recommended): Reject the entire config reload.** If any approved mapping fails validation, `validate_config()` returns `Err(ConfigError::Validation)`. The config reload handler in `reload.rs` already logs the error and keeps the previous config. This is the safest approach -- an operator cannot accidentally activate a bad mapping.

**Option B (Alternative): Warn and demote.** Log a warning but accept the config, with the mapping demoted to `approved = false`. This is more complex (requires mutating the config during validation) and violates the principle that config files are the source of truth.

**Recommendation: Option A.** It matches the existing validation pattern and ensures operator safety. The operator must fix the config file to proceed.

**Example:**

```rust
// In validate_config(), after existing validation
for event in &events.events {
    if !event.approved {
        continue; // Only validate approved mappings
    }

    // Check 1: At least 2 venue instruments
    let venue_count = [
        event.venues.deribit.is_some(),
        event.venues.polymarket.is_some(),
        event.venues.kalshi.is_some(),
    ]
    .iter()
    .filter(|&&v| v)
    .count();

    if venue_count < 2 {
        return Err(ConfigError::Validation {
            file: "events.toml".to_string(),
            message: format!(
                "approved event '{}' has only {} venue(s) -- at least 2 required",
                event.id, venue_count
            ),
        });
    }

    // Check 2: Expiry not in the past
    if let Ok(expiry_date) = NaiveDate::parse_from_str(&event.expiry, "%Y-%m-%d") {
        let today = chrono::Utc::now().date_naive();
        if expiry_date < today {
            return Err(ConfigError::Validation {
                file: "events.toml".to_string(),
                message: format!(
                    "approved event '{}' has expired (expiry {} < today {})",
                    event.id, expiry_date, today
                ),
            });
        }
    }
}
```

**Note on "instruments still active on their venues" (PROP-04 requirement):** This check cannot be performed synchronously during config validation because it requires async REST API calls to each venue. Two practical approaches:

1. **Validation-time check (not feasible):** `validate_config()` is called synchronously from the `notify` file watcher thread. Adding async venue API calls here would require restructuring the reload path.

2. **Post-reload async validation (recommended):** After the config reload propagates through the `watch` channel, the `ContractLifecycleManager` performs an async venue activity check on the next poll cycle. Newly approved mappings whose instruments are not found in the latest discovery data get flagged with a WARN log. This leverages the existing `DiscoveredInstrument.is_active` field and the absence-tracking system. The validation still happens; it just happens asynchronously after reload rather than synchronously blocking the reload.

3. **Hybrid approach:** The synchronous validation checks venue count and expiry (fast, local). The async check verifies instrument activity (slow, requires API calls). Both are clearly documented to the operator via logs.

**Recommendation: Hybrid approach (#3).** Synchronous validation catches configuration errors immediately (rejecting the reload). Async validation catches stale instruments on the next poll cycle with warning logs.

### Anti-Patterns to Avoid

- **Blocking async calls in validate_config:** The validation function is called from a sync `std::thread` context (the notify watcher). Never use `block_on` or similar -- it will deadlock the tokio runtime.
- **Per-candidate file writes:** Never write to events.toml once per candidate. Always batch mutations into a single `DocumentMut` and write once (already correctly implemented in `batched_toml_write`).
- **Logging at DEBUG for operator-critical events:** Proposals must be at WARN level to ensure operator visibility in stdout (filtered to INFO by default). WARN guarantees visibility.
- **Mutable config during validation:** Do not modify `EventsConfig` inside `validate_config()`. The function should remain a pure check that returns Ok or Err.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML formatting preservation | Custom TOML serializer | `toml_edit::DocumentMut` | Preserves comments, whitespace, and manual edits; battle-tested |
| Prometheus metrics | Custom HTTP endpoint with counters | `metrics::gauge!` / `metrics::counter!` | Already wired to exporter; zero-cost macros, auto-discovered by Prometheus |
| Structured JSON logs | Custom JSON serializer | `tracing::warn!` with named fields | Already configured with JSON file layer; fields become searchable JSON keys |
| Atomic file write | Custom write-and-sync | Existing `atomic_write` in `persistence/atomic.rs` or the lifecycle `atomic_write` method | Both implementations handle Windows remove-before-rename |

**Key insight:** All four PROP requirements are extensions of existing infrastructure. No new crates, no new architectural patterns. The risk is in getting the details right (log levels, field names, validation ordering), not in building new systems.

## Common Pitfalls

### Pitfall 1: Config Reload Validation Breaking Existing Configs

**What goes wrong:** Adding venue-count validation for approved mappings could break existing configs where approved events have only 1 venue (e.g., the BTC-100K example in the current events.toml has 3 venues, but the BTC-120K unapproved example has only 1).
**Why it happens:** New validation rules applied retroactively to all events, including previously valid ones.
**How to avoid:** Only apply the 2-venue minimum check to `approved = true` mappings. Unapproved candidates correctly have 1 venue (discovered from a single venue before cross-matching). The existing `events.toml` already has `approved = false` for the single-venue candidate, so this is safe.
**Warning signs:** Config reload failures immediately after deploying Phase 20 code.

### Pitfall 2: Expiry Validation Timezone Sensitivity

**What goes wrong:** Comparing expiry date against "today" without considering timezone can cause a mapping to be rejected as expired on the same day it should still be active.
**Why it happens:** `chrono::Utc::now().date_naive()` returns today's UTC date, but Deribit settlement is at 08:00 UTC. An event expiring "today" should still be valid until 08:00 UTC.
**How to avoid:** Use `Utc::now().date_naive()` for the comparison (already the project pattern). Consider events expiring "today" as valid (use `<` not `<=` for the past-expiry check). This gives a full day buffer.
**Warning signs:** Events rejected as expired on their actual expiry date.

### Pitfall 3: Metric Name Collisions

**What goes wrong:** Choosing metric names that collide with existing metrics, causing confusing Prometheus data.
**Why it happens:** The codebase has 30+ metrics; names like `lifecycle_candidates_discovered` already exist.
**How to avoid:** Use a distinct `proposals_` prefix for all new Phase 20 metrics: `proposals_pending`, `proposals_total`. The existing `lifecycle_candidates_discovered` counter can remain as it tracks a different thing (candidates found during matching, before deduplication).
**Warning signs:** Unexpected values in Prometheus dashboard; same metric name with different labels.

### Pitfall 4: WARN Log Spam for Existing Proposals

**What goes wrong:** Every poll cycle re-logs proposals that already exist in events.toml, flooding the operator's console.
**Why it happens:** If the proposal logging is placed before the `filter_new_candidates_fuzzy` deduplication step.
**How to avoid:** Log ONLY after filtering (i.e., only for truly new proposals that will be written to TOML). The current code already filters first, so place logging after filtering but before the TOML write. The existing `for candidate in &candidates_to_append` loop is the correct location.
**Warning signs:** WARN logs repeating the same event_id every poll cycle.

### Pitfall 5: Windows File Watcher Race with Atomic Write

**What goes wrong:** On Windows, the atomic write (remove + rename) produces two file system events. The `notify_debouncer_mini` with 500ms debounce may trigger two config reloads.
**Why it happens:** Windows cannot atomic-rename over an existing file; the code does `remove_file` then `rename`. Each produces a separate FS event.
**How to avoid:** The debouncer already handles this with 500ms aggregation. No additional changes needed. The existing lifecycle.rs `atomic_write` uses this pattern successfully.
**Warning signs:** Double config reload log messages after a single poll cycle write.

## Code Examples

Verified patterns from the existing codebase:

### Existing Proposal Writing (PROP-01 - Already Implemented)

```rust
// Source: src/events/toml_writer.rs - build_candidate_table
fn build_candidate_table(candidate: &CandidateMapping) -> Table {
    let mut entry = Table::new();
    entry["id"] = value(&candidate.id);
    entry["asset"] = value(&candidate.asset);
    entry["strike"] = value(&candidate.strike);
    entry["direction"] = value(candidate.direction.to_string());
    entry["expiry"] = value(&candidate.expiry);
    entry["approved"] = value(false);  // <-- PROP-01: always false
    entry["status"] = value("active");
    entry["discovered_at"] = value(chrono::Utc::now().to_rfc3339());
    entry["expiry_confidence"] = value(candidate.expiry_confidence.to_string());
    // ... venue sub-tables ...
    entry
}
```

### Existing Batched Write (PROP-01 - Already Implemented)

```rust
// Source: src/events/lifecycle.rs - batched_toml_write
async fn batched_toml_write(
    &self,
    candidates: &[CandidateMapping],
    expire_ids: &[String],
) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
    let mut doc: DocumentMut = content.parse()?;

    if !candidates.is_empty() {
        append_candidates_to_doc(&mut doc, candidates)?;
    }
    if !expire_ids.is_empty() {
        mark_expired_batch_in_doc(&mut doc, expire_ids)?;
    }

    self.atomic_write(&doc.to_string()).await
}
```

### Existing Metrics Pattern

```rust
// Source: src/events/lifecycle.rs - existing counter usage
metrics::counter!("lifecycle_discovery_polls", "venue" => "deribit").increment(1);
metrics::counter!("lifecycle_candidates_discovered").increment(1);
metrics::gauge!("lifecycle_expiry_warnings").set(warning_count as f64);
```

### Existing Validation Pattern (PROP-04 Extension Point)

```rust
// Source: src/config/validation.rs - existing per-event validation
for event in &events.events {
    let has_venue = event.venues.deribit.is_some()
        || event.venues.polymarket.is_some()
        || event.venues.kalshi.is_some();

    if !has_venue {
        return Err(ConfigError::Validation {
            file: "events.toml".to_string(),
            message: format!(
                "event '{}' has no venue mappings configured",
                event.id
            ),
        });
    }
    // ... more checks ...
}
```

### Existing Config Reload Pattern

```rust
// Source: src/config/reload.rs - error handling preserves previous config
match super::load_config(&config_dir) {
    Ok(new_config) => {
        tracing::info!("config reloaded successfully");
        let _ = config_tx.send(new_config);
    }
    Err(e) => {
        tracing::error!(
            error = %e,
            "config reload failed, keeping previous"
        );
    }
}
```

### Registry Pending Count Helper (New for PROP-03)

```rust
// Proposed addition to src/events/registry.rs
pub fn pending_count(&self) -> usize {
    self.mappings
        .iter()
        .filter(|m| !m.approved && m.status == LifecycleStatus::Active)
        .count()
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single-candidate TOML writes | Batched `DocumentMut` mutations | Phase 18 | Eliminates write/file-watcher race conditions |
| INFO-level discovery logs | WARN-level proposal logs (Phase 20) | Phase 20 | Ensures operator visibility in default stdout filter |
| No config reload validation for approved events | Venue count + expiry validation (Phase 20) | Phase 20 | Prevents activating unsafe mappings |

**Deprecated/outdated:**
- `append_candidate_to_toml` (single-candidate version): Still exists for backward compatibility but `append_candidates_to_doc` (batch version) is preferred. No need to remove.

## Open Questions

1. **Instrument Activity Check Scope**
   - What we know: PROP-04 requires checking "instruments still active on their venues." The synchronous validation path cannot make async API calls. The discovery data from the most recent poll cycle contains `is_active` flags.
   - What's unclear: Whether "instruments still active" should block config reload entirely, or just produce a warning.
   - Recommendation: Hybrid approach -- synchronous validation checks venue count and expiry (blocking). Async instrument activity check happens in the next poll cycle via the existing absence tracker. Document this clearly in operator logs.

2. **SIGHUP vs File Watcher for Config Reload**
   - What we know: The success criteria mention "On config reload (SIGHUP)" but the actual implementation uses file-system watching via the `notify` crate, not Unix signals. Windows (the current development platform) does not support SIGHUP.
   - What's unclear: Whether SIGHUP support is required or if file-watcher-triggered reload is sufficient.
   - Recommendation: The file watcher IS the config reload mechanism. Treat "SIGHUP" in the requirements as shorthand for "config reload trigger." The file watcher already achieves the same effect cross-platform. No SIGHUP handler needed.

3. **Validation Timing for Newly Approved Mappings**
   - What we know: An operator approves a mapping by editing events.toml (changing `approved = false` to `approved = true`). The file watcher detects the change and triggers validation.
   - What's unclear: Whether the validation should distinguish between "mapping was already approved" and "mapping is being newly approved" (i.e., should existing approved mappings be re-validated on every reload?).
   - Recommendation: Validate ALL approved mappings on every reload. This is simpler, safer, and catches cases where an expiry date passes while the system is running. The cost is negligible (iterate events list, parse dates).

## Sources

### Primary (HIGH confidence)
- **Codebase analysis:** `src/events/toml_writer.rs`, `src/events/lifecycle.rs`, `src/config/validation.rs`, `src/config/reload.rs`, `src/config/events.rs`, `src/events/registry.rs`, `src/events/discovery.rs`, `src/metrics_export/mod.rs`, `src/logging/layers.rs`
- **Existing events.toml:** `config/events.toml` -- confirmed structure with `approved`, `status`, `discovered_at` fields
- **Existing Cargo.toml:** Confirmed `toml_edit = "0.22"`, `tracing = "0.1"`, `metrics = "0.24"`, `metrics-exporter-prometheus = "0.18"`, `chrono = "0.4"`

### Secondary (MEDIUM confidence)
- **Phase 18/19 plans:** Confirmed batch write pattern, `CandidateMapping` structure, `ExpiryConfidence` type, fuzzy matching integration in poll cycle
- **Project decisions (STATE.md):** `approved = false` is non-negotiable safety mechanism; batched TOML writes per poll cycle

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in use; no new dependencies needed
- Architecture: HIGH - All patterns are extensions of existing code; no new architectural concepts
- Pitfalls: HIGH - Based on direct codebase analysis of existing patterns and known Windows quirks
- PROP-01 status: HIGH - Already implemented, needs verification only
- PROP-02 implementation: HIGH - Simple upgrade of existing log statement
- PROP-03 implementation: HIGH - Standard `metrics::gauge!`/`counter!` pattern used throughout
- PROP-04 validation: MEDIUM - Synchronous venue-count and expiry checks are straightforward; async instrument-activity check requires careful design

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain; no external API changes expected)
