# Phase 21: Lifecycle Management and Integration - Research

**Researched:** 2026-02-27
**Domain:** Event lifecycle state management, TOML document mutation (removal/archival), background task integration
**Confidence:** HIGH

## Summary

Phase 21 is the final phase of the v1.2 Automated Event Management milestone. It adds four capabilities to the existing `ContractLifecycleManager`: (1) archiving expired events from `events.toml` to `events_archive.toml` after a configurable retention period (LIFE-01), (2) auto-cleaning unapproved candidates whose expiry date has passed (LIFE-02), (3) adding a `Retired` variant to `LifecycleStatus` for fully settled and archived events (LIFE-03), and (4) integrating the full discover-match-propose pipeline as a periodic background task within the existing poll cycle (INTG-01).

The codebase already has all the infrastructure needed. The `ContractLifecycleManager::poll_cycle()` method in `src/events/lifecycle.rs` already runs the full discovery-match-propose pipeline (venue polling, cross-venue matching via `find_cross_venue_candidates_fuzzy`, candidate filtering via `filter_new_candidates_fuzzy`, batched TOML writes via `batched_toml_write`, and registry refresh). The `toml_edit::ArrayOfTables` type provides `retain()` and `remove()` methods for filtering entries out of a TOML document in-place. The `DiscoveryConfig` struct in `src/config/events.rs` already supports `#[serde(default)]` fields for new configuration additions.

**Primary recommendation:** Extend the existing `poll_cycle()` with two new steps (archive expired events, clean unapproved candidates) using `toml_edit::ArrayOfTables::retain()` for in-place removal. Add `Retired` to `LifecycleStatus` enum. Add `archive_retention_days` to `DiscoveryConfig`. INTG-01 is already substantially implemented -- verify and document. No new crates needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| LIFE-01 | System archives expired events older than configurable retention period (default 30 days) from events.toml to events_archive.toml | Use `toml_edit::ArrayOfTables::retain()` to remove entries from events.toml `DocumentMut`. Write removed entries to `events_archive.toml` using `append_candidates_to_doc` pattern (or direct table push). Add `archive_retention_days: u32` to `DiscoveryConfig` with default 30. Date comparison uses `expiry` field + retention offset vs `Utc::now().date_naive()`. Archive file uses same `[[events]]` format for human readability. |
| LIFE-02 | System auto-cleans unapproved candidates past their expiry date | In the same `retain()` pass over the `[[events]]` array, remove entries where `approved == false` AND `expiry < today`. These are stale proposals that were never approved before the event occurred. Log each removal at WARN level. No archival needed for unapproved candidates (they contain no operator-authored data). |
| LIFE-03 | System adds Retired status to LifecycleStatus for fully settled and archived events | Add `Retired` variant to the `LifecycleStatus` enum in `src/config/events.rs`. Update `Display`, `Default`, serde `rename_all = "lowercase"` -- the variant automatically serializes to `"retired"`. Events transition to `Retired` when they are archived (moved to `events_archive.toml`). The `active_approved()` filter in `EventRegistry` already excludes non-`Active` statuses, so `Retired` entries are automatically excluded from runtime queries. |
| INTG-01 | Discovery manager runs as periodic background task within ContractLifecycleManager poll cycle | **Already implemented.** The `ContractLifecycleManager::run()` method (line 156-179 of lifecycle.rs) runs an `interval.tick()` loop. `poll_cycle()` executes the full discover-match-propose pipeline: venue discovery (Deribit/Kalshi/Polymarket), cross-venue matching (`find_cross_venue_candidates_fuzzy`), candidate filtering (`filter_new_candidates_fuzzy`), WARN-level proposal logging, Prometheus metrics, batched TOML write, and registry refresh. Verification: confirm this runs as `tokio::spawn(lifecycle_manager.run())` in `main.rs` (line 280) and produces candidate entries visible in events.toml after one cycle. |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `toml_edit` | 0.22 | Format-preserving TOML mutations with `ArrayOfTables::retain()` for entry removal | Already in Cargo.toml; `retain()` predicate-based filtering is the correct API for removing entries while preserving document formatting |
| `chrono` | 0.4 | Date arithmetic for retention period calculation | Already used throughout; `NaiveDate` parsing and comparison for expiry checks |
| `toml` | 0.8 | Deserialization for archive file creation and registry refresh | Already in Cargo.toml; used for re-parsing events.toml after modifications |
| `tokio` | 1 | Async file I/O for reading/writing both events.toml and events_archive.toml | Already in use; `tokio::fs::read_to_string` / `tokio::fs::write` |
| `tracing` | 0.1 | Structured logging for archival and cleanup operations | Already configured with dual stdout + file layers |
| `metrics` | 0.24 | Prometheus counters for archived and cleaned event counts | Already wired to Prometheus exporter |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `anyhow` | 1.0 | Error propagation in archival/cleanup functions | Already used in all toml_writer functions |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `ArrayOfTables::retain()` for removal | Manual index tracking + `remove(index)` in reverse order | `retain()` is cleaner, single pass, no index bookkeeping; use `retain()` |
| Separate archive file (`events_archive.toml`) | SQLite database for archive | TOML is consistent with the existing human-readable, git-trackable approach; requirements explicitly specify `events_archive.toml` |
| Auto-remove unapproved candidates | Archive unapproved candidates to archive file | Unapproved candidates contain no operator-authored data; removal is cleaner than archival. Archival should be reserved for approved events that have completed their lifecycle. |

