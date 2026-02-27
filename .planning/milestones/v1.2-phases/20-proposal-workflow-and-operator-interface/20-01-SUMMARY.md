---
phase: 20-proposal-workflow-and-operator-interface
plan: 01
subsystem: events
tags: [tracing, prometheus, metrics, lifecycle, proposals]

# Dependency graph
requires:
  - phase: 18-lifecycle-toml-persistence
    provides: "batched TOML writes with approved=false, build_candidate_table, atomic_write"
  - phase: 19-polymarket-discovery-and-cross-venue-matching
    provides: "cross-venue fuzzy matching, CandidateMapping with polymarket/kalshi venues"
provides:
  - "WARN-level structured proposal logging with 7 fields (event_id, matched_venues, instruments, expiry, confidence)"
  - "proposals_total Prometheus counter per new proposal written"
  - "proposals_pending Prometheus gauge set each poll cycle"
  - "EventRegistry::pending_count() helper method"
affects: [20-02-PLAN, operator-dashboards, alerting]

# Tech tracking
tech-stack:
  added: []
  patterns: ["proposals_pending gauge pattern: always set at end of poll_cycle for consistency"]

key-files:
  created: []
  modified:
    - "src/events/lifecycle.rs"
    - "src/events/registry.rs"

key-decisions:
  - "Kept lifecycle_candidates_discovered counter alongside new proposals_total for backward compatibility"
  - "Set proposals_pending gauge unconditionally at end of every poll cycle (not just after writes) to stay current even when proposals are approved externally"
  - "Adapted plan's tracing::warn! fields to match CandidateMapping struct (Option<String> for deribit/kalshi, Option<(String,String)> for polymarket) rather than EventMapping struct"

patterns-established:
  - "Proposal metrics pattern: counter per-item inside loop, gauge once at cycle end"

requirements-completed: [PROP-01, PROP-02, PROP-03]

# Metrics
duration: 2min
completed: 2026-02-27
---

# Phase 20 Plan 01: Proposal Logging and Metrics Summary

**WARN-level structured proposal logging with event_id/venues/instruments/expiry/confidence fields, proposals_total counter, and proposals_pending gauge for operator visibility**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-27T08:21:01Z
- **Completed:** 2026-02-27T08:23:28Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Upgraded candidate proposal logging from INFO to WARN level with all 7 required structured fields (event_id, matched_venues, deribit_instrument, polymarket_instrument, kalshi_instrument, expiry, confidence)
- Added proposals_total Prometheus counter that increments once per new proposal written to events.toml
- Added proposals_pending Prometheus gauge that reflects the count of active unapproved mappings after every poll cycle
- Added pending_count() helper method to EventRegistry with unit test
- Verified PROP-01: build_candidate_table sets approved=false and batched_toml_write uses atomic write pattern

## Task Commits

Each task was committed atomically:

1. **Task 1: Add pending_count() helper, upgrade proposal logging + metrics** - `0fd367e` (feat)

## Files Created/Modified
- `src/events/registry.rs` - Added pending_count() method and pending_count_returns_active_unapproved test
- `src/events/lifecycle.rs` - Upgraded candidate loop to tracing::warn! with structured fields, added proposals_total counter and proposals_pending gauge

## Decisions Made
- Kept existing `lifecycle_candidates_discovered` counter alongside new `proposals_total` for backward compatibility (they track different concepts: total candidates found vs. proposals written)
- Set `proposals_pending` gauge unconditionally at end of every poll cycle, not just after writes, so it stays current even when proposals are approved externally via config reload
- Adapted tracing::warn! field extraction to match CandidateMapping struct (deribit/kalshi as Option<String>, polymarket as Option<(String, String)> with map to extract condition_id)

## Deviations from Plan

None - plan executed exactly as written (minor adaptation of field access patterns to match actual CandidateMapping struct types).

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Proposal logging and metrics complete, ready for Phase 20 Plan 02 (operator interface and approval workflow)
- proposals_pending gauge enables dashboard monitoring of unapproved proposal backlog
- WARN-level logs enable log-based alerting for new cross-venue candidate discoveries

---
*Phase: 20-proposal-workflow-and-operator-interface*
*Completed: 2026-02-27*
