use std::path::Path;

use crate::{AppError, map, project};

/// FR-3: PostToolUse:Edit|Write — update map entry for the changed file.
/// FR-10: Resolve foreign project for map/symbol updates.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    // Resolve the file's own project first, fall back to cwd project
    let (wp_dir, relative) = if let Some(resolved) = project::resolve_foreign(&ctx.file_path) {
        if resolved.wp_dir.join("map.md").exists() {
            (resolved.wp_dir, resolved.relative_path)
        } else {
            super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
            return Ok(());
        }
    } else if let Some(rel) = ctx.relative_path() {
        if ctx.wp_dir.exists() && ctx.wp_dir.join("map.md").exists() {
            (ctx.wp_dir.clone(), rel)
        } else {
            super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
            return Ok(());
        }
    } else {
        super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
        return Ok(());
    };

    let rel_path = Path::new(&relative);
    if !map::scan::should_map_file(rel_path) {
        super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
        return Ok(());
    }

    let abs_path = Path::new(&ctx.file_path);
    if abs_path.exists() {
        // Re-parse the changed file and update its map entry + symbols
        if let Ok(content) = std::fs::read_to_string(abs_path) {
            let description = map::extract::extract_description(abs_path, &content);
            let token_estimate = map::estimate_tokens(&content, abs_path);

            let entry = map::MapEntry {
                path: relative.clone(),
                description,
                token_estimate,
            };

            map::update_entry(&wp_dir, entry)?;

            // Update symbol index for this file
            let mut file_symbols = map::extract::extract_symbols(abs_path, &content);
            for sym in &mut file_symbols {
                sym.file_path.clone_from(&relative);
            }
            let _ = map::index::update_file_symbols(&wp_dir, &relative, &file_symbols);
        }
    } else {
        // File was deleted — remove from map, index, and symbols
        let _ = map::index::remove(&wp_dir, &relative);
        let _ = map::index::remove_file_symbols(&wp_dir, &relative);
        let mut entries = map::read_map(&wp_dir)?;
        entries.retain(|e| e.path != relative);
        map::write_map(&wp_dir, &entries)?;
    }

    super::emit_hook_output(
        super::HookEvent::PostToolUse,
        None,
        &format!("[waypoint] map updated: {relative}"),
    );
    Ok(())
}
