# Phase 13: Phase 4 Verification & Cleanup - Research

**Researched:** 2026-02-24
**Domain:** Phase verification (goal-backward), dead code cleanup, test infrastructure
**Confidence:** HIGH

## Summary

Phase 13 is a verification and cleanup phase, not a feature-build phase. Its two tasks are: (1) perform formal goal-backward verification of Phase 4 (Multi-Venue Feeds) against requirements FEED-03, FEED-04, FEED-05, and RELY-04 -- producing the missing `04-VERIFICATION.md` that the v1.0 audit flagged as a placeholder; and (2) resolve the `NormalizedDataSource` trait dead code in `src/feed/traits.rs` to satisfy TEST-01.

The codebase is in excellent shape for both tasks. Phase 4 code is complete and integrated -- Polymarket and Kalshi clients, processors, supervisors, and multi-venue pipeline are all wired and tested (360+ tests passing). The `NormalizedDataSource` trait at `src/feed/traits.rs:40-45` has zero implementations across the entire codebase. It was defined speculatively in Phase 2 but the actual mock/test path uses `RawDataSource` (via `SyntheticDataSource`) and `ReplayDataSource`, making `NormalizedDataSource` dead code.

**Primary recommendation:** Create two plans: Plan 1 performs the formal Phase 4 verification (static code analysis of existing artifacts against requirements), and Plan 2 removes the `NormalizedDataSource` trait as dead code since the mock layer already functions correctly through `RawDataSource`, satisfying TEST-01 through the existing abstraction.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FEED-03 | System connects to Polymarket CLOB WebSocket and subscribes to order book updates for target condition IDs | Polymarket client at `src/feed/polymarket/client.rs` connects to CLOB market channel, subscribes with token IDs. PolymarketSupervisor provides reconnection. 5 Polymarket source files verified present. 13+ unit tests in messages.rs and normalize.rs. |
| FEED-04 | System normalizes Polymarket order books from probability space (0-1) with bid/ask/depth | PolymarketProcessor at `normalize.rs` converts book events to MarketSnapshot. Prices ARE probabilities (no conversion needed). bid_probability/ask_probability populated directly from price strings. Staleness gate and exchange timestamp handling present. |
| FEED-05 | System connects to Kalshi feed and normalizes contracts into probability + expiry schema | KalshiClient at `src/feed/kalshi/client.rs` connects with RSA-PSS auth headers. KalshiProcessor converts cents to probability via `Decimal::new(cents, 2)`. BTreeMap-based incremental order book. Derived asks from complementary side. 7 Kalshi source files verified. 28+ unit tests. Phase 12 added heartbeat timeout and exchange timestamp propagation. |
| RELY-04 | Feed drops degrade gracefully -- remaining feeds continue operating, affected instruments marked unavailable, degraded state surfaced in metrics | `run_live_multi_venue()` at `pipeline.rs:107` spawns independent `CancellationToken` per venue. Missing Kalshi credentials produce warning and skip (lines 270-278). `VenueHealth` tracker at `health.rs` with `mark_available()`/`mark_unavailable()` + metrics gauges. 8 VenueHealth unit tests. |
| TEST-01 | Mock data layer via trait-based abstraction over data sources -- full pipeline runnable without live venue connections | `RawDataSource` trait at `traits.rs:24-33` with two implementations: `SyntheticDataSource` (mock) and `ReplayDataSource` (replay). Pipeline runs in Mock mode via `DataMode::Mock`. `NormalizedDataSource` trait (traits.rs:40-45) is dead code with zero implementations -- needs removal or implementation. |
</phase_requirements>

## Standard Stack

This phase does not introduce new libraries. It is a verification and cleanup phase operating on existing code.

