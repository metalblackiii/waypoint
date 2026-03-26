use crate::{AppError, project};

/// FR-16: PostToolUseFailure:Edit|Write — suggest trap search.
/// FR-11: Include -C context for foreign files.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or("<unknown>");
    let cwd = super::extract_cwd(&payload).unwrap_or(".");

    // Only suggest -C when the file belongs to a different project than cwd
    let cwd_root = project::find_root(std::path::Path::new(cwd));
    let context_flag = project::resolve_foreign(file_path)
        .filter(|resolved| cwd_root.as_ref() != Some(&resolved.root))
        .map(|resolved| format!(" -C \"{}\"", resolved.root.display()));

    let context = if let Some(flag) = context_flag {
        format!(
            "[waypoint] Edit/Write failed for {file_path}. \
             If this is a known issue, check: waypoint trap search{flag} \"<error>\""
        )
    } else {
        format!(
            "[waypoint] Edit/Write failed for {file_path}. \
             If this is a known issue, check: waypoint trap search \"<error>\""
        )
    };

    super::emit_hook_output(super::HookEvent::PostToolUse, None, &context);
    Ok(())
}
