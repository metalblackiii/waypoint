use crate::{AppError, ledger, map, project};

use crate::map::MAP_STALE_DAYS;

/// File count drift threshold (fraction). If actual count differs from the
/// map header count by more than this ratio, trigger a rescan.
const FILE_COUNT_DRIFT_THRESHOLD: f64 = 0.03;

/// Minimum file count to emit arch context. Small projects don't benefit
/// from architecture summary — the map is sufficient.
const ARCH_FILE_THRESHOLD: i64 = 20;

/// `SessionStart` — auto-scan, emit arch context, record session start.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;
    let wp_dir = project::ensure_initialized(&ctx.project_root)?;

    // Auto-scan if map.md doesn't exist or is stale
    let fresh_arch = if should_rescan(&wp_dir, &ctx.project_root) {
        let output = map::scan::scan_project(&ctx.project_root)?;
        map::write_map(&wp_dir, &output.entries)?;
        if let Err(e) = map::index::rebuild_symbols(&wp_dir, &output.symbols) {
            eprintln!("Warning: symbol index failed: {e}");
        }
        if let Err(e) = map::index::rebuild_imports(&wp_dir, &output.imports) {
            eprintln!("Warning: import index failed: {e}");
        }
        // Pass the computed summary directly to emit_arch_context to avoid a
        // write-then-read round-trip through SQLite on every rescan.
        map::index::rebuild_arch_summary(&wp_dir, &output.entries, &output.imports).ok()
    } else {
        None
    };

    // Emit arch context if project is large enough
    emit_arch_context(&wp_dir, &ctx.project_root, fresh_arch);

    // Record session start (silent failure)
    let _ = ledger::record_event(
        ledger::EventKind::SessionStart,
        &ctx.project_root.to_string_lossy(),
        0,
    );

    // Purge old ledger events once per session, not per hook
    let _ = ledger::purge_old_events();

    Ok(())
}

/// Emit architecture context via hook output if the project has enough files.
///
/// `precomputed` is the `ArchSummary` returned by a rescan that happened this
/// session. When `Some`, it is used directly to avoid a write-then-read
/// `SQLite` round-trip. When `None`, the summary is read from the DB instead.
fn emit_arch_context(
    wp_dir: &std::path::Path,
    project_root: &std::path::Path,
    precomputed: Option<map::index::ArchSummary>,
) {
    let project_str = project_root.to_string_lossy();
    let Some(arch) = precomputed.or_else(|| map::index::get_arch_summary(wp_dir).ok().flatten())
    else {
        let _ = ledger::record_event(ledger::EventKind::ArchMiss, &project_str, 0);
        return;
    };

    if arch.file_count < ARCH_FILE_THRESHOLD {
        let _ = ledger::record_event(ledger::EventKind::ArchMiss, &project_str, 0);
        return;
    }

    let mut context = arch.lang_dist;
    if !arch.hotspots.is_empty() {
        context.push('\n');
        context.push_str(&arch.hotspots);
    }

    super::emit_hook_output(super::HookEvent::SessionStart, None, &context);
    let _ = ledger::record_event(ledger::EventKind::ArchHit, &project_str, 0);
}

/// Decide whether to rescan based on map existence and file mtimes.
///
/// Triggers a rescan when any of these are true:
/// - map.md doesn't exist or has an unparseable header
/// - mtime data is available and any file has changed (precise)
/// - no mtime data (legacy map): falls back to age + file-count drift
fn should_rescan(wp_dir: &std::path::Path, project_root: &std::path::Path) -> bool {
    let Some(header) = map::parse_map_header(wp_dir) else {
        return true;
    };

    // Prefer mtime-based staleness (precise, same cost as stat-only walk)
    if let Ok(stored_mtimes) = map::index::get_stored_mtimes(wp_dir)
        && !stored_mtimes.is_empty()
    {
        return has_mtime_drift(project_root, &stored_mtimes);
    }

    // Legacy fallback: age + file-count drift (for maps without mtime data)
    let age = chrono::Utc::now() - header.generated_at;
    if age.num_days() >= MAP_STALE_DAYS {
        return true;
    }

    let actual_count = map::scan::count_scannable_files(project_root);
    #[allow(clippy::cast_precision_loss)]
    let drift =
        (actual_count as f64 - header.file_count as f64).abs() / header.file_count.max(1) as f64;

    drift > FILE_COUNT_DRIFT_THRESHOLD
}

/// Compare file mtimes against stored values. Returns `true` if any file changed,
/// was added, or was removed. Stat-only — does not read file content.
pub(crate) fn has_mtime_drift(
    project_root: &std::path::Path,
    stored: &std::collections::HashMap<String, i64>,
) -> bool {
    let mut seen = 0usize;

    for entry in map::scan::project_walker(project_root) {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        if !map::scan::is_scannable(entry.path()) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(project_root)
            .unwrap_or(entry.path())
            .to_string_lossy();

        let current_mtime = std::fs::metadata(entry.path())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                // Millis since epoch fits comfortably in i64
                {
                    d.as_millis() as i64
                }
            });

        match (stored.get(relative.as_ref()), current_mtime) {
            (None, Some(_)) | (Some(_), None) => return true, // new or stat-failed
            (Some(&s), Some(c)) if s != c => return true,     // changed mtime
            _ => {}
        }
        seen += 1;
    }

    // Fewer files walked than stored → removals
    seen < stored.len()
}
