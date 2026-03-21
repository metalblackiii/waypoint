use crate::AppError;

/// FR-16: PostToolUseFailure:Edit|Write — suggest trap search.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or("<unknown>");

    let context = format!(
        "[waypoint] Edit/Write failed for {file_path}. \
         If this is a known issue, check: waypoint trap search \"<error>\""
    );

    super::emit_hook_output(super::HookEvent::PostToolUse, None, &context);
    Ok(())
}
