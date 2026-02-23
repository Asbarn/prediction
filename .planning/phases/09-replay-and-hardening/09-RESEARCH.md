# Phase 9: Replay and Hardening - Research

**Researched:** 2026-02-23
**Domain:** Deterministic replay, HTTP health endpoints, JSONL schema stabilization
**Confidence:** HIGH

## Summary

Phase 9 is the final v1 phase, turning the accumulated data and full pipeline into a validated testing and analysis corpus. It has three distinct workstreams: (1) deterministic replay that feeds recorded JSONL through the full pipeline producing identical computation results, (2) an HTTP `/health` endpoint reporting per-feed connection status, last update times, active event count, and system uptime, and (3) stable, documented JSONL schemas for all recorded data (feeds, spreads, signals, P&L) enabling offline analysis with Python/Jupyter.

The codebase already has significant infrastructure for this phase. The `ReplayDataSource` in `src/feed/mock/replay.rs` reads JSONL recordings and produces `RawMessage` items -- but currently only for single-venue Deribit replay. The `VenueHealth` tracker in `src/feed/health.rs` was explicitly designed with Phase 9 in mind (per its doc comment) and already tracks per-venue connection status, last message time, and connection count. All JSONL loggers (feed recordings, spread logs, signal logs, paper trade logs) already serialize to JSON with serde -- the schemas just need to be stabilized and documented.

**Primary recommendation:** Use `axum` 0.8 for the `/health` HTTP endpoint on a separate port (the existing `metrics-exporter-prometheus` HTTP listener does not support custom routes). Extend the existing `ReplayDataSource` to support multi-venue replay with deterministic time control. Document JSONL schemas via a schema specification file and add serde roundtrip tests to lock them down.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| OBSV-05 | HTTP `/health` endpoint reporting: per-feed connection status, last update time per feed, active event count, system uptime | Axum 0.8 health endpoint with shared `Arc<HealthState>` reading from existing `VenueHealth` trackers + event registry count |
| OBSV-06 | JSONL schema for all recorded data (feeds, spreads, signals, P&L) is stable and documented for offline analysis tooling (Python/Jupyter) | Catalog all 4 JSONL schema types, add `schema_version` field, write schema spec doc, add serde roundtrip golden tests |
| TEST-02 | Deterministic replay from recorded JSONL feeds through the full pipeline with identical computation | Extend `ReplayDataSource` to multi-venue, bypass staleness gates during replay, use recorded timestamps instead of `Utc::now()` |
| TEST-03 | Feed recordings serve as replay corpus for backtesting and debugging pricing discrepancies | Multi-file replay across venue subdirectories, time-ordered merge, configurable speed, verification harness comparing output JSONL |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| axum | 0.8.8 | HTTP health endpoint server | Official tokio ecosystem web framework; already compatible with project's hyper 1.x and tokio 1.x; verified no dependency conflicts via `cargo add --dry-run` |
| tokio | 1.x (existing) | Async runtime, TCP listener | Already in use throughout the project |
| serde / serde_json | 1.x (existing) | JSONL serialization and schema enforcement | Already in use for all JSONL loggers |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4.x (existing) | Timestamp formatting in health response | Already a dependency |
| uuid | 1.x (existing) | Trace IDs in health response | Already a dependency |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| axum 0.8 | Raw hyper 1.x service_fn | More boilerplate, no routing ergonomics; axum is only ~200KB additional compile, zero new transitive deps beyond what metrics-exporter-prometheus already brings |
| axum 0.8 | warp | warp is less maintained, filter-based API less ergonomic for single endpoint |
| Separate health port | Embed in Prometheus listener | metrics-exporter-prometheus 0.18 HTTP listener responds to ALL GET paths with metrics payload; no custom route support (verified via docs.rs) |

**Installation:**
```bash
cargo add axum@0.8 --no-default-features --features json,tokio
```

