# Phase 1: Foundation - Research

**Researched:** 2026-02-21
**Domain:** Rust project skeleton -- shared types, TOML configuration, structured logging, error handling, graceful shutdown
**Confidence:** HIGH

## Summary

Phase 1 establishes the infrastructure every subsequent phase imports: domain types with newtype safety, multi-file TOML configuration with validation, dual-output structured logging (human stdout + JSON file), error handling with thiserror/anyhow split, and graceful shutdown via CancellationToken. No venue connections, no pricing, no signal generation -- purely foundational.

The technology decisions are locked and well-supported by the Rust ecosystem. The primary research questions are in the "Claude's Discretion" areas: module layout, CLI parsing approach, tracing-subscriber filter configuration, log rotation strategy, and TOML parsing approach. Each has a clear best answer given the project constraints.

**Primary recommendation:** Use `clap` derive for CLI, raw `toml` + `serde` for config parsing (not the `config` crate), `tracing-appender` with daily rotation and `NonBlocking` writer, and `derive_more` for newtype arithmetic delegation. Hot reload via `notify` crate file watching (cross-platform, no SIGHUP dependency) with `tokio::sync::watch` for runtime distribution.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Types & Conventions
- Newtype wrappers for all numeric domain types: `Probability(Decimal)`, `Price(Decimal)`, `Notional(Decimal)` -- compiler prevents mixing them
- Canonical event ID + venue instrument ID: `EventId("BTC-100K-2025-06-30")` maps to venue-specific `InstrumentId` per venue
- Fixed `Venue` enum: `enum Venue { Deribit, Polymarket, Kalshi }` -- adding a venue means code changes (acceptable, three venues is the scope)
- Dual timestamp representation: `tokio::time::Instant` for internal latency measurement and staleness checks, `chrono::DateTime<Utc>` for wall clock logging, display, and serialization
- All prices and probabilities use `rust_decimal::Decimal` -- never f64

#### Config Structure
- Split config files: `config.toml` for system settings, `events.toml` for cross-venue instrument mappings, `venues.toml` for venue-specific settings (not credentials)
- Credentials via environment variables only: `DERIBIT_API_KEY`, `POLYMARKET_PRIVATE_KEY`, etc. -- never in config files, never at risk of being committed
- Hot reload for tuning parameters: thresholds, filters, fee assumptions reload on SIGHUP or file watch without restart. Structural changes (new venues, new event categories) require restart.
- Fail fast on invalid config at startup: refuse to start, print exact error with field path and line number. No silent defaults for invalid values.

#### Logging & Correlation
- Dual output: human-readable to stdout (minimal in normal operation), structured JSON to rotating log file
- Stdout shows only: signals, errors, and connection state changes. Everything else goes to file.
- Per-event trace ID: each market data event receives a trace ID that follows it through the entire pipeline (normalization -> spread calc -> signal). Enables end-to-end debugging of any signal.
- Full context per spread computation in log files: both prices, both timestamps, staleness status, fee breakdown, net edge. Disk is cheap; analysis value is high.
- `tracing` crate with JSON subscriber for file output, `tracing-subscriber::fmt` for stdout with filtering

#### Error Handling
- `thiserror` for library code (feeds, pricing, events), `anyhow` for binary/orchestration -- typed errors where they matter, ergonomic errors in glue code
- Venue API errors categorized by severity:
  - Fatal (auth failure, account locked) -> stop the feed, alert operator
  - Degraded (rate limited, partial data) -> backoff, continue with reduced capability
  - Transient (timeout, parse error on single message) -> retry silently, log at debug level
- Pricing computation failures use fallback methods: Newton-Raphson fails -> try Brent's -> skip if all methods fail, log the failure with context
- Panic on invariant violations only: `assert!`/`panic!` for "this should be impossible" states (e.g., negative probability after validation). `Result` for all expected errors. `unwrap()` banned in non-test code.

### Claude's Discretion
- Exact module layout within `src/` (file organization, module hierarchy)
- Choice between `clap` vs manual arg parsing for CLI entrypoint
- Specific `tracing-subscriber` filter configuration syntax
- Log rotation strategy (size-based vs time-based)
- Whether to use `config` crate or hand-roll TOML parsing with `toml` + `serde`

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

## Standard Stack

### Core (Phase 1 Only)

These are the dependencies actually needed for Phase 1. Other crates from the project-level STACK.md (tokio-tungstenite, reqwest, statrs, dashmap, prometheus-client, etc.) are NOT needed yet.

