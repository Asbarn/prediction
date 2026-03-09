---
phase: 42-rest-polling-fallback-source-coordination
verified: 2026-03-09T13:35:00Z
status: passed
score: 7/7 must-haves verified
---

# Phase 42: REST Polling Fallback & Source Coordination Verification Report

**Phase Goal:** Polymarket price data is available via REST polling when WebSocket is unreliable, with clean exclusive-mode switching between sources
**Verified:** 2026-03-09T13:35:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | REST poller fetches Polymarket midpoint prices via /midpoint endpoint and produces MarketSnapshot values | VERIFIED | `rest_poller.rs` L69-87: `fetch_midpoint` calls `/midpoint?token_id=`, parses response, returns Decimal. L149-177: constructs full MarketSnapshot with bid=ask=midpoint, probabilities, sequence, trace_id. 219 lines total. |
| 2 | REST polling interval and config fields are loaded from TOML with sensible defaults | VERIFIED | `venues.rs` L164-172: three fields (`rest_poll_interval_secs`, `ws_recovery_check_secs`, `ws_recovery_threshold`) with serde defaults (5, 60, 3). `venues.toml` L25-28: documented as commented options. |
| 3 | Rate limiting via existing VenueRateLimiter prevents exceeding Polymarket API limits | VERIFIED | `rest_poller.rs` L70: `self.rate_limiter.wait().await` called before every HTTP request in `fetch_midpoint`. |
| 4 | System runs exactly one data source at a time (WS or REST), never both simultaneously | VERIFIED | `coordinator.rs` L8-9: design invariant comment. L131: child_cancel created per WS session. L212: `child_cancel.cancel()` before switching. L290-291: `rest_cancel.cancel()` before switching back. State machine loop ensures only one arm runs at a time. |
| 5 | Coordinator switches to REST when WS data timeout fires or connection fails | VERIFIED | `coordinator.rs` L177-181: when processor channel closes and health unavailable, switches to REST. L194-205: periodic health check detects unavailability, waits 5s grace, then switches. Both paths emit `Some(SourceMode::Rest)`. |
| 6 | Coordinator probes WS recovery periodically and switches back after sustained messages | VERIFIED | `coordinator.rs` L257-259: recovery interval from config. L284: calls `probe_ws_recovery`. L328-366: probe creates temporary PolymarketClient on separate channel, counts messages via `count_messages` (L369-417), returns true only when threshold met. On success, cancels REST first (L291), switches to WS. |
| 7 | Prometheus gauge shows current data source mode (WS=0, REST=1) | VERIFIED | `coordinator.rs` L84-88: initial gauge set to 0. L216-220: set to 1.0 on WS->REST switch. L294-298: set to 0.0 on REST->WS switch. Counter `feed_source_switches_total` with from/to labels at L222-228 and L300-306. |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/feed/polymarket/rest_poller.rs` | PolymarketRestPoller with run() producing MarketSnapshot (min 80 lines) | VERIFIED | 219 lines. Complete struct with `new`, `fetch_midpoint`, `run`. Full MarketSnapshot construction, metrics, health marking, cancellation. |
| `src/config/venues.rs` | REST polling config fields containing `rest_poll_interval_secs` | VERIFIED | L164-172: three fields with serde defaults. Default functions at L83-93. |
| `src/feed/polymarket/coordinator.rs` | SourceCoordinator state machine with exclusive WS/REST switching (min 120 lines) | VERIFIED | 419 lines. SourceMode enum, SourceCoordinator struct, `run` with state machine loop, `run_ws_mode`, `run_rest_mode`, `probe_ws_recovery`, `count_messages`. |
| `src/feed/pipeline.rs` | Pipeline spawns SourceCoordinator instead of direct PolymarketSupervisor | VERIFIED | L38: imports `SourceCoordinator`. L255-264: creates and spawns coordinator. L268-275: forwards to fan-in via `forward_snapshots`. No direct `PolymarketSupervisor` reference in pipeline. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| rest_poller.rs | venues.rs | PolymarketConfig fields | WIRED | L100: `self.config.rest_poll_interval_secs` used for poll interval |
| rest_poller.rs | rate_limiter.rs | VenueRateLimiter::wait() | WIRED | L70: `self.rate_limiter.wait().await` |
| coordinator.rs | supervisor.rs | Spawns PolymarketSupervisor in WS mode | WIRED | L137-143: creates and spawns supervisor |
| coordinator.rs | rest_poller.rs | Spawns PolymarketRestPoller in REST mode | WIRED | L247-254: creates and spawns poller |
| pipeline.rs | coordinator.rs | Pipeline spawns coordinator | WIRED | L255: `SourceCoordinator::new(...)`, L264: `tokio::spawn(coordinator.run(...))` |
| coordinator.rs | pipeline.rs | Sends MarketSnapshot to fan-in channel | WIRED | L80: `run(self, snapshot_tx: mpsc::Sender<MarketSnapshot>)`. Pipeline L254: creates channel, L264: passes tx to coordinator, L268: forwards rx to fan-in |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-----------|-------------|--------|----------|
| POLY-04 | 42-01, 42-02 | REST polling fallback fetches Polymarket prices when WebSocket is unavailable | SATISFIED | rest_poller.rs fetches /midpoint, produces MarketSnapshot. Coordinator activates REST mode when WS fails. |
| POLY-05 | 42-02 | Source coordinator switches between WebSocket and REST modes exclusively | SATISFIED | coordinator.rs implements exclusive-mode state machine with cancel-before-switch invariant, probe-based recovery. |

No orphaned requirements found. REQUIREMENTS.md maps POLY-04 and POLY-05 to Phase 42, both covered by plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No TODO/FIXME/placeholder/stub patterns found in any phase artifact |

No `/book` endpoint usage in rest_poller.rs (only `/midpoint` per design constraint). No empty implementations, no console.log stubs, no return-null patterns.

### Human Verification Required

### 1. REST Polling Produces Valid Snapshots

**Test:** Deploy to EC2, temporarily block WS (e.g., firewall rule), observe REST polling in logs and Grafana
**Expected:** `feed_source_mode{venue=polymarket}` gauge switches to 1, `feed_rest_polls_total` counter increments, MarketSnapshot values appear downstream
**Why human:** Requires live Polymarket API access and network manipulation

### 2. WS Recovery Probe and Switch-Back

**Test:** After REST mode is active, unblock WS, wait for `ws_recovery_check_secs` (default 60s)
**Expected:** Log "WS probe successful, switching back to WebSocket", gauge returns to 0, no duplicate snapshots during transition
**Why human:** Requires real WS connection recovery and timing verification

### 3. Exclusive Mode Guarantee Under Load

**Test:** Monitor snapshot stream during WS->REST and REST->WS transitions with active subscriptions
**Expected:** No overlapping snapshots from both sources, no gap longer than poll interval + switch time
**Why human:** Race condition detection requires real concurrent execution

### Gaps Summary

No gaps found. All 7 observable truths verified. All artifacts exist, are substantive (well above minimum line counts), and are fully wired. All key links confirmed. Both requirements (POLY-04, POLY-05) satisfied. No anti-patterns detected. Three commits verified in git history.

---

_Verified: 2026-03-09T13:35:00Z_
_Verifier: Claude (gsd-verifier)_
