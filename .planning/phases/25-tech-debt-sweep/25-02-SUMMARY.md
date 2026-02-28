---
phase: 25-tech-debt-sweep
plan: 02
subsystem: feed
tags: [kalshi, staleness, normalization, market-data]

# Dependency graph
requires:
  - phase: 04-prediction-feeds
    provides: "KalshiProcessor with exchange_timestamp tracking"
  - phase: 12-spread-signals
    provides: "SpreadEngine staleness gate (passes_staleness_gate)"
provides:
  - "Kalshi is_stale computed from exchange_timestamp age vs staleness_threshold_ms"
  - "Stale Kalshi data correctly rejected by SpreadEngine staleness gate"
affects: [spread-signals, alerting]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Uniform staleness computation across all venue normalizers (Kalshi, Polymarket, Deribit)"

key-files:
  created: []
  modified:
    - "src/feed/kalshi/normalize.rs"

key-decisions:
  - "unwrap_or(false) for missing exchange_timestamp -- cannot determine staleness without a timestamp, so default fresh"

patterns-established:
  - "Staleness pattern: exchange_ts_ms.map(age > threshold).unwrap_or(false) -- consistent across all normalizers"

requirements-completed: [FIX-03]

# Metrics
duration: 3min
completed: 2026-02-28
---

# Phase 25 Plan 02: Kalshi Staleness Fix Summary

**Kalshi is_stale computed from exchange_timestamp age instead of hardcoded false, enabling SpreadEngine staleness gate for Kalshi data**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-28T06:56:48Z
- **Completed:** 2026-02-28T06:59:57Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Replaced hardcoded `is_stale = false` with staleness computation using exchange_ts_ms age vs staleness_threshold_ms
- Added tracing::warn log when Kalshi exchange data is stale (includes market ticker, age_ms, threshold_ms)
- Pattern now mirrors Polymarket normalizer staleness computation exactly

## Task Commits

Each task was committed atomically:

1. **Task 1: Compute Kalshi is_stale from exchange_timestamp (FIX-03)** - `d20b7a1` (fix)

## Files Created/Modified
- `src/feed/kalshi/normalize.rs` - Replaced hardcoded is_stale=false with exchange timestamp age computation and stale data warning log

## Decisions Made
- Used `unwrap_or(false)` when exchange_ts_ms is None: consistent with previous behavior and correct since staleness cannot be determined without a timestamp
- Followed Polymarket normalizer pattern exactly for consistency across venues

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three venue normalizers (Deribit, Polymarket, Kalshi) now compute is_stale from exchange timestamps
- SpreadEngine staleness gate will correctly reject stale data from all venues
- staleness_threshold_ms remains configurable per venue in venues.toml

## Self-Check: PASSED

- FOUND: src/feed/kalshi/normalize.rs
- FOUND: d20b7a1 (task 1 commit)
- FOUND: 25-02-SUMMARY.md

---
*Phase: 25-tech-debt-sweep*
*Completed: 2026-02-28*
