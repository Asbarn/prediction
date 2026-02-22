use anyhow::{anyhow, Context};
use toml_edit::{value, DocumentMut, Table};

use crate::config::Direction;

/// Input for appending a candidate mapping to events.toml.
///
/// Contains the structured fields for a cross-venue match discovered
/// by the auto-discovery system. Written with `approved = false`.
#[derive(Debug, Clone)]
pub struct CandidateMapping {
    /// Canonical event ID (e.g., "BTC-100K-2025-06-27").
    pub id: String,
    /// Underlying asset (e.g., "BTC").
    pub asset: String,
    /// Strike price as string for precision.
    pub strike: String,
    /// Direction: above or below.
    pub direction: Direction,
    /// Expiry date (YYYY-MM-DD).
    pub expiry: String,
    /// Venue-specific instrument identifiers.
    pub venues: CandidateVenues,
}

/// Venue-specific identifiers for a candidate mapping.
#[derive(Debug, Clone)]
pub struct CandidateVenues {
    /// Deribit instrument name (e.g., "BTC-27JUN25-100000-C").
    pub deribit: Option<String>,
    /// Polymarket (condition_id, token_id).
    pub polymarket: Option<(String, String)>,
    /// Kalshi ticker (e.g., "KXBTCD-25JUN30-T100000").
    pub kalshi: Option<String>,
}

/// Append a candidate mapping to events.toml content, preserving existing formatting.
///
/// Uses `toml_edit` to parse and modify the document in-place, keeping
/// all existing comments, formatting, and manual edits intact. The new
/// entry is appended to the `[[events]]` array with `approved = false`.
///
/// # Errors
///
/// Returns an error if the TOML content cannot be parsed or the `[[events]]`
/// array cannot be accessed.
pub fn append_candidate_to_toml(
    existing_content: &str,
    candidate: &CandidateMapping,
) -> anyhow::Result<String> {
    let mut doc: DocumentMut = existing_content
        .parse()
        .context("failed to parse existing events.toml content")?;

    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    let mut entry = Table::new();
    entry["id"] = value(&candidate.id);
    entry["asset"] = value(&candidate.asset);
    entry["strike"] = value(&candidate.strike);
    entry["direction"] = value(candidate.direction.to_string());
    entry["expiry"] = value(&candidate.expiry);
    entry["approved"] = value(false);
    entry["status"] = value("active");
    entry["discovered_at"] = value(chrono::Utc::now().to_rfc3339());

    // Add venue-specific sub-tables
    let mut venues = Table::new();

    if let Some(ref instrument) = candidate.venues.deribit {
        let mut deribit = Table::new();
        deribit["instrument"] = value(instrument);
        venues["deribit"] = toml_edit::Item::Table(deribit);
    }

    if let Some((ref condition_id, ref token_id)) = candidate.venues.polymarket {
        let mut polymarket = Table::new();
        polymarket["condition_id"] = value(condition_id);
        polymarket["token_id"] = value(token_id);
        venues["polymarket"] = toml_edit::Item::Table(polymarket);
    }

    if let Some(ref ticker) = candidate.venues.kalshi {
        let mut kalshi = Table::new();
        kalshi["ticker"] = value(ticker);
        venues["kalshi"] = toml_edit::Item::Table(kalshi);
    }

    entry["venues"] = toml_edit::Item::Table(venues);
    events.push(entry);

    Ok(doc.to_string())
}

/// Mark an event mapping as expired in events.toml content.
///
/// Finds the event entry with the given event_id and sets its status to "expired".
/// Preserves all other formatting and comments.
///
/// # Errors
///
/// Returns an error if the TOML content cannot be parsed, the `[[events]]`
/// array is missing, or the specified event_id is not found.
pub fn mark_expired_in_toml(existing_content: &str, event_id: &str) -> anyhow::Result<String> {
    let mut doc: DocumentMut = existing_content
        .parse()
        .context("failed to parse existing events.toml content")?;

    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    let mut found = false;
    for i in 0..events.len() {
        if let Some(table) = events.get_mut(i) {
            if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                if id == event_id {
                    table["status"] = value("expired");
                    found = true;
                    break;
                }
            }
        }
    }

    if !found {
        return Err(anyhow!("event '{}' not found in events.toml", event_id));
    }

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"# Event mapping configuration
# User comments should be preserved

[risk_weights]
time_per_hour = 0.05

[[events]]
id = "BTC-100K-2025-06-27"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-06-27"
approved = true
status = "active"