Note: `json` feature brings serde_json (already present). Minimal feature set avoids pulling in multipart/ws/form dependencies.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── health/              # NEW: Health endpoint module
│   └── mod.rs           # HealthState, axum handler, server spawn
├── feed/
│   ├── mock/
│   │   └── replay.rs    # EXTEND: Multi-venue replay with timestamp control
│   └── health.rs        # EXISTING: VenueHealth tracker (read by health endpoint)
├── replay/              # NEW: Full-pipeline replay orchestration
│   └── mod.rs           # ReplayOrchestrator, multi-venue JSONL merge, output capture
└── ...
```

### Pattern 1: Shared Health State via Arc
**What:** A `HealthState` struct wraps references to all `VenueHealth` trackers, the `EventRegistry`, and a startup timestamp. Passed to axum as shared state.
**When to use:** When the health endpoint needs to read from multiple independent subsystems without coupling them.
**Example:**
```rust
use std::sync::Arc;
use axum::{extract::State, routing::get, Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::events::registry::EventRegistry;
use crate::feed::health::VenueHealth;

#[derive(Clone)]
pub struct HealthState {
    pub venue_health: Vec<Arc<VenueHealth>>,
    pub event_registry: Arc<RwLock<EventRegistry>>,
    pub started_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: i64,
    pub feeds: Vec<FeedStatus>,
    pub active_event_count: usize,
}

#[derive(Serialize)]
pub struct FeedStatus {
    pub venue: String,
    pub connected: bool,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub connection_count: u64,
}

async fn health_handler(State(state): State<HealthState>) -> Json<HealthResponse> {
    let uptime = Utc::now().signed_duration_since(state.started_at).num_seconds();
    let feeds: Vec<FeedStatus> = state.venue_health.iter().map(|vh| {
        FeedStatus {
            venue: vh.venue().to_string(),
            connected: vh.is_available(),
            last_message_at: vh.last_message_at(),
            last_error: vh.last_error(),
            connection_count: vh.connection_count(),
        }
    }).collect();
    let active_event_count = state.event_registry.read().await.event_count();
    Json(HealthResponse {
        status: if feeds.iter().any(|f| f.connected) { "ok".into() } else { "degraded".into() },
        uptime_secs: uptime,
        feeds,
        active_event_count,
    })
}

pub async fn start_health_server(state: HealthState, port: u16) {
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("health server bind");
    axum::serve(listener, app).await.ok();
}
```

### Pattern 2: Multi-Venue Deterministic Replay
**What:** A `ReplayOrchestrator` reads JSONL files from multiple venue subdirectories, merges events by `local_ts` in chronological order, and feeds them through per-venue processors into the shared pipeline -- using recorded timestamps rather than live wall-clock time.
**When to use:** For TEST-02 and TEST-03 -- replaying historical periods to reproduce signals.
**Key insight:** The existing `ReplayDataSource` already handles single-venue Deribit replay at configurable speed. For multi-venue, we need to:
1. Read JSONL files from `recordings/deribit/`, `recordings/polymarket/`, `recordings/kalshi/`
2. Parse all `RecordLine` entries and sort by `local_ts` globally
3. Route each entry to the correct venue processor based on `venue` field
4. Use recorded `local_ts` for staleness calculations (not `Utc::now()`)

**Example (multi-venue merge concept):**
```rust
use crate::feed::traits::RecordLine;

struct ReplayCorpus {
    entries: Vec<RecordLine>,  // All venues, sorted by local_ts
}

impl ReplayCorpus {
    async fn load_directory(recordings_dir: &Path) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        for venue_dir in ["deribit", "polymarket", "kalshi"] {
            let dir = recordings_dir.join(venue_dir);
            if !dir.exists() { continue; }
            for file in sorted_jsonl_files(&dir).await? {
                let contents = tokio::fs::read_to_string(&file).await?;
                for line in contents.lines() {
                    if let Ok(record) = serde_json::from_str::<RecordLine>(line) {
                        entries.push(record);
                    }
                }
            }
        }
        entries.sort_by_key(|e| e.local_ts);
        Ok(Self { entries })
    }
}
```

### Pattern 3: Staleness Bypass for Replay Mode
**What:** During replay, staleness gates compare message timestamps against each other (relative time), not against wall-clock time. This is critical for determinism.
**When to use:** In replay mode, staleness checks like `now_ms - exchange_ts > threshold` will always reject old data.
**Approaches (ordered by complexity):**

1. **Disable staleness gates entirely in replay mode** (simplest, recommended for v1): Pass a `replay_mode: bool` flag to `SpreadEngine` and `CrossAssetEngine` that skips staleness checks. This is the lowest-risk approach -- staleness gates are about live-data freshness, which is meaningless in replay.

2. **Virtual clock**: Maintain a "replay clock" that advances to `local_ts` of each processed message. Replace `Utc::now()` in staleness checks with `replay_clock.now()`. Higher complexity, more accurate for inter-message staleness.

3. **Tokio time pause**: Use `tokio::time::pause()` and `tokio::time::advance()` in tests. Only works with `tokio::time::Instant`, not `chrono::Utc::now()` -- so most staleness checks would still use wall-clock time. Not suitable here.

**Recommendation:** Approach 1 for this phase. The `is_stale` flag on `MarketSnapshot` already provides per-message staleness from the processor layer. The engine-level staleness gates are redundant during replay.

### Anti-Patterns to Avoid
- **Using `Utc::now()` in replay-sensitive code paths:** The codebase has 20 files calling `Utc::now()`. For deterministic replay, engines must NOT compare recorded timestamps against wall-clock time. Either disable those checks or use a virtual clock.
- **Merging JSONL files by filename date only:** Files from different venues may cover different time ranges within the same date. Always sort by `local_ts` across all files.
- **Blocking the health endpoint on pipeline state:** The health endpoint must be a lightweight read of atomics/mutexes, never waiting on pipeline channels. `VenueHealth` already uses `AtomicBool` and `Mutex` for this.
- **Coupling JSONL schema to internal Rust types:** Schema documentation should describe the JSON structure, not the Rust struct. Fields like `DualTimestamp` serialize to a single ISO 8601 string -- the schema must document this serialized form.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP server for health endpoint | Custom TCP listener with manual HTTP parsing | axum 0.8 with `Router::new().route("/health", get(handler))` | HTTP/1.1 compliance, graceful shutdown, error handling, keep-alive -- all trivial to get wrong |
| JSON schema validation | Custom schema validation code | Serde roundtrip golden tests (serialize -> deserialize -> assert_eq) | Roundtrip tests catch schema drift without a separate schema validator |
| Multi-file JSONL reading | Custom directory walking and sorting | `std::fs::read_dir` + filter `.jsonl` + sort by filename | Filenames are date-stamped (YYYY-MM-DD.jsonl), lexicographic sort = chronological order |
| JSONL schema documentation | Custom documentation generator | Hand-written Markdown with example JSON objects generated from test fixtures | Simpler, more maintainable, and the test fixtures serve as the source of truth |

**Key insight:** This phase is primarily about wiring existing infrastructure together, not building new complex systems. The feed health tracker, JSONL writers, replay source, and pipeline assembly all exist. The work is integration and stabilization.

## Common Pitfalls

### Pitfall 1: Staleness Gates Kill Replay
**What goes wrong:** Replay feeds historical data through engines that check `Utc::now() - exchange_timestamp > threshold`. Since exchange timestamps are days/weeks old, every message is rejected as stale.
**Why it happens:** Staleness logic was designed for live feeds where data should be seconds old.
**How to avoid:** Pass `replay_mode: bool` to `SpreadEngine::run()` and `CrossAssetEngine::run()`. When true, skip staleness comparison against wall clock. The processor-level `is_stale` flag (based on sequence gaps) still works correctly in replay.
**Warning signs:** Replay produces zero spread computations and zero signals. All messages logged as "stale, skipping."

### Pitfall 2: Non-Deterministic DualTimestamp in Replay
**What goes wrong:** The existing `ReplayDataSource` creates `DualTimestamp::now()` for each replayed message. This means the `timestamp.wall` field in `MarketSnapshot` is the current wall time, not the recorded time. Downstream engines then have inconsistent time references.
**Why it happens:** `DualTimestamp::now()` was the pragmatic choice for single-venue replay where timing didn't matter.
**How to avoid:** For full-pipeline replay, construct `DualTimestamp` using the recorded `local_ts` instead of `Utc::now()`. The `mono` field can use `Instant::now()` (it's only used for in-process elapsed timing).
**Warning signs:** Replayed signal timestamps don't match the original recording period.

### Pitfall 3: Health Endpoint Port Conflict with Prometheus
**What goes wrong:** If the health endpoint uses the same port as the Prometheus metrics exporter (default 9000), one of them fails to bind.
**Why it happens:** Both need an HTTP listener on a TCP port.
**How to avoid:** Use a separate port for the health endpoint (e.g., 9001). Add a `health_port` field to `SystemConfig` with `#[serde(default)]`. Prometheus stays on its existing `prometheus.port` (default 9000).
**Warning signs:** "address already in use" error at startup.

