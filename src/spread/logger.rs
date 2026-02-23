//! JSONL spread log writer with daily file rotation.
//!
//! Serializes every `SpreadResult` to a JSON line and writes to a
//! date-stamped file in the configured log directory. Periodic flushing
//! balances write performance with data durability.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::{NaiveDate, Utc};

use crate::spread::patterns::SpreadResult;

/// JSONL spread log writer with daily file rotation.
///
/// Each day gets a new file: `{log_dir}/{YYYY-MM-DD}.jsonl`.
/// Files are opened in append mode for crash safety.
pub struct SpreadLogger {
    /// Base directory for log files.
    log_dir: PathBuf,
    /// Current buffered writer (if open).
    writer: Option<BufWriter<File>>,
    /// Date of the currently open file.
    current_date: Option<NaiveDate>,
    /// Number of writes since last flush.
    writes_since_flush: u64,
    /// Flush after this many writes.
    flush_interval: u64,
}

impl SpreadLogger {
    /// Create a new SpreadLogger writing to the given directory.
    ///
    /// The directory is created if it does not exist.
    /// File opening is deferred until the first write.
    pub fn new(log_dir: &str) -> Self {
        Self {
            log_dir: PathBuf::from(log_dir),
            writer: None,
            current_date: None,
            writes_since_flush: 0,
            flush_interval: 100,
        }
    }

    /// Log a spread result as a JSON line.
    ///
    /// Handles daily file rotation: if the current date has changed since the
    /// last write, the old file is flushed and a new one is opened.
    pub async fn log(&mut self, result: &SpreadResult) -> anyhow::Result<()> {
        let today = Utc::now().date_naive();

        // Rotate file if date changed or no file open
        if self.current_date != Some(today) {
            self.rotate_file(today)?;
        }

        let line = serde_json::to_string(result)?;
        if let Some(ref mut writer) = self.writer {
            writeln!(writer, "{}", line)?;
            self.writes_since_flush += 1;

            if self.writes_since_flush >= self.flush_interval {
                writer.flush()?;
                self.writes_since_flush = 0;
            }
        }

        Ok(())
    }

    /// Flush any buffered data to disk.
    pub fn flush(&mut self) -> anyhow::Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.flush()?;
            self.writes_since_flush = 0;
        }
        Ok(())
    }

    /// Open a new file for the given date, closing any existing file.
    fn rotate_file(&mut self, date: NaiveDate) -> anyhow::Result<()> {
        // Flush and close old writer
        if let Some(ref mut writer) = self.writer {
            writer.flush()?;
        }

        // Ensure directory exists
        fs::create_dir_all(&self.log_dir)?;

        let filename = format!("{}.jsonl", date.format("%Y-%m-%d"));
        let path = self.log_dir.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        tracing::info!(path = %path.display(), "opened spread log file");

        self.writer = Some(BufWriter::new(file));
        self.current_date = Some(date);
        self.writes_since_flush = 0;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spread::patterns::{SpreadPattern, SpreadResult};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_result() -> SpreadResult {
        SpreadResult {
            event_id: "test-event".to_string(),
            pattern: SpreadPattern::BuyPolyYesSellKalshiYes,
            gross_spread: dec("0.05"),
            net_spread: dec("0.03"),
            buy_fill_price: dec("0.45"),
            sell_fill_price: dec("0.50"),
            buy_fee: dec("0.005"),
            sell_fee: dec("0.007"),
            carry_cost: dec("0.002"),
            total_cost: dec("0.014"),
            buy_fill_ratio: dec("1.0"),
            sell_fill_ratio: dec("0.95"),
            target_notional: dec("500"),
            timestamp_ms: 1700000000000,
            poly_exchange_ts: Some(1700000000100),
            kalshi_exchange_ts: None,
            threshold: Some(dec("0.025")),
            threshold_components: None,
        }
    }

    #[tokio::test]
    async fn logger_writes_jsonl_file() {
        let dir = std::env::temp_dir().join("spread_logger_test");
        let _ = fs::remove_dir_all(&dir);

        let mut logger = SpreadLogger::new(dir.to_str().unwrap());
        let result = make_result();

        logger.log(&result).await.unwrap();
        logger.flush().unwrap();

        // Check file was created
        let today = Utc::now().date_naive();
        let filename = format!("{}.jsonl", today.format("%Y-%m-%d"));
        let path = dir.join(filename);
        assert!(path.exists(), "JSONL file should exist");

        // Check content is valid JSON
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "should have 1 line");

        let _: serde_json::Value = serde_json::from_str(lines[0]).unwrap();

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn logger_appends_multiple_results() {
        let dir = std::env::temp_dir().join("spread_logger_test_multi");
        let _ = fs::remove_dir_all(&dir);

        let mut logger = SpreadLogger::new(dir.to_str().unwrap());
        let result = make_result();

        for _ in 0..5 {
            logger.log(&result).await.unwrap();
        }
        logger.flush().unwrap();

        let today = Utc::now().date_naive();
        let filename = format!("{}.jsonl", today.format("%Y-%m-%d"));
        let path = dir.join(filename);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 5, "should have 5 lines");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn logger_creates_directory_if_missing() {
        let dir = std::env::temp_dir().join("spread_logger_deep/nested/dir");
        let _ = fs::remove_dir_all(std::env::temp_dir().join("spread_logger_deep"));

        let mut logger = SpreadLogger::new(dir.to_str().unwrap());
        let result = make_result();
        logger.log(&result).await.unwrap();
        logger.flush().unwrap();

        assert!(dir.exists(), "nested directory should be created");

        // Cleanup
        let _ = fs::remove_dir_all(std::env::temp_dir().join("spread_logger_deep"));
    }
}
