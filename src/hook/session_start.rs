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

    // Emit session context: arch (if large enough) + command reminder (always)
    emit_session_context(&wp_dir, &ctx.project_root, fresh_arch);

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

/// Compact command digest emitted every session. Backed by a global
/// `AGENTS.md`/`CLAUDE.md` directive as of 2026-07-15 (dotfiles
/// `codex/AGENTS.md`, "Search: waypoint > rg > Grep tool > grep") — this hook
/// text is the fallback for contexts that directive doesn't reach (e.g.
/// Task-tool subagents needing their own delivery via `SubagentStart`, below).
/// `ask` is deliberately omitted (~0% organic use, per
/// waypoint-guidance-2026-06-01 assessment).
///
/// `pub(crate)` so `subagent_start` can reuse the same digest — `SessionStart`
/// context never reaches Task-tool subagents (separate hook, separate fresh
/// context per Claude Code's subagent-isolation model), so they need their
/// own delivery of this text via `SubagentStart`.
pub(crate) const COMMAND_DIGEST: &str =
    "waypoint CLI on PATH — run `waypoint find`/`callers`/`impact` (see `waypoint --help`) before grep/rg/reading.";

/// Emit session context: arch summary (for large projects) + command digest (always).
///
/// `precomputed` is the `ArchSummary` returned by a rescan that happened this
/// session. When `Some`, it is used directly to avoid a write-then-read
/// `SQLite` round-trip. When `None`, the summary is read from the DB instead.
fn emit_session_context(
    wp_dir: &std::path::Path,
    project_root: &std::path::Path,
    precomputed: Option<map::index::ArchSummary>,
) {
    let project_str = project_root.to_string_lossy();

    let arch_context =
        match precomputed.or_else(|| map::index::get_arch_summary(wp_dir).ok().flatten()) {
            Some(arch) if arch.file_count >= ARCH_FILE_THRESHOLD => {
                let _ = ledger::record_event(ledger::EventKind::ArchHit, &project_str, 0);
                let mut ctx = arch.lang_dist;
                if !arch.hotspots.is_empty() {
                    ctx.push('\n');
                    ctx.push_str(&arch.hotspots);
                }
                Some(ctx)
            }
            _ => {
                let _ = ledger::record_event(ledger::EventKind::ArchMiss, &project_str, 0);
                None
            }
        };

    let context = match arch_context {
        Some(arch) => format!("{arch}\n{COMMAND_DIGEST}"),
        None => COMMAND_DIGEST.to_string(),
    };

    super::emit_hook_output(super::HookEvent::SessionStart, None, &context);
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
#[must_use]
pub fn has_mtime_drift<S: std::hash::BuildHasher>(
    project_root: &std::path::Path,
    stored: &std::collections::HashMap<String, i64, S>,
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

        // Capture metadata once — used for both mtime and the empty-file guard below.
        let meta = std::fs::metadata(entry.path()).ok();
        let current_mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                // Millis since epoch fits comfortably in i64
                {
                    d.as_millis() as i64
                }
            });

        match (stored.get(relative.as_ref()), current_mtime) {
            // New file not in stored — only flag drift if scan_project would index it.
            // `meta` is always Some here: current_mtime is derived from meta,
            // so Some(_) in the pattern implies meta is Some.
            (None, Some(_)) => {
                // Only flag drift if scan_project would index this file.
                // Read content to mirror its trim().is_empty() check — avoids
                // perpetual rescans for whitespace-only files scan_project skips.
                let would_index = meta.as_ref().is_some_and(|m| m.len() > 0)
                    && std::fs::read_to_string(entry.path()).is_ok_and(|s| !s.trim().is_empty());
                if would_index {
                    return true;
                }
            }
            // Stored file disappeared or stat failed.
            (Some(_), None) => return true,
            // Stored file mtime changed.
            (Some(&s), Some(c)) if s != c => return true,
            // Stored file found with matching mtime — count it.
            _ => seen += 1,
        }
    }

    // Fewer files walked than stored → removals
    seen < stored.len()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::UNIX_EPOCH;
    use tempfile::TempDir;

    fn mtime_ms(path: &std::path::Path) -> i64 {
        #[allow(clippy::cast_possible_truncation)]
        // Unix millis (~1.7 trillion) fits comfortably in i64 (~9.2 quintillion)
        {
            std::fs::metadata(path)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        }
    }

    /// Regression: 0-byte scannable files were never stored by `scan_project`,
    /// so `has_mtime_drift` incorrectly treated them as new files and returned true.
    #[test]
    fn zero_byte_scannable_file_does_not_cause_drift() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("empty.js"), "").unwrap();
        let stored: HashMap<String, i64> = HashMap::new();
        assert!(!has_mtime_drift(tmp.path(), &stored));
    }

    /// Whitespace-only files are also skipped by `scan_project` — perpetual rescans
    /// would occur if they were treated as new indexable files.
    #[test]
    fn whitespace_only_scannable_file_does_not_cause_drift() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("blank.js"), "\n  \n\t\n").unwrap();
        let stored: HashMap<String, i64> = HashMap::new();
        assert!(!has_mtime_drift(tmp.path(), &stored));
    }

    #[test]
    fn new_non_empty_file_causes_drift() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.js"), "console.log('hi');").unwrap();
        let stored: HashMap<String, i64> = HashMap::new();
        assert!(has_mtime_drift(tmp.path(), &stored));
    }

    #[test]
    fn deleted_stored_file_causes_drift() {
        let tmp = TempDir::new().unwrap();
        // stored references a file that no longer exists on disk
        let mut stored = HashMap::new();
        stored.insert("gone.js".to_string(), 12345_i64);
        assert!(has_mtime_drift(tmp.path(), &stored));
    }

    #[test]
    fn unchanged_stored_file_does_not_cause_drift() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("main.js");
        std::fs::write(&file, "console.log('hi');").unwrap();
        let mut stored = HashMap::new();
        stored.insert("main.js".to_string(), mtime_ms(&file));
        assert!(!has_mtime_drift(tmp.path(), &stored));
    }
}
