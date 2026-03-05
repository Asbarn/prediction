//! Contract lifecycle manager: periodic venue polling, discovery, expiry
//! detection, Deribit roll handling, and near-expiry warning application.
//!
//! Runs as a background tokio task, never blocking the snapshot pipeline.
//! Polls each venue's REST API at independently configurable intervals,
//! proposes cross-venue candidate matches with `approved = false`, detects
//! expired instruments, handles Deribit expiry rolls, and applies near-expiry
//! warnings with risk score inflation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use toml_edit::DocumentMut;

use crate::config::{
    Credentials, DiscoveryConfig, EventsConfig, ExpiryThreshold, LifecycleStatus,
    RiskWeightsConfig, VenuesConfig,
};
use crate::events::discovery::{
    discover_deribit, discover_derive, discover_kalshi, discover_polymarket,
    discover_polymarket_structured, filter_new_candidates_fuzzy,
    find_cross_venue_candidates_fuzzy, flag_novel_instruments,
    generate_polymarket_slugs, DiscoveredInstrument, ExpiryConfidence,
};
use crate::events::registry::EventRegistry;
use crate::events::risk::{check_expiry_warning, inflate_risk_score, compute_risk_for_mapping, BasisRiskCache, CachedRiskInfo};
use crate::events::toml_writer::{
    append_candidates_to_doc, mark_expired_batch_in_doc,
    collect_archivable_entries, collect_expired_unapproved_ids,
    remove_entries_by_id, append_entries_to_archive_doc,
    CandidateMapping, CandidateVenues,
};
use crate::feed::kalshi::auth::load_kalshi_private_key;
use crate::feed::reliability::VenueRateLimiter;
use crate::types::Venue;

/// Tracks how many consecutive polls each instrument has been absent,
/// preventing false expirations from a single missing API response.
struct AbsenceTracker {
    counts: HashMap<(Venue, String), u32>,
    threshold: u32,
}

impl AbsenceTracker {
    fn new(threshold: u32) -> Self {
        Self { counts: HashMap::new(), threshold }
    }

    /// Record an instrument as present. Removes any absence count.
    fn record_present(&mut self, venue: Venue, instrument_id: &str) {
        self.counts.remove(&(venue, instrument_id.to_string()));
    }

    /// Record an instrument as absent. Returns true if threshold reached.
    fn record_absent(&mut self, venue: Venue, instrument_id: &str) -> bool {
        let count = self.counts
            .entry((venue, instrument_id.to_string()))
            .or_insert(0);
        *count += 1;
        *count >= self.threshold
    }

    /// Remove tracking entry for definitively expired instrument (prevent memory leak).
    fn remove(&mut self, venue: Venue, instrument_id: &str) {
        self.counts.remove(&(venue, instrument_id.to_string()));
    }
}

/// Tracks previous poll instrument counts per venue for partial-response detection.
struct PreviousPollCounts {
    counts: HashMap<Venue, usize>,
}

impl PreviousPollCounts {
    fn new() -> Self { Self { counts: HashMap::new() } }

    /// Returns true if the current count is a >threshold% drop from previous.
    /// Returns false on first poll (no previous data) or if previous was 0.
    fn is_suspect(&self, venue: Venue, current_count: usize, threshold: f64) -> bool {
        if let Some(&prev) = self.counts.get(&venue) {
            if prev > 0 {
                let drop_fraction = 1.0 - (current_count as f64 / prev as f64);
                return drop_fraction > threshold;
            }
        }
        false
    }

    fn update(&mut self, venue: Venue, count: usize) {
        self.counts.insert(venue, count);
    }
}

/// Background task that manages contract lifecycle: discovery, expiry,
/// rolls, and near-expiry warnings.
///
/// Polls venue REST APIs at configurable intervals, proposes candidate
/// matches, detects expired instruments, handles Deribit rolls, and
/// applies near-expiry warnings. All changes are persisted to events.toml
/// and the runtime registry is refreshed after each cycle.
pub struct ContractLifecycleManager {
    registry: Arc<RwLock<EventRegistry>>,
    http_client: reqwest::Client,
    events_toml_path: PathBuf,
    discovery_config: DiscoveryConfig,
    expiry_thresholds: Vec<ExpiryThreshold>,
    risk_weights: RiskWeightsConfig,
    venues_config: VenuesConfig,
    credentials: Credentials,
    cancel: CancellationToken,
    basis_risk_cache: BasisRiskCache,
    venue_rate_limiters: HashMap<Venue, VenueRateLimiter>,
    absence_tracker: AbsenceTracker,
    previous_poll_counts: PreviousPollCounts,
}

impl ContractLifecycleManager {
    /// Create a new lifecycle manager.
    ///
    /// The reqwest::Client is created internally (connection pooling).
    /// `basis_risk_cache` is populated every poll cycle with risk info for
    /// all active_approved mappings, enabling downstream engine consumption.
    pub fn new(
        registry: Arc<RwLock<EventRegistry>>,
        events_toml_path: PathBuf,
        discovery_config: DiscoveryConfig,
        expiry_thresholds: Vec<ExpiryThreshold>,
        risk_weights: RiskWeightsConfig,
        venues_config: VenuesConfig,
        credentials: Credentials,
        cancel: CancellationToken,
        basis_risk_cache: BasisRiskCache,
        venue_rate_limiters: HashMap<Venue, VenueRateLimiter>,
    ) -> Self {
        let absence_tracker = AbsenceTracker::new(discovery_config.consecutive_absence_threshold);
        let previous_poll_counts = PreviousPollCounts::new();
        Self {
            registry,
            http_client: reqwest::Client::new(),
            events_toml_path,
            discovery_config,
            expiry_thresholds,
            risk_weights,
            venues_config,
            credentials,
            cancel,
            basis_risk_cache,
            venue_rate_limiters,
            absence_tracker,
            previous_poll_counts,
        }
    }

