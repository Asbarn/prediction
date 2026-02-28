# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-28)

**Core value:** Accurately detect and quantify real arbitrage opportunities between prediction market prices and options-implied probabilities -- with every false signal caught before it costs money.
**Current focus:** v1.4 Analysis Tooling -- Phase 29: End-to-End Verification

## Current Position

Phase: 29 (fourth of 4 in v1.4) — End-to-End Verification
Plan: 2 of 2 in current phase
Status: Phase 29 complete — v1.4 Analysis Tooling milestone complete
Last activity: 2026-02-28 — Completed 29-02 (signal-scoring E2E golden value tests)

Progress (v1.4): [██████████] 100%

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

**v1.3 Summary:**
- Plans completed: 7
- Phases: 4 (22-25)
- LOC delta: +827 (35,580 total)
- Timeline: 2 days (2026-02-27 to 2026-02-28)

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Full decision history in .planning/milestones/v1.0-ROADMAP.md, v1.1-ROADMAP.md, v1.2-ROADMAP.md, and v1.3-ROADMAP.md

**v1.4 Decisions:**
- 26-01: Decimal for financial mean, f64 for statistical functions (precision vs. computation boundary)
- 26-01: files_in_dir_prefixed for settlement/trade log naming conventions
- 26-01: tempfile dev-dependency for filesystem integration tests
- 26-02: Synchronous fn main() for CLI binaries (no tokio runtime for batch tools)
- 26-02: LoadingSummary as placeholder output before Phases 27-28 add analysis computations
- 26-02: Re-export comfy_table::Table from output.rs for downstream consumers
- 27-01: Aggregate distribution shown first, venue-pair breakdown repeats per-pair detail
- 27-01: Hourly table uses net spread only (primary actionable metric)
- 27-01: Clone SpreadResult refs for per-event computation (simple over dual-signature)
- 27-01: SpreadPattern derives Ord+Hash for BTreeMap key use
- 28-01: Boolean gross_hit/net_hit fields for hit rate computation (not P&L sign)
- 28-01: 365.25-day year for prediction market Sharpe annualization (not 252 trading days)
- 28-01: PSR uses Bailey & Lopez de Prado formula with Fisher bias-corrected moments
- 28-01: statrs 0.18 StudentsT CDF for p-values and Normal CDF for PSR
- 28-02: ScoringOutput wrapper with skip_serializing_if for clean JSON without --by-event
- 28-02: Loading summary to stderr, scoring output to stdout (clean piping for JSON mode)
- 28-02: BTreeMap for by-event grouping ensuring deterministic key ordering
- 29-01: Generate JSONL fixtures via serde serialization (not hand-written JSON) to prevent schema drift
- 29-01: serde_json::Value navigation for flexible JSON assertions in E2E tests
- 29-01: Epsilon tolerance: 1e-6 for deterministic stats, 1e-4 for stddev
- 29-02: cargo_bin() resolves binary from current_exe parent chain for cross-platform compatibility
- 29-02: Shared write_golden_fixture() helper reused across Tests 1-4 for consistency
- 29-02: Epsilon 1e-4 for golden value assertions (sufficient precision for statistical results)

### Pending Todos

None.

### Blockers/Concerns

- Settlement correlation join logic (signal_log to settlement_log by event_id + direction) needs confirmed before Phase 28 planning -- 30-min investigation
- DualTimestamp::deserialize calls tokio::time::Instant::now() -- may pull tokio dep into sync-only CLI binaries
- Polymarket groupItemTitle format is not guaranteed stable (permissionless market creation)
- Windows atomic rename produces DELETE + RENAME events that may race with file watcher debouncer

## Session Continuity

Last session: 2026-02-28
Stopped at: Completed 29-02-PLAN.md (signal-scoring E2E golden value tests)
Next action: v1.4 milestone complete - all phases (26-29) done
