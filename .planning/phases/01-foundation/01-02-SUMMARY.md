---
phase: 01-foundation
plan: 02
subsystem: config-logging
tags: [toml, serde, tracing, tracing-subscriber, tracing-appender, envfilter, config-validation, credentials]

# Dependency graph
requires:
  - "01-01: ConfigError type, project scaffold with all Phase 1 dependencies"
provides:
  - "Config loading: load_config() -> AppConfig from 3 TOML files + env var credentials"
  - "Config structs: SystemConfig, EventsConfig, VenuesConfig, Credentials"
  - "Cross-field validation with fail-fast error reporting"
  - "Dual-output logging: init_logging() -> WorkerGuard"
  - "Per-layer filtering: stdout (human-readable) + file (JSON, daily-rotating)"
  - "Example config files: config/config.toml, config/events.toml, config/venues.toml"
affects: [01-03, 02-feeds, 03-normalization, 05-event-mapping]

# Tech tracking
tech-stack:
  added: []
  patterns: [fail-fast-config, per-layer-filtering, credential-redaction, generic-toml-loader]

key-files:
  created:
    - src/config/mod.rs
    - src/config/system.rs
    - src/config/events.rs
    - src/config/venues.rs
    - src/config/credentials.rs
    - src/config/validation.rs
    - src/config/reload.rs
    - src/logging/mod.rs
    - src/logging/layers.rs
    - config/config.toml
    - config/events.toml
    - config/venues.toml
  modified:
    - src/lib.rs
    - tests/smoke_test.rs

key-decisions:
  - "load_credentials() returns Credentials directly (not Result) since all fields are optional in Phase 1"
  - "Credentials custom Debug redacts present values with *** to prevent accidental secret exposure"
  - "URL validation uses simple prefix checking (wss:// and https://) rather than full URL parsing"
  - "Filter strings scoped to crate: prediction={level} rather than global filter"

patterns-established:
  - "Generic TOML loader: load_toml<T: DeserializeOwned>(dir, filename) -> Result<T, ConfigError>"
  - "Per-layer filtering: Registry::default().with(layer.with_filter(filter)) for independent output filtering"
  - "Credential loading: env vars only, Option<String> for all fields, custom Debug for redaction"
  - "Cross-field validation: separate validate_config() step after parsing, before returning AppConfig"

# Metrics
duration: 6min
completed: 2026-02-21
---

# Phase 1 Plan 2: Config & Logging Summary

**Three-file TOML config system with fail-fast validation, env-var credential loading with redacted debug, and dual-output tracing (human stdout + JSON daily-rotating file) via per-layer EnvFilter on Registry**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-21T22:45:02Z
- **Completed:** 2026-02-21T22:51:25Z
- **Tasks:** 2
- **Files modified:** 14

## Accomplishments
- Config loading system parses 3 TOML files (system, events, venues) via generic `load_toml<T>` with toml 0.8 line/column error reporting
- Cross-field validation catches zero thresholds, events without venue mappings, and invalid URL schemes
- Credentials loaded from 5 environment variables with custom Debug that redacts secrets
- Dual-output logging: human-readable stdout + structured JSON to daily-rotating file, each with independent EnvFilter via Registry pattern
- 6 new integration tests covering config loading, parse errors, validation rejection, and credential redaction
- Example config files ready for immediate use

## Task Commits

Each task was committed atomically:

1. **Task 1: Configuration loading system with fail-fast validation** - `7e651d1` (feat)
2. **Task 2: Dual-output structured logging with per-layer filtering** - `1632666` (feat)

## Files Created/Modified
- `src/config/mod.rs` - AppConfig struct, load_config(), generic load_toml<T>()
- `src/config/system.rs` - SystemConfig, LoggingConfig, StalenessConfig, SignalConfig
- `src/config/events.rs` - EventsConfig, EventMapping, venue mapping structs
- `src/config/venues.rs` - VenuesConfig, DeribitConfig, PolymarketConfig, KalshiConfig
- `src/config/credentials.rs` - Credentials with env var loading and redacted Debug
- `src/config/validation.rs` - Cross-field validation with descriptive error messages
- `src/config/reload.rs` - Placeholder for Plan 03 hot-reload implementation
- `src/logging/mod.rs` - Module root, re-exports init_logging
- `src/logging/layers.rs` - Dual-output logging with per-layer filtering via Registry
- `config/config.toml` - System settings: logging, staleness, signals
- `config/events.toml` - Example BTC-100K event with all three venue mappings
- `config/venues.toml` - Deribit, Polymarket, Kalshi connection settings
- `src/lib.rs` - Added pub mod config and pub mod logging
- `tests/smoke_test.rs` - Added 6 config/credential tests (22 total)

## Decisions Made
- `load_credentials()` returns `Credentials` directly (not `Result<Credentials, ConfigError>`) because all credential fields are `Option<String>` in Phase 1 -- no venue connections require them yet. The `ConfigError::MissingEnvVar` variant exists for Phase 2+ when specific feeds require their credentials.
- Custom `Debug` on `Credentials` shows "***" for present values, preventing accidental secret exposure in logs or error messages.
- URL validation uses simple prefix checking (`starts_with("wss://")` / `starts_with("https://")`) rather than full URL parsing. This is sufficient for catching obvious misconfiguration without adding a URL parser dependency.
- Logging filter strings are scoped to the crate (`prediction={level}`) rather than using a global filter, ensuring third-party crate logs don't pollute output.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Config system available via `prediction::config::load_config()`
- Logging available via `prediction::logging::init_logging()`
- Plan 03 (shutdown + main.rs wiring) can now wire config + logging + CancellationToken
- `config/reload.rs` placeholder ready for Plan 03 hot-reload implementation
- Example config files ready for use during development

## Self-Check: PASSED

All 14 files verified present. Both task commits (7e651d1, 1632666) verified in git log.

---
*Phase: 01-foundation*
*Completed: 2026-02-21*
