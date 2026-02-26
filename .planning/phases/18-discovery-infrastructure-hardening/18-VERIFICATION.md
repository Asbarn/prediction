---
phase: 18-discovery-infrastructure-hardening
verified: 2026-02-26T22:30:00Z
status: passed
score: 7/7 must-haves verified
---

# Phase 18: Discovery Infrastructure Hardening Verification Report

**Phase Goal:** Venue discovery polling is production-safe with shared rate limiters, consecutive-absence expiry guards that prevent false expirations, and batched TOML writes that eliminate race conditions
**Verified:** 2026-02-26T22:30:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | DiscoveryConfig has `consecutive_absence_threshold` (default 3) and `partial_response_threshold` (default 0.2) | VERIFIED | `src/config/events.rs` lines 224-231: both fields present with `#[serde(default = ...)]`; free functions at lines 265-266 return `3` and `0.2`; `Default` impl at lines 246-258 uses both defaults |
| 2 | `toml_writer` exposes batch mutation functions that operate on `DocumentMut` in-place without file I/O | VERIFIED | `src/events/toml_writer.rs`: `append_candidates_to_doc` (line 115) takes `&mut DocumentMut`, no fs calls; `mark_expired_batch_in_doc` (line 141) takes `&mut DocumentMut`, no fs calls |
| 3 | Batch functions support both candidate appends and expired-status marks on a single `DocumentMut` | VERIFIED | `batched_toml_write` in `lifecycle.rs` lines 614-631: calls `append_candidates_to_doc` then `mark_expired_batch_in_doc` on same `doc`, then serializes once via `atomic_write` |
| 4 | Discovery polls for Deribit and Kalshi use shared `VenueRateLimiter` instances from the feed pipeline | VERIFIED | `main.rs` lines 265-278: clones `pipeline_handles.venue_rate_limiters` HashMap into lifecycle manager; `lifecycle.rs` lines 199-215: passes `self.venue_rate_limiters.get(&Venue::Deribit)` to `discover_deribit`; lines 272-281: same for Kalshi |
| 5 | An instrument absent from a single API response is NOT marked expired — only N consecutive absences trigger expiry | VERIFIED | `AbsenceTracker` struct in `lifecycle.rs` lines 37-65: `record_absent` increments count and returns `count >= threshold`; `poll_cycle` lines 421-460: uses `record_absent` result to decide expiry, no direct-expiry on first absence |
| 6 | All TOML modifications within a single poll cycle are batched into one atomic write | VERIFIED | `poll_cycle` lines 484-499: single `batched_toml_write` call only when `needs_write = !candidates_to_append.is_empty() || !events_to_expire.is_empty()`; no per-item `append_candidate` or `mark_expired` calls exist (grep confirms zero matches) |
| 7 | A partial API response (instrument count drop >20%) is logged as suspect and does not trigger expirations | VERIFIED | `PreviousPollCounts::is_suspect` in `lifecycle.rs` lines 77-88: checks fractional drop against threshold; `poll_cycle` lines 225-239 (Deribit) and 290-304 (Kalshi): sets `deribit_suspect`/`kalshi_suspect` flag; expiry evaluation at lines 428, 445 skips venues flagged as suspect |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/config/events.rs` | DiscoveryConfig with `consecutive_absence_threshold` and `partial_response_threshold` fields | VERIFIED | Lines 224-231: both fields present; serde default functions at lines 265-266; Default impl includes both at lines 254-255 |
| `src/events/toml_writer.rs` | Batch TOML mutation functions `append_candidates_to_doc`, `mark_expired_batch_in_doc` | VERIFIED | `append_candidates_to_doc` at line 115 (pub, takes `&mut DocumentMut`); `mark_expired_batch_in_doc` at line 141 (pub, takes `&mut DocumentMut`); private `build_candidate_table` helper at line 41 shared with existing single-append function |
| `src/events/lifecycle.rs` | Hardened `poll_cycle` with `AbsenceTracker`, partial-response detection, batched writes, shared rate limiters | VERIFIED | `AbsenceTracker` struct lines 37-65; `PreviousPollCounts` struct lines 68-90; `poll_cycle` is `&mut self` with all four mechanisms wired; `batched_toml_write` at lines 614-631; `atomic_write` with Windows guard at lines 634-646 |
| `src/events/discovery.rs` | Rate-limited `discover_deribit` and `discover_kalshi` accepting `VenueRateLimiter` parameter | VERIFIED | `discover_deribit` signature at line 96-101: `rate_limiter: Option<&VenueRateLimiter>`; `limiter.wait().await` called at line 106 before each HTTP GET; same pattern in `discover_kalshi` lines 189-196 and line 205 |
| `src/main.rs` | Shared `venue_rate_limiters` passed to `ContractLifecycleManager::new()` | VERIFIED | Lines 265-278: `pipeline_handles.venue_rate_limiters.clone()` assigned to `venue_rate_limiters_for_lifecycle`, passed as final arg to `ContractLifecycleManager::new()` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/main.rs` | `src/events/lifecycle.rs` | `pipeline_handles.venue_rate_limiters` passed to `ContractLifecycleManager::new()` | WIRED | `main.rs` lines 265-278: clone from `pipeline_handles.venue_rate_limiters`, passed as parameter; `lifecycle.rs` line 131 accepts it in `new()`, stores in `self.venue_rate_limiters` |
| `src/events/lifecycle.rs` | `src/events/discovery.rs` | rate limiter passed to `discover_deribit`/`discover_kalshi` calls | WIRED | `lifecycle.rs` line 199: `let deribit_limiter = self.venue_rate_limiters.get(&Venue::Deribit)`; line 214: passed to `discover_deribit`; line 272: same for Kalshi |
| `src/events/lifecycle.rs` | `src/events/toml_writer.rs` | batch mutation functions called once per poll cycle | WIRED | `lifecycle.rs` line 30: imports `append_candidates_to_doc, mark_expired_batch_in_doc`; `batched_toml_write` at lines 624 and 627 calls both; zero calls to old `append_candidate_to_toml` or `mark_expired_in_toml` in poll_cycle |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| DISC-02 | 18-02-PLAN.md | System polls Deribit and Kalshi APIs for new instruments with shared rate limiters and consecutive-absence expiry guards | SATISFIED | `discover_deribit` and `discover_kalshi` accept `Option<&VenueRateLimiter>` and call `limiter.wait().await`; shared limiters cloned from `pipeline_handles.venue_rate_limiters`; `AbsenceTracker` requires N consecutive absences before expiry |
| LIFE-04 | 18-01-PLAN.md, 18-02-PLAN.md | System requires N consecutive absence polls before marking an instrument as expired (prevents false expirations from partial API responses) | SATISFIED | `AbsenceTracker` in `lifecycle.rs` with configurable `threshold` from `DiscoveryConfig.consecutive_absence_threshold` (default 3); `record_absent` returns `count >= threshold`; partial-response guard additionally skips expiry evaluation entirely when instrument count drops >20% |
| INTG-03 | 18-01-PLAN.md, 18-02-PLAN.md | All TOML writes use existing VenueRateLimiter and batch writes per poll cycle (not per-candidate) | SATISFIED | `batched_toml_write` reads file once, applies all mutations via `append_candidates_to_doc` + `mark_expired_batch_in_doc`, calls `atomic_write` once; grep of `lifecycle.rs` confirms zero calls to per-item write functions in `poll_cycle` |

