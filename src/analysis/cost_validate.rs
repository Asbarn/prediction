//! Cost parameter validation against documented exchange fee schedules.
//!
//! Compares configured fee parameters to their expected values from exchange
//! documentation, producing a validation report with source citations.

use rust_decimal::Decimal;
use serde::Serialize;
use std::fmt;

use crate::analysis::output::{new_table, set_numeric_columns, Table};
use crate::signal::config::SignalGenerationConfig;

/// Validation status for a single parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ValidationStatus {
    /// Config value matches expected value.
    Match,
    /// Config value differs from expected value.
    Mismatch,
    /// Expected parameter is missing from config.
    Missing,
    /// Parameter has no documented external reference (operator-defined).
    Undocumented,
}

impl fmt::Display for ValidationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationStatus::Match => write!(f, "MATCH"),
            ValidationStatus::Mismatch => write!(f, "MISMATCH"),
            ValidationStatus::Missing => write!(f, "MISSING"),
            ValidationStatus::Undocumented => write!(f, "UNDOCUMENTED"),
        }
    }
}

/// A single validation entry comparing a config parameter to its expected value.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationEntry {
    pub parameter: String,
    pub config_value: String,
    pub expected_value: String,
    pub source: String,
    pub status: ValidationStatus,
    pub notes: String,
}

/// Aggregated validation report.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub entries: Vec<ValidationEntry>,
    pub matches: usize,
    pub mismatches: usize,
    pub missing: usize,
    pub undocumented: usize,
}

impl ValidationReport {
    /// Returns true if report has no mismatches or missing parameters.
    pub fn is_clean(&self) -> bool {
        self.mismatches == 0 && self.missing == 0
    }
}

