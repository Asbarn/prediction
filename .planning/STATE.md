# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-26)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.2 Automated Event Management -- Phase 21 (Lifecycle Management and Integration)

## Current Position

Phase: 21 of 21 (Lifecycle Management and Integration)
Plan: 02 of 02 complete
Status: Phase Complete
Last activity: 2026-02-27 -- Completed 21-02 (Archive-and-Cleanup Poll Cycle Integration)

Progress: [##############################] 100% (v1.2: phase 21 plan 02 complete -- milestone done)

## Performance Metrics

**v1.0 Summary:**
- Total plans completed: 36
- Total phases: 13
- Lines of Rust: 22,751
- Timeline: 4 days (2026-02-21 to 2026-02-24)

**v1.1 Summary:**
- Plans completed: 11
- Phases: 4 (14-17)
- LOC delta: +14,943 (32,631 total)
- Timeline: 5 days (2026-02-21 to 2026-02-26)

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Full decision history in .planning/milestones/v1.0-ROADMAP.md and .planning/milestones/v1.1-ROADMAP.md

Recent decisions:
- 21-02: archive_and_cleanup returns bool for conditional registry refresh (OR-ed with needs_write)
- 21-02: Archive-then-remove: archive file written atomically before entries removed from events.toml
- 21-02: Integration test uses toml_writer functions directly to avoid full manager construction
- 21-01: Retired variant after Expired in enum for logical lifecycle progression
- 21-01: Archive retention default 30 days for financial data retention
- 21-01: Defensive TOML parsing with unwrap_or defaults for missing fields
- 21-01: Strict less-than for expired unapproved check, consistent with validation.rs
- 20-02: Strict less-than for expiry check (events expiring today still valid for Deribit 08:00 UTC settlement)
- 20-02: Venue activity check gated behind non-empty discovery data per venue to avoid false warnings
- 20-02: Updated example events.toml to far-future dates to pass approved-mapping validation
- 20-01: Kept lifecycle_candidates_discovered counter alongside proposals_total for backward compatibility
- 20-01: proposals_pending gauge set unconditionally at end of every poll cycle for external-approval consistency
- v1.2: Live subscription management deferred to v1.3 -- restart-on-approval is acceptable
- v1.2: approved = false approval gate is non-negotiable safety mechanism
- v1.2: Single new dependency: strsim = "0.11" (already compiled transitively via clap_builder)
- v1.2: Batched TOML writes per poll cycle to avoid write/file-watcher race conditions
- v1.2: N consecutive absence polls before marking instrument expired (prevents false expirations)
- 18-01: Factored build_candidate_table helper to deduplicate table construction
- 18-01: Batch functions take &mut DocumentMut returning Result<()> for "parse once, mutate N, write once" pattern
- 18-02: Refactored handle_deribit_roll to pure find_deribit_roll for batched write compatibility
- 18-02: Windows atomic_write uses remove-before-rename via #[cfg(target_os = "windows")]
- 19-01: String parsing over regex for Polymarket question text (3 predictable patterns)
- 19-01: endDateIso is authoritative expiry source; question text dates are NOT parsed
- 19-01: ExpiryConfidence::High default for existing CandidateMapping constructions (proper scoring in Plan 02)
- 19-02: FuzzyMatchKey uses 3 fields (asset/strike/direction), expiry checked separately against tolerance
- 19-02: Earliest expiry date as representative for event_id (most conservative)
- 19-02: extra_venue_id field on DiscoveredInstrument for Polymarket token_id propagation

### Pending Todos

None.

### Blockers/Concerns

- Polymarket groupItemTitle format is not guaranteed stable (permissionless market creation)
- Windows atomic rename produces DELETE + RENAME events that may race with file watcher debouncer
- Kalshi may introduce new ticker patterns that bypass extract_kalshi_asset parser
- EventRegistry.refresh() behavior with new EventMapping entries needs verification

## Session Continuity

Last session: 2026-02-27
Stopped at: Completed 21-02-PLAN.md (Archive-and-Cleanup Poll Cycle Integration)
Next action: v1.2 milestone complete -- all 21 phases, all plans executed
