# Phase 1: Foundation - Context

**Gathered:** 2026-02-21
**Status:** Ready for planning

<domain>
## Phase Boundary

Project skeleton with shared types, configuration, structured logging, and graceful shutdown. This phase establishes the infrastructure every subsequent phase imports — domain types, config loading, logging pipeline, error handling patterns, and clean shutdown behavior. No venue connections, no pricing, no signal generation.

</domain>

<decisions>
## Implementation Decisions

### Types & Conventions
- Newtype wrappers for all numeric domain types: `Probability(Decimal)`, `Price(Decimal)`, `Notional(Decimal)` — compiler prevents mixing them
- Canonical event ID + venue instrument ID: `EventId("BTC-100K-2025-06-30")` maps to venue-specific `InstrumentId` per venue
- Fixed `Venue` enum: `enum Venue { Deribit, Polymarket, Kalshi }` — adding a venue means code changes (acceptable, three venues is the scope)
- Dual timestamp representation: `tokio::time::Instant` for internal latency measurement and staleness checks, `chrono::DateTime<Utc>` for wall clock logging, display, and serialization
- All prices and probabilities use `rust_decimal::Decimal` — never f64

### Config Structure
- Split config files: `config.toml` for system settings, `events.toml` for cross-venue instrument mappings, `venues.toml` for venue-specific settings (not credentials)
- Credentials via environment variables only: `DERIBIT_API_KEY`, `POLYMARKET_PRIVATE_KEY`, etc. — never in config files, never at risk of being committed
- Hot reload for tuning parameters: thresholds, filters, fee assumptions reload on SIGHUP or file watch without restart. Structural changes (new venues, new event categories) require restart.
- Fail fast on invalid config at startup: refuse to start, print exact error with field path and line number. No silent defaults for invalid values.

### Logging & Correlation
- Dual output: human-readable to stdout (minimal in normal operation), structured JSON to rotating log file
- Stdout shows only: signals, errors, and connection state changes. Everything else goes to file.
- Per-event trace ID: each market data event receives a trace ID that follows it through the entire pipeline (normalization → spread calc → signal). Enables end-to-end debugging of any signal.
- Full context per spread computation in log files: both prices, both timestamps, staleness status, fee breakdown, net edge. Disk is cheap; analysis value is high.
- `tracing` crate with JSON subscriber for file output, `tracing-subscriber::fmt` for stdout with filtering

### Error Handling
- `thiserror` for library code (feeds, pricing, events), `anyhow` for binary/orchestration — typed errors where they matter, ergonomic errors in glue code
- Venue API errors categorized by severity:
  - Fatal (auth failure, account locked) → stop the feed, alert operator
  - Degraded (rate limited, partial data) → backoff, continue with reduced capability
  - Transient (timeout, parse error on single message) → retry silently, log at debug level
- Pricing computation failures use fallback methods: Newton-Raphson fails → try Brent's → skip if all methods fail, log the failure with context
- Panic on invariant violations only: `assert!`/`panic!` for "this should be impossible" states (e.g., negative probability after validation). `Result` for all expected errors. `unwrap()` banned in non-test code.

### Claude's Discretion
- Exact module layout within `src/` (file organization, module hierarchy)
- Choice between `clap` vs manual arg parsing for CLI entrypoint
- Specific `tracing-subscriber` filter configuration syntax
- Log rotation strategy (size-based vs time-based)
- Whether to use `config` crate or hand-roll TOML parsing with `toml` + `serde`

</decisions>

<specifics>
## Specific Ideas

- The split config approach mirrors how the system will be operated: events.toml gets edited constantly during paper trading (adding/removing BTC events to track), config.toml rarely changes, venue credentials never touch disk
- Severity-categorized errors should be machine-parseable — a monitoring system should be able to alert on Fatal without parsing free-text
- Per-event trace IDs are critical for post-hoc analysis: "why did this signal fire?" should be answerable by grepping one ID

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-foundation*
*Context gathered: 2026-02-21*
