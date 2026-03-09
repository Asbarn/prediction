---
phase: 47-cost-model-validation
plan: 01
subsystem: analysis
tags: [cost-model, fees, deribit, derive, polymarket, cli, validation]

requires:
  - phase: 46-diagnostic-cli-tools
    provides: analysis output helpers, CLI patterns (clap, comfy-table)
provides:
  - Venue-differentiated options fee calculation (Derive 0.04%+$0.50 vs Deribit 0.03%)
  - On-chain cost fields (gas_cost_usd, bridge_cost_amortized_usd) in PolymarketFeeConfig
  - cost-validate CLI for auditing fee parameters against exchange documentation
affects: [47-02, signal-engine, cost-audit, spread-config]

tech-stack:
  added: []
  patterns: [venue-aware fee dispatch in signal engine, config validation with source citations]

key-files:
  created:
    - src/analysis/cost_validate.rs
    - src/bin/cost_validate.rs
  modified:
    - src/spread/config.rs
    - src/signal/config.rs
    - src/signal/engine.rs
    - src/spread/cost_model.rs
    - src/analysis/mod.rs
    - Cargo.toml

key-decisions:
  - "On-chain costs fold into dollar normalization alongside prediction_fee to avoid breaking CostBreakdown JSONL schema"
  - "Bridge cost defaults to zero (Undocumented status) since it depends on operator bridging pattern"

patterns-established:
  - "Venue match dispatch for fee calculation: match prob.source_venue { Venue::Derive => ..., _ => deribit_rate }"
  - "Config validation pattern: expected value + source citation per parameter"

requirements-completed: [COST-01, COST-03]

duration: 8min
completed: 2026-03-09
---

# Phase 47 Plan 01: Cost Model Validation Summary

**Venue-differentiated options fees (Derive 0.04% + $0.50 base vs Deribit 0.03%), Polygon gas/bridge cost fields, and cost-validate CLI with exchange fee citations**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T21:48:27Z
- **Completed:** 2026-03-09T21:56:03Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Signal engine now dispatches venue-specific fee rates for Derive vs Deribit options
- PolymarketFeeConfig extended with gas_cost_usd ($0.01 default) and bridge_cost_amortized_usd (operator-configured)
- cost-validate CLI produces validation report with 9 parameters, each citing exchange documentation
- Table and JSON output modes with exit code 0 (clean) / 1 (issues found)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add venue-differentiated fee config and on-chain cost fields** - `0a9b121` (feat)
2. **Task 2: Create cost-validate CLI with exchange fee documentation** - `0398fb8` (feat)

## Files Created/Modified
- `src/spread/config.rs` - Added gas_cost_usd and bridge_cost_amortized_usd to PolymarketFeeConfig
- `src/signal/config.rs` - Added derive_taker_fee_rate (0.0004) and derive_base_fee_usd ($0.50)
- `src/signal/engine.rs` - Venue-aware options fee dispatch + on-chain cost normalization
- `src/spread/cost_model.rs` - Updated test struct literals for new PolymarketFeeConfig fields
- `src/analysis/cost_validate.rs` - Validation logic with 9 parameters and source citations
- `src/analysis/mod.rs` - Registered cost_validate module
- `src/bin/cost_validate.rs` - cost-validate CLI binary
- `Cargo.toml` - Added [[bin]] entry for cost-validate

## Decisions Made
- On-chain costs (gas + bridge) fold into the dollar-denominated normalization alongside prediction_fee rather than adding new CostBreakdown fields, to avoid breaking downstream JSONL parsing
- Bridge cost defaults to zero with Undocumented validation status since it depends on operator's bridging pattern ($5-20 from Ethereum, $0.50-2 from exchanges)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed PolymarketFeeConfig struct literals in existing tests**
- **Found during:** Task 1 (adding new fields to PolymarketFeeConfig)
- **Issue:** Existing tests in cost_model.rs and engine.rs constructed PolymarketFeeConfig without the new gas_cost_usd and bridge_cost_amortized_usd fields
- **Fix:** Added missing fields (set to Decimal::ZERO in engine tests, used ..Default::default() in cost_model tests)
- **Files modified:** src/spread/cost_model.rs, src/signal/engine.rs
- **Verification:** All 674 tests pass
- **Committed in:** 0a9b121 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary fix for compilation after adding required fields. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- cost-validate CLI ready for operator use
- Venue-aware fee model ready for production deployment
- Plan 47-02 can proceed with additional cost model validation work

---
*Phase: 47-cost-model-validation*
*Completed: 2026-03-09*
