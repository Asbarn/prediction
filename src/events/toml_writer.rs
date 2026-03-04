use anyhow::{anyhow, Context};
use chrono::NaiveDate;
use toml_edit::{value, DocumentMut, Table};

use crate::config::Direction;
use crate::events::discovery::ExpiryConfidence;

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
    /// Confidence score for expiry alignment between matched venues.
    pub expiry_confidence: ExpiryConfidence,
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
    /// Derive instrument name (e.g., "BTC-20250627-100000-C").
    pub derive: Option<String>,
}

/// Build a TOML `Table` for a candidate mapping with all standard fields.
///
/// Used by both the single-candidate `append_candidate_to_toml` and the
/// batch `append_candidates_to_doc` to avoid duplicating field-population logic.
fn build_candidate_table(candidate: &CandidateMapping) -> Table {
    let mut entry = Table::new();
    entry["id"] = value(&candidate.id);
    entry["asset"] = value(&candidate.asset);
    entry["strike"] = value(&candidate.strike);
    entry["direction"] = value(candidate.direction.to_string());
    entry["expiry"] = value(&candidate.expiry);
    entry["approved"] = value(false);
    entry["status"] = value("active");
    entry["discovered_at"] = value(chrono::Utc::now().to_rfc3339());
    entry["expiry_confidence"] = value(candidate.expiry_confidence.to_string());

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

    if let Some(ref instrument) = candidate.venues.derive {
        let mut derive = Table::new();
        derive["instrument"] = value(instrument);
        venues["derive"] = toml_edit::Item::Table(derive);
    }

    entry["venues"] = toml_edit::Item::Table(venues);
    entry
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

    let entry = build_candidate_table(candidate);
    events.push(entry);

    Ok(doc.to_string())
}

/// Append multiple candidate mappings to a `DocumentMut` in-place (no file I/O).
///
/// Operates directly on the provided document, building a TOML table for each
/// candidate and pushing it to the `[[events]]` array. This enables batched
/// writes: parse once, append N candidates, then serialize and write once.
///
/// # Errors
///
/// Returns an error if the `[[events]]` array of tables is missing from the document.
pub fn append_candidates_to_doc(
    doc: &mut DocumentMut,
    candidates: &[CandidateMapping],
) -> anyhow::Result<()> {
    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    for candidate in candidates {
        let entry = build_candidate_table(candidate);
        events.push(entry);
    }

    Ok(())
}

