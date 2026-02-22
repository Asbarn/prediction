use std::path::PathBuf;

use chrono::Utc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::feed::traits::RecordLine;
use crate::types::Venue;

/// Async JSONL writer with daily file rotation.
///
/// Each venue gets its own subdirectory, and files rotate daily with the
/// naming pattern `{base_dir}/{venue}/{date}.jsonl`. The writer uses
/// buffered I/O. Two write modes are available:
///
/// - [`write_line`](Self::write_line): flush after every write (safe, slower)
/// - [`write_line_no_flush`](Self::write_line_no_flush): no flush (use with periodic flush for throughput)
pub struct JsonlWriter {
    base_dir: PathBuf,
    venue: Venue,
    current_date: String,
    writer: Option<BufWriter<File>>,
}

impl JsonlWriter {
    /// Create a new writer for a specific venue.
    ///
    /// No file is opened until the first `write_line` call.
    pub fn new(base_dir: PathBuf, venue: Venue) -> Self {
        Self {
            base_dir,
            venue,
            current_date: String::new(),
            writer: None,
        }
    }

    /// Write a single record line to the JSONL file (flush-per-write variant).
    ///
    /// Automatically rotates to a new file when the date changes.
    /// Flushes after every write for maximum data safety. Use
    /// [`write_line_no_flush`](Self::write_line_no_flush) with periodic flush
    /// for higher throughput.
    pub async fn write_line(&mut self, line: &RecordLine) -> std::io::Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();

        if self.current_date != today || self.writer.is_none() {
            self.rotate(&today).await?;
        }

        let json = serde_json::to_string(line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        if let Some(ref mut w) = self.writer {
            w.write_all(json.as_bytes()).await?;
            w.write_all(b"\n").await?;
            w.flush().await?;
        }

        Ok(())
    }

    /// Write a single record line without flushing.
    ///
    /// Used with periodic flush for higher throughput. The caller is
    /// responsible for calling [`flush`](Self::flush) at regular intervals
    /// (e.g., every 1 second) and on shutdown.
    pub async fn write_line_no_flush(&mut self, line: &RecordLine) -> std::io::Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();

        if self.current_date != today || self.writer.is_none() {
            self.rotate(&today).await?;
        }

        let json = serde_json::to_string(line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        if let Some(ref mut w) = self.writer {
            w.write_all(json.as_bytes()).await?;
            w.write_all(b"\n").await?;
        }

        Ok(())
    }

    /// Rotate to a new JSONL file for the given date.
    ///
    /// Flushes and drops the current writer, then opens a new file at
    /// `{base_dir}/{venue}/{date}.jsonl` in append mode.
    async fn rotate(&mut self, date: &str) -> std::io::Result<()> {
        // Flush and drop the current writer
        if let Some(ref mut w) = self.writer.take() {
            w.flush().await?;
        }

        let dir = self.base_dir.join(self.venue.to_string());
        tokio::fs::create_dir_all(&dir).await?;

        let path = dir.join(format!("{date}.jsonl"));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        self.writer = Some(BufWriter::new(file));
        self.current_date = date.to_string();

        tracing::info!(path = %path.display(), "Recording to {}", path.display());

        Ok(())
    }

    /// Flush the underlying BufWriter if one is open.
    ///
    /// Called during graceful shutdown to ensure all buffered data
    /// reaches disk.
    pub async fn flush(&mut self) -> std::io::Result<()> {
        if let Some(ref mut w) = self.writer {
            w.flush().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_record_line() -> RecordLine {
        RecordLine {
            raw: r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"book.BTC-27JUN25-100000-C.none.20.100ms","data":{"bids":[],"asks":[]}}}"#.to_string(),
            local_ts: Utc::now(),
            venue: Venue::Deribit,
            channel: "book.BTC-27JUN25-100000-C.none.20.100ms".to_string(),
            instrument: Some("BTC-27JUN25-100000-C".to_string()),
        }
    }

    #[tokio::test]
    async fn write_line_creates_directory_and_file() {
        let tmp = std::env::temp_dir().join(format!("prediction_test_{}", uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))));
        let mut writer = JsonlWriter::new(tmp.clone(), Venue::Deribit);

        let line = make_record_line();
        writer.write_line(&line).await.expect("write_line should succeed");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let file_path = tmp.join("deribit").join(format!("{today}.jsonl"));
        assert!(file_path.exists(), "JSONL file should exist at {}", file_path.display());

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn multiple_writes_produce_valid_jsonl() {
        let tmp = std::env::temp_dir().join(format!("prediction_test_{}", uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))));
        let mut writer = JsonlWriter::new(tmp.clone(), Venue::Deribit);

        // Write 5 lines
        for _ in 0..5 {
            let line = make_record_line();
            writer.write_line(&line).await.expect("write_line should succeed");
        }
        writer.flush().await.expect("flush should succeed");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let file_path = tmp.join("deribit").join(format!("{today}.jsonl"));
        let contents = tokio::fs::read_to_string(&file_path).await.expect("should read file");

        let lines: Vec<&str> = contents.trim().split('\n').collect();
        assert_eq!(lines.len(), 5, "should have 5 JSONL lines");

        // Each line should be valid JSON
        for (i, json_line) in lines.iter().enumerate() {
            let value: serde_json::Value = serde_json::from_str(json_line)
                .unwrap_or_else(|e| panic!("line {i} should be valid JSON: {e}"));

            // Verify expected fields exist
            assert!(value.get("raw").is_some(), "line {i} should have 'raw' field");
            assert!(value.get("local_ts").is_some(), "line {i} should have 'local_ts' field");
            assert!(value.get("venue").is_some(), "line {i} should have 'venue' field");
            assert!(value.get("channel").is_some(), "line {i} should have 'channel' field");
            assert!(value.get("instrument").is_some(), "line {i} should have 'instrument' field");
        }

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn rotate_creates_new_file() {
        let tmp = std::env::temp_dir().join(format!("prediction_test_{}", uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))));
        let mut writer = JsonlWriter::new(tmp.clone(), Venue::Deribit);

        // Manually trigger rotation with two different dates
        writer.rotate("2026-01-01").await.expect("rotate should succeed");
        assert_eq!(writer.current_date, "2026-01-01");

        let file1 = tmp.join("deribit").join("2026-01-01.jsonl");
        assert!(file1.exists(), "first rotated file should exist");

        writer.rotate("2026-01-02").await.expect("rotate should succeed");
        assert_eq!(writer.current_date, "2026-01-02");

        let file2 = tmp.join("deribit").join("2026-01-02.jsonl");
        assert!(file2.exists(), "second rotated file should exist");

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn flush_with_no_writer_is_noop() {
        let tmp = std::env::temp_dir().join(format!("prediction_test_{}", uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext))));
        let mut writer = JsonlWriter::new(tmp, Venue::Deribit);
        // Should not panic or error
        writer.flush().await.expect("flush on empty writer should succeed");
    }
}
