use std::collections::HashSet;
use std::hash::Hash;
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Notify, RwLock};
use tokio_util::sync::CancellationToken;

use crate::events::registry::EventRegistry;

/// Polymarket subscription carrying both condition_id and token_id.
///
/// Unlike Deribit and Kalshi which use a single string identifier,
/// Polymarket subscriptions require both the condition_id (market)
/// and token_id (outcome) to build subscription messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolymarketSubscription {
    pub condition_id: String,
    pub token_id: String,
}

/// Instruments and event IDs to clean up after unsubscribe.
///
/// Sent via mpsc channel to downstream engines after reconciliation
/// removes instruments from the desired subscription set. Processors
/// use per-venue instrument lists; engines use event_ids.
#[derive(Debug, Clone)]
pub struct CleanupEvent {
    pub deribit_instruments: Vec<String>,
    pub kalshi_tickers: Vec<String>,
    pub polymarket_token_ids: Vec<String>,
    pub derive_instruments: Vec<String>,
    pub event_ids: Vec<String>,
}

/// Sender handles for per-venue instrument watch channels.
///
/// Held by `SubscriptionManager` to push updated instrument lists
/// after reconciliation computes diffs.
pub struct SubscriptionSenders {
    pub deribit: watch::Sender<Vec<String>>,
    pub polymarket: watch::Sender<Vec<PolymarketSubscription>>,
    pub kalshi: watch::Sender<Vec<String>>,
    pub derive: watch::Sender<Vec<String>>,
}

/// Receiver handles for per-venue instrument watch channels.
///
/// Passed to supervisors so they can read the latest instrument list
/// at the top of their reconnect loop.
pub struct SubscriptionReceivers {
    pub deribit: watch::Receiver<Vec<String>>,
    pub polymarket: watch::Receiver<Vec<PolymarketSubscription>>,
    pub kalshi: watch::Receiver<Vec<String>>,
    pub derive: watch::Receiver<Vec<String>>,
}

/// Reconciliation engine that bridges config hot-reload to feed supervisors.
///
/// Computes per-venue instrument diffs when `events.toml` changes, logs
/// structured diff output, and pushes updated instrument lists via watch
/// channels. Only active+approved mappings contribute to the desired
/// subscription set (OPS-02 safety gate).
///
/// # Architecture
///
/// The `SubscriptionManager` awaits `registry_notify` signals from the
/// config reload subscriber. When notified, it reads the registry
/// (which has already been refreshed by the config reload subscriber),
/// computes set differences against the current subscription state,
/// logs structured diffs, and sends updated lists via watch channels.
///
/// The `biased` select ensures cancellation is checked first, and
/// `Notify` coalesces multiple rapid config changes into a single
/// reconciliation pass.
pub struct SubscriptionManager {
    registry: Arc<RwLock<EventRegistry>>,
    registry_notify: Arc<Notify>,
    cancel: CancellationToken,
    deribit_tx: watch::Sender<Vec<String>>,
    polymarket_tx: watch::Sender<Vec<PolymarketSubscription>>,
    kalshi_tx: watch::Sender<Vec<String>>,
    derive_tx: watch::Sender<Vec<String>>,
    current_deribit: HashSet<String>,
    current_polymarket: HashSet<PolymarketSubscription>,
    current_kalshi: HashSet<String>,
    current_derive: HashSet<String>,
    dry_run: bool,
    cleanup_txs: Vec<mpsc::Sender<CleanupEvent>>,
}

impl SubscriptionManager {
    /// Create a new SubscriptionManager.
    ///
    /// The `current_*` sets start empty -- the first reconciliation will
    /// populate them and detect the initial instrument state as "added".
    pub fn new(
        registry: Arc<RwLock<EventRegistry>>,
        registry_notify: Arc<Notify>,
        cancel: CancellationToken,
        senders: SubscriptionSenders,
        dry_run: bool,
        cleanup_txs: Vec<mpsc::Sender<CleanupEvent>>,
    ) -> Self {
        Self {
            registry,
            registry_notify,
            cancel,
            deribit_tx: senders.deribit,
            polymarket_tx: senders.polymarket,
            kalshi_tx: senders.kalshi,
            derive_tx: senders.derive,
            current_deribit: HashSet::new(),
            current_polymarket: HashSet::new(),
            current_kalshi: HashSet::new(),
            current_derive: HashSet::new(),
            dry_run,
            cleanup_txs,
        }
    }

