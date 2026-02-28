---
phase: 23-dynamic-supervisor-subscriptions
verified: 2026-02-27T20:00:00Z
status: passed
score: 3/3 must-haves verified
re_verification: null
gaps: []
human_verification:
  - test: "Approve a new instrument in events.toml and observe the system reconnect and subscribe without restart"
    expected: "All three supervisors log 'instrument list updated, reconnecting' and reconnect with the new instrument list within one config reload cycle"
    why_human: "Requires live venue connections and a running SubscriptionManager feeding real watch channel updates; cannot verify runtime reconnect behavior via static analysis"
---

# Phase 23: Dynamic Supervisor Subscriptions Verification Report

**Phase Goal:** Operator can approve new instruments or archive expired ones and see the system subscribe/unsubscribe feeds without restart
**Verified:** 2026-02-27T20:00:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | When operator approves a new instrument in events.toml, the system subscribes to that instrument's feeds on the relevant venues within one config reload cycle without restart | VERIFIED | All three supervisors have `changed()` select branches that break the inner forwarding loop and re-enter the outer reconnect loop, where `borrow().clone()` reads the updated instrument list pushed by SubscriptionManager. SubscriptionManager.run() reacts to Notify fired by the config hot-reload subscriber in main.rs. |
| 2 | When an event is archived (moved to events_archive.toml with Retired status), the system unsubscribes from that instrument's feeds within one config reload cycle without restart | VERIFIED | Same mechanism as truth 1 in reverse. SubscriptionManager computes diffs against `active_approved()` and pushes reduced instrument lists. `changed()` fires in each supervisor's inner select, backoff resets, and the outer loop creates a fresh client subscribing only to remaining instruments. The retired instrument's feed stops because the new connection never subscribes to it. |
| 3 | All three venue supervisors (Deribit, Polymarket, Kalshi) accept watch channel updates and reconnect with the updated instrument list | VERIFIED | Deribit: `instruments_rx: watch::Receiver<Vec<String>>` (line 31, deribit/supervisor.rs). Polymarket: `assets_rx: watch::Receiver<Vec<PolymarketSubscription>>` (line 26, polymarket/supervisor.rs). Kalshi: `tickers_rx: watch::Receiver<Vec<String>>` (line 29, kalshi/supervisor.rs). All three have `changed()` branch in inner select at lines 122, 94, 109 respectively. |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/feed/deribit/supervisor.rs` | DeribitSupervisor with watch::Receiver<Vec<String>> and changed() select branch | VERIFIED | `instruments_rx: watch::Receiver<Vec<String>>` field (line 31); `borrow_and_update()` at run() init (line 69); `borrow().clone()` at reconnect top (line 90); `changed()` with Ok/Err arms in inner select (lines 122-135); `backoff.reset()` in Ok arm (line 126); `run(mut self, ...)` signature (line 65) |
| `src/feed/polymarket/supervisor.rs` | PolymarketSupervisor with watch::Receiver<Vec<PolymarketSubscription>> and changed() select branch | VERIFIED | `assets_rx: watch::Receiver<Vec<PolymarketSubscription>>` field (line 26); `borrow_and_update()` at run() init (line 44); `borrow().clone()` + PolymarketSubscription-to-PolymarketAsset conversion at reconnect top (lines 64-69); `changed()` with Ok/Err arms (lines 94-105); `backoff.reset()` in Ok arm (line 98); `run(mut self, ...)` (line 42) |
| `src/feed/kalshi/supervisor.rs` | KalshiSupervisor with watch::Receiver<Vec<String>> and changed() select branch | VERIFIED | `tickers_rx: watch::Receiver<Vec<String>>` field (line 29); `borrow_and_update()` at run() init (line 56); `borrow().clone()` + config injection at reconnect top (lines 76-78); `changed()` with Ok/Err arms (lines 109-120); `backoff.reset()` in Ok arm (line 113); `run(mut self, ...)` (line 54) |
| `src/feed/pipeline.rs` | run_live_multi_venue threading subscription receivers to supervisor constructors | VERIFIED | `run_multi_venue_pipeline()` signature accepts `subscription_rx: Option<SubscriptionReceivers>` (line 95); destructured at `run_live_multi_venue()` entry (lines 135-138); per-venue `match` blocks create receivers or one-shot fallback channels before each supervisor constructor (lines 161-167, 217-227, 285-291); `subscription_rx: None` returned in PipelineHandles (line 351) since receivers are consumed |
| `src/main.rs` | Subscription receivers passed through pipeline function instead of post-hoc attachment | VERIFIED | `sub_receivers` passed as final argument to `run_multi_venue_pipeline()` (line 229); no post-hoc `pipeline_handles.subscription_rx = ...` assignment present; SubscriptionManager spawned with `sub_senders` after pipeline starts (lines 345-354) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/subscription/manager.rs` | `src/feed/deribit/supervisor.rs` | watch::Sender -> watch::Receiver for Vec<String> instruments | WIRED | `SubscriptionReceivers.deribit: watch::Receiver<Vec<String>>` (manager.rs line 36) destructured in pipeline.rs line 136 and passed into `DeribitSupervisor::new()`. `instruments_rx.changed()` in inner select (deribit/supervisor.rs line 122). |
| `src/subscription/manager.rs` | `src/feed/polymarket/supervisor.rs` | watch::Sender -> watch::Receiver for Vec<PolymarketSubscription> assets | WIRED | `SubscriptionReceivers.polymarket: watch::Receiver<Vec<PolymarketSubscription>>` (manager.rs line 37) destructured in pipeline.rs line 136 and passed into `PolymarketSupervisor::new()`. `assets_rx.changed()` in inner select (polymarket/supervisor.rs line 94). |
| `src/subscription/manager.rs` | `src/feed/kalshi/supervisor.rs` | watch::Sender -> watch::Receiver for Vec<String> tickers | WIRED | `SubscriptionReceivers.kalshi: watch::Receiver<Vec<String>>` (manager.rs line 38) destructured in pipeline.rs line 136 and passed into `KalshiSupervisor::new()`. `tickers_rx.changed()` in inner select (kalshi/supervisor.rs line 109). |
| `src/main.rs` | `src/feed/pipeline.rs` | Option<SubscriptionReceivers> passed into run_multi_venue_pipeline() | WIRED | `sub_receivers` variable passed as final argument at main.rs line 229. `run_multi_venue_pipeline()` signature accepts `subscription_rx: Option<SubscriptionReceivers>` at pipeline.rs line 95. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| SUB-01 | 23-01-PLAN.md | System subscribes to newly approved instrument feeds without restart when operator sets `approved = true` in events.toml | SATISFIED | All three supervisors have `changed()` branches that reconnect with updated instrument lists read via `borrow().clone()`. SubscriptionManager pushes updates to watch senders when config hot-reload detects newly approved instruments. Marked complete in REQUIREMENTS.md (line 78). |
| SUB-02 | 23-01-PLAN.md | System unsubscribes from expired/retired instrument feeds without restart when events are archived | SATISFIED | Same reconnect mechanism. When archived instruments are removed from `active_approved()`, SubscriptionManager pushes reduced lists. Supervisors reconnect with the shorter list; the archived instrument's feed stops because the fresh client does not subscribe to it. Marked complete in REQUIREMENTS.md (line 79). |

