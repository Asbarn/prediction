---
phase: 09-replay-and-hardening
verified: 2026-02-23T23:15:00Z
status: passed
score: 9/9 must-haves verified
re_verification: true
  previous_status: gaps_found
  previous_score: 8/9
  gaps_closed:
    - "HTTP GET /health returns JSON with per-feed connection status, last update time, active event count, and uptime"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Start with --mock or --replay and curl http://localhost:9001/health"
    expected: "JSON response with status, uptime_secs, feeds array, and active_event_count returned in under 100ms"
    why_human: "Cannot verify HTTP bind and serve over network in static analysis"
  - test: "Start in live mode, verify feeds connect, then curl http://localhost:9001/health"
    expected: "connected=true for venues that are actually connected; status='ok' rather than 'degraded'"
    why_human: "Requires live WebSocket connections to confirm VenueHealth wiring updates correctly at runtime"
---

# Phase 9: Replay and Hardening Verification Report

**Phase Goal:** The system supports deterministic replay from recorded feed data, exposes a health endpoint for operational monitoring, and stabilizes the JSONL schema for offline analysis -- turning accumulated data into a validated testing and analysis corpus.

**Verified:** 2026-02-23T23:15:00Z
**Status:** passed
**Re-verification:** Yes -- after gap closure plan 09-03 (VenueHealth wiring)

---

## Re-verification Summary

Previous verification (2026-02-23T20:30:00Z) found one gap: Truth #1 (OBSV-05) was PARTIAL because `VenueHealth` instances created in `run_live_multi_venue()` were never passed to the supervisors. Supervisors had no reference to call `mark_available()` or `record_message()`, causing the `/health` endpoint to always report `connected=false` and `last_message_at=null` in live mode.

Gap closure plan 09-03 (commit `6746f6f`) wired `Arc<VenueHealth>` to all three supervisors and called `record_message()` in `forward_snapshots`. This re-verification confirms the gap is closed.

---

## Goal Achievement

### Observable Truths

| #  | Truth                                                                                                                      | Status      | Evidence                                                                                                        |
|----|---------------------------------------------------------------------------------------------------------------------------|-------------|----------------------------------------------------------------------------------------------------------------|
| 1  | HTTP GET /health returns JSON with per-feed connection status, last update time, active event count, and uptime           | VERIFIED    | VenueHealth now wired to all 3 supervisors. Each calls increment_connections() on attempt, mark_available() on first message, mark_unavailable() on disconnect/error. forward_snapshots calls record_message() per forwarded snapshot. See verification detail below. |
| 2  | Health endpoint runs on a separate configurable port (default 9001) without conflicting with Prometheus (port 9000)       | VERIFIED    | HealthConfig.default() = port 9001; Prometheus default = 9000; tokio::spawn(start_health_server) in main.rs -- no regression   |
| 3  | All four JSONL types (RecordLine, SpreadResult, ArbSignal, TradeEvent) have serde roundtrip golden tests that fail on schema changes | VERIFIED | tests/schema_golden_test.rs: 11 tests, all passing on re-run; covers RecordLine (2), SpreadResult (3), ArbSignal (2), TradeEvent (4) -- no regression |
| 4  | SpreadResult and TradeEvent gain Deserialize derives for offline analysis tooling                                          | VERIFIED    | SpreadResult: `#[derive(Debug, Clone, Serialize, Deserialize)]`; TradeEvent: `#[derive(Debug, serde::Serialize, serde::Deserialize)]` -- no regression |
| 5  | JSONL schema documentation exists as a schema spec                                                                        | VERIFIED    | `## JSONL Schema (v1.0)` doc comments on RecordLine, SpreadResult, ArbSignal, TradeEvent -- no regression      |
| 6  | Recorded JSONL feeds from multiple venues can be replayed through the full pipeline producing spread and signal computations | VERIFIED  | 5 pipeline tests pass including multi_venue_replay_pipeline_processes_deribit_recordings -- no regression      |
| 7  | Replay mode bypasses staleness gates so historical data is not rejected as stale                                          | VERIFIED    | SpreadEngine.replay_mode + CrossAssetEngine.replay_mode; bypass logic unchanged -- no regression               |
| 8  | Missing venue recordings (e.g., no Kalshi directory) degrade gracefully with a warning, not a crash                      | VERIFIED    | multi_venue_replay_graceful_empty_dir test passes -- no regression                                             |
| 9  | Replay CLI supports a directory path pointing to the recordings/ directory (not a single file)                           | VERIFIED    | DataMode::Replay { path, speed } unchanged -- no regression                                                    |

