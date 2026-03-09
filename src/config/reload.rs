use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;

use super::AppConfig;

/// Config hot-reload via file system watching.
///
/// Uses the `notify` crate to watch a config directory for TOML file changes.
/// When a change is detected, re-parses all config files and distributes the
/// new `AppConfig` via a `tokio::sync::watch` channel. Consumers snapshot
/// the config once per processing cycle for consistency.
///
/// The file watcher runs on a dedicated OS thread (not a tokio task) because
/// `notify` uses blocking OS APIs (inotify on Linux, ReadDirectoryChangesW
/// on Windows, FSEvents on macOS).
pub struct ConfigReloader {
    /// Held to keep the watcher thread's channel open.
    /// The watcher thread owns the Sender; this Receiver keeps us
    /// connected. Dropping this struct is fine -- the watcher thread
    /// will detect the Sender's receivers are gone and exit gracefully.
    _rx: watch::Receiver<AppConfig>,
}

impl ConfigReloader {
    /// Start watching the config directory for TOML file changes.
    ///
    /// Creates a `watch` channel seeded with `initial_config` and spawns
    /// a background OS thread that watches `config_dir` for changes.
    /// When a `.toml` file changes, re-parses all config and sends the
    /// new value through the channel. On parse/validation failure, logs
    /// the error and keeps the previous config.
    ///
    /// Returns `(ConfigReloader, Receiver)`. The `ConfigReloader` must be
    /// held alive to keep the watcher running. The `Receiver` can be cloned
    /// and distributed to consumers.
    ///
    /// # Errors
    ///
    /// Returns an error if the file watcher cannot be created or the
    /// config directory cannot be watched.
    pub fn start(
        config_dir: PathBuf,
        initial_config: AppConfig,
    ) -> anyhow::Result<(Self, watch::Receiver<AppConfig>)> {
        let (config_tx, config_rx) = watch::channel(initial_config);
        let consumer_rx = config_rx.clone();

        // Spawn file watcher on a dedicated OS thread (notify uses blocking APIs)
        let watch_dir = config_dir.clone();
        std::thread::spawn(move || {
            Self::watch_loop(watch_dir, config_tx);
        });

        Ok((Self { _rx: config_rx }, consumer_rx))
    }

    /// Internal watch loop running on a dedicated OS thread.
    ///
    /// Uses `notify_debouncer_mini` with 500ms debounce to avoid
    /// rapid-fire reloads from editor save patterns (tmp file + rename).
    /// Additionally hashes raw file contents to skip reloads when files
    /// haven't actually changed (Docker bind mounts can generate spurious
    /// inotify events).
    fn watch_loop(config_dir: PathBuf, config_tx: watch::Sender<AppConfig>) {
        use notify_debouncer_mini::new_debouncer;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = match new_debouncer(Duration::from_millis(500), tx) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("failed to create file watcher: {e}");
                return;
            }
        };

        if let Err(e) = debouncer
            .watcher()
            .watch(&config_dir, notify::RecursiveMode::NonRecursive)
        {
            tracing::error!("failed to watch config directory: {e}");
            return;
        }

        tracing::debug!(dir = %config_dir.display(), "config file watcher started");

        // Track content hash to skip reloads when files haven't actually changed
        let mut last_hash = Self::hash_config_files(&config_dir);

        loop {
            match rx.recv() {
                Ok(Ok(events)) => {
                    let config_changed = events.iter().any(|e| {
                        e.path
                            .extension()
                            .map_or(false, |ext| ext == "toml")
                    });
                    if config_changed {
                        let current_hash = Self::hash_config_files(&config_dir);
                        if current_hash == last_hash {
                            tracing::trace!("config files unchanged (spurious fs event), skipping reload");
                            continue;
                        }
                        match super::load_config(&config_dir) {
                            Ok(new_config) => {
                                last_hash = current_hash;
                                tracing::info!("config reloaded successfully");
                                let _ = config_tx.send(new_config);
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    "config reload failed, keeping previous"
                                );
                            }
                        }
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "file watch error");
                }
                Err(_) => {
                    // Channel closed -- debouncer dropped
                    tracing::debug!("file watcher channel closed, stopping");
                    break;
                }
            }
        }
    }

    /// Hash the contents of all config TOML files for change detection.
    fn hash_config_files(config_dir: &PathBuf) -> u64 {
        let mut hasher = DefaultHasher::new();
        for name in &["config.toml", "events.toml", "venues.toml"] {
            match std::fs::read(config_dir.join(name)) {
                Ok(bytes) => bytes.hash(&mut hasher),
                Err(_) => 0u8.hash(&mut hasher),
            }
        }
        hasher.finish()
    }
}
