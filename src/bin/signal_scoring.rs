use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

use prediction::analysis::io::{load_jsonl, DateRange};
use prediction::analysis::output::{
    new_table, render_output, section_header, set_numeric_columns, OutputFormat, Table,
};
use prediction::analysis::scoring::{compute_scoring, ScoringResult};
use prediction::paper_trade::analyzer::AnalysisSettlementRecord;

#[derive(Parser)]
#[command(name = "signal-scoring")]
#[command(about = "Score signal quality and compute go/no-go metrics from settlement data")]
struct Cli {
    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    from: Option<NaiveDate>,

    /// End date (YYYY-MM-DD), defaults to today
    #[arg(long)]
    to: Option<NaiveDate>,

    /// Analyze last N days (alternative to --from/--to)
    #[arg(long)]
    last: Option<u32>,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: OutputFormat,

    /// Break down results by event_id
    #[arg(long)]
    by_event: bool,

    /// Settlement logs directory
    #[arg(long, default_value = "settlement_logs")]
    settlement_dir: PathBuf,
}

/// Wrapper for JSON output when --by-event is used.
#[derive(Debug, Clone, Serialize)]
struct ScoringOutput {
    aggregate: ScoringResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    by_event: Option<BTreeMap<String, ScoringResult>>,
}

