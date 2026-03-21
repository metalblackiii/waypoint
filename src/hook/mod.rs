pub mod post_failure;
pub mod post_write;
pub mod pre_read;
pub mod pre_write;
pub mod session_start;

use std::path::{Path, PathBuf};

use crate::{AppError, project};

/// Shared context for hooks that resolve a project root from stdin.
pub(crate) struct HookContext {
    pub(crate) file_path: String,
    pub(crate) project_root: PathBuf,
    pub(crate) wp_dir: PathBuf,
}

impl HookContext {
    /// Parse stdin, extract cwd/`file_path`, and resolve the project root.
    pub(crate) fn from_stdin() -> Result<Self, AppError> {
        let payload = read_stdin()?;
        Ok(Self::from_payload(&payload))
    }

    /// Build context from an already-parsed JSON payload.
    fn from_payload(payload: &serde_json::Value) -> Self {
        let file_path = extract_file_path(payload).unwrap_or("").to_string();
        let cwd = extract_cwd(payload).unwrap_or(".");
        let cwd_path = Path::new(cwd);

        let project_root = project::find_root(cwd_path)
            .or_else(|| project::find_root(Path::new(&file_path)))
            .unwrap_or_else(|| cwd_path.to_path_buf());

        let wp_dir = project::waypoint_dir(&project_root);

        Self {
            file_path,
            project_root,
            wp_dir,
        }
    }

    /// Strip the project root prefix from `file_path` to get a relative path.
    /// Returns `None` if the file is outside this project.
    #[must_use]
    pub(crate) fn relative_path(&self) -> Option<String> {
        Path::new(&self.file_path)
            .strip_prefix(&self.project_root)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    }
}

/// Hook event types for Claude Code hook responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookEvent {
    PreToolUse,
    PostToolUse,
}

impl HookEvent {
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
        }
    }
}

/// Permission decisions for hook responses.
///
/// `None` defers to Claude Code's permission system (preferred).
/// Avoid `Allow` as it bypasses normal permission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // variants represent valid wire protocol values
pub(crate) enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

impl PermissionDecision {
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

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
pub(crate) fn emit_hook_output(
    event: HookEvent,
    permission: Option<PermissionDecision>,
    context: &str,
) {
    let json = build_hook_output(event, permission, context);
    println!("{}", serde_json::to_string(&json).unwrap_or_default());
}

fn build_hook_output(
    event: HookEvent,
    permission: Option<PermissionDecision>,
    context: &str,
) -> serde_json::Value {
    let mut hook = serde_json::json!({ "hookEventName": event.as_str() });
    if let Some(decision) = permission {
        hook["permissionDecision"] = serde_json::json!(decision.as_str());
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
        let output = build_hook_output(
            HookEvent::PreToolUse,
            Some(PermissionDecision::Allow),
            "some context",
        );
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "allow");
        assert_eq!(hook["additionalContext"], "some context");
    }

    #[test]
    fn build_pre_tool_use_allow_empty_context() {
        let output = build_hook_output(HookEvent::PreToolUse, Some(PermissionDecision::Allow), "");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PreToolUse");
        assert_eq!(hook["permissionDecision"], "allow");
        assert!(hook.get("additionalContext").is_none());
    }

    #[test]
    fn build_post_tool_use_no_permission() {
        let output = build_hook_output(HookEvent::PostToolUse, None, "updated file");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PostToolUse");
        assert!(hook.get("permissionDecision").is_none());
        assert_eq!(hook["additionalContext"], "updated file");
    }

    #[test]
    fn build_post_tool_use_empty() {
        let output = build_hook_output(HookEvent::PostToolUse, None, "");
        let hook = &output["hookSpecificOutput"];

        assert_eq!(hook["hookEventName"], "PostToolUse");
        assert!(hook.get("permissionDecision").is_none());
        assert!(hook.get("additionalContext").is_none());
    }

    #[test]
    fn hook_context_resolves_root_from_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let sub = tmp.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();

        let payload = serde_json::json!({
            "cwd": sub.to_string_lossy(),
            "tool_input": { "file_path": sub.join("main.rs").to_string_lossy().as_ref() }
        });

        let ctx = HookContext::from_payload(&payload);
        assert_eq!(ctx.project_root, tmp.path());
        assert_eq!(ctx.wp_dir, tmp.path().join(".waypoint"));
    }

    #[test]
    fn hook_context_falls_back_to_file_path_for_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();

        let payload = serde_json::json!({
            "cwd": "/nonexistent",
            "tool_input": { "file_path": tmp.path().join("foo.rs").to_string_lossy().as_ref() }
        });

        let ctx = HookContext::from_payload(&payload);
        assert_eq!(ctx.project_root, tmp.path());
    }

    #[test]
    fn hook_context_falls_back_to_cwd_when_no_root() {
        let payload = serde_json::json!({
            "cwd": "/tmp",
            "tool_input": { "file_path": "/tmp/some_file.rs" }
        });

        let ctx = HookContext::from_payload(&payload);
        assert_eq!(ctx.project_root, Path::new("/tmp"));
    }

    #[test]
    fn relative_path_inside_project() {
        let ctx = HookContext {
            file_path: "/project/src/main.rs".into(),
            project_root: PathBuf::from("/project"),
            wp_dir: PathBuf::from("/project/.waypoint"),
        };
        assert_eq!(ctx.relative_path(), Some("src/main.rs".into()));
    }

    #[test]
    fn relative_path_outside_project() {
        let ctx = HookContext {
            file_path: "/other/file.rs".into(),
            project_root: PathBuf::from("/project"),
            wp_dir: PathBuf::from("/project/.waypoint"),
        };
        assert_eq!(ctx.relative_path(), None);
    }

    #[test]
    fn relative_path_empty_file_path() {
        let ctx = HookContext {
            file_path: String::new(),
            project_root: PathBuf::from("/project"),
            wp_dir: PathBuf::from("/project/.waypoint"),
        };
        assert_eq!(ctx.relative_path(), None);
    }
}
