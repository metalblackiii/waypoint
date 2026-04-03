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

/// Check sibling entries in the same directory for files that no longer exist on disk.
/// Returns relative paths of stale entries.
fn collect_stale_siblings(
    wp_dir: &Path,
    project_root: Option<&Path>,
    current_relative: &str,
) -> Vec<String> {
    let Some(root) = project_root else {
        return Vec::new();
    };

    let dir_prefix = Path::new(current_relative)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let Ok(siblings) = map::index::entries_in_dir(wp_dir, &dir_prefix) else {
        return Vec::new();
    };

    siblings
        .into_iter()
        .filter(|path| *path != current_relative && !root.join(path).exists())
        .collect()
}

/// Remove stale entries from the index, symbol tables, and map.md.
///
/// Attempts DB removal for each path, tracking which succeeded. Map.md is
/// then updated to remove only the successfully-cleaned DB entries, keeping
/// the two in sync even on partial failure.
fn remove_stale_entries(wp_dir: &Path, stale: &[String]) -> Result<(), AppError> {
    let mut cleaned: Vec<&str> = Vec::with_capacity(stale.len());

    for path in stale {
        if map::index::remove(wp_dir, path).is_ok() {
            cleaned.push(path);
            // Best-effort symbol cleanup — index row (stale discovery driver)
            // is already gone, so map.md will be updated regardless.
            let _ = map::index::remove_file_symbols(wp_dir, path);
        }
    }

    if !cleaned.is_empty() {
        let mut entries = map::read_map(wp_dir)?;
        entries.retain(|e| !cleaned.contains(&e.path.as_str()));
        map::write_map(wp_dir, &entries)?;
    }

    Ok(())
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
    let project_root = wp_dir.parent();

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

        // Clean up stale sibling entries (catches renames without manual scan)
        let stale = collect_stale_siblings(&wp_dir, project_root, &relative);
        if !stale.is_empty() {
            remove_stale_entries(&wp_dir, &stale)?;
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

    #[test]
    fn collect_stale_siblings_finds_renamed_file() {
        let tmp = setup_waypoint_project();
        let wp_dir = tmp.path().join(".waypoint");

        // Seed index with two entries in src/
        let old_entry = map::MapEntry {
            path: "src/old_name.rs".into(),
            description: "old file".into(),
            token_estimate: 100,
        };
        let main_entry = map::MapEntry {
            path: "src/main.rs".into(),
            description: "main".into(),
            token_estimate: 50,
        };
        map::index::upsert(&wp_dir, &old_entry).unwrap();
        map::index::upsert(&wp_dir, &main_entry).unwrap();

        // src/old_name.rs doesn't exist on disk — it's stale
        let stale = collect_stale_siblings(&wp_dir, Some(tmp.path()), "src/main.rs");
        assert_eq!(stale, vec!["src/old_name.rs"]);
    }

    #[test]
    fn collect_stale_siblings_ignores_existing_files() {
        let tmp = setup_waypoint_project();
        let wp_dir = tmp.path().join(".waypoint");

        let main_entry = map::MapEntry {
            path: "src/main.rs".into(),
            description: "main".into(),
            token_estimate: 50,
        };
        map::index::upsert(&wp_dir, &main_entry).unwrap();

        // src/main.rs exists on disk — nothing stale
        let stale = collect_stale_siblings(&wp_dir, Some(tmp.path()), "src/main.rs");
        assert!(stale.is_empty());
    }

    #[test]
    fn remove_stale_entries_cleans_map_and_index() {
        let tmp = setup_waypoint_project();
        let wp_dir = tmp.path().join(".waypoint");

        // Write map.md with two entries
        let entries = vec![
            map::MapEntry {
                path: "src/main.rs".into(),
                description: "main".into(),
                token_estimate: 50,
            },
            map::MapEntry {
                path: "src/old.rs".into(),
                description: "old".into(),
                token_estimate: 100,
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();
        for e in &entries {
            map::index::upsert(&wp_dir, e).unwrap();
        }

        remove_stale_entries(&wp_dir, &["src/old.rs".into()]).unwrap();

        // Index should no longer have the stale entry
        assert!(map::index::lookup(&wp_dir, "src/old.rs").unwrap().is_none());
        // map.md should only have main.rs
        let remaining = map::read_map(&wp_dir).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].path, "src/main.rs");
    }
}