**Score: 9/9 truths verified**

---

## Gap Closure: Truth #1 (OBSV-05) -- Detail

### What was wrong (previous verification)

`run_live_multi_venue()` created `VenueHealth::new(Venue::Deribit)` etc. and pushed the Arc into `venue_health_handles`, but never passed the Arc to the supervisors. Supervisors had no `health` field and could not call lifecycle methods. The health endpoint always reported `connected=false` and `last_message_at=null`.

### What plan 09-03 fixed (commit 6746f6f)

**DeribitSupervisor** (`src/feed/deribit/supervisor.rs`):
- Added `health: Arc<VenueHealth>` field and `use crate::feed::health::VenueHealth` import
- `new()` accepts `health: Arc<VenueHealth>` as final parameter
- Line 84: `self.health.increment_connections()` on each connection attempt
- Line 121: `self.health.mark_available()` on first message received (after backoff.reset())
- Line 135: `self.health.mark_unavailable("connection lost".to_string())` on channel close
- Line 148: `self.health.mark_unavailable(format!("connection failed: {e}"))` on connect error

**PolymarketSupervisor** (`src/feed/polymarket/supervisor.rs`):
- Same pattern: `health: Arc<VenueHealth>` field; `new()` accepts health
- Line 54: `self.health.increment_connections()`
- Line 82: `self.health.mark_available()`
- Line 95: `self.health.mark_unavailable("connection lost".to_string())`
- Line 108: `self.health.mark_unavailable(format!("connection failed: {e}"))`

**KalshiSupervisor** (`src/feed/kalshi/supervisor.rs`):
- Same pattern: `health: Arc<VenueHealth>` field; `new()` accepts health
- Line 70: `self.health.increment_connections()`
- Line 104: `self.health.mark_available()`
- Line 117: `self.health.mark_unavailable("connection lost".to_string())`
- Line 130: `self.health.mark_unavailable(format!("connection failed: {e}"))`

**forward_snapshots** (`src/feed/pipeline.rs`):
- Signature updated: `health: Option<Arc<VenueHealth>>` parameter added
- Line 331-333: `if let Some(h) = &health { h.record_message(); }` before each forwarded snapshot
- Called with `Some(health.clone())` for all three live venues (lines 155-161, 195-201, 247-253)

**Pipeline wiring** (`src/feed/pipeline.rs`):
- Deribit: `DeribitSupervisor::new(..., health.clone())` at line 136-142
- Polymarket: `PolymarketSupervisor::new(..., health.clone())` at line 179-183
- Kalshi: `KalshiSupervisor::new(..., health.clone())` at line 229-235

**Replay compatibility** (`src/replay/mod.rs`):
- `forward_snapshots(...)` called with `None` health (line 230) -- replay has no live connection to track

**Single-venue run_pipeline** (`src/feed/pipeline.rs`):
- Live path creates ephemeral `VenueHealth::new(Venue::Deribit)` (line 382) and passes to `DeribitSupervisor::new(..., health)` (line 384-390) -- not surfaced to caller since this path is legacy/single-venue

---

## Required Artifacts

### Plan 09-01 Artifacts (Regression Check)

| Artifact                        | Status      | Details                                                                      |
|---------------------------------|-------------|------------------------------------------------------------------------------|
| `src/health/mod.rs`             | VERIFIED    | Unchanged; 177 lines; all types present                                      |
| `src/config/system.rs`          | VERIFIED    | Unchanged; HealthConfig { port: u16, enabled: bool }                        |
| `src/events/registry.rs`        | VERIFIED    | Unchanged; event_count() method present                                      |
| `tests/schema_golden_test.rs`   | VERIFIED    | 11/11 tests pass on re-run                                                   |

