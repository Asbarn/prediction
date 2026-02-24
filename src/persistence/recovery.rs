//! Checkpoint loading and JSONL trade event replay for startup recovery.
//!
//! On startup, the system loads the most recent checkpoint (if any) and then
//! replays any trade events from JSONL log files that occurred after the
//! checkpoint timestamp. This bridges the gap between the last checkpoint
//! and the actual state at shutdown/crash time.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::paper_trade::tracker::TradeEvent;
use crate::persistence::CheckpointState;

/// Load the most recent checkpoint from the given directory.
///
/// Returns `Ok(None)` if no checkpoint file exists (first run).
/// Returns `Err` if the file exists but cannot be parsed.
pub fn load_checkpoint(checkpoint_dir: &Path) -> anyhow::Result<Option<CheckpointState>> {
    let checkpoint_path = checkpoint_dir.join("checkpoint.json");
    if !checkpoint_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&checkpoint_path)?;
    let state: CheckpointState = serde_json::from_str(&content)?;
    Ok(Some(state))
}

/// Replay trade events from JSONL log files that occurred after the given timestamp.
///
/// Scans all `.jsonl` files in `log_dir`, parses each line as a `TradeEvent`,
/// and collects events with `timestamp_ms > after_ms`. Returns events sorted
/// by timestamp for deterministic replay order.
///
/// The caller is responsible for applying these events to the `PaperTradeTracker`
/// via `apply_trade_event()`.
pub fn replay_trade_events(
    log_dir: &Path,
    after_ms: i64,
) -> anyhow::Result<Vec<TradeEvent>> {
    let mut events = Vec::new();

    if !log_dir.exists() {
        return Ok(events);
    }

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        // Only process .jsonl files
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let reader = BufReader::new(File::open(&path)?);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // Parse the trade event
            let event: TradeEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!(
                        file = %path.display(),
                        error = %err,
                        "skipping unparseable JSONL line during replay"
                    );
                    continue;
                }
            };
            // Filter: only events strictly after checkpoint
            if event.timestamp_ms() > after_ms {
                events.push(event);
            }
        }
    }

    // Sort by timestamp for deterministic replay order
    events.sort_by_key(|e| e.timestamp_ms());

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::CheckpointState;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_load_checkpoint_missing_dir() {
        let dir = std::env::temp_dir().join("recovery_test_missing_dir_xyzzy");
        let _ = fs::remove_dir_all(&dir); // ensure it doesn't exist
        let result = load_checkpoint(&dir).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_checkpoint_missing_file() {
        let dir = std::env::temp_dir().join("recovery_test_missing_file");
        let _ = fs::create_dir_all(&dir);
        // Remove checkpoint.json if it exists from a previous run
        let _ = fs::remove_file(dir.join("checkpoint.json"));

        let result = load_checkpoint(&dir).unwrap();
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_checkpoint_valid_file() {
        let dir = std::env::temp_dir().join("recovery_test_valid_checkpoint");
        let _ = fs::create_dir_all(&dir);

        let state = CheckpointState {
            version: 1,
            checkpoint_timestamp_ms: 1700000005000,
            pending: HashMap::new(),
            open: Vec::new(),
            daily_rollups: HashMap::new(),
            total_trades: 42,
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let path = dir.join("checkpoint.json");
        fs::write(&path, &json).unwrap();

        let loaded = load_checkpoint(&dir).unwrap().expect("should load checkpoint");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.checkpoint_timestamp_ms, 1700000005000);
        assert_eq!(loaded.total_trades, 42);
        assert!(loaded.pending.is_empty());
        assert!(loaded.open.is_empty());
        assert!(loaded.daily_rollups.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replay_filters_by_timestamp() {
        let dir = std::env::temp_dir().join("recovery_test_replay_filter");
        let _ = fs::create_dir_all(&dir);

        // Write a JSONL file with events at timestamps 100, 200, 300
        let events = vec![
            r#"{"type":"signal","trade_id":"t1","event_id":"e1","pattern":"BuyPolyYesSellKalshiYes","signal_spread":"0.03","notional":"500","timestamp_ms":100}"#,
            r#"{"type":"signal","trade_id":"t2","event_id":"e2","pattern":"BuyPolyYesSellKalshiYes","signal_spread":"0.04","notional":"500","timestamp_ms":200}"#,
            r#"{"type":"signal","trade_id":"t3","event_id":"e3","pattern":"BuyPolyYesSellKalshiYes","signal_spread":"0.05","notional":"500","timestamp_ms":300}"#,
        ];

        let path = dir.join("trades-2026-01-01.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for event in &events {
            writeln!(file, "{}", event).unwrap();
        }
        file.flush().unwrap();

        // Replay with after_ms=150, should get events at 200 and 300
        let replayed = replay_trade_events(&dir, 150).unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].timestamp_ms(), 200);
        assert_eq!(replayed[1].timestamp_ms(), 300);

        // Replay with after_ms=0, should get all 3
        let replayed_all = replay_trade_events(&dir, 0).unwrap();
        assert_eq!(replayed_all.len(), 3);

        // Replay with after_ms=300, should get none
        let replayed_none = replay_trade_events(&dir, 300).unwrap();
        assert_eq!(replayed_none.len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replay_empty_dir() {
        let dir = std::env::temp_dir().join("recovery_test_replay_empty");
        let _ = fs::create_dir_all(&dir);

        // Empty directory -> no events
        let replayed = replay_trade_events(&dir, 0).unwrap();
        assert!(replayed.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replay_nonexistent_dir() {
        let dir = std::env::temp_dir().join("recovery_test_replay_nodir_xyzzy");
        let _ = fs::remove_dir_all(&dir);

        let replayed = replay_trade_events(&dir, 0).unwrap();
        assert!(replayed.is_empty());
    }
}
