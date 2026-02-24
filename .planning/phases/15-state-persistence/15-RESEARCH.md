# Phase 15: State Persistence - Research

**Researched:** 2026-02-24
**Domain:** File-based checkpoint/recovery for paper trade state and signal accumulators in Rust
**Confidence:** HIGH

## Summary

Phase 15 adds checkpoint-based state persistence so that multi-week paper trading sessions survive process restarts. The system must periodically serialize paper trade positions, daily P&L rollups, and signal analysis accumulator state to a JSON checkpoint file, and recover that state on startup by loading the checkpoint then replaying any JSONL trade events that occurred after the checkpoint timestamp.

The core technical challenge is straightforward: the state is small (< 200KB per the Out of Scope rationale in REQUIREMENTS.md), all key data structures already derive `Serialize`/`Deserialize`, and the existing JSONL trade logger provides a write-ahead log for gap recovery. The primary risks are (1) Windows-specific atomicity concerns for checkpoint writes, (2) correctly sequencing recovery replay against checkpoint timestamps, and (3) not blocking the hot path with synchronous I/O.

**Primary recommendation:** Implement a single `CheckpointManager` that runs as a periodic tokio task, serializes a `CheckpointState` struct to JSON, writes it atomically via write-to-temp-then-rename, and on startup loads the checkpoint then replays JSONL trade events after the checkpoint timestamp. No new crate dependencies required -- use `std::fs::rename` (which supports overwrite on Windows 10+) with a fallback remove-then-rename for older/edge-case Windows configurations per the concern flagged in STATE.md.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PRST-01 | System periodically checkpoints paper trade state to JSON file | CheckpointManager periodic task with configurable interval, serializes positions + rollups to JSON |
| PRST-02 | Checkpoint writes use atomic write-then-rename pattern (Windows-compatible) | Write to `.checkpoint.tmp` in same directory, then `std::fs::rename` (overwrite-capable on Windows 10 1607+), with remove-then-rename fallback |
| PRST-03 | System recovers paper trade state from checkpoint on startup | `CheckpointManager::load()` at startup, restores PaperTradeTracker state before entering event loop |
| PRST-04 | System replays JSONL trade events after checkpoint timestamp for complete recovery | After checkpoint load, scan JSONL trade log files for events with `timestamp_ms > checkpoint_timestamp`, replay signal/entry/mtm/settlement events to reconstruct gap state |
| PRST-05 | Checkpoint includes signal analysis accumulator state | `CheckpointState` struct includes `DailyAggregator` rollup data (HashMap of DailyRollup) and `total_trades` counter |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_json | 1.0 | Checkpoint serialization/deserialization | Already in dependency tree, all key types derive Serialize/Deserialize |
| std::fs | (stdlib) | File I/O, rename, temp file creation | No external dependency needed for < 200KB state files |
| chrono | 0.4 | Timestamp comparison for replay window | Already in dependency tree |
| tokio | 1.x | Periodic checkpoint timer, async coordination | Already the async runtime |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::io::BufReader | (stdlib) | Efficient JSONL replay parsing | During startup recovery replay |
| std::io::BufWriter | (stdlib) | Buffered checkpoint writes | During checkpoint serialization |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| JSON checkpoint | SQLite (rusqlite) | Overkill for < 200KB state; adds dependency; JSON is human-readable for debugging |
| std::fs::rename | tempfile crate | Would add dependency; std::fs::rename works on Windows 10+; project constraint is zero new dependencies for v1.1 |
| Periodic checkpoint | Write-ahead log only | Slower recovery (replay entire log); checkpoint + replay is standard pattern |

**Installation:** No new dependencies required. All functionality built on existing crate tree.

## Architecture Patterns

### Recommended Project Structure

```
src/
├── persistence/
│   ├── mod.rs            # Module root, re-exports
│   ├── checkpoint.rs     # CheckpointState struct, serialization, atomic write
│   ├── recovery.rs       # Checkpoint loading + JSONL replay logic
│   └── manager.rs        # CheckpointManager periodic task
├── paper_trade/
│   ├── tracker.rs        # Modified: accept restored state, expose state for checkpoint
│   └── ...
└── config/
    └── system.rs         # Modified: add PersistenceConfig
```

### Pattern 1: Checkpoint + WAL Recovery

**What:** Periodically serialize full state to a checkpoint file. On crash recovery, load the last good checkpoint then replay the write-ahead log (existing JSONL trade events) from the checkpoint timestamp forward.

**When to use:** When state is small enough to serialize fully but you need point-in-time recovery between checkpoints.

