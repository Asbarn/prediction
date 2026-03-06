---
phase: 32-pipeline-wiring-and-observability
verified: 2026-03-06T12:00:00Z
status: passed
score: 8/8 must-haves verified
---

# Phase 32: Pipeline Wiring and Observability Verification Report

**Phase Goal:** Derive snapshots flow through the live multi-venue pipeline and SpreadEngine/SignalEngine produce cross-venue signals automatically
**Verified:** 2026-03-06
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SubscriptionManager tracks Derive instruments with HashSet diff reconciliation | VERIFIED | `current_derive: HashSet<String>` field (manager.rs:86), `compute_diff` called at line 190, diff logic lines 255-271 |
| 2 | Watch channel pushes sorted Derive instrument updates to supervisor | VERIFIED | Lines 315-319: sorted Vec created, `derive_tx.send_replace(instruments)` called |
| 3 | CleanupEvent.derive_instruments populated from actual diff (not Vec::new()) | VERIFIED | Line 343: `derive_instruments: removed_dr.into_iter().collect()` |
| 4 | Subscription metrics emit with venue=derive label | VERIFIED | Lines 366-400: gauge `subscription_active`, counters `subscription_activations_total` and `subscription_removals_total` all with `"venue" => "derive"` |
| 5 | Derive snapshots flow through run_live_multi_venue and reach SpreadEngine/SignalEngine/PaperTradeTracker | VERIFIED | pipeline.rs:370-427: complete Derive block spawns supervisor, processor, and forward_snapshots to shared fan-in channel; block placed before `drop(snapshot_tx)` at line 430 |
| 6 | DeriveSupervisor receives dynamic instrument updates via watch channel from SubscriptionManager | VERIFIED | pipeline.rs:391-397: `derive_rx` from SubscriptionReceivers passed as `instruments_rx`; line 399 passes to `DeriveSupervisor::new()` |
| 7 | Prometheus metrics expose Derive connection status, message rate, subscription count, and reconnection events | VERIFIED | health.rs:49 `feed_available` gauge, health.rs:78 `feed_reconnections_total` counter, manager.rs:366 `subscription_active` gauge, manager.rs:393-399 activation/removal counters |
| 8 | Derive pipeline has crash isolation via child CancellationToken | VERIFIED | pipeline.rs:374: `let venue_cancel = cancel.child_token();` |

**Score:** 8/8 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/subscription/manager.rs` | Derive venue support in SubscriptionManager | VERIFIED | Contains `current_derive`, `derive_tx`, 4-tuple return, diff/reconcile block, metrics -- 469 lines, fully substantive |
| `src/feed/pipeline.rs` | Derive pipeline block in run_live_multi_venue | VERIFIED | Contains `DeriveSupervisor`, `DeriveProcessor`, `forward_snapshots(Venue::Derive)` -- lines 370-427, fully wired |
| `src/main.rs` | Derive venue availability log | VERIFIED | Line 153: `derive = "available (public, no auth)"` in venue availability tracing |
| `src/feed/health.rs` | Reconnection counter metric for all venues | VERIFIED | Line 78: `feed_reconnections_total` counter in `increment_connections()` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| manager.rs | SubscriptionSenders/Receivers | `derive` field with `watch::Sender`/`Receiver` | WIRED | Lines 43, 54: both structs have `derive` field |
| manager.rs reconcile() | CleanupEvent | derive_instruments from removed set | WIRED | Line 343: `removed_dr.into_iter().collect()` |
| pipeline.rs | snapshot_tx | forward_snapshots with Venue::Derive | WIRED | Line 417-424: clones fan_in_tx, spawns forward_snapshots |
| pipeline.rs | DeriveSupervisor | tokio::spawn(supervisor.run(supervisor_tx)) | WIRED | Lines 398-405: constructed and spawned |
| pipeline.rs | DeriveProcessor | tokio::spawn(processor.run()) | WIRED | Lines 407-414: constructed and spawned |
| main.rs | SubscriptionReceivers | derive field destructured | WIRED | pipeline.rs:160-163 destructures `rx.derive`; main.rs logs availability |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| PIPE-03 | 32-01 | SubscriptionManager extended with Derive venue support (HashSet diff, watch channel, Notify ordering) | SATISFIED | 4-venue reconciliation with identical pattern to Deribit/Polymarket/Kalshi |
| PIPE-04 | 32-02 | Derive wired into run_live_multi_venue -- SpreadEngine, SignalEngine, PaperTradeTracker receive Derive snapshots automatically | SATISFIED | Derive pipeline block spawns supervisor/processor/forwarder, publishes to shared fan-in channel consumed by all downstream engines |
| PIPE-05 | 32-02 | Prometheus metrics for Derive feed (connection state, message rate, subscription count) | SATISFIED | feed_available gauge, feed_reconnections_total counter, subscription_active gauge, subscription_activations/removals counters |

No orphaned requirements found -- all 3 requirement IDs (PIPE-03, PIPE-04, PIPE-05) mapped to this phase in REQUIREMENTS.md are claimed by plans and satisfied.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns detected in any modified files |

No TODOs, FIXMEs, placeholders, empty implementations, or stub patterns found in manager.rs, pipeline.rs, health.rs, or main.rs.

### Human Verification Required

### 1. Derive WebSocket Connection and Snapshot Flow

**Test:** Start the application in Live mode with a Derive instrument configured in events.toml. Observe logs for "Derive pipeline started" and subsequent MarketSnapshot flow.
**Expected:** DeriveSupervisor connects to Lyra WebSocket, DeriveProcessor normalizes messages into MarketSnapshots, SpreadEngine/SignalEngine produce cross-venue signals including Derive data.
**Why human:** Requires live WebSocket connection to Derive/Lyra API and real-time observation of data flow.

### 2. Prometheus Metrics Endpoint

**Test:** With the application running, query the Prometheus metrics endpoint for `feed_reconnections_total{venue="derive"}`, `subscription_active{venue="derive"}`, and `feed_available{venue="derive"}`.
**Expected:** All metrics present with appropriate values reflecting current state.
**Why human:** Requires running application with Prometheus recorder installed and HTTP endpoint accessible.

### Gaps Summary

No gaps found. All 8 must-haves verified across both plans. All 3 requirements (PIPE-03, PIPE-04, PIPE-05) satisfied. The Derive pipeline is fully wired into run_live_multi_venue following the identical pattern established by Deribit/Polymarket/Kalshi. The project compiles cleanly with `cargo check`.

---

_Verified: 2026-03-06_
_Verifier: Claude (gsd-verifier)_