All three requirement IDs from both plan frontmatters are accounted for. No orphaned requirements found — REQUIREMENTS.md phase mapping table shows DISC-02, LIFE-04, INTG-03 all mapped to Phase 18 (lines 90, 100, 103).

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | — | — | — | — |

No TODOs, FIXMEs, stubs, empty implementations, or placeholder comments found in any of the four modified files.

### Gaps Summary

No gaps. All seven observable truths verified against actual codebase. All artifacts exist, are substantive, and are wired. All three requirement IDs satisfied with direct code evidence.

---

## Verification Detail

### Compilation
- `cargo check`: passes with zero errors (2 unrelated dead-code warnings in pre-existing code)
- `cargo test`: 576 tests pass (519 lib + 16 + 5 + 11 + 22 unit tests + 3 doctests)

### Commits Verified
All five commits from plan summaries confirmed present in git log:
- `83265b1` — feat(18-01): extend DiscoveryConfig with absence and partial-response thresholds
- `3472051` — feat(18-01): add batch TOML mutation functions to toml_writer
- `23de6f1` — feat(18-02): add rate limiter param to discovery and AbsenceTracker to lifecycle
- `85ab1e8` — feat(18-02): refactor poll_cycle for batched writes, absence tracking, partial-response detection
- `87434ca` — feat(18-02): wire shared venue rate limiters from pipeline to lifecycle manager

### Key Implementation Notes

**Kalshi field name correction (deviation from plan):** Plan pseudocode used `kalshi.instrument` but `KalshiMapping` struct has a `ticker` field. Implementation correctly uses `kalshi.ticker` at `lifecycle.rs` lines 446 and 451. This was caught and auto-fixed during execution.

**`VenueRateLimiter` is Arc-wrapped:** `src/feed/reliability/rate_limiter.rs` line 25 shows the struct contains `limiter: Arc<GovernorLimiter>`, making `.clone()` cheap — cloning the HashMap in `main.rs` shares the underlying limiter state across lifecycle, feeds, and settlement checkers.

**`poll_cycle` changed to `&mut self`:** Required for mutable access to `absence_tracker` and `previous_poll_counts`; `run()` uses owned `self` so the `select!` call site compiles cleanly.

**Windows atomic write guard:** `#[cfg(target_os = "windows")]` remove-before-rename present at `lifecycle.rs` lines 639-642 — directly applicable to the project's development environment (Windows 11).

---

_Verified: 2026-02-26T22:30:00Z_
_Verifier: Claude (gsd-verifier)_