### Core (Existing)
| Library | Version | Purpose | Relevance |
|---------|---------|---------|-----------|
| tokio | 1.x | Async runtime | Polymarket/Kalshi supervisors use tokio tasks |
| tokio-tungstenite | 0.24 | WebSocket client | Both venue clients use this |
| backoff | 0.4 | Exponential backoff | Both supervisors use ExponentialBackoffBuilder |
| rsa | 0.9 | RSA-PSS signing | Kalshi authentication |
| rust_decimal | 1.x | Decimal arithmetic | Probability normalization |
| metrics | 0.24 | Metrics facade | feed_latency_ms, feed_available gauges |

### Alternatives Considered
None -- this is verification, not implementation.

## Architecture Patterns

### Pattern 1: Goal-Backward Verification
**What:** Verification starts from the requirement text, traces through the code, and confirms each aspect is satisfied with specific file:line evidence.
**When to use:** When a phase has completed implementation but lacks formal verification.
**Example:** See `03-VERIFICATION.md` (Phase 3) for the canonical pattern used in this project. Key structure:
- Observable Truths table (derived from phase success criteria)
- Required Artifacts table (files and what they must contain)
- Key Link Verification table (from -> to -> via)
- Requirements Coverage table
- Anti-Patterns Found section
- Human Verification Required section

### Pattern 2: Dead Code Removal
**What:** Remove unused trait/type definitions that were speculatively created but never implemented.
**When to use:** When audit identifies dead code that provides no value and creates false expectations.
**Evidence for removal:** `NormalizedDataSource` has zero implementations (confirmed by grep across entire codebase). The trait was defined in Phase 2's 02-01-PLAN.md as a "snapshot-level abstraction for testing downstream consumers in isolation" but the actual test path uses `RawDataSource` (SyntheticDataSource produces raw Deribit-format messages that flow through DeribitProcessor).

### Recommended Verification Structure
```
.planning/phases/04-multi-venue-feeds/
  04-VERIFICATION.md    # REPLACE placeholder with formal verification
```

The verification must cover ALL Phase 4 plans (01, 02, 03) and their success criteria, plus the cross-cutting requirements FEED-03, FEED-04, FEED-05, RELY-04.

### Anti-Patterns to Avoid
- **Verification by assumption:** Do not assume Phase 4 code works because other phases built on top of it. Verify each requirement independently with file:line evidence.
- **Shallow verification:** Do not just check that files exist. Verify the functional behavior: connection, normalization math, degradation behavior.
- **Over-engineering TEST-01:** The existing `RawDataSource` + `SyntheticDataSource` already satisfies the "full pipeline runnable without live connections" requirement. Removing `NormalizedDataSource` dead code is the correct action -- not adding a new implementation for it.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Phase 4 VERIFICATION.md | New verification approach | Follow existing 03-VERIFICATION.md format exactly | Consistency with all other verified phases (8/9 use this format) |
| NormalizedDataSource implementation | New mock that produces MarketSnapshot directly | Remove the dead trait | SyntheticDataSource -> DeribitProcessor path already works for full pipeline mocking; adding a parallel path adds maintenance burden with no benefit |

**Key insight:** This phase is about confirming existing work and cleaning up, not building new features. The bar for changes is "minimum edits with maximum correctness."

## Common Pitfalls

### Pitfall 1: Treating Verification as Rubber-Stamping
**What goes wrong:** Verification simply restates what the plan said it would do without independently confirming the code.
**Why it happens:** Plans claim to deliver requirements, so it's tempting to just check the plan summary.
**How to avoid:** Trace each requirement to specific source code with file:line references. Read the actual implementation, don't just read summaries.
**Warning signs:** Verification evidence cites SUMMARY.md or PLAN.md rather than source files.

### Pitfall 2: Missing Cross-Phase Integration in Verification
**What goes wrong:** Phase 4 code is verified in isolation but cross-phase wiring is missed (exactly what happened in the original placeholder verification).
**Why it happens:** Phase verification focuses on the phase's own deliverables.
**How to avoid:** The v1.0 audit already confirmed integration wiring for FEED-03/04/05/RELY-04 is present. The verification should reference this, noting that Phase 10 completed the event_id annotation that Phase 4's forward_snapshots needed.
**Warning signs:** No mention of `pipeline.rs::forward_snapshots` or `main.rs` wiring in the verification.