    /// Extract per-venue instrument sets from active_approved() mappings.
    ///
    /// Only active+approved mappings contribute to the desired subscription
    /// set, enforcing the OPS-02 safety gate. Unapproved candidates and
    /// non-Active lifecycle statuses are excluded.
    fn compute_desired_instruments(
        registry: &EventRegistry,
    ) -> (HashSet<String>, HashSet<PolymarketSubscription>, HashSet<String>, HashSet<String>) {
        let mut deribit = HashSet::new();
        let mut polymarket = HashSet::new();
        let mut kalshi = HashSet::new();
        let mut derive = HashSet::new();

        for mapping in registry.active_approved() {
            if let Some(ref d) = mapping.venues.deribit {
                deribit.insert(d.instrument.clone());
            }
            if let Some(ref p) = mapping.venues.polymarket {
                polymarket.insert(PolymarketSubscription {
                    condition_id: p.condition_id.clone(),
                    token_id: p.token_id.clone(),
                });
            }
            if let Some(ref k) = mapping.venues.kalshi {
                kalshi.insert(k.ticker.clone());
            }
            if let Some(ref dr) = mapping.venues.derive {
                derive.insert(dr.instrument.clone());
            }
        }

        (deribit, polymarket, kalshi, derive)
    }

    /// Compute the set difference between current and desired instrument sets.
    ///
    /// Returns `(added, removed)` where added items are in desired but not
    /// current, and removed items are in current but not desired.
    fn compute_diff<T: Clone + Eq + Hash>(
        current: &HashSet<T>,
        desired: &HashSet<T>,
    ) -> (Vec<T>, Vec<T>) {
        let added: Vec<T> = desired.difference(current).cloned().collect();
        let removed: Vec<T> = current.difference(desired).cloned().collect();
        (added, removed)
    }

