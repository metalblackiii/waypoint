use std::fmt::Write;

use crate::{AppError, learning, ledger, map, project};

/// FR-2: PreToolUse:Read — inject file map context.
/// FR-8: Annotate foreign project availability.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    // Resolve the file's own project first (handles nested and sibling repos),
    // then fall back to the cwd project
    let (wp_dir, relative, project_label, is_foreign) =
        if let Some(resolved) = project::resolve_foreign(&ctx.file_path) {
            let is_foreign = resolved.root != ctx.project_root;
            (
                resolved.wp_dir,
                resolved.relative_path,
                resolved.root.to_string_lossy().into_owned(),
                is_foreign,
            )
        } else if let Some(rel) = ctx.relative_path() {
            if ctx.wp_dir.exists() {
                (
                    ctx.wp_dir.clone(),
                    rel,
                    ctx.project_root.to_string_lossy().into_owned(),
                    false,
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

    let mut context = if let Some(entry) = entry {
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

    // FR-7: Surface contextual learnings for this file
    let learnings = match learning::read_learnings(&wp_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[waypoint] warning: failed to read learnings: {e}");
            Vec::new()
        }
    };
    let matching_learnings = learning::learnings_for_file(&learnings, &relative);
    if !matching_learnings.is_empty() {
        let entries: Vec<String> = matching_learnings
            .iter()
            .map(|l| format!("{}: {}", l.id, l.entry))
            .collect();
        let _ = write!(
            context,
            "\n[waypoint] learnings for {}: {}",
            relative,
            entries.join(" | ")
        );
    }

    // FR-8: Annotate foreign project so the AI knows cross-project data is available
    if is_foreign {
        let foreign_note = format!("\n[waypoint] foreign: {project_label}");
        context.push_str(&foreign_note);
    }

    super::emit_hook_output(super::HookEvent::PreToolUse, None, &context);
    Ok(())
}