/// Build a table with five scoring sections from a ScoringResult.
fn scoring_table(result: &ScoringResult) -> Table {
    let mut table = new_table(&["Metric", "Value"]);
    set_numeric_columns(&mut table, &[1]);

    // Section 1: Hit Rates
    if let Some(ref hr) = result.hit_rates {
        section_header(&mut table, "=== HIT RATES ===", 2);
        table.add_row(vec![
            format!("Gross Hit Rate (n={})", hr.total),
            format!("{:.1}%", hr.gross_rate * 100.0),
        ]);
        table.add_row(vec![
            "  95% CI".to_string(),
            format!(
                "[{:.1}%, {:.1}%]",
                hr.gross_ci_95.0 * 100.0,
                hr.gross_ci_95.1 * 100.0
            ),
        ]);
        table.add_row(vec![
            "  99% CI".to_string(),
            format!(
                "[{:.1}%, {:.1}%]",
                hr.gross_ci_99.0 * 100.0,
                hr.gross_ci_99.1 * 100.0
            ),
        ]);
        table.add_row(vec![
            format!("Net Hit Rate (n={})", hr.total),
            format!("{:.1}%", hr.net_rate * 100.0),
        ]);
        table.add_row(vec![
            "  95% CI".to_string(),
            format!(
                "[{:.1}%, {:.1}%]",
                hr.net_ci_95.0 * 100.0,
                hr.net_ci_95.1 * 100.0
            ),
        ]);
        table.add_row(vec![
            "  99% CI".to_string(),
            format!(
                "[{:.1}%, {:.1}%]",
                hr.net_ci_99.0 * 100.0,
                hr.net_ci_99.1 * 100.0
            ),
        ]);
    }

    // Section 2: Edge Test
    if let Some(ref et) = result.edge_test {
        section_header(&mut table, "=== COST-ADJUSTED EDGE ===", 2);
        table.add_row(vec![
            format!("Mean Edge (n={})", et.n),
            format!("{:.6}", et.mean_edge),
        ]);
        table.add_row(vec![
            "Std Error".to_string(),
            format!("{:.6}", et.std_error),
        ]);
        table.add_row(vec![
            "t-Statistic".to_string(),
            format!("{:.4}", et.t_statistic),
        ]);
        table.add_row(vec![
            "p-Value".to_string(),
            if et.p_value < 0.0001 {
                "< 0.0001".to_string()
            } else {
                format!("{:.4}", et.p_value)
            },
        ]);
        table.add_row(vec![
            "95% CI".to_string(),
            format!("[{:.6}, {:.6}]", et.ci_95.0, et.ci_95.1),
        ]);
        table.add_row(vec![
            "Significant (p < 0.05)".to_string(),
            if et.p_value < 0.05 {
                "Yes".to_string()
            } else {
                "No".to_string()
            },
        ]);
    }

    // Section 3: Sharpe Ratio
    if let Some(ref sr) = result.sharpe {
        section_header(&mut table, "=== SHARPE RATIO ===", 2);
        table.add_row(vec![
            format!("Per-Trade Sharpe (n={})", sr.n),
            format!("{:.4}", sr.per_trade_sharpe),
        ]);
        table.add_row(vec![
            "Annualized Sharpe".to_string(),
            sr.annualized_sharpe
                .map(|a| format!("{:.4}", a))
                .unwrap_or_else(|| "N/A (single day)".to_string()),
        ]);
        table.add_row(vec![
            "Trades/Year".to_string(),
            sr.trades_per_year
                .map(|t| format!("{:.1}", t))
                .unwrap_or_else(|| "N/A".to_string()),
        ]);
        table.add_row(vec![
            "PSR (P[SR > 0])".to_string(),
            sr.psr
                .map(|p| format!("{:.1}%", p * 100.0))
                .unwrap_or_else(|| "N/A".to_string()),
        ]);
    }

    // Section 4: Maximum Drawdown
    if let Some(ref dd) = result.drawdown {
        section_header(&mut table, "=== MAXIMUM DRAWDOWN ===", 2);
        table.add_row(vec![
            "Max Drawdown (abs)".to_string(),
            format!("{:.6}", dd.max_drawdown_abs),
        ]);
        table.add_row(vec![
            "Max Drawdown (%)".to_string(),
            dd.max_drawdown_pct
                .map(|p| format!("{:.1}%", p))
                .unwrap_or_else(|| "N/A".to_string()),
        ]);
        table.add_row(vec![
            "Peak Date".to_string(),
            dd.peak_date.clone(),
        ]);
        table.add_row(vec![
            "Trough Date".to_string(),
            dd.trough_date.clone(),
        ]);
        table.add_row(vec![
            "Recovery Date".to_string(),
            dd.recovery_date
                .clone()
                .unwrap_or_else(|| "Ongoing".to_string()),
        ]);
        table.add_row(vec![
            "Current Drawdown (abs)".to_string(),
            format!("{:.6}", dd.current_drawdown_abs),
        ]);
        table.add_row(vec![
            "Current Drawdown (%)".to_string(),
            dd.current_drawdown_pct
                .map(|p| format!("{:.1}%", p))
                .unwrap_or_else(|| "N/A".to_string()),
        ]);
    }

    table
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let range = DateRange::from_args(cli.from, cli.to, cli.last)?;

    // Load settlement data from settlement_logs/ with "settlements-" prefix
    let files = range.files_in_dir_prefixed(&cli.settlement_dir, "settlements-");
    let result = load_jsonl::<AnalysisSettlementRecord>(&files);

    // Print loading summary to stderr (keep visible but separate from output)
    eprintln!(
        "Loaded {} records from {} files ({} missing, {} errors) for {}",
        result.records.len(),
        result.files_loaded,
        result.files_missing,
        result.errors,
        range
    );

    if result.errors > 0 {
        eprintln!("Warning: {} malformed JSONL lines skipped", result.errors);
    }

    // Handle empty data
    if result.records.is_empty() {
        println!("No settled positions in range {range}");
        return Ok(());
    }

    // Compute aggregate scoring
    let aggregate = compute_scoring(&result.records);

    if cli.by_event {
        // Group records by event_id
        let mut groups: BTreeMap<String, Vec<AnalysisSettlementRecord>> = BTreeMap::new();
        for record in &result.records {
            groups
                .entry(record.event_id.clone())
                .or_default()
                .push(record.clone());
        }

        // Compute per-event scoring
        let mut by_event_scoring: BTreeMap<String, ScoringResult> = BTreeMap::new();
        for (event_id, event_records) in &groups {
            by_event_scoring.insert(event_id.clone(), compute_scoring(event_records));
        }

        let output = ScoringOutput {
            aggregate,
            by_event: Some(by_event_scoring),
        };

        match cli.output {
            OutputFormat::Table => {
                // Render aggregate
                let table = scoring_table(&output.aggregate);
                println!("{table}");

                // Render per-event breakdowns
                if let Some(ref events) = output.by_event {
                    for (event_id, event_scoring) in events {
                        println!("\n=== Event: {event_id} ===");
                        let event_table = scoring_table(event_scoring);
                        println!("{event_table}");
                    }
                }
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&output)?;
                println!("{json}");
            }
        }
    } else {
        // Simple aggregate output
        render_output(&aggregate, &cli.output, scoring_table);
    }

    Ok(())
}