    /// Reconcile current subscriptions against the registry state.
    ///
    /// Acquires the registry read lock, computes desired instruments,
    /// drops the lock (critical -- avoids holding lock during watch send),
    /// computes per-venue diffs, logs structured output, and sends
    /// updated instrument lists via watch channels if any venue has changes.
    ///
    /// When `dry_run` is true, diffs are logged and internal state is updated
    /// (so subsequent diffs are meaningful), but watch channel sends, cleanup
    /// events, and metrics are all skipped.
    async fn reconcile(&mut self) {
        // Acquire registry read lock and compute desired instruments.
        let reg = self.registry.read().await;
        let (desired_d, desired_p, desired_k, desired_dr) = Self::compute_desired_instruments(&reg);
        // CRITICAL: Drop read lock before watch send to avoid priority inversion
        // with the config reload subscriber's write lock acquisition.
        drop(reg);

        // Compute per-venue diffs.
        let (added_d, removed_d) = Self::compute_diff(&self.current_deribit, &desired_d);
        let (added_p, removed_p) = Self::compute_diff(&self.current_polymarket, &desired_p);
        let (added_k, removed_k) = Self::compute_diff(&self.current_kalshi, &desired_k);

        // Log structured diffs per venue (OBS-03).
        let deribit_changed = !added_d.is_empty() || !removed_d.is_empty();
        let polymarket_changed = !added_p.is_empty() || !removed_p.is_empty();
        let kalshi_changed = !added_k.is_empty() || !removed_k.is_empty();

        if deribit_changed {
            tracing::info!(
                venue = %"deribit",
                added_count = added_d.len(),
                removed_count = removed_d.len(),
                total = desired_d.len(),
                added = ?added_d,
                removed = ?removed_d,
                "subscription reconciliation: diff computed"
            );
        } else {
            tracing::debug!(
                venue = %"deribit",
                total = self.current_deribit.len(),
                "subscription reconciliation: no changes"
            );
        }

        if polymarket_changed {
            // Log token_ids for readability in structured output.
            let added_tokens: Vec<&str> = added_p.iter().map(|s| s.token_id.as_str()).collect();
            let removed_tokens: Vec<&str> = removed_p.iter().map(|s| s.token_id.as_str()).collect();
            tracing::info!(
                venue = %"polymarket",
                added_count = added_p.len(),
                removed_count = removed_p.len(),
                total = desired_p.len(),
                added = ?added_tokens,
                removed = ?removed_tokens,
                "subscription reconciliation: diff computed"
            );
        } else {
            tracing::debug!(
                venue = %"polymarket",
                total = self.current_polymarket.len(),
                "subscription reconciliation: no changes"
            );
        }

        if kalshi_changed {
            tracing::info!(
                venue = %"kalshi",
                added_count = added_k.len(),
                removed_count = removed_k.len(),
                total = desired_k.len(),
                added = ?added_k,
                removed = ?removed_k,
                "subscription reconciliation: diff computed"
            );
        } else {
            tracing::debug!(
                venue = %"kalshi",
                total = self.current_kalshi.len(),
                "subscription reconciliation: no changes"
            );
        }

        // Dry-run guard: update internal state so subsequent diffs are meaningful
        // (Pitfall 3), but skip watch sends, cleanup sends, and metrics.
        if self.dry_run {
            tracing::info!(
                deribit_add = added_d.len(),
                deribit_remove = removed_d.len(),
                polymarket_add = added_p.len(),
                polymarket_remove = removed_p.len(),
                kalshi_add = added_k.len(),
                kalshi_remove = removed_k.len(),
                "DRY RUN: reconciliation would apply these changes"
            );
            // Update internal state so subsequent diffs are meaningful (Pitfall 3)
            self.current_deribit = desired_d;
            self.current_polymarket = desired_p;
            self.current_kalshi = desired_k;
            return;
        }

        // Send updated instrument lists via watch channels only if there are changes.
        if deribit_changed {
            let mut instruments: Vec<String> = desired_d.iter().cloned().collect();
            instruments.sort();
            self.deribit_tx.send_replace(instruments);
        }

        if polymarket_changed {
            let mut subscriptions: Vec<PolymarketSubscription> =
                desired_p.iter().cloned().collect();
            subscriptions.sort_by(|a, b| a.token_id.cmp(&b.token_id));
            self.polymarket_tx.send_replace(subscriptions);
        }

        if kalshi_changed {
            let mut tickers: Vec<String> = desired_k.iter().cloned().collect();
            tickers.sort();
            self.kalshi_tx.send_replace(tickers);
        }

        // If ALL three venues have empty diffs, log at debug level.
        if !deribit_changed && !polymarket_changed && !kalshi_changed {
            tracing::debug!("subscription reconciliation: no changes across all venues");
        }

        // Capture diff lengths before cleanup event consumes the removed vectors.
        let added_d_len = added_d.len();
        let removed_d_len = removed_d.len();
        let added_p_len = added_p.len();
        let removed_p_len = removed_p.len();
        let added_k_len = added_k.len();
        let removed_k_len = removed_k.len();

        // Send cleanup events for removed instruments (SUB-05).
        let has_removals = !removed_d.is_empty() || !removed_p.is_empty() || !removed_k.is_empty();
        if has_removals {
            let cleanup = CleanupEvent {
                deribit_instruments: removed_d,
                kalshi_tickers: removed_k,
                polymarket_token_ids: removed_p.iter().map(|s| s.token_id.clone()).collect(),
                derive_instruments: Vec::new(), // Populated in Phase 32 when Derive is wired to SubscriptionManager
                event_ids: Vec::new(), // Populated by Plan 02 when wiring is complete
            };
            for tx in &self.cleanup_txs {
                if let Err(e) = tx.try_send(cleanup.clone()) {
                    tracing::warn!(error = %e, "cleanup channel send failed (best-effort)");
                }
            }
        }

        // Update current state to the new desired sets.
        self.current_deribit = desired_d;
        self.current_polymarket = desired_p;
        self.current_kalshi = desired_k;

        // Emit subscription metrics (OBS-01: gauges, OBS-02: counters).
        metrics::gauge!("subscription_active", "venue" => "deribit")
            .set(self.current_deribit.len() as f64);
        metrics::gauge!("subscription_active", "venue" => "polymarket")
            .set(self.current_polymarket.len() as f64);
        metrics::gauge!("subscription_active", "venue" => "kalshi")
            .set(self.current_kalshi.len() as f64);

        if deribit_changed {
            metrics::counter!("subscription_activations_total", "venue" => "deribit")
                .increment(added_d_len as u64);
            if removed_d_len > 0 {
                metrics::counter!("subscription_removals_total", "venue" => "deribit")
                    .increment(removed_d_len as u64);
            }
        }
        if polymarket_changed {
            metrics::counter!("subscription_activations_total", "venue" => "polymarket")
                .increment(added_p_len as u64);
            if removed_p_len > 0 {
                metrics::counter!("subscription_removals_total", "venue" => "polymarket")
                    .increment(removed_p_len as u64);
            }
        }
        if kalshi_changed {
            metrics::counter!("subscription_activations_total", "venue" => "kalshi")
                .increment(added_k_len as u64);
            if removed_k_len > 0 {
                metrics::counter!("subscription_removals_total", "venue" => "kalshi")
                    .increment(removed_k_len as u64);
            }
        }
    }

