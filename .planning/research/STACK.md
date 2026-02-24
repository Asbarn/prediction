# Stack Research: v1.1 Paper Trading Validation

**Domain:** Settlement outcome tracking, signal analysis, failure alerting, file-based state persistence
**Researched:** 2026-02-24
**Confidence:** HIGH

## Scope

This document covers ONLY the stack additions needed for v1.1. The existing v1.0 stack (tokio, rust_decimal, serde/serde_json, axum, metrics/prometheus, statrs, tracing, chrono, reqwest, etc.) is validated and unchanged. See v1.0 STACK.md for those decisions.

## Existing Stack (DO NOT Re-add)

These are already in `Cargo.toml` and validated by v1.0. Listed here to prevent duplicate research:

| Technology | Version | Purpose |
|------------|---------|---------|
| tokio | 1.x (full) | Async runtime |
| serde + serde_json | 1.0 | Serialization |
| toml | 0.8 | Config parsing |
| rust_decimal | 1.40 | Decimal arithmetic |
| statrs | 0.18 | Statistical distributions |
| chrono | 0.4 | Date/time |
| tracing / tracing-subscriber / tracing-appender | 0.1 / 0.3 / 0.2 | Structured logging |
| metrics + metrics-exporter-prometheus | 0.24 / 0.18 | Prometheus metrics |
| axum | 0.8 | HTTP endpoint |
| reqwest | 0.12 | HTTP client |
| uuid | 1.x (v7) | Time-ordered IDs |
| thiserror / anyhow | 2.0 / 1.0 | Error handling |
| clap | 4.5 | CLI |
| backoff | 0.4 | Reconnection backoff |

## New Dependencies Required

### Verdict: ZERO new crate dependencies needed

After thorough analysis of all four v1.1 features against the existing dependency tree, **no new crates are required**. Every capability can be built with what is already in `Cargo.toml`. This is the correct approach for a solo-trader system that prizes reliability and minimal attack surface over feature velocity.

Here is why, feature by feature:

---

### 1. Settlement Outcome Tracking

**Need:** Poll venue APIs for settlement results, compare against signal predictions.

**Already have:**
- `reqwest 0.12` -- HTTP client for REST API polling (Deribit `public/get_last_settlements_by_currency`, Kalshi `GET /markets/{ticker}` with `result` field, Polymarket CLOB API market status)
- `serde + serde_json` -- Deserialize settlement responses
- `chrono` -- Parse settlement timestamps, schedule polling
- `tokio::time::interval` -- Periodic polling loop (no cron crate needed; settlements are polled every N minutes, not at cron-expression times)
- `tracing` -- Log settlement match/mismatch events

**How it works:**
- A `SettlementTracker` task runs a `tokio::time::interval` loop (e.g., every 5 minutes)
- Polls each venue's public settlement API via `reqwest`
- Deserializes responses into typed structs with `serde`
- Matches settlement outcomes against stored signal predictions by event_id
- Logs results to JSONL using the existing `BufWriter<File>` + daily rotation pattern (already proven in `SignalLogger` and `TradeLogger`)

**Venue API details (HIGH confidence -- from official documentation):**
- Deribit: `public/get_last_settlements_by_currency` (public, no auth needed, 20 req/s rate limit applies)
- Kalshi: `GET /markets/{ticker}` returns `result` field ("yes"/"no") when settled; `GET /portfolio/settlements` returns settlement history
- Polymarket: Market status via CLOB API `GET /markets/{condition_id}` shows resolution state

**Why no new crates:**
- `tokio::time::interval` is simpler and more reliable than `tokio-cron-scheduler` for fixed-interval polling. Settlement outcomes do not require cron expressions -- they need "check every 5 minutes."
- `reqwest` already handles all HTTP needs. No specialized settlement client exists or is needed.

---

### 2. Signal Analysis Tooling

**Need:** Compute hit rate, edge measurement, false positive rate, time-to-convergence from historical signal + settlement data.

**Already have:**
- `statrs 0.18` -- Statistical distributions (already used for Black-76 pricing). Provides normal distribution for confidence intervals, but signal analysis metrics (hit rate, false positive rate) are simple ratio calculations that need no statistical library.
- `rust_decimal` -- Exact arithmetic for P&L calculations
- `serde + serde_json` -- Read historical JSONL signal logs, write analysis output
- `std::fs` / `std::io::BufReader` -- Read JSONL files line by line
- `chrono` -- Time-to-convergence calculations (signal timestamp vs settlement timestamp)

**Key metrics and how they are computed (no new libraries):**