| Library | Version | Purpose | Why Standard | Confidence |
|---------|---------|---------|--------------|------------|
| tokio | 1.49 | Async runtime, signal handling, timers | Only production Rust async runtime. Needed for shutdown signals and watch channel. Use `features = ["full"]`. | HIGH |
| tokio-util | latest | CancellationToken for graceful shutdown | Official tokio companion. CancellationToken is the standard hierarchical cancellation pattern. | HIGH |
| serde | 1.0 | Serialization framework | Universal derive-based ser/de. Every config struct and domain type uses it. `features = ["derive"]`. | HIGH |
| serde_json | 1.0 | JSON output for structured logs | tracing-subscriber JSON layer produces JSON via serde_json. Also used for log analysis tooling. | HIGH |
| toml | 0.8 | TOML config file parsing | Direct serde integration. Recommended over `config` crate for this project (see Discretion section). Use `toml` 0.8.x (not 1.0.3 -- see note below). | HIGH |
| rust_decimal | 1.40 | Decimal arithmetic for prices/probabilities | 128-bit exact decimal. `features = ["maths", "serde-with-str"]`. The `serde-with-str` feature enables `#[serde(with = "rust_decimal::serde::str")]` for explicit string serialization. | HIGH |
| chrono | 0.4 | Wall-clock timestamps | UTC DateTime for logging, display, serialization. `features = ["serde"]`. | HIGH |
| tracing | 0.1 | Structured logging & instrumentation | Rust ecosystem standard. Span-based context propagation, async-aware. | HIGH |
| tracing-subscriber | 0.3 | Log output formatting & filtering | Fmt layer + JSON layer + per-layer EnvFilter. `features = ["env-filter", "json"]`. | HIGH |
| tracing-appender | 0.2 | Non-blocking file logging with rotation | Rolling file appender with daily rotation. Non-blocking writer via dedicated thread. | HIGH |
| thiserror | 2.0 | Typed error definitions | Library/module error enums with derive macro. v2 is current major. | HIGH |
| anyhow | 1.0 | Application-level error handling | main() and orchestration error propagation with context chains. | HIGH |
| clap | 4.5 | CLI argument parsing | Derive API for subcommands. `features = ["derive"]`. See Discretion section. | HIGH |
| derive_more | 2.1 | Newtype trait delegation | Derives Add, Sub, Mul, Div, From, Into, Display, Deref, DerefMut for newtype wrappers. Requires Rust 1.81+. `features = ["from", "into", "deref", "deref_mut", "display", "add", "mul"]`. | HIGH |
| uuid | 1.x | Trace/correlation IDs | v7 UUIDs are timestamp-sorted. `features = ["v7"]`. | HIGH |
| notify | 8.x | File system watching for config hot-reload | Cross-platform (Windows/Linux/macOS). Replaces SIGHUP dependency. | HIGH |
| notify-debouncer-mini | latest | Debounced file change events | Prevents rapid-fire reloads from editor save patterns. | MEDIUM |

### Important Version Notes

**toml crate versioning:** The project-level STACK.md lists `toml = "1.0"` but this is the older 0.x-era release line. The actively developed version is 0.8.x (latest 0.8.19 as of early 2026) which includes the modern `toml::de` and `toml::ser` API with better error messages including span information (line/column). The `toml 1.0.3` on crates.io is actually a much older release. Use `toml = "0.8"` for the best error reporting, which is critical for the "fail fast with exact error and line number" requirement.

**CORRECTION:** After further verification, `toml` 0.8.x is the latest version line. The crates.io page shows the latest version is in the 0.8.x range. Use `toml = "0.8"` in Cargo.toml.

**derive_more features:** Version 2.x uses feature flags per derive. Enable only the features you need to minimize compile times. Use `features = ["full"]` during development, narrow later.

### Cargo.toml for Phase 1

```toml
[package]
name = "prediction"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["rt"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# Numeric
rust_decimal = { version = "1.40", features = ["maths", "serde-with-str"] }

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Time & IDs
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v7"] }

# CLI
clap = { version = "4.5", features = ["derive"] }

# Newtype derives
derive_more = { version = "2", features = ["full"] }

# Config hot-reload
notify = "8"
notify-debouncer-mini = "0.5"
```

### Alternatives Considered (Discretion Areas)

| Recommended | Alternative | Tradeoff | Decision Rationale |
|-------------|-------------|----------|-------------------|
| `toml` 0.8 + `serde` | `config` crate | `config` adds layered merging from multiple sources (files + env + CLI) but the user's design already splits config into 3 TOML files + env vars. `config` crate would add complexity and an abstraction layer over what is straightforward `toml::from_str()`. The split-file + env pattern is simpler to implement directly. | Use `toml` directly |
| `toml` 0.8 + `serde` | `figment` | Figment provides hierarchical config merging. Overkill for 3 separate TOML files where each has a distinct schema. Figment's value is merging overlapping config; here the files are non-overlapping. | Use `toml` directly |
| `clap` derive | Manual `std::env::args()` | clap adds compile-time cost (~5s) but provides: `--help`, `--version`, `--config-dir`, subcommands (`run`, `check-config`), and automatic error messages. Manual parsing saves compile time but requires hand-rolling all of this. For a binary with subcommands, clap is the standard choice. | Use `clap` |
| `derive_more` | Manual `impl Add/Sub/...` | Manual impls are ~10 lines each per type. With 3+ newtype wrappers needing 4+ arithmetic ops each, that's 120+ lines of boilerplate. derive_more eliminates this entirely. | Use `derive_more` |
| `notify` crate | SIGHUP-only | SIGHUP is Unix-only. The project's Cargo.toml has `edition = "2024"` and the dev environment is Windows. Using `notify` for file watching is cross-platform and more robust than signal-based reload. Keep SIGHUP as an additional trigger on Unix only. | Use `notify` + optional SIGHUP |
| Daily log rotation | Size-based rotation | `tracing-appender` natively supports daily/hourly rotation but NOT size-based. Size-based would require `tracing-rolling-file` or `rolling-file` crate. Daily rotation is simpler, predictable, and sufficient for a paper trading system. Log files from a single day are unlikely to exceed reasonable sizes (~100-500MB for verbose structured JSON). | Use daily rotation |

