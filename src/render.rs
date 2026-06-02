//! Shared row formatting for `find` / `sketch` / `ask` output.
//!
//! The CLI dispatch and the `UserPromptSubmit` hook both render through these
//! functions so their output can never drift — the hook injects exactly what a
//! user would see if they ran the command themselves.

use crate::ask::AskResult;
use crate::map::index::SymbolRow;

/// Format `sketch` rows: `  file:start-end  signature` per line.
#[must_use]
pub fn sketch_rows(rows: &[SymbolRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "  {}:{}-{}  {}",
                r.file_path, r.line_start, r.line_end, r.signature
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format `find` rows: `  kind  name  file:line` per line.
#[must_use]
pub fn find_rows(rows: &[SymbolRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "  {:6}  {:<30}  {}:{}",
                r.kind, r.name, r.file_path, r.line_start
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format `ask` rows: `  path  score  reason`, with optional `--explain` axes.
#[must_use]
pub fn ask_rows(rows: &[AskResult], explain: bool) -> String {
    let max_path = rows.iter().map(|r| r.path.len()).max().unwrap_or(0);
    rows.iter()
        .map(|r| {
            if explain {
                format!(
                    "  {:<max_path$}  {:.2}  desc={:.2} sym={:.2}  {}",
                    r.path, r.score, r.desc_score, r.symbol_score, r.reason
                )
            } else {
                format!("  {:<max_path$}  {:.2}  {}", r.path, r.score, r.reason)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
