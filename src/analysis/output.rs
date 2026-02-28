use serde::Serialize;

// Re-export so downstream code can use prediction::analysis::output::Table
// without adding comfy-table as a direct dependency.
pub use comfy_table::Table;
use comfy_table::{CellAlignment, ContentArrangement};

/// Output format for CLI analysis tools.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    /// Aligned terminal table (default).
    Table,
    /// Machine-readable JSON.
    Json,
}

/// Render serialisable data as either a terminal table or pretty JSON.
///
/// - `Table` variant calls `table_fn` to build a [`comfy_table::Table`], then prints it.
/// - `Json` variant serialises `data` to pretty JSON and prints it.
pub fn render_output<T: Serialize>(
    data: &T,
    format: &OutputFormat,
    table_fn: impl FnOnce(&T) -> Table,
) {
    match format {
        OutputFormat::Table => {
            let table = table_fn(data);
            println!("{table}");
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(data).unwrap();
            println!("{json}");
        }
    }
}

/// Create a new table with dynamic content arrangement and the given headers.
pub fn new_table(headers: &[&str]) -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(headers);
    table
}

/// Right-justify the specified column indices (for numeric data).
pub fn set_numeric_columns(table: &mut Table, columns: &[usize]) {
    for &idx in columns {
        if let Some(col) = table.column_mut(idx) {
            col.set_cell_alignment(CellAlignment::Right);
        }
    }
}

/// Insert a section header row: first cell contains `text`, remaining cells are empty.
pub fn section_header(table: &mut Table, text: &str, col_count: usize) {
    let mut row: Vec<String> = Vec::with_capacity(col_count);
    row.push(text.to_string());
    for _ in 1..col_count {
        row.push(String::new());
    }
    table.add_row(row);
}

/// Summary of the JSONL loading phase, rendered before analysis output.
#[derive(Debug, Clone, Serialize)]
pub struct LoadingSummary {
    pub date_range: String,
    pub files_loaded: usize,
    pub files_missing: usize,
    pub records_loaded: usize,
    pub parse_errors: usize,
    pub events_found: usize,
}

/// Render a [`LoadingSummary`] as either a two-column terminal table or JSON.
pub fn render_loading_summary(summary: &LoadingSummary, format: &OutputFormat) {
    render_output(summary, format, |s| {
        let mut table = new_table(&["Metric", "Value"]);
        set_numeric_columns(&mut table, &[1]);
        table.add_row(vec!["Date Range".to_string(), s.date_range.clone()]);
        table.add_row(vec![
            "Files Loaded".to_string(),
            s.files_loaded.to_string(),
        ]);
        table.add_row(vec![
            "Files Missing".to_string(),
            s.files_missing.to_string(),
        ]);
        table.add_row(vec![
            "Records Loaded".to_string(),
            s.records_loaded.to_string(),
        ]);
        table.add_row(vec![
            "Parse Errors".to_string(),
            s.parse_errors.to_string(),
        ]);
        if s.events_found > 0 {
            table.add_row(vec![
                "Events Found".to_string(),
                s.events_found.to_string(),
            ]);
        }
        table
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_parses_table() {
        use clap::ValueEnum;
        let val = OutputFormat::from_str("table", true).unwrap();
        assert!(matches!(val, OutputFormat::Table));
    }

    #[test]
    fn output_format_parses_json() {
        use clap::ValueEnum;
        let val = OutputFormat::from_str("json", true).unwrap();
        assert!(matches!(val, OutputFormat::Json));
    }

    #[test]
    fn loading_summary_serializes_to_json() {
        let summary = LoadingSummary {
            date_range: "2026-01-01 to 2026-01-07".to_string(),
            files_loaded: 5,
            files_missing: 2,
            records_loaded: 1200,
            parse_errors: 3,
            events_found: 10,
        };

        let json = serde_json::to_string_pretty(&summary).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["date_range"], "2026-01-01 to 2026-01-07");
        assert_eq!(parsed["files_loaded"], 5);
        assert_eq!(parsed["files_missing"], 2);
        assert_eq!(parsed["records_loaded"], 1200);
        assert_eq!(parsed["parse_errors"], 3);
        assert_eq!(parsed["events_found"], 10);
    }

    #[test]
    fn new_table_sets_headers() {
        let table = new_table(&["A", "B", "C"]);
        let rendered = format!("{table}");
        assert!(rendered.contains("A"));
        assert!(rendered.contains("B"));
        assert!(rendered.contains("C"));
    }
}
