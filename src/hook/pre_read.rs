use crate::{AppError, ledger, map};

/// FR-2: PreToolUse:Read — inject file map context.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    // AC-5: No .waypoint directory — exit silently
    if !ctx.wp_dir.exists() {
        super::emit_hook_output(super::HookEvent::PreToolUse, None, "");
        return Ok(());
    }

    let Some(relative) = ctx.relative_path() else {
        // File is outside this project — skip map lookup
        return Ok(());
    };

    let entries = map::read_map(&ctx.wp_dir)?;

    let context = if let Some(entry) = map::lookup(&entries, &relative) {
        let _ = ledger::record_event(
            ledger::EventKind::MapHit,
            &ctx.project_root.to_string_lossy(),
            #[allow(clippy::cast_possible_wrap)] // token estimates won't exceed i64::MAX
            {
                entry.token_estimate as i64
            },
        );

        format!(
            "[waypoint] map: {} — {} (~{} tok)",
            entry.path, entry.description, entry.token_estimate
        )
    } else {
        let _ = ledger::record_event(
            ledger::EventKind::MapMiss,
            &ctx.project_root.to_string_lossy(),
            0,
        );
        String::new()
    };

    super::emit_hook_output(super::HookEvent::PreToolUse, None, &context);
    Ok(())
}