[events.venues.deribit]
instrument = "BTC-27JUN25-100000-C"
"#;

    #[test]
    fn append_preserves_existing_content() {
        let candidate = CandidateMapping {
            id: "BTC-120K-2025-07-25".to_string(),
            asset: "BTC".to_string(),
            strike: "120000".to_string(),
            direction: Direction::Above,
            expiry: "2025-07-25".to_string(),
            venues: CandidateVenues {
                deribit: Some("BTC-25JUL25-120000-C".to_string()),
                polymarket: None,
                kalshi: None,
            },
        };

        let result = append_candidate_to_toml(SAMPLE_TOML, &candidate).unwrap();

        // Original content preserved
        assert!(result.contains("# Event mapping configuration"));
        assert!(result.contains("# User comments should be preserved"));
        assert!(result.contains("BTC-100K-2025-06-27"));
        assert!(result.contains("BTC-27JUN25-100000-C"));

        // New entry added
        assert!(result.contains("BTC-120K-2025-07-25"));
        assert!(result.contains("BTC-25JUL25-120000-C"));
    }

    #[test]
    fn append_adds_entry_with_approved_false() {
        let candidate = CandidateMapping {
            id: "ETH-5K-2025-08-01".to_string(),
            asset: "ETH".to_string(),
            strike: "5000".to_string(),
            direction: Direction::Below,
            expiry: "2025-08-01".to_string(),
            venues: CandidateVenues {
                deribit: Some("ETH-01AUG25-5000-P".to_string()),
                polymarket: Some(("0xdef".to_string(), "67890".to_string())),
                kalshi: None,
            },
        };

        let result = append_candidate_to_toml(SAMPLE_TOML, &candidate).unwrap();

        // Parse result to verify structure
        let doc: DocumentMut = result.parse().unwrap();
        let events = doc["events"].as_array_of_tables().unwrap();

        // Should have 2 events now (original + appended)
        assert_eq!(events.len(), 2);

        let new_event = events.get(1).unwrap();
        assert_eq!(new_event["id"].as_str().unwrap(), "ETH-5K-2025-08-01");
        assert_eq!(new_event["approved"].as_bool().unwrap(), false);
        assert_eq!(new_event["status"].as_str().unwrap(), "active");
        assert!(new_event["discovered_at"].as_str().is_some());
        assert_eq!(new_event["direction"].as_str().unwrap(), "below");
    }

    #[test]
    fn append_with_all_venues() {
        let candidate = CandidateMapping {
            id: "BTC-150K".to_string(),
            asset: "BTC".to_string(),
            strike: "150000".to_string(),
            direction: Direction::Above,
            expiry: "2025-12-26".to_string(),
            venues: CandidateVenues {
                deribit: Some("BTC-26DEC25-150000-C".to_string()),
                polymarket: Some(("0xaaa".to_string(), "99999".to_string())),
                kalshi: Some("KXBTCD-25DEC30-T150000".to_string()),
            },
        };

        let result = append_candidate_to_toml(SAMPLE_TOML, &candidate).unwrap();

        assert!(result.contains("BTC-26DEC25-150000-C"));
        assert!(result.contains("99999"));
        assert!(result.contains("KXBTCD-25DEC30-T150000"));
    }

    #[test]
    fn mark_expired_changes_status() {
        let result = mark_expired_in_toml(SAMPLE_TOML, "BTC-100K-2025-06-27").unwrap();

        let doc: DocumentMut = result.parse().unwrap();
        let events = doc["events"].as_array_of_tables().unwrap();
        let event = events.get(0).unwrap();
        assert_eq!(event["status"].as_str().unwrap(), "expired");

        // Other fields preserved
        assert_eq!(event["id"].as_str().unwrap(), "BTC-100K-2025-06-27");
        assert_eq!(event["approved"].as_bool().unwrap(), true);
    }

    #[test]
    fn mark_expired_not_found_returns_error() {
        let result = mark_expired_in_toml(SAMPLE_TOML, "NONEXISTENT");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not found in events.toml"));
    }

    #[test]
    fn append_to_invalid_toml_returns_error() {
        let candidate = CandidateMapping {
            id: "test".to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-01-01".to_string(),
            venues: CandidateVenues {
                deribit: None,
                polymarket: None,
                kalshi: None,
            },
        };

        let result = append_candidate_to_toml("{{invalid toml", &candidate);
        assert!(result.is_err());
    }

    #[test]
    fn append_to_toml_without_events_array_returns_error() {
        let toml_without_events = "[risk_weights]\ntime_per_hour = 0.05\n";

        let candidate = CandidateMapping {
            id: "test".to_string(),
            asset: "BTC".to_string(),
            strike: "100000".to_string(),
            direction: Direction::Above,
            expiry: "2025-01-01".to_string(),
            venues: CandidateVenues {
                deribit: None,
                polymarket: None,
                kalshi: None,
            },
        };

        let result = append_candidate_to_toml(toml_without_events, &candidate);
        assert!(result.is_err());
    }
}