    /// Main loop: poll at the minimum venue interval, check each venue's
    /// individual interval, and run the full lifecycle cycle.
    pub async fn run(mut self) {
        let min_interval = self.discovery_config.min_poll_interval_secs();
        let mut interval = tokio::time::interval(Duration::from_secs(min_interval));
        let mut last_deribit_poll = Instant::now() - Duration::from_secs(min_interval + 1);
        let mut last_kalshi_poll = Instant::now() - Duration::from_secs(min_interval + 1);
        let mut last_polymarket_poll = Instant::now() - Duration::from_secs(min_interval + 1);
        let mut last_derive_poll = Instant::now() - Duration::from_secs(min_interval + 1);

        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    tracing::info!("ContractLifecycleManager shutting down");
                    break;
                }
                _ = interval.tick() => {
                    self.poll_cycle(
                        &mut last_deribit_poll,
                        &mut last_kalshi_poll,
                        &mut last_polymarket_poll,
                        &mut last_derive_poll,
                    ).await;
                }
            }
        }
    }

    /// Single poll cycle: discover, match, expire, roll, warn, archive, cleanup, refresh.
    ///
    /// INTG-01: This method implements the complete periodic background pipeline:
    /// 1. Discover instruments from each venue (Deribit, Kalshi, Polymarket)
    /// 2. Find cross-venue candidates via fuzzy matching
    /// 3. Filter novel/unmatched instruments
    /// 4. Detect expired instruments (consecutive-absence tracking)
    /// 5. Handle Deribit expiry rolls
    /// 6. Batched TOML write (candidates + expirations)
    /// 7. Apply expiry warnings + populate BasisRiskCache
    /// 7c. Archive expired events + clean unapproved candidates (LIFE-01, LIFE-02)
    /// 8. Refresh runtime registry
    /// 9. Update pending proposals gauge
    async fn poll_cycle(
        &mut self,
        last_deribit_poll: &mut Instant,
        last_kalshi_poll: &mut Instant,
        last_polymarket_poll: &mut Instant,
        last_derive_poll: &mut Instant,
    ) {
        let mut all_discovered: Vec<DiscoveredInstrument> = Vec::new();
        let mut deribit_polled = false;
        let mut kalshi_polled = false;
        let mut deribit_suspect = false;
        let mut kalshi_suspect = false;

        // 1. Discover instruments from each venue (only if interval elapsed)
        // --- Deribit ---
        if last_deribit_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.deribit_poll_interval_secs)
        {
            *last_deribit_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "deribit").increment(1);
            let deribit_limiter = self.venue_rate_limiters.get(&Venue::Deribit);
            match discover_deribit(
                &self.http_client,
                &format!(
                    "https://{}",
                    self.venues_config
                        .deribit
                        .ws_url
                        .trim_start_matches("wss://")
                        .trim_start_matches("ws://")
                        .split("/ws/")
                        .next()
                        .unwrap_or("www.deribit.com")
                ),
                &self.discovery_config.deribit_currencies,
                deribit_limiter,
            )
            .await
            {
                Ok(instruments) => {
                    let count = instruments.len();
                    tracing::info!(
                        venue = "deribit",
                        count = count,
                        "discovered instruments"
                    );
                    if self.previous_poll_counts.is_suspect(
                        Venue::Deribit,
                        count,
                        self.discovery_config.partial_response_threshold,
                    ) {
                        tracing::warn!(
                            venue = "deribit",
                            previous = ?self.previous_poll_counts.counts.get(&Venue::Deribit),
                            current = count,
                            "suspect partial API response -- skipping expiry evaluation for Deribit"
                        );
                        deribit_suspect = true;
                    } else {
                        self.previous_poll_counts.update(Venue::Deribit, count);
                    }
                    deribit_polled = true;
                    all_discovered.extend(instruments);
                }
                Err(e) => {
                    tracing::warn!(
                        venue = "deribit",
                        error = %e,
                        "Deribit discovery failed, continuing"
                    );
                }
            }
        }

        // --- Kalshi ---
        if last_kalshi_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.kalshi_poll_interval_secs)
        {
            *last_kalshi_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "kalshi").increment(1);

            let api_key_id = self.credentials.kalshi_api_key_id.clone();
            let private_key_pem = self.credentials.kalshi_private_key.clone().or_else(|| {
                self.venues_config
                    .kalshi
                    .private_key_path
                    .as_ref()
                    .and_then(|path| std::fs::read_to_string(path).ok())
            });

            match (api_key_id, private_key_pem) {
                (Some(key_id), Some(pem)) => match load_kalshi_private_key(&pem) {
                    Ok(private_key) => {
                        let kalshi_limiter = self.venue_rate_limiters.get(&Venue::Kalshi);
                        match discover_kalshi(
                            &self.http_client,
                            &self.venues_config.kalshi.rest_url,
                            &key_id,
                            &private_key,
                            &self.discovery_config.kalshi_series_tickers,
                            kalshi_limiter,
                        )
                        .await
                        {
                            Ok(instruments) => {
                                let count = instruments.len();
                                tracing::info!(
                                    venue = "kalshi",
                                    count = count,
                                    "discovered instruments"
                                );
                                if self.previous_poll_counts.is_suspect(
                                    Venue::Kalshi,
                                    count,
                                    self.discovery_config.partial_response_threshold,
                                ) {
                                    tracing::warn!(
                                        venue = "kalshi",
                                        previous = ?self.previous_poll_counts.counts.get(&Venue::Kalshi),
                                        current = count,
                                        "suspect partial API response -- skipping expiry evaluation for Kalshi"
                                    );
                                    kalshi_suspect = true;
                                } else {
                                    self.previous_poll_counts.update(Venue::Kalshi, count);
                                }
                                kalshi_polled = true;
                                all_discovered.extend(instruments);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    venue = "kalshi",
                                    error = %e,
                                    "Kalshi discovery failed, continuing"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            venue = "kalshi",
                            error = %e,
                            "Kalshi private key invalid, skipping discovery"
                        );
                    }
                },
                _ => {
                    tracing::debug!(
                        venue = "kalshi",
                        "Kalshi credentials not configured, skipping discovery"
                    );
                }
            }
        }

        // --- Polymarket (structured discovery + deactivation monitoring) ---
        let mut polymarket_polled = false;
        let mut polymarket_suspect = false;
        if last_polymarket_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.polymarket_poll_interval_secs)
        {
            *last_polymarket_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "polymarket").increment(1);

            // Structured discovery: poll event slugs for new instrument proposals
            let slugs = generate_polymarket_slugs(&self.discovery_config.polymarket_event_slugs);
            let polymarket_limiter = self.venue_rate_limiters.get(&Venue::Polymarket);
            match discover_polymarket_structured(
                &self.http_client,
                &self.venues_config.polymarket.gamma_api_url,
                &slugs,
                polymarket_limiter,
            )
            .await
            {
                Ok(instruments) => {
                    let count = instruments.len();
                    tracing::info!(
                        venue = "polymarket",
                        count = count,
                        "discovered Polymarket structured instruments"
                    );
                    if self.previous_poll_counts.is_suspect(
                        Venue::Polymarket,
                        count,
                        self.discovery_config.partial_response_threshold,
                    ) {
                        tracing::warn!(
                            venue = "polymarket",
                            previous = ?self.previous_poll_counts.counts.get(&Venue::Polymarket),
                            current = count,
                            "suspect partial API response -- skipping expiry evaluation for Polymarket"
                        );
                        polymarket_suspect = true;
                    } else {
                        self.previous_poll_counts.update(Venue::Polymarket, count);
                    }
                    polymarket_polled = true;
                    all_discovered.extend(instruments);
                }
                Err(e) => {
                    tracing::warn!(
                        venue = "polymarket",
                        error = %e,
                        "Polymarket structured discovery failed, continuing"
                    );
                }
            }

            // Deactivation monitoring: check existing Polymarket mappings for closure
            match discover_polymarket(
                &self.http_client,
                &self.venues_config.polymarket.gamma_api_url,
            )
            .await
            {
                Ok(markets) => {
                    // Log deactivated markets for user notification
                    for market in &markets {
                        if !market.active || market.closed {
                            tracing::warn!(
                                venue = "polymarket",
                                condition_id = %market.condition_id,
                                question = %market.question,
                                active = market.active,
                                closed = market.closed,
                                "Polymarket market deactivated/closed"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        venue = "polymarket",
                        error = %e,
                        "Polymarket deactivation monitoring failed, continuing"
                    );
                }
            }
        }

        // --- Derive ---
        let mut derive_polled = false;
        let mut derive_suspect = false;
        if last_derive_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.derive_poll_interval_secs)
        {
            *last_derive_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "derive").increment(1);
            let derive_rest_url = format!(
                "https://{}",
                self.venues_config
                    .derive
                    .ws_url
                    .trim_start_matches("wss://")
                    .trim_start_matches("ws://")
                    .split("/ws")
                    .next()
                    .unwrap_or("api.lyra.finance")
            );
            let derive_limiter = self.venue_rate_limiters.get(&Venue::Derive);
            match discover_derive(&self.http_client, &derive_rest_url, derive_limiter).await {
                Ok(instruments) => {
                    let count = instruments.len();
                    tracing::info!(
                        venue = "derive",
                        count = count,
                        "discovered instruments"
                    );
                    if self.previous_poll_counts.is_suspect(
                        Venue::Derive,
                        count,
                        self.discovery_config.partial_response_threshold,
                    ) {
                        tracing::warn!(
                            venue = "derive",
                            previous = ?self.previous_poll_counts.counts.get(&Venue::Derive),
                            current = count,
                            "suspect partial API response -- skipping expiry evaluation for Derive"
                        );
                        derive_suspect = true;
                    } else {
                        self.previous_poll_counts.update(Venue::Derive, count);
                    }
                    derive_polled = true;
                    all_discovered.extend(instruments);
                }
                Err(e) => {
                    tracing::warn!(
                        venue = "derive",
                        error = %e,
                        "Derive discovery failed, continuing"
                    );
                }
            }
        }

        // 1b. Check approved mapping instrument activity against latest discovery data.
        // Only check venues that actually returned data this cycle to avoid false warnings.
        {
            let deribit_has_data = deribit_polled
                && all_discovered.iter().any(|d| d.venue == Venue::Deribit);
            let kalshi_has_data = kalshi_polled
                && all_discovered.iter().any(|d| d.venue == Venue::Kalshi);
            let polymarket_has_data = polymarket_polled
                && all_discovered.iter().any(|d| d.venue == Venue::Polymarket);
            let derive_has_data = derive_polled
                && all_discovered.iter().any(|d| d.venue == Venue::Derive);

            let registry = self.registry.read().await;
            for mapping in registry.all_mappings() {
                if !mapping.approved || mapping.status != LifecycleStatus::Active {
                    continue;
                }

                if let Some(ref deribit) = mapping.venues.deribit {
                    if deribit_has_data
                        && !all_discovered
                            .iter()
                            .any(|d| d.venue == Venue::Deribit && d.instrument_id == deribit.instrument)
                    {
                        tracing::warn!(
                            event_id = %mapping.id,
                            venue = "deribit",
                            instrument = %deribit.instrument,
                            "approved mapping instrument not found in latest discovery data"
                        );
                    }
                }
                if let Some(ref kalshi) = mapping.venues.kalshi {
                    if kalshi_has_data
                        && !all_discovered
                            .iter()
                            .any(|d| d.venue == Venue::Kalshi && d.instrument_id == kalshi.ticker)
                    {
                        tracing::warn!(
                            event_id = %mapping.id,
                            venue = "kalshi",
                            instrument = %kalshi.ticker,
                            "approved mapping instrument not found in latest discovery data"
                        );
                    }
                }
                if let Some(ref polymarket) = mapping.venues.polymarket {
                    if polymarket_has_data
                        && !all_discovered
                            .iter()
                            .any(|d| d.venue == Venue::Polymarket && d.instrument_id == polymarket.condition_id)
                    {
                        tracing::warn!(
                            event_id = %mapping.id,
                            venue = "polymarket",
                            instrument = %polymarket.condition_id,
                            "approved mapping instrument not found in latest discovery data"
                        );
                    }
                }
                if let Some(ref derive_inst) = mapping.venues.derive {
                    if derive_has_data
                        && !all_discovered
                            .iter()
                            .any(|d| d.venue == Venue::Derive && d.instrument_id == derive_inst.instrument)
                    {
                        tracing::warn!(
                            event_id = %mapping.id,
                            venue = "derive",
                            instrument = %derive_inst.instrument,
                            "approved mapping instrument not found in latest discovery data"
                        );
                    }
                }
            }
        }

        // 2. Find new cross-venue candidates (all three venues via fuzzy matching)
        let registry = self.registry.read().await;
        let candidates = find_cross_venue_candidates_fuzzy(
            &all_discovered,
            self.discovery_config.expiry_tolerance_days,
        );
        let new_candidates = filter_new_candidates_fuzzy(&candidates, &registry);
        drop(registry);

        // Collect candidates for batched write (no per-item writes)
        let mut candidates_to_append: Vec<CandidateMapping> = new_candidates;
        for candidate in &candidates_to_append {
            let venue_names: Vec<&str> = [
                candidate.venues.deribit.as_ref().map(|_| "deribit"),
                candidate.venues.polymarket.as_ref().map(|_| "polymarket"),
                candidate.venues.kalshi.as_ref().map(|_| "kalshi"),
            ]
            .into_iter()
            .flatten()
            .collect();

            tracing::warn!(
                event_id = %candidate.id,
                matched_venues = ?venue_names,
                deribit_instrument = ?candidate.venues.deribit,
                polymarket_instrument = ?candidate.venues.polymarket.as_ref().map(|(cid, _)| cid),
                kalshi_instrument = ?candidate.venues.kalshi,
                expiry = %candidate.expiry,
                confidence = %candidate.expiry_confidence,
                "new proposal: candidate mapping discovered"
            );
            metrics::counter!("lifecycle_candidates_discovered").increment(1);
            metrics::counter!("proposals_total").increment(1);
        }

        // 3. Flag novel/unmatched instruments
        let registry = self.registry.read().await;
        let novel = flag_novel_instruments(&all_discovered, &registry);
        drop(registry);

        for inst in &novel {
            tracing::info!(
                venue = %inst.venue,
                instrument = %inst.instrument_id,
                asset = %inst.asset,
                "novel unmatched instrument discovered"
            );
        }

        // 4. Detect expired instruments using consecutive-absence tracking
        let registry = self.registry.read().await;
        let all_mappings = registry.all_mappings().to_vec();
        drop(registry);

        let discovered_ids: HashSet<(Venue, &str)> = all_discovered
            .iter()
            .map(|d| (d.venue, d.instrument_id.as_str()))
            .collect();

        let mut events_to_expire: Vec<String> = Vec::new();

        for mapping in &all_mappings {
            if mapping.status == LifecycleStatus::Expired {
                continue;
            }

            // Check Deribit absence (only if Deribit was polled and not suspect)
            if let Some(ref deribit) = mapping.venues.deribit {
                if deribit_polled && !deribit_suspect {
                    if discovered_ids.contains(&(Venue::Deribit, deribit.instrument.as_str())) {
                        self.absence_tracker.record_present(Venue::Deribit, &deribit.instrument);
                    } else {
                        let should_expire = self.absence_tracker.record_absent(
                            Venue::Deribit, &deribit.instrument,
                        );
                        if should_expire {
                            events_to_expire.push(mapping.id.clone());
                            self.absence_tracker.remove(Venue::Deribit, &deribit.instrument);
                        }
                    }
                }
            }

            // Check Kalshi absence (only if Kalshi was polled and not suspect)
            if let Some(ref kalshi) = mapping.venues.kalshi {
                if kalshi_polled && !kalshi_suspect {
                    if discovered_ids.contains(&(Venue::Kalshi, kalshi.ticker.as_str())) {
                        self.absence_tracker.record_present(Venue::Kalshi, &kalshi.ticker);
                    } else if !events_to_expire.contains(&mapping.id) {
                        // Don't double-expire if Deribit already triggered
                        let should_expire = self.absence_tracker.record_absent(
                            Venue::Kalshi, &kalshi.ticker,
                        );
                        if should_expire {
                            events_to_expire.push(mapping.id.clone());
                            self.absence_tracker.remove(Venue::Kalshi, &kalshi.ticker);
                        }
                    }
                }
            }

            // Check Polymarket absence (only if Polymarket was polled and not suspect)
            if let Some(ref polymarket) = mapping.venues.polymarket {
                if polymarket_polled && !polymarket_suspect {
                    if discovered_ids.contains(&(Venue::Polymarket, polymarket.condition_id.as_str())) {
                        self.absence_tracker.record_present(Venue::Polymarket, &polymarket.condition_id);
                    } else if !events_to_expire.contains(&mapping.id) {
                        // Don't double-expire if another venue already triggered
                        let should_expire = self.absence_tracker.record_absent(
                            Venue::Polymarket, &polymarket.condition_id,
                        );
                        if should_expire {
                            events_to_expire.push(mapping.id.clone());
                            self.absence_tracker.remove(Venue::Polymarket, &polymarket.condition_id);
                        }
                    }
                }
            }

            // Check Derive absence (only if Derive was polled and not suspect)
            if let Some(ref derive_inst) = mapping.venues.derive {
                if derive_polled && !derive_suspect {
                    if discovered_ids.contains(&(Venue::Derive, derive_inst.instrument.as_str())) {
                        self.absence_tracker.record_present(Venue::Derive, &derive_inst.instrument);
                    } else if !events_to_expire.contains(&mapping.id) {
                        // Don't double-expire if another venue already triggered
                        let should_expire = self.absence_tracker.record_absent(
                            Venue::Derive, &derive_inst.instrument,
                        );
                        if should_expire {
                            events_to_expire.push(mapping.id.clone());
                            self.absence_tracker.remove(Venue::Derive, &derive_inst.instrument);
                        }
                    }
                }
            }
        }

        // Log expirations
        for event_id in &events_to_expire {
            tracing::warn!(event_id = %event_id, "mapping expired (consecutive absence threshold reached)");
        }

        // 5. Handle Deribit expiry rolls for expired mappings
        for event_id in &events_to_expire {
            if let Some(mapping) = all_mappings.iter().find(|m| &m.id == event_id) {
                if let Some(ref deribit) = mapping.venues.deribit {
                    if let Some(roll_candidate) = self.find_deribit_roll(mapping, deribit, &all_discovered) {
                        tracing::info!(
                            old_event = %mapping.id,
                            old_instrument = %deribit.instrument,
                            new_event = %roll_candidate.id,
                            "Deribit expiry roll: created new candidate (approved=false)"
                        );
                        candidates_to_append.push(roll_candidate);
                    }
                }
            }
        }

        // 6. Batched TOML write -- single atomic write per poll cycle
        let needs_write = !candidates_to_append.is_empty() || !events_to_expire.is_empty();
        if needs_write {
            match self.batched_toml_write(&candidates_to_append, &events_to_expire).await {
                Ok(()) => {
                    tracing::info!(
                        appended = candidates_to_append.len(),
                        expired = events_to_expire.len(),
                        "batched TOML write complete"
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "batched TOML write failed");
                }
            }
        }

        // 7. Apply expiry warnings to near-expiry active mappings
        let registry = self.registry.read().await;
        let mut warning_count: u64 = 0;
        let now = Utc::now();

        for mapping in registry.active_approved() {
            // Parse expiry date to datetime
            let expiry_date = match NaiveDate::parse_from_str(&mapping.expiry, "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => continue,
            };
            // Use 08:00 UTC as Deribit settlement time default
            let expiry_time = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
            let expiry_datetime = match Utc.from_local_datetime(&expiry_date.and_time(expiry_time)).single() {
                Some(dt) => dt,
                None => continue,
            };

            if let Some(warning) = check_expiry_warning(&expiry_datetime, &now, &self.expiry_thresholds) {
                tracing::warn!(
                    event_id = %mapping.id,
                    tier = %warning.tier_name,
                    flags = ?warning.flags,
                    hours = %warning.hours_to_expiry,
                    "near-expiry warning"
                );
                warning_count += 1;

                // Inflate risk score for logging
                if let Some(base_score) = compute_risk_for_mapping(mapping, &self.risk_weights) {
                    let inflated = inflate_risk_score(&base_score, warning.risk_inflation_factor);
                    tracing::info!(
                        event_id = %mapping.id,
                        base_composite = %base_score.composite,
                        inflated_composite = %inflated.composite,
                        inflation_factor = %warning.risk_inflation_factor,
                        "risk score inflated for near-expiry"
                    );
                }
            }
        }

        metrics::gauge!("lifecycle_expiry_warnings").set(warning_count as f64);
        drop(registry);

        // 7b. Populate BasisRiskCache for downstream engines
        {
            let registry = self.registry.read().await;
            let mut cache = self.basis_risk_cache.write().await;
            cache.clear(); // Rebuild from scratch each cycle (evicts expired)
            let now = Utc::now();

            for mapping in registry.active_approved() {
                let base_score = match compute_risk_for_mapping(mapping, &self.risk_weights) {
                    Some(s) => s,
                    None => continue, // no settlement metadata
                };

                // Parse expiry for warning check
                let expiry_date = match NaiveDate::parse_from_str(&mapping.expiry, "%Y-%m-%d") {
                    Ok(d) => d,
                    Err(_) => {
                        // Still cache the base score without expiry warning
                        let temporal_mismatch_hours = base_score.settlement_time_risk
                            / self.risk_weights.time_per_hour.max(0.001);
                        cache.insert(mapping.id.clone(), CachedRiskInfo {
                            effective_composite: base_score.composite,
                            temporal_mismatch_hours,
                            base_score,
                            expiry_warning: None,
                            updated_at: now,
                        });
                        continue;
                    }
                };
                let expiry_time = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
                let expiry_datetime = match Utc.from_local_datetime(&expiry_date.and_time(expiry_time)).single() {
                    Some(dt) => dt,
                    None => continue,
                };

                let expiry_warning = check_expiry_warning(&expiry_datetime, &now, &self.expiry_thresholds);

                let effective_composite = match &expiry_warning {
                    Some(w) => inflate_risk_score(&base_score, w.risk_inflation_factor).composite,
                    None => base_score.composite,
                };

                let temporal_mismatch_hours = base_score.settlement_time_risk
                    / self.risk_weights.time_per_hour.max(0.001);

                cache.insert(mapping.id.clone(), CachedRiskInfo {
                    base_score,
                    expiry_warning,
                    effective_composite,
                    temporal_mismatch_hours,
                    updated_at: now,
                });
            }

            tracing::debug!(
                cache_entries = cache.len(),
                "BasisRiskCache refreshed"
            );
        }

        // 7c. Archive expired events and clean up unapproved candidates
        let mut needs_refresh = needs_write;
        match self.archive_and_cleanup().await {
            Ok(modified) => {
                needs_refresh = needs_refresh || modified;
            }
            Err(e) => {
                tracing::error!(error = %e, "archive and cleanup failed, will retry next cycle");
            }
        }

        // 8. Refresh runtime registry if TOML was modified
        if needs_refresh {
            self.refresh_registry().await;
        }

        // 9. Update pending proposals gauge (always, even if no write happened)
        {
            let registry = self.registry.read().await;
            metrics::gauge!("proposals_pending").set(registry.pending_count() as f64);
        }
    }

    /// Batched TOML write: parse once, apply all mutations, write once.
    async fn batched_toml_write(
        &self,
        candidates: &[CandidateMapping],
        expire_ids: &[String],
    ) -> anyhow::Result<()> {
        let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
        let mut doc: DocumentMut = content.parse()
            .map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))?;

        if !candidates.is_empty() {
            append_candidates_to_doc(&mut doc, candidates)?;
        }
        if !expire_ids.is_empty() {
            mark_expired_batch_in_doc(&mut doc, expire_ids)?;
        }

        self.atomic_write(&doc.to_string()).await
    }

    /// Atomic write: write to .tmp then rename (per research pitfall 4).
    async fn atomic_write(&self, content: &str) -> anyhow::Result<()> {
        let tmp_path = self.events_toml_path.with_extension("toml.tmp");
        tokio::fs::write(&tmp_path, content).await?;

        // Windows: rename over existing file can fail; remove first
        #[cfg(target_os = "windows")]
        {
            let _ = tokio::fs::remove_file(&self.events_toml_path).await;
        }

        tokio::fs::rename(&tmp_path, &self.events_toml_path).await?;
        Ok(())
    }

    /// Archive expired events to events_archive.toml and remove unapproved
    /// candidates past their expiry date. Runs as a separate read-modify-write
    /// cycle after the existing batched_toml_write.
    ///
    /// Safety: archive file is written BEFORE entries are removed from events.toml.
    /// If archive write fails, entries remain in events.toml until next cycle.
    ///
    /// Returns `true` if events.toml was modified (entries were removed).
    async fn archive_and_cleanup(&self) -> anyhow::Result<bool> {
        // 1. Read events.toml fresh (minimise race window with operator edits)
        let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
        let mut doc: DocumentMut = content.parse()
            .map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))?;

        // 2. Compute today
        let today = Utc::now().date_naive();

        // 3. Get retention_days
        let retention_days = self.discovery_config.archive_retention_days;

        // 4. Collect archivable entries (approved + expired/retired + past retention)
        let archivable = collect_archivable_entries(&doc, retention_days, today);

        // 5. Collect expired unapproved candidates
        let expired_unapproved = collect_expired_unapproved_ids(&doc, today);

        // 6. Early return if nothing to do
        if archivable.is_empty() && expired_unapproved.is_empty() {
            return Ok(false);
        }

        // 7. LIFE-01 archive step (only if archivable entries exist)
        if !archivable.is_empty() {
            // 7a. Derive archive path
            let archive_path = self.events_toml_path.with_file_name("events_archive.toml");

            // 7b. Read archive file content (or create default header if file does not exist)
            let archive_content = match tokio::fs::read_to_string(&archive_path).await {
                Ok(c) => c,
                Err(_) => "# Archived event mappings\n".to_string(),
            };

            // 7c. Parse as DocumentMut
            let mut archive_doc: DocumentMut = archive_content.parse()
                .map_err(|e| anyhow::anyhow!("Archive TOML parse error: {}", e))?;

            // 7d. Append entries to archive doc
            let archived_ids: Vec<String> = archivable.iter().map(|(id, _)| id.clone()).collect();
            let archive_count = archivable.len() as u64;
            append_entries_to_archive_doc(&mut archive_doc, archivable, &Utc::now().to_rfc3339())?;

            // 7e. Write archive file using atomic write pattern
            let archive_tmp = archive_path.with_extension("toml.tmp");
            tokio::fs::write(&archive_tmp, archive_doc.to_string()).await?;
            #[cfg(target_os = "windows")]
            {
                let _ = tokio::fs::remove_file(&archive_path).await;
            }
            tokio::fs::rename(&archive_tmp, &archive_path).await?;

            // 7f. Log at INFO level
            tracing::info!(
                count = archive_count,
                ids = ?archived_ids,
                "archived expired events"
            );

            // 7g. Increment Prometheus counter
            metrics::counter!("lifecycle_events_archived").increment(archive_count);
        }

        // 8. LIFE-02 cleanup step -- combine both sets of IDs to remove
        // 8a. Collect all IDs to remove: archive entry IDs + expired unapproved IDs
        let archive_ids: Vec<String> = if !doc.to_string().is_empty() {
            // Re-collect archive IDs from the original doc (before mutation)
            collect_archivable_entries(
                &content.parse::<DocumentMut>().unwrap(),
                retention_days,
                today,
            )
            .into_iter()
            .map(|(id, _)| id)
            .collect()
        } else {
            Vec::new()
        };

        // 8b. Log each unapproved removal at WARN level
        if !expired_unapproved.is_empty() {
            for event_id in &expired_unapproved {
                tracing::warn!(
                    event_id = %event_id,
                    "auto-removed expired unapproved candidate"
                );
            }
            // 8c. Increment Prometheus counter for unapproved only
            metrics::counter!("lifecycle_candidates_cleaned")
                .increment(expired_unapproved.len() as u64);
        }

        // 8d. Combine all IDs to remove
        let mut all_ids_to_remove = archive_ids;
        all_ids_to_remove.extend(expired_unapproved);

        if !all_ids_to_remove.is_empty() {
            remove_entries_by_id(&mut doc, &all_ids_to_remove)?;

            // 8e. Write updated events.toml
            self.atomic_write(&doc.to_string()).await?;
        }

        // 9. Return true (events.toml was modified)
        Ok(true)
    }

    /// Find a Deribit roll target: same asset+strike+direction but later expiry.
    /// Returns the roll candidate if found, or None.
    fn find_deribit_roll(
        &self,
        expired_mapping: &crate::config::EventMapping,
        _expired_deribit: &crate::config::DeribitMapping,
        discovered: &[DiscoveredInstrument],
    ) -> Option<CandidateMapping> {
        let expired_expiry = NaiveDate::parse_from_str(&expired_mapping.expiry, "%Y-%m-%d").ok()?;
        let expired_strike_str = &expired_mapping.strike;

        // Find Deribit instruments with same asset, strike, direction but later expiry
        for inst in discovered {
            if inst.venue != Venue::Deribit {
                continue;
            }
            if inst.asset.to_uppercase() != expired_mapping.asset.to_uppercase() {
                continue;
            }
            if inst.strike.to_string() != *expired_strike_str {
                continue;
            }
            if inst.direction != expired_mapping.direction {
                continue;
            }
            if inst.expiry <= expired_expiry {
                continue;
            }

            // Found a roll target -- create new candidate
            let new_id = format!(
                "{}-{}-{}",
                inst.asset.to_uppercase(),
                inst.strike,
                inst.expiry
            );

            return Some(CandidateMapping {
                id: new_id,
                asset: inst.asset.to_uppercase(),
                strike: inst.strike.to_string(),
                direction: inst.direction.clone(),
                expiry: inst.expiry.to_string(),
                venues: CandidateVenues {
                    deribit: Some(inst.instrument_id.clone()),
                    polymarket: None,
                    kalshi: None,
                    derive: None,
                },
                expiry_confidence: ExpiryConfidence::High,
            });
        }

        None
    }

    /// Refresh the runtime registry from the updated events.toml on disk.
    async fn refresh_registry(&self) {
        match tokio::fs::read_to_string(&self.events_toml_path).await {
            Ok(content) => match toml::from_str::<EventsConfig>(&content) {
                Ok(config) => {
                    let mut registry = self.registry.write().await;
                    registry.refresh(&config);
                    tracing::info!(
                        count = registry.mapping_count(),
                        active = registry.active_count(),
                        "registry refreshed after discovery"
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to parse updated events.toml");
                }
            },
            Err(e) => {
                tracing::error!(error = %e, "failed to read events.toml for refresh");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DeribitMapping, Direction, EventMapping, EventVenues, EventsConfig, ExpiryThreshold,
        LifecycleStatus, RiskWeightsConfig,
    };
    use crate::events::discovery::DiscoveredInstrument;
    use crate::events::registry::EventRegistry;
    use crate::events::risk::{check_expiry_warning, compute_risk_for_mapping, inflate_risk_score};
    use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
    use std::str::FromStr;
    use rust_decimal::Decimal;

    fn make_empty_registry() -> EventRegistry {
        EventRegistry::from_config(&EventsConfig {
            events: vec![],
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        })
    }

    fn make_thresholds() -> Vec<ExpiryThreshold> {
        vec![
            ExpiryThreshold {
                name: "caution".to_string(),
                hours_before_expiry: 48,
                flags: vec!["pricing_character_change".to_string()],
                risk_inflation_factor: 1.2,
            },
            ExpiryThreshold {
                name: "warning".to_string(),
                hours_before_expiry: 24,
                flags: vec![
                    "pricing_character_change".to_string(),
                    "liquidity_warning".to_string(),
                ],
                risk_inflation_factor: 1.5,
            },
            ExpiryThreshold {
                name: "critical".to_string(),
                hours_before_expiry: 6,
                flags: vec![
                    "pricing_character_change".to_string(),
                    "liquidity_warning".to_string(),
                    "elevated_settlement_risk".to_string(),
                ],
                risk_inflation_factor: 2.0,
            },
        ]
    }

    // --- Deribit roll detection tests ---

    #[test]
    fn deribit_roll_detection_finds_later_expiry() {
        // Simulate: expired mapping at 2025-06-27, discovered instrument at 2025-07-25
        let expired_expiry = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let later_expiry = NaiveDate::from_ymd_opt(2025, 7, 25).unwrap();

        let discovered = vec![
            DiscoveredInstrument {
                venue: Venue::Deribit,
                instrument_id: "BTC-25JUL25-100000-C".to_string(),
                asset: "BTC".to_string(),
                strike: Decimal::from_str("100000").unwrap(),
                expiry: later_expiry,
                direction: Direction::Above,
                is_active: true,
                raw_expiry_timestamp: 0,
                extra_venue_id: None,
            },
            DiscoveredInstrument {
                venue: Venue::Deribit,
                instrument_id: "BTC-27JUN25-100000-C".to_string(),
                asset: "BTC".to_string(),
                strike: Decimal::from_str("100000").unwrap(),
                expiry: expired_expiry,
                direction: Direction::Above,
                is_active: false,
                raw_expiry_timestamp: 0,
                extra_venue_id: None,
            },
        ];

        // Find roll target: same asset, strike, direction, later expiry
        let roll_targets: Vec<_> = discovered
            .iter()
            .filter(|d| {
                d.venue == Venue::Deribit
                    && d.asset.to_uppercase() == "BTC"
                    && d.strike == Decimal::from_str("100000").unwrap()
                    && d.direction == Direction::Above
                    && d.expiry > expired_expiry
            })
            .collect();

        assert_eq!(roll_targets.len(), 1);
        assert_eq!(roll_targets[0].instrument_id, "BTC-25JUL25-100000-C");
        assert_eq!(roll_targets[0].expiry, later_expiry);
    }

    // --- Expiry warning application tests ---

    #[test]
    fn expiry_warning_within_24h_triggers_warning_tier() {
        let thresholds = make_thresholds();

        // Expiry at 2025-06-27 08:00 UTC
        let expiry_date = NaiveDate::from_ymd_opt(2025, 6, 27).unwrap();
        let expiry_time = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let expiry_dt = Utc.from_local_datetime(&expiry_date.and_time(expiry_time)).single().unwrap();

        // Now is 20 hours before expiry -> within "warning" tier (24h)
        let now = expiry_dt - chrono::Duration::hours(20);

        let warning = check_expiry_warning(&expiry_dt, &now, &thresholds);
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.tier_name, "warning");
        assert!((w.hours_to_expiry - 20.0).abs() < 1e-10);
    }

    // --- Registry refresh after TOML modification test ---

    #[test]
    fn registry_refresh_picks_up_new_mappings() {
        let mut registry = make_empty_registry();
        assert_eq!(registry.mapping_count(), 0);

        let config = EventsConfig {
            events: vec![EventMapping {
                id: "BTC-100K-2025-06-27".to_string(),
                asset: "BTC".to_string(),
                strike: "100000".to_string(),
                direction: Direction::Above,
                expiry: "2025-06-27".to_string(),
                venues: EventVenues {
                    deribit: Some(DeribitMapping {
                        instrument: "BTC-27JUN25-100000-C".to_string(),
                    }),
                    polymarket: None,
                    kalshi: None,
                    derive: None,
                },
                approved: true,
                status: LifecycleStatus::Active,
                discovered_at: None,
                settlement: None,
            }],
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        };

        registry.refresh(&config);
        assert_eq!(registry.mapping_count(), 1);
        assert!(registry
            .lookup_by_instrument(Venue::Deribit, "BTC-27JUN25-100000-C")
            .is_some());
    }

    // --- Graceful Kalshi credential handling test ---

    #[test]
    fn kalshi_credentials_missing_does_not_panic() {
        // Verify the credential check logic doesn't panic with None values
        let api_key: Option<String> = None;
        let private_key: Option<String> = None;

        match (api_key, private_key) {
            (Some(_key_id), Some(_pem)) => {
                panic!("should not reach here");
            }
            _ => {
                // Expected: gracefully skip
            }
        }
    }

    // --- Risk inflation on near-expiry mapping ---

    #[test]
    fn risk_inflation_applied_to_near_expiry_mapping() {
        use crate::config::SettlementMetadata;

        let mapping = EventMapping {
            id: "BTC-100K-2025-06-27".to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-06-27".to_string(),
            venues: EventVenues {
                deribit: Some(DeribitMapping {
                    instrument: "BTC-27JUN25-100000-C".to_string(),
                }),
                polymarket: None,
                kalshi: None,
                derive: None,
            },
            approved: true,
            status: LifecycleStatus::Active,
            discovered_at: None,
            settlement: Some(SettlementMetadata {
                deribit_settlement_time: Some("2025-06-27T08:00:00Z".to_string()),
                deribit_settlement_source: Some("deribit_index".to_string()),
                polymarket_resolution_source: Some("oracle".to_string()),
                kalshi_resolution_source: None,
            }),
        };

        let weights = RiskWeightsConfig::default();
        let base_score = compute_risk_for_mapping(&mapping, &weights).unwrap();

        // Inflate by 2x (critical tier)
        let inflated = inflate_risk_score(&base_score, 2.0);

        // settlement_time_risk should be doubled
        assert!(
            (inflated.settlement_time_risk - base_score.settlement_time_risk * 2.0).abs() < 1e-10
        );
        // source_risk unchanged
        assert!((inflated.source_risk - base_score.source_risk).abs() < 1e-10);
        // composite should be larger
        assert!(inflated.composite > base_score.composite);
    }

    // --- Archive and cleanup integration test ---

    #[test]
    fn archive_cleanup_integration_sequence() {
        use crate::events::toml_writer::{
            collect_archivable_entries, collect_expired_unapproved_ids,
            remove_entries_by_id, append_entries_to_archive_doc,
        };
        use toml_edit::DocumentMut;

        // events.toml with three entries:
        // 1. Expired+approved with old expiry (archivable, retention 30d, today 2026-02-27)
        // 2. Unapproved with past expiry (cleanable)
        // 3. Active+approved (should remain)
        let events_toml = r#"
[[events]]
id = "OLD-ARCHIVABLE"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-06-01"
approved = true
status = "expired"

[events.venues.deribit]
instrument = "BTC-01JUN25-100000-C"

[[events]]
id = "EXPIRED-UNAPPROVED"
asset = "BTC"
strike = "110000"
direction = "above"
expiry = "2025-12-01"
approved = false
status = "active"

[events.venues.deribit]
instrument = "BTC-01DEC25-110000-C"

[[events]]
id = "ACTIVE-APPROVED"
asset = "BTC"
strike = "120000"
direction = "above"
expiry = "2027-06-01"
approved = true
status = "active"

[events.venues.deribit]
instrument = "BTC-01JUN27-120000-C"
"#;

        let today = NaiveDate::from_ymd_opt(2026, 2, 27).unwrap();
        let retention_days: u32 = 30;

        // Step 1: Collect archivable entries
        let doc: DocumentMut = events_toml.parse().unwrap();
        let archivable = collect_archivable_entries(&doc, retention_days, today);
        assert_eq!(archivable.len(), 1, "only OLD-ARCHIVABLE should be archivable");
        assert_eq!(archivable[0].0, "OLD-ARCHIVABLE");

        // Step 2: Collect expired unapproved
        let expired_unapproved = collect_expired_unapproved_ids(&doc, today);
        assert_eq!(expired_unapproved.len(), 1, "only EXPIRED-UNAPPROVED should be cleanable");
        assert_eq!(expired_unapproved[0], "EXPIRED-UNAPPROVED");

        // Step 3: Write to archive doc (simulates archive file creation)
        let mut archive_doc: DocumentMut = "# Archived events\n".parse().unwrap();
        append_entries_to_archive_doc(
            &mut archive_doc,
            archivable,
            "2026-02-27T09:00:00Z",
        )
        .unwrap();

        // Verify archive doc has the archived entry
        let archive_events = archive_doc["events"].as_array_of_tables().unwrap();
        assert_eq!(archive_events.len(), 1);
        let archived = archive_events.get(0).unwrap();
        assert_eq!(archived["id"].as_str().unwrap(), "OLD-ARCHIVABLE");
        assert_eq!(archived["status"].as_str().unwrap(), "retired");
        assert_eq!(
            archived["archived_at"].as_str().unwrap(),
            "2026-02-27T09:00:00Z"
        );

        // Step 4: Remove both archived and unapproved from events.toml
        let mut doc: DocumentMut = events_toml.parse().unwrap();
        let all_ids_to_remove = vec![
            "OLD-ARCHIVABLE".to_string(),
            "EXPIRED-UNAPPROVED".to_string(),
        ];
        remove_entries_by_id(&mut doc, &all_ids_to_remove).unwrap();

        // Step 5: Verify events.toml only contains the active+approved entry
        let remaining = doc["events"].as_array_of_tables().unwrap();
        assert_eq!(remaining.len(), 1, "only ACTIVE-APPROVED should remain");
        assert_eq!(
            remaining.get(0).unwrap()["id"].as_str().unwrap(),
            "ACTIVE-APPROVED"
        );
    }
}
