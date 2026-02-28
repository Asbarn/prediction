# Prediction Market Arbitrage System

## What This Is

A production-grade cross-venue arbitrage signal generator in Rust that detects pricing discrepancies between crypto prediction markets (Polymarket, Kalshi) and options markets (Deribit). Compares prediction market binary contract prices against options-implied probabilities derived via Black-76 pricing with call spread replication, generates trading signals when spreads exceed cost-adjusted thresholds, tracks settlement outcomes for signal validation, computes statistical evidence of signal quality, automatically discovers new cross-venue instrument matches with operator-approved proposals, and dynamically manages feed subscriptions as instruments are approved or retired. Built as a single-binary service for a solo trader with 35,580 lines of Rust and full deterministic replay capability.

## Core Value

Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.

## Requirements

### Validated

- v1.0 Deribit WebSocket feed with order book maintenance and JSON-RPC 2.0 parsing
- v1.0 Polymarket CLOB WebSocket feed with probability-space order books
- v1.0 Kalshi feed (WebSocket) normalized to same schema
- v1.0 Unified MarketSnapshot bus via bounded async channels
- v1.0 Raw feed recording to line-delimited JSON with timestamps
- v1.0 Event registry mapping equivalent instruments across venues (TOML-driven)
- v1.0 Settlement basis analyzer quantifying expiry/oracle/resolution differences
- v1.0 IV solver (Newton-Raphson/Brent) for Black-76 options pricing
- v1.0 Probability extractor: N(d2), call spread replication, smile interpolation
- v1.0 Greeks calculator (delta, vega, theta) for position monitoring
- v1.0 Spread calculator with cost, slippage, funding, and basis risk adjustments
- v1.0 Staleness detection rejecting stale data per configurable threshold
- v1.0 Signal generation with dynamic edge thresholds
- v1.0 Continuous spread logging and periodic aggregate metrics
- v1.0 Prometheus metrics exporter
- v1.0 Structured logging via tracing with JSON output and correlation IDs
- v1.0 Mock/replay data layer for development and backtesting
- v1.0 Config-driven TOML for all parameters
- v1.0 Graceful degradation on feed drops
- v1.0 Deterministic replay from recorded feeds
- v1.0 HTTP /health endpoint
- v1.0 Paper trade P&L tracking
- v1.0 Contract lifecycle management with expiry rolls
- v1.0 Per-venue heartbeat monitoring and reconnection supervisors
- v1.1 Settlement outcome tracking from Deribit, Kalshi, and Polymarket with 4-tier polling
- v1.1 Signal analysis tooling (hit rate, edge, false positive rate, time-to-convergence, threshold effectiveness)
- v1.1 Failure alerting for degraded states (feed silence, partial coverage, signal gap, stage liveness)
- v1.1 File-based state persistence with atomic checkpoints and JSONL replay recovery

- v1.2 Automated three-venue market discovery (Polymarket Gamma API, Deribit, Kalshi) with shared rate limiters and absence guards
- v1.2 Cross-venue fuzzy matching (asset/strike/direction) with configurable expiry tolerance and confidence scoring
- v1.2 Automatic proposal writing to events.toml (approved = false) with structured WARN logging and Prometheus metrics
- v1.2 Approved-mapping validation on config reload (venue count, expiry, instrument activity)
- v1.2 Expired event archival to events_archive.toml with Retired lifecycle status
- v1.2 Unapproved candidate auto-cleanup and full pipeline as periodic background task

- v1.3 Dynamic feed subscription for newly approved instruments without restart (reconnect-based, all 3 venues)
- v1.3 Dynamic feed unsubscription for expired/retired instruments with stale state cleanup across 5 engines
- v1.3 Config-change-driven subscription reconciliation with per-venue HashSet diff, Notify ordering, and structured tracing
- v1.3 Prometheus subscription metrics (active gauge, activation/removal counters per venue) and dry-run reconciliation mode
- v1.3 iv_spread populated from actual IV solver bid-ask spread, options book depth config-driven, Kalshi staleness from exchange_timestamp