**Orphaned requirements check:** REQUIREMENTS.md maps SUB-01 and SUB-02 to Phase 23 (lines 78-79). Both are declared in the plan's `requirements` field. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | - |

No TODOs, FIXMEs, placeholder returns, empty handlers, or stub implementations found in any of the six modified files.

### Human Verification Required

#### 1. Live Reconnect on Instrument Approval

**Test:** In a running live instance, add a new instrument with `approved = true` to events.toml and save the file.
**Expected:** Within one config reload cycle (~5s), all three venue supervisors log "instrument list updated, reconnecting", reconnect, and the new instrument's feed data begins flowing through the pipeline.
**Why human:** Requires live venue WebSocket connections, a running SubscriptionManager, and actual config hot-reload triggering. Cannot verify runtime reconnect behavior through static grep-based analysis.

#### 2. Live Unsubscribe on Event Archival

**Test:** In a running live instance, move an existing approved event to events_archive.toml (or set its status to Retired) and save.
**Expected:** Within one config reload cycle, all three supervisors reconnect without the archived instrument. Log entries show the reduced instrument count. No further data for the retired instrument flows through.
**Why human:** Same reason as test 1; requires runtime observation.

### Gaps Summary

No gaps. All three observable truths are verified by substantive, wired implementations:

- All three supervisors have been modified from static `Vec<String>` fields to `watch::Receiver` fields, with proper `borrow_and_update()` at init, `borrow().clone()` at reconnect top, and `changed()` Ok/Err branches in their inner select loops.
- The pipeline function correctly threads `Option<SubscriptionReceivers>` from `main.rs` through `run_multi_venue_pipeline()` into `run_live_multi_venue()`, where it is destructured and each per-venue receiver is passed to its supervisor constructor.
- Mock/Replay modes receive one-shot watch channels seeded with config values so the supervisor interface remains uniform with no behavioral change.
- The post-hoc attachment pattern from main.rs has been removed. SubscriptionManager is spawned with `sub_senders` after the pipeline starts.
- Both requirement IDs (SUB-01, SUB-02) assigned to Phase 23 in REQUIREMENTS.md are satisfied by the implementation.
- Both task commits (`8377e48`, `b6fada2`) are confirmed present in git history.

The phase goal is fully achieved at the code level. Human verification of the runtime reconnect behavior is recommended before closing the milestone.

---

_Verified: 2026-02-27T20:00:00Z_
_Verifier: Claude (gsd-verifier)_