/// Validate signal generation config parameters against documented exchange values.
pub fn validate_signal_config(config: &SignalGenerationConfig) -> ValidationReport {
    let mut entries = Vec::new();

    // Deribit taker fee rate
    let deribit_expected = Decimal::new(3, 4); // 0.0003
    entries.push(ValidationEntry {
        parameter: "deribit_taker_fee_rate".to_string(),
        config_value: config.deribit_taker_fee_rate.to_string(),
        expected_value: "0.0003".to_string(),
        source: "Deribit: 0.03% of underlying (base tier)".to_string(),
        status: if config.deribit_taker_fee_rate == deribit_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Derive taker fee rate
    let derive_expected = Decimal::new(4, 4); // 0.0004
    entries.push(ValidationEntry {
        parameter: "derive_taker_fee_rate".to_string(),
        config_value: config.derive_taker_fee_rate.to_string(),
        expected_value: "0.0004".to_string(),
        source: "Derive help center: 0.04% of notional (taker)".to_string(),
        status: if config.derive_taker_fee_rate == derive_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Derive base fee
    let derive_base_expected = Decimal::new(50, 2); // 0.50
    entries.push(ValidationEntry {
        parameter: "derive_base_fee_usd".to_string(),
        config_value: config.derive_base_fee_usd.to_string(),
        expected_value: "0.50".to_string(),
        source: "Derive help center: $0.50 base fee per trade".to_string(),
        status: if config.derive_base_fee_usd == derive_base_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Polymarket fee rate
    let poly_fee_expected = Decimal::new(25, 2); // 0.25
    entries.push(ValidationEntry {
        parameter: "polymarket_fees.fee_rate".to_string(),
        config_value: config.polymarket_fees.fee_rate.to_string(),
        expected_value: "0.25".to_string(),
        source: "Polymarket docs: fee_rate=0.25 for crypto markets".to_string(),
        status: if config.polymarket_fees.fee_rate == poly_fee_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Polymarket exponent
    entries.push(ValidationEntry {
        parameter: "polymarket_fees.exponent".to_string(),
        config_value: config.polymarket_fees.exponent.to_string(),
        expected_value: "2".to_string(),
        source: "Polymarket docs: exponent=2 for crypto markets".to_string(),
        status: if config.polymarket_fees.exponent == 2 {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Gas cost
    let gas_expected = Decimal::new(1, 2); // 0.01
    entries.push(ValidationEntry {
        parameter: "polymarket_fees.gas_cost_usd".to_string(),
        config_value: config.polymarket_fees.gas_cost_usd.to_string(),
        expected_value: "0.01".to_string(),
        source: "PolygonScan: ~$0.005-0.01 avg tx fee".to_string(),
        status: if config.polymarket_fees.gas_cost_usd == gas_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Bridge cost (undocumented if zero)
    entries.push(ValidationEntry {
        parameter: "polymarket_fees.bridge_cost_amortized_usd".to_string(),
        config_value: config.polymarket_fees.bridge_cost_amortized_usd.to_string(),
        expected_value: "operator-defined".to_string(),
        source: "N/A".to_string(),
        status: if config.polymarket_fees.bridge_cost_amortized_usd == Decimal::ZERO {
            ValidationStatus::Undocumented
        } else {
            ValidationStatus::Match
        },
        notes: if config.polymarket_fees.bridge_cost_amortized_usd == Decimal::ZERO {
            "Operator must set based on bridging pattern ($5-20 per bridge from Ethereum, $0.50-2 from exchanges)".to_string()
        } else {
            String::new()
        },
    });

    // Carry annualized rate
    let carry_expected = Decimal::new(5, 2); // 0.05
    entries.push(ValidationEntry {
        parameter: "carry.annualized_rate".to_string(),
        config_value: config.carry.annualized_rate.to_string(),
        expected_value: "0.05".to_string(),
        source: "Conservative DeFi lending rate assumption".to_string(),
        status: if config.carry.annualized_rate == carry_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Basis risk scale
    let basis_expected = Decimal::new(1, 2); // 0.01
    entries.push(ValidationEntry {
        parameter: "basis_risk_scale".to_string(),
        config_value: config.basis_risk_scale.to_string(),
        expected_value: "0.01".to_string(),
        source: "Internal: 1% of composite basis risk score".to_string(),
        status: if config.basis_risk_scale == basis_expected {
            ValidationStatus::Match
        } else {
            ValidationStatus::Mismatch
        },
        notes: String::new(),
    });

    // Compute tallies
    let matches = entries
        .iter()
        .filter(|e| e.status == ValidationStatus::Match)
        .count();
    let mismatches = entries
        .iter()
        .filter(|e| e.status == ValidationStatus::Mismatch)
        .count();
    let missing = entries
        .iter()
        .filter(|e| e.status == ValidationStatus::Missing)
        .count();
    let undocumented = entries
        .iter()
        .filter(|e| e.status == ValidationStatus::Undocumented)
        .count();

    ValidationReport {
        entries,
        matches,
        mismatches,
        missing,
        undocumented,
    }
}

/// Build a comfy-table rendering of a ValidationReport.
pub fn validation_table(report: &ValidationReport) -> Table {
    let mut table = new_table(&["Parameter", "Config Value", "Expected", "Status", "Source"]);
    set_numeric_columns(&mut table, &[1, 2]);

    for entry in &report.entries {
        let mut row = vec![
            entry.parameter.clone(),
            entry.config_value.clone(),
            entry.expected_value.clone(),
            entry.status.to_string(),
            entry.source.clone(),
        ];
        if !entry.notes.is_empty() {
            // Append notes to source column for compact display
            row[4] = format!("{} [{}]", entry.source, entry.notes);
        }
        table.add_row(row);
    }

    // Summary row
    table.add_row(vec![
        "TOTALS".to_string(),
        String::new(),
        String::new(),
        format!(
            "{} match, {} mismatch, {} missing, {} undoc",
            report.matches, report.mismatches, report.missing, report.undocumented,
        ),
        String::new(),
    ]);

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates_all_match() {
        let config = SignalGenerationConfig::default();
        let report = validate_signal_config(&config);

        // All should be Match except bridge_cost which is Undocumented
        assert_eq!(report.mismatches, 0);
        assert_eq!(report.missing, 0);
        assert_eq!(report.undocumented, 1); // bridge_cost_amortized_usd
        assert_eq!(report.matches, report.entries.len() - 1);
        assert!(report.is_clean());
    }

    #[test]
    fn mismatch_detected_for_wrong_deribit_rate() {
        let mut config = SignalGenerationConfig::default();
        config.deribit_taker_fee_rate = Decimal::new(5, 4); // 0.0005 instead of 0.0003
        let report = validate_signal_config(&config);

        assert_eq!(report.mismatches, 1);
        let deribit_entry = report
            .entries
            .iter()
            .find(|e| e.parameter == "deribit_taker_fee_rate")
            .unwrap();
        assert_eq!(deribit_entry.status, ValidationStatus::Mismatch);
        assert!(!report.is_clean());
    }

    #[test]
    fn bridge_cost_becomes_match_when_set() {
        let mut config = SignalGenerationConfig::default();
        config.polymarket_fees.bridge_cost_amortized_usd = Decimal::new(1, 0); // $1.00
        let report = validate_signal_config(&config);

        let bridge_entry = report
            .entries
            .iter()
            .find(|e| e.parameter == "polymarket_fees.bridge_cost_amortized_usd")
            .unwrap();
        assert_eq!(bridge_entry.status, ValidationStatus::Match);
        assert_eq!(report.undocumented, 0);
    }

    #[test]
    fn validation_table_renders() {
        let config = SignalGenerationConfig::default();
        let report = validate_signal_config(&config);
        let table = validation_table(&report);
        let rendered = format!("{table}");
        assert!(rendered.contains("deribit_taker_fee_rate"));
        assert!(rendered.contains("MATCH"));
        assert!(rendered.contains("TOTALS"));
    }

    #[test]
    fn report_serializes_to_json() {
        let config = SignalGenerationConfig::default();
        let report = validate_signal_config(&config);
        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["entries"].is_array());
        assert_eq!(parsed["mismatches"], 0);
    }
}
