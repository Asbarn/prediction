# Phase 16: Settlement Outcome Tracking - Context

**Gathered:** 2026-02-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Determine how prediction market events and options expirations actually resolved across all three venues (Deribit, Polymarket, Kalshi). Settle paper trade positions with per-leg P&L computation. Provide ground truth for Phase 17 signal quality analysis. Handle events that resolved while the system was offline.

</domain>

<decisions>
## Implementation Decisions

### Polling Strategy

- **Three-tier trigger model:**
  - Deribit: Poll settlement endpoint once after 08:00 UTC on expiry day, retry with backoff. Deterministic timing from ContractLifecycleManager.
  - Prediction markets with paired Deribit expiry: Lazy polling (30-60 min) → aggressive (2-5 min) once Deribit leg settles. Deribit settlement is the trigger anchor.
  - Prediction markets without paired option (Poly-vs-Kalshi only): Fixed moderate cadence. Contract's stated end date provides rough timing.
- **Four-phase polling cadence after trigger:**
  - Aggressive (0-4 hours): Every 2-5 minutes — covers Polymarket optimistic oracle (98.5% of markets) and Kalshi normal resolution.
  - Patient (4-96 hours): Every 15-30 minutes — UMA DVM dispute window.
  - Lazy (96 hours - 7 days): Every 2-4 hours, log at WARN level.
  - Timeout at 7 days: Stop polling, mark as `resolution_timeout`, emit metric. Operator investigates.
- **Timeout is configurable per venue in TOML.** `resolution_timeout` is a distinct state from settled outcomes, never contaminates signal quality metrics.

### Architecture

- **Dedicated SettlementMonitor tokio task** following the AlertMonitor pattern from Phase 14. Separate from PaperTradeTracker's hot path.
- **Data flow:** ContractLifecycleManager → SettlementMonitor (expiry awareness) → venue API polling → SettlementOutcome on channel → PaperTradeTracker (position settlement).
- **Trait-based venue client** for testability. Each venue implements a resolution check trait returning `ResolutionResult`.
- **Shared rate limiter** from Phase 3 (Arc<RateLimiter> per venue). No independent rate budget — single source of truth for API budget across feeds, settlement, and future execution.

### Resolution Logic

- **Authoritative API status, not price inference.** Polymarket: Gamma API market status + resolution field. Kalshi: settlement endpoint with explicit status. Deribit: get_delivery_prices for TWAP settlement value.
- **Two-stage check:** Query authoritative API for resolution status, then sanity-check outcome token prices against declared outcome. If mismatch (e.g., status says "resolved YES" but YES token at $0.40), mark as `resolution_anomaly`, do not auto-settle.
- **ResolutionResult enum:**
  - `NotYetResolved` — keep polling
  - `Resolved { outcome, settlement_price, resolved_at }` — emit SettlementOutcome
  - `Disputed { dispute_started }` — stay in patient polling tier
  - `Ambiguous { raw_data }` — Kalshi 6.3(c) case, special handling
- **WS price collapse** (token price locking to 0.00/1.00) is a trigger for when to start aggressive polling, not the settlement determination.

### Settlement Outcome Type

- **Rich SettlementOutcome struct** carrying full context:
  - `event_id`, `venue`, `outcome: OutcomeKind` (Yes/No/Ambiguous/Timeout)
  - `settlement_price: Option<Decimal>` — Deribit TWAP, Kalshi 6.3(c) price, None for clean binary
  - `resolved_at` (when venue confirmed) vs `detected_at` (when SettlementMonitor observed)
  - `resolution_source: ResolutionSource` — GammaApi, DeribitDelivery, KalshiSettlement
  - `raw_response: Option<String>` — for debugging during paper trading
- **No numeric confidence field.** Trust comes from `ResolutionSource` enum (GammaApi is authoritative, PriceInference is not).

### Cross-Venue Divergence

- **Per-leg settlement, not per-event.** Each venue leg settles independently using its own SettlementOutcome. Combined P&L = sum of legs.
- **SettlementDivergence annotation** when venues disagree on same event:
  - `divergence_type: DivergenceType` (BinaryDisagree, PriceMismatch, TimingGap, AmbiguousResolution)
  - `basis_risk_score_at_entry` — what the system predicted at signal time
  - `actual_impact_bps` — how much divergence affected P&L