### Active

(No active requirements -- planning next milestone)

### Out of Scope

- Order execution / trade placement -- v2 after paper trading validation
- Venue API authentication for private/trading endpoints -- v2
- Real-time P&L and position tracking -- v2
- Risk limits engine and kill switch -- v2
- Margin monitoring -- v2
- Multi-asset support (ETH, SOL) -- after BTC binary events validated; architecture supports via config
- UI / dashboard -- solo trader monitors via logs and metrics
- AI/ML signal prediction -- arbs are event-driven, not pattern-driven
- Sub-millisecond latency -- arb windows are minutes-to-hours
- NLP/ML-based Polymarket question parsing -- regex sufficient for predictable BTC price patterns
- Automatic approval of high-confidence matches -- human gate is non-negotiable safety mechanism
- Database-backed event store -- TOML sufficient at dozens-to-hundreds of entries, human-readable and git-trackable

## Context

**Shipped v1.3 Live Subscription Management** (2026-02-28) with 35,580 LOC Rust across 25 phases (4 milestones).
Tech stack: Rust (2024 edition), tokio, rust_decimal, serde, axum, metrics/prometheus, statrs, tracing, strsim.
3 venues operational: Deribit (WebSocket + REST settlement + discovery), Polymarket (CLOB WebSocket + Gamma API discovery), Kalshi (WebSocket + REST + discovery).
Dynamic subscription: SubscriptionManager with per-venue reconciliation, watch channels to supervisors, Notify ordering, dry-run mode, and stale state cleanup across 5 engines.
Automated event management: three-venue discovery with fuzzy matching, confidence-scored proposals, approved-mapping validation, expired event archival, and periodic background pipeline.
Settlement tracking: 3 venue resolution checkers with 4-tier polling cadence, startup backfill, and auto-settlement.
Signal analysis: hit rate, cost-adjusted edge, false positive rate, time-to-convergence, threshold effectiveness tracking.

**System status:** Fully operational with dynamic subscription management. When operator approves/archives instruments in events.toml, the system subscribes/unsubscribes feeds without restart and cleans up stale internal state. System can run unattended for paper trading with self-managing event lifecycle and self-managing feed subscriptions.

**Next priority:** Run extended paper trading to validate discovery + signal quality before v2 execution engine.

**Known tech debt:** 10 non-blocking items from v1.0 + 2 low-severity from v1.2 + 3 non-critical from v1.3 audit. See MILESTONES.md for full list.

## Constraints

