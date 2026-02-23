use std::collections::HashMap;

use crate::config::{EventMapping, EventsConfig, LifecycleStatus};
use crate::types::Venue;

/// In-memory event registry with dual-index lookup.
///
/// Built from `EventsConfig`, provides O(1) lookups by:
/// - (Venue, instrument_id) -> EventMapping
/// - event_id -> EventMapping
///
/// Supports filtering for active+approved mappings only, and can be
/// refreshed from updated config (after discovery appends or config reload).
pub struct EventRegistry {
    /// All mappings (including pending and expired for reference).
    mappings: Vec<EventMapping>,
    /// Index: (Venue, instrument_id_string) -> index into mappings vec.
    instrument_index: HashMap<(Venue, String), usize>,
    /// Index: event_id -> indices into mappings vec.
    event_index: HashMap<String, Vec<usize>>,
}

impl EventRegistry {
    /// Build registry from loaded EventsConfig.
    ///
    /// Iterates all mappings and indexes each venue's instrument identifiers
    /// for O(1) lookup. Also builds an event_id -> mapping index.
    pub fn from_config(config: &EventsConfig) -> Self {
        let mut registry = Self {
            mappings: config.events.clone(),
            instrument_index: HashMap::new(),
            event_index: HashMap::new(),
        };
        registry.build_indexes();
        registry
    }

    /// Lookup event mapping by venue-specific instrument ID.
    ///
    /// Used in the pipeline to annotate MarketSnapshot with event_id.
    pub fn lookup_by_instrument(&self, venue: Venue, instrument_id: &str) -> Option<&EventMapping> {
        self.instrument_index
            .get(&(venue, instrument_id.to_string()))
            .map(|&idx| &self.mappings[idx])
    }

    /// Lookup event mapping by canonical event ID.
    ///
    /// Returns the first matching mapping (typically there is one mapping per event_id).
    pub fn lookup_by_event_id(&self, event_id: &str) -> Option<&EventMapping> {
        self.event_index
            .get(event_id)
            .and_then(|indices| indices.first())
            .map(|&idx| &self.mappings[idx])
    }

    /// Iterate over all active, approved mappings.
    ///
    /// Excludes expired mappings and unapproved candidates from runtime queries.
    pub fn active_approved(&self) -> impl Iterator<Item = &EventMapping> {
        self.mappings
            .iter()
            .filter(|m| m.approved && m.status == LifecycleStatus::Active)
    }

    /// Return all mappings as a slice (including pending and expired).
    pub fn all_mappings(&self) -> &[EventMapping] {
        &self.mappings
    }

    /// Refresh the registry from updated config.
    ///
    /// Clears and rebuilds all indexes. Called after discovery appends
    /// new candidates or config file is reloaded.
    pub fn refresh(&mut self, config: &EventsConfig) {
        self.mappings = config.events.clone();
        self.instrument_index.clear();
        self.event_index.clear();
        self.build_indexes();
    }