- **Three analytics buckets** for Phase 17: concordant (clean binary, core signal quality), divergent (venues disagree, measures basis risk prediction), ambiguous (6.3(c)/timeout, quarantined from metrics).

### Auto-Settlement & P&L

- **Settle each leg immediately** as its SettlementOutcome arrives. Don't wait for all legs.
- **Both raw and fee-adjusted P&L per leg** via SettledLeg struct:
  - `raw_pnl` (settlement - entry) * notional * direction
  - `entry_fee`, `exit_fee`, `slippage_estimate` — from SpreadEngine's existing fee models
  - `net_pnl` = raw_pnl - fees - slippage
  - `fee_model_version` — enables retroactive P&L recalculation when fee models are refined
- **Position-level rollup:** total raw P&L, total net P&L, total fees, total slippage, net-to-gross ratio.
- **Daily rollup headline number = net P&L** (fee-adjusted), raw available as drill-down.

### Position Lifecycle & Memory Management

- **Position states:** Open → PartiallySettled → FullySettled → evicted
- **Remove settled positions from active tracker** after computing divergence annotation and flushing SettlementRecord to JSONL.
- **48-hour retention window** in bounded `recently_settled` VecDeque (capped at 100 positions or 48 hours, whichever evicts first).
- **Timeout positions** evicted immediately — operator investigates via JSONL logs.
- **Prometheus metrics emitted at settlement time** for live dashboard: `paper_trades_settled_total`, `paper_trade_net_pnl` histogram, `paper_trade_settlement_latency_seconds`, `paper_trade_divergence_total`.
- **JSONL SettlementRecord is single source of truth** for all historical analysis. Contains complete SettledLeg structs, position rollup, divergence annotation, fee model versions.

### Offline Backfill

- **Checkpoint-anchored with configurable 7-day cap.** On startup, scan only events that had open positions at last checkpoint, starting from each event's last-check timestamp.
- **Stale position handling:** If time_since_last_check > max_lookback (7d default), mark as `resolution_timeout` immediately, emit to JSONL, skip. Prevents stale checkpoint from causing unbounded API load.
- **Backfill queue processed oldest-first** using `try_acquire` on shared rate limiter (non-blocking, feeds take priority during startup contention).
- **Missing checkpoint = clean start.** No phantom settlements for events the system never traded.
- **Extend Phase 15 CheckpointState** with settlement-related fields (last_settlement_check per position, polling tier). Single file, single atomic write.

### Claude's Discretion

- Exact REST client implementation for each venue's settlement API
- Retry and backoff timing constants within the tiered cadence framework
- Internal data structures for the backfill queue and polling scheduler
- SettlementMonitor's internal state machine for tracking polling tier transitions

</decisions>

<specifics>
## Specific Ideas

- Polymarket UMA oracle: optimistic path resolves ~98.5% of markets within 2 hours. DVM disputes take 48-96 hours. The polling tiers map directly to these real-world timelines.
- Kalshi Rule 6.3(c): When an event doesn't resolve cleanly, Kalshi settles at last-traded price (not binary 0/1). This needs explicit `OutcomeKind::Ambiguous` handling — not a signal failure, a venue mechanics issue.
- The Cardi B case (Kalshi $0.26 vs Polymarket $1.00 for same event) is the canonical divergence example. Per-leg P&L computation captures the true edge (74 cents on a cross-venue position) that per-event "pick one venue" would destroy.
- Net-to-gross ratio (total_net_pnl / total_raw_pnl) is the single number that tells the operator what fraction of gross edge survives transaction costs. Below 0.3 = wider thresholds needed. Above 0.7 = fee model might be optimistic.
- SettlementMonitor should use read access to EventRegistry (which venues to poll) and ContractLifecycleManager's expiry state (when to start polling). Both are already shared via Arc.

</specifics>

<deferred>
## Deferred Ideas

- Priority tiers for rate limiter (execution > settlement > feeds) — v2 concern when live execution engine exists
- Automatic basis risk score updates from divergence data — v2, operator review for now
- Dashboard UI for settlement tracking — v2 MON-01, Prometheus metrics sufficient for v1.1

</deferred>

---

*Phase: 16-settlement-outcome-tracking*
*Context gathered: 2026-02-24*
