pub mod post_write;
pub mod pre_read;
pub mod pre_write;
pub mod session_start;
pub mod stop;

use serde::Deserialize;

/// Common fields present in all hook stdin payloads.
#[derive(Debug, Deserialize)]
pub struct HookInput {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub hook_event_name: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
}

/// Read full stdin into a string and parse as JSON.
pub fn read_stdin() -> Result<serde_json::Value, crate::AppError> {
    let input = std::io::read_to_string(std::io::stdin())?;
    let value: serde_json::Value = serde_json::from_str(&input)?;
    Ok(value)
}

/// Extract file_path from tool_input in the hook payload.
pub fn extract_file_path(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("tool_input")
        .and_then(|ti| ti.get("file_path"))
        .and_then(|fp| fp.as_str())
        .map(String::from)
}

/// Write a JSON payload to .waypoint/<filename> relative to cwd from the hook input.
pub fn log_to_waypoint_dir(
    payload: &serde_json::Value,
    filename: &str,
) -> Result<(), crate::AppError> {
    let cwd = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let dir = std::path::Path::new(cwd).join(".waypoint");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    let formatted = serde_json::to_string_pretty(payload)?;
    std::fs::write(path, formatted)?;
    Ok(())
}
