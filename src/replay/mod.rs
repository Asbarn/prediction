//! Multi-venue replay orchestration from recorded JSONL feeds.
//!
//! Loads recordings from a directory structure:
//! ```text
//! recordings/
//! ├── deribit/
//! │   ├── 2026-02-23_12-00.jsonl
//! │   └── 2026-02-23_13-00.jsonl
//! ├── polymarket/
//! │   └── 2026-02-23_12-00.jsonl
//! └── kalshi/
//!     └── 2026-02-23_12-00.jsonl
//! ```
//!
//! Entries across all venues are merged and sorted by `local_ts` for
//! deterministic time-ordered replay. Missing venue directories are
//! skipped with a warning (graceful degradation, RELY-04 pattern).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::config::VenuesConfig;
use crate::events::registry::EventRegistry;
use crate::feed::deribit::normalize::DeribitProcessor;
use crate::feed::mock::replay::ReplayDataSource;
use crate::feed::pipeline::{forward_snapshots, PipelineHandles};
use crate::feed::polymarket::normalize::PolymarketProcessor;
use crate::feed::kalshi::normalize::KalshiProcessor;
use crate::feed::traits::{RawDataSource, RecordLine};
use crate::subscription::CleanupEvent;
use crate::types::{MarketSnapshot, Venue};

/// Fan-in buffer size for the shared multi-venue replay channel.
const FAN_IN_BUFFER: usize = 1024;

/// All venue subdirectory names we scan for recordings.
const VENUE_DIRS: &[(Venue, &str)] = &[
    (Venue::Deribit, "deribit"),
    (Venue::Polymarket, "polymarket"),
    (Venue::Kalshi, "kalshi"),
];

/// A loaded corpus of recorded JSONL lines across multiple venues,
/// sorted by `local_ts` for deterministic replay ordering.
pub struct ReplayCorpus {
    /// All entries merged and sorted by local_ts.
    pub entries: Vec<RecordLine>,
}

impl ReplayCorpus {
    /// Load all JSONL recordings from a directory containing per-venue
    /// subdirectories (`deribit/`, `polymarket/`, `kalshi/`).
    ///
    /// Missing venue directories are skipped with a warning (graceful
    /// degradation per RELY-04). Unparseable lines are skipped with a
    /// warning. Entries are sorted by `local_ts` across all venues.
    pub async fn load_directory(recordings_dir: &Path) -> anyhow::Result<Self> {
        let mut all_entries: Vec<RecordLine> = Vec::new();
        let mut per_venue_count: HashMap<Venue, usize> = HashMap::new();

        for &(venue, dir_name) in VENUE_DIRS {
            let venue_dir = recordings_dir.join(dir_name);

            if !venue_dir.exists() {
                tracing::warn!(
                    venue = %venue,
                    path = %venue_dir.display(),
                    "venue recording directory not found, skipping"
                );
                continue;
            }

            let mut jsonl_files: Vec<PathBuf> = Vec::new();
            let mut read_dir = tokio::fs::read_dir(&venue_dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    jsonl_files.push(path);
                }
            }

            // Sort by filename for chronological order
            jsonl_files.sort();

            let mut venue_count = 0usize;
            for file_path in &jsonl_files {
                let contents = tokio::fs::read_to_string(file_path).await?;
                for (i, line_str) in contents.lines().enumerate() {
                    if line_str.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<RecordLine>(line_str) {
                        Ok(record) => {
                            venue_count += 1;
                            all_entries.push(record);
                        }
                        Err(e) => {
                            tracing::warn!(
                                file = %file_path.display(),
                                line = i + 1,
                                error = %e,
                                "skipping unparseable JSONL line"
                            );
                        }
                    }
                }
            }

            per_venue_count.insert(venue, venue_count);
            tracing::info!(
                venue = %venue,
                files = jsonl_files.len(),
                entries = venue_count,
                "loaded venue recordings"
            );
        }

        // Sort all entries by local_ts across all venues
        all_entries.sort_by_key(|e| e.local_ts);

        // Log summary
        let total = all_entries.len();
        if total > 0 {
            let first_ts = all_entries.first().map(|e| e.local_ts);
            let last_ts = all_entries.last().map(|e| e.local_ts);
            tracing::info!(
                total_entries = total,
                first_ts = ?first_ts,
                last_ts = ?last_ts,
                per_venue = ?per_venue_count,
                "replay corpus loaded"
            );
        } else {
            tracing::warn!("replay corpus is empty -- no JSONL entries found");
        }