**No new dependencies required.** All libraries are already in Cargo.toml.

## Architecture Patterns

### Recommended Project Structure

No new files needed. Changes are modifications to existing files:

```
src/
  config/
    events.rs       # Add Retired to LifecycleStatus, add archive_retention_days to DiscoveryConfig
    validation.rs    # (No changes -- Retired status is valid by serde rename_all)
  events/
    lifecycle.rs     # Add archive_and_cleanup step to poll_cycle, new helper methods
    toml_writer.rs   # Add remove_entries_from_doc() and archive helper functions
    registry.rs      # (No changes -- active_approved() already filters non-Active)
config/
    events.toml      # Add archive_retention_days to [discovery] section
```

### Pattern 1: Two-Pass Retain-and-Collect for Archival

**What:** Use `ArrayOfTables::retain()` to simultaneously remove entries from events.toml and collect the removed entries for writing to the archive file.
**When to use:** During the archive step of the poll cycle.
**Why this pattern:** `retain()` gives us a single pass over the array where the predicate decides keep/remove. We cannot directly collect inside `retain()` because it takes `&Table` not ownership, but we can collect the IDs/indices of entries to archive in a pre-pass, then use `retain()` to remove them.

```rust
// Pre-pass: identify entries to archive
let today = Utc::now().date_naive();
let mut archive_ids: Vec<String> = Vec::new();

let events_array = doc["events"].as_array_of_tables().unwrap();
for i in 0..events_array.len() {
    if let Some(table) = events_array.get(i) {
        let status = table.get("status").and_then(|v| v.as_str()).unwrap_or("active");
        let approved = table.get("approved").and_then(|v| v.as_bool()).unwrap_or(true);
        let expiry_str = table.get("expiry").and_then(|v| v.as_str()).unwrap_or("");

        if let Ok(expiry_date) = NaiveDate::parse_from_str(expiry_str, "%Y-%m-%d") {
            // LIFE-01: Archive expired events older than retention period
            if status == "expired" || status == "retired" {
                let age_days = (today - expiry_date).num_days();
                if age_days > retention_days as i64 {
                    if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                        archive_ids.push(id.to_string());
                    }
                }
            }
            // LIFE-02: Remove unapproved candidates past expiry
            if !approved && expiry_date < today {
                if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                    archive_ids.push(id.to_string()); // track for logging
                }
            }
        }
    }
}

// Remove from events.toml document
let events_array_mut = doc["events"].as_array_of_tables_mut().unwrap();
events_array_mut.retain(|table| {
    let id = table.get("id").and_then(|v| v.as_str()).unwrap_or("");
    !archive_ids.contains(&id.to_string())
});
```

**Important note:** The unapproved candidate cleanup (LIFE-02) removes entries entirely (no archive needed). The expired event archival (LIFE-01) must first append the entries to `events_archive.toml` before removing them from `events.toml`. The implementation should separate these two operations clearly.

### Pattern 2: Archive File Append-or-Create

**What:** Append archived entries to `events_archive.toml`, creating the file if it does not exist.
**When to use:** When entries are being archived from events.toml.