/// Mark multiple events as expired in a `DocumentMut` in-place (no file I/O).
///
/// Iterates the `[[events]]` array and sets `status = "expired"` for each
/// matching event ID. IDs are assumed unique, so the search breaks after
/// finding each match. This enables batched writes: parse once, mark N
/// events expired, then serialize and write once.
///
/// # Errors
///
/// Returns an error if the `[[events]]` array of tables is missing from the document.
pub fn mark_expired_batch_in_doc(
    doc: &mut DocumentMut,
    event_ids: &[String],
) -> anyhow::Result<()> {
    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    for target_id in event_ids {
        for i in 0..events.len() {
            if let Some(table) = events.get_mut(i) {
                if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                    if id == target_id {
                        table["status"] = value("expired");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
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

/// Collect entries from `[[events]]` that are eligible for archival.
///
/// An entry is archivable if:
/// - `approved == true` (only archive operator-approved events, not auto-discovered candidates)
/// - `status` is `"expired"` or `"retired"`
/// - `expiry` date + `retention_days` < `today` (entry is older than retention period)
///
/// Returns a `Vec<(String, Table)>` of `(id, cloned_table)` pairs.
pub fn collect_archivable_entries(
    doc: &DocumentMut,
    retention_days: u32,
    today: NaiveDate,
) -> Vec<(String, Table)> {
    let events = match doc["events"].as_array_of_tables() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for i in 0..events.len() {
        if let Some(table) = events.get(i) {
            let approved = table
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !approved {
                continue;
            }

            let status = table
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("active");
            if status != "expired" && status != "retired" {
                continue;
            }

            let expiry_str = match table.get("expiry").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let expiry_date = match NaiveDate::parse_from_str(expiry_str, "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => continue,
            };

            let cutoff = today - chrono::Duration::days(retention_days as i64);
            if expiry_date < cutoff {
                if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                    result.push((id.to_string(), table.clone()));
                }
            }
        }
    }

    result
}

/// Collect IDs of unapproved candidates that have passed their expiry date.
///
/// An entry matches if:
/// - `approved == false`
/// - `expiry` date < `today` (strict less-than, consistent with validation.rs)
///
/// Entries without an explicit `approved` field default to `true` (i.e., manually-authored
/// entries are NOT removed).
pub fn collect_expired_unapproved_ids(
    doc: &DocumentMut,
    today: NaiveDate,
) -> Vec<String> {
    let events = match doc["events"].as_array_of_tables() {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for i in 0..events.len() {
        if let Some(table) = events.get(i) {
            let approved = table
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if approved {
                continue;
            }

            let expiry_str = match table.get("expiry").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => continue,
            };
            let expiry_date = match NaiveDate::parse_from_str(expiry_str, "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => continue,
            };

            if expiry_date < today {
                if let Some(id) = table.get("id").and_then(|v| v.as_str()) {
                    result.push(id.to_string());
                }
            }
        }
    }

    result
}

/// Remove entries from the `[[events]]` array by ID.
///
/// Uses `ArrayOfTables::retain()` to keep only entries whose ID is NOT in `ids_to_remove`.
pub fn remove_entries_by_id(
    doc: &mut DocumentMut,
    ids_to_remove: &[String],
) -> anyhow::Result<()> {
    let events = doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("events.toml missing [[events]] array of tables"))?;

    events.retain(|table| {
        let id = table.get("id").and_then(|v| v.as_str()).unwrap_or("");
        !ids_to_remove.iter().any(|remove_id| remove_id == id)
    });

    Ok(())
}

/// Append archived entries to an archive document.
///
/// For each entry:
/// - Sets `status = "retired"` on the table
/// - Adds `archived_at = <timestamp>` field
/// - Pushes to the `[[events]]` array of the archive document
///
/// Creates the `[[events]]` array if it does not exist in the archive doc.
pub fn append_entries_to_archive_doc(
    archive_doc: &mut DocumentMut,
    entries: Vec<(String, Table)>,
    archived_at: &str,
) -> anyhow::Result<()> {
    // Ensure [[events]] array exists in the archive doc
    if archive_doc.get("events").is_none() {
        use toml_edit::ArrayOfTables;
        let arr = ArrayOfTables::new();
        archive_doc["events"] = toml_edit::Item::ArrayOfTables(arr);
    }

    let events = archive_doc["events"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("archive doc missing [[events]] array of tables"))?;

    for (_id, mut table) in entries {
        table["status"] = value("retired");
        table["archived_at"] = value(archived_at);
        events.push(table);
    }

    Ok(())
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
                derive: None,
            },
            expiry_confidence: ExpiryConfidence::High,
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
                derive: None,
            },
            expiry_confidence: ExpiryConfidence::High,
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
                derive: None,
            },
            expiry_confidence: ExpiryConfidence::High,
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
                derive: None,
            },
            expiry_confidence: ExpiryConfidence::High,
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
                derive: None,
            },
            expiry_confidence: ExpiryConfidence::High,
        };

        let result = append_candidate_to_toml(toml_without_events, &candidate);
        assert!(result.is_err());
    }

    // --- Archive and cleanup helper tests ---

    /// Helper TOML with entries in various archivable states.
    const ARCHIVE_TOML: &str = r#"
[[events]]
id = "OLD-EXPIRED-APPROVED"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-01-01"
approved = true
status = "expired"

[events.venues.deribit]
instrument = "BTC-01JAN25-100000-C"

[[events]]
id = "RECENT-EXPIRED-APPROVED"
asset = "BTC"
strike = "110000"
direction = "above"
expiry = "2026-02-20"
approved = true
status = "expired"

[events.venues.deribit]
instrument = "BTC-20FEB26-110000-C"

[[events]]
id = "OLD-EXPIRED-UNAPPROVED"
asset = "BTC"
strike = "120000"
direction = "above"
expiry = "2025-01-01"
approved = false
status = "expired"

