use crate::{AppError, ledger, map, project};

use crate::map::MAP_STALE_DAYS;

/// File count drift threshold (fraction). If actual count differs from the
/// map header count by more than this ratio, trigger a rescan.
const FILE_COUNT_DRIFT_THRESHOLD: f64 = 0.10;

/// `SessionStart` — auto-scan and inject trap log reminder.
///
/// `SessionStart` hooks use plain stdout for context injection.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;
    let wp_dir = project::ensure_initialized(&ctx.project_root)?;

    // Auto-scan if map.md doesn't exist or is stale
    if should_rescan(&wp_dir, &ctx.project_root) {
        let output = map::scan::scan_project(&ctx.project_root)?;
        map::write_map(&wp_dir, &output.entries)?;
        let _ = map::index::rebuild_symbols(&wp_dir, &output.symbols);
    }

    let mut output = String::new();

    // Invocation prompt
    output.push_str(
        "To log a bug fix: \
         waypoint trap log --error \"<msg>\" --file \"<path>\" \
         --cause \"<root cause>\" --fix \"<what you did>\" --tags \"<comma-separated>\"\n",
    );

    // Record session start (silent failure)
    let _ = ledger::record_event(
        ledger::EventKind::SessionStart,
        &ctx.project_root.to_string_lossy(),
        0,
    );

    // Purge old ledger events once per session, not per hook
    let _ = ledger::purge_old_events();

    print!("{output}");
    Ok(())
}

/// Decide whether to rescan based on map existence, age, and file count drift.
///
/// Triggers a rescan when any of these are true:
/// - map.md doesn't exist
/// - map is older than `MAP_STALE_DAYS`
/// - file count differs from map header by more than `FILE_COUNT_DRIFT_THRESHOLD`
fn should_rescan(wp_dir: &std::path::Path, project_root: &std::path::Path) -> bool {
    let Some(header) = map::parse_map_header(wp_dir) else {
        // No map or unparseable header → rescan
        return true;
    };

    // Age check: rescan if map is older than threshold
    let age = chrono::Utc::now() - header.generated_at;
    if age.num_days() >= MAP_STALE_DAYS {
        return true;
    }

    // File count drift: stat-only walk, compare to header
    let actual_count = map::scan::count_scannable_files(project_root);
    #[allow(clippy::cast_precision_loss)]
    let drift =
        (actual_count as f64 - header.file_count as f64).abs() / header.file_count.max(1) as f64;

    drift > FILE_COUNT_DRIFT_THRESHOLD
}
