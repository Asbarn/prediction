---
phase: 01-foundation
plan: 03
subsystem: entrypoint
tags: [clap, tokio, cancellation-token, notify, watch-channel, graceful-shutdown, config-hot-reload, cli]

# Dependency graph
requires:
  - "01-01: Domain types (Venue, Price, Probability, etc.) and error types (ConfigError, VenueError)"
  - "01-02: Config loading (load_config, AppConfig) and logging (init_logging, WorkerGuard)"
provides:
  - "Binary entrypoint: cargo run starts system with config, logging, shutdown, and hot-reload"
  - "CLI: clap derive with run (default) and check-config subcommands"
  - "Graceful shutdown: CancellationToken cancelled on Ctrl+C (all platforms) and SIGTERM (Unix)"
  - "Config hot-reload: notify file watcher with watch channel distribution"
  - "Integration test suite: 16 tests covering type safety, errors, and config contracts"
affects: [02-feeds, 03-normalization, 05-event-mapping, 07-pricing, 08-signals]

# Tech tracking
tech-stack:
  added: []
  patterns: [cancellation-token-shutdown, file-watch-hot-reload, clap-derive-cli, watch-channel-config-distribution]

key-files:
  created:
    - src/shutdown.rs
    - tests/integration.rs
  modified:
    - src/main.rs
    - src/config/reload.rs
    - src/lib.rs
    - .gitignore
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "ConfigReloader::start returns (ConfigReloader, Receiver) instead of (Sender, Receiver) since Sender is moved into watcher thread"
  - "Upgraded notify-debouncer-mini from 0.5 to 0.7 to resolve notify version conflict (0.5 depends on notify 7, project uses notify 8)"
  - "Config hot-reload uses dedicated OS thread (std::thread::spawn) not tokio::spawn, because notify uses blocking OS APIs"
  - "Shutdown handler does NOT handle SIGHUP -- config reload is file-watcher-only for cross-platform consistency"

patterns-established:
  - "CancellationToken: root token in main(), distributed to subsystems via clone, shutdown_signal task cancels on OS signal"
  - "Config distribution: watch::channel with latest-value semantics, consumers snapshot once per processing cycle"
  - "CLI routing: clap derive with Optional<Commands> defaulting to Run when no subcommand given"
  - "Log guard lifetime: _log_guard held in main() scope, drops last to flush remaining logs"

# Metrics
duration: 9min
completed: 2026-02-22
---

# Phase 1 Plan 3: Binary Entrypoint & Integration Summary

**Clap CLI with graceful CancellationToken shutdown, notify-based config hot-reload via watch channel, and 16 integration tests completing the Phase 1 foundation**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-21T22:55:48Z
- **Completed:** 2026-02-21T23:04:55Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Complete binary entrypoint: `cargo run` starts with config loading, dual-output logging, signal handling, and config hot-reload
- Clap CLI with `run` (default) and `check-config` subcommands, `--config-dir` flag with `config/` default
- Cross-platform graceful shutdown via CancellationToken: Ctrl+C on all platforms, SIGTERM on Unix
- Config hot-reload with notify file watcher on dedicated OS thread, 500ms debounce, watch channel distribution
- 16 new integration tests validating type safety, error severity, config loading, and config validation
- All 38 tests pass (16 integration + 22 smoke) with zero warnings
- All 5 Phase 1 success criteria from ROADMAP.md are satisfied

## Task Commits

Each task was committed atomically:

1. **Task 1: Graceful shutdown, config hot-reload, and binary entrypoint** - `9cc5450` (feat)
2. **Task 2: Integration smoke tests** - `0f7c2b5` (test)

## Files Created/Modified
- `src/main.rs` - Complete binary entrypoint with CLI parsing, config, logging, shutdown, hot-reload
- `src/shutdown.rs` - Cross-platform signal handling with CancellationToken
- `src/config/reload.rs` - Config hot-reload via notify file watcher and watch channel
- `src/lib.rs` - Added `pub mod shutdown` declaration
- `tests/integration.rs` - 16 integration tests for Phase 1 contracts
- `.gitignore` - Added /logs, *.log ignore rules
- `Cargo.toml` - Upgraded notify-debouncer-mini from 0.5 to 0.7
- `Cargo.lock` - Updated dependency tree