**How it works in this system:**

1. **Checkpoint write (periodic):** Every N seconds (configurable, default 60s), serialize `CheckpointState` to `{checkpoint_dir}/checkpoint.json.tmp`, then rename to `checkpoint.json`
2. **Clean shutdown:** Force a final checkpoint before exit (triggered by CancellationToken)
3. **Startup recovery:** Load `checkpoint.json`, extract `checkpoint_timestamp_ms`, scan JSONL trade log files for events after that timestamp, replay them into the restored state
4. **Crash recovery:** Same as startup -- the last good checkpoint is always a complete, atomic file. Events between last checkpoint and crash are recovered from JSONL.

```rust
/// Complete checkpoint state -- everything needed to restore PaperTradeTracker.
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointState {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Timestamp when this checkpoint was written (epoch millis).
    pub checkpoint_timestamp_ms: i64,
    /// Pending positions awaiting next-tick fill.
    pub pending: HashMap<String, Vec<PaperPosition>>,
    /// Active open positions.
    pub open: Vec<PaperPosition>,
    /// Daily P&L rollup data.
    pub daily_rollups: HashMap<String, DailyRollup>,
    /// Running total trade count.
    pub total_trades: u64,
}
```

### Pattern 2: Atomic Write via Temp-then-Rename

**What:** Write checkpoint to a temporary file in the same directory, then rename over the target. This ensures the checkpoint file is always either the old complete version or the new complete version -- never a partial write.

**When to use:** Any time you need crash-safe file updates.

**Platform considerations:**

On **Unix/Linux**, `rename()` is atomic -- guaranteed by POSIX. On **Windows 10 1607+**, `std::fs::rename` uses `FileRenameInfoEx` with POSIX semantics which supports atomic overwrite. On **older Windows or edge cases**, rename may fail if the target exists and is locked. The fallback pattern is:

```rust
fn atomic_write(target: &Path, content: &[u8]) -> anyhow::Result<()> {
    let tmp = target.with_extension("tmp");

    // Write to temp file
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?;  // fsync for durability

    // Atomic rename (works on Windows 10 1607+)
    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Fallback for edge cases: remove target, then rename
            tracing::warn!(error = %e, "atomic rename failed, using remove-then-rename fallback");
            let _ = std::fs::remove_file(target);
            std::fs::rename(&tmp, target)?;
            Ok(())
        }
    }
}
```

**Critical detail:** Call `file.sync_all()` before rename to ensure data is on disk, not just in the OS buffer cache. Without this, a power failure could result in a zero-length file.

### Pattern 3: State Extraction and Restoration on PaperTradeTracker

**What:** Add methods to PaperTradeTracker to extract its current state for checkpointing and to restore state from a checkpoint before entering the event loop.

**When to use:** The tracker owns the authoritative state. Rather than making all fields public or cloning the whole struct, provide focused `snapshot_state()` and `restore_state()` methods.

```rust
impl PaperTradeTracker {
    /// Extract current state for checkpointing.
    pub fn snapshot_state(&self) -> CheckpointState {
        CheckpointState {
            version: 1,
            checkpoint_timestamp_ms: chrono::Utc::now().timestamp_millis(),
            pending: self.pending.clone(),
            open: self.open.clone(),
            daily_rollups: self.aggregator.export_rollups(),
            total_trades: self.total_trades,
        }
    }

    /// Restore state from a checkpoint.
    pub fn restore_state(&mut self, state: CheckpointState) {
        self.pending = state.pending;
        self.open = state.open;
        self.aggregator.import_rollups(state.daily_rollups);
        self.total_trades = state.total_trades;
    }
}
```

### Pattern 4: JSONL Replay for Gap Recovery

**What:** After loading a checkpoint, scan JSONL trade event files for events with `timestamp_ms > checkpoint_timestamp_ms` and replay them to update state.

**When to use:** To recover state changes that occurred between the last checkpoint and shutdown/crash.

**Replay mechanics:**

1. Determine which JSONL files could contain post-checkpoint events (by date in filename)
2. Read each relevant file line-by-line
3. Parse each line as `TradeEvent`
4. Skip events with `timestamp_ms <= checkpoint_timestamp_ms`
5. Apply signal/entry/mtm/settlement events to restore state