### Pitfall 3: Removing NormalizedDataSource Without Checking All References
**What goes wrong:** Removing the trait but forgetting to update imports or doc comments that reference it.
**Why it happens:** Dead code can still be referenced in documentation or comments.
**How to avoid:** Grep for ALL occurrences of "NormalizedDataSource" in source files. Current occurrences:
- `src/feed/traits.rs:40` -- the trait definition (REMOVE)
- No other source file references it (confirmed by grep)
Planning docs reference it but those are historical records, not code.
**Warning signs:** Compilation failure after removal.

### Pitfall 4: Ambiguity on TEST-01 Satisfaction
**What goes wrong:** TEST-01 says "Mock data layer via trait-based abstraction over data sources" -- it's ambiguous whether removing `NormalizedDataSource` weakens this.
**Why it happens:** The requirement mentions "trait-based abstraction" and `NormalizedDataSource` IS a trait.
**How to avoid:** The `RawDataSource` trait IS the trait-based abstraction that enables mock/replay. `SyntheticDataSource` implements `RawDataSource` and the pipeline runs end-to-end in Mock mode without live connections. Removing an unused second trait does not weaken the abstraction -- it strengthens it by removing confusion about which trait is the real mock seam.
**Warning signs:** None if this reasoning is documented in the verification.

## Code Examples

### Current Dead Code (to be removed)
```rust
// Source: src/feed/traits.rs:35-45
/// Normalized data source.
///
/// Produces `MarketSnapshot` directly, bypassing WS parsing.
/// Used for testing downstream consumers in isolation from the
/// venue-specific parsing and normalization layers.
pub trait NormalizedDataSource: Send + 'static {
    /// Start the data source and return a receiver for normalized snapshots.
    fn start(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<mpsc::Receiver<MarketSnapshot>>> + Send;
}
```

### Working Mock Infrastructure (already satisfies TEST-01)
```rust
// Source: src/feed/mock/synthetic.rs:59-60
impl crate::feed::traits::RawDataSource for SyntheticDataSource {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        // Generates realistic Deribit-format JSON-RPC messages
        // Pipeline processes these identically to live data
    }
}
```

```rust
// Source: src/feed/pipeline.rs:95-103 (Mock mode)
DataMode::Mock => {
    let snapshot_rx =
        run_pipeline(DataMode::Mock, &config.deribit, recording_dir, cancel).await?;
    Ok(PipelineHandles {
        snapshot_rx,
        venue_health: vec![],
    })
}
```