### Pitfall 4: Schema Breaking Changes in JSONL
**What goes wrong:** A field name change or type change in a Serialize struct silently breaks offline Python analysis scripts that rely on specific field names.
**Why it happens:** Rust serde derives produce schema implicitly from struct field names. A rename is a silent schema break.
**How to avoid:** (1) Add `schema_version: "1.0"` field to all JSONL output types. (2) Add golden file tests that serialize a known struct and compare against a stored JSON snapshot. (3) Use `#[serde(rename = "...")]` explicitly when field names must differ from Rust names.
**Warning signs:** Python/Jupyter notebooks fail to parse new JSONL files after a code change.

### Pitfall 5: Multi-Venue Replay Missing Venue Recordings
**What goes wrong:** User has Deribit recordings but no Polymarket/Kalshi recordings. Multi-venue replay crashes or produces no cross-venue signals.
**Why it happens:** Replay orchestrator expects all three venue directories.
**How to avoid:** Graceful degradation: if a venue directory is missing or empty, log a warning and continue with available venues. This matches the live mode behavior (RELY-04).
**Warning signs:** "No such file or directory" errors, or replay produces only single-venue output.

### Pitfall 6: EventRegistry Not Populated in Replay Mode
**What goes wrong:** Replay produces MarketSnapshots but no spread/signal computations because the EventRegistry has no mappings for the replayed instruments.
**Why it happens:** In live mode, the ContractLifecycleManager discovers and populates the registry. In replay mode, lifecycle manager is not running.
**How to avoid:** Load `events.toml` into the EventRegistry before starting replay (same as live mode -- the registry loads from config, not from discovery). The existing `EventRegistry::from_config()` handles this.
**Warning signs:** All snapshots logged as "unmapped instrument, skipping."

