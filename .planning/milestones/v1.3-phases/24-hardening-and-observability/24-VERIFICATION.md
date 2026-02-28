---
phase: 24-hardening-and-observability
verified: 2026-02-27T22:30:00Z
status: passed
score: 10/10 must-haves verified
re_verification: false
---

# Phase 24: Hardening and Observability Verification Report

**Phase Goal:** Subscription lifecycle is observable via metrics and safe to operate with dry-run mode, and unsubscribed instruments leave no stale state
**Verified:** 2026-02-27T22:30:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | Prometheus gauge `subscription_active{venue}` reflects current subscription count per venue after each reconciliation | VERIFIED | `manager.rs:316-321` — 3 `metrics::gauge!` calls after current state update |
| 2  | Prometheus counters `subscription_activations_total{venue}` and `subscription_removals_total{venue}` increment by diff size on each reconciliation | VERIFIED | `manager.rs:324-345` — 6 `metrics::counter!` calls conditioned on venue changed + diff size |
| 3  | When `dry_run = true`, reconciliation logs diffs and updates internal state, but does NOT send watch channel updates, cleanup events, or emit metrics | VERIFIED | `manager.rs:244-259` — early return guard after logging, sets `current_*` sets, returns before watch sends and metrics |
| 4  | When `dry_run = true`, metrics are NOT emitted (gauges and counters reflect actual state only) | VERIFIED | Dry-run guard `return;` at line 258 exits before all `metrics::` calls at lines 316-345 |
| 5  | After an instrument is unsubscribed, `SpreadEngine.latest` and `SpreadEngine.stats` no longer contain entries for the corresponding event_id | VERIFIED | `spread/engine.rs:173-191` — `cleanup_rx.recv()` branch retains only `active_ids` from registry |
| 6  | After an instrument is unsubscribed, `CrossAssetEngine.latest_prob`, `latest_pred`, and `stats` no longer contain entries for the corresponding event_id | VERIFIED | `signal/engine.rs:188-209` — `cleanup_rx.recv()` branch retains all three maps against `active_ids` |
| 7  | After an instrument is unsubscribed, `DeribitProcessor.books` and `tickers` no longer contain entries for the removed Deribit instruments | VERIFIED | `feed/deribit/normalize.rs:132-144` — `cleanup_rx.recv()` branch retains entries not in `deribit_instruments` set |
| 8  | After an instrument is unsubscribed, `KalshiProcessor.books` and `last_exchange_ts` no longer contain entries for the removed Kalshi tickers | VERIFIED | `feed/kalshi/normalize.rs:94-106` — `cleanup_rx.recv()` branch retains entries not in `kalshi_tickers` set |
| 9  | After an instrument is unsubscribed, `PricingEngine.iv_cache` no longer contains entries for the removed Deribit instruments (smiles/smile_points left intact) | VERIFIED | `pricing/engine.rs:125-137` — evicts `iv_cache` by `deribit_instruments`; no `smiles`/`smile_points` eviction |
| 10 | Cleanup is event-driven via mpsc channel, not periodic polling | VERIFIED | All 5 engines use `tokio::select!` with `Some(cleanup) = cleanup_rx.recv()` branch — no timers involved |

**Score:** 10/10 truths verified

---

### Required Artifacts

#### Plan 01 Artifacts

| Artifact | Provides | Status | Details |
|----------|----------|--------|---------|
| `src/config/system.rs` | `SubscriptionConfig` struct with `dry_run` field | VERIFIED | Lines 209-227: struct with `pub dry_run: bool`, `#[serde(default)]`, `Default` impl (false); `SystemConfig.subscription` field at line 58 |
| `src/subscription/manager.rs` | Metrics emission, dry-run guard, `CleanupEvent` struct | VERIFIED | `CleanupEvent` at lines 22-32; `dry_run`/`cleanup_txs` fields at lines 82-83; dry-run guard lines 244-259; metrics lines 316-345 |
| `src/main.rs` | `dry_run` config passed to `SubscriptionManager` | VERIFIED | Line 353: `config.system.subscription.dry_run` passed to `SubscriptionManager::new()` |

