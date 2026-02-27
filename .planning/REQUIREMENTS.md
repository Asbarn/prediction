# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-02-27
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.3 Requirements

Requirements for v1.3 Live Subscription Management. Each maps to roadmap phases.

### Subscription Management

- [ ] **SUB-01**: System subscribes to newly approved instrument feeds without restart when operator sets `approved = true` in events.toml
- [ ] **SUB-02**: System unsubscribes from expired/retired instrument feeds without restart when events are archived
- [x] **SUB-03**: Config change to events.toml triggers automatic reconciliation that computes per-venue instrument diffs and issues minimal subscribe/unsubscribe actions
- [x] **SUB-04**: Registry refresh completes before subscription reconciliation reads registry state (ordering guarantee)
- [ ] **SUB-05**: Stale internal state (order books, snapshots, rolling stats) is cleaned up after instruments are unsubscribed
- [x] **SUB-06**: Reconnect-based subscription for all three venues uses latest instrument list from registry, not static startup config

### Observability

- [ ] **OBS-01**: Prometheus gauges show per-venue active subscription count
- [ ] **OBS-02**: Prometheus counters track subscription activations and removals per venue
- [x] **OBS-03**: Structured tracing logs emit subscription diffs on each reconciliation (instruments added/removed per venue)

### Operational Safety

- [ ] **OPS-01**: Dry-run reconciliation mode (config flag) logs what actions would be taken without sending subscribe/unsubscribe commands
- [x] **OPS-02**: Only instruments from `active_approved()` event mappings are subscribed (safety gate preserved)

### Tech Debt (Behavior-Changing)

- [ ] **FIX-01**: `iv_spread` field populated from IV solver metadata instead of always 0.0
- [ ] **FIX-02**: Options `book_depth_levels` read from config instead of hardcoded 0
- [ ] **FIX-03**: Kalshi `is_stale` computed from exchange_timestamp instead of always false

## Future Requirements

Deferred to future milestone. Tracked but not in current roadmap.

### Subscription Enhancements

- **SUB-F01**: In-connection incremental subscribe/unsubscribe for Deribit (avoid reconnect gap)
- **SUB-F02**: Kalshi subscription ID (sid) tracking for native unsubscribe
- **SUB-F03**: Subscription health validation (detect silent subscription failures)
- **SUB-F04**: Graceful subscription transition with overlap period during instrument rolls

### Tech Debt (Non-Behavior-Changing)

- **FIX-F01**: RecordLine.channel empty string for all recorded messages
- **FIX-F02**: pricing_brent_fallbacks_total Prometheus counter not implemented
- **FIX-F03**: Replay processor JoinHandle silently dropped
- **FIX-F04**: Stale REQUIREMENTS.md checkboxes from v1.0
- **FIX-F05**: Expired instrument BTC-27JUN25-100000-C in events.toml
- **FIX-F06**: Kalshi market_tickers = [] empty default
- **FIX-F07**: Missing [health] and [signal_generation] config sections
- **FIX-F08**: Unused exact-match functions from v1.2

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Per-instrument connection isolation | Single connection per venue handles 500+ channels; per-instrument connections violate rate limits |
| Full pipeline restart on subscription change | Defeats the purpose of v1.3; incremental changes on existing connections |
| Automatic approval of subscription changes | Violates `approved = false` safety gate; human review is non-negotiable |
| WebSocket connection pooling/multiplexing | Over-engineered at dozens-of-instruments scale |
| Bidirectional subscription sync (venue -> system) | Only Kalshi supports list_subscriptions; inconsistent abstraction across venues |
| In-connection subscribe/unsubscribe | Reconnect-based approach is uniform and sufficient; per-venue protocols differ |
| Non-behavior-changing tech debt | 8 items deferred to keep scope tight; individually low-impact |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| SUB-01 | Phase 23 | Pending |
| SUB-02 | Phase 23 | Pending |
| SUB-03 | Phase 22 | Complete |
| SUB-04 | Phase 22 | Complete |
| SUB-05 | Phase 24 | Pending |
| SUB-06 | Phase 22 | Complete |
| OBS-01 | Phase 24 | Pending |
| OBS-02 | Phase 24 | Pending |
| OBS-03 | Phase 22 | Complete |
| OPS-01 | Phase 24 | Pending |
| OPS-02 | Phase 22 | Complete |
| FIX-01 | Phase 25 | Pending |
| FIX-02 | Phase 25 | Pending |
| FIX-03 | Phase 25 | Pending |

**Coverage:**
- v1.3 requirements: 14 total
- Mapped to phases: 14
- Unmapped: 0

---
*Requirements defined: 2026-02-27*
*Last updated: 2026-02-27 after roadmap creation*