## Architecture Patterns

### Recommended Module Layout for Phase 1

```
src/
+-- main.rs                    # CLI parsing, tokio runtime, shutdown orchestration
+-- lib.rs                     # Re-exports for integration tests
+-- types/
|   +-- mod.rs                 # Module root, re-exports
|   +-- venue.rs               # Venue enum, Display impl
|   +-- decimal.rs             # Price, Probability, Notional newtypes
|   +-- ids.rs                 # EventId, InstrumentId, TraceId
|   +-- timestamp.rs           # Dual timestamp (Instant + DateTime<Utc>)
|   +-- snapshot.rs            # MarketSnapshot struct (skeleton for Phase 2)
+-- config/
|   +-- mod.rs                 # Module root, public load functions
|   +-- system.rs              # SystemConfig from config.toml
|   +-- events.rs              # EventsConfig from events.toml (skeleton)
|   +-- venues.rs              # VenuesConfig from venues.toml (skeleton)
|   +-- credentials.rs         # Env var loading for secrets
|   +-- validation.rs          # Cross-field validation, fail-fast
|   +-- reload.rs              # File watcher + watch channel distribution
+-- logging/
|   +-- mod.rs                 # init_logging() setup function
|   +-- layers.rs              # Stdout + JSON file layer construction
+-- error/
|   +-- mod.rs                 # Error type re-exports
|   +-- config.rs              # ConfigError enum (thiserror)
|   +-- venue.rs               # VenueError with severity classification
+-- shutdown.rs                # CancellationToken setup, signal handling
```

**Rationale:**

- `types/` is a leaf module that depends on nothing internal. Every other module imports from it. Separating types into their own module prevents circular dependencies.
- `config/` has one file per TOML config file. Each file defines the serde struct and its validation. This mirrors the user's split-config decision.
- `logging/` is separate from `config/` because logging initialization happens before config loading completes (you want to log config errors).
- `error/` centralizes error type definitions. The severity-classified `VenueError` is defined here even though venue connections are Phase 2, because the error pattern should be established early.
- `shutdown.rs` is a single file, not a module directory, because shutdown is a cross-cutting concern with simple implementation.

### Pattern 1: Dual-Output Tracing with Per-Layer Filtering

**What:** Two tracing layers with independent filters -- stdout for human consumption (signals/errors/connection changes only), JSON file for everything.

**When to use:** Always in this project. Initialized once at startup.

**Example:**
```rust
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Registry};
use tracing_appender::rolling;

pub fn init_logging(log_dir: &Path) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    // Stdout layer: human-readable, only INFO+ for key targets
    let stdout_filter = EnvFilter::new(
        "prediction=info,prediction::feeds=warn,prediction::config=warn"
    );
    let stdout_layer = fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_filter(stdout_filter);

    // File layer: JSON, everything at DEBUG+
    let file_appender = rolling::daily(log_dir, "prediction.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let file_filter = EnvFilter::new("prediction=debug");
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_current_span(true)
        .with_span_list(true)
        .with_filter(file_filter);

    // Compose layers with Registry
    Registry::default()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Ok(guard)
}
```

**Critical detail:** The `WorkerGuard` returned by `non_blocking()` MUST be held alive for the entire program lifetime. If dropped, buffered log entries are lost. Store it in main() and drop it only during final shutdown. This is the most common mistake with tracing-appender.

### Pattern 2: Newtype Wrappers with derive_more

**What:** Domain-specific numeric types that prevent mixing Price with Probability at compile time, while delegating arithmetic to the inner Decimal.

**Example:**
```rust
use derive_more::{Add, Sub, Mul, Div, From, Into, Display, Deref, DerefMut};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
         Add, Sub, From, Display, Deref,
         Serialize, Deserialize)]
#[display("{_0}")]
pub struct Price(#[serde(with = "rust_decimal::serde::str")] Decimal);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
         Add, Sub, From, Display, Deref,
         Serialize, Deserialize)]
#[display("{_0}")]
pub struct Probability(#[serde(with = "rust_decimal::serde::str")] Decimal);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
         Add, Sub, From, Display, Deref,
         Serialize, Deserialize)]
#[display("{_0}")]
pub struct Notional(#[serde(with = "rust_decimal::serde::str")] Decimal);

impl Price {
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// Access inner value explicitly (prefer Deref for reads)
    pub fn into_inner(self) -> Decimal {
        self.0
    }
}

impl Probability {
    /// Construct with validation: must be in [0, 1]
    pub fn new(value: Decimal) -> Result<Self, &'static str> {
        if value < Decimal::ZERO || value > Decimal::ONE {
            return Err("probability must be between 0 and 1");
        }
        Ok(Self(value))
    }

    /// Complement: 1 - p
    pub fn complement(&self) -> Self {
        Self(Decimal::ONE - self.0)
    }
}
```