```rust
fn replay_events_after(
    log_dir: &Path,
    after_ms: i64,
    tracker: &mut PaperTradeTracker,
) -> anyhow::Result<u64> {
    let mut replayed = 0u64;

    // Find relevant JSONL files (checkpoint date and later)
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let reader = BufReader::new(File::open(&path)?);
        for line in reader.lines() {
            let line = line?;
            let event: TradeEvent = serde_json::from_str(&line)?;
            if event.timestamp_ms() > after_ms {
                tracker.apply_trade_event(event);
                replayed += 1;
            }
        }
    }

    Ok(replayed)
}
```

### Anti-Patterns to Avoid

- **Blocking the hot path with checkpoint I/O:** Checkpoint writes must happen on a separate task or use non-blocking I/O. The event loop (signal/snapshot processing) must not block waiting for a checkpoint to finish writing.
- **Checkpointing on every event:** Wasteful -- state is only checkpointed periodically (e.g., every 60s). The JSONL trade log already captures every event for gap recovery.
- **Serializing MTM history in checkpoint:** The `mtm_history` Vec on PaperPosition can grow large over time. Consider truncating or excluding it from checkpoints since it can be reconstructed from JSONL replay if needed. However, since positions are expected to be few (paper trading at low frequency), this may not be a practical concern.
- **Using a separate thread for I/O without coordination:** The PaperTradeTracker is single-threaded (runs in one tokio task). State extraction must happen within that task's event loop to avoid data races. Use a `tokio::spawn_blocking` or a channel to offload the actual file write.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Atomic file writes | Custom file locking / journal | write-to-temp + rename | Standard pattern; edge cases around partial writes, OS buffer caching, and cross-platform behavior are well-understood |
| JSONL parsing | Custom line-by-line parser | serde_json::from_str per line | TradeEvent already derives Deserialize; serde handles tagged enum deserialization |
| Timestamp ordering | Custom sort/merge of log files | Filter by timestamp_ms field per line | JSONL files are already chronologically ordered within each file; filtering is sufficient |
| Schema versioning | Migration framework | Version field + match on deserialize | State is small; if schema changes, just add the version field and handle old versions explicitly |

**Key insight:** The existing JSONL trade logger is effectively a write-ahead log. The checkpoint is an optimization to avoid replaying the entire log history on every restart. Both components already exist in different forms -- this phase wires them together.

## Common Pitfalls

### Pitfall 1: Forgetting `sync_all()` Before Rename

**What goes wrong:** Data appears written (in OS page cache) but a power failure or crash before the OS flushes to disk results in a zero-length or corrupted temp file. The rename succeeds but the new file is empty.

**Why it happens:** `File::write_all` and `BufWriter::flush()` push data to the OS but not necessarily to the physical disk.

**How to avoid:** Always call `file.sync_all()` (or `sync_data()`) before renaming. This is a blocking call -- on spinning disks it can take 5-15ms, on SSDs < 1ms. Acceptable for a 60-second checkpoint interval.

**Warning signs:** Checkpoint files that are 0 bytes after an unexpected shutdown.

### Pitfall 2: Windows Rename Failures When Target Is Open

**What goes wrong:** `std::fs::rename` fails with "Access Denied" if another process has the target file open without `FILE_SHARE_DELETE`.

**Why it happens:** Windows file locking semantics differ from Unix. If a monitoring tool, antivirus, or backup agent has the checkpoint file open, rename fails.

**How to avoid:** Use the fallback pattern (remove-then-rename). Also, the `FILE_RENAME_POSIX_SEMANTICS` flag on Windows 10 1607+ handles this case. This project targets Windows 11, so the primary path should work. The fallback catches edge cases.

**Warning signs:** Intermittent "Access Denied" errors on checkpoint write.

### Pitfall 3: Replay Applying Events That Are Already In Checkpoint

**What goes wrong:** If the checkpoint timestamp and a trade event have the same millisecond timestamp, the event might be double-counted.

**Why it happens:** Checkpoint happens at time T, event also at time T. If replay uses `>= T` instead of `> T`, the event is applied twice.

**How to avoid:** Use strict `> checkpoint_timestamp_ms` for replay filtering. The checkpoint already contains the state at `checkpoint_timestamp_ms`, so only events strictly after that time need replay. Accept the theoretical risk of losing events at the exact same millisecond (astronomically unlikely in practice at paper trading frequencies).

**Warning signs:** Duplicate positions after restart, trade count mismatches.

### Pitfall 4: Blocking the Event Loop During Checkpoint

**What goes wrong:** If checkpoint serialization + file I/O happens synchronously in the PaperTradeTracker's event loop, it blocks signal and snapshot processing for the duration of the write.

**Why it happens:** Serializing a few hundred KB of JSON and writing to disk can take 1-10ms. At paper trading frequencies this is acceptable, but the pattern should be correct.

