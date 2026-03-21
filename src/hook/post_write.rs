use crate::AppError;

/// FR-5: PostToolUse:Edit|Write probe.
///
/// Reads JSON from stdin, extracts file_path, returns additionalContext
/// to test whether PostToolUse supports context injection.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_else(|| "<unknown>".into());

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": format!("[waypoint] map updated: {file_path}")
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
