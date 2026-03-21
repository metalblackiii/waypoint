pub mod post_failure;
pub mod post_write;
pub mod pre_read;
pub mod pre_write;
pub mod session_start;

/// Read full stdin and parse as JSON.
pub fn read_stdin() -> Result<serde_json::Value, crate::AppError> {
    let input = std::io::read_to_string(std::io::stdin())?;
    let value: serde_json::Value = serde_json::from_str(&input)?;
    Ok(value)
}

/// Extract `file_path` from `tool_input` in the hook payload.
#[must_use]
pub fn extract_file_path(payload: &serde_json::Value) -> Option<&str> {
    payload
        .get("tool_input")
        .and_then(|ti| ti.get("file_path"))
        .and_then(|fp| fp.as_str())
}

/// Extract cwd from the hook payload.
#[must_use]
pub fn extract_cwd(payload: &serde_json::Value) -> Option<&str> {
    payload.get("cwd").and_then(|v| v.as_str())
}

/// Emit a JSON hook response to stdout.
///
/// `event_name`: `"PreToolUse"` or `"PostToolUse"`
/// `permission`: `None` to defer to Claude Code's permission system (preferred),
///               or `Some("deny")` / `Some("ask")` to override. Avoid `Some("allow")`
///               as it bypasses normal permission checks.
/// `context`: additional context string (omitted when empty)
pub fn emit_hook_output(event_name: &str, permission: Option<&str>, context: &str) {
    let json = build_hook_output(event_name, permission, context);
    println!("{}", serde_json::to_string(&json).unwrap_or_default());
}

fn build_hook_output(
    event_name: &str,
    permission: Option<&str>,
    context: &str,
) -> serde_json::Value {
    let mut hook = serde_json::json!({ "hookEventName": event_name });
    if let Some(decision) = permission {
        hook["permissionDecision"] = serde_json::json!(decision);
    }
    if !context.is_empty() {
        hook["additionalContext"] = serde_json::json!(context);
    }
    serde_json::json!({ "hookSpecificOutput": hook })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn extract_file_path_from_payload() {
        let payload = serde_json::json!({
            "tool_input": { "file_path": "/src/main.rs" }
        });
        assert_eq!(extract_file_path(&payload), Some("/src/main.rs"));
    }

    #[test]
    fn extract_file_path_missing() {
        let payload = serde_json::json!({ "tool_input": {} });
        assert_eq!(extract_file_path(&payload), None);
    }

    #[test]
    fn extract_cwd_from_payload() {
        let payload = serde_json::json!({ "cwd": "/home/user/project" });
        assert_eq!(extract_cwd(&payload), Some("/home/user/project"));
    }

    #[test]
    fn extract_cwd_missing() {
        let payload = serde_json::json!({});
        assert_eq!(extract_cwd(&payload), None);
    }

    #[test]
    fn build_pre_tool_use_allow_with_context() {
        let output = build_hook_output("PreToolUse", Some("allow"), "some context");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "allow");
        assert_eq!(hook["additionalContext"], "some context");
    }

    #[test]
    fn build_pre_tool_use_allow_empty_context() {
        let output = build_hook_output("PreToolUse", Some("allow"), "");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "allow");
        assert!(hook.get("additionalContext").is_none());
    }

    #[test]
    fn build_post_tool_use_no_permission() {
        let output = build_hook_output("PostToolUse", None, "updated file");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PostToolUse");
        assert!(hook.get("permissionDecision").is_none());
        assert_eq!(hook["additionalContext"], "updated file");
    }

    #[test]
    fn build_post_tool_use_empty() {
        let output = build_hook_output("PostToolUse", None, "");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PostToolUse");
        assert!(hook.get("permissionDecision").is_none());
        assert!(hook.get("additionalContext").is_none());
    }
}
