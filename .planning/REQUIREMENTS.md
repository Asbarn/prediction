# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-02-24
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.1 Requirements

Requirements for Paper Trading Validation milestone. Each maps to roadmap phases.

### Settlement Tracking

- [x] **STTL-01**: System polls Deribit REST API for delivery/settlement prices after options expiry
- [x] **STTL-02**: System polls Kalshi REST API for event resolution results
- [x] **STTL-03**: System infers Polymarket event resolution from Gamma API (closed flag + price lock to 0 or 1)
- [x] **STTL-04**: Settlement outcomes are normalized to a unified SettlementOutcome type across all venues
- [x] **STTL-05**: Settlement outcomes are logged to JSONL for historical analysis
- [x] **STTL-06**: Paper trade positions are auto-settled when settlement outcomes arrive
- [x] **STTL-07**: System detects and processes events that expired while offline (backfill on startup)

### Signal Analysis

- [x] **ANLZ-01**: System computes hit rate (profitable-at-settlement / total-settled positions)
- [x] **ANLZ-02**: System computes cost-adjusted average edge per settled position
- [x] **ANLZ-03**: System computes false positive rate (signals resulting in loss at settlement)
- [x] **ANLZ-04**: System computes time-to-convergence (signal generation to price convergence duration)
- [x] **ANLZ-05**: System correlates threshold status (PassedBoth / PassedStaticOnly / Filtered) with settlement outcomes
- [ ] **ANLZ-06**: Analysis metrics are exposed as Prometheus gauges
- [ ] **ANLZ-07**: Analysis results are logged to structured JSONL

### Failure Alerting

- [x] **ALRT-01**: System tracks liveness timestamps per pipeline stage (last spread computed, last signal evaluated, last settlement checked)
- [x] **ALRT-02**: System detects feed silence (venue connected but no messages) beyond configurable threshold
- [x] **ALRT-03**: System detects partial venue coverage (fewer venues reporting than expected)
- [x] **ALRT-04**: System detects signal evaluation gap (no signals evaluated beyond configurable threshold)
- [x] **ALRT-05**: Alerts are emitted via tracing::warn! with structured context
- [x] **ALRT-06**: Alert conditions are exposed as Prometheus metrics

### State Persistence

- [x] **PRST-01**: System periodically checkpoints paper trade state to JSON file
- [x] **PRST-02**: Checkpoint writes use atomic write-then-rename pattern (Windows-compatible)
- [x] **PRST-03**: System recovers paper trade state from checkpoint on startup
- [x] **PRST-04**: System replays JSONL trade events after checkpoint timestamp for complete recovery
- [x] **PRST-05**: Checkpoint includes signal analysis accumulator state

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Enhanced Alerting

- **ALRT-07**: Webhook POST notifications (Slack/Discord/Telegram) for operator alerts
- **ALRT-08**: Health endpoint extension with active alert summary
- **ALRT-09**: Alert deduplication with configurable cooldown periods

### Advanced Analysis

- **ANLZ-08**: Per-pattern performance breakdown by SpreadPattern
- **ANLZ-09**: Per-direction performance breakdown by ArbDirection
- **ANLZ-10**: Cross-venue settlement discrepancy detection (same event, different outcomes)

### Monitoring

- **MON-01**: Terminal UI (TUI) dashboard for live signal and feed monitoring
- **MON-02**: Automated statistical reports (spread distributions, signal frequency, P&L curves)

### Operations

- **OPS-01**: Config maintenance with expired instrument detection and auto-rotation
- **OPS-02**: Full system state persistence (beyond paper P&L and signal history)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Full database (SQLite/PostgreSQL) | < 200KB state volume; file-based persistence sufficient |
| Real-time signal analytics dashboard | Statistics require settlement data arriving hours/days later; real-time display misleading |
| Automated threshold adjustment | Premature on sparse data; surface metrics and let operator adjust TOML |
| Email/SMS/PagerDuty integration | Emit to Prometheus; let Alertmanager handle routing if needed |
| Historical data backfill from venue APIs | Separate data engineering task; v1.1 accumulates going forward |
| Order execution / trade placement | v2 after paper trading validation |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| STTL-01 | Phase 16 | Complete |
| STTL-02 | Phase 16 | Complete |
| STTL-03 | Phase 16 | Complete |
| STTL-04 | Phase 16 | Complete |
| STTL-05 | Phase 16 | Complete |
| STTL-06 | Phase 16 | Complete |
| STTL-07 | Phase 16 | Complete |
| ANLZ-01 | Phase 17 | Complete |
| ANLZ-02 | Phase 17 | Complete |
| ANLZ-03 | Phase 17 | Complete |
| ANLZ-04 | Phase 17 | Complete |
| ANLZ-05 | Phase 17 | Complete |
| ANLZ-06 | Phase 17 | Pending |
| ANLZ-07 | Phase 17 | Pending |
| ALRT-01 | Phase 14 | Complete |
| ALRT-02 | Phase 14 | Complete |
| ALRT-03 | Phase 14 | Complete |
| ALRT-04 | Phase 14 | Complete |
| ALRT-05 | Phase 14 | Complete |
| ALRT-06 | Phase 14 | Complete |
| PRST-01 | Phase 15 | Complete |
| PRST-02 | Phase 15 | Complete |
| PRST-03 | Phase 15 | Complete |
| PRST-04 | Phase 15 | Complete |
| PRST-05 | Phase 15 | Complete |

**Coverage:**
- v1.1 requirements: 25 total
- Mapped to phases: 25
- Unmapped: 0

---
*Requirements defined: 2026-02-24*
*Last updated: 2026-02-24 after roadmap creation*