- **Language**: Rust (latest stable, 2024 edition)
- **Async runtime**: tokio
- **Decimal arithmetic**: `rust_decimal` for all prices and probabilities
- **Deployment**: Single-binary Linux service
- **Deribit API**: 20 req/s rate limit on private endpoints
- **Polymarket**: On-chain (Polygon) -- gas, wallet, approvals matter for v2 execution
- **Kalshi**: US-regulated, different API semantics and fee structures

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Paper trading before execution | Validate signal quality before risking capital | v1.0 Validated -- system ready for paper trading |
| BTC-only initially | Highest liquidity across all venues | v1.0 Validated -- BTC pipeline complete |
| JSONL feed recording | Easy to grep/parse; human-readable for debugging | v1.0 Validated -- stable schema, golden tests |
| Call spread replication as primary digital pricing | More robust than naive N(d2) under volatility skew | v1.0 Validated -- primary method with confidence scoring |
| Config-driven via TOML | No recompilation for parameter changes during tuning | v1.0 Validated -- all parameters configurable |
| Mock + real data layers | Development without API keys; recordings become replay corpus | v1.0 Validated -- deterministic replay operational |
| 9-phase structure (expanded from 6) | Clearer delivery boundaries (reliability separate from connection, etc.) | v1.0 Validated -- clean incremental delivery |
| Deribit feed first | Proves pipeline architecture before multi-venue complexity | v1.0 Validated -- architecture held through 3 venues |
| Prediction market arb before cross-asset | Validates pipeline with simpler math before Black-76 | v1.0 Validated -- both spread engines operational |
| Gamma omitted from Greeks | User decision: delta/vega/theta sufficient for paper trading | v1.0 Accepted |
| Flat extrapolation for vol surface | Returns boundary IV rather than None for extreme strikes | v1.0 Validated -- graceful degradation |
| Non-blocking try_send for secondary engines | Primary engine (SpreadEngine) blocking, others best-effort | v1.0 Validated -- no pipeline stalls |
| BasisRiskCache with try_read | Never blocks engine hot path; zero premium on lock contention | v1.0 Validated -- no measurable latency impact |
| Zero new crate dependencies for v1.1 | All features built on existing dependency tree | v1.1 Validated -- no supply chain growth |
| Alerting first in v1.1 build order | Monitors running during rest of development | v1.1 Validated -- caught no issues during dev |
| AtomicI64 for pipeline liveness timestamps | Lock-free reads in hot path vs Mutex<DateTime> | v1.1 Validated -- zero contention |
| VenueChecker enum dispatch (not async-trait) | Zero new dependencies for venue settlement checking | v1.1 Validated -- clean pattern |
| Checkpoint version as u32 (not semver) | Compact schema evolution with backward-compatible serde(default) | v1.1 Validated -- v1 through v4 forward-compatible |
| Filtered signals via try_send (best-effort) | Avoid backpressure on SpreadEngine hot path | v1.1 Validated -- no pipeline stalls |
| String parsing over regex for Polymarket questions | 3 predictable BTC price patterns; regex adds complexity | v1.2 Validated -- parses all current formats |
| endDateIso as authoritative expiry source | Question text dates vary; API field is canonical | v1.2 Validated -- reliable across venues |
| FuzzyMatchKey (asset/strike/direction) for matching | Expiry checked separately against tolerance window | v1.2 Validated -- handles cross-venue expiry differences |
| Batched TOML writes per poll cycle | Prevents write/file-watcher race conditions | v1.2 Validated -- atomic write pattern |
| N consecutive absences before expiry | Prevents false expirations from partial API responses | v1.2 Validated -- configurable threshold |
| approved = false as non-negotiable safety gate | Human approval required before capital allocation | v1.2 Validated -- core safety mechanism |
| Live subscription management deferred to v1.3 | Restart-on-approval is acceptable for paper trading | v1.2 Accepted -- shipped in v1.3 |
| Single new dependency (strsim) for v1.2 | Already compiled transitively via clap_builder | v1.2 Validated -- zero supply chain growth |
| Archive-then-remove safety pattern | Archive file written atomically before entries removed | v1.2 Validated -- no data loss risk |
| Reconnect-based subscription for all 3 venues | Uniform approach avoids per-venue protocol differences | v1.3 Validated -- all venues respond to watch channel updates |
| tokio::sync::watch for instrument list push | Latest-value semantics; supervisors always get current list | v1.3 Validated -- clean pattern |
| tokio::sync::Notify for registry-before-subscription ordering | Ensures registry refresh completes before reconciliation reads | v1.3 Validated -- no race conditions |
| Zero new crate dependencies for v1.3 | Continues v1.1/v1.2 pattern of building on existing deps | v1.3 Validated -- zero supply chain growth |
| Tech debt sweep in separate final phase | Clean bisectability; behavior changes isolated from subscription work | v1.3 Validated -- clean separation |
| Vec<mpsc::Sender> for cleanup channels | Fixed number of consumers (5 engines); no broadcast needed | v1.3 Validated -- simple, correct |
| Registry-retain pattern for stale state cleanup | Engines read active_approved(), retain matching entries only | v1.3 Validated -- authoritative cleanup source |

---
*Last updated: 2026-02-28 after v1.3 milestone completion*
