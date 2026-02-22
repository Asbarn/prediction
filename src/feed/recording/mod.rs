pub mod writer;

pub use writer::JsonlWriter;

use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::feed::traits::{RecordLine, Recorder};
use crate::types::Venue;

/// Non-blocking recording service that writes raw WebSocket messages to JSONL files.
///
/// Spawns a dedicated tokio task that reads from a bounded channel and writes
/// to disk via [`JsonlWriter`]. The pipeline never blocks on disk I/O -- buffer
/// overflow drops messages via `try_send` (drop newest strategy).
///
/// # Usage
///
/// ```rust,no_run
/// use std::path::PathBuf;
/// use prediction::feed::recording::RecordingService;
/// use prediction::types::Venue;
/// use tokio_util::sync::CancellationToken;
///
/// # async fn example() {
/// let cancel = CancellationToken::new();
/// let svc = RecordingService::start(
///     PathBuf::from("recordings"),
///     Venue::Deribit,
///     cancel.clone(),
/// );
/// // svc.record(line) -- non-blocking, drops on overflow
/// # }
/// ```
pub struct RecordingService {
    tx: mpsc::Sender<RecordLine>,
}

impl RecordingService {
    /// Start the recording service with a bounded channel and background writer task.
    ///
    /// The channel buffer size is 8192 messages, as recommended by research analysis.
    /// The background task drains all remaining messages on shutdown before flushing.
    pub fn start(base_dir: PathBuf, venue: Venue, cancel: CancellationToken) -> Self {
        let (tx, rx) = mpsc::channel::<RecordLine>(8192);
        let writer = JsonlWriter::new(base_dir, venue);

        tokio::spawn(recording_task(rx, writer, venue, cancel));

        RecordingService { tx }
    }

    /// Get a clone of the sender for passing to other tasks.
    ///
    /// Useful when the processor task needs to record messages directly
    /// without going through the `RecordingService` API.
    pub fn sender(&self) -> mpsc::Sender<RecordLine> {
        self.tx.clone()
    }
}

impl Recorder for RecordingService {
    /// Record a single line. Non-blocking -- drops the message if the
    /// buffer is full rather than blocking the data pipeline.
    fn record(&self, line: RecordLine) {
        match self.tx.try_send(line) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("recording buffer full, dropping message (drop newest)");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("recording channel closed unexpectedly");
            }
        }
    }
}

/// Background task that reads from the channel and writes to the JSONL file.
///
/// Uses periodic flush (1-second interval) instead of flush-per-write for
/// higher throughput. On cancellation, drains all remaining messages from
/// the channel and performs a final flush to ensure no data is lost.
async fn recording_task(
    mut rx: mpsc::Receiver<RecordLine>,
    mut writer: JsonlWriter,
    venue: Venue,
    cancel: CancellationToken,
) {
    let mut flush_interval = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut messages_since_flush: u64 = 0;

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                // Drain remaining messages before exit
                while let Ok(line) = rx.try_recv() {
                    let _ = writer.write_line_no_flush(&line).await;
                }
                let _ = writer.flush().await;
                tracing::info!(
                    venue = %venue,
                    "recording service shut down, all buffered lines flushed"
                );
                break;
            }

            msg = rx.recv() => {
                match msg {
                    Some(line) => {
                        if let Err(e) = writer.write_line_no_flush(&line).await {
                            tracing::error!(error = %e, "failed to write recording line");
                        }
                        messages_since_flush += 1;
                    }
                    None => {
                        // All senders dropped -- final flush and exit
                        let _ = writer.flush().await;
                        break;
                    }
                }
            }

            _ = flush_interval.tick() => {
                if messages_since_flush > 0 {
                    if let Err(e) = writer.flush().await {
                        tracing::error!(error = %e, "periodic flush failed");
                    }
                    messages_since_flush = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_record_line(msg: &str) -> RecordLine {
        RecordLine {
            raw: msg.to_string(),
            local_ts: Utc::now(),
            venue: Venue::Deribit,
            channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
            instrument: Some("BTC-27JUN25-100000-C".to_string()),
        }
    }

    #[tokio::test]
    async fn try_send_does_not_panic_on_full_channel() {
        // Create a service with a very small buffer to force overflow
        let tmp = std::env::temp_dir().join(format!(
            "prediction_test_{}",
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ));
        let cancel = CancellationToken::new();

        // Use a tiny channel: buffer size 2
        let (tx, rx) = mpsc::channel::<RecordLine>(2);
        let _writer = JsonlWriter::new(tmp.clone(), Venue::Deribit);
        tokio::spawn(async move {
            // Don't consume messages -- let the channel fill up
            let _rx = rx;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });

        let svc = RecordingService { tx };

        // Fill the buffer
        svc.record(make_record_line("msg1"));
        svc.record(make_record_line("msg2"));
        // This should log a warning but NOT panic
        svc.record(make_record_line("msg3"));
        svc.record(make_record_line("msg4"));

        // If we got here without panicking, the test passes
        cancel.cancel();
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn recording_task_drains_on_shutdown() {
        let tmp = std::env::temp_dir().join(format!(
            "prediction_test_{}",
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ));
        let cancel = CancellationToken::new();
        let svc = RecordingService::start(tmp.clone(), Venue::Deribit, cancel.clone());

        // Send a few messages
        for i in 0..5 {
            svc.record(make_record_line(&format!("drain_msg_{i}")));
        }

        // Small delay to let some messages be written
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Cancel and wait for shutdown
        cancel.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify all 5 messages were written
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let file_path = tmp.join("deribit").join(format!("{today}.jsonl"));
        let contents = tokio::fs::read_to_string(&file_path)
            .await
            .expect("JSONL file should exist after drain");
        let lines: Vec<&str> = contents.trim().split('\n').collect();
        assert_eq!(lines.len(), 5, "all 5 messages should be drained to disk");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn round_trip_record_and_verify_on_disk() {
        let tmp = std::env::temp_dir().join(format!(
            "prediction_test_{}",
            uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))
        ));
        let cancel = CancellationToken::new();
        let svc = RecordingService::start(tmp.clone(), Venue::Deribit, cancel.clone());

        let line = make_record_line(r#"{"test":"round_trip_data"}"#);
        svc.record(line);

        // Wait for write
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Shutdown to flush
        cancel.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let file_path = tmp.join("deribit").join(format!("{today}.jsonl"));
        let contents = tokio::fs::read_to_string(&file_path)
            .await
            .expect("JSONL file should exist");

        let value: serde_json::Value =
            serde_json::from_str(contents.trim()).expect("should be valid JSON");
        assert_eq!(
            value["raw"].as_str().unwrap(),
            r#"{"test":"round_trip_data"}"#
        );
        assert_eq!(value["venue"].as_str().unwrap(), "deribit");
        assert_eq!(
            value["channel"].as_str().unwrap(),
            "book.BTC-27JUN25-100000-C.none.20.100ms"
        );
        assert_eq!(
            value["instrument"].as_str().unwrap(),
            "BTC-27JUN25-100000-C"
        );

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
