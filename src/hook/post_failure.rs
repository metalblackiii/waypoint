use crate::AppError;

/// FR-16: PostToolUseFailure:Edit|Write — suggest trap search.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_else(|| "<unknown>".into());

    let context = format!(
        "[waypoint] Edit/Write failed for {file_path}. \
         If this is a known issue, check: waypoint trap search \"<error>\""
    );

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