### Plan 09-02 Artifacts (Regression Check)

| Artifact                    | Status      | Details                                                                          |
|-----------------------------|-------------|----------------------------------------------------------------------------------|
| `src/replay/mod.rs`         | VERIFIED    | Updated to pass None health to forward_snapshots; all replay logic intact       |
| `src/feed/mock/replay.rs`   | VERIFIED    | Unchanged                                                                        |
| `src/spread/engine.rs`      | VERIFIED    | Unchanged; replay_mode bypass intact                                             |
| `src/signal/engine.rs`      | VERIFIED    | Unchanged; replay_mode bypass intact                                             |

### Plan 09-03 Artifacts (New -- Gap Closure)

| Artifact                           | Expected                                              | Status      | Details                                                  |
|------------------------------------|-------------------------------------------------------|-------------|----------------------------------------------------------|
| `src/feed/deribit/supervisor.rs`   | VenueHealth lifecycle calls in DeribitSupervisor      | VERIFIED    | health field; increment_connections, mark_available, mark_unavailable all present |
| `src/feed/polymarket/supervisor.rs` | VenueHealth lifecycle calls in PolymarketSupervisor  | VERIFIED    | health field; increment_connections, mark_available, mark_unavailable all present |
| `src/feed/kalshi/supervisor.rs`    | VenueHealth lifecycle calls in KalshiSupervisor       | VERIFIED    | health field; increment_connections, mark_available, mark_unavailable all present |
| `src/feed/pipeline.rs`             | VenueHealth passed to supervisors and forward_snapshots | VERIFIED  | health.clone() to all 3 supervisors; Some(health.clone()) to all 3 forward_snapshots calls; record_message() in forwarding body |

---

## Key Link Verification

### Plan 09-01 Key Links (Regression Check)

| From                    | To                        | Via                                          | Status   | Details                                                                       |
|-------------------------|---------------------------|----------------------------------------------|----------|-------------------------------------------------------------------------------|
| `src/health/mod.rs`     | `src/feed/health.rs`      | Arc<VenueHealth> in HealthState              | WIRED    | Unchanged                                                                     |
| `src/health/mod.rs`     | `src/events/registry.rs`  | event_count() via Arc<RwLock<EventRegistry>> | WIRED    | Unchanged                                                                     |
| `src/main.rs`           | `src/health/mod.rs`       | tokio::spawn(start_health_server)            | WIRED    | Unchanged                                                                     |
| `src/feed/pipeline.rs`  | `src/main.rs`             | PipelineHandles returning VenueHealth        | WIRED    | Unchanged                                                                     |

### Plan 09-03 Key Links (Gap Closure -- Primary Focus)

| From                        | To                                  | Via                                         | Status   | Details                                                                      |
|-----------------------------|-------------------------------------|---------------------------------------------|----------|------------------------------------------------------------------------------|
| `src/feed/pipeline.rs`      | `src/feed/deribit/supervisor.rs`    | Arc<VenueHealth> constructor parameter      | WIRED    | pipeline.rs line 136-142: DeribitSupervisor::new(..., health.clone())        |
| `src/feed/pipeline.rs`      | `src/feed/polymarket/supervisor.rs` | Arc<VenueHealth> constructor parameter      | WIRED    | pipeline.rs line 179-183: PolymarketSupervisor::new(..., health.clone())     |
| `src/feed/pipeline.rs`      | `src/feed/kalshi/supervisor.rs`     | Arc<VenueHealth> constructor parameter      | WIRED    | pipeline.rs line 229-235: KalshiSupervisor::new(..., health.clone())         |
| `src/feed/pipeline.rs`      | `src/feed/health.rs`                | forward_snapshots calls health.record_message() | WIRED | pipeline.rs line 317: Option<Arc<VenueHealth>> param; line 332: h.record_message() |

---

## Requirements Coverage