## Code Examples

### Health Endpoint Configuration Extension
```rust
// In src/config/system.rs
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HealthConfig {
    /// Port for the HTTP /health endpoint.
    pub port: u16,
    /// Whether to enable the health endpoint.
    pub enabled: bool,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            port: 9001,
            enabled: true,
        }
    }
}
```

### Multi-Venue Replay Pipeline Wiring
```rust
// Conceptual wiring in main.rs or a replay module
pub async fn run_replay_pipeline(
    recordings_dir: PathBuf,
    config: &VenuesConfig,
    speed: f64,
    cancel: CancellationToken,
) -> anyhow::Result<mpsc::Receiver<MarketSnapshot>> {
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<MarketSnapshot>(1024);

    // Load and merge all venue recordings
    let corpus = ReplayCorpus::load_directory(&recordings_dir).await?;

    // Group by venue for per-venue processing
    let mut by_venue: HashMap<Venue, Vec<RecordLine>> = HashMap::new();
    for entry in corpus.entries {
        by_venue.entry(entry.venue).or_default().push(entry);
    }

    // Spawn per-venue replay -> processor -> fan-in
    for (venue, entries) in by_venue {
        let tx = snapshot_tx.clone();
        let cancel = cancel.clone();
        // Each venue gets its own processor, feeding into shared channel
        tokio::spawn(replay_venue(venue, entries, speed, tx, cancel));
    }

    drop(snapshot_tx); // Close when all spawned tasks finish
    Ok(snapshot_rx)
}
```

### JSONL Schema Version Field
```rust
// Add schema_version to serialized output
// Approach: wrapper struct for JSONL output that adds version metadata

#[derive(Serialize)]
struct VersionedRecord<T: Serialize> {
    schema_version: &'static str,
    #[serde(flatten)]
    data: T,
}

// Usage in logger:
let versioned = VersionedRecord {
    schema_version: "1.0",
    data: &spread_result,
};
let line = serde_json::to_string(&versioned)?;
```

