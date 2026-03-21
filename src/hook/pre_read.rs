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
        print_allow("");
        return Ok(());
    }

    let entries = map::read_map(&wp_dir)?;

    let relative = Path::new(&file_path)
        .strip_prefix(&project_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file_path.clone());

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

    print_allow(&context);
    Ok(())
}

fn print_allow(context: &str) {
    let output = if context.is_empty() {
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow"
            }
        })
    } else {
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "additionalContext": context
            }
        })
    };
    println!("{}", serde_json::to_string(&output).unwrap_or_default());
}
