use crate::{AppError, ledger, trap};

/// FR-12: PreToolUse:Edit|Write — inject trap warnings.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    if !ctx.wp_dir.exists() {
        super::emit_hook_output("PreToolUse", None, "");
        return Ok(());
    }

    let Some(relative) = ctx.relative_path() else {
        super::emit_hook_output("PreToolUse", None, "");
        return Ok(());
    };

    let traps = trap::read_traps(&ctx.wp_dir)?;

    let matching = trap::traps_for_file(&traps, &relative);

    let context = if matching.is_empty() {
        String::new()
    } else {
        let _ = ledger::record_event(
            ledger::EventKind::TrapHit,
            &ctx.project_root.to_string_lossy(),
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

    super::emit_hook_output("PreToolUse", None, &context);
    Ok(())
}