[events.venues.deribit]
instrument = "BTC-01JAN25-120000-C"
"#;

    #[test]
    fn test_collect_archivable_entries_filters_correctly() {
        let doc: DocumentMut = ARCHIVE_TOML.parse().unwrap();
        // Today = 2026-02-27, retention = 30 days
        // Cutoff = 2026-01-28
        // OLD-EXPIRED-APPROVED: expiry 2025-01-01 < 2026-01-28 => archivable
        // RECENT-EXPIRED-APPROVED: expiry 2026-02-20 > 2026-01-28 => not yet
        // OLD-EXPIRED-UNAPPROVED: approved=false => not archivable
        let today = NaiveDate::from_ymd_opt(2026, 2, 27).unwrap();
        let result = collect_archivable_entries(&doc, 30, today);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "OLD-EXPIRED-APPROVED");
    }

    #[test]
    fn test_collect_expired_unapproved_ids() {
        let toml = r#"
[[events]]
id = "UNAPPROVED-PAST"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2025-12-01"
approved = false
status = "active"

[events.venues.deribit]
instrument = "BTC-01DEC25-100000-C"

[[events]]
id = "UNAPPROVED-FUTURE"
asset = "BTC"
strike = "110000"
direction = "above"
expiry = "2027-06-01"
approved = false
status = "active"

[events.venues.deribit]
instrument = "BTC-01JUN27-110000-C"

[[events]]
id = "APPROVED-PAST"
asset = "BTC"
strike = "120000"
direction = "above"
expiry = "2025-12-01"
approved = true
status = "expired"

[events.venues.deribit]
instrument = "BTC-01DEC25-120000-C"
"#;
        let doc: DocumentMut = toml.parse().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 2, 27).unwrap();
        let result = collect_expired_unapproved_ids(&doc, today);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "UNAPPROVED-PAST");
    }

    #[test]
    fn test_remove_entries_by_id() {
        let toml = r#"
[[events]]
id = "KEEP-1"
asset = "BTC"
strike = "100000"
direction = "above"
expiry = "2026-06-01"

[events.venues.deribit]
instrument = "BTC-01JUN26-100000-C"

[[events]]
id = "REMOVE-ME"
asset = "BTC"
strike = "110000"
direction = "above"
expiry = "2025-01-01"

[events.venues.deribit]
instrument = "BTC-01JAN25-110000-C"

[[events]]
id = "KEEP-2"
asset = "BTC"
strike = "120000"
direction = "above"
expiry = "2026-12-01"

[events.venues.deribit]
instrument = "BTC-01DEC26-120000-C"
"#;
        let mut doc: DocumentMut = toml.parse().unwrap();
        remove_entries_by_id(&mut doc, &["REMOVE-ME".to_string()]).unwrap();

        let events = doc["events"].as_array_of_tables().unwrap();
        assert_eq!(events.len(), 2);

        let ids: Vec<&str> = (0..events.len())
            .map(|i| events.get(i).unwrap()["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"KEEP-1"));
        assert!(ids.contains(&"KEEP-2"));
        assert!(!ids.contains(&"REMOVE-ME"));
    }

    #[test]
    fn test_append_entries_to_archive_doc() {
        // Start with an empty archive document
        let mut archive_doc: DocumentMut = "".parse().unwrap();

        // Create two entries to archive
        let mut table1 = Table::new();
        table1["id"] = value("EVENT-1");
        table1["asset"] = value("BTC");
        table1["status"] = value("expired");

        let mut table2 = Table::new();
        table2["id"] = value("EVENT-2");
        table2["asset"] = value("ETH");
        table2["status"] = value("expired");

        let entries = vec![
            ("EVENT-1".to_string(), table1),
            ("EVENT-2".to_string(), table2),
        ];

        append_entries_to_archive_doc(
            &mut archive_doc,
            entries,
            "2026-02-27T09:00:00Z",
        )
        .unwrap();

        let events = archive_doc["events"].as_array_of_tables().unwrap();
        assert_eq!(events.len(), 2);

        let e1 = events.get(0).unwrap();
        assert_eq!(e1["id"].as_str().unwrap(), "EVENT-1");
        assert_eq!(e1["status"].as_str().unwrap(), "retired");
        assert_eq!(e1["archived_at"].as_str().unwrap(), "2026-02-27T09:00:00Z");

        let e2 = events.get(1).unwrap();
        assert_eq!(e2["id"].as_str().unwrap(), "EVENT-2");
        assert_eq!(e2["status"].as_str().unwrap(), "retired");
        assert_eq!(e2["archived_at"].as_str().unwrap(), "2026-02-27T09:00:00Z");
    }
}