### Golden Test for Schema Stability
```rust
#[test]
fn spread_result_schema_stable() {
    let result = SpreadResult { /* known fixture values */ };
    let json = serde_json::to_value(&result).unwrap();

    // Verify all expected fields are present
    let expected_fields = [
        "event_id", "pattern", "gross_spread", "net_spread",
        "buy_fill_price", "sell_fill_price", "buy_fee", "sell_fee",
        "carry_cost", "total_cost", "buy_fill_ratio", "sell_fill_ratio",
        "target_notional", "timestamp_ms", "poly_exchange_ts",
        "kalshi_exchange_ts", "threshold", "threshold_components",
    ];
    for field in expected_fields {
        assert!(json.get(field).is_some(), "missing field: {field}");
    }

    // Verify Decimal fields serialize as strings (not numbers)
    assert!(json["gross_spread"].is_string());
    assert!(json["net_spread"].is_string());
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| warp for HTTP | axum 0.8 (tokio-native) | 2024 | axum is now the standard tokio HTTP framework; warp is maintenance-mode |
| Manual HTTP with hyper 0.14 | axum 0.8 built on hyper 1.x | hyper 1.0 stable Jan 2024, axum 0.8 Jan 2025 | Clean separation of transport (hyper) and routing (axum) |
| JSON Schema for validation | Serde golden tests | Ongoing best practice | JSON Schema adds a validation dependency; serde roundtrip tests catch the same issues with zero new deps |

**Deprecated/outdated:**
- hyper 0.14: Superseded by hyper 1.0; the project already uses hyper 1.x via metrics-exporter-prometheus
- warp: Maintenance mode, not recommended for new projects

## Open Questions

1. **VenueHealth instances are not currently accessible from main.rs**
   - What we know: `VenueHealth::new()` is called inside `run_live_multi_venue()` in `pipeline.rs`, but the `Arc<VenueHealth>` instances are not returned to the caller. The health endpoint needs access to these.
   - What's unclear: Whether to refactor `run_multi_venue_pipeline()` to return health handles, or to create a shared `FeedHealthRegistry` that pipeline.rs populates and main.rs reads.
   - Recommendation: Refactor `run_multi_venue_pipeline()` to accept a `Vec<Arc<VenueHealth>>` parameter that it populates, or return a `PipelineHandles` struct containing both `snapshot_rx` and venue health references. The latter is cleaner.

2. **Replay output comparison methodology**
   - What we know: TEST-02 requires "identical computation results." We need a way to compare replay output against a known-good baseline.
   - What's unclear: What "identical" means given floating-point arithmetic and UUID generation. UUIDs will differ between runs. Floating-point results should be bitwise identical if inputs are identical.
   - Recommendation: For comparison, exclude fields that are inherently non-deterministic (signal_id UUIDs, DualTimestamp mono field). Compare all numeric fields. Use `serde_json::Value` comparison with a custom comparator that ignores specific keys.

3. **EventRegistry `event_count()` method does not exist yet**
   - What we know: The health endpoint needs to report "active event count." EventRegistry has `from_config()`, `lookup_by_instrument()`, but no `event_count()` or `len()` method.
   - What's unclear: Whether "active event count" means total mapped events, or only events with recent market data.
   - Recommendation: Add `pub fn event_count(&self) -> usize` returning the number of registered event mappings. For "active" (events with recent data), the health endpoint can cross-reference with engine state in a future iteration.

4. **Recordings directory structure has nesting issue**
   - What we know: `recordings/deribit/deribit/` and `recordings/polymarket/polymarket/` directories exist (double nesting). The `RecordingService::start()` in `pipeline.rs` is called with `recording_dir.join("deribit")` and `JsonlWriter::new()` joins `venue.to_string()` again.
   - What's unclear: Whether existing recordings are in the double-nested path or the single-nested path.
   - Recommendation: Fix the double-nesting in pipeline.rs (pass base `recording_dir` to `RecordingService::start()` which already adds the venue subdirectory). For replay, support both paths with a fallback check.

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/feed/mock/replay.rs` -- existing ReplayDataSource implementation
- Codebase analysis: `src/feed/health.rs` -- existing VenueHealth tracker (designed for Phase 9)
- Codebase analysis: `src/feed/pipeline.rs` -- pipeline assembly showing DataMode::Replay path
- Codebase analysis: `src/feed/recording/writer.rs` -- JSONL recording format and RecordLine schema
- Codebase analysis: `src/signal/types.rs` -- ArbSignal JSONL schema with full Serialize/Deserialize
- Codebase analysis: `src/spread/patterns.rs` -- SpreadResult JSONL schema
- Codebase analysis: `src/paper_trade/position.rs` -- PaperPosition JSONL schema
- Codebase analysis: `Cargo.toml` -- dependency analysis confirming axum 0.8 compatibility
- Context7: `/tokio-rs/axum` -- axum 0.8 State extractor, Router, JSON response patterns
- docs.rs: `metrics-exporter-prometheus 0.18` -- confirmed HTTP listener responds to ALL GET paths, no custom route support

