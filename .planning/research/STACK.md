# Stack Research

**Domain:** Cross-venue crypto prediction market / options market arbitrage system
**Researched:** 2026-02-21
**Confidence:** HIGH

## Recommended Stack

### Core Runtime & Async

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| tokio | 1.49.0 | Async runtime | The only production-grade Rust async runtime. Work-stealing scheduler, built-in timers, TCP/UDP, signal handling. Every async crate in this stack depends on it. Use `features = ["full"]` for development; narrow to specific features for release builds if binary size matters. LTS releases: 1.43.x (until March 2026), 1.47.x (until Sept 2026). |
| tokio-tungstenite | 0.28.0 | WebSocket client | Best-in-class tokio WebSocket integration. Recent versions (>0.26.2) closed the performance gap with fastwebsockets. Streams-based API composes naturally with tokio select! and channel patterns. Enable `rustls-tls-webpki-roots` feature for TLS -- avoids linking OpenSSL on Linux. |
| reqwest | 0.13.2 | REST API client | De facto async HTTP client in Rust. Built on hyper. Use for Deribit/Kalshi/Polymarket REST endpoints (auth, order submission, instrument lookup). Enable features: `json`, `rustls-tls`. |

### Serialization & Data Formats

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| serde | 1.0.228 | Serialization framework | Universal. Every exchange API model derives Serialize/Deserialize. Zero runtime overhead with derive macros. |
| serde_json | 1.0.149 | JSON parsing | All three venues use JSON over WebSocket and REST. Fast, streaming-capable, zero-copy deserialization with `&'a str` borrows when possible. |
| toml | 1.0.3 | Config file parsing | Clean, human-readable config format. Serde integration means config structs get `#[derive(Deserialize)]` and you are done. TOML is the Rust ecosystem standard for configuration. |
| postcard | 1.1.3 | Binary serialization for feed recording | Compact, fast, serde-compatible. Use for recording raw feed data to disk for replay/backtesting. Actively maintained. **Do NOT use bincode** -- it was abandoned (RUSTSEC-2025-0141) after a harassment incident; v3.0.0 is a poison pill that does not compile. |

### Numeric & Financial Math

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| rust_decimal | 1.40.0 | Decimal arithmetic | 128-bit decimal type. Exact representation of prices, quantities, probabilities. No floating-point drift. Serde support built in. Use `features = ["maths"]` for exp/ln/pow needed in Black-76 pricing. Critical: exchanges send decimal strings; rust_decimal parses these losslessly. |
| statrs | 0.18.0 | Statistical distributions | Provides `Normal` distribution with CDF -- needed for Black-76 digital option pricing (N(d1), N(d2)). Also useful for confidence intervals on spread calculations. Pure Rust, no external C dependencies. |
| ordered-float | 5.1.0 | Float ordering for internal calculations | Use `OrderedFloat<f64>` or `NotNan<f64>` where you need floats as map keys or in sorted structures (e.g., implied vol caches). Implements Ord/Eq/Hash. Use sparingly -- prefer rust_decimal for prices. |

### Observability

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| tracing | 0.1.44 | Structured logging & instrumentation | The Rust ecosystem standard. Span-based context propagation means you can trace a single market update through the entire pipeline. Async-aware -- spans survive across .await points. |
| tracing-subscriber | 0.3.22 | Log output formatting | Use `fmt` layer with `json()` formatter for machine-readable logs, `pretty()` for development. Enable `env-filter` feature for runtime log level control via RUST_LOG. |
| tracing-appender | 0.2.4 | Non-blocking file logging | Rolling file appender (daily/hourly rotation). Non-blocking writer backed by a dedicated thread -- prevents I/O from blocking the hot path. Combine with tracing-subscriber for production log pipeline. |
| prometheus-client | 0.24.0 | Metrics export | The official Prometheus Rust client (maintained under github.com/prometheus/client_rust). OpenMetrics-compliant. Use over the older `prometheus` crate (TiKV) -- prometheus-client is the canonical choice going forward. Expose via a tiny axum/hyper endpoint for scraping. |

### Concurrency & Data Structures

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| dashmap | 6.1.0 | Concurrent hash map | Lock-free concurrent HashMap. Use for shared order book state, instrument caches, and vol surface lookups accessed from multiple async tasks. Drop-in replacement for `RwLock<HashMap>` with better performance under contention. Use stable 6.1.0, not 7.0.0-rc2. |
| tokio::sync::mpsc | (in tokio) | Async message passing | Prefer tokio channels over crossbeam for async code. Context-switching within the same thread to the next coroutine is cheaper than cross-thread communication. Use bounded channels for backpressure between feed handlers and pricing engine. |

