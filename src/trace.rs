use std::collections::HashSet;
use std::path::Path;

use crate::AppError;
use crate::map::index;

/// Direction of call graph traversal.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum TraceDirection {
    /// Functions that call the target (who calls this?).
    Inbound,
    /// Functions the target calls (what does this call?).
    Outbound,
    /// Both directions.
    Both,
}

/// Walk function call chains from a starting symbol.
///
/// Uses recursive DFS up to `depth` hops, printing a tree of call relationships.
/// V1: same-file calls only — cross-file edges are stored but not yet traversed.
pub fn run(
    wp_dir: &Path,
    symbol: &str,
    direction: &TraceDirection,
    depth: usize,
) -> Result<(), AppError> {
    // Resolve starting symbol via sketch
    let results = index::sketch(wp_dir, symbol)?;
    if results.is_empty() {
        println!("No symbol found: {symbol}");
        return Ok(());
    }

    // Prefer first result (exported-first ordering from sketch).
    // Warn when ambiguous so the user knows which was chosen.
    if results.len() > 1 {
        eprintln!(
            "note: {} symbols match '{}'; tracing first (use qualified name to disambiguate)",
            results.len(),
            symbol
        );
    }
    let start = &results[0];
    let file = &start.file_path;

    println!("trace: {}  [{}]", start.name, file);

    let show_outbound = matches!(direction, TraceDirection::Outbound | TraceDirection::Both);
    let show_inbound = matches!(direction, TraceDirection::Inbound | TraceDirection::Both);

    if show_outbound {
        println!("outbound:");
        let mut visited = HashSet::new();
        visited.insert(start.name.clone());
        trace_outbound(wp_dir, &start.name, file, 1, depth, &mut visited)?;
    }

    if show_inbound {
        println!("inbound:");
        let mut visited = HashSet::new();
        visited.insert(start.name.clone());
        trace_inbound(wp_dir, &start.name, file, 1, depth, &mut visited)?;
    }

    Ok(())
}

/// Recursive outbound: find callees up to depth via DFS.
fn trace_outbound(
    wp_dir: &Path,
    symbol: &str,
    file: &str,
    current_depth: usize,
    max_depth: usize,
    visited: &mut HashSet<String>,
) -> Result<(), AppError> {
    if current_depth > max_depth {
        return Ok(());
    }

    let callees = index::find_callees(wp_dir, symbol, file)?;

    for (callee, line) in &callees {
        let indent = "  ".repeat(current_depth);
        let new = visited.insert(callee.clone());
        let cycle_marker = if new { "" } else { " (cycle)" };
        println!("{indent}→ {callee:<30} {file}:{line}{cycle_marker}");

        if new {
            trace_outbound(wp_dir, callee, file, current_depth + 1, max_depth, visited)?;
        }
    }

    Ok(())
}

/// Recursive inbound: find callers up to depth via DFS.
fn trace_inbound(
    wp_dir: &Path,
    symbol: &str,
    file: &str,
    current_depth: usize,
    max_depth: usize,
    visited: &mut HashSet<String>,
) -> Result<(), AppError> {
    if current_depth > max_depth {
        return Ok(());
    }

    let callers = index::find_callers_of(wp_dir, symbol, file)?;

    for (caller, caller_file, line) in &callers {
        let indent = "  ".repeat(current_depth);
        let new = visited.insert(caller.clone());
        let cycle_marker = if new { "" } else { " (cycle)" };
        println!("{indent}← {caller:<30} {caller_file}:{line}{cycle_marker}");

        if new {
            trace_inbound(
                wp_dir,
                caller,
                caller_file,
                current_depth + 1,
                max_depth,
                visited,
            )?;
        }
    }

    Ok(())
}