```rust
// Read or create archive document
let archive_path = events_toml_path.with_file_name("events_archive.toml");
let archive_content = match tokio::fs::read_to_string(&archive_path).await {
    Ok(content) => content,
    Err(_) => {
        // Create minimal valid archive document
        "# Archived event mappings (auto-generated)\n# Moved from events.toml after retention period\n\n".to_string()
    }
};

let mut archive_doc: DocumentMut = archive_content.parse()?;

// Ensure [[events]] array exists in archive
if archive_doc.get("events").is_none() {
    archive_doc["events"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
}

// Append archived entries (as raw Tables copied from source doc)
let archive_events = archive_doc["events"].as_array_of_tables_mut().unwrap();
for table in &tables_to_archive {
    let mut archived_table = table.clone();
    archived_table["status"] = toml_edit::value("retired");
    archived_table["archived_at"] = toml_edit::value(Utc::now().to_rfc3339());
    archive_events.push(archived_table);
}

// Atomic write to archive
atomic_write_to(&archive_path, &archive_doc.to_string()).await?;
```

### Pattern 3: LifecycleStatus Retired Variant

**What:** Add `Retired` to the existing `LifecycleStatus` enum.
**When to use:** When events are archived to `events_archive.toml`.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStatus {
    Active,
    Expiring,
    Expired,
    Retired,  // NEW: fully settled and archived
}

impl std::fmt::Display for LifecycleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleStatus::Active => write!(f, "active"),
            LifecycleStatus::Expiring => write!(f, "expiring"),
            LifecycleStatus::Expired => write!(f, "expired"),
            LifecycleStatus::Retired => write!(f, "retired"),
        }
    }
}
```

The `Retired` status is set on entries when they are written to the archive file. The `active_approved()` filter in `EventRegistry` already filters by `status == LifecycleStatus::Active`, so `Retired` entries are automatically excluded from runtime queries without any changes to the registry.

### Pattern 4: Poll Cycle Integration Point

**What:** Add archive-and-cleanup as a new step in the existing `poll_cycle()` method.
**When to use:** After the existing expiry detection and before the registry refresh.

```rust
// Existing poll_cycle flow:
// 1. Discover instruments from each venue
// 2. Find new cross-venue candidates (fuzzy matching)
// 3. Flag novel/unmatched instruments
// 4. Detect expired instruments (consecutive-absence tracking)
// 5. Handle Deribit expiry rolls
// 6. Batched TOML write (candidates + expirations)
// 7. Apply expiry warnings
// 7b. Populate BasisRiskCache
// NEW STEP: Archive expired events + clean unapproved candidates
// 8. Refresh runtime registry
// 9. Update pending proposals gauge