### Error Handling

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| thiserror | 2.0.18 | Typed error definitions | Use in library/module code. Define specific error enums for each subsystem (feed errors, pricing errors, config errors). Derive macro generates Display/Error impls. v2 is the current major. |
| anyhow | 1.0.102 | Application-level errors | Use at the application boundary (main, top-level orchestration). Wraps any error with context chains. Pairs with thiserror: modules define specific errors, application code wraps them with anyhow for ergonomic propagation. |

### Time & Identifiers

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| chrono | 0.4.43 | Timestamps & date math | UTC timestamp handling, option expiry date calculations, feed timestamp parsing. Well-maintained, security issues resolved since 0.4.20. All exchange APIs return UTC timestamps. |
| uuid | (latest) | Unique event/signal IDs | Use v7 UUIDs -- timestamp-sorted, so signal logs are naturally ordered. Feature: `features = ["v7"]`. |

### CLI & Configuration

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| clap | 4.5.60 | Command-line parsing | Use derive API. Subcommands for: `run` (main service), `replay` (feed playback), `config check` (validate config). Feature: `features = ["derive"]`. |

### Development & Testing Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| cargo-nextest | Fast test runner | Parallel test execution, better output than `cargo test`. |
| cargo-watch | Auto-rebuild on save | `cargo watch -x check -x test` for tight feedback loop. |
| tokio-console | Async runtime inspection | Connect to running process to inspect task states, waker stats, resource usage. Enable `tokio_unstable` cfg flag + `tracing` feature on tokio. |
| cargo-deny | Dependency auditing | Check for RUSTSEC advisories (catches things like the bincode situation), license issues, duplicate deps. |

## Cargo.toml Skeleton

```toml
[package]
name = "prediction"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# Async runtime
tokio = { version = "1.49", features = ["full"] }

# Networking
tokio-tungstenite = { version = "0.28", features = ["rustls-tls-webpki-roots"] }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "1.0"
postcard = { version = "1.1", features = ["use-std"] }

# Numeric / math
rust_decimal = { version = "1.40", features = ["maths", "serde-str"] }
statrs = "0.18"
ordered-float = "5.1"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"
prometheus-client = "0.24"

# Concurrency
dashmap = "6.1"

# Error handling
thiserror = "2.0"
anyhow = "1.0"

# Time & IDs
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v7"] }

# CLI
clap = { version = "4.5", features = ["derive"] }
```

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| tokio-tungstenite | fastwebsockets 0.10.0 | Only if sub-microsecond WebSocket frame parsing is the bottleneck (unlikely -- network latency dominates). fastwebsockets is lower-level, requires manual frame handling, and has no built-in TLS integration. tokio-tungstenite's recent perf improvements make it the pragmatic choice. |
| reqwest | hyper (direct) | Only if you need connection-level control (custom keepalive, HTTP/2 multiplexing tuning). reqwest wraps hyper and handles connection pooling, TLS, cookies, redirects. For exchange REST APIs, reqwest is the right abstraction level. |
| postcard | rmp-serde (MessagePack) | If feed recordings need to be read by non-Rust tools. MessagePack has broad cross-language support. Postcard is faster and smaller but Rust-only. Since this is a single-binary Rust system, postcard is preferred. |
| postcard | rkyv 0.8 | If zero-copy deserialization of recorded feeds is needed for ultra-fast replay. rkyv is faster but requires `#[derive(Archive)]` on every type and has a steeper learning curve. Start with postcard; migrate hot paths to rkyv only if replay speed becomes a bottleneck. |
| rust_decimal | f64 | Never for prices/quantities. Use f64 only in internal math (Black-76 intermediate calculations, implied vol solver iterations) where exact decimal representation is unnecessary and speed matters. Convert back to rust_decimal at boundaries. |
| prometheus-client | metrics + metrics-exporter-prometheus | If you want a facade pattern (like tracing for logs). The `metrics` crate provides `counter!()`, `gauge!()`, `histogram!()` macros with a pluggable backend. Good if other dependencies already use it. For a greenfield project, prometheus-client is simpler and has no indirection. |
| toml | figment | If you need hierarchical config from multiple sources (file + env vars + CLI args). Figment supports layered config merging. For a single TOML config file with env var overrides, `toml` + manual env overlay is simpler and has fewer dependencies. Consider figment if config complexity grows. |
| chrono | jiff | If comprehensive IANA timezone support is critical. For this project, all times are UTC (exchange timestamps, option expiries), so chrono is sufficient and better-known. |
| dashmap | std::sync::RwLock\<HashMap\> | If contention is low (single writer, rare reads). DashMap shines under concurrent read-heavy workloads like order book lookups from multiple pricing tasks. For this system, dashmap is the right default. |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| bincode (any version) | Unmaintained. RUSTSEC-2025-0141. v3.0.0 is a poison pill that refuses to compile. The maintainer ceased development after a harassment incident. | postcard 1.1.3 for binary serialization |
| native-tls feature on tokio-tungstenite/reqwest | Links against platform OpenSSL/SChannel. Cross-compilation headaches, supply-chain risk (OpenSSL CVEs). | rustls-tls-webpki-roots -- pure Rust TLS, audited, no C dependencies |
| prometheus (TiKV crate) | Older, unofficial. prometheus-client is the canonical Prometheus Rust client maintained under the Prometheus GitHub org. | prometheus-client 0.24.0 |
| log crate | Predecessor to tracing. No span context, no structured fields, no async awareness. Many crates still emit log records, but tracing-subscriber captures them via its compatibility layer. | tracing 0.1 |
| deribit-rs / deribit crate | Last updated 2021-2022. Uses outdated tokio 0.2/1.x patterns. Deribit API has evolved. | Build your own Deribit client with tokio-tungstenite + serde. The API is simple JSON-RPC over WebSocket. Custom client gives you full control over reconnection, auth refresh, and type safety. |
| kalshi-rust / kalshi-rs | Community-maintained with uncertain update cadence. API may drift. | Build your own Kalshi client. REST API with RSA-PSS auth. Custom client ensures you handle API changes immediately. |
| polymarket rs-clob-client | Official but may lag behind API changes. Pulling in someone else's dependency tree for what is a thin HTTP/WS wrapper adds risk. | Build your own Polymarket CLOB client. WebSocket + REST with EIP-712 signing. |
| Any pre-built exchange SDK | Opaque error handling, version lag, dependency bloat, impedance mismatch with your internal types. | Custom thin clients per venue. You need ~5 message types per venue. The effort is small; the control is worth it. |

