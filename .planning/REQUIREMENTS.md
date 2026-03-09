# Requirements: Prediction Market Arbitrage System

**Defined:** 2026-03-09
**Core Value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## v1.8 Requirements

Requirements for Signal Quality Validation milestone. Each maps to roadmap phases.

### Bug Fixes

- [x] **FIX-01**: Cost model subtracts fees in the same unit space as raw spread (probability-space, not dollar-space)
- [x] **FIX-02**: Kalshi taker fee calculation rounds to cents (not integers) via correct Decimal rounding
- [ ] **FIX-03**: Spread logger produces SpreadResult JSONL entries for active Polymarket-vs-options pairs (not gated on Kalshi presence)

### Instrument Quality

- [ ] **INST-01**: Production events.toml contains active near-the-money BTC instrument mappings with real liquidity
- [ ] **INST-02**: Instrument match-audit CLI validates that paired contracts represent the same economic bet (strike, expiry, direction alignment)
- [ ] **INST-03**: Discovery pipeline filters out deep OTM contracts where Polymarket bid-ask spread exceeds configurable threshold

### Diagnostic Tooling

- [ ] **DIAG-01**: Cost-audit CLI breaks down cost components per signal and identifies which costs dominate negative edge
- [ ] **DIAG-02**: Book-depth CLI analyzes Polymarket order book quality (effective spread, fill simulation, depth at price levels)
- [ ] **DIAG-03**: Stats module extended with Pearson correlation and KS test for signal analysis

### Cost Model Validation

- [ ] **COST-01**: Cost model parameters validated against exchange fee documentation (Deribit, Derive, Polymarket)
- [ ] **COST-02**: Parameter sensitivity analysis shows which cost components have largest impact on net edge
- [ ] **COST-03**: On-chain execution costs (gas, bridging) estimated and included in Polymarket leg cost model

### Statistical Validation

- [ ] **STAT-01**: Signal analysis accounts for autocorrelation (effective sample size, not raw count)
- [ ] **STAT-02**: Out-of-sample validation separates training/tuning data from evaluation data
- [ ] **STAT-03**: Final go/no-go report with confidence intervals on expected edge after all fixes applied

## Future Requirements

### Execution Readiness

- **EXEC-01**: Real-time cost parameter updates from live market data
- **EXEC-02**: Minimum liquidity threshold gate before signal emission
- **EXEC-03**: Regime detection for volatility-dependent cost adjustments

## Out of Scope

| Feature | Reason |
|---------|--------|
| Order execution / trade placement | v2 after signal quality validated |
| Live cost parameter auto-tuning | Need validated baseline first; manual tuning sufficient for v1.8 |
| ML-based signal prediction | Arbs are event-driven; statistical validation is the right approach |
| Multi-asset (ETH, SOL) | Validate BTC first before expanding |
| Real-time liquidity dashboard | Offline CLI analysis sufficient for validation; live panels are v1.9 |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| FIX-01 | Phase 44 | Complete |
| FIX-02 | Phase 44 | Complete |
| FIX-03 | Phase 44 | Pending |
| INST-01 | Phase 45 | Pending |
| INST-02 | Phase 45 | Pending |
| INST-03 | Phase 45 | Pending |
| DIAG-01 | Phase 46 | Pending |
| DIAG-02 | Phase 46 | Pending |
| DIAG-03 | Phase 46 | Pending |
| COST-01 | Phase 47 | Pending |
| COST-02 | Phase 47 | Pending |
| COST-03 | Phase 47 | Pending |
| STAT-01 | Phase 48 | Pending |
| STAT-02 | Phase 48 | Pending |
| STAT-03 | Phase 48 | Pending |

**Coverage:**
- v1.8 requirements: 15 total
- Mapped to phases: 15
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after roadmap creation*
