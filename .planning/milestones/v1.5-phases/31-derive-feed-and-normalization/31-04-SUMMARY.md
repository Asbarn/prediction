---
phase: 31-derive-feed-and-normalization
plan: 04
subsystem: feed
tags: [derive, normalize, processor, market-snapshot, orderbook, ticker-slim, usdc]

# Dependency graph
requires:
  - phase: 31-01
    provides: "DeriveMessage types, DeriveBook, DeriveChannelKind, channel helpers"
provides:
  - "DeriveProcessor for raw WS frame -> MarketSnapshot conversion"
  - "CleanupEvent.derive_instruments field for processor state eviction"
affects: [32-derive-subscription-wiring, feed-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns: [dual-source-snapshot-gating, usdc-passthrough-pricing]

key-files:
  created: [src/feed/derive/normalize.rs]
  modified: [src/feed/derive/mod.rs, src/subscription/manager.rs]

key-decisions:
  - "Snapshot emission requires BOTH book AND ticker data (dual-source gating)"
  - "USDC prices pass through without conversion (unlike Deribit BTC-inverse)"
  - "Stale data skips emission entirely (not emitted with is_stale=true)"
  - "rho=0.0 hardcoded since Derive does not provide rho in option_pricing"

patterns-established:
  - "Dual-source gating: DeriveProcessor only emits MarketSnapshot when both orderbook and ticker_slim data exist"
  - "USDC passthrough: Derive prices flow through as-is, no denomination conversion"

requirements-completed: [FEED-03, FEED-05, NORM-03, NORM-04]

# Metrics
duration: 8min
completed: 2026-03-04
---

# Phase 31 Plan 04: Derive Processor and Normalization Summary

**DeriveProcessor converts raw WS frames to MarketSnapshots with USDC prices, greeks, and staleness detection; CleanupEvent extended with derive_instruments**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-04T16:16:23Z
- **Completed:** 2026-03-04T16:24:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- DeriveProcessor with message routing, per-instrument book/ticker state, and MarketSnapshot emission
- Dual-source snapshot gating: requires both orderbook AND ticker_slim data before emitting
- CleanupEvent struct extended with derive_instruments field for processor state eviction
- 7 unit tests covering snapshot building, message routing, recording, and staleness

## Task Commits

Each task was committed atomically:

1. **Task 2: Add derive_instruments field to CleanupEvent** - `4af4470` (feat)
2. **Task 1: Create DeriveProcessor for message parsing and MarketSnapshot emission** - `6fe45fb` (feat)

_Note: Task 2 committed first as Task 1 depends on derive_instruments field_

## Files Created/Modified
- `src/feed/derive/normalize.rs` - DeriveProcessor with message routing, book/ticker state, MarketSnapshot emission, staleness detection, recording wiring
- `src/feed/derive/mod.rs` - Added `pub mod normalize` declaration
- `src/subscription/manager.rs` - Added `derive_instruments: Vec<String>` to CleanupEvent struct and construction site

## Decisions Made
- Snapshot emission requires BOTH book AND ticker data (dual-source gating) -- matches plan requirement that both data sources are needed for a complete snapshot
- USDC prices pass through without conversion -- Derive uses linear USDC pricing unlike Deribit's BTC-inverse
- Stale snapshots are skipped entirely rather than emitted with is_stale=true -- plan specifies "do NOT emit snapshot"
- rho hardcoded to 0.0 since Derive's option_pricing does not include rho field
- Task execution order swapped (Task 2 before Task 1) because Task 1 depends on derive_instruments field

## Deviations from Plan

None - plan executed exactly as written (task order adjusted for dependency but all content matches plan).

## Issues Encountered
- `crate::config::venues::DeriveConfig` import path was private module; resolved by using re-exported `crate::config::DeriveConfig`
- 31-03 running in parallel had already added `pub mod client` and `pub mod supervisor` to mod.rs; reconciled by reading current state before editing

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- DeriveProcessor ready for integration with DeriveClient (31-03) and feed pipeline (Phase 32)
- CleanupEvent ready for Derive instrument cleanup wiring in Phase 32
- All Derive feed components (messages, book, channels, client, supervisor, normalize) are complete

---
*Phase: 31-derive-feed-and-normalization*
*Completed: 2026-03-04*
