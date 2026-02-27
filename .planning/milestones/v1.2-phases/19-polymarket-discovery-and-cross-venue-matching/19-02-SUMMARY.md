---
phase: 19-polymarket-discovery-and-cross-venue-matching
plan: 02
subsystem: events
tags: [fuzzy-matching, cross-venue, expiry-tolerance, polymarket, lifecycle, three-venue]

# Dependency graph
requires:
  - phase: 19-polymarket-discovery-and-cross-venue-matching
    plan: 01
    provides: discover_polymarket_structured, ExpiryConfidence, compute_expiry_confidence, generate_polymarket_slugs, DiscoveryConfig with expiry_tolerance_days
  - phase: 18-discovery-infrastructure-hardening
    provides: discovery polling, CandidateMapping, build_candidate_table, VenueRateLimiter, batched TOML writes
provides:
  - FuzzyMatchKey (asset/strike/direction, no expiry) for tolerance-based matching
  - find_cross_venue_candidates_fuzzy() with configurable expiry tolerance
  - filter_new_candidates_fuzzy() using earliest expiry as representative date
  - Three-venue candidate generation (Deribit + Kalshi + Polymarket)
  - Polymarket structured discovery integrated into lifecycle poll_cycle
  - Polymarket absence tracking in poll_cycle
  - extra_venue_id field on DiscoveredInstrument for Polymarket token_id propagation
affects: [20-approval-workflow, 21-monitoring-observability]

# Tech tracking
tech-stack:
  added: []
  patterns: [fuzzy match key grouping without expiry, expiry tolerance window filtering, earliest-expiry-as-representative for event ID generation, extra_venue_id propagation pattern for venue-specific secondary IDs]

key-files:
  created: []
  modified:
    - src/events/discovery.rs
    - src/events/lifecycle.rs

key-decisions:
  - "FuzzyMatchKey uses three fields (asset/strike/direction) -- expiry excluded and checked separately against tolerance"
  - "Earliest expiry date used as representative for event_id generation (most conservative, per research)"
  - "extra_venue_id field added to DiscoveredInstrument for Polymarket token_id -- avoids breaking DiscoveredInstrument's venue-agnostic design"
  - "Existing find_cross_venue_candidates and filter_new_candidates preserved for backward compatibility"
  - "flag_novel_instruments switched to FuzzyMatchKey so instruments with tolerance-matching expiry dates across venues are not falsely flagged"

patterns-established:
  - "Fuzzy matching: group by non-temporal fields, then filter by temporal tolerance window"
  - "Extra venue ID: optional field for venue-specific secondary identifiers"

requirements-completed: [DISC-03, DISC-04]

# Metrics
duration: 7min
completed: 2026-02-27
---

# Phase 19 Plan 02: Cross-Venue Fuzzy Matching and Lifecycle Integration Summary

**FuzzyMatchKey tolerance-based cross-venue matching replacing exact-expiry matching, with Polymarket structured discovery wired into lifecycle poll_cycle for three-venue candidate generation**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-27T07:30:09Z
- **Completed:** 2026-02-27T07:37:20Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- FuzzyMatchKey groups instruments by asset/strike/direction, ignoring expiry -- enabling Deribit Friday + Kalshi/Polymarket end-of-month matching within configurable tolerance window
- find_cross_venue_candidates_fuzzy produces multi-venue groups with ExpiryConfidence scoring (High/Medium/Low based on date spread)
- filter_new_candidates_fuzzy generates CandidateMapping with earliest expiry as event_id, full three-venue data including Polymarket condition_id + token_id
- Lifecycle poll_cycle now runs discover_polymarket_structured via slug patterns, feeds all three venues into fuzzy matching, and writes three-venue candidates to events.toml
- Polymarket absence tracking added alongside Deribit and Kalshi for consistent expiry detection
- 8 new unit tests covering fuzzy matching tolerance, three-venue grouping, confidence levels, event_id generation, and Polymarket venue ID propagation

## Task Commits

Each task was committed atomically:

1. **Task 1: Add FuzzyMatchKey, fuzzy matching function, and update candidate filtering** - `a6d96d8` (feat)
2. **Task 2: Wire Polymarket structured discovery and fuzzy matching into lifecycle poll_cycle** - `8637b1e` (feat)

## Files Created/Modified
- `src/events/discovery.rs` - Added FuzzyMatchKey struct, find_cross_venue_candidates_fuzzy, filter_new_candidates_fuzzy, extra_venue_id field on DiscoveredInstrument, updated flag_novel_instruments to use FuzzyMatchKey, 8 new tests
- `src/events/lifecycle.rs` - Replaced exact matching with fuzzy matching in poll_cycle, added Polymarket structured discovery with slug expansion, added Polymarket absence tracking, enhanced candidate logging with venue count and confidence

## Decisions Made
- FuzzyMatchKey uses three fields (asset/strike/direction) with expiry checked separately against tolerance -- cleanly separates grouping from temporal filtering
- Earliest expiry date used as representative for event_id generation (most conservative approach per research)
- extra_venue_id field added to DiscoveredInstrument for Polymarket token_id propagation -- avoids breaking the venue-agnostic design with a Polymarket-specific field
- Existing exact-matching functions preserved for backward compatibility (not removed)
- flag_novel_instruments switched to FuzzyMatchKey so tolerance-matching instruments across venues are not falsely flagged as novel

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed extra_venue_id field missing in lifecycle.rs test constructions**
- **Found during:** Task 1
- **Issue:** Adding extra_venue_id to DiscoveredInstrument broke lifecycle.rs test code that constructs DiscoveredInstrument without the new field
- **Fix:** Added extra_venue_id: None to all DiscoveredInstrument constructions in lifecycle.rs tests
- **Files modified:** src/events/lifecycle.rs
- **Verification:** cargo check passes
- **Committed in:** a6d96d8 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Auto-fix necessary for compilation after adding new required field. No scope creep.

## Issues Encountered
None - plan executed cleanly with both tasks compiling and all 595 tests passing.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Three-venue fuzzy matching fully operational: Deribit + Kalshi + Polymarket instruments grouped by asset/strike/direction with configurable expiry tolerance
- ExpiryConfidence scoring flows through to events.toml TOML output
- Lifecycle poll_cycle produces three-venue candidates with approved=false for human review
- Phase 19 complete: all Polymarket discovery and cross-venue matching requirements implemented
- Ready for Phase 20 (Approval Workflow) and Phase 21 (Monitoring/Observability)

---
## Self-Check: PASSED
