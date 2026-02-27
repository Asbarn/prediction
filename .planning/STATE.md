# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-26)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.2 Automated Event Management -- Phase 19 (Polymarket Discovery and Cross-Venue Matching)

## Current Position

Phase: 19 of 21 (Polymarket Discovery and Cross-Venue Matching)
Plan: 01 of 02 complete
Status: In Progress
Last activity: 2026-02-27 -- Completed 19-01 (Polymarket Structured Discovery)

Progress: [#########                     ] 30% (v1.2: phase 19 in progress, 19-01 complete)

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

### Pending Todos

None.

### Blockers/Concerns

- Polymarket groupItemTitle format is not guaranteed stable (permissionless market creation)
- Windows atomic rename produces DELETE + RENAME events that may race with file watcher debouncer
- Kalshi may introduce new ticker patterns that bypass extract_kalshi_asset parser
- EventRegistry.refresh() behavior with new EventMapping entries needs verification

## Session Continuity

Last session: 2026-02-27
Stopped at: Completed 19-01-PLAN.md (Polymarket Structured Discovery)
Next action: Execute 19-02 (Cross-Venue Fuzzy Matching and Lifecycle Integration)
