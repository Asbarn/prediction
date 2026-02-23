//! JSONL signal log writer with daily file rotation.
//!
//! Serializes every `ArbSignal` to a JSON line and writes to a
//! date-stamped file in the configured log directory. Periodic flushing
//! balances write performance with data durability.
//!
//! Follows the exact `SpreadLogger` pattern from `src/spread/logger.rs`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::{NaiveDate, Utc};

use crate::signal::types::ArbSignal;

/// JSONL signal log writer with daily file rotation.
///
/// Each day gets a new file: `{log_dir}/{YYYY-MM-DD}.jsonl`.
/// Files are opened in append mode for crash safety.
pub struct SignalLogger {
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

impl SignalLogger {
    /// Create a new SignalLogger writing to the given directory.
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

    /// Log an arb signal as a JSON line.
    ///
    /// Handles daily file rotation: if the current date has changed since the
    /// last write, the old file is flushed and a new one is opened.
    ///
    /// Note: the method uses `async fn` signature for consistency with
    /// SpreadLogger's interface, but performs no async I/O internally.
    pub async fn log(&mut self, signal: &ArbSignal) -> anyhow::Result<()> {
        let today = Utc::now().date_naive();

        // Rotate file if date changed or no file open
        if self.current_date != Some(today) {
            self.rotate_file(today)?;
        }

        let line = serde_json::to_string(signal)?;
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

        tracing::info!(path = %path.display(), "opened signal log file");

        self.writer = Some(BufWriter::new(file));
        self.current_date = Some(date);
        self.writes_since_flush = 0;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::types::{ConfidenceComponents, PricingMethod};
    use crate::signal::types::{
        ArbDirection, ArbSignal, CostBreakdown, LegInfo, ThresholdStatus,
    };
    use crate::types::{DualTimestamp, Venue};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn make_signal() -> ArbSignal {
        ArbSignal {
            signal_id: uuid::Uuid::now_v7().to_string(),
            event_id: "evt-test-logger".to_string(),
            direction: ArbDirection::BuyPredictionSellOptions,
            raw_spread: dec("0.04"),
            net_edge: dec("0.028"),
            confidence: 0.75,
            prediction_leg: LegInfo {
                venue: Venue::Polymarket,
                instrument_id: "poly-test".to_string(),
                probability: dec("0.45"),
                executable_price: dec("0.46"),
                book_depth_levels: 3,
                fill_ratio: dec("0.90"),
            },
            options_leg: LegInfo {
                venue: Venue::Deribit,
                instrument_id: "BTC-27JUN25-100000-C".to_string(),
                probability: dec("0.49"),
                executable_price: dec("0.50"),
                book_depth_levels: 5,
                fill_ratio: dec("1.0"),
            },
            timestamp: DualTimestamp::now(),
            ttl_secs: 30,
            pricing_method: PricingMethod::CallSpreadReplication,
            confidence_components: ConfidenceComponents {
                iv_spread: 0.9,
                book_depth: 0.8,
                method_agreement: 0.7,
                solver_convergence: 0.95,
            },
            solver_meta: None,
            iv_spread: 0.03,
            skew_adjustment: -0.005,
            cost_breakdown: CostBreakdown {
                prediction_fee: dec("0.005"),
                options_fee_estimate: dec("0.0003"),
                carry_cost: dec("0.002"),
                prediction_slippage: dec("0.001"),
                options_spread_cost: dec("0.003"),
                liquidity_factor: dec("0.95"),
                total_cost: dec("0.0113"),
            },
            prediction_venue: Venue::Polymarket,
            threshold_status: ThresholdStatus::PassedBoth,
            threshold_value: dec("0.025"),
            threshold_components: None,
        }
    }

    #[tokio::test]
    async fn logger_writes_valid_jsonl_file() {
        let dir = std::env::temp_dir().join("signal_logger_test");
        let _ = fs::remove_dir_all(&dir);

        let mut logger = SignalLogger::new(dir.to_str().unwrap());
        let signal = make_signal();

        logger.log(&signal).await.unwrap();
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

        // Verify parseable as ArbSignal
        let parsed: ArbSignal = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed.event_id, "evt-test-logger");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn logger_appends_multiple_signals() {
        let dir = std::env::temp_dir().join("signal_logger_test_multi");
        let _ = fs::remove_dir_all(&dir);

        let mut logger = SignalLogger::new(dir.to_str().unwrap());
        let signal = make_signal();

        for _ in 0..5 {
            logger.log(&signal).await.unwrap();
        }
        logger.flush().unwrap();

        let today = Utc::now().date_naive();
        let filename = format!("{}.jsonl", today.format("%Y-%m-%d"));
        let path = dir.join(filename);
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 5, "should have 5 lines");

        // Each line is valid JSON
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn logger_creates_directory_if_missing() {
        let dir = std::env::temp_dir().join("signal_logger_deep/nested/dir");
        let _ = fs::remove_dir_all(std::env::temp_dir().join("signal_logger_deep"));

        let mut logger = SignalLogger::new(dir.to_str().unwrap());
        let signal = make_signal();
        logger.log(&signal).await.unwrap();
        logger.flush().unwrap();

        assert!(dir.exists(), "nested directory should be created");

        // Cleanup
        let _ = fs::remove_dir_all(std::env::temp_dir().join("signal_logger_deep"));
    }
}