### Secondary (MEDIUM confidence)
- [Announcing axum 0.8.0](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) -- version compatibility and features
- [Deterministic simulation testing for async Rust](https://s2.dev/blog/dst) -- tokio time control patterns
- [barter-rs](https://github.com/barter-rs/barter-rs) -- Rust event-driven backtesting framework patterns
- [JSONL Best Practices](https://jsonltools.com/jsonl-best-practices) -- schema versioning, documentation practices

### Tertiary (LOW confidence)
- None. All findings verified against codebase or official documentation.

## JSONL Schema Inventory

Complete catalog of all JSONL output types that need schema stabilization:

### 1. Feed Recordings (`recordings/{venue}/{date}.jsonl`)
**Source struct:** `RecordLine` in `src/feed/traits.rs`
**Fields:** `raw` (string), `local_ts` (ISO 8601), `venue` (string), `channel` (string), `instrument` (string|null)
**Status:** Stable. Simple, flat structure. Already has Serialize + Deserialize.

### 2. Spread Logs (`spread_logs/{date}.jsonl`)
**Source struct:** `SpreadResult` in `src/spread/patterns.rs`
**Fields:** `event_id`, `pattern`, `gross_spread` (string decimal), `net_spread` (string decimal), `buy_fill_price`, `sell_fill_price`, `buy_fee`, `sell_fee`, `carry_cost`, `total_cost`, `buy_fill_ratio`, `sell_fill_ratio`, `target_notional`, `timestamp_ms`, `poly_exchange_ts`, `kalshi_exchange_ts`, `threshold`, `threshold_components`
**Status:** Stable. Has Serialize but NOT Deserialize. Needs `Deserialize` derive added.

### 3. Signal Logs (`signal_logs/{date}.jsonl`)
**Source struct:** `ArbSignal` in `src/signal/types.rs`
**Fields:** Full signal with nested `LegInfo`, `CostBreakdown`, `ConfidenceComponents`, `SolverResult`, `ThresholdComponents`, `DualTimestamp`
**Status:** Stable. Already has both Serialize + Deserialize. Complex nested structure -- schema documentation essential.

### 4. Paper Trade Logs (`paper_trades/{date}.jsonl`)
**Source struct:** `TradeEvent` in `src/paper_trade/tracker.rs` (wraps `PaperPosition`)
**Fields:** Trade lifecycle events (Fill, MtmUpdate, DailySummary)
**Status:** Stable. Has Serialize but NOT Deserialize. Needs `Deserialize` derive added.

### Schema Documentation Approach
For each JSONL type:
1. Generate a canonical example JSON object from test fixtures
2. Document each field: name, type, unit, nullable, description
3. Add `schema_version: "1.0"` field via wrapper or direct addition
4. Add golden serde roundtrip tests that fail on any field addition/removal/rename

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - axum 0.8 verified compatible, all existing crate dependencies confirmed
- Architecture: HIGH - patterns directly follow existing codebase conventions (Arc shared state, mpsc channels, CancellationToken shutdown)
- Pitfalls: HIGH - all identified from direct codebase analysis of staleness checks, timestamp handling, and recording paths
- JSONL schemas: HIGH - complete inventory from reading all logger and type source files

**Research date:** 2026-02-23
**Valid until:** 2026-03-23 (stable domain, no fast-moving dependencies)