**How to avoid:** Two viable approaches:
1. **Inline with timer tick (simplest):** Since checkpoints happen every 60s and paper trading is low-frequency, a brief 1-10ms pause is acceptable. Use a `tokio::time::interval` tick in the select loop.
2. **Channel-based offload:** Snapshot state in the event loop (fast, just clone), send to a dedicated writer task via channel. More complex but non-blocking.

For this project, approach 1 (inline) is recommended given the low frequency and small state size.

### Pitfall 5: Schema Evolution Breaking Recovery

**What goes wrong:** A code change adds/removes/renames a field in `PaperPosition` or `DailyRollup`, and existing checkpoint files fail to deserialize.

**Why it happens:** `serde_json` strict mode rejects unknown fields by default and requires all non-Option fields to be present.

**How to avoid:**
- Include `version: u32` in `CheckpointState` for schema detection
- Use `#[serde(default)]` on all new fields
- Use `#[serde(deny_unknown_fields)]` NEVER -- allow forward compat
- Test deserialization of old checkpoint formats in unit tests

**Warning signs:** Startup panics with "missing field" serde errors after code updates.

## Code Examples

Verified patterns from the existing codebase:

### Existing TradeEvent Already Has Serde Support

The `TradeEvent` enum in `src/paper_trade/tracker.rs` already derives `Serialize` and `Deserialize` with tagged union format:

```rust
// Source: src/paper_trade/tracker.rs:133-174
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum TradeEvent {
    #[serde(rename = "signal")]
    Signal { trade_id: String, event_id: String, /* ... */ timestamp_ms: i64 },
    #[serde(rename = "entry")]
    Entry { trade_id: String, event_id: String, /* ... */ timestamp_ms: i64 },
    #[serde(rename = "mtm")]
    Mtm { trade_id: String, event_id: String, /* ... */ timestamp_ms: i64 },
    #[serde(rename = "settlement")]
    Settlement { trade_id: String, event_id: String, /* ... */ timestamp_ms: i64 },
}
```

Each variant carries `timestamp_ms` which the replay logic uses for filtering.

### PaperPosition Already Derives Serialize/Deserialize

```rust
// Source: src/paper_trade/position.rs:28-61
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperPosition {
    pub id: String,
    pub event_id: String,
    pub pattern: SpreadPattern,
    pub status: PositionStatus,
    #[serde(with = "rust_decimal::serde::str")]
    pub notional: Decimal,
    // ... all fields already have serde support
}
```

### DailyRollup Already Derives Serialize

```rust
// Source: src/paper_trade/aggregator.rs:24-48
#[derive(Debug, Clone, Serialize)]
pub struct DailyRollup {
    pub date: String,
    pub trade_count: usize,
    pub signal_count: usize,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_pnl: Decimal,
    // ...
}
```

Note: `DailyRollup` derives `Serialize` but not `Deserialize`. Must add `Deserialize` derive for checkpoint loading.

### DailyAggregator Needs Export/Import Methods

The `DailyAggregator` struct in `src/paper_trade/aggregator.rs` holds a `HashMap<String, DailyRollup>` but does not expose it. New methods needed:

```rust
impl DailyAggregator {
    /// Export all rollup data for checkpointing.
    pub fn export_rollups(&self) -> HashMap<String, DailyRollup> {
        self.daily_pnl.clone()
    }

    /// Import rollup data from a checkpoint.
    pub fn import_rollups(&mut self, rollups: HashMap<String, DailyRollup>) {
        self.daily_pnl = rollups;
    }
}
```

### PaperTradeConfig Extension for Persistence

```rust
// New config section in config.toml
[persistence]
enabled = true
checkpoint_dir = "state"                  # Directory for checkpoint files
checkpoint_interval_secs = 60             # How often to write checkpoints
trade_log_dir = "paper_trades"            # Same as paper_trade.log_dir (for replay source)
```

### Existing Shutdown Pattern (CancellationToken)

```rust
// Source: src/paper_trade/tracker.rs:219-229
_ = cancel.cancelled() => {
    tracing::info!(
        total_trades = self.total_trades,
        open_positions = self.open.len(),
        "PaperTradeTracker shutting down"
    );
    self.aggregator.emit_daily_summary(&today);
    let _ = self.trade_logger.flush();
    break;
}
```

The shutdown handler already flushes the trade logger. Phase 15 adds a final checkpoint write here.

### CheckpointManager Integration Into main.rs

