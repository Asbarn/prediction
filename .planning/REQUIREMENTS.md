# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-03-09
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.7 Requirements

Requirements for Prediction Market Signal Pipeline. Each maps to roadmap phases.

### Polymarket Connectivity

- [ ] **POLY-01**: System diagnoses Polymarket WebSocket failure mode from EC2 (connection reset vs silent freeze vs geo-block)
- [ ] **POLY-02**: Polymarket supervisor detects data inactivity (silent freeze) and triggers reconnection after configurable timeout
- [ ] **POLY-03**: Polymarket WebSocket feed connects and delivers order book data from production EC2 instance
- [ ] **POLY-04**: REST polling fallback fetches Polymarket prices when WebSocket is unavailable, using existing reqwest/governor
- [ ] **POLY-05**: Source coordinator switches between WebSocket and REST modes exclusively (no duplicate/conflicting prices)

### Signal Engine

- [ ] **SIG-01**: ImpliedProbability struct includes source venue field (Deribit or Derive) instead of hardcoded Deribit
- [ ] **SIG-02**: CrossAssetEngine generates ArbSignals using implied probabilities from any options venue (not just Deribit)
- [ ] **SIG-03**: CrossAssetEngine generates signals with a single prediction market venue (Polymarket alone, without requiring Kalshi)

### Verification

- [ ] **VER-01**: Production system generates cross-asset arbitrage signals visible in Grafana dashboards
- [ ] **VER-02**: Signal and spread JSONL logs contain entries from live production data

## Future Requirements

### Spread Engine Generalization

- **SPREAD-01**: SpreadEngine supports single prediction market vs options-implied probability spreads
- **SPREAD-02**: SpreadPattern enum uses venue-generic identifiers instead of Polymarket/Kalshi-specific names

### Multi-Venue Options

- **OPT-01**: Cross-options-venue spread detection (Deribit vs Derive) for same-instrument pricing discrepancies

## Out of Scope

| Feature | Reason |
|---------|--------|
| SpreadEngine refactoring | SpreadEngine handles prediction-vs-prediction correctly; CrossAssetEngine is the right target for v1.7 |
| Kalshi connectivity fixes | Geo-blocked from Poland; no path to resolution without US-based infrastructure |
| Deribit-vs-Derive options spreads | Not the core value proposition; prediction market arbitrage is priority |
| REST fallback for other venues | Deribit/Derive/Kalshi WebSocket feeds are stable; only Polymarket needs REST fallback |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| POLY-01 | Phase 40 | Pending |
| POLY-02 | Phase 40 | Pending |
| POLY-03 | Phase 40 | Pending |
| POLY-04 | Phase 42 | Pending |
| POLY-05 | Phase 42 | Pending |
| SIG-01 | Phase 41 | Pending |
| SIG-02 | Phase 41 | Pending |
| SIG-03 | Phase 41 | Pending |
| VER-01 | Phase 43 | Pending |
| VER-02 | Phase 43 | Pending |

**Coverage:**
- v1.7 requirements: 10 total
- Mapped to phases: 10
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after roadmap creation*