| Metric | Formula | Dependencies |
|--------|---------|--------------|
| Hit rate | `correct_signals / total_settled_signals` | `rust_decimal` division |
| False positive rate | `signals_that_lost / total_signals` | `rust_decimal` division |
| Average edge | `mean(realized_pnl per signal)` | `rust_decimal` sum/count |
| Edge decay | `signal_edge_at_t0 - edge_at_settlement` | `rust_decimal` subtraction |
| Time-to-convergence | `settlement_ts - signal_ts` | `chrono::Duration` |
| Sharpe proxy | `mean(daily_pnl) / stddev(daily_pnl)` | `statrs` or hand-rolled f64 math |
| Win/loss distribution | histogram of realized P&L buckets | Simple Vec sorting |

**Why no new crates:**
- Signal analysis is arithmetic on JSONL data. The calculations are ratios, means, and standard deviations -- all trivially implemented in ~50 lines of Rust.
- `statrs` is already available for anything requiring distribution functions. There is no "signal analysis" crate in the Rust ecosystem that would add value over custom code for this specific domain.
- The analysis can run as a CLI subcommand (via existing `clap`) that reads JSONL files and outputs a summary, or as an in-process task that computes rolling metrics.

---

### 3. Failure Alerting

**Need:** Detect degraded states (stale data, partial feeds, silent failures) and notify the operator.

**Already have:**
- `VenueHealth` (per-venue atomic health trackers) -- Already tracks connected/disconnected, last_message_at, last_error
- `metrics` -- Already emits `feed_available` gauge per venue, `arb_staleness_rejections` counter
- `tracing` -- Already logs all degraded state transitions
- `reqwest 0.12` -- HTTP client for webhook POST notifications
- `tokio::time::interval` -- Periodic health sweep
- `axum 0.8` -- Already serves `/health` endpoint

**Alert delivery strategy:**

The system should emit alerts via **webhook POST** (to Slack, Discord, Telegram bot, or any HTTP endpoint). This is the standard approach for solo-trader alerting because:
1. The operator already has `reqwest` in the dependency tree
2. Webhooks work with every notification platform (Slack incoming webhooks, Discord webhooks, Telegram Bot API, PagerDuty, etc.)
3. No SMTP configuration complexity (no `lettre` crate needed)
4. A single `async fn send_alert(url: &str, message: &str)` using `reqwest::Client::post(url).json(&payload).send().await` is ~10 lines

**Alert conditions (built on existing infrastructure):**

| Condition | Detection Source | Already Exists? |
|-----------|-----------------|-----------------|
| Feed disconnect | `VenueHealth::is_available()` | YES |
| Stale data (no messages for N seconds) | `VenueHealth::last_message_at()` + duration check | YES (needs sweep loop) |
| All feeds down | All `VenueHealth` instances unavailable | YES (needs aggregation) |
| Silent failure (engine running but no signals for N minutes) | `metrics::counter!("arb_computations_total")` stall detection | Partial (counter exists, needs staleness check) |
| High staleness rejection rate | `metrics::counter!("arb_staleness_rejections")` rate | Partial (counter exists, needs rate windowing) |
| Paper trade position stuck | Position in `Pending` status for > N minutes | Needs implementation |

**Why no `lettre` or `event-notification` crate:**
- Email alerting adds SMTP configuration complexity (server, port, credentials, TLS) with no benefit over webhooks for a solo trader
- `event-notification` is an unnecessary abstraction layer when `reqwest.post(webhook_url).json(&body).send()` does the job in one line
- Webhook URL is a single TOML config parameter -- vastly simpler than email configuration

**Why no `tokio-cron-scheduler`:**
- The health sweep is a simple `tokio::time::interval(Duration::from_secs(30))` loop
- Cron expressions add complexity without benefit for fixed-interval checks
- The existing codebase already uses `tokio::time::interval` for periodic tasks in `CrossAssetEngine` and `PaperTradeTracker`

---

### 4. File-Based State Persistence

**Need:** Persist paper P&L and signal history across restarts. Load state on startup, save periodically.

**Already have:**
- `serde + serde_json` -- Serialize/deserialize state structs
- `std::fs` + `std::io::BufWriter` -- File I/O (already used in `SignalLogger`, `TradeLogger`, `JsonlWriter`)
- `tokio::fs` -- Async file I/O (already used in `JsonlWriter`)
- `chrono` -- Timestamps for state snapshots

**Persistence strategy: Atomic JSON snapshot files**

The correct approach for this system is periodic atomic writes of small JSON state files, NOT a database, NOT append-only logs for state (those are for event logs, which already exist).

**How atomic writes work without new crates:**

