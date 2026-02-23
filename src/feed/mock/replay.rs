//! Replay data source that reads JSONL recordings and produces RawMessages.
//!
//! Feeds previously recorded WebSocket frames through the pipeline at
//! configurable speed: 0.0 = instant, 1.0 = real-time, 10.0 = 10x fast-forward.

use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::traits::{RawMessage, RecordLine};
use crate::types::DualTimestamp;

/// Buffer size for the replay message channel.
const REPLAY_BUFFER: usize = 1024;

/// Internal source for replay: either a JSONL file path or pre-loaded records.
enum ReplaySource {
    File(PathBuf),
    Records(Vec<RecordLine>),
}

/// Replays a JSONL recording file or pre-loaded records, producing `RawMessage`
/// items at configurable speed.
///
/// Each entry is expected to be a `RecordLine` containing the raw WebSocket
/// frame text and a `local_ts` timestamp for pacing. The `received_at`
/// timestamp on produced `RawMessage` uses the recorded `local_ts` (not
/// `Utc::now()`) so downstream processors see historically accurate timestamps.
pub struct ReplayDataSource {
    source: ReplaySource,
    speed: f64,
    cancel: CancellationToken,
}

impl ReplayDataSource {
    /// Create a new replay data source from a JSONL file.
    ///
    /// - `file_path`: Path to the JSONL recording file
    /// - `speed`: Replay speed multiplier (0.0 = instant, 1.0 = real-time, 10.0 = 10x)
    /// - `cancel`: Cancellation token for graceful shutdown
    pub fn new(file_path: PathBuf, speed: f64, cancel: CancellationToken) -> Self {
        Self {
            source: ReplaySource::File(file_path),
            speed,
            cancel,
        }
    }

    /// Create a replay data source from pre-loaded records (no file I/O).
    ///
    /// Used by the multi-venue replay pipeline to feed grouped records
    /// directly without writing temporary files.
    pub fn from_records(
        records: Vec<RecordLine>,
        speed: f64,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            source: ReplaySource::Records(records),
            speed,
            cancel,
        }
    }
}

/// Shared replay loop: feeds records through the channel with timing control.
///
/// Uses the recorded `local_ts` for the `DualTimestamp` wall clock so that
/// downstream processors see historically accurate timestamps. The monotonic
/// instant is set to `Instant::now()` since it has no meaningful replay value
/// (per research pitfall #2).
async fn replay_records(
    lines: Vec<RecordLine>,
    speed: f64,
    cancel: CancellationToken,
    tx: mpsc::Sender<RawMessage>,
) {
    let total_lines = lines.len();
    let mut prev_ts: Option<chrono::DateTime<chrono::Utc>> = None;

    for (i, record) in lines.into_iter().enumerate() {
        // Compute sleep duration based on inter-message timing
        if speed > 0.0 {
            if let Some(prev) = prev_ts {
                let delta = record.local_ts.signed_duration_since(prev);
                let delta_ms = delta.num_milliseconds().max(0) as f64;
                let sleep_ms = delta_ms / speed;
                if sleep_ms > 0.0 {
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            tracing::info!(replayed = i, total = total_lines, "replay cancelled");
                            return;
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_secs_f64(sleep_ms / 1000.0)) => {}
                    }
                }
            }
        }

        prev_ts = Some(record.local_ts);

        // Use the recorded local_ts for the wall clock so downstream
        // processors see historically accurate timestamps.
        let raw = RawMessage {
            text: record.raw,
            received_at: DualTimestamp {
                mono: tokio::time::Instant::now(),
                wall: record.local_ts,
            },
        };

        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(replayed = i, total = total_lines, "replay cancelled");
                return;
            }
            result = tx.send(raw) => {
                if result.is_err() {
                    tracing::warn!("replay receiver dropped, stopping");
                    return;
                }
            }
        }
    }

    tracing::info!(total = total_lines, "replay complete");
    // Sender drops here, receiver gets None
}

