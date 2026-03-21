use crate::AppError;

/// FR-3: SessionStart probe.
///
/// Logs the full input contract to .waypoint/spike-session-start.json.
/// Writes context text to stdout (SessionStart uses plain stdout, not JSON additionalContext).
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    super::log_to_waypoint_dir(&payload, "spike-session-start.json")?;

    // SessionStart: plain stdout becomes context Claude sees
    print!("[waypoint] journal: 2 preferences, 1 do-not-repeat entry loaded");
    Ok(())
}