// The archive step should run AFTER the batched TOML write (step 6)
// so that newly expired events from this cycle are not immediately archived
// (they need to wait the retention period). It should run BEFORE the
// registry refresh (step 8) so the refresh picks up the cleaned document.
```

The archive operation is a separate TOML read-modify-write cycle from the existing batched write. This is intentional -- the archival operates on the already-updated `events.toml` (after candidates and expirations have been written) and produces a second atomic write that removes archived entries.

### Anti-Patterns to Avoid

- **Archiving within the existing batched_toml_write:** Do not combine the archive operation with the candidate append and expiry marking. The archive involves a second file (`events_archive.toml`) and the logic is fundamentally different (read from source, write to archive, then remove from source). Keep it as a separate step.
- **Removing entries without archiving first:** Always write to `events_archive.toml` before removing from `events.toml`. If the archive write fails, do not remove entries -- log an error and retry next cycle.
- **Archiving unapproved candidates:** Unapproved candidates contain only auto-discovered data with no operator additions. Archiving them adds noise to the archive file. Remove them directly (LIFE-02).
- **Running archive on every poll cycle unconditionally:** The archive operation involves file I/O for two files. Only run it when there are actually entries to archive or clean. Check first, then act.
- **Modifying the existing batched_toml_write signature:** The current `batched_toml_write(&self, candidates, expire_ids)` works well. Add a new method for archival rather than overloading the existing one.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Removing entries from TOML array | Manual index tracking with reverse-order removal | `ArrayOfTables::retain(predicate)` | Single-pass, no index bookkeeping, cannot have off-by-one errors |
| Archive file format | Custom serialization format | Same `[[events]]` TOML array-of-tables format | Human-readable, git-trackable, consistent with existing config, can be re-imported if needed |
| Date arithmetic for retention | Manual day counting | `chrono::NaiveDate` subtraction returning `Duration::num_days()` | Already the project pattern for all date comparisons |
| Atomic file write | Custom write-and-sync | Existing `atomic_write` pattern in `lifecycle.rs` | Already handles Windows remove-before-rename; reuse for archive file writes |

**Key insight:** This phase is entirely about extending the existing `poll_cycle()` with new lifecycle management steps. All the hard problems (venue discovery, cross-venue matching, batched TOML writes, registry refresh, atomic writes) are already solved. The new work is straightforward date comparison logic and TOML document manipulation using APIs that already exist in the codebase.

## Common Pitfalls

### Pitfall 1: Archive Write Failure Causing Data Loss

**What goes wrong:** The system removes entries from `events.toml` but the write to `events_archive.toml` fails (disk full, permissions, etc.), permanently losing the archived data.
**Why it happens:** The remove-from-source and write-to-archive are two separate file operations that are not atomic together.
**How to avoid:** Always write to the archive file FIRST, verify the write succeeded, THEN remove from the source file. If the archive write fails, log an error and skip the removal -- the entries remain in events.toml until the next cycle. This is the "archive-then-remove" pattern.
**Warning signs:** `events_archive.toml` missing entries that were in `events.toml`, gaps in the archive history.

### Pitfall 2: Retain Predicate Operating on Raw TOML Values

**What goes wrong:** The `retain()` predicate reads TOML values using `table.get("field").and_then(|v| v.as_str())` but some fields may be missing or have unexpected types, causing entries to be incorrectly retained or removed.
**Why it happens:** Auto-discovered entries (from Phases 18-20) have `discovered_at`, `expiry_confidence`, and consistent field types. But manually authored entries may have different formatting or missing optional fields. The TOML `retain()` predicate operates on raw `toml_edit::Table` values, not deserialized `EventMapping` structs.
**How to avoid:** Use defensive parsing with sensible defaults: `unwrap_or("active")` for status, `unwrap_or(true)` for approved (matching the serde default), and skip entries where the expiry date cannot be parsed. Log warnings for unparseable entries.
**Warning signs:** Manually authored events disappearing from events.toml, or stale entries never being archived.

### Pitfall 3: Archive File Growing Unboundedly

**What goes wrong:** `events_archive.toml` accumulates all historical events and eventually becomes very large, causing slow parse times or even memory issues.
**Why it happens:** There is no retention policy for the archive file itself.
**How to avoid:** For v1.2, this is acceptable -- the archive grows at the rate of expired events (likely single-digit per month for the current BTC-only scope). Document in the config file that the archive may need manual trimming for long-running deployments. A future version could add archive rotation (yearly files) or max-entries trimming.
**Warning signs:** `events_archive.toml` file size exceeding 100KB (would require hundreds of events, unlikely in v1.2).

### Pitfall 4: Unapproved Candidate Cleanup Racing with Operator Approval

**What goes wrong:** An operator edits `events.toml` to approve a candidate (`approved = false` -> `approved = true`) at the same moment the lifecycle manager's cleanup removes it (because its expiry date has passed). The file watcher sees the operator's edit, but the lifecycle manager overwrites it with the cleaned version.
**Why it happens:** The lifecycle manager reads events.toml, decides to remove the candidate (expiry passed, still unapproved), and writes the file. Meanwhile, the operator has edited the file to approve it. The lifecycle manager's write overwrites the operator's approval.
**How to avoid:** The lifecycle manager already re-reads events.toml at the start of each TOML write operation (`batched_toml_write` reads the file fresh). The archive/cleanup step should also read fresh. The risk window is small (between the read and the write). The existing 500ms debounce on the file watcher provides additional safety. Document in operator guidance: approve candidates before their expiry date to avoid this race.
**Warning signs:** Operator-approved events disappearing from events.toml after a poll cycle.

### Pitfall 5: LifecycleStatus::Retired Breaking Existing Pattern Matches

**What goes wrong:** Adding `Retired` to the enum causes non-exhaustive match warnings or unexpected behavior in existing `match` statements that only handle `Active`, `Expiring`, `Expired`.
**Why it happens:** Rust's exhaustive matching catches this at compile time, but some code may use wildcard patterns (`_`) that silently handle `Retired` incorrectly.
**How to avoid:** After adding the variant, compile and examine every match/comparison involving `LifecycleStatus`. The `active_approved()` filter uses `== LifecycleStatus::Active` (equality check, not pattern match), so `Retired` is automatically excluded. The `mark_expired_batch_in_doc` function checks `status == "expired"` as a string in TOML, so `Retired` entries are unaffected. The validation in `validation.rs` does not match on status at all.
**Warning signs:** Compile errors (good -- caught immediately) or silently incorrect behavior where `Retired` events are treated as active.

## Code Examples

Verified patterns from the existing codebase:

### Existing ArrayOfTables Manipulation (toml_writer.rs)

```rust
// Source: src/events/toml_writer.rs - mark_expired_batch_in_doc
// Shows existing pattern for iterating and mutating [[events]] array
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