The checkpoint manager integrates into the existing pipeline by receiving a channel from PaperTradeTracker:

```rust
// In main.rs, after creating PaperTradeTracker:

// Load checkpoint if exists
let checkpoint_state = persistence::recovery::load_checkpoint(&persistence_config)?;
if let Some(state) = &checkpoint_state {
    tracing::info!(
        checkpoint_ts = state.checkpoint_timestamp_ms,
        positions = state.open.len(),
        trades = state.total_trades,
        "loaded checkpoint state"
    );
}

let mut paper_tracker = PaperTradeTracker::new(paper_trade_config);

if let Some(state) = checkpoint_state {
    let replay_count = persistence::recovery::replay_trade_events(
        &persistence_config.trade_log_dir,
        state.checkpoint_timestamp_ms,
        &mut paper_tracker,
    )?;
    paper_tracker.restore_state(state);
    tracing::info!(replayed = replay_count, "JSONL replay complete");
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `MoveFileExW` for rename on Windows | `FileRenameInfoEx` with POSIX semantics | Windows 10 1607 / Rust 1.78+ | Atomic rename-over-existing now works on modern Windows |
| External WAL crate | Checkpoint + existing JSONL as WAL | N/A (project-specific) | Zero new dependencies; leverages existing infrastructure |

**Deprecated/outdated:**
- `atomicwrites` crate: Last updated 2021, still works but unnecessary given std::fs::rename improvements on modern Windows.

## Open Questions

1. **Should MTM history be included in checkpoints?**
   - What we know: MTM history can grow unbounded on long-lived positions. Each entry is ~100 bytes. With log_mtm=true and frequent snapshots, a position could accumulate thousands of MTM entries.
   - What's unclear: Practical size impact at paper trading frequency (likely minimal -- positions are few).
   - Recommendation: Include in checkpoint for simplicity. If size becomes an issue, add a `max_mtm_entries_in_checkpoint` config and truncate to the most recent N entries. MTM history is also preserved in JSONL logs for full reconstruction.

2. **Should the checkpoint interval be adaptive?**
   - What we know: A fixed 60-second interval is simple and sufficient. The maximum data loss on crash is 60 seconds of trade events, recoverable from JSONL.
   - What's unclear: Whether operators would want more frequent checkpoints during high-activity periods.
   - Recommendation: Fixed interval for v1.1. An adaptive scheme (checkpoint when N events accumulate OR interval elapses, whichever first) is a v2 enhancement if needed.

3. **Ordering of recovery: restore then replay, or replay into empty then checkpoint?**
   - What we know: Restore-then-replay is standard. Load checkpoint (provides baseline), then replay gap events on top.
   - What's unclear: Edge case where a position is both in the checkpoint and has later events in JSONL (e.g., position was Open in checkpoint, got a Settlement event in the gap).
   - Recommendation: Replay must handle idempotency for this case. A Settlement event for a position already in the `open` list should settle it. A Signal event creating a position that's already in `pending` should be skipped (deduplicate by trade_id).

## Sources

### Primary (HIGH confidence)
- `src/paper_trade/tracker.rs` - Existing PaperTradeTracker structure with TradeEvent JSONL logging
- `src/paper_trade/position.rs` - PaperPosition with full Serialize/Deserialize support
- `src/paper_trade/aggregator.rs` - DailyAggregator with DailyRollup (needs Deserialize added)
- `src/config/system.rs` - SystemConfig pattern for new config sections
- `src/shutdown.rs` - CancellationToken shutdown pattern
- [std::fs::rename docs](https://doc.rust-lang.org/std/fs/fn.rename.html) - Platform-specific rename behavior
- [Rust PR #131072](https://github.com/rust-lang/rust/pull/131072) - POSIX rename semantics on Windows

### Secondary (MEDIUM confidence)
- [Rust issue #123985](https://github.com/rust-lang/rust/issues/123985) - Windows rename edge cases
- `.planning/STATE.md` - Flagged concern: "Windows rename() is not atomic when target exists -- needs remove-before-rename in persistence"

### Tertiary (LOW confidence)
- None -- all findings verified against codebase and official docs

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in dependency tree; types already serializable
- Architecture: HIGH - Checkpoint + WAL is a well-understood pattern; codebase structure is clear
- Pitfalls: HIGH - Windows atomicity concern was already flagged in STATE.md; solutions verified against Rust std docs and recent PRs
- Recovery replay: MEDIUM - Idempotency of replay events needs careful implementation; edge cases around duplicate trade_ids require testing

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable domain, no fast-moving external dependencies)