impl crate::feed::traits::RawDataSource for ReplayDataSource {
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<RawMessage>> {
        let speed = self.speed;
        let cancel = self.cancel.clone();

        let lines: Vec<RecordLine> = match &self.source {
            ReplaySource::File(path) => {
                tracing::info!(
                    path = %path.display(),
                    speed = speed,
                    "starting JSONL replay from file"
                );

                let contents = tokio::fs::read_to_string(path)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("failed to read replay file {}: {}", path.display(), e)
                    })?;

                let mut parsed = Vec::new();
                for (i, line_str) in contents.lines().enumerate() {
                    if line_str.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<RecordLine>(line_str) {
                        Ok(record) => parsed.push(record),
                        Err(e) => {
                            tracing::warn!(
                                line = i + 1,
                                error = %e,
                                "skipping unparseable JSONL line in replay file"
                            );
                        }
                    }
                }

                let total = parsed.len();
                tracing::info!(lines = total, "parsed replay file");

                if parsed.is_empty() {
                    anyhow::bail!(
                        "replay file {} contains no valid JSONL lines",
                        path.display()
                    );
                }

                parsed
            }
            ReplaySource::Records(records) => {
                tracing::info!(
                    entries = records.len(),
                    speed = speed,
                    "starting JSONL replay from records"
                );

                if records.is_empty() {
                    anyhow::bail!("replay records are empty");
                }

                records.clone()
            }
        };

        let (tx, rx) = mpsc::channel::<RawMessage>(REPLAY_BUFFER);

        tokio::spawn(replay_records(lines, speed, cancel, tx));

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::traits::RawDataSource;
    use crate::types::Venue;
    use chrono::Utc;

    /// Create a temp JSONL file with sample RecordLines.
    async fn create_temp_jsonl(lines: &[RecordLine]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "prediction_replay_test_{}.jsonl",
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ));
        let mut content = String::new();
        for line in lines {
            content.push_str(&serde_json::to_string(line).unwrap());
            content.push('\n');
        }
        tokio::fs::write(&path, &content).await.unwrap();
        path
    }

    fn make_record_line(raw: &str, ts: chrono::DateTime<Utc>) -> RecordLine {
        RecordLine {
            raw: raw.to_string(),
            local_ts: ts,
            venue: Venue::Deribit,
            channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
            instrument: Some("BTC-27JUN25-100000-C".to_string()),
        }
    }

    #[tokio::test]
    async fn replay_reads_all_lines_at_instant_speed() {
        let base_ts = Utc::now();
        let lines: Vec<RecordLine> = (0..5)
            .map(|i| {
                let ts = base_ts + chrono::Duration::milliseconds(i * 100);
                make_record_line(
                    &format!(r#"{{"jsonrpc":"2.0","method":"subscription","params":{{"channel":"book.BTC-27JUN25-100000-C.none.20.100ms","data":{{"timestamp":1703001600000,"instrument_name":"BTC-27JUN25-100000-C","change_id":{},"bids":[[0.0055,10.0]],"asks":[[0.0060,8.0]]}}}}}}"#, 100 + i),
                    ts,
                )
            })
            .collect();

        let path = create_temp_jsonl(&lines).await;
        let cancel = CancellationToken::new();
        let source = ReplayDataSource::new(path.clone(), 0.0, cancel.clone());

        let mut rx = source.start().await.expect("start should succeed");

        let mut received = Vec::new();
        while let Some(msg) = rx.recv().await {
            received.push(msg);
        }

        assert_eq!(received.len(), 5, "should receive all 5 messages");

        // Verify content of first message
        assert!(received[0].text.contains("change_id"));

        // Cleanup
        cancel.cancel();
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn replay_respects_cancellation() {
        let base_ts = Utc::now();
        let lines: Vec<RecordLine> = (0..100)
            .map(|i| {
                let ts = base_ts + chrono::Duration::milliseconds(i * 1000); // 1 second apart
                make_record_line(r#"{"test":"cancel"}"#, ts)
            })
            .collect();

        let path = create_temp_jsonl(&lines).await;
        let cancel = CancellationToken::new();
        let source = ReplayDataSource::new(path.clone(), 1.0, cancel.clone());

        let mut rx = source.start().await.expect("start should succeed");

        // Receive a few messages
        let _ = rx.recv().await;

        // Cancel early
        cancel.cancel();

        // Drain remaining
        let mut count = 0;
        while let Some(_) = rx.recv().await {
            count += 1;
        }

        // Should NOT have received all 100 messages (cancelled after ~1)
        assert!(count < 90, "should have been cancelled early, got {count} messages");

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn replay_from_records_produces_messages() {
        let base_ts = Utc::now();
        let records: Vec<RecordLine> = (0..3)
            .map(|i| {
                let ts = base_ts + chrono::Duration::milliseconds(i * 50);
                RecordLine {
                    raw: format!(r#"{{"jsonrpc":"2.0","method":"subscription","params":{{"channel":"book.BTC-27JUN25-100000-C.none.20.100ms","data":{{"timestamp":1703001600000,"instrument_name":"BTC-27JUN25-100000-C","change_id":{},"bids":[[0.0055,10.0]],"asks":[[0.0060,8.0]]}}}}}}"#, 200 + i),
                    local_ts: ts,
                    venue: Venue::Deribit,
                    channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
                    instrument: Some("BTC-27JUN25-100000-C".to_string()),
                }
            })
            .collect();

        let cancel = CancellationToken::new();
        let source = ReplayDataSource::from_records(records, 0.0, cancel.clone());

        let mut rx = source.start().await.expect("start should succeed");

        let mut received = Vec::new();
        while let Some(msg) = rx.recv().await {
            received.push(msg);
        }

        assert_eq!(received.len(), 3, "should receive all 3 messages from records");
        assert!(received[0].text.contains("change_id"));

        // Verify that received_at uses the recorded timestamp, not current time
        // The wall clock should be close to base_ts (within a few seconds),
        // not the current time when this assertion runs
        let wall_ts = received[0].received_at.wall();
        let diff = (wall_ts - base_ts).num_seconds().abs();
        assert!(
            diff < 2,
            "received_at.wall should use recorded local_ts, not Utc::now(). diff={}s",
            diff
        );

        cancel.cancel();
    }
}