### Existing Batched TOML Write (lifecycle.rs)

```rust
// Source: src/events/lifecycle.rs - batched_toml_write
async fn batched_toml_write(
    &self,
    candidates: &[CandidateMapping],
    expire_ids: &[String],
) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
    let mut doc: DocumentMut = content.parse()
        .map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))?;

    if !candidates.is_empty() {
        append_candidates_to_doc(&mut doc, candidates)?;
    }
    if !expire_ids.is_empty() {
        mark_expired_batch_in_doc(&mut doc, expire_ids)?;
    }

    self.atomic_write(&doc.to_string()).await
}
```

### Existing Atomic Write with Windows Handling (lifecycle.rs)

```rust
// Source: src/events/lifecycle.rs - atomic_write
async fn atomic_write(&self, content: &str) -> anyhow::Result<()> {
    let tmp_path = self.events_toml_path.with_extension("toml.tmp");
    tokio::fs::write(&tmp_path, content).await?;

    #[cfg(target_os = "windows")]
    {
        let _ = tokio::fs::remove_file(&self.events_toml_path).await;
    }

    tokio::fs::rename(&tmp_path, &self.events_toml_path).await?;
    Ok(())
}
```

### Existing LifecycleStatus Enum (config/events.rs)

```rust
// Source: src/config/events.rs - LifecycleStatus
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStatus {
    Active,
    Expiring,
    Expired,
    // Phase 21 adds: Retired
}
```

### Existing Discovery Integration in poll_cycle (lifecycle.rs)

```rust
// Source: src/events/lifecycle.rs - poll_cycle (lines 482-516)
// This shows the INTG-01 pipeline already running:
// 1. find_cross_venue_candidates_fuzzy (matching)
// 2. filter_new_candidates_fuzzy (deduplication)
// 3. WARN-level proposal logging
// 4. metrics::counter!("proposals_total").increment(1)
// 5. batched_toml_write (atomic TOML write)
// 6. refresh_registry (runtime update)
let registry = self.registry.read().await;
let candidates = find_cross_venue_candidates_fuzzy(
    &all_discovered,
    self.discovery_config.expiry_tolerance_days,
);
let new_candidates = filter_new_candidates_fuzzy(&candidates, &registry);
drop(registry);
```

### New: retain() API for Entry Removal (toml_edit 0.22)

