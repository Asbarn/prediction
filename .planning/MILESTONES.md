# Milestones

## v1.0 MVP (Shipped: 2026-02-24)

**Phases completed:** 13 phases, 36 plans
**Lines of Rust:** 22,751
**Timeline:** 4 days (2026-02-21 to 2026-02-24)
**Commits:** 160
**Audit:** 46/46 requirements satisfied, 13/13 phases verified, 8/8 E2E flows

**Key accomplishments:**
- Three-venue feed infrastructure (Deribit WebSocket, Polymarket CLOB, Kalshi) with unified MarketSnapshot channel, reconnection supervisors, and staleness detection
- Black-76 options pricing engine with IV solver (Newton-Raphson + Brent), vol surface interpolation, call spread replication for digital payoffs, and Greeks
- Cross-asset arbitrage signal detection between options-implied probabilities and prediction market prices with configurable dynamic thresholds
- Cost-adjusted spread computation incorporating settlement basis risk premium, venue-specific transaction fees, slippage, and staleness protection
- Deterministic replay from recorded feeds, Prometheus metrics exporter, HTTP health endpoint, paper trade P&L tracking, and stable JSONL schema
- Config-driven event registry mapping cross-venue instruments with quantified settlement basis risk scores and contract lifecycle management

**Tech debt carried forward:** 13 non-blocking items (iv_spread always 0.0, expired test instrument, empty Kalshi default config, options book depth hardcoded 0)

---


## v1.1 Paper Trading Validation (Shipped: 2026-02-26)

**Phases completed:** 4 phases, 11 plans
**LOC delta:** +14,943 lines (32,631 LOC Rust total)
**Timeline:** 5 days (2026-02-21 to 2026-02-26)
**Commits:** 47
**Requirements:** 25/25 satisfied

**Key accomplishments:**
- Failure alerting with 4 alert types (feed silence, partial coverage, signal gap, stage liveness), Prometheus gauges, and cooldown-based dedup preventing log spam
- Atomic checkpoint persistence with JSONL replay recovery — paper trades, daily rollups, and signal analysis accumulators survive restarts
- Three-venue settlement outcome tracking (Deribit delivery prices, Kalshi resolution results, Polymarket Gamma API inference) with 4-tier polling cadence and startup backfill
- Full settlement integration into PaperTradeTracker with per-leg P&L, cross-venue divergence detection, and daily-rotating settlement JSONL
- Signal analysis tooling computing hit rate, cost-adjusted edge, false positive rate, time-to-convergence, and threshold effectiveness across PassedBoth/PassedStaticOnly/Filtered categories
- Filtered signal tracking channel with hypothetical hit rate correlation to answer "are thresholds too aggressive?"

**Tech debt carried forward:** 13 non-blocking items from v1.0 (unchanged)

---


## v1.2 Automated Event Management (Shipped: 2026-02-27)

**Phases completed:** 4 phases, 8 plans
**LOC delta:** +2,122 lines (34,753 LOC Rust total)
**Timeline:** 2 days (2026-02-26 to 2026-02-27)
**Commits:** 16
**Requirements:** 15/15 satisfied

**Key accomplishments:**
- Production-safe venue discovery polling with shared VenueRateLimiter instances, consecutive-absence guards preventing false expirations, and batched TOML writes eliminating race conditions
- Polymarket Gamma API structured discovery with question text parsing (3 BTC price patterns), ExpiryConfidence scoring, and unified Vec<DiscoveredInstrument> type across all three venues
- Cross-venue fuzzy matching via FuzzyMatchKey (asset/strike/direction) with configurable expiry tolerance window, handling Deribit Friday and Kalshi end-of-month expiry differences
- Proposal workflow with WARN-level structured tracing logs, Prometheus proposals_pending gauge and proposals_total counter, and atomic TOML writes preserving formatting
- Approved-mapping validation on config reload (venue count >= 2, expiry not past) plus async instrument-activity warnings gated behind discovery data availability
- Event archival from events.toml to events_archive.toml with Retired lifecycle status, automatic unapproved-candidate cleanup, and full pipeline running as periodic background task in ContractLifecycleManager

**Tech debt carried forward:** 13 non-blocking items from v1.0 (unchanged) + 2 low-severity items (unused exact-match functions preserved for backward compat, expiry_confidence TOML field is write-only)

---

