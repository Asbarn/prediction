---
phase: 12-kalshi-feed-hardening
verified: 2026-02-24T17:00:00Z
status: passed
score: 4/4 must-haves verified
re_verification: false
---

# Phase 12: Kalshi Feed Hardening Verification Report

**Phase Goal:** Add heartbeat/dead-connection detection to the Kalshi supervisor and handle the Kalshi protocol limitation of missing exchange timestamps with best-effort estimation.
**Verified:** 2026-02-24
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Kalshi supervisor detects dead connections within 30s of last message/ping and reconnects | VERIFIED | `client.rs:142-169`: `timeout_duration = Duration::from_millis(heartbeat_timeout_ms)`, `last_message_at` updated on Text/Ping/Pong/Binary/Frame, `sleep_until(timeout_deadline)` branch breaks the loop triggering supervisor reconnect; counter `feed_heartbeat_timeouts` emitted |
| 2 | Kalshi orderbook_delta messages with ts field produce MarketSnapshot with exchange_timestamp set | VERIFIED | `normalize.rs:235-287`: `last_exchange_ts` HashMap tracks per-market ts from delta, `chrono::DateTime::parse_from_rfc3339` converts to millis, `exchange_timestamp: exchange_ts_ms` on snapshot; confirmed by `processor_propagates_exchange_timestamp` test asserting `Some(1705314600000)` |
| 3 | Kalshi feed emits feed_latency_ms and feed_last_latency_ms metrics when exchange_timestamp is available | VERIFIED | `normalize.rs:256-261`: `metrics::histogram!("feed_latency_ms", "venue" => "kalshi")` and `metrics::gauge!("feed_last_latency_ms", "venue" => "kalshi")` inside `if let Some(exchange_ts) = exchange_ts_ms` guard |
| 4 | Existing Kalshi message parsing (flat format) continues to work for backward compatibility with recordings | VERIFIED | `messages.rs:99-106`: flat-vs-nested detection via `value.get("msg").filter(|v| v.is_object())`; `parse_flat_delta_still_works` and `parse_delta_ts_field_optional` tests confirm flat format still parses correctly with `ts: None` |

**Score:** 4/4 truths verified

---

## Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/config/venues.rs` | `heartbeat_timeout_ms` field on KalshiConfig | VERIFIED | Line 162: `pub heartbeat_timeout_ms: u64` with `#[serde(default = "default_kalshi_heartbeat_timeout")]`; line 169-171: `fn default_kalshi_heartbeat_timeout() -> u64 { 30_000 }` |
| `src/feed/kalshi/client.rs` | Dead connection timeout in WS loop using tokio::time::sleep_until | VERIFIED | Lines 138-169: `heartbeat_timeout_ms` read from config, `last_message_at = Instant::now()` initialized, `sleep_until(timeout_deadline)` in biased select; `last_message_at` updated on all live message arms |
| `src/feed/kalshi/messages.rs` | ts field on OrderbookDeltaData and nested msg wrapper support | VERIFIED | Line 63: `pub ts: Option<String>` with doc comment; lines 99-106: nested format detection in `parse()`; 4 new tests covering nested delta with ts, nested snapshot, flat compat, ts optional |
| `src/feed/kalshi/normalize.rs` | Exchange timestamp propagation and latency metrics emission | VERIFIED | Lines 48/70: `last_exchange_ts: HashMap<String, String>` field and init; lines 138-141: ts captured from delta; lines 235-261: chrono parse + metrics emission; line 287: `exchange_timestamp: exchange_ts_ms` |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/feed/kalshi/client.rs` | `src/config/venues.rs` | `KalshiConfig.heartbeat_timeout_ms` read for timeout duration | WIRED | `client.rs:138`: `let heartbeat_timeout_ms = self.config.heartbeat_timeout_ms;` then `Duration::from_millis(heartbeat_timeout_ms)` at line 142 |
| `src/feed/kalshi/messages.rs` | `src/feed/kalshi/normalize.rs` | `OrderbookDeltaData.ts` field read during snapshot production | WIRED | `normalize.rs:138`: `if let Some(ref ts) = data.ts` captures ts from delta and inserts into `last_exchange_ts`; ts flows through HashMap into `produce_snapshot` |
| `src/feed/kalshi/normalize.rs` | `MarketSnapshot.exchange_timestamp` | Parsed ts millis set on snapshot | WIRED | `normalize.rs:287`: `exchange_timestamp: exchange_ts_ms` — variable is `Option<i64>` derived from `last_exchange_ts.get(market_ticker)` parsed via `chrono::DateTime::parse_from_rfc3339` |

---

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|---------|
| RELY-02 | 12-01-PLAN.md | Detect stale connections via per-venue heartbeat monitoring (distinguish "quiet market" from "dead connection") | SATISFIED | 3-branch biased select in `client.rs` with `sleep_until` timeout at 30s (3x Kalshi's 10s Ping); `feed_heartbeat_timeouts` counter emitted on trigger; Ping/Pong messages update `last_message_at` so quiet-but-live connections are not falsely disconnected |
| FEED-08 | 12-01-PLAN.md | System logs exchange-reported timestamps alongside local receipt timestamps for each message | SATISFIED | `MarketSnapshot.exchange_timestamp` set from Kalshi `ts` field (ISO 8601) when available; `MarketSnapshot.timestamp` is always the local `received_at` DualTimestamp; both fields present on every snapshot produced |
| TIME-02 | 12-01-PLAN.md | All logged data includes both local receipt timestamp and exchange-reported timestamp for post-hoc latency analysis | SATISFIED | Every Kalshi `MarketSnapshot` carries `timestamp: received_at` (local) and `exchange_timestamp: exchange_ts_ms` (exchange, best-effort); protocol limitation (second-precision, deltas only) documented in code comments at normalize.rs:232-234 |
| TIME-03 | 12-01-PLAN.md | Per-feed latency characteristics documented and tracked in metrics | SATISFIED | `feed_latency_ms` histogram and `feed_last_latency_ms` gauge emitted with `"venue" => "kalshi"` label (normalize.rs:259-260); counter `feed_messages_total` always emitted; second-precision jitter limitation documented in code comment at normalize.rs:255 |

All four requirement IDs from the PLAN frontmatter are accounted for. REQUIREMENTS.md tracking table marks all four Phase 12 / Complete.

---

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/feed/kalshi/normalize.rs` | 252 | `let is_stale = false;` — staleness_threshold_ms stored but staleness not computed | Info | Pre-existing design from Phase 4 (commit `76c7ac1`); staleness gate via exchange_timestamp not implemented for Kalshi; does not block phase 12 goals (no staleness requirement in RELY-02/FEED-08/TIME-02/TIME-03); tracked separately as RELY-03 |

No blocker or warning anti-patterns in phase 12 changes.

---

## Human Verification Required

### 1. Live Kalshi connection heartbeat timeout

**Test:** Connect to live Kalshi WS API, block inbound traffic for 31+ seconds (firewall rule or network namespace), observe application logs.
**Expected:** Warning log "Kalshi heartbeat timeout -- no messages/pings received, connection assumed dead" appears within 30-35s; Kalshi supervisor reconnects; `feed_heartbeat_timeouts{venue="kalshi"}` counter increments by 1.
**Why human:** Dead-connection timeout requires a live TCP connection and controlled network failure; cannot be simulated in unit tests without a mock WS server.

### 2. Live Kalshi ts field precision verification

**Test:** Subscribe to a Kalshi market in production, observe `exchange_timestamp` values in logged MarketSnapshot data, compare to local receipt times.
**Expected:** `exchange_timestamp` values are second-aligned (always divisible by 1000ms) confirming second-precision; `feed_latency_ms{venue="kalshi"}` histogram shows up to 999ms of inherent jitter due to second-precision truncation.
**Why human:** Requires live Kalshi API access and real message observation; the `ts` field format (second vs sub-second precision) is confirmed by documentation and Go SDK but not directly testable from recordings.

---

## Gaps Summary

No gaps. All four observable truths verified. All four artifacts pass levels 1-3 (exists, substantive, wired). All three key links confirmed wired in the actual code. All four requirement IDs are satisfied with direct implementation evidence. Test suite: 38 Kalshi module tests pass, 0 failures. Commits `87ebaf6` (Task 1) and `8da73a6` (Task 2) exist and match SUMMARY.md. The only notable finding is the pre-existing `is_stale = false` hardcode, which predates this phase and is not within scope of the phase 12 requirements.

---

_Verified: 2026-02-24_
_Verifier: Claude (gsd-verifier)_