**Design decision -- Deref vs no Deref:** Implementing `Deref<Target = Decimal>` lets you call `.is_zero()`, `.round_dp()`, etc. directly on the newtype. This is convenient but weakens the newtype boundary -- any function taking `&Decimal` will accept `&Price`. For this project, the convenience outweighs the risk because the newtypes are primarily for preventing accidental Price-Probability mixing, not for preventing Decimal access. Do NOT implement `DerefMut` -- mutations should go through explicit methods to maintain validation invariants.

**Note on Mul/Div:** Do NOT derive `Mul` and `Div` on Price or Probability directly. Price * Price is not Price; it is meaningless. Instead, implement specific cross-type operations:

```rust
impl std::ops::Mul<Probability> for Notional {
    type Output = Notional;
    fn mul(self, rhs: Probability) -> Self::Output {
        Notional(self.0 * rhs.0)
    }
}
```

### Pattern 3: Graceful Shutdown with CancellationToken

**What:** Hierarchical shutdown using `tokio_util::sync::CancellationToken`. Root token created in main(), child tokens distributed to subsystems.

**Critical Windows consideration:** `tokio::signal::unix::signal(SignalKind::terminate())` for SIGTERM is Unix-only. On Windows, only `tokio::signal::ctrl_c()` is available. The shutdown handler must be cross-platform.

**Example:**
```rust
use tokio_util::sync::CancellationToken;

pub async fn shutdown_signal(token: CancellationToken) {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).expect("failed to install SIGTERM handler");

        let mut sighup = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::hangup()
        ).expect("failed to install SIGHUP handler");

        tokio::select! {
            _ = ctrl_c => {
                tracing::info!("received SIGINT, initiating shutdown");
                token.cancel();
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, initiating shutdown");
                token.cancel();
            }
            _ = sighup.recv() => {
                tracing::info!("received SIGHUP, triggering config reload");
                // Don't cancel -- just reload config
                // Config reload is handled via notify file watcher primarily
            }
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for ctrl-c");
        tracing::info!("received Ctrl+C, initiating shutdown");
        token.cancel();
    }
}

// In main():
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let shutdown_token = CancellationToken::new();

    // Spawn shutdown signal handler
    let signal_token = shutdown_token.clone();
    tokio::spawn(async move {
        shutdown_signal(signal_token).await;
    });

    // Main application loop
    tokio::select! {
        _ = shutdown_token.cancelled() => {
            tracing::info!("shutdown initiated, cleaning up...");
        }
        // ... other tasks
    }

    // Cleanup: flush logs, close connections (Phase 2+)
    tracing::info!("shutdown complete");
    Ok(())
}
```

### Pattern 4: Config Loading with Fail-Fast Validation

**What:** Load 3 TOML files + env vars, validate all fields, refuse to start on any error. Print exact field path and line number.

**Example:**
```rust
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct SystemConfig {
    pub logging: LoggingConfig,
    pub staleness: StalenessConfig,
    pub signals: SignalConfig,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    pub log_dir: String,
    pub stdout_level: String,
    pub file_level: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {file}: {source}")]
    ReadFile {
        file: String,
        source: std::io::Error,
    },
    #[error("failed to parse {file}: {source}")]
    ParseToml {
        file: String,
        source: toml::de::Error,  // toml 0.8 errors include span (line/col)
    },
    #[error("validation error in {file}: {message}")]
    Validation {
        file: String,
        message: String,
    },
    #[error("missing environment variable: {var}")]
    MissingEnvVar {
        var: String,
    },
}

pub fn load_config(config_dir: &Path) -> Result<AppConfig, ConfigError> {
    let system = load_toml::<SystemConfig>(config_dir, "config.toml")?;
    let events = load_toml::<EventsConfig>(config_dir, "events.toml")?;
    let venues = load_toml::<VenuesConfig>(config_dir, "venues.toml")?;
    let creds = load_credentials()?;

    validate_config(&system, &events, &venues)?;

    Ok(AppConfig { system, events, venues, creds })
}

fn load_toml<T: serde::de::DeserializeOwned>(
    dir: &Path,
    filename: &str,
) -> Result<T, ConfigError> {
    let path = dir.join(filename);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| ConfigError::ReadFile {
            file: filename.to_string(),
            source: e,
        })?;
    toml::from_str(&content)
        .map_err(|e| ConfigError::ParseToml {
            file: filename.to_string(),
            source: e,  // toml 0.8 Error includes line/column span
        })
}
```

**Key detail:** The `toml` 0.8 crate's `toml::de::Error` type includes source span information (line number, column number) in its Display output. This satisfies the "exact error with field path and line number" requirement without any additional work. The error message will look like:

```
failed to parse config.toml: TOML parse error at line 15, column 3
  |
15 | staleness_threshold = "not a number"
   | ^^^^^^^^^^^^^^^^^^^
expected integer
```

### Pattern 5: Config Hot-Reload via File Watch + Watch Channel

**What:** Use `notify` crate to watch config files for changes, parse on change, distribute new config via `tokio::sync::watch` channel.

