use std::path::Path;

use crate::{AppError, ledger, map, project};

fn resolve_target(ctx: &super::HookContext) -> Option<(std::path::PathBuf, String)> {
    if let Some(resolved) = project::resolve_foreign(&ctx.file_path)
        && resolved.wp_dir.join("map.md").exists()
    {
        return Some((resolved.wp_dir, resolved.relative_path));
    }

    if let Some(relative) = ctx.relative_path()
        && ctx.wp_dir.exists()
        && ctx.wp_dir.join("map.md").exists()
    {
        return Some((ctx.wp_dir.clone(), relative));
    }

    None
}

/// FR-3: PostToolUse:Edit|Write — update map entry for the changed file.
/// FR-10: Resolve foreign project for map/symbol updates.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    let Some((wp_dir, relative)) = resolve_target(&ctx) else {
        super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
        return Ok(());
    };

    if let Some(project_root) = wp_dir.parent() {
        let _ = ledger::record_first_edit_if_needed(&project_root.to_string_lossy());
    }

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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn setup_waypoint_project() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".waypoint")).unwrap();
        std::fs::write(tmp.path().join(".waypoint/map.md"), "# map\n").unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        tmp
    }

    fn payload(cwd: &TempDir, target: &TempDir) -> serde_json::Value {
        serde_json::json!({
            "cwd": cwd.path().to_string_lossy(),
            "tool_input": {
                "file_path": target.path().join("src/main.rs").to_string_lossy().as_ref()
            }
        })
    }

    #[test]
    fn resolve_target_uses_foreign_project_waypoint() {
        let project_a = setup_waypoint_project();
        let project_b = setup_waypoint_project();
        let ctx = super::super::HookContext::from_payload(&payload(&project_a, &project_b));

        let (wp_dir, relative) = resolve_target(&ctx).unwrap();

        assert_eq!(wp_dir, project_b.path().join(".waypoint"));
        assert_eq!(relative, "src/main.rs");
    }

    #[test]
    fn resolve_target_returns_none_for_unmanaged_foreign_project() {
        let project_a = setup_waypoint_project();
        let project_b = TempDir::new().unwrap();
        std::fs::create_dir(project_b.path().join(".git")).unwrap();
        std::fs::create_dir_all(project_b.path().join("src")).unwrap();
        std::fs::write(project_b.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let ctx = super::super::HookContext::from_payload(&payload(&project_a, &project_b));

        assert!(resolve_target(&ctx).is_none());
    }
}
