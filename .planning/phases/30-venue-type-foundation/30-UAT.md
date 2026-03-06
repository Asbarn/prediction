---
status: complete
phase: v1.5-derive-integration (phases 30-33)
source: 30-01-SUMMARY.md, 30-02-SUMMARY.md, 31-01-SUMMARY.md, 31-02-SUMMARY.md, 31-03-SUMMARY.md, 31-04-SUMMARY.md, 32-01-SUMMARY.md, 32-02-SUMMARY.md, 33-01-SUMMARY.md, 33-02-SUMMARY.md
started: 2026-03-06T00:00:00Z
updated: 2026-03-06T00:10:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Build and Tests Pass
expected: `cargo check` compiles with zero errors. `cargo test --lib` passes all tests (600+). No todo!() or unreachable!() placeholders related to Derive.
result: pass

### 2. Derive Config in venues.toml
expected: `config/venues.toml` has a `[derive]` section with `ws_url = "wss://api.lyra.finance/ws"`, rate_limit, book_depth, staleness, and reconnect settings. The config deserializes without error.
result: pass

### 3. Binary Starts with Derive Venue Logged
expected: Running the binary (or checking main.rs) shows Derive logged as "available (public, no auth)" in the venue availability output at startup. All 4 venues (Deribit, Polymarket, Kalshi, Derive) are listed.
result: pass
verified: grep confirms `derive = "available (public, no auth)"` at main.rs:153

### 4. Derive Pipeline Wired in run_live_multi_venue
expected: `src/feed/pipeline.rs` contains a Derive pipeline block that spawns DeriveSupervisor, DeriveProcessor, RecordingService, and forward_snapshots. The block appears BEFORE `drop(snapshot_tx)`. Derive snapshots flow to the shared fan-in channel.
result: pass
verified: DeriveSupervisor::new at line 398, drop(snapshot_tx) at line 430

### 5. SubscriptionManager 4-Venue Support
expected: `src/subscription/manager.rs` has `current_derive: HashSet<String>`, a Derive diff/reconcile block, watch channel push, and metrics with `venue = "derive"`. `CleanupEvent.derive_instruments` is populated from actual diff (not Vec::new()).
result: pass
verified: current_derive at line 86, derive_instruments from removed_dr.into_iter().collect() at line 343

### 6. Prometheus Metrics for Derive
expected: The codebase emits these Derive-tagged metrics: `feed_available`, `feed_latency_ms`, `feed_messages_total`, `subscription_active`, `subscription_activations_total`, `subscription_removals_total`, and `feed_reconnections_total`. The reconnection counter is in VenueHealth and benefits all venues.
result: pass
verified: feed_latency_ms and feed_messages_total in normalize.rs, subscription metrics in manager.rs, feed_reconnections_total in health.rs:78

### 7. Derive Instrument Discovery REST Call
expected: `src/events/discovery.rs` has a `discover_derive()` function that POSTs to Derive's `/public/get_instruments` endpoint, parses BTC options into `Vec<DiscoveredInstrument>` with correct strike (Decimal), expiry (NaiveDate from epoch), and direction (Call/Put).
result: pass
verified: discover_derive at line 672, POST to /public/get_instruments at line 681

### 8. Cross-Venue Matching Includes Derive
expected: Both `filter_new_candidates()` and `filter_new_candidates_fuzzy()` in discovery.rs have active `Venue::Derive` match arms (no empty `{}` stubs). Matched candidates populate `CandidateVenues.derive` for TOML writing.
result: pass
verified: Active arms at lines 823 and 947 with `derive = Some(inst.instrument_id.clone())`

### 9. Derive Discovery in Lifecycle Manager
expected: `src/events/lifecycle.rs` calls `discover_derive()` on a configurable interval (`derive_poll_interval_secs` defaults to 300). Approved mappings with Derive instruments are absence-checked. Discovery participates in `min_poll_interval_secs` calculation.
result: pass
verified: discover_derive import at line 25, polling block at lines 445-462, derive_poll_interval_secs default 300 in events.rs:272, chained in min_poll_interval_secs at line 262

### 10. Integration Test Compiles
expected: `cargo test --test integration` and `cargo test --test smoke_test` both compile and pass. Test fixtures include `[derive]` config sections.
result: pass
verified: [derive] sections in integration.rs:210, smoke_test.rs:283, smoke_test.rs:328

## Summary

total: 10
passed: 10
issues: 0
pending: 0
skipped: 0

## Gaps

[none]