#### Plan 02 Artifacts

| Artifact | Provides | Status | Details |
|----------|----------|--------|---------|
| `src/spread/engine.rs` | `cleanup_rx` select branch evicting stale `latest`/`stats` | VERIFIED | Lines 173-191: branch uses registry `active_approved()` to retain matching entries |
| `src/signal/engine.rs` | `cleanup_rx` select branch evicting `latest_prob`/`latest_pred`/`stats` | VERIFIED | Lines 188-209: branch retains all three HashMaps against registry active set |
| `src/pricing/engine.rs` | `cleanup_rx` select branch evicting stale `iv_cache` entries | VERIFIED | Lines 125-137: evicts by `deribit_instruments` directly; smiles/smile_points untouched |
| `src/feed/deribit/normalize.rs` | `cleanup_rx` select branch evicting stale `books`/`tickers` | VERIFIED | Lines 132-144: `cleanup_rx` field on struct, constructor param, select branch present |
| `src/feed/kalshi/normalize.rs` | `cleanup_rx` select branch evicting stale `books`/`last_exchange_ts` | VERIFIED | Lines 94-106: `cleanup_rx` field on struct, constructor param, select branch present |
| `src/feed/pipeline.rs` | Cleanup channel creation and receiver threading to processors | VERIFIED | Lines 146-150: 5 `mpsc::channel::<CleanupEvent>(8)` created; `deribit_cleanup_rx` passed to `DeribitProcessor::new()` at line 203; `kalshi_cleanup_rx` passed at line 329; `cleanup_txs` collected at lines 370-376; engine receivers in `PipelineHandles.engine_cleanup_rxs` |
| `src/main.rs` | Cleanup senders passed to `SubscriptionManager`; engine receivers to engines | VERIFIED | Line 234: `cleanup_txs` extracted; line 354: passed to `SubscriptionManager::new()`; lines 439-465: `engine_cleanup_rxs` destructured and passed to SpreadEngine/PricingEngine/CrossAssetEngine |

---

### Key Link Verification

#### Plan 01 Key Links

| From | To | Via | Status | Evidence |
|------|----|-----|--------|----------|
| `src/config/system.rs` | `src/subscription/manager.rs` | `dry_run: bool` passed to `SubscriptionManager::new()` | WIRED | `manager.rs:97` — `dry_run: bool` parameter; `system.rs:220` — `pub dry_run: bool`; `main.rs:353` — passed from config |
| `src/subscription/manager.rs` | metrics crate | `gauge!/counter!` macros in `reconcile()` | WIRED | 3 gauge calls (lines 316-321) + 6 counter calls (lines 324-345) confirmed by grep count |

#### Plan 02 Key Links

| From | To | Via | Status | Evidence |
|------|----|-----|--------|----------|
| `src/subscription/manager.rs` | `src/spread/engine.rs` | mpsc cleanup channel — `cleanup_rx.recv()` | WIRED | `spread/engine.rs:173` — `Some(_cleanup) = cleanup_rx.recv()` branch active |
| `src/subscription/manager.rs` | `src/signal/engine.rs` | mpsc cleanup channel — `cleanup_rx.recv()` | WIRED | `signal/engine.rs:188` — `Some(_cleanup) = cleanup_rx.recv()` branch active |
| `src/subscription/manager.rs` | `src/feed/deribit/normalize.rs` | mpsc cleanup channel — `cleanup_rx.recv()` | WIRED | `normalize.rs:132` — `Some(cleanup) = self.cleanup_rx.recv()` branch active |
| `src/subscription/manager.rs` | `src/feed/kalshi/normalize.rs` | mpsc cleanup channel — `cleanup_rx.recv()` | WIRED | `normalize.rs:94` — `Some(cleanup) = self.cleanup_rx.recv()` branch active |
| `src/subscription/manager.rs` | `src/pricing/engine.rs` | mpsc cleanup channel — `cleanup_rx.recv()` | WIRED | `engine.rs:125` — `Some(cleanup) = cleanup_rx.recv()` branch active |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| OBS-01 | 24-01 | Prometheus gauges show per-venue active subscription count | SATISFIED | `manager.rs:316-321` — 3 `metrics::gauge!("subscription_active", "venue" => ...)` calls after state update |
| OBS-02 | 24-01 | Prometheus counters track subscription activations and removals per venue | SATISFIED | `manager.rs:324-345` — `subscription_activations_total` and `subscription_removals_total` per venue |
| OPS-01 | 24-01 | Dry-run reconciliation mode logs what actions would be taken without sending subscribe/unsubscribe commands | SATISFIED | `manager.rs:244-259` — dry-run guard logs `"DRY RUN: reconciliation would apply these changes"`, updates internal state, returns before all side effects |
| SUB-05 | 24-02 | Stale internal state (order books, snapshots, rolling stats) is cleaned up after instruments are unsubscribed | SATISFIED | All 5 stateful engines have `cleanup_rx` select branches with `.retain()` calls; channels fully wired from `SubscriptionManager` senders through `pipeline.rs` to engine receivers |