    /// Run the subscription manager event loop.
    ///
    /// Awaits registry change notifications and reconciles subscriptions.
    /// The `biased` select ensures cancellation is checked before
    /// notification, providing clean shutdown behavior.
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    tracing::info!("SubscriptionManager shutting down");
                    break;
                }
                _ = self.registry_notify.notified() => {
                    self.reconcile().await;
                }
            }
        }
    }

    /// Create per-venue watch channels seeded with initial instrument lists.
    ///
    /// Reads the registry to compute the initial desired instruments, then
    /// creates watch channels with those values. This avoids Pitfall 2
    /// (empty initial value causing supervisors to connect with zero
    /// instruments before the first reconciliation runs).
    ///
    /// Returns `(senders, receivers)` -- senders go to `SubscriptionManager::new()`,
    /// receivers go to supervisors (wired in Phase 23).
    pub fn create_channels(registry: &EventRegistry) -> (SubscriptionSenders, SubscriptionReceivers) {
        let (desired_d, desired_p, desired_k, desired_dr) = Self::compute_desired_instruments(registry);

        let mut initial_deribit: Vec<String> = desired_d.into_iter().collect();
        initial_deribit.sort();

        let mut initial_polymarket: Vec<PolymarketSubscription> = desired_p.into_iter().collect();
        initial_polymarket.sort_by(|a, b| a.token_id.cmp(&b.token_id));

        let mut initial_kalshi: Vec<String> = desired_k.into_iter().collect();
        initial_kalshi.sort();

        let mut initial_derive: Vec<String> = desired_dr.into_iter().collect();
        initial_derive.sort();

        let (deribit_tx, deribit_rx) = watch::channel(initial_deribit);
        let (polymarket_tx, polymarket_rx) = watch::channel(initial_polymarket);
        let (kalshi_tx, kalshi_rx) = watch::channel(initial_kalshi);
        let (derive_tx, derive_rx) = watch::channel(initial_derive);

        let senders = SubscriptionSenders {
            deribit: deribit_tx,
            polymarket: polymarket_tx,
            kalshi: kalshi_tx,
            derive: derive_tx,
        };

        let receivers = SubscriptionReceivers {
            deribit: deribit_rx,
            polymarket: polymarket_rx,
            kalshi: kalshi_rx,
            derive: derive_rx,
        };

        (senders, receivers)
    }
}
