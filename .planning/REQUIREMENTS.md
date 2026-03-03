# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-03-03
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.5 Requirements

Requirements for Derive.xyz venue integration. Each maps to roadmap phases.

### Feed Infrastructure

- [ ] **FEED-01**: Derive WebSocket client connects to `wss://api.lyra.finance/ws` with JSON-RPC 2.0 and auto-reconnection
- [ ] **FEED-02**: Derive orderbook state maintenance from WebSocket subscription with bid/ask depth
- [ ] **FEED-03**: Derive ticker data parsing (mark price, mark IV, bid IV, ask IV, underlying price, greeks)
- [ ] **FEED-04**: DeriveSupervisor with heartbeat monitoring, reconnection, and watch channel for dynamic subscriptions
- [ ] **FEED-05**: JSONL raw feed recording for Derive messages (same pattern as Deribit/Polymarket/Kalshi)

### Data Normalization

- [ ] **NORM-01**: USDC-linear to normalized price conversion for Derive option premiums (Derive quotes in USDC, system needs consistent denomination)
- [ ] **NORM-02**: Derive instrument name parser for `BTC-YYYYMMDD-STRIKE-C/P` format with unit tests
- [ ] **NORM-03**: MarketSnapshot emission from Derive data with all required fields (venue, instrument, bids, asks, IV, greeks, timestamps)
- [ ] **NORM-04**: Staleness detection for Derive snapshots using configurable threshold

### Pipeline Integration

- [ ] **PIPE-01**: `Venue::Derive` enum variant added with all exhaustive match arms resolved across codebase
- [ ] **PIPE-02**: Derive config section in venues.toml (WebSocket URL, rate limits, book depth, staleness threshold)
- [ ] **PIPE-03**: SubscriptionManager extended with Derive venue support (HashSet diff, watch channel, Notify ordering)
- [ ] **PIPE-04**: Derive wired into `run_live_multi_venue()` pipeline -- SpreadEngine, SignalEngine, PaperTradeTracker receive Derive snapshots automatically
- [ ] **PIPE-05**: Prometheus metrics for Derive feed (connection state, message rate, subscription count)

### Discovery & Matching

- [ ] **DISC-01**: Derive REST-based instrument listing via `public/get_instruments` endpoint
- [ ] **DISC-02**: Cross-venue matching between Derive BTC options and Deribit/Polymarket instruments using existing FuzzyMatchKey
- [ ] **DISC-03**: Proposal writing for discovered Derive matches to events.toml (approved = false)
- [ ] **DISC-04**: Discovery integrated into ContractLifecycleManager periodic background pipeline

## Future Requirements

### Settlement Tracking

- **SETL-01**: Derive on-chain settlement resolution checking for expired options
- **SETL-02**: Settlement outcome integration into PaperTradeTracker for Derive legs

## Out of Scope

| Feature | Reason |
|---------|--------|
| Derive private/trading endpoints | v2 execution engine scope; v1.5 is read-only market data |
| Ethereum wallet authentication (k256) | Only needed for private endpoints; public market data requires no auth |
| Derive settlement tracking | Deferred to future; Deribit + Polymarket settlement sufficient for soak test validation |
| ETH/SOL options on Derive | BTC-only initially; same constraint as existing venues |
| Derive-specific analysis CLI extensions | Existing spread-analytics and signal-scoring CLIs work automatically with new venue data |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FEED-01 | Phase 31 | Pending |
| FEED-02 | Phase 31 | Pending |
| FEED-03 | Phase 31 | Pending |
| FEED-04 | Phase 31 | Pending |
| FEED-05 | Phase 31 | Pending |
| NORM-01 | Phase 31 | Pending |
| NORM-02 | Phase 31 | Pending |
| NORM-03 | Phase 31 | Pending |
| NORM-04 | Phase 31 | Pending |
| PIPE-01 | Phase 30 | Pending |
| PIPE-02 | Phase 30 | Pending |
| PIPE-03 | Phase 32 | Pending |
| PIPE-04 | Phase 32 | Pending |
| PIPE-05 | Phase 32 | Pending |
| DISC-01 | Phase 33 | Pending |
| DISC-02 | Phase 33 | Pending |
| DISC-03 | Phase 33 | Pending |
| DISC-04 | Phase 33 | Pending |

**Coverage:**
- v1.5 requirements: 18 total
- Mapped to phases: 18
- Unmapped: 0

---
*Requirements defined: 2026-03-03*
*Last updated: 2026-03-03 after roadmap creation*