```rust
// Write to temp file, then rename (atomic on all filesystems)
use std::fs;
use std::io::Write;

fn save_state(path: &str, state: &impl serde::Serialize) -> anyhow::Result<()> {
    let tmp_path = format!("{}.tmp", path);
    let data = serde_json::to_string_pretty(state)?;
    fs::write(&tmp_path, data.as_bytes())?;
    fs::rename(&tmp_path, path)?; // atomic on same filesystem
    Ok(())
}
```

This is the exact pattern used by every production system for small state files (< 1MB). `std::fs::rename` is atomic on all major filesystems when source and destination are on the same mount point. No `tempfile`, `atomicwrites`, or `atomic-write-file` crate needed.

**State files to persist:**

| File | Contents | Size Estimate | Write Frequency |
|------|----------|---------------|-----------------|
| `state/paper_positions.json` | Open + pending paper trade positions | < 50KB | Every position change or every 60s |
| `state/daily_aggregates.json` | Daily P&L rollups | < 10KB | Every trade settlement or daily |
| `state/signal_outcomes.json` | Signal-to-settlement outcome map | < 100KB (rolling 30 days) | Every settlement match |
| `state/alert_state.json` | Alert cooldown timestamps, last alert sent | < 1KB | Every alert |

**Why no database (SQLite, sled, RocksDB):**
- Total state is < 200KB. A database adds 1-2MB of binary size and operational complexity for zero benefit.
- The system already uses JSONL files for event logs. State files are a natural extension.
- `serde_json` round-trips perfectly with `rust_decimal` (using `#[serde(with = "rust_decimal::serde::str")]` as already done throughout the codebase).
- Restart is rare (days/weeks). Loading 200KB of JSON on startup takes < 1ms.

**Why no `tempfile` crate:**
- `tempfile` provides cross-platform temp file creation with automatic cleanup. For atomic state writes, we need `write-then-rename` on a known path -- `std::fs::write` + `std::fs::rename` is simpler and has no cleanup semantics to manage.
- The codebase is already deployed on Linux (per PROJECT.md: "Single-binary Linux service"). `rename()` is atomic on Linux ext4/xfs/btrfs.

**Why no `fs4` / `fs2` (file locking):**
- Single-process system. There is no concurrent writer. File locking protects against multiple processes writing the same file -- irrelevant here.
- If the operator accidentally runs two instances, the WebSocket connections themselves will conflict (venues rate-limit by IP/key), making file locking moot.

---

## Integration Points with Existing Architecture

### New modules and where they connect:

```
src/
  settlement/          # NEW: Settlement outcome tracking
    mod.rs             # SettlementTracker task
    types.rs           # SettlementOutcome, OutcomeMatch
    poller.rs          # Per-venue settlement polling via reqwest
  analysis/            # NEW: Signal analysis tooling
    mod.rs             # AnalysisEngine or CLI subcommand
    metrics.rs         # Hit rate, edge, FPR calculations
    report.rs          # Summary report generation
  alerting/            # NEW: Failure alerting
    mod.rs             # AlertManager task
    conditions.rs      # Alert condition evaluators
    webhook.rs         # reqwest-based webhook sender
  persistence/         # NEW: File-based state persistence
    mod.rs             # StateManager (load/save)
    types.rs           # Persisted state structs
```

### Channel wiring (extends existing mpsc pattern):

| Source | Channel | Destination |
|--------|---------|-------------|
| `CrossAssetEngine` | `mpsc::Sender<ArbSignal>` | `SettlementTracker` (stores predictions for later comparison) |
| `SettlementTracker` | `mpsc::Sender<OutcomeMatch>` | `AnalysisEngine` (computes hit rate etc.) |
| `VenueHealth` (existing) | Read by | `AlertManager` (periodic sweep) |
| `StateManager` | Called by | `PaperTradeTracker`, `SettlementTracker` (periodic save) |

### Config additions to `SystemConfig` (extends existing TOML):

```toml
[settlement]
poll_interval_secs = 300        # 5 minutes
lookback_days = 30              # How far back to check
deribit_settlement_currency = "BTC"

[alerting]
enabled = true
webhook_url = ""                # Slack/Discord/Telegram webhook URL
sweep_interval_secs = 30        # Health check frequency
stale_feed_threshold_secs = 120 # Alert if no messages for 2 minutes
alert_cooldown_secs = 300       # Don't re-alert same condition for 5 minutes

[persistence]
enabled = true
state_dir = "state"             # Directory for state files
save_interval_secs = 60         # Periodic save frequency
```

