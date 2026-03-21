use crate::AppError;

/// FR-6: Stop probe.
///
/// Logs the full input contract to .waypoint/spike-stop.json and exits cleanly.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    super::log_to_waypoint_dir(&payload, "spike-stop.json")?;
    Ok(())
}
