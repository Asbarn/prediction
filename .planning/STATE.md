# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-27)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.3 Live Subscription Management -- Phase 25 (Tech Debt Sweep)

## Current Position

Phase: 25 of 25 (Tech Debt Sweep)
Plan: 2 of 2 in current phase
Status: Executing Phase 25
Last activity: 2026-02-28 -- Completed 25-02 Kalshi Staleness Fix

Progress: [████████████████████████████████] 100% (Phase 25, plan 2 of 2)

## Performance Metrics

**v1.0 Summary:**
- Total plans completed: 36
- Total phases: 13
- Lines of Rust: 22,751
- Timeline: 4 days (2026-02-21 to 2026-02-24)

**v1.1 Summary:**
- Plans completed: 11
- Phases: 4 (14-17)
- LOC delta: +14,943 (32,631 total)
- Timeline: 5 days (2026-02-21 to 2026-02-26)

**v1.2 Summary:**
- Plans completed: 8
- Phases: 4 (18-21)
- LOC delta: +2,122 (34,753 total)
- Timeline: 2 days (2026-02-26 to 2026-02-27)

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Full decision history in .planning/milestones/v1.0-ROADMAP.md, v1.1-ROADMAP.md, and v1.2-ROADMAP.md

Key v1.3 decisions:
- [Phase 25]: unwrap_or(false) for missing Kalshi exchange_timestamp -- cannot determine staleness without timestamp
- Reconnect-based subscription for all 3 venues (uniform, avoids per-venue protocol differences)
- tokio::sync::watch for pushing instrument lists to supervisors (latest-value semantics)
- tokio::sync::Notify for registry-before-subscription ordering
- Zero new crate dependencies (continues v1.1/v1.2 pattern)
- Tech debt sweep in separate final phase for clean bisectability
- [Phase 22]: SubscriptionManager takes SubscriptionSenders struct for cleaner constructor API
- [Phase 22]: Explicit drop(reg) before notify_one() to prevent deadlock with read lock acquisition
- [Phase 22]: sub_senders/sub_receivers wrapped in Option for clean flow out of is_live block
- [Phase 23]: PolymarketAsset re-exported from config/mod.rs for supervisor import
- [Phase 23]: One-shot watch channels with immediate sender drop for Mock/Replay modes
- [Phase 23]: Subscription receivers consumed by pipeline function, not post-hoc attached
- [Phase 24]: Metrics emitted after state update so gauges reflect actual current subscription counts
- [Phase 24]: Dry-run skips metrics emission (gauges/counters reflect actual state only)
- [Phase 24]: cleanup_txs uses Vec<mpsc::Sender> not broadcast (fixed number of consumers)
- [Phase 24]: try_send for cleanup events: best-effort non-blocking with warn log on failure
- [Phase 24]: SpreadEngine/CrossAssetEngine use registry active_approved() for cleanup (authoritative source)
- [Phase 24]: PricingEngine uses deribit_instruments from CleanupEvent directly (instrument-keyed, no registry needed)
- [Phase 24]: smiles/smile_points NOT cleaned (Research Pitfall 5: shared expiry dates)
- [Phase 24]: engine_cleanup_rxs returned via PipelineHandles tuple for main.rs-spawned engines

### Pending Todos

None.

### Blockers/Concerns

- Polymarket groupItemTitle format is not guaranteed stable (permissionless market creation)
- Windows atomic rename produces DELETE + RENAME events that may race with file watcher debouncer
- Kalshi may introduce new ticker patterns that bypass extract_kalshi_asset parser
- Stale state after unsubscribe is the primary risk -- SpreadEngine/DeribitProcessor/KalshiProcessor HashMaps grow monotonically (addressed in Phase 24)

## Session Continuity

Last session: 2026-02-28
Stopped at: Completed 25-02-PLAN.md (Kalshi Staleness Fix)
Next action: Continue Phase 25 remaining plans (if any) or complete milestone
