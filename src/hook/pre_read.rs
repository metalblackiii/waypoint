use std::path::Path;

use crate::{AppError, ledger, map, project};

/// FR-2: PreToolUse:Read — inject file map context.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_default();
    let cwd = super::extract_cwd(&payload).unwrap_or_else(|| ".".into());
    let cwd_path = Path::new(&cwd);

    let project_root = project::find_root(cwd_path)
        .or_else(|| project::find_root(Path::new(&file_path)))
        .unwrap_or_else(|| cwd_path.to_path_buf());

    let wp_dir = project::waypoint_dir(&project_root);

    // AC-5: No .waypoint directory — exit silently
    if !wp_dir.exists() {
        super::emit_hook_output("PreToolUse", None, "");
        return Ok(());
    }

    let entries = map::read_map(&wp_dir)?;

    let relative = match Path::new(&file_path).strip_prefix(&project_root) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => {
            // File is outside this project — skip map lookup
            return Ok(());
        }
    };

    let context = if let Some(entry) = map::lookup(&entries, &relative) {
        let _ = ledger::record_event(
            ledger::EventKind::MapHit,
            &project_root.to_string_lossy(),
            entry.token_estimate as i64,
        );

        format!(
            "[waypoint] map: {} — {} (~{} tok)",
            entry.path, entry.description, entry.token_estimate
        )
    } else {
        let _ = ledger::record_event(
            ledger::EventKind::MapMiss,
            &project_root.to_string_lossy(),
            0,
        );
        String::new()
    };

    super::emit_hook_output("PreToolUse", None, &context);
    Ok(())
}
