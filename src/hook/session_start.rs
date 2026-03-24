use crate::{AppError, journal, ledger, map, project};

/// Maximum age before a map is considered stale regardless of file count.
const MAP_MAX_AGE_DAYS: i64 = 14;

/// File count drift threshold (fraction). If actual count differs from the
/// map header count by more than this ratio, trigger a rescan.
const FILE_COUNT_DRIFT_THRESHOLD: f64 = 0.10;

/// FR-7: `SessionStart` — inject journal context and auto-scan.
///
/// `SessionStart` hooks use plain stdout for context injection.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;
    let wp_dir = project::ensure_initialized(&ctx.project_root)?;

    // FR-22: Auto-scan if map.md doesn't exist or is stale
    if should_rescan(&wp_dir, &ctx.project_root) {
        let output = map::scan::scan_project(&ctx.project_root)?;
        map::write_map(&wp_dir, &output.entries)?;
        let _ = map::index::rebuild_symbols(&wp_dir, &output.symbols);
    }

    // FR-7: Inject journal contents
    let journal_content = journal::read_journal(&wp_dir)?;

    let mut output = String::new();

    if !journal_content.trim().is_empty() {
        output.push_str(&journal_content);
        output.push('\n');
    }

    // FR-10: Invocation prompt for journal
    output.push_str(
        "To log a correction or preference: \
         waypoint journal add --section <preferences|learnings|do-not-repeat> \"<entry>\"\n",
    );

    // FR-15: Invocation prompt for traps
    output.push_str(
        "To log a bug fix: \
         waypoint trap log --error \"<msg>\" --file \"<path>\" \
         --cause \"<root cause>\" --fix \"<what you did>\" --tags \"<comma-separated>\"\n",
    );

    // FR-17: Record session start (silent failure)
    let _ = ledger::record_event(
        ledger::EventKind::SessionStart,
        &ctx.project_root.to_string_lossy(),
        0,
    );

    // FR-19: Purge old ledger events once per session, not per hook
    let _ = ledger::purge_old_events();

    print!("{output}");
    Ok(())
}

/// Decide whether to rescan based on map existence, age, and file count drift.
///
/// Triggers a rescan when any of these are true:
/// - map.md doesn't exist
/// - map is older than `MAP_MAX_AGE_DAYS`
/// - file count differs from map header by more than `FILE_COUNT_DRIFT_THRESHOLD`
fn should_rescan(wp_dir: &std::path::Path, project_root: &std::path::Path) -> bool {
    let Some(header) = map::parse_map_header(wp_dir) else {
        // No map or unparseable header → rescan
        return true;
    };

    // Age check: rescan if map is older than threshold
    let age = chrono::Utc::now() - header.generated_at;
    if age.num_days() >= MAP_MAX_AGE_DAYS {
        return true;
    }

    // File count drift: stat-only walk, compare to header
    let actual_count = map::scan::count_scannable_files(project_root);
    #[allow(clippy::cast_precision_loss)]
    let drift =
        (actual_count as f64 - header.file_count as f64).abs() / header.file_count.max(1) as f64;

    drift > FILE_COUNT_DRIFT_THRESHOLD
}