## Stack Patterns by Variant

**If adding execution (v2+):**
- Add `ethers-rs` or `alloy` for Polygon on-chain interaction (Polymarket settlement)
- Add `ring` or `rsa` crate for Kalshi RSA-PSS signature authentication
- Add rate limiter: `governor` crate for respecting exchange rate limits
- Consider `tower` middleware for retry/timeout/rate-limit composition on HTTP clients

**If deploying as Docker container:**
- Use `rustls` (already recommended) -- no OpenSSL to install in container
- Static linking with `musl` target: `cross build --target x86_64-unknown-linux-musl`
- Binary size ~15-25 MB with full stack, ~8-12 MB with `strip` and `opt-level = "z"`

**If adding backtesting/replay:**
- postcard for feed recording/replay (already in stack)
- Consider `mmap` via `memmap2` crate for memory-mapped replay of large feed files
- `indicatif` for progress bars during replay runs

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| tokio 1.49 | tokio-tungstenite 0.28 | Both require tokio 1.x. tokio-tungstenite 0.28 depends on tokio >=1.0. |
| tokio 1.49 | reqwest 0.13 | reqwest 0.13 uses hyper 1.x + tokio 1.x. Major version bump from reqwest 0.12. |
| tracing 0.1 | tracing-subscriber 0.3 | Matched pair. tracing-subscriber 0.3.x works with tracing 0.1.x. |
| tracing 0.1 | tracing-appender 0.2 | tracing-appender 0.2.x depends on tracing 0.1.x. |
| serde 1.0 | All serde-dependent crates | Serde maintains backward compat within 1.x. No conflicts expected. |
| rust_decimal 1.40 | serde 1.0 | Use `serde-str` feature to serialize as string (matches exchange API formats). |
| statrs 0.18 | Rust >= 1.87 | statrs 0.18 requires Rust 1.87+. This is the newest MSRV requirement in the stack -- ensure your toolchain meets it. If on an older Rust, pin statrs to 0.17.x (Rust 1.65+). |
| tokio-tungstenite 0.28 | rustls via rustls-tls-webpki-roots | Requires one TLS feature enabled for wss:// connections. Neither native-tls nor rustls is default. |
| reqwest 0.13 | rustls-tls | reqwest 0.13 defaults to no TLS. Must explicitly enable `rustls-tls`. |

## Critical Notes

### MSRV (Minimum Supported Rust Version)

The binding constraint is **statrs 0.18.0 requiring Rust 1.87+**. All other crates work with Rust 1.70+. If you cannot use Rust 1.87, downgrade statrs to 0.17.x or implement the Normal CDF yourself (it is ~20 lines using the error function approximation).

**Recommendation:** Use the latest stable Rust (currently 1.85 per edition 2024 support). If statrs 0.18 fails to build, either:
1. Update to latest nightly/beta that is >= 1.87, or
2. Pin `statrs = "0.17"` (confirmed working on Rust 1.65+), or
3. Implement Normal CDF directly using the Abramowitz & Stegun approximation.

### Why Build Custom Exchange Clients

For Deribit, Polymarket, and Kalshi, building thin custom clients is strongly recommended over using community SDKs:

