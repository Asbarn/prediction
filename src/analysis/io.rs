use anyhow::bail;
use chrono::NaiveDate;
use serde::de::DeserializeOwned;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// A date range for filtering JSONL log files.
#[derive(Debug, Clone)]
pub struct DateRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl DateRange {
    /// Resolve a date range from CLI arguments.
    ///
    /// Accepted combinations:
    /// - `(Some(from), Some(to), None)` -- explicit range
    /// - `(Some(from), None, None)` -- from date to today
    /// - `(None, None, Some(n))` -- last N days ending today
    /// - `(None, None, None)` -- error: no range specified
    /// - Any mix of `--from`/`--to` with `--last` -- error: conflicting
    pub fn from_args(
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
        last: Option<u32>,
    ) -> anyhow::Result<Self> {
        let today = chrono::Utc::now().date_naive();
        match (from, to, last) {
            (Some(f), Some(t), None) => Ok(Self { from: f, to: t }),
            (Some(f), None, None) => Ok(Self { from: f, to: today }),
            (None, None, Some(n)) => Ok(Self {
                from: today - chrono::Duration::days(n as i64),
                to: today,
            }),
            (None, None, None) => bail!("Specify --from/--to or --last N"),
            _ => bail!("Use --from/--to OR --last, not both"),
        }
    }

    /// Enumerate JSONL files in `dir` for each date in the range.
    ///
    /// Constructs filenames as `{YYYY-MM-DD}.jsonl` (no prefix).
    /// Only returns paths where the file actually exists.
    pub fn files_in_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut date = self.from;
        while date <= self.to {
            let path = dir.join(format!("{}.jsonl", date.format("%Y-%m-%d")));
            if path.exists() {
                files.push(path);
            }
            date += chrono::Duration::days(1);
        }
        files
    }

    /// Enumerate JSONL files in `dir` with a filename prefix for each date in the range.
    ///
    /// Constructs filenames as `{prefix}{YYYY-MM-DD}.jsonl`.
    /// Handles settlement logs (`settlements-`) and paper trade logs (`trades-`).
    /// Only returns paths where the file actually exists.
    pub fn files_in_dir_prefixed(&self, dir: &Path, prefix: &str) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut date = self.from;
        while date <= self.to {
            let path = dir.join(format!("{prefix}{}.jsonl", date.format("%Y-%m-%d")));
            if path.exists() {
                files.push(path);
            }
            date += chrono::Duration::days(1);
        }
        files
    }
}

/// Split a DateRange chronologically into train and test portions.
///
/// The first `(1 - test_fraction)` of days form the training set; the remainder
/// forms the test set. The two ranges are contiguous with no gap or overlap.
///
/// `test_fraction` is clamped to `[0.0, 1.0]`.
pub fn train_test_split(range: &DateRange, test_fraction: f64) -> (DateRange, DateRange) {
    let test_fraction = test_fraction.clamp(0.0, 1.0);
    let total_days = (range.to - range.from).num_days();
    if total_days <= 0 {
        // Degenerate: return full range as train, zero-width test
        return (
            range.clone(),
            DateRange {
                from: range.to,
                to: range.to,
            },
        );
    }

    let train_days = ((total_days as f64) * (1.0 - test_fraction)).floor() as i64;
    let split_date = range.from + chrono::Duration::days(train_days);

    let train = DateRange {
        from: range.from,
        to: split_date - chrono::Duration::days(1),
    };
    let test = DateRange {
        from: split_date,
        to: range.to,
    };

    (train, test)
}

impl fmt::Display for DateRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.from, self.to)
    }
}

/// Result of a tolerant JSONL loading operation.
pub struct LoadResult<T> {
    pub records: Vec<T>,
    pub errors: usize,
    pub files_loaded: usize,
    pub files_missing: usize,
}

