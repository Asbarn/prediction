//! Atomic file write utility for crash-safe persistence.
//!
//! Writes content to a temporary file, fsyncs, then renames over the target.
//! On Windows, where rename-over-existing can fail, falls back to
//! remove-then-rename.

use std::io::Write;
use std::path::Path;

/// Write content to a file atomically using write-to-temp-then-rename.
///
/// On Windows, falls back to remove-then-rename if rename-over-existing fails.
/// The temporary file uses a `.tmp` extension alongside the target path.
pub fn atomic_write(target: &Path, content: &[u8]) -> anyhow::Result<()> {
    let tmp = target.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(content)?;
    file.sync_all()?; // fsync for durability before rename

    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!(error = %e, "atomic rename failed, using remove-then-rename fallback");
            let _ = std::fs::remove_file(target);
            std::fs::rename(&tmp, target)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = std::env::temp_dir().join("atomic_write_test_create");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("test_output.json");

        // Clean up from any previous run
        let _ = fs::remove_file(&target);

        let content = b"{\"version\": 1, \"data\": \"hello\"}";
        atomic_write(&target, content).expect("atomic write should succeed");

        let read_back = fs::read(&target).expect("should read file");
        assert_eq!(read_back, content);

        // Verify no temp file left behind
        let tmp = target.with_extension("tmp");
        assert!(!tmp.exists(), "temp file should not remain");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = std::env::temp_dir().join("atomic_write_test_overwrite");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("test_overwrite.json");

        // Write first content
        let content1 = b"first version";
        atomic_write(&target, content1).expect("first write should succeed");
        assert_eq!(fs::read(&target).unwrap(), content1);

        // Overwrite with second content
        let content2 = b"second version -- different length";
        atomic_write(&target, content2).expect("second write should succeed");
        assert_eq!(fs::read(&target).unwrap(), content2);

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }
}