## Decisions Made
- `ConfigReloader::start()` returns `(ConfigReloader, watch::Receiver<AppConfig>)` instead of `(watch::Sender, watch::Receiver)` because the Sender must be moved into the watcher thread. The ConfigReloader struct holds an internal Receiver to keep the channel alive.
- Upgraded `notify-debouncer-mini` from 0.5 to 0.7 because 0.5 depends on `notify` 7.x while the project uses `notify` 8.x, causing two different versions of the same crate (types incompatible).
- Shutdown handler only handles Ctrl+C (all platforms) and SIGTERM (Unix). SIGHUP is NOT handled -- config reload is solely via file watcher for cross-platform consistency, per research recommendation.
- The `_log_guard` is the last thing dropped in main(), ensuring the "shutdown complete" log message is flushed to the file layer.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Upgraded notify-debouncer-mini from 0.5 to 0.7**
- **Found during:** Task 1 (config hot-reload implementation)
- **Issue:** `notify-debouncer-mini` 0.5 depends on `notify` 7.x, but Cargo.toml specifies `notify` 8.x. This caused two versions of the `notify` crate to coexist, making `notify::RecursiveMode` from 8.x incompatible with the debouncer's `watch()` method expecting 7.x types.
- **Fix:** Updated Cargo.toml to `notify-debouncer-mini = "0.7"` which depends on `notify` 8.x, resolving the version conflict.
- **Files modified:** Cargo.toml, Cargo.lock
- **Verification:** `cargo build` compiles without type mismatch errors
- **Committed in:** 9cc5450 (Task 1 commit)

**2. [Rule 3 - Blocking] Fixed notify 8.x Error type (not iterable)**
- **Found during:** Task 1 (config hot-reload implementation)
- **Issue:** The research code example used `for e in &errors` on the error branch, but `notify` 8.x `Error` is a single struct, not iterable like in older versions.
- **Fix:** Changed `for e in &errors { warn }` to `warn(error)` treating it as a single error value.
- **Files modified:** src/config/reload.rs
- **Verification:** `cargo build` compiles clean
- **Committed in:** 9cc5450 (Task 1 commit)

**3. [Rule 1 - Bug] Fixed ConfigReloader API to match watch::Sender move semantics**
- **Found during:** Task 1 (config hot-reload implementation)
- **Issue:** The plan specified returning `(watch::Sender, watch::Receiver)` but `watch::Sender` is not Clone, and the Sender must be moved into the watcher thread. Cannot return what was moved.
- **Fix:** Changed return type to `(ConfigReloader, watch::Receiver)` where ConfigReloader holds an internal Receiver to keep the channel alive. Updated main.rs to hold `_config_reloader` instead of `_config_tx`.
- **Files modified:** src/config/reload.rs, src/main.rs
- **Verification:** `cargo build` compiles, hot-reload works at runtime
- **Committed in:** 9cc5450 (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (2 blocking, 1 bug)
**Impact on plan:** All fixes were necessary for compilation and correct API design. No scope creep. The notify version mismatch was a pre-existing issue in the dependency spec from research.

## Issues Encountered
None beyond the auto-fixed deviations documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 1 foundation is complete: all success criteria met
- Binary starts, loads config, logs structured JSON, handles signals, reloads config on file changes
- Ready for Phase 2 (Deribit WebSocket feed) which will:
  - Import `CancellationToken` pattern for feed lifecycle
  - Use `watch::Receiver<AppConfig>` for runtime config access
  - Write market data to `MarketSnapshot` structs
  - Log via the established tracing infrastructure
- All domain types, error types, config, and logging are stable public APIs

## Self-Check: PASSED

All 9 files verified present. Both task commits (9cc5450, 0f7c2b5) verified in git log.

---
*Phase: 01-foundation*
*Completed: 2026-02-22*
