use std::path::Path;

use crate::{AppError, ledger, project, trap};

/// FR-12: PreToolUse:Edit|Write — inject trap warnings.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_default();
    let cwd = super::extract_cwd(&payload).unwrap_or_else(|| ".".into());
    let cwd_path = Path::new(&cwd);

    let project_root = project::find_root(cwd_path)
        .or_else(|| project::find_root(Path::new(&file_path)))
        .unwrap_or_else(|| cwd_path.to_path_buf());

    let wp_dir = project::waypoint_dir(&project_root);

    if !wp_dir.exists() {
        super::emit_hook_output("PreToolUse", Some("allow"), "");
        return Ok(());
    }

    let traps = trap::read_traps(&wp_dir)?;

    let relative = Path::new(&file_path)
        .strip_prefix(&project_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file_path.clone());

    let matching = trap::traps_for_file(&traps, &relative);

    let context = if matching.is_empty() {
        String::new()
    } else {
        let _ = ledger::record_event(
            ledger::EventKind::TrapHit,
            &project_root.to_string_lossy(),
            0,
        );

        let warnings: Vec<String> = matching
            .iter()
            .map(|t| format!("{}: {} → {}", t.id, t.error_message, t.fix))
            .collect();

        format!(
            "[waypoint] traps for {}: {}",
            relative,
            warnings.join(" | ")
        )
    };

    super::emit_hook_output("PreToolUse", Some("allow"), &context);
    Ok(())
}