### Verification Evidence Examples (for FEED-03)
```
Requirement: "System connects to Polymarket CLOB WebSocket and subscribes
              to order book updates for target condition IDs"

Evidence chain:
1. PolymarketClient::start() at client.rs:49 -> connect_async(ws_url)
2. Subscribe message at client.rs:73: {"assets_ids": [token_ids], "type": "market"}
3. Reader loop at client.rs:106-168 forwards text frames as RawMessage
4. PolymarketSupervisor at supervisor.rs:35 wraps with reconnection
5. pipeline.rs:181 creates PolymarketSupervisor in live multi-venue pipeline
6. Unit tests: messages.rs has parsing tests, normalize.rs has normalization tests
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Placeholder 04-VERIFICATION.md | To be replaced with formal verification | Phase 13 | Closes the only unverified phase in the project |
| Dual traits (RawDataSource + NormalizedDataSource) | Single trait (RawDataSource only) | Phase 13 | Removes dead code; mock pipeline works via RawDataSource |
| TEST-01 marked "partial" | TEST-01 satisfied after cleanup | Phase 13 | Mock pipeline was always functional; removing dead trait resolves the audit finding |

## Detailed Requirement Analysis

### FEED-03: Polymarket CLOB WebSocket Connection
**Requirement text:** "System connects to Polymarket CLOB WebSocket and subscribes to order book updates for target condition IDs"
**Code location:** `src/feed/polymarket/client.rs`
**Key evidence:**
- WebSocket connection: `connect_async(ws_url)` at line 53-59
- Subscription: `{"assets_ids": [token_ids], "type": "market"}` at lines 66-89
- Token IDs from config: `config.assets.iter().map(|a| a.token_id.as_str())` at lines 66-71
- PING heartbeat: sends WebSocket PING every `ping_interval_ms` (default 10s) at lines 116-122
- Raw frame forwarding: text frames wrapped as `RawMessage` with `DualTimestamp::now()` at lines 127-137
**Tests:** Message parsing tests in `messages.rs`, normalization tests in `normalize.rs`
**Status:** READY for verification -- code is complete, tested, and wired into pipeline

### FEED-04: Polymarket Normalization (probability space)
**Requirement text:** "System normalizes Polymarket order books from probability space (0-1) with bid/ask/depth"
**Code location:** `src/feed/polymarket/normalize.rs`
**Key evidence:**
- Probability-space normalization: "Polymarket prices ARE probabilities" -- prices parsed directly to Decimal (line 218-221)
- bid_probability from best bid price (line 166-168)
- ask_probability from best ask price (line 169-170)
- Full depth_bids and depth_asks arrays populated (lines 149-161)
- Staleness gate with exchange timestamp (lines 132-146)
- Exchange timestamp from book event `timestamp` field (line 132)
- Latency metrics emitted (lines 172-178)
**Tests:** `processor_normalizes_book_event`, `processor_handles_stale_data`, `processor_handles_price_change_without_crash`, `parse_price_level_valid/invalid`
**Status:** READY for verification

### FEED-05: Kalshi Connection and Normalization
**Requirement text:** "System connects to Kalshi feed (REST polling or WebSocket) and normalizes contracts into probability + expiry schema"
**Code location:** `src/feed/kalshi/` (7 files)
**Key evidence:**
- RSA-PSS authentication: `auth.rs` sign_kalshi_request, `client.rs` auth headers (lines 78-95)
- WebSocket connection: `connect_async(request)` at client.rs:97-102
- Subscription: `{"cmd": "subscribe", "params": {"channels": ["orderbook_delta"]}}` at client.rs:109-131
- Incremental book: BTreeMap-based `KalshiBook` with apply_snapshot/apply_delta
- Cents-to-probability: `Decimal::new(cents, 2)` at normalize.rs:30-32
- Derived asks from complementary side: `100 - NO_bid_cents` at normalize.rs:194
- Heartbeat timeout (Phase 12): `heartbeat_timeout_ms` at client.rs:142, dead-connection detection at client.rs:160-169
- Exchange timestamp propagation (Phase 12): `last_exchange_ts` HashMap from delta ts field at normalize.rs:138-141
- Latency metrics: `feed_latency_ms` histogram at normalize.rs:258-261
**Tests:** auth signing, message parsing, book management, normalization (28+ tests), exchange timestamp propagation
**Status:** READY for verification

### RELY-04: Graceful Degradation
**Requirement text:** "Feed drops degrade gracefully -- remaining feeds continue operating, affected instruments marked unavailable, degraded state surfaced in metrics"
**Code location:** `src/feed/pipeline.rs` (run_live_multi_venue), `src/feed/health.rs`
**Key evidence:**
- Independent CancellationToken per venue: `cancel.child_token()` at pipeline.rs lines 121, 173, 224
- Shared fan-in channel: `mpsc::channel::<MarketSnapshot>(FAN_IN_BUFFER)` at line 114, each venue gets `snapshot_tx.clone()`
- Missing Kalshi credentials produce warning and skip: pipeline.rs lines 270-278
- VenueHealth tracker: `mark_available()` / `mark_unavailable(error)` with metrics gauges
- Health state observable: `is_available()`, `last_error()`, `last_message_at()`, `connection_count()`
- Supervisor pattern: each venue supervisor calls `health.mark_unavailable()` on connection loss and `health.mark_available()` on first message after reconnect
**Tests:** 8 VenueHealth unit tests covering lifecycle, 160+ overall tests at Phase 4 completion
**Status:** READY for verification

### TEST-01: Mock Data Layer
**Requirement text:** "Mock data layer via trait-based abstraction over data sources -- full pipeline runnable without live venue connections"
**Code location:** `src/feed/traits.rs`, `src/feed/mock/`, `src/feed/pipeline.rs`
**Key evidence:**
- `RawDataSource` trait (traits.rs:24-33): the primary abstraction, with two implementations:
  - `SyntheticDataSource` (mock/synthetic.rs): generates realistic Deribit-format JSON-RPC messages
  - `ReplayDataSource` (mock/replay.rs): replays recorded JSONL files
  - `PolymarketClient` (polymarket/client.rs:179): implements RawDataSource
  - `KalshiClient` (kalshi/client.rs:257): implements RawDataSource
- `DataMode::Mock` at pipeline.rs:95-103: creates SyntheticDataSource and runs full pipeline
- `NormalizedDataSource` trait (traits.rs:40-45): DEAD CODE, zero implementations
- The mock pipeline is functionally complete -- SyntheticDataSource produces messages, DeribitProcessor normalizes them, pipeline outputs MarketSnapshot
**Resolution:** Remove `NormalizedDataSource` trait. The `RawDataSource` trait IS the trait-based abstraction. Mock pipeline already works end-to-end.

## Open Questions

1. **Should NormalizedDataSource be removed or implemented?**
   - What we know: Zero implementations exist. The mock layer works through RawDataSource. The audit identified it as dead code.
   - What's unclear: Could a NormalizedDataSource be useful in the future for testing downstream consumers without running a processor?
   - Recommendation: **Remove it.** The success criteria says "either implemented with at least one concrete implementation or removed as dead code." Implementation adds complexity with no current consumer. The existing `RawDataSource` + processor path already enables full pipeline testing. If needed later, it can be re-added in a future phase with a concrete use case.

2. **Does the verification need to re-verify Phase 12 changes to Kalshi?**
   - What we know: Phase 12 added heartbeat timeout and exchange timestamp propagation to Kalshi, which strengthens FEED-05.
   - What's unclear: Whether Phase 13's verification should include Phase 12 changes or just Phase 4 original scope.
   - Recommendation: **Include Phase 12 enhancements** in the verification evidence for FEED-05. They are part of the same Kalshi feed functionality and the audit flagged Kalshi as needing these improvements. The verification should note that FEED-05 was initially partial and was completed by Phase 12 hardening.

## Sources

### Primary (HIGH confidence)
- `src/feed/polymarket/` -- 5 source files examined directly
- `src/feed/kalshi/` -- 7 source files examined directly
- `src/feed/pipeline.rs` -- full multi-venue pipeline wiring examined
- `src/feed/health.rs` -- VenueHealth tracker examined
- `src/feed/traits.rs` -- RawDataSource and NormalizedDataSource traits examined
- `src/feed/mock/synthetic.rs` -- SyntheticDataSource (mock) examined
- `.planning/v1.0-MILESTONE-AUDIT.md` -- audit findings for Phase 4 gaps
- `.planning/phases/04-multi-venue-feeds/04-{01,02,03}-SUMMARY.md` -- Phase 4 completion records
- `.planning/phases/03-feed-infrastructure/03-VERIFICATION.md` -- canonical verification format
- `.planning/phases/10-critical-pipeline-wiring/10-VERIFICATION.md` -- recent verification example

### Secondary (MEDIUM confidence)
- Grep results confirming NormalizedDataSource has zero implementations
- Test suite: 360+ lib tests, 16 integration, 22 smoke, all passing

## Metadata

**Confidence breakdown:**
- Verification approach: HIGH -- existing codebase verification, all source files directly examined
- Architecture/cleanup: HIGH -- dead code is definitively identified via grep with zero implementations
- Pitfalls: HIGH -- based on actual audit findings and examination of prior verification documents

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable -- verification of existing code, no moving targets)