**Note on OBS-03:** This requirement (structured tracing logs emit subscription diffs on each reconciliation) was assigned to Phase 22 in REQUIREMENTS.md and is NOT claimed by any Phase 24 plan — correctly excluded. Evidence of OBS-03 implementation exists at `manager.rs:185-240` (diff logging per venue), completed in Phase 22.

**Coverage:** 4/4 Phase 24 requirements satisfied. Zero orphaned requirements for this phase.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/subscription/manager.rs` | 301 | `event_ids: Vec::new()` with comment "Populated by Plan 02..." | INFO | Intentional design decision documented in both plans: engines use registry `active_approved()` instead of event_ids in CleanupEvent. The empty Vec is correct and the comment is accurate. Not a stub. |

No blockers or warnings found. The `event_ids: Vec::new()` in `CleanupEvent` is a deliberate architectural choice (research Pitfall 2): SpreadEngine and CrossAssetEngine use the `EventRegistry.active_approved()` approach which is more authoritative than pre-resolved IDs that may already be removed from the registry by the time the cleanup event arrives.

---

### Human Verification Required

None. All critical behaviors are fully verifiable through static code analysis:
- Metrics emission paths are direct macro calls with no conditional indirection beyond the dry-run guard
- Dry-run guard logic is straightforward: `if self.dry_run { ...; return; }`
- All 5 cleanup branches are structurally complete with `.retain()` calls and log output
- Compilation passes (`cargo check` succeeds with only pre-existing unrelated warnings)

---

### Commit Verification

All 4 task commits exist in git history and are reachable:
- `116b51f` — feat(24-01): add SubscriptionConfig, CleanupEvent struct, and dry_run/cleanup_txs fields
- `bb95807` — feat(24-01): add metrics emission, dry-run guard, and cleanup sender to reconcile()
- `dac2405` — feat(24-02): add cleanup_rx select branch to SpreadEngine, CrossAssetEngine, PricingEngine
- `5f6ac9d` — feat(24-02): wire cleanup channels to DeribitProcessor, KalshiProcessor, pipeline, and main

---

### Gaps Summary

No gaps. All phase must-haves are verified at all three levels (exists, substantive, wired).

The phase goal is fully achieved:
- **Observable**: 3 Prometheus gauges + 6 Prometheus counters per-venue active in `reconcile()` after state update
- **Safe to operate**: `dry_run = true` in `[subscription]` config skips all side effects while keeping internal state current for meaningful subsequent diffs
- **No stale state**: All 5 stateful engines (SpreadEngine, CrossAssetEngine, PricingEngine, DeribitProcessor, KalshiProcessor) evict stale entries via event-driven mpsc cleanup channels on unsubscribe

---

_Verified: 2026-02-27T22:30:00Z_
_Verifier: Claude (gsd-verifier)_