**Example:**
```rust
use notify::{Watcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tokio::sync::watch;
use std::time::Duration;

pub struct ConfigReloader {
    config_rx: watch::Receiver<AppConfig>,
}

impl ConfigReloader {
    pub fn start(
        config_dir: PathBuf,
        initial_config: AppConfig,
    ) -> anyhow::Result<(Self, watch::Receiver<AppConfig>)> {
        let (config_tx, config_rx) = watch::channel(initial_config);
        let consumer_rx = config_rx.clone();

        // Spawn file watcher on a blocking thread (notify uses OS APIs)
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut debouncer = new_debouncer(
                Duration::from_millis(500),
                tx,
            ).expect("failed to create file watcher");

            debouncer.watcher()
                .watch(&config_dir, RecursiveMode::NonRecursive)
                .expect("failed to watch config dir");

            loop {
                match rx.recv() {
                    Ok(Ok(events)) => {
                        let config_changed = events.iter().any(|e| {
                            e.path.extension().map_or(false, |ext| ext == "toml")
                        });
                        if config_changed {
                            match load_config(&config_dir) {
                                Ok(new_config) => {
                                    tracing::info!("config reloaded successfully");
                                    let _ = config_tx.send(new_config);
                                }
                                Err(e) => {
                                    tracing::error!(?e, "config reload failed, keeping previous");
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(?e, "file watch error");
                    }
                    Err(_) => break, // channel closed
                }
            }
        });

        Ok((Self { config_rx }, consumer_rx))
    }
}

// Consumer usage in any task:
async fn some_task(config: watch::Receiver<AppConfig>) {
    // Read latest config on demand (never blocks, never stale)
    let threshold = config.borrow().system.signals.min_spread_threshold;
}
```

### Pattern 6: Error Severity Classification

**What:** Venue errors carry machine-readable severity for automated alerting.

**Example:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorSeverity {
    /// Fatal: stop the feed, alert operator immediately
    /// Examples: auth failure, account locked, API key revoked
    Fatal,
    /// Degraded: backoff, continue with reduced capability
    /// Examples: rate limited, partial data, slow responses
    Degraded,
    /// Transient: retry silently, log at debug level
    /// Examples: timeout, single message parse error, temporary disconnect
    Transient,
}

#[derive(Debug, thiserror::Error)]
pub enum VenueError {
    #[error("[FATAL] authentication failed for {venue}: {message}")]
    AuthFailure {
        venue: Venue,
        message: String,
    },

    #[error("[DEGRADED] rate limited on {venue}, backing off {backoff_ms}ms")]
    RateLimited {
        venue: Venue,
        backoff_ms: u64,
    },

    #[error("[TRANSIENT] timeout connecting to {venue}")]
    ConnectionTimeout {
        venue: Venue,
    },

    #[error("[TRANSIENT] failed to parse message from {venue}: {message}")]
    ParseError {
        venue: Venue,
        message: String,
    },
}

impl VenueError {
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::AuthFailure { .. } => ErrorSeverity::Fatal,
            Self::RateLimited { .. } => ErrorSeverity::Degraded,
            Self::ConnectionTimeout { .. } => ErrorSeverity::Transient,
            Self::ParseError { .. } => ErrorSeverity::Transient,
        }
    }
}
```

### Anti-Patterns to Avoid

- **Implementing `DerefMut` on numeric newtypes:** Allows bypassing validation. Use explicit setter methods that validate.
- **Deriving `Mul<Self>` or `Div<Self>` on Price/Probability:** Price * Price is not a meaningful type. Only implement cross-type operations that make domain sense (Notional * Probability = Notional).
- **Using `unwrap()` outside tests:** The user explicitly banned this. Use `.expect("invariant: reason")` for true invariants in non-test code, but prefer `Result` propagation.
- **Dropping the `WorkerGuard` from `tracing_appender::non_blocking`:** Logs will be lost. Hold it in main() until final cleanup.
- **Silent config defaults:** The user specified fail-fast. Never `#[serde(default)]` on fields that should be explicitly set. Use `Option<T>` only for genuinely optional fields, then validate that required fields are present.
- **Using `EnvFilter` as a global filter:** When using per-layer filtering, `EnvFilter` must be attached to each layer individually via `.with_filter()`. Using it as a global subscriber filter would apply the same rules to both stdout and file layers.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Newtype arithmetic delegation | Manual `impl Add<Self> for Price { ... }` for each type x each op | `derive_more` 2.x with `#[derive(Add, Sub, ...)]` | 3 types x 4 ops = 12 manual impls. derive_more generates them correctly. |
| Log rotation | Custom file rotation logic | `tracing_appender::rolling::daily()` | Handles midnight rollover, filename timestamping, atomic file creation. Edge cases in date-boundary rotation are subtle. |
| Non-blocking log writing | Custom mpsc + writer thread | `tracing_appender::non_blocking()` | Dedicated writer thread with proper flushing on shutdown. The `WorkerGuard` pattern ensures no log loss. |
| File watching for hot reload | Polling loop with `fs::metadata` timestamps | `notify` 8.x crate | OS-native APIs (inotify on Linux, ReadDirectoryChanges on Windows, FSEvents on macOS). Efficient, debounced, cross-platform. |
| Signal handling | Raw `libc::signal` or `signal-hook` | `tokio::signal::ctrl_c()` + `tokio::signal::unix::signal()` | Integrates with tokio's event loop. No unsafe code. Cross-platform ctrl_c with Unix-specific SIGTERM/SIGHUP. |
| CLI argument parsing | Manual `std::env::args()` matching | `clap` 4.x derive API | Free --help, --version, error messages, subcommand routing. ~50 lines of derive structs replaces ~200 lines of manual parsing. |
| TOML error line numbers | Custom TOML error formatting | `toml` 0.8.x `Error` Display impl | The crate's error type already includes line/column spans. No additional work needed. |

