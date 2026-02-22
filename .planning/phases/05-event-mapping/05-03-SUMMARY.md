---
phase: 05-event-mapping
plan: 03
subsystem: events
tags: [discovery, lifecycle, rest-api, cross-venue-matching, expiry-roll, deribit, kalshi, polymarket]

# Dependency graph
requires:
  - phase: 05-event-mapping
    plan: 01
    provides: "EventRegistry, TOML writer, CandidateMapping, EventsConfig schema"
  - phase: 05-event-mapping
    plan: 02
    provides: "BasisRiskScore, ExpiryWarning, check_expiry_warning, inflate_risk_score"
  - phase: 04-multi-venue-feeds
    provides: "Multi-venue pipeline, Kalshi auth (sign_kalshi_request), VenuesConfig"
provides:
  - "Per-venue REST discovery functions (Deribit, Kalshi, Polymarket)"
  - "Cross-venue candidate matching with exact four-field key"
  - "ContractLifecycleManager background task with periodic polling"
  - "Deribit expiry roll handling (new candidates without approval carry-over)"
  - "Near-expiry warning application with risk score inflation"
  - "Pipeline EventRegistry pass-through parameter for Phase 6"
affects: [06-pricing-engine, 07-signal-generation]

# Tech tracking
tech-stack:
  added: []
  patterns: [per-venue REST polling, four-field exact matching, atomic TOML write-back, background lifecycle task]

key-files:
  created:
    - src/events/discovery.rs
    - src/events/lifecycle.rs
  modified:
    - src/events/mod.rs
    - src/config/events.rs
    - src/main.rs
    - src/feed/pipeline.rs

key-decisions:
  - "DiscoveryConfig.min_poll_interval_secs() used as lifecycle tick interval; venues polled independently"
  - "Kalshi asset extracted from ticker prefix (KX{ASSET}D pattern) rather than separate API field"
  - "Polymarket discovery limited to deactivation monitoring in v1 (no structured field extraction)"
  - "Pipeline accepts optional EventRegistry parameter (pass-through for Phase 6 annotation)"
  - "Lifecycle manager uses Instant-based per-venue tracking rather than separate timers"

patterns-established:
  - "Per-venue REST discovery: normalized DiscoveredInstrument from venue-specific API responses"
  - "Exact four-field MatchKey (asset+strike+expiry+direction) for cross-venue candidate matching"
  - "Atomic TOML writes: write to .tmp then rename for crash safety"
  - "Background lifecycle task with CancellationToken child for graceful shutdown"

requirements-completed: [EVNT-04, EVNT-05]

# Metrics
duration: 14min
completed: 2026-02-22
---

# Phase 5 Plan 03: Contract Lifecycle Manager Summary

**Per-venue REST discovery with cross-venue four-field exact matching, ContractLifecycleManager background task with expiry detection, Deribit roll handling, and near-expiry warning application**

## Performance

- **Duration:** 14 min
- **Started:** 2026-02-22T21:21:33Z
- **Completed:** 2026-02-22T21:35:45Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Built per-venue REST discovery functions: Deribit (public get_instruments), Kalshi (RSA-PSS auth, paginated), Polymarket (Gamma API deactivation monitoring)
- Implemented DiscoveredInstrument normalization from venue-specific API responses with Decimal strikes and NaiveDate expiries
- Cross-venue candidate matching using exact four-field MatchKey (asset, strike, expiry, direction) per user decision
- filter_new_candidates skips already-registered mappings; flag_novel_instruments identifies unmatched single-venue instruments
- ContractLifecycleManager runs as background tokio task with per-venue poll interval tracking
- Discovery proposes candidates with approved=false, appended to events.toml via format-preserving TOML writer
- Expired instrument detection when mapped instruments no longer appear in venue API response
- Deribit expiry roll handling: same asset/strike/direction with later expiry creates fresh candidate (no approval carry-over)
- Near-expiry warnings with configurable tiers and settlement_time_risk inflation
- Atomic TOML writes via .tmp file + rename for crash safety
- Main.rs spawns lifecycle manager in Live mode only (not Mock/Replay)
- Pipeline accepts optional Arc<RwLock<EventRegistry>> for Phase 6 snapshot annotation
- All 215 tests pass (171 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc)

