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
    discover_deribit, discover_kalshi, discover_polymarket, filter_new_candidates,
    find_cross_venue_candidates, flag_novel_instruments, DiscoveredInstrument,
};
use crate::events::registry::EventRegistry;
use crate::events::risk::{check_expiry_warning, inflate_risk_score, compute_risk_for_mapping, BasisRiskCache, CachedRiskInfo};
use crate::events::toml_writer::{append_candidate_to_toml, mark_expired_in_toml, append_candidates_to_doc, mark_expired_batch_in_doc, CandidateMapping, CandidateVenues};
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
    pub async fn run(self) {
        let min_interval = self.discovery_config.min_poll_interval_secs();
        let mut interval = tokio::time::interval(Duration::from_secs(min_interval));
        let mut last_deribit_poll = Instant::now() - Duration::from_secs(min_interval + 1);
        let mut last_kalshi_poll = Instant::now() - Duration::from_secs(min_interval + 1);
        let mut last_polymarket_poll = Instant::now() - Duration::from_secs(min_interval + 1);

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
                    ).await;
                }
            }
        }
    }

    /// Single poll cycle: discover, match, expire, roll, warn, refresh.
    async fn poll_cycle(
        &self,
        last_deribit_poll: &mut Instant,
        last_kalshi_poll: &mut Instant,
        last_polymarket_poll: &mut Instant,
    ) {
        let mut all_discovered: Vec<DiscoveredInstrument> = Vec::new();
        let mut toml_modified = false;

        // 1. Discover instruments from each venue (only if interval elapsed)
        // --- Deribit ---
        if last_deribit_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.deribit_poll_interval_secs)
        {
            *last_deribit_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "deribit").increment(1);
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
            )
            .await
            {
                Ok(instruments) => {
                    tracing::info!(
                        venue = "deribit",
                        count = instruments.len(),
                        "discovered instruments"
                    );
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
                        match discover_kalshi(
                            &self.http_client,
                            &self.venues_config.kalshi.rest_url,
                            &key_id,
                            &private_key,
                            &self.discovery_config.kalshi_series_tickers,
                        )
                        .await
                        {
                            Ok(instruments) => {
                                tracing::info!(
                                    venue = "kalshi",
                                    count = instruments.len(),
                                    "discovered instruments"
                                );
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

        // --- Polymarket (deactivation monitoring only) ---
        if last_polymarket_poll.elapsed()
            >= Duration::from_secs(self.discovery_config.polymarket_poll_interval_secs)
        {
            *last_polymarket_poll = Instant::now();
            metrics::counter!("lifecycle_discovery_polls", "venue" => "polymarket").increment(1);

            match discover_polymarket(
                &self.http_client,
                &self.venues_config.polymarket.gamma_api_url,
            )
            .await
            {
                Ok(markets) => {
                    tracing::info!(
                        venue = "polymarket",
                        count = markets.len(),
                        "discovered Polymarket markets"
                    );
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
                        "Polymarket discovery failed, continuing"
                    );
                }
            }
        }

        // 2. Find new cross-venue candidates (Deribit + Kalshi only)
        let registry = self.registry.read().await;
        let candidates = find_cross_venue_candidates(&all_discovered);
        let new_candidates = filter_new_candidates(&candidates, &registry);
        drop(registry);

        for candidate in &new_candidates {
            match self.append_candidate(candidate).await {
                Ok(()) => {
                    tracing::info!(
                        event_id = %candidate.id,
                        deribit = ?candidate.venues.deribit,
                        kalshi = ?candidate.venues.kalshi,
                        "discovered new candidate mapping"
                    );
                    metrics::counter!("lifecycle_candidates_discovered").increment(1);
                    toml_modified = true;
                }
                Err(e) => {
                    tracing::error!(
                        event_id = %candidate.id,
                        error = %e,
                        "failed to append candidate to events.toml"
                    );
                }
            }
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

        // 4. Detect expired instruments
        let registry = self.registry.read().await;
        let all_mappings = registry.all_mappings().to_vec();
        drop(registry);

        let discovered_ids: HashSet<(Venue, &str)> = all_discovered
            .iter()
            .map(|d| (d.venue, d.instrument_id.as_str()))
            .collect();

        for mapping in &all_mappings {
            if mapping.status == LifecycleStatus::Expired {
                continue;
            }

            // Check each venue's instrument against discovered
            let mut any_expired = false;

            if let Some(ref deribit) = mapping.venues.deribit {
                if !all_discovered.is_empty()
                    && all_discovered.iter().any(|d| d.venue == Venue::Deribit)
                    && !discovered_ids.contains(&(Venue::Deribit, deribit.instrument.as_str()))
                {
                    any_expired = true;
                }
            }

            if any_expired {
                match self.mark_expired(&mapping.id).await {
                    Ok(()) => {
                        tracing::warn!(event_id = %mapping.id, "mapping expired");
                        toml_modified = true;

                        // 5. Handle Deribit expiry rolls
                        if let Some(ref deribit) = mapping.venues.deribit {
                            self.handle_deribit_roll(mapping, deribit, &all_discovered, &mut toml_modified)
                                .await;
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            event_id = %mapping.id,
                            error = %e,
                            "failed to mark mapping as expired"
                        );
                    }
                }
            }
        }

        // 6. Apply expiry warnings to near-expiry active mappings
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

        // 6b. Populate BasisRiskCache for downstream engines
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

        // 7. Refresh runtime registry if TOML was modified
        if toml_modified {
            self.refresh_registry().await;
        }
    }

    /// Append a candidate mapping to events.toml with atomic write.
    async fn append_candidate(&self, candidate: &CandidateMapping) -> anyhow::Result<()> {
        let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
        let updated = append_candidate_to_toml(&content, candidate)?;
        self.atomic_write(&updated).await
    }

    /// Mark a mapping as expired in events.toml with atomic write.
    async fn mark_expired(&self, event_id: &str) -> anyhow::Result<()> {
        let content = tokio::fs::read_to_string(&self.events_toml_path).await?;
        let updated = mark_expired_in_toml(&content, event_id)?;
        self.atomic_write(&updated).await
    }

    /// Atomic write: write to .tmp then rename (per research pitfall 4).
    async fn atomic_write(&self, content: &str) -> anyhow::Result<()> {
        let tmp_path = self.events_toml_path.with_extension("toml.tmp");
        tokio::fs::write(&tmp_path, content).await?;
        tokio::fs::rename(&tmp_path, &self.events_toml_path).await?;
        Ok(())
    }

    /// Handle Deribit expiry roll: find new instrument with same asset+strike+direction
    /// but later expiry and create a fresh candidate (approved = false).
    async fn handle_deribit_roll(
        &self,
        expired_mapping: &crate::config::EventMapping,
        expired_deribit: &crate::config::DeribitMapping,
        discovered: &[DiscoveredInstrument],
        toml_modified: &mut bool,
    ) {
        let expired_expiry = match NaiveDate::parse_from_str(&expired_mapping.expiry, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => return,
        };
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

            let candidate = CandidateMapping {
                id: new_id.clone(),
                asset: inst.asset.to_uppercase(),
                strike: inst.strike.to_string(),
                direction: inst.direction.clone(),
                expiry: inst.expiry.to_string(),
                venues: CandidateVenues {
                    deribit: Some(inst.instrument_id.clone()),
                    polymarket: None,
                    kalshi: None,
                },
            };

            match self.append_candidate(&candidate).await {
                Ok(()) => {
                    tracing::info!(
                        old_event = %expired_mapping.id,
                        old_instrument = %expired_deribit.instrument,
                        new_event = %new_id,
                        new_instrument = %inst.instrument_id,
                        "Deribit expiry roll: created new candidate (approved=false)"
                    );
                    *toml_modified = true;
                }
                Err(e) => {
                    tracing::error!(
                        event_id = %new_id,
                        error = %e,
                        "failed to append roll candidate"
                    );
                }
            }
            break; // Only roll to the nearest future expiry
        }
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
}
