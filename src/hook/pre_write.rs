use crate::{AppError, ledger, project, trap};

/// FR-12: PreToolUse:Edit|Write — inject trap warnings.
/// FR-9: Resolve foreign project for trap checks.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    // Resolve the file's own project first, fall back to cwd project
    let (wp_dir, relative, project_label) =
        if let Some(resolved) = project::resolve_foreign(&ctx.file_path) {
            (
                resolved.wp_dir,
                resolved.relative_path,
                resolved.root.to_string_lossy().into_owned(),
            )
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
            super::emit_hook_output(super::HookEvent::PreToolUse, None, "");
            return Ok(());
        };

    let traps = trap::read_traps(&wp_dir)?;

    let matching = trap::traps_for_file(&traps, &relative);

    let context = if matching.is_empty() {
        String::new()
    } else {
        let _ = ledger::record_event(ledger::EventKind::TrapHit, &project_label, 0);

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

    super::emit_hook_output(super::HookEvent::PreToolUse, None, &context);
    Ok(())
}