```rust
// Source: docs.rs/toml_edit/0.22 - ArrayOfTables::retain
// Verified via docs.rs: retain() exists and takes FnMut(&Table) -> bool
let events = doc["events"].as_array_of_tables_mut().unwrap();
events.retain(|table| {
    // Return true to keep, false to remove
    let id = table.get("id").and_then(|v| v.as_str()).unwrap_or("");
    !ids_to_remove.contains(&id.to_string())
});
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Expired events accumulate forever in events.toml | Archived to events_archive.toml after retention period (Phase 21) | Phase 21 | Keeps active config small and readable |
| Unapproved candidates accumulate forever | Auto-cleaned after expiry date passes (Phase 21) | Phase 21 | Reduces operator noise; events.toml only shows actionable items |
| LifecycleStatus: Active/Expiring/Expired | Active/Expiring/Expired/Retired (Phase 21) | Phase 21 | Distinguishes "settlement pending" from "fully done" |
| Discovery pipeline described as future work | Runs within ContractLifecycleManager poll cycle (Phases 18-20, verified Phase 21) | Phase 20 | Full automation; operator only reviews proposals |

**Deprecated/outdated:**
- `append_candidate_to_toml` (single-candidate version in toml_writer.rs): Still exists for backward compatibility but batch version `append_candidates_to_doc` is used exclusively by the lifecycle manager.
- `mark_expired_in_toml` (single-entry version): Similarly superseded by `mark_expired_batch_in_doc`.

## Open Questions

1. **Archive File Table Cloning**
   - What we know: `toml_edit::ArrayOfTables::retain()` takes `&Table` in the predicate, which is immutable. To archive entries before removing them, we need to collect the Table data before calling `retain()`. The `Table` type implements `Clone`.
   - What's unclear: Whether cloning `toml_edit::Table` preserves all formatting (comments, whitespace). Since the archive file is auto-generated, exact formatting preservation is not critical -- but it would be nice.
   - Recommendation: Iterate the array first to collect entries to archive (clone the Table values), then call `retain()` to remove them. Even if formatting is slightly different in the archive, the data integrity is preserved.

2. **Cleanup Frequency**
   - What we know: The archive/cleanup runs as part of the poll cycle, which ticks at `min_poll_interval_secs` (default 300 seconds / 5 minutes). Running date comparisons every 5 minutes is computationally trivial.
   - What's unclear: Whether there is value in running the archive/cleanup less frequently (e.g., once per hour or once per day) to reduce file I/O.
   - Recommendation: Run the check every poll cycle but only perform file I/O when there are actually entries to archive or clean. The date comparison is O(n) where n is typically <20 events. The cost is negligible.

3. **Archive File Config Watcher**
   - What we know: The `ConfigReloader` watches the config directory for `.toml` changes. Writing `events_archive.toml` to the same directory will trigger a config reload.
   - What's unclear: Whether this causes issues -- the reload re-parses `events.toml`, `config.toml`, and `venues.toml`. It does NOT parse `events_archive.toml` (which is not part of `load_config`).
   - Recommendation: The file watcher filters by extension (`.toml`), so `events_archive.toml` will trigger a reload. This is harmless -- the reload re-reads the (already updated) `events.toml` and the registry refresh is idempotent. The 500ms debounce will coalesce the events.toml write and events_archive.toml write into a single reload. No changes needed.

## Sources

### Primary (HIGH confidence)
- **Codebase analysis:** `src/events/lifecycle.rs` (ContractLifecycleManager, poll_cycle, batched_toml_write, atomic_write), `src/events/toml_writer.rs` (append_candidates_to_doc, mark_expired_batch_in_doc, build_candidate_table), `src/config/events.rs` (LifecycleStatus, DiscoveryConfig, EventMapping), `src/events/registry.rs` (EventRegistry, active_approved, refresh), `src/main.rs` (lifecycle manager wiring), `src/config/validation.rs` (validate_config), `src/config/reload.rs` (ConfigReloader)
- **toml_edit 0.22 API docs (docs.rs):** Confirmed `ArrayOfTables::retain(FnMut(&Table) -> bool)`, `ArrayOfTables::remove(usize)`, `Table::clone()`, `ArrayOfTables::push(Table)` -- all methods needed for archival
- **Existing events.toml:** Confirmed `[[events]]` array-of-tables structure with `id`, `status`, `approved`, `expiry`, `discovered_at` fields
- **Cargo.toml:** Confirmed `toml_edit = "0.22"`, `chrono = "0.4"`, `toml = "0.8"` -- no new dependencies needed

### Secondary (MEDIUM confidence)
- **Phase 18-20 research and plans:** Confirmed batched TOML write pattern, candidate mapping structure, fuzzy matching pipeline, proposal logging, metrics
- **Project decisions (STATE.md):** `approved = false` safety gate is non-negotiable; batched writes per poll cycle; strict less-than for expiry checks

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in use; zero new dependencies
- Architecture: HIGH - All patterns are direct extensions of existing `poll_cycle()` logic using verified `toml_edit` APIs
- Pitfalls: HIGH - Based on direct codebase analysis; race conditions between lifecycle manager and file watcher are well-understood from Phase 18 research
- LIFE-01 (archival): HIGH - `ArrayOfTables::retain()` confirmed; archive-then-remove pattern is straightforward
- LIFE-02 (cleanup): HIGH - Simple date comparison + `retain()` predicate; no external dependencies
- LIFE-03 (Retired status): HIGH - Single enum variant addition with automatic serde support
- INTG-01 (integration): HIGH - Already implemented in Phases 18-20; Phase 21 verifies and documents

**Research date:** 2026-02-27
**Valid until:** 2026-03-27 (stable domain; no external API changes expected)