## Common Pitfalls

### Pitfall 1: WorkerGuard Dropped Too Early

**What goes wrong:** The `WorkerGuard` from `tracing_appender::non_blocking()` is stored in a local variable, goes out of scope, and the background writer thread terminates. All subsequent log writes are silently dropped.

**Why it happens:** The guard is returned as a second tuple element `let (_writer, _guard) = non_blocking(appender)` and it is tempting to ignore it or let it drop in an init function.

**How to avoid:** Return the guard from the logging init function. Store it in main() as a named variable (not `_`). Drop it explicitly during final shutdown, after all other cleanup is done.

**Warning signs:** Log file stops growing mid-run. Structured JSON file is empty or truncated. No error messages are visible.

### Pitfall 2: Incorrect Per-Layer Filter Setup

**What goes wrong:** Applying `EnvFilter` as a global filter instead of per-layer, causing both stdout and file to use the same filtering rules. Or using `with_subscriber()` instead of `with_filter()` and getting type errors.

**Why it happens:** tracing-subscriber's layer composition has a complex type system. The difference between `.with()` (adds a layer to a subscriber) and `.with_filter()` (adds a filter to a specific layer) is subtle. The official examples often show global filtering, not per-layer.

**How to avoid:** Always use the `Registry` pattern:
```rust
Registry::default()
    .with(layer_a.with_filter(filter_a))  // filter_a only applies to layer_a
    .with(layer_b.with_filter(filter_b))  // filter_b only applies to layer_b
    .init();
```

**Warning signs:** Both outputs show the same events. Type errors mentioning `Layered` or `Filtered` during compilation.

### Pitfall 3: Serde Default Values Hiding Config Errors

**What goes wrong:** Using `#[serde(default)]` on config fields means a missing field silently gets a default value instead of causing a parse error. The binary starts successfully with incorrect configuration.

**Why it happens:** `#[serde(default)]` is convenient during development but violates the "fail fast on invalid config" requirement.

**How to avoid:** Do NOT use `#[serde(default)]` on required fields. If a field has a sensible default, use a two-layer approach: parse into a raw struct without defaults, then convert to a validated struct where defaults are applied explicitly with logging:
```rust
// Raw from TOML (no defaults -- missing fields cause parse error)
#[derive(Deserialize)]
struct RawStalenessConfig {
    threshold_ms: u64,      // Required, no default
    max_skew_ms: Option<u64>, // Explicitly optional
}

// Validated with defaults for optional fields
struct StalenessConfig {
    threshold_ms: u64,
    max_skew_ms: u64, // Default applied with log message
}
```

### Pitfall 4: Tokio Instant Not Serializable

**What goes wrong:** `tokio::time::Instant` (and `std::time::Instant`) cannot be serialized or deserialized. Attempting to `#[derive(Serialize)]` on a struct containing `Instant` fails to compile.

**Why it happens:** `Instant` is a monotonic clock value with no meaningful representation outside the current process. It cannot be converted to wall-clock time.

**How to avoid:** The user decided on dual timestamps: `Instant` for internal staleness/latency, `DateTime<Utc>` for serialization. In the `Timestamp` type, mark the `Instant` field as `#[serde(skip)]` and serialize only the `DateTime<Utc>`:
```rust
#[derive(Debug, Clone)]
pub struct DualTimestamp {
    /// Monotonic instant for latency measurement and staleness checks
    /// Not serialized -- only meaningful within this process
    pub mono: tokio::time::Instant,
    /// Wall clock time for logging, display, and serialization
    pub wall: chrono::DateTime<chrono::Utc>,
}

impl Serialize for DualTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wall.serialize(serializer)
    }
}

impl DualTimestamp {
    pub fn now() -> Self {
        Self {
            mono: tokio::time::Instant::now(),
            wall: chrono::Utc::now(),
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.mono.elapsed()
    }
}
```

### Pitfall 5: Config Reload Race Conditions

**What goes wrong:** Config reload replaces the entire config atomically via watch channel, but consumers read fields at different times. Consumer A reads the old threshold, consumer B reads the new threshold, and they make inconsistent decisions.

**Why it happens:** The watch channel provides latest-value semantics, but borrowing happens at the point of use, not at a coordinated checkpoint.

**How to avoid:** Each consumer should snapshot the config once per processing cycle (e.g., once per market data event), then use that snapshot consistently throughout the cycle:
```rust
async fn process_event(config_rx: &watch::Receiver<AppConfig>, event: MarketEvent) {
    // Snapshot config ONCE at the start of processing
    let config = config_rx.borrow().clone();

    // Use config.staleness.threshold_ms consistently
    if event.age_ms() > config.staleness.threshold_ms {
        // reject
    }
    // Use config.signals.min_spread for comparison
    // Both checks use the same config snapshot
}
```

### Pitfall 6: Decimal Serialization Mismatch

