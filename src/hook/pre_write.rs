use crate::AppError;

/// FR-4: PreToolUse:Edit|Write probe.
///
/// Reads JSON from stdin, extracts file_path, returns advisory additionalContext
/// about known traps for the file.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_else(|| "<unknown>".into());

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "additionalContext": format!("[waypoint] traps: no known traps for {file_path}")
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