    /// Total number of mappings (including pending and expired).
    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }

    /// Total number of registered event mappings.
    ///
    /// Used by the health endpoint to report active event count.
    /// Includes all mappings (active, expiring, expired, pending).
    pub fn event_count(&self) -> usize {
        self.mappings.len()
    }

    /// Count of active, approved mappings.
    pub fn active_count(&self) -> usize {
        self.mappings
            .iter()
            .filter(|m| m.approved && m.status == LifecycleStatus::Active)
            .count()
    }

    /// Build both instrument and event indexes from the mappings vec.
    fn build_indexes(&mut self) {
        for (idx, mapping) in self.mappings.iter().enumerate() {
            // Index each venue's instrument identifier
            if let Some(ref deribit) = mapping.venues.deribit {
                self.instrument_index
                    .insert((Venue::Deribit, deribit.instrument.clone()), idx);
            }
            if let Some(ref polymarket) = mapping.venues.polymarket {
                self.instrument_index
                    .insert((Venue::Polymarket, polymarket.token_id.clone()), idx);
            }
            if let Some(ref kalshi) = mapping.venues.kalshi {
                self.instrument_index
                    .insert((Venue::Kalshi, kalshi.ticker.clone()), idx);
            }

            // Index by event_id
            self.event_index
                .entry(mapping.id.clone())
                .or_default()
                .push(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DeribitMapping, Direction, EventMapping, EventVenues, EventsConfig, KalshiMapping,
        LifecycleStatus, PolymarketMapping,
    };

    fn make_config(events: Vec<EventMapping>) -> EventsConfig {
        EventsConfig {
            events,
            risk_weights: None,
            discovery: None,
            expiry_thresholds: vec![],
        }
    }

    fn make_mapping(
        id: &str,
        approved: bool,
        status: LifecycleStatus,
        deribit: Option<&str>,
        polymarket: Option<(&str, &str)>,
        kalshi: Option<&str>,
    ) -> EventMapping {
        EventMapping {
            id: id.to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-06-27".to_string(),
            venues: EventVenues {
                deribit: deribit.map(|i| DeribitMapping {
                    instrument: i.to_string(),
                }),
                polymarket: polymarket.map(|(cid, tid)| PolymarketMapping {
                    condition_id: cid.to_string(),
                    token_id: tid.to_string(),
                }),
                kalshi: kalshi.map(|t| KalshiMapping {
                    ticker: t.to_string(),
                }),
            },
            approved,
            status,
            discovered_at: None,
            settlement: None,
        }
    }

    #[test]
    fn from_config_with_empty_config() {
        let config = make_config(vec![]);
        let registry = EventRegistry::from_config(&config);
        assert_eq!(registry.mapping_count(), 0);
        assert_eq!(registry.active_count(), 0);
        assert!(registry.all_mappings().is_empty());
    }

    #[test]
    fn lookup_by_deribit_instrument() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            Some("BTC-27JUN25-100000-C"),
            None,
            None,
        )]);
        let registry = EventRegistry::from_config(&config);

        let result = registry.lookup_by_instrument(Venue::Deribit, "BTC-27JUN25-100000-C");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "BTC-100K");
    }

    #[test]
    fn lookup_by_polymarket_token_id() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            None,
            Some(("0xabc", "12345")),
            None,
        )]);
        let registry = EventRegistry::from_config(&config);

        let result = registry.lookup_by_instrument(Venue::Polymarket, "12345");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "BTC-100K");
    }

    #[test]
    fn lookup_by_kalshi_ticker() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            None,
            None,
            Some("KXBTCD-25JUN30-T100000"),
        )]);
        let registry = EventRegistry::from_config(&config);

        let result = registry.lookup_by_instrument(Venue::Kalshi, "KXBTCD-25JUN30-T100000");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "BTC-100K");
    }

    #[test]
    fn lookup_by_event_id() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            Some("BTC-27JUN25-100000-C"),
            Some(("0xabc", "12345")),
            Some("KXBTCD-25JUN30-T100000"),
        )]);
        let registry = EventRegistry::from_config(&config);

        let result = registry.lookup_by_event_id("BTC-100K");
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "BTC-100K");
    }

    #[test]
    fn lookup_by_event_id_not_found() {
        let config = make_config(vec![]);
        let registry = EventRegistry::from_config(&config);

        assert!(registry.lookup_by_event_id("nonexistent").is_none());
    }

    #[test]
    fn active_approved_filters_expired_and_unapproved() {
        let config = make_config(vec![
            make_mapping(
                "active-approved",
                true,
                LifecycleStatus::Active,
                Some("INST-1"),
                None,
                None,
            ),
            make_mapping(
                "active-unapproved",
                false,
                LifecycleStatus::Active,
                Some("INST-2"),
                None,
                None,
            ),
            make_mapping(
                "expired-approved",
                true,
                LifecycleStatus::Expired,
                Some("INST-3"),
                None,
                None,
            ),
            make_mapping(
                "expiring-approved",
                true,
                LifecycleStatus::Expiring,
                Some("INST-4"),
                None,
                None,
            ),
        ]);
        let registry = EventRegistry::from_config(&config);

        assert_eq!(registry.mapping_count(), 4);
        assert_eq!(registry.active_count(), 1);

        let active: Vec<_> = registry.active_approved().collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "active-approved");
    }

    #[test]
    fn refresh_rebuilds_indexes() {
        let config1 = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            Some("INST-OLD"),
            None,
            None,
        )]);
        let mut registry = EventRegistry::from_config(&config1);
        assert!(registry
            .lookup_by_instrument(Venue::Deribit, "INST-OLD")
            .is_some());

        let config2 = make_config(vec![make_mapping(
            "BTC-120K",
            true,
            LifecycleStatus::Active,
            Some("INST-NEW"),
            None,
            None,
        )]);
        registry.refresh(&config2);

        assert!(registry
            .lookup_by_instrument(Venue::Deribit, "INST-OLD")
            .is_none());
        assert!(registry
            .lookup_by_instrument(Venue::Deribit, "INST-NEW")
            .is_some());
        assert_eq!(registry.mapping_count(), 1);
        assert_eq!(registry.lookup_by_event_id("BTC-120K").unwrap().id, "BTC-120K");
    }

    #[test]
    fn missing_venue_mapping_returns_none() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            Some("INST-1"),
            None, // no polymarket
            None, // no kalshi
        )]);
        let registry = EventRegistry::from_config(&config);

        assert!(registry
            .lookup_by_instrument(Venue::Polymarket, "12345")
            .is_none());
        assert!(registry
            .lookup_by_instrument(Venue::Kalshi, "KXBTC-SOMETHING")
            .is_none());
    }

    #[test]
    fn multi_venue_mapping_indexed_all_venues() {
        let config = make_config(vec![make_mapping(
            "BTC-100K",
            true,
            LifecycleStatus::Active,
            Some("BTC-27JUN25-100000-C"),
            Some(("0xabc", "12345")),
            Some("KXBTCD-25JUN30-T100000"),
        )]);
        let registry = EventRegistry::from_config(&config);

        // All three venues should resolve to the same mapping
        let d = registry.lookup_by_instrument(Venue::Deribit, "BTC-27JUN25-100000-C");
        let p = registry.lookup_by_instrument(Venue::Polymarket, "12345");
        let k = registry.lookup_by_instrument(Venue::Kalshi, "KXBTCD-25JUN30-T100000");

        assert_eq!(d.unwrap().id, "BTC-100K");
        assert_eq!(p.unwrap().id, "BTC-100K");
        assert_eq!(k.unwrap().id, "BTC-100K");
    }
}