1. **Type safety**: Your internal types (decimal prices, strongly-typed instrument IDs) should not depend on some SDK's type decisions.
2. **Reconnection control**: Each venue has different heartbeat/ping requirements. You need custom reconnection logic per venue.
3. **Auth lifecycle**: Deribit uses token refresh, Kalshi uses RSA-PSS, Polymarket uses EIP-712. Each is ~50 lines of auth code.
4. **Minimal surface**: You need ~5 message types per venue (subscribe, orderbook snapshot, orderbook delta, ticker, heartbeat). A full SDK brings hundreds of unused types.
5. **No version lag**: When an exchange updates its API, you fix your 200-line client immediately instead of waiting for an upstream PR.

### Performance Budget Estimation

For the <1ms internal processing latency target:
- **JSON deserialization** (serde_json): ~1-5 us per typical orderbook message
- **Decimal parsing** (rust_decimal): ~50-100 ns per value
- **Black-76 pricing** (f64 math + statrs CDF): ~200-500 ns per evaluation
- **Channel send** (tokio mpsc): ~20-50 ns
- **DashMap lookup**: ~50-100 ns

Total estimated hot-path latency: **~10-50 us** -- well within the <1ms budget. The bottleneck will be network I/O, not computation.

## Sources

- [tokio crate](https://crates.io/crates/tokio) -- version 1.49.0 confirmed via docs.rs (HIGH confidence)
- [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) -- version 0.28.0 confirmed via docs.rs (HIGH confidence)
- [reqwest](https://docs.rs/crate/reqwest/latest) -- version 0.13.2 confirmed via docs.rs (HIGH confidence)
- [serde](https://docs.rs/crate/serde/latest) -- version 1.0.228 confirmed via docs.rs (HIGH confidence)
- [serde_json](https://docs.rs/crate/serde_json/latest) -- version 1.0.149 confirmed via docs.rs (HIGH confidence)
- [rust_decimal](https://docs.rs/crate/rust_decimal/latest) -- version 1.40.0 confirmed via docs.rs (HIGH confidence)
- [statrs](https://docs.rs/crate/statrs/latest) -- version 0.18.0 confirmed via docs.rs (HIGH confidence)
- [tracing](https://docs.rs/crate/tracing/latest) -- version 0.1.44 confirmed via docs.rs (HIGH confidence)
- [tracing-subscriber](https://docs.rs/crate/tracing-subscriber/latest) -- version 0.3.22 confirmed via docs.rs (HIGH confidence)
- [tracing-appender](https://docs.rs/crate/tracing-appender/latest) -- version 0.2.4 confirmed via docs.rs (HIGH confidence)
- [prometheus-client](https://docs.rs/crate/prometheus-client/latest) -- version 0.24.0 confirmed via docs.rs (HIGH confidence)
- [dashmap](https://docs.rs/crate/dashmap/latest) -- stable version 6.1.0 confirmed via docs.rs (HIGH confidence)
- [thiserror](https://docs.rs/crate/thiserror/latest) -- version 2.0.18 confirmed via docs.rs (HIGH confidence)
- [anyhow](https://docs.rs/crate/anyhow/latest) -- version 1.0.102 confirmed via docs.rs (HIGH confidence)
- [chrono](https://docs.rs/crate/chrono/latest) -- version 0.4.43 confirmed via docs.rs (HIGH confidence)
- [clap](https://docs.rs/crate/clap/latest) -- version 4.5.60 confirmed via docs.rs (HIGH confidence)
- [toml](https://docs.rs/crate/toml/latest) -- version 1.0.3 confirmed via docs.rs (HIGH confidence)
- [postcard](https://docs.rs/crate/postcard/latest) -- version 1.1.3 confirmed via docs.rs (HIGH confidence)
- [ordered-float](https://docs.rs/crate/ordered-float/latest) -- version 5.1.0 confirmed via docs.rs (HIGH confidence)
- [bincode RUSTSEC-2025-0141](https://github.com/tursodatabase/libsql/issues/2207) -- unmaintained status confirmed via multiple sources (HIGH confidence)
- [prometheus/client_rust](https://github.com/prometheus/client_rust) -- official Prometheus Rust client (HIGH confidence)
- [Rust serialization benchmarks](https://github.com/djkoloski/rust_serialization_benchmark) -- benchmark methodology (MEDIUM confidence)
- [tokio-tungstenite TLS features](https://lib.rs/crates/tokio-tungstenite) -- rustls feature flags documented (HIGH confidence)
- [statrs MSRV 1.87](https://github.com/statrs-dev/statrs) -- MSRV requirement from GitHub README (MEDIUM confidence -- verify on your toolchain)

---
*Stack research for: Cross-venue crypto/prediction market arbitrage system*
*Researched: 2026-02-21*
