use crate::AppError;

/// FR-2: PreToolUse:Read probe.
///
/// Reads JSON from stdin, extracts file_path, returns advisory additionalContext.
/// With --large, injects ~2K chars to test size limits (FR-9).
pub fn run(large: bool) -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or_else(|| "<unknown>".into());

    let context = if large {
        // FR-9: ~2K chars to test additionalContext size limits
        let padding = "x".repeat(1900);
        format!(
            "[waypoint] map: {file_path} — SIZE LIMIT TEST — \
             This is a ~2K char payload to verify no truncation occurs \
             in additionalContext. Padding follows: {padding}"
        )
    } else {
        format!(
            "[waypoint] map: {file_path} — probe description \
             (~100 tok). This file is tracked by waypoint. \
             No map data available yet (spike mode)."
        )
    };

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "additionalContext": context
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