## Task Commits

Each task was committed atomically:

1. **Task 1: Per-venue REST discovery and cross-venue candidate matching** - `cad9688` (feat)
2. **Task 2: ContractLifecycleManager and main.rs integration** - `f5d751c` (feat)

## Files Created/Modified
- `src/events/discovery.rs` - DiscoveredInstrument, MatchKey, discover_deribit, discover_kalshi, discover_polymarket, find_cross_venue_candidates, filter_new_candidates, flag_novel_instruments (14 tests)
- `src/events/lifecycle.rs` - ContractLifecycleManager with poll_cycle, expiry detection, Deribit roll handling, expiry warning application, atomic TOML writes (5 tests)
- `src/events/mod.rs` - Added `pub mod discovery` and `pub mod lifecycle`
- `src/config/events.rs` - Added DiscoveryConfig::min_poll_interval_secs() method
- `src/main.rs` - Spawns ContractLifecycleManager in Live mode with child CancellationToken, builds shared EventRegistry
- `src/feed/pipeline.rs` - run_multi_venue_pipeline accepts optional EventRegistry parameter

## Decisions Made
- DiscoveryConfig.min_poll_interval_secs() returns minimum across all venue poll intervals, used as lifecycle tick interval; each venue is polled only when its own interval has elapsed
- Kalshi asset is extracted from ticker prefix using KX{ASSET}D pattern (e.g., KXBTCD -> BTC) rather than requiring a separate API field
- Polymarket discovery in v1 is limited to deactivation monitoring (logging closed/inactive markets) -- no structured field extraction from question text (per research recommendation)
- Pipeline's EventRegistry parameter is optional (None in Mock/Replay modes, Some in Live) to avoid breaking existing callers while preparing for Phase 6
- Lifecycle manager initializes last_poll timestamps to trigger immediate first poll on startup

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] DiscoveryConfig missing min_poll_interval_secs()**
- **Found during:** Task 1
- **Issue:** Plan references `self.discovery_config.min_poll_interval_secs()` but the method didn't exist on DiscoveryConfig
- **Fix:** Added `min_poll_interval_secs()` method to DiscoveryConfig in config/events.rs
- **Files modified:** src/config/events.rs
- **Committed in:** cad9688 (Task 1 commit)

**2. [Rule 3 - Blocking] rust_decimal_macros crate not available**
- **Found during:** Task 1 (test compilation)
- **Issue:** Tests used `dec!()` macro which requires `rust_decimal_macros` crate not in Cargo.toml
- **Fix:** Replaced `dec!()` with `Decimal::from_str().unwrap()` in tests
- **Files modified:** src/events/discovery.rs
- **Committed in:** cad9688 (Task 1 commit)

**3. [Rule 2 - Missing functionality] Deribit base URL extraction from ws_url**
- **Found during:** Task 2
- **Issue:** Deribit discovery needs REST base URL but config only has ws_url
- **Fix:** Extract hostname from ws_url by stripping protocol and /ws/ path, prepending https://
- **Files modified:** src/events/lifecycle.rs
- **Committed in:** f5d751c (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 missing method, 1 missing crate workaround, 1 URL extraction)
**Impact on plan:** Minor. All fixes are straightforward and don't change the architecture.

## Issues Encountered
None

## User Setup Required
None - discovery runs automatically in Live mode. Events.toml candidates require user review (set approved=true).

## Next Phase Readiness
- Phase 5 (Event Mapping) is complete: registry, risk scoring, and lifecycle management all operational
- EventRegistry is threaded through pipeline for Phase 6 snapshot annotation
- ContractLifecycleManager provides continuous instrument discovery and lifecycle management
- All 215 project tests pass

## Self-Check: PASSED

- All 6 key files exist on disk
- Both task commits found in git log (cad9688, f5d751c)
- All 215 tests pass (171 lib + 16 integration + 3 pipeline + 22 smoke + 3 doc)
- cargo run -- check-config validates successfully

---
*Phase: 05-event-mapping*
*Completed: 2026-02-22*