| Requirement | Source Plan | Description                                                                                       | Status      | Evidence                                                                              |
|-------------|-------------|---------------------------------------------------------------------------------------------------|-------------|---------------------------------------------------------------------------------------|
| OBSV-05     | 09-01, 09-03 | HTTP /health endpoint: per-feed connection status, last update time, active event count, uptime  | VERIFIED    | Endpoint delivers correct JSON structure. VenueHealth now wired to all 3 supervisors: mark_available() on connect, mark_unavailable() on disconnect/error, increment_connections() on each attempt, record_message() per forwarded snapshot. Per-feed status will accurately reflect live connection state. |
| OBSV-06     | 09-01       | JSONL schema stable and documented for offline analysis tooling                                    | VERIFIED    | 11/11 golden tests pass; Deserialize on SpreadResult + TradeEvent; Schema v1.0 doc comments; no regression |
| TEST-02     | 09-02       | Deterministic replay from recorded JSONL feeds through the full pipeline                           | VERIFIED    | 5/5 pipeline tests pass; replay None-health path confirmed in replay/mod.rs           |
| TEST-03     | 09-02       | Feed recordings serve as replay corpus for backtesting and debugging                               | VERIFIED    | ReplayCorpus.load_directory() unchanged; no regression                                |

No orphaned requirements. All four IDs (OBSV-05, OBSV-06, TEST-02, TEST-03) claimed by plans and mapped to Phase 9 in REQUIREMENTS.md.

---

## Anti-Patterns Found

| File                         | Issue                                                                                        | Severity | Impact                                                                           |
|------------------------------|----------------------------------------------------------------------------------------------|----------|----------------------------------------------------------------------------------|
| `src/replay/mod.rs` line 221 | `drop(processor_task)` -- processor JoinHandle silently dropped without monitoring           | Info     | Pre-existing; not introduced by 09-03; processor panics in replay go unnoticed; not a goal blocker |

No new anti-patterns introduced by plan 09-03. No TODO/FIXME/placeholder comments in phase-9 files.

---

## Build and Test Confirmation

- `cargo build`: Finished with 0 errors, 2 pre-existing warnings (unrelated: unused `time_to_expiry` field in options engine)
- `cargo test --test schema_golden_test`: 11/11 passed
- `cargo test --test pipeline_test`: 5/5 passed (including multi-venue replay and graceful empty dir tests)
- Commit `6746f6f`: `feat(09-03): wire VenueHealth to supervisors and forward_snapshots` -- verified in git log

---

## Human Verification Required

### 1. Health Endpoint HTTP Response

**Test:** Start the system with `cargo run -- --mock`, then `curl http://localhost:9001/health`
**Expected:** JSON response with `status`, `uptime_secs`, `feeds` array, `active_event_count` fields returned in under 100ms.
**Why human:** Cannot verify HTTP bind + axum serve over network in static analysis.

### 2. Live Mode VenueHealth Accuracy

**Test:** Start with live WebSocket connections: `cargo run -- 2>&1`, wait for "Deribit pipeline started", then `curl http://localhost:9001/health | jq .feeds`
**Expected:** `connected: true` for Deribit (and other configured venues); `last_message_at` populated with a recent timestamp; `status: "ok"` rather than `"degraded"`. This confirms the gap closure works at runtime.
**Why human:** Requires live WebSocket connections and network access.

---

## Gaps Summary

No gaps remaining. The single gap from the initial verification (VenueHealth not wired to supervisors) has been closed by plan 09-03.

All 9 must-haves are fully verified:
- OBSV-05: Health endpoint now reports accurate per-feed connection state in live mode
- OBSV-06: JSONL schema stable with Deserialize derives and 11 golden tests
- TEST-02: Multi-venue deterministic replay through full pipeline with staleness bypass
- TEST-03: Recordings directory model with graceful degradation

Build is clean, 5+ pipeline tests pass, 11 schema golden tests pass. Phase 9 goal is achieved.

---

_Verified: 2026-02-23T23:15:00Z_
_Verifier: Claude (gsd-verifier)_
_Re-verification after gap closure plan 09-03_
