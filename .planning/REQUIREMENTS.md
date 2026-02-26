# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-02-26
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.2 Requirements

Requirements for Automated Event Management milestone. Each maps to roadmap phases.

### Discovery

- [ ] **DISC-01**: System polls Polymarket Gamma API with crypto category filtering and extracts structured fields (asset, strike, direction, expiry) from groupItemTitle patterns
- [ ] **DISC-02**: System polls Deribit and Kalshi APIs for new instruments with shared rate limiters and consecutive-absence expiry guards
- [ ] **DISC-03**: System matches cross-venue instruments using exact asset/strike/direction with configurable expiry date tolerance window (default 7 days)
- [ ] **DISC-04**: System generates cross-venue candidate proposals including instruments from all matched venues with expiry confidence scoring (HIGH/MEDIUM/LOW based on date difference)

### Proposals

- [ ] **PROP-01**: System writes candidate mappings to events.toml with approved = false via atomic TOML writes preserving formatting and comments
- [ ] **PROP-02**: System emits structured tracing log with event_id, matched venues, instruments, expiry dates, and confidence when a new candidate is proposed
- [ ] **PROP-03**: System exposes Prometheus gauges for pending proposal count and total proposals counter
- [ ] **PROP-04**: System validates approved mappings on config reload (at least 2 venue instruments, instruments still active, expiry not passed)

### Lifecycle

- [ ] **LIFE-01**: System archives expired events older than configurable retention period (default 30 days) from events.toml to events_archive.toml
- [ ] **LIFE-02**: System auto-cleans unapproved candidates past their expiry date
- [ ] **LIFE-03**: System adds Retired status to LifecycleStatus for fully settled and archived events
- [x] **LIFE-04**: System requires N consecutive absence polls before marking an instrument as expired (prevents false expirations from partial API responses)

### Integration

- [ ] **INTG-01**: Discovery manager runs as periodic background task within ContractLifecycleManager poll cycle
- [ ] **INTG-02**: Polymarket discovery returns Vec<DiscoveredInstrument> (same type as Deribit/Kalshi) for unified cross-venue matching
- [x] **INTG-03**: All TOML writes use existing VenueRateLimiter and batch writes per poll cycle (not per-candidate)

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Live Subscription Management

- **SUBS-01**: Dynamic feed subscription for newly approved instruments without restart
- **SUBS-02**: Dynamic feed unsubscription for expired/retired instruments
- **SUBS-03**: Config-change-driven subscription reconciliation (diff old vs new active instrument sets)

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

- **OPS-01**: Configurable Polymarket question text pattern library (TOML-driven regex)
- **OPS-02**: Match confidence composite scoring (expiry alignment + venue count + liquidity)
- **OPS-03**: Historical match accuracy tracking (approval rate metrics)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| NLP/ML-based Polymarket question parsing | Regex sufficient for predictable BTC price patterns; ML adds heavyweight deps |
| Full-text fuzzy matching across venues | Wrong approach for structured data; high false positive rate |
| Automatic approval of high-confidence matches | Safety risk for capital allocation; human gate is non-negotiable |
| Real-time venue event streams for discovery | No venue supports push notifications for new instruments; polling is sufficient |
| Multi-asset discovery (ETH, SOL) in v1.2 | Validate BTC automation first; architecture supports multi-asset via config |
| Database-backed event store | TOML is sufficient at dozens-to-hundreds of entries; human-readable and git-trackable |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| DISC-01 | Phase 19 | Pending |
| DISC-02 | Phase 18 | Pending |
| DISC-03 | Phase 19 | Pending |
| DISC-04 | Phase 19 | Pending |
| PROP-01 | Phase 20 | Pending |
| PROP-02 | Phase 20 | Pending |
| PROP-03 | Phase 20 | Pending |
| PROP-04 | Phase 20 | Pending |
| LIFE-01 | Phase 21 | Pending |
| LIFE-02 | Phase 21 | Pending |
| LIFE-03 | Phase 21 | Pending |
| LIFE-04 | Phase 18 | Complete |
| INTG-01 | Phase 21 | Pending |
| INTG-02 | Phase 19 | Pending |
| INTG-03 | Phase 18 | Complete |

**Coverage:**
- v1.2 requirements: 15 total
- Mapped to phases: 15
- Unmapped: 0

---
*Requirements defined: 2026-02-26*
*Last updated: 2026-02-26 after roadmap creation (v1.2 phases 18-21)*