**What goes wrong:** Without the `serde-with-str` feature, `rust_decimal` serializes as a JSON number. JSON numbers lose precision for large decimals. Config files with decimal values like `0.0156` parse correctly, but round-tripping through JSON (for structured logs) can lose trailing precision.

**Why it happens:** JSON number representation is IEEE 754 float, which cannot exactly represent many decimal fractions.

**How to avoid:** Enable the `serde-with-str` feature on `rust_decimal`. Use `#[serde(with = "rust_decimal::serde::str")]` on decimal fields in types that will be serialized to JSON. For TOML config parsing, decimals parse from TOML float values correctly because `rust_decimal` handles the conversion. The string serialization is primarily needed for JSON output in logs.

## Code Examples

### Complete CLI Entrypoint with Clap

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "prediction")]
#[command(about = "Cross-venue prediction market arbitrage signal generator")]
#[command(version)]
pub struct Cli {
    /// Directory containing config.toml, events.toml, venues.toml
    #[arg(long, default_value = "config")]
    pub config_dir: PathBuf,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the main application (default if no subcommand given)
    Run,
    /// Validate configuration files without starting
    CheckConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_dir = &cli.config_dir;

    match cli.command.unwrap_or(Commands::Run) {
        Commands::CheckConfig => {
            // Validate config and exit
            match crate::config::load_config(config_dir) {
                Ok(config) => {
                    println!("Configuration valid.");
                    println!("  System: {:?}", config_dir.join("config.toml"));
                    println!("  Events: {:?}", config_dir.join("events.toml"));
                    println!("  Venues: {:?}", config_dir.join("venues.toml"));
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Configuration error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Run => {
            // Load config (fail fast)
            let config = crate::config::load_config(config_dir)?;

            // Initialize logging (must happen before anything else)
            let _log_guard = crate::logging::init_logging(&config.system.logging)?;

            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                config_dir = %config_dir.display(),
                "prediction system starting"
            );

            // Setup graceful shutdown
            let shutdown_token = CancellationToken::new();
            let signal_token = shutdown_token.clone();
            tokio::spawn(async move {
                crate::shutdown::shutdown_signal(signal_token).await;
            });

            // Start config hot-reload
            let (_reloader, config_rx) = crate::config::reload::ConfigReloader::start(
                config_dir.to_path_buf(),
                config,
            )?;

            // Main loop: wait for shutdown
            shutdown_token.cancelled().await;

            tracing::info!("shutdown complete");
            // _log_guard drops here, flushing remaining logs
            Ok(())
        }
    }
}
```

### Trace ID Infrastructure

```rust
use uuid::Uuid;
use std::fmt;

/// Unique trace ID assigned to each market data event.
/// Follows the event through normalization -> spread calc -> signal.
/// Uses UUID v7 for timestamp-sorted ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(Uuid);

impl TraceId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Usage with tracing spans:
fn process_market_event(event: &MarketEvent) {
    let trace_id = TraceId::new();
    let span = tracing::info_span!(
        "market_event",
        trace_id = %trace_id,
        venue = %event.venue,
        instrument = %event.instrument_id,
    );
    let _enter = span.enter();

    // All logs within this scope include the trace_id
    tracing::debug!(bid = %event.bid, ask = %event.ask, "received market data");
}
```

### Venue Enum with Display

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Venue {
    Deribit,
    Polymarket,
    Kalshi,
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Venue::Deribit => write!(f, "deribit"),
            Venue::Polymarket => write!(f, "polymarket"),
            Venue::Kalshi => write!(f, "kalshi"),
        }
    }
}

impl Venue {
    /// Environment variable prefix for this venue's credentials
    pub fn env_prefix(&self) -> &'static str {
        match self {
            Venue::Deribit => "DERIBIT",
            Venue::Polymarket => "POLYMARKET",
            Venue::Kalshi => "KALSHI",
        }
    }
}
```

### Example Config TOML Files

**config.toml:**
```toml
[logging]
log_dir = "logs"
stdout_level = "info"
file_level = "debug"

[staleness]
threshold_ms = 5000
max_skew_ms = 2000

[signals]
min_spread_bps = 100    # 1% minimum spread
cooldown_ms = 5000      # Don't re-signal same event within 5s
```

**venues.toml:**
```toml
[deribit]
ws_url = "wss://www.deribit.com/ws/api/v2"
rate_limit_per_second = 20
heartbeat_interval_ms = 10000

[polymarket]
ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
rest_url = "https://clob.polymarket.com"
chain_id = 137  # Polygon mainnet

[kalshi]
rest_url = "https://trading-api.kalshi.com/trade-api/v2"
ws_url = "wss://trading-api.kalshi.com/trade-api/ws/v2"
```

**events.toml:**
```toml
[[events]]
id = "BTC-100K-2025-06-30"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-06-30"

[events.venues.deribit]
instrument = "BTC-27JUN25-100000-C"

[events.venues.polymarket]
condition_id = "0x..."
token_id = "12345"

[events.venues.kalshi]
ticker = "KXBTCD-25JUN30-T100000"
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tracing-subscriber` global filter | Per-layer filtering via `.with_filter()` | tracing-subscriber 0.3.x | Each output (stdout/file) can filter independently |
| `thiserror` 1.x | `thiserror` 2.0 | Late 2024 | Same derive API, updated internals. No migration needed. |
| `derive_more` 0.99 | `derive_more` 2.x | 2025 | Feature-gated derives, updated syntax. Use `features = ["full"]`. |
| `toml` 0.5 | `toml` 0.8 | 2023 | Better error messages with span info (line/col). Critical for fail-fast config. |
| `log` crate | `tracing` crate | Settled ~2021 | Structured fields, span context, async-aware. `tracing` is the standard. |
| Manual signal handling | `tokio::signal` + `CancellationToken` | tokio 1.x / tokio-util 0.7 | Hierarchical, composable, no unsafe. |
| SIGHUP for config reload | `notify` crate file watching | `notify` 6+ | Cross-platform. Windows has no SIGHUP. |

**Deprecated/outdated:**
- `log` crate: predecessor to `tracing`, no span support, no structured fields, no async awareness. Compatibility layer in tracing-subscriber captures legacy `log` output.
- `bincode`: RUSTSEC-2025-0141, unmaintained. Not needed in Phase 1 anyway.
- `env_logger`: Predecessor to tracing-subscriber. No JSON output, no per-layer filtering.

## Open Questions

1. **notify crate on Windows with long-running process**
   - What we know: `notify` uses `ReadDirectoryChangesW` on Windows, which is well-supported.
   - What is unclear: Whether editor save patterns (tmp file + rename) on Windows trigger the correct events.
   - Recommendation: The debouncer handles this. Test during Phase 1 implementation on the dev Windows machine. If issues arise, fall back to a simple polling interval.

2. **tracing-subscriber JSON output format customization**
   - What we know: The JSON layer includes `timestamp`, `level`, `target`, `fields`, and `spans` by default.
   - What is unclear: Whether the default format includes enough information for the "full context per spread computation" requirement, or whether custom fields need to be added.
   - Recommendation: Start with default JSON format. The requirement for full context (prices, timestamps, staleness, fees, edge) will be met through `tracing::info!()` structured fields, not through subscriber customization. Validate during Phase 2 when actual data flows.

3. **rust_decimal `serde-with-str` vs `serde-str` feature flag naming**
   - What we know: The feature is called `serde-with-str` and enables the `rust_decimal::serde::str` module for use with `#[serde(with = "...")]`.
   - What is unclear: Whether enabling the `serde-str` feature (which changes the DEFAULT serialization behavior) is preferable to `serde-with-str` (which requires explicit `#[serde(with)]` annotation).
   - Recommendation: Use `serde-with-str` (explicit annotation). This prevents surprising behavior where Decimal serializes as string unexpectedly in some contexts. Be explicit at each serialization point.

## Sources

### Primary (HIGH confidence)
- [tracing-subscriber fmt::Layer docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/struct.Layer.html) -- JSON layer configuration, per-layer filtering
- [tracing-subscriber filter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/index.html) -- EnvFilter per-layer usage
- [tracing-appender RollingFileAppender](https://docs.rs/tracing-appender/latest/tracing_appender/rolling/struct.RollingFileAppender.html) -- rotation options, builder API
- [tracing-appender non_blocking](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html) -- WorkerGuard pattern
- [tokio::signal](https://docs.rs/tokio/latest/tokio/signal/index.html) -- ctrl_c, Unix SIGTERM/SIGHUP
- [tokio_util CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) -- hierarchical cancellation API
- [rust_decimal::serde](https://docs.rs/rust_decimal/latest/rust_decimal/serde/index.html) -- serde modules, feature flags
- [derive_more](https://docs.rs/derive_more/latest/derive_more/) -- v2.1.1, supported derives
- [clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) -- subcommand derive pattern
- [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown) -- official shutdown patterns

### Secondary (MEDIUM confidence)
- [notify crate](https://docs.rs/notify/) -- v8.2.0, cross-platform file watching
- [notify-debouncer-mini](https://docs.rs/notify-debouncer-mini/latest/notify_debouncer_mini/) -- debounced events
- [nutype crate](https://docs.rs/nutype/latest/nutype/) -- validated newtypes (considered but derive_more preferred for this use case)
- [tracing-rolling-file](https://docs.rs/tracing-rolling-file/latest/tracing_rolling_file/) -- size-based rotation alternative (not needed)
- [Rust CLI recommendations](https://rust-cli-recommendations.sunshowers.io/handling-arguments.html) -- clap best practices

### Tertiary (LOW confidence)
- [rust-hot-reloader](https://github.com/junkurihara/rust-hot-reloader) -- hot reload patterns with notify (reference implementation)
- [Type Hell in Tracing (forum)](https://users.rust-lang.org/t/type-hell-in-tracing-multiple-output-layers/126764) -- community workarounds for multi-layer type issues

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all crates are well-established, versions verified via docs.rs
- Architecture: HIGH -- patterns are standard tokio idioms documented in official guides
- Pitfalls: HIGH -- based on official documentation warnings (WorkerGuard, per-layer filtering)
- Code examples: MEDIUM -- synthesized from official docs, not copy-pasted from verified running code

**Research date:** 2026-02-21
**Valid until:** 2026-04-21 (stable ecosystem, 60 days)
