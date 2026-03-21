use std::path::Path;

use crate::{AppError, map};

/// FR-3: PostToolUse:Edit|Write — update map entry for the changed file.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    if !ctx.wp_dir.exists() || !ctx.wp_dir.join("map.md").exists() {
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    }

    let Some(relative) = ctx.relative_path() else {
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    };

    let rel_path = Path::new(&relative);
    if !map::scan::should_map_file(rel_path) {
        super::emit_hook_output("PostToolUse", None, "");
        return Ok(());
    }

    let abs_path = Path::new(&ctx.file_path);
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

            map::update_entry(&ctx.wp_dir, entry)?;
        }
    } else {
        // File was deleted — remove from map
        let mut entries = map::read_map(&ctx.wp_dir)?;
        entries.retain(|e| e.path != relative);
        map::write_map(&ctx.wp_dir, &entries)?;
    }

    super::emit_hook_output(
        "PostToolUse",
        None,
        &format!("[waypoint] map updated: {relative}"),
    );
    Ok(())
}
