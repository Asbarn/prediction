---
phase: 19-polymarket-discovery-and-cross-venue-matching
plan: 01
subsystem: events
tags: [polymarket, gamma-api, discovery, expiry-confidence, question-parsing]

# Dependency graph
requires:
  - phase: 18-discovery-infrastructure-hardening
    provides: discovery polling, CandidateMapping, build_candidate_table, VenueRateLimiter
provides:
  - discover_polymarket_structured() returning Vec<DiscoveredInstrument>
  - parse_polymarket_question() extracting asset/strike/direction from question text
  - ExpiryConfidence enum (High/Medium/Low) with compute_expiry_confidence()
  - generate_polymarket_slugs() for event slug pattern expansion
  - DiscoveryConfig with expiry_tolerance_days and polymarket_event_slugs
  - CandidateMapping with expiry_confidence field written to TOML
affects: [19-02 cross-venue fuzzy matching, lifecycle integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [string-based question parsing without regex, slug-pattern templating with month/year placeholders, expiry confidence scoring from date spread]

key-files:
  created: []
  modified:
    - src/events/discovery.rs
    - src/config/events.rs
    - src/events/toml_writer.rs
    - src/events/lifecycle.rs

key-decisions:
  - "String parsing over regex for Polymarket question text -- regex crate not in dependency tree, 3 predictable patterns handled by strip_prefix/find"
  - "endDateIso is the authoritative expiry source -- question text dates are NOT parsed"
  - "ExpiryConfidence::High default for existing CandidateMapping constructions -- proper scoring deferred to Plan 02 fuzzy matching"
  - "Prefer Yes outcome token from Polymarket tokens array, fallback to first token"

patterns-established:
  - "Question text parsing: strip_prefix/strip_suffix/find for predictable NLP patterns"
  - "Slug templating: {month}/{year} placeholder replacement from chrono::Utc::now()"
  - "Expiry confidence: date spread categorization (<=2d High, <=7d Medium, >7d Low)"

requirements-completed: [DISC-01, INTG-02]

# Metrics
duration: 6min
completed: 2026-02-27
---

# Phase 19 Plan 01: Polymarket Discovery and Cross-Venue Matching Summary

**Polymarket structured discovery via Gamma API slug polling with question text parsing (reach/hit/dip patterns) and ExpiryConfidence scoring for cross-venue candidate proposals**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-27T07:20:21Z
- **Completed:** 2026-02-27T07:26:46Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Polymarket question parser handles "reach $X", "hit $X", "dip to $X" patterns with comma-separated strikes across BTC/ETH/SOL assets
- discover_polymarket_structured() polls Gamma API events by slug, deduplicates by conditionId, and returns Vec<DiscoveredInstrument> (same type as Deribit/Kalshi)
- ExpiryConfidence enum with compute_expiry_confidence() ready for Plan 02 fuzzy matching
- DiscoveryConfig extended with expiry_tolerance_days (default 7) and polymarket_event_slugs (2 default patterns) with backward-compatible serde defaults
- CandidateMapping writes expiry_confidence to events.toml via build_candidate_table
- 11 new unit tests covering parsing, normalization, confidence scoring, slug generation, and API response deserialization

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend DiscoveryConfig, add ExpiryConfidence enum, extend CandidateMapping** - `abff22d` (feat)
2. **Task 2: Implement Polymarket question parser and structured discovery function** - `cab1a19` (feat)

## Files Created/Modified
- `src/config/events.rs` - Added expiry_tolerance_days and polymarket_event_slugs fields to DiscoveryConfig with serde defaults
- `src/events/discovery.rs` - Added ExpiryConfidence enum, compute_expiry_confidence, generate_polymarket_slugs, normalize_polymarket_asset, parse_polymarket_question, GammaEventResponse, discover_polymarket_structured, plus 11 unit tests
- `src/events/toml_writer.rs` - Added expiry_confidence field to CandidateMapping, fixed test construction
- `src/events/lifecycle.rs` - Added ExpiryConfidence import and field to find_deribit_roll CandidateMapping construction

## Decisions Made
- String parsing over regex for Polymarket question text: regex crate not in dependency tree, 3 predictable patterns handled by strip_prefix/find
- endDateIso is the authoritative expiry source: question text dates are NOT parsed (per research recommendation)
- ExpiryConfidence::High default for existing CandidateMapping constructions: proper scoring deferred to Plan 02 fuzzy matching
- Prefer "Yes" outcome token from Polymarket tokens array, fallback to first token

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed missing expiry_confidence in lifecycle.rs CandidateMapping**
- **Found during:** Task 1
- **Issue:** Adding expiry_confidence to CandidateMapping broke lifecycle.rs find_deribit_roll which constructs CandidateMapping without the new field
- **Fix:** Added ExpiryConfidence import and expiry_confidence: ExpiryConfidence::High to the construction in lifecycle.rs
- **Files modified:** src/events/lifecycle.rs
- **Verification:** cargo check passes
- **Committed in:** abff22d (Task 1 commit)

**2. [Rule 3 - Blocking] Fixed missing expiry_confidence in toml_writer test**
- **Found during:** Task 1
- **Issue:** The append_to_toml_without_events_array_returns_error test constructed CandidateMapping without expiry_confidence
- **Fix:** Added expiry_confidence: ExpiryConfidence::High to the test construction
- **Files modified:** src/events/toml_writer.rs
- **Verification:** cargo test passes
- **Committed in:** abff22d (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both auto-fixes necessary for compilation after adding new required field. No scope creep.

## Issues Encountered
None - most of Task 1's types and config fields were already present in the codebase from a prior phase setup, so the primary work was fixing compilation errors and implementing the parser/discovery function.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- discover_polymarket_structured() is ready for lifecycle integration (Plan 02)
- ExpiryConfidence and compute_expiry_confidence ready for fuzzy matching (Plan 02)
- FuzzyMatchKey and tolerance-based matching will be implemented in Plan 02
- All existing tests pass (587 total including 11 new)

---
## Self-Check: PASSED

- All 4 modified files exist on disk
- Both task commits (abff22d, cab1a19) verified in git log
- 587 tests pass (530 lib + 16 + 5 + 11 + 22 + 3 doc)
- cargo check compiles without errors

---
*Phase: 19-polymarket-discovery-and-cross-venue-matching*
*Completed: 2026-02-27*