All new config sections use `#[serde(default)]` to maintain backward compatibility with existing config files, following the established pattern in `SystemConfig`.

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `std::fs::write` + `rename` for state | `tempfile` crate | Adds dependency for something `std::fs` does in 3 lines. `tempfile` is for temporary files with automatic cleanup -- not what we need. |
| `std::fs::write` + `rename` for state | `atomic-write-file` crate | Same logic. Single-process, known paths, Linux target. The write-rename pattern is trivial. |
| Webhook via `reqwest` | `lettre` (email) | SMTP config is 10x more complex than a webhook URL. Every notification platform supports webhooks. |
| Webhook via `reqwest` | `event-notification` crate | Abstraction layer over what is a single `reqwest.post().json().send()` call. |
| `tokio::time::interval` | `tokio-cron-scheduler` | Fixed intervals are simpler and sufficient. Cron expressions add complexity for zero benefit in this use case. |
| No database | SQLite via `rusqlite` | Total state < 200KB. Database adds binary bloat, operational complexity, migration burden. |
| No database | `sled` embedded DB | `sled` is effectively abandoned (no releases since 2022). Even if it weren't, same objection as SQLite. |
| Hand-rolled signal metrics | `ta-statistics` crate | The metrics are simple ratios. Adding a time-series analysis crate for `wins / total` is over-engineering. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `sled` | Abandoned since 2022, known data corruption issues | `serde_json` state files |
| `bincode` | Abandoned, RUSTSEC-2025-0141, v3.0.0 does not compile | `serde_json` for state (human-readable, debuggable) |
| `rusqlite` / SQLite | Massive overkill for < 200KB of state | `serde_json` state files |
| `lettre` | SMTP complexity for a solo-trader system | Webhook via `reqwest` |
| `tokio-cron-scheduler` | Unnecessary complexity over `tokio::time::interval` | `tokio::time::interval` |
| Any ORM crate | No database, no ORM | Direct `serde_json` serialization |

## Version Compatibility

No new dependencies means no new version compatibility concerns. The existing `Cargo.toml` lockfile remains unchanged for v1.1.

Key constraint: Rust 2024 edition (1.85+) is already specified in `Cargo.toml`. All existing crates support this.

## Cargo.toml Changes

**None required.** The existing dependency set covers all v1.1 needs:

```toml
# EXISTING -- no changes needed for v1.1
tokio = { version = "1", features = ["full"] }          # interval, fs, channels
serde = { version = "1.0", features = ["derive"] }      # state serialization
serde_json = "1.0"                                       # JSON state files
chrono = { version = "0.4", features = ["serde"] }       # timestamps
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }  # webhook + settlement polling
tracing = "0.1"                                          # logging
metrics = "0.24"                                         # alert condition metrics
axum = { version = "0.8", ... }                          # health endpoint
rust_decimal = { version = "1.40", ... }                 # P&L arithmetic
statrs = "0.18"                                          # statistical analysis
clap = { version = "4.5", features = ["derive"] }        # analysis CLI subcommand
uuid = { version = "1", features = ["v7", "serde"] }     # outcome match IDs
thiserror = "2.0"                                        # error types
anyhow = "1.0"                                           # error propagation
```

## Sources

- [Deribit API Documentation](https://docs.deribit.com/) -- Settlement endpoint `public/get_last_settlements_by_currency` (HIGH confidence)
- [Kalshi API Documentation](https://docs.kalshi.com/api-reference/portfolio/get-settlements) -- `GET /portfolio/settlements` endpoint (HIGH confidence)
- [Polymarket Developer Docs](https://docs.polymarket.com/developers/resolution/UMA) -- UMA Oracle resolution mechanism (MEDIUM confidence -- settlement query API still evolving)
- [tempfile crate](https://crates.io/crates/tempfile) -- v3.20.0, evaluated and rejected as unnecessary (HIGH confidence)
- [atomic-write-file crate](https://crates.io/crates/atomic-write-file) -- Evaluated and rejected as unnecessary for single-process use (HIGH confidence)
- [lettre crate](https://crates.io/crates/lettre) -- v0.10+, evaluated and rejected in favor of webhook approach (HIGH confidence)
- [serde_json latest](https://crates.io/crates/serde_json) -- v1.0.149 confirmed active maintenance through 2026 (HIGH confidence)
- Existing codebase analysis: `src/signal/logger.rs`, `src/paper_trade/tracker.rs`, `src/feed/recording/writer.rs`, `src/feed/health.rs`, `src/health/mod.rs` -- Established patterns for JSONL logging, daily rotation, health tracking, periodic tasks (HIGH confidence)

---
*Stack research for: v1.1 Paper Trading Validation*
*Researched: 2026-02-24*