/// Load and deserialize JSONL records from a list of files.
///
/// Tolerant: skips malformed lines (incrementing `errors`), skips missing files
/// (incrementing `files_missing`), and never aborts. Empty/whitespace-only lines
/// are silently skipped without counting as errors.
pub fn load_jsonl<T: DeserializeOwned>(files: &[PathBuf]) -> LoadResult<T> {
    let mut result = LoadResult {
        records: Vec::new(),
        errors: 0,
        files_loaded: 0,
        files_missing: 0,
    };

    for path in files {
        if !path.exists() {
            result.files_missing += 1;
            continue;
        }

        match File::open(path) {
            Ok(file) => {
                result.files_loaded += 1;
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    match line {
                        Ok(text) if text.trim().is_empty() => continue,
                        Ok(text) => match serde_json::from_str::<T>(&text) {
                            Ok(val) => result.records.push(val),
                            Err(_) => result.errors += 1,
                        },
                        Err(_) => result.errors += 1,
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to open {}: {e}", path.display());
                result.errors += 1;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde::Deserialize;
    use std::fs;

    #[test]
    fn from_args_explicit_range() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let range = DateRange::from_args(Some(from), Some(to), None).unwrap();
        assert_eq!(range.from, from);
        assert_eq!(range.to, to);
    }

    #[test]
    fn from_args_last_n_days() {
        let range = DateRange::from_args(None, None, Some(7)).unwrap();
        let today = chrono::Utc::now().date_naive();
        assert_eq!(range.to, today);
        assert_eq!(range.from, today - chrono::Duration::days(7));
    }

    #[test]
    fn from_args_no_args_returns_err() {
        let result = DateRange::from_args(None, None, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Specify --from/--to or --last N")
        );
    }

    #[test]
    fn from_args_conflicting_returns_err() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let result = DateRange::from_args(Some(from), None, Some(7));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Use --from/--to OR --last, not both")
        );
    }

    #[test]
    fn from_args_from_only_defaults_to_today() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let range = DateRange::from_args(Some(from), None, None).unwrap();
        let today = chrono::Utc::now().date_naive();
        assert_eq!(range.from, from);
        assert_eq!(range.to, today);
    }

    #[test]
    fn display_format() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        };
        assert_eq!(format!("{range}"), "2026-01-01 to 2026-01-31");
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestRecord {
        name: String,
        value: i32,
    }

    #[test]
    fn load_jsonl_tolerant_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.jsonl");
        fs::write(
            &file_path,
            r#"{"name":"a","value":1}
{"name":"b","value":2}
not valid json

{"name":"c","value":3}
"#,
        )
        .unwrap();

        let result = load_jsonl::<TestRecord>(&[file_path]);
        assert_eq!(result.records.len(), 3, "should parse 3 valid records");
        assert_eq!(result.errors, 1, "should count 1 malformed line");
        assert_eq!(result.files_loaded, 1);
        assert_eq!(result.files_missing, 0);

        assert_eq!(result.records[0].name, "a");
        assert_eq!(result.records[1].name, "b");
        assert_eq!(result.records[2].name, "c");
    }

    #[test]
    fn load_jsonl_missing_file() {
        let missing = PathBuf::from("/nonexistent/file.jsonl");
        let result = load_jsonl::<TestRecord>(&[missing]);
        assert_eq!(result.records.len(), 0);
        assert_eq!(result.files_missing, 1);
        assert_eq!(result.files_loaded, 0);
    }

    #[test]
    fn files_in_dir_returns_only_existing() {
        let dir = tempfile::tempdir().unwrap();

        // Create files for Jan 1 and Jan 3, but not Jan 2
        fs::write(dir.path().join("2026-01-01.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("2026-01-03.jsonl"), "{}").unwrap();

        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
        };

        let files = range.files_in_dir(dir.path());
        assert_eq!(files.len(), 2, "should find 2 existing files (skip Jan 2)");
        assert!(files[0].ends_with("2026-01-01.jsonl"));
        assert!(files[1].ends_with("2026-01-03.jsonl"));
    }

    #[test]
    fn files_in_dir_prefixed_returns_only_existing() {
        let dir = tempfile::tempdir().unwrap();

        fs::write(dir.path().join("settlements-2026-01-01.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("settlements-2026-01-02.jsonl"), "{}").unwrap();

        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
        };

        let files = range.files_in_dir_prefixed(dir.path(), "settlements-");
        assert_eq!(files.len(), 2, "should find 2 prefixed files (skip Jan 3)");
        assert!(files[0].ends_with("settlements-2026-01-01.jsonl"));
        assert!(files[1].ends_with("settlements-2026-01-02.jsonl"));
    }

    // -- Train/test split tests --

    #[test]
    fn split_10_days_30_percent_test() {
        // Jan 1-10: total_days = 9. train_days = floor(9 * 0.7) = 6, split at Jan 7
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
        };
        let (train, test) = super::train_test_split(&range, 0.3);

        // Train: Jan 1 - Jan 6 (6 days inclusive)
        assert_eq!(train.from, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(train.to, NaiveDate::from_ymd_opt(2026, 1, 6).unwrap());
        // Test: Jan 7 - Jan 10 (4 days inclusive)
        assert_eq!(test.from, NaiveDate::from_ymd_opt(2026, 1, 7).unwrap());
        assert_eq!(test.to, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
    }

    #[test]
    fn split_10_days_50_percent_test() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
        };
        let (train, test) = super::train_test_split(&range, 0.5);

        // train_days = floor(9 * 0.5) = 4, split at Jan 5 (0-indexed from Jan 1)
        // Actually: total_days = 9, train_days = floor(9*0.5) = 4
        // split_date = Jan 1 + 4 = Jan 5
        // Train: Jan 1 - Jan 4, Test: Jan 5 - Jan 10
        assert_eq!(train.from, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(test.to, NaiveDate::from_ymd_opt(2026, 1, 10).unwrap());
        // Contiguous: train.to + 1 == test.from
        assert_eq!(
            train.to + chrono::Duration::days(1),
            test.from,
            "Train and test should be contiguous"
        );
    }

    #[test]
    fn split_1_day_range() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        };
        let (train, test) = super::train_test_split(&range, 0.3);
        // total_days = 0 -> degenerate case
        assert_eq!(train.from, range.from);
        assert_eq!(train.to, range.to);
        assert_eq!(test.from, range.to);
        assert_eq!(test.to, range.to);
    }

    #[test]
    fn split_fraction_zero_all_training() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
        };
        let (train, test) = super::train_test_split(&range, 0.0);
        // train_days = floor(9 * 1.0) = 9, split at Jan 10
        // Train: Jan 1 - Jan 9, Test: Jan 10 - Jan 10
        assert_eq!(train.from, range.from);
        assert_eq!(test.to, range.to);
    }

    #[test]
    fn split_fraction_one_all_testing() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
        };
        let (_train, test) = super::train_test_split(&range, 1.0);
        // train_days = floor(9 * 0.0) = 0, split at Jan 1
        // Test covers the full range
        assert_eq!(test.from, range.from);
        assert_eq!(test.to, range.to);
    }

    #[test]
    fn split_contiguous_no_gap() {
        let range = DateRange {
            from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 20).unwrap(),
        };
        let (train, test) = super::train_test_split(&range, 0.3);
        assert_eq!(
            train.to + chrono::Duration::days(1),
            test.from,
            "No gap: train.to + 1 day == test.from"
        );
    }
}
