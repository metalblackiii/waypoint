use std::path::Path;

use crate::{AppError, map, project};

/// FR-3: PostToolUse:Edit|Write — update map entry for the changed file.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let file_path = super::extract_file_path(&payload).unwrap_or("");
    let cwd = super::extract_cwd(&payload).unwrap_or(".");
    let cwd_path = Path::new(cwd);

    let project_root = project::find_root(cwd_path)
        .or_else(|| project::find_root(Path::new(file_path)))
        .unwrap_or_else(|| cwd_path.to_path_buf());

    let wp_dir = project::waypoint_dir(&project_root);

    if !wp_dir.exists() || !wp_dir.join("map.md").exists() {
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    }

    let abs_path = Path::new(file_path);
    let Ok(stripped) = abs_path.strip_prefix(&project_root) else {
        // File is outside this project — don't pollute the map
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    };
    let relative = stripped.to_string_lossy().to_string();

    if !map::scan::should_map_file(stripped) {
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    }

    if abs_path.exists() {
        // Re-parse the changed file and update its map entry
        if let Ok(content) = std::fs::read_to_string(abs_path) {
            let description = map::extract::extract_description(abs_path, &content);
            let token_estimate = map::estimate_tokens(&content, abs_path);

            let entry = map::MapEntry {
                path: relative.clone(),
                description,
                token_estimate,
            };

            map::update_entry(&wp_dir, entry)?;
        }
    } else {
        // File was deleted — remove from map
        let mut entries = map::read_map(&wp_dir)?;
        entries.retain(|e| e.path != relative);
        map::write_map(&wp_dir, &entries)?;
    }

    super::emit_hook_output(
        "PostToolUse",
        None,
        &format!("[waypoint] map updated: {relative}"),
    );
    Ok(())
}
