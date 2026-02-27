# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-27)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.3 Live Subscription Management -- Phase 22 (Subscription Manager Core)

## Current Position

Phase: 22 of 25 (Subscription Manager Core)
Plan: 1 of 2 in current phase
Status: Executing
Last activity: 2026-02-27 -- Completed 22-01 SubscriptionManager core reconciliation logic

Progress: [███████████████░░░░░░░░░░░░░░░░] 50%

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
- Reconnect-based subscription for all 3 venues (uniform, avoids per-venue protocol differences)
- tokio::sync::watch for pushing instrument lists to supervisors (latest-value semantics)
- tokio::sync::Notify for registry-before-subscription ordering
- Zero new crate dependencies (continues v1.1/v1.2 pattern)
- Tech debt sweep in separate final phase for clean bisectability
- [Phase 22]: SubscriptionManager takes SubscriptionSenders struct for cleaner constructor API

### Pending Todos

None.

### Blockers/Concerns

- Polymarket groupItemTitle format is not guaranteed stable (permissionless market creation)
- Windows atomic rename produces DELETE + RENAME events that may race with file watcher debouncer
- Kalshi may introduce new ticker patterns that bypass extract_kalshi_asset parser
- Stale state after unsubscribe is the primary risk -- SpreadEngine/DeribitProcessor/KalshiProcessor HashMaps grow monotonically (addressed in Phase 24)

## Session Continuity

Last session: 2026-02-27
Stopped at: Completed 22-01-PLAN.md (SubscriptionManager core)
Next action: Execute 22-02-PLAN.md (main.rs wiring)
