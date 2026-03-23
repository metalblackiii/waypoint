use std::path::Path;

use crate::{AppError, ledger, map, project};

/// FR-2: PreToolUse:Read — inject file map context.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    // Resolve the file's own project first (handles nested and sibling repos),
    // then fall back to the cwd project
    let (wp_dir, relative, project_label) =
        if let Some(resolved) = resolve_foreign_project(&ctx.file_path) {
            resolved
        } else if let Some(rel) = ctx.relative_path() {
            if ctx.wp_dir.exists() {
                (
                    ctx.wp_dir.clone(),
                    rel,
                    ctx.project_root.to_string_lossy().into_owned(),
                )
            } else {
                super::emit_hook_output(super::HookEvent::PreToolUse, None, "");
                return Ok(());
            }
        } else {
            // AC-5: No usable .waypoint directory — exit silently
            super::emit_hook_output(super::HookEvent::PreToolUse, None, "");
            return Ok(());
        };

    // O(1) indexed lookup; fall back to full parse if index is unavailable
    let entry = match map::index::lookup(&wp_dir, &relative) {
        Ok(Some(e)) => Some(e),
        Ok(None) => None,
        Err(_) => {
            let entries = map::read_map(&wp_dir)?;
            map::lookup(&entries, &relative).cloned()
        }
    };

    let context = if let Some(entry) = entry {
        let _ = ledger::record_event(
            ledger::EventKind::MapHit,
            &project_label,
            #[allow(clippy::cast_possible_wrap)]
            {
                entry.token_estimate as i64
            },
        );

        format!(
            "[waypoint] map: {} — {} (~{} tok)",
            entry.path, entry.description, entry.token_estimate
        )
    } else {
        let _ = ledger::record_event(ledger::EventKind::MapMiss, &project_label, 0);
        String::new()
    };

    super::emit_hook_output(super::HookEvent::PreToolUse, None, &context);
    Ok(())
}

/// Resolve a foreign file's project root and return its waypoint dir,
/// the file's relative path within that project, and a label for ledger events.
/// Returns `None` if the file doesn't belong to any waypoint-managed project.
fn resolve_foreign_project(file_path: &str) -> Option<(std::path::PathBuf, String, String)> {
    let path = Path::new(file_path);
    let foreign_root = project::find_root(path)?;
    let wp_dir = project::waypoint_dir(&foreign_root);
    if !wp_dir.exists() {
        return None;
    }
    let relative = path.strip_prefix(&foreign_root).ok()?;
    Some((
        wp_dir,
        relative.to_string_lossy().into_owned(),
        foreign_root.to_string_lossy().into_owned(),
    ))
}