        Ok(Self {
            entries: all_entries,
        })
    }

    /// Returns the set of venues that have data in this corpus.
    pub fn venues(&self) -> HashSet<Venue> {
        self.entries.iter().map(|e| e.venue).collect()
    }
}

/// Run the multi-venue replay pipeline.
///
/// Loads all recordings from `recordings_dir`, groups entries by venue,
/// creates per-venue `ReplayDataSource` + processor pipelines, and
/// returns `PipelineHandles` with a shared fan-in channel.
///
/// The `speed` parameter controls replay speed: 0.0 = instant, 1.0 = real-time.
pub async fn run_replay_pipeline(
    recordings_dir: PathBuf,
    config: &VenuesConfig,
    speed: f64,
    cancel: CancellationToken,
    event_registry: Option<Arc<RwLock<EventRegistry>>>,
) -> anyhow::Result<PipelineHandles> {
    let corpus = ReplayCorpus::load_directory(&recordings_dir).await?;

    if corpus.entries.is_empty() {
        tracing::warn!("no replay entries found, pipeline will produce no data");
        let (_tx, rx) = mpsc::channel::<MarketSnapshot>(1);
        return Ok(PipelineHandles {
            snapshot_rx: rx,
            venue_health: vec![],
            venue_rate_limiters: std::collections::HashMap::new(),
            subscription_rx: None,
            cleanup_txs: Vec::new(),
            engine_cleanup_rxs: None,
        });
    }

    // Group entries by venue
    let mut by_venue: HashMap<Venue, Vec<RecordLine>> = HashMap::new();
    for entry in corpus.entries {
        by_venue.entry(entry.venue).or_default().push(entry);
    }

    let (snapshot_tx, snapshot_rx) = mpsc::channel::<MarketSnapshot>(FAN_IN_BUFFER);

    for (venue, records) in by_venue {
        let entry_count = records.len();
        let venue_cancel = cancel.child_token();

        // Create ReplayDataSource from records (no temp files needed)
        let source = ReplayDataSource::from_records(records, speed, venue_cancel.clone());
        let raw_rx = source.start().await?;

        // Create the appropriate per-venue processor
        let (venue_snapshot_rx, processor_task) = match venue {
            Venue::Deribit => {
                // Create cleanup channel; sender is dropped immediately so
                // receiver returns None in the select branch (no cleanup in replay).
                let (_tx, cleanup_rx) = mpsc::channel::<CleanupEvent>(1);
                let (processor, rx) = DeribitProcessor::new(
                    raw_rx,
                    None, // no recording during replay
                    venue_cancel.clone(),
                    config.deribit.staleness_threshold_ms,
                    cleanup_rx,
                );
                (rx, tokio::spawn(processor.run()))
            }
            Venue::Polymarket => {
                let (processor, rx) = PolymarketProcessor::new(
                    raw_rx,
                    None,
                    venue_cancel.clone(),
                    config.polymarket.staleness_threshold_ms,
                );
                (rx, tokio::spawn(processor.run()))
            }
            Venue::Kalshi => {
                // Create cleanup channel; sender is dropped immediately.
                let (_tx, cleanup_rx) = mpsc::channel::<CleanupEvent>(1);
                let (processor, rx) = KalshiProcessor::new(
                    raw_rx,
                    None,
                    venue_cancel.clone(),
                    config.kalshi.staleness_threshold_ms,
                    cleanup_rx,
                );
                (rx, tokio::spawn(processor.run()))
            }
            Venue::Derive => {
                anyhow::bail!("Derive venue replay not yet implemented (planned for v1.5 Phase 31)");
            }
        };

        // Hold the processor task handle to suppress unused warnings
        drop(processor_task);

        // Forward from per-venue processor to shared fan-in
        let fan_in_tx = snapshot_tx.clone();
        tokio::spawn(forward_snapshots(
            venue_snapshot_rx,
            fan_in_tx,
            venue,
            venue_cancel,
            None,
            event_registry.clone(),
        ));

        tracing::info!(
            venue = %venue,
            entries = entry_count,
            speed = speed,
            "replay pipeline started for venue"
        );
    }

    // Drop the original sender so channel closes when all venues finish
    drop(snapshot_tx);

    Ok(PipelineHandles {
        snapshot_rx,
        venue_health: vec![],
        venue_rate_limiters: std::collections::HashMap::new(),
        subscription_rx: None,
        cleanup_txs: Vec::new(),
        engine_cleanup_rxs: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_record(venue: Venue, raw: &str, ts: chrono::DateTime<chrono::Utc>) -> RecordLine {
        RecordLine {
            raw: raw.to_string(),
            local_ts: ts,
            venue,
            channel: format!("test.{}", venue),
            instrument: Some("TEST-INST".to_string()),
        }
    }

    async fn create_venue_dir(
        base: &Path,
        venue_name: &str,
        records: &[RecordLine],
    ) -> PathBuf {
        let dir = base.join(venue_name);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let file = dir.join("test_recording.jsonl");
        let mut content = String::new();
        for record in records {
            content.push_str(&serde_json::to_string(record).unwrap());
            content.push('\n');
        }
        tokio::fs::write(&file, &content).await.unwrap();
        dir
    }

    #[tokio::test]
    async fn load_directory_multi_venue_sorted() {
        let tmp = std::env::temp_dir().join(format!(
            "replay_test_multi_{}",
            uuid::Uuid::now_v7()
        ));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let base_ts = Utc::now();

        // Deribit entries at t+0 and t+200ms
        let deribit_records = vec![
            make_record(
                Venue::Deribit,
                r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"test","data":{}}}"#,
                base_ts,
            ),
            make_record(
                Venue::Deribit,
                r#"{"jsonrpc":"2.0","method":"subscription","params":{"channel":"test","data":{}}}"#,
                base_ts + chrono::Duration::milliseconds(200),
            ),
        ];

        // Polymarket entries at t+100ms and t+300ms
        let poly_records = vec![
            make_record(
                Venue::Polymarket,
                r#"[{"event":"price_change","market":"0x123","price":"0.50"}]"#,
                base_ts + chrono::Duration::milliseconds(100),
            ),
            make_record(
                Venue::Polymarket,
                r#"[{"event":"price_change","market":"0x123","price":"0.55"}]"#,
                base_ts + chrono::Duration::milliseconds(300),
            ),
        ];

        create_venue_dir(&tmp, "deribit", &deribit_records).await;
        create_venue_dir(&tmp, "polymarket", &poly_records).await;
        // No kalshi directory -- should degrade gracefully

        let corpus = ReplayCorpus::load_directory(&tmp).await.unwrap();

        assert_eq!(corpus.entries.len(), 4, "should load all 4 entries");

        // Verify sorted by local_ts across venues
        for pair in corpus.entries.windows(2) {
            assert!(
                pair[0].local_ts <= pair[1].local_ts,
                "entries should be sorted by local_ts"
            );
        }

        // Verify interleaved venue order: deribit, poly, deribit, poly
        assert_eq!(corpus.entries[0].venue, Venue::Deribit);
        assert_eq!(corpus.entries[1].venue, Venue::Polymarket);
        assert_eq!(corpus.entries[2].venue, Venue::Deribit);
        assert_eq!(corpus.entries[3].venue, Venue::Polymarket);

        // Verify venues() returns both
        let venues = corpus.venues();
        assert!(venues.contains(&Venue::Deribit));
        assert!(venues.contains(&Venue::Polymarket));
        assert!(!venues.contains(&Venue::Kalshi), "Kalshi had no directory");

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn load_directory_missing_all_venues() {
        let tmp = std::env::temp_dir().join(format!(
            "replay_test_empty_{}",
            uuid::Uuid::now_v7()
        ));
        tokio::fs::create_dir_all(&tmp).await.unwrap();
        // No venue subdirectories at all

        let corpus = ReplayCorpus::load_directory(&tmp).await.unwrap();
        assert!(corpus.entries.is_empty(), "should be empty with no venue dirs");
        assert!(corpus.venues().is_empty());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn load_directory_skips_bad_lines() {
        let tmp = std::env::temp_dir().join(format!(
            "replay_test_bad_lines_{}",
            uuid::Uuid::now_v7()
        ));
        let dir = tmp.join("deribit");
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let base_ts = Utc::now();
        let good_record = make_record(Venue::Deribit, r#"{"test":"good"}"#, base_ts);
        let content = format!(
            "{}\nthis is not valid json\n{}\n",
            serde_json::to_string(&good_record).unwrap(),
            serde_json::to_string(&good_record).unwrap(),
        );
        tokio::fs::write(dir.join("test.jsonl"), &content).await.unwrap();

        let corpus = ReplayCorpus::load_directory(&tmp).await.unwrap();
        assert_eq!(corpus.entries.len(), 2, "should have 2 good entries, skipping bad line");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
