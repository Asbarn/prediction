---
phase: 25-tech-debt-sweep
verified: 2026-02-28T09:00:00Z
status: passed
score: 7/7 must-haves verified
re_verification: false
---

# Phase 25: Tech Debt Sweep Verification Report

**Phase Goal:** Three behavior-changing tech debt items from v1.0 are fixed so metrics and staleness detection reflect real data
**Verified:** 2026-02-28T09:00:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | iv_spread on ArbSignal contains actual ask_iv - bid_iv from IV solver, not 0.0 | VERIFIED | `src/signal/engine.rs:499` reads `prob.iv_spread` directly; `src/pricing/engine.rs:393` populates it as `iv_spread.max(0.0)` |
| 2 | options_leg.book_depth_levels on ArbSignal reflects actual Deribit snapshot depth, not 0 | VERIFIED | `src/signal/engine.rs:547` reads `prob.options_book_depth`; `src/pricing/engine.rs:394` populates from `snapshot.depth_bids.len()` |
| 3 | Deribit book subscription channel uses book_depth_levels from [deribit] config, not hardcoded 20 | VERIFIED | `src/feed/deribit/channels.rs:121` uses `format!("book.{}.none.{}.100ms", inst, book_depth_levels)`; `src/feed/deribit/client.rs:105` passes `self.config.book_depth_levels` |
| 4 | Existing configs without book_depth_levels work with default 20 | VERIFIED | `src/config/venues.rs:101` has `#[serde(default = "default_book_depth_levels")]`; default returns 20 (line 54-56) |
| 5 | Kalshi is_stale is true when exchange_timestamp age exceeds staleness_threshold_ms | VERIFIED | `src/feed/kalshi/normalize.rs:272-278` computes `age_ms > self.staleness_threshold_ms` |
| 6 | Kalshi is_stale is false when exchange_timestamp is fresh or unavailable | VERIFIED | `.unwrap_or(false)` at line 278; None case defaults to false (cannot determine staleness without timestamp) |
| 7 | SpreadEngine correctly rejects stale Kalshi snapshots via existing gate | VERIFIED | `is_stale` field on MarketSnapshot is populated (line 326) and fed into snapshot channel; existing staleness gate in SpreadEngine will consume it |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/pricing/types.rs` | ImpliedProbability with iv_spread and options_book_depth fields | VERIFIED | `pub iv_spread: f64` at line 178; `pub options_book_depth: usize` at line 182 |
| `src/config/venues.rs` | DeribitConfig with book_depth_levels field | VERIFIED | `pub book_depth_levels: u32` at line 102 with `#[serde(default = "default_book_depth_levels")]` |
| `src/feed/deribit/channels.rs` | Parameterized book depth in channel construction | VERIFIED | `build_subscription_channels(instruments: &[String], book_depth_levels: u32)` at line 117; format string uses `book_depth_levels` parameter at line 121 |
| `src/feed/kalshi/normalize.rs` | Staleness computation from exchange_timestamp | VERIFIED | Lines 272-278 compute is_stale from `exchange_ts_ms` age vs `self.staleness_threshold_ms` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/pricing/engine.rs` | `src/pricing/types.rs` ImpliedProbability.iv_spread | `iv_spread.max(0.0)` at line 393 | WIRED | `iv_spread` computed as `ask_iv - bid_iv` (line 282 context); assigned with clamp in normal pricing path |
| `src/signal/engine.rs` | `src/pricing/types.rs` ImpliedProbability.iv_spread | `prob.iv_spread` at line 499 | WIRED | Direct field read; replaces the old multi-line match block that always returned 0.0 |
| `src/feed/deribit/channels.rs` | `src/config/venues.rs` DeribitConfig.book_depth_levels | `build_subscription_channels` parameter | WIRED | Client passes `self.config.book_depth_levels` at `src/feed/deribit/client.rs:105`; function uses it in format string |
| `src/pricing/engine.rs` | `src/pricing/types.rs` ImpliedProbability.options_book_depth | `snapshot.depth_bids.len()` at line 394 | WIRED | Normal path uses actual snapshot depth; near-expiry path correctly uses 0 (line 508) |
| `src/signal/engine.rs` | ArbSignal options_leg.book_depth_levels | `prob.options_book_depth` at line 547 | WIRED | Replaces previous hardcoded 0 |
| `src/feed/kalshi/normalize.rs` | MarketSnapshot.is_stale | `age_ms > self.staleness_threshold_ms` at lines 272-278 | WIRED | Computed value assigned to `is_stale` field of MarketSnapshot at line 326 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| FIX-01 | 25-01-PLAN.md | iv_spread populated from IV solver metadata instead of always 0.0 | SATISFIED | `prob.iv_spread` at signal/engine.rs:499; `iv_spread.max(0.0)` at pricing/engine.rs:393 |
| FIX-02 | 25-01-PLAN.md | Options book_depth_levels read from config instead of hardcoded 0 | SATISFIED | `#[serde(default)]` in venues.rs; parameterized channels.rs; `prob.options_book_depth` in signal/engine.rs:547 |
| FIX-03 | 25-02-PLAN.md | Kalshi is_stale computed from exchange_timestamp instead of always false | SATISFIED | Staleness computation at normalize.rs:272-278; no `let is_stale = false` found in file |

No orphaned requirements. All three FIX-* IDs declared in plan frontmatter are covered, and REQUIREMENTS.md traceability table marks all three Complete for Phase 25.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | - | - | - | - |

No TODO/FIXME/placeholder comments found in any modified file. No stub return patterns detected. No console.log equivalents (the `tracing::warn` for stale Kalshi data is production-appropriate behavior, not a stub).

Note: `channels.rs` test assertions reference `.none.20.100ms` string literals at lines 138, 178, 215, 231 -- these are test expectations for the default depth=20 case, not hardcoded production format strings. The production format string at line 121 is correctly parameterized.

### Human Verification Required

None. All three fixes are data-pipeline wiring changes with no visual or UX components. Verification is fully automated via code inspection and test execution.

### Build and Test Results

- `cargo build`: Finished with 1 warning (pre-existing dead_code), zero errors
- `cargo test`: 548 unit tests + 16 integration tests + 22 pipeline tests + 5 feed tests + 11 other tests + 3 doc-tests = all passed, 0 failed

### Gaps Summary

No gaps. All three behavior-changing fixes are fully implemented, wired through the pipeline, and covered by passing tests.

- FIX-01 (iv_spread): Field exists on ImpliedProbability, populated in both pricing paths, read in CrossAssetEngine, flows to ArbSignal.
- FIX-02 (book_depth_levels): DeribitConfig has the serde-defaulted field, channel builder is parameterized, client passes config value, snapshot depth flows to options leg.
- FIX-03 (Kalshi is_stale): Hardcoded `false` replaced with timestamp age computation, mirrors Polymarket pattern, warning log added, test suite confirms exchange_timestamp propagation.

---

_Verified: 2026-02-28T09:00:00Z_
_Verifier: Claude (gsd-verifier)_
