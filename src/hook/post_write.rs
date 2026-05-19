use std::collections::HashMap;
use std::path::Path;

use crate::map::extract::{
    extract_description, extract_imports, extract_symbols, resolve_import_path,
};
use crate::map::scan::{content_hash, file_mtime, gzip_density, should_map_file};
use crate::map::{self, MapEntry, estimate_tokens, index};
use crate::{AppError, project};

/// Symbol names too generic to produce useful signature-change warnings.
const COMMON_NAMES: &[&str] = &[
    "new",
    "from",
    "default",
    "clone",
    "fmt",
    "drop",
    "into",
    "try_from",
    "try_into",
    "to_string",
    "as_ref",
    "deref",
    "eq",
    "hash",
    "cmp",
    "partial_cmp",
    "map",
    "filter",
    "reduce",
    "then",
    "catch",
    "get",
    "set",
    "init",
    "create",
    "delete",
    "update",
    "find",
];

/// Maximum importer files to list in a signature-change warning.
const MAX_IMPORTERS_SHOWN: usize = 5;

/// PostToolUse:Edit|Write — incremental map/symbol/import update.
///
/// After every Edit or Write, re-parses the changed file and updates the map
/// entry, symbol index, and import index for that single file. Detects exported
/// signature changes and warns about downstream callers. Cleans stale sibling
/// entries left behind by renames.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    let Some((wp_dir, relative, project_root)) = resolve_target(&ctx) else {
        super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
        return Ok(());
    };

    // Skip files that should not be in the map (binary, hidden, etc.)
    let rel_path = Path::new(&relative);
    if !should_map_file(rel_path) {
        super::emit_hook_output(super::HookEvent::PostToolUse, None, "");
        return Ok(());
    }

    let abs_path = project_root.join(&relative);
    let mut output_parts: Vec<String> = Vec::new();

    if abs_path.exists() {
        update_existing_file(
            &wp_dir,
            &project_root,
            &abs_path,
            &relative,
            &mut output_parts,
        )?;
    } else {
        remove_deleted_file(&wp_dir, &relative)?;
        output_parts.push(format!("[waypoint] map removed: {relative}"));
    }

    let context = output_parts.join("\n");
    super::emit_hook_output(super::HookEvent::PostToolUse, None, &context);
    Ok(())
}

/// Resolve the target file to its waypoint project.
/// Returns `None` if no usable `.waypoint/map.md` exists.
fn resolve_target(
    ctx: &super::HookContext,
) -> Option<(std::path::PathBuf, String, std::path::PathBuf)> {
    // Try foreign project resolution first (handles cross-project edits)
    if let Some(resolved) = project::resolve_foreign(&ctx.file_path) {
        let map_path = resolved.wp_dir.join("map.md");
        if map_path.exists() {
            return Some((resolved.wp_dir, resolved.relative_path, resolved.root));
        }
    }

    // Fall back to cwd project
    let relative = ctx.relative_path()?;
    let map_path = ctx.wp_dir.join("map.md");
    if map_path.exists() {
        Some((ctx.wp_dir.clone(), relative, ctx.project_root.clone()))
    } else {
        None
    }
}

/// Update map entry, symbols, and imports for an existing file.
fn update_existing_file(
    wp_dir: &Path,
    project_root: &Path,
    abs_path: &Path,
    relative: &str,
    output: &mut Vec<String>,
) -> Result<(), AppError> {
    let Ok(content) = std::fs::read_to_string(abs_path) else {
        // Unreadable file (binary, permissions) — skip silently
        return Ok(());
    };

    if content.trim().is_empty() {
        // Empty/whitespace-only — treat as deleted from map
        remove_deleted_file(wp_dir, relative)?;
        output.push(format!("[waypoint] map removed: {relative}"));
        return Ok(());
    }

    // Build and upsert map entry
    let description = extract_description(abs_path, &content);
    let token_estimate = estimate_tokens(&content, abs_path);
    let density = gzip_density(content.as_bytes());
    let hash = content_hash(content.as_bytes());
    let mtime = file_mtime(abs_path);

    let entry = MapEntry {
        path: relative.to_string(),
        description,
        token_estimate,
        density,
        content_hash: Some(hash),
        mtime_ms: mtime,
    };
    map::update_entry(wp_dir, entry)?;
    output.push(format!("[waypoint] map updated: {relative}"));

    // Snapshot exported symbols BEFORE replacing them (for signature comparison)
    let old_exported = index::exported_symbols_for_file(wp_dir, relative).unwrap_or_default();

    // Extract and update symbols (best-effort — don't fail the hook)
    let ext = abs_path.extension().and_then(|e| e.to_str());
    if matches!(
        ext,
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go")
    ) {
        let mut file_symbols = extract_symbols(abs_path, &content);
        for sym in &mut file_symbols {
            sym.file_path = relative.to_string();
        }
        let _ = index::update_file_symbols(wp_dir, relative, &file_symbols);

        // Extract and update imports
        let ext_str = ext.unwrap_or("");
        let mut file_imports = extract_imports(abs_path, &content);
        for imp in &mut file_imports {
            imp.source_file = relative.to_string();
            if let Some(resolved) =
                resolve_import_path(&imp.source_file, &imp.raw_path, ext_str, project_root)
            {
                imp.target_path = resolved;
            }
        }
        let _ = index::update_file_imports(wp_dir, relative, &file_imports);

        // Detect signature changes on exported symbols
        let sig_warnings = detect_signature_changes(wp_dir, relative, &old_exported, &file_symbols);
        if !sig_warnings.is_empty() {
            output.push(sig_warnings);
        }
    }

    // Clean stale sibling entries (rename detection)
    let stale = collect_stale_siblings(wp_dir, project_root, relative);
    if !stale.is_empty() {
        let cleaned = remove_stale_entries(wp_dir, &stale);
        for path in &cleaned {
            output.push(format!("[waypoint] stale removed: {path}"));
        }
    }

    Ok(())
}

/// Remove a deleted file from map, symbols, and imports.
fn remove_deleted_file(wp_dir: &Path, relative: &str) -> Result<(), AppError> {
    let _ = index::remove_file_symbols(wp_dir, relative);
    let _ = index::remove_file_imports(wp_dir, relative);
    // remove from map_entries index
    let _ = index::remove(wp_dir, relative);
    // remove from map.md
    map::remove_entry(wp_dir, relative)?;
    Ok(())
}

/// Compare old vs new exported symbols to detect signature changes.
/// Returns formatted warning string (empty if no changes detected).
fn detect_signature_changes(
    wp_dir: &Path,
    file_path: &str,
    old_exported: &[index::SymbolRow],
    new_symbols: &[crate::map::extract::Symbol],
) -> String {
    let old_sigs: HashMap<&str, &str> = old_exported
        .iter()
        .map(|s| (s.name.as_str(), s.signature.as_str()))
        .collect();

    let mut warnings = Vec::new();

    for sym in new_symbols {
        if !sym.exported {
            continue;
        }
        if COMMON_NAMES.contains(&sym.name.as_str()) {
            continue;
        }
        let Some(old_sig) = old_sigs.get(sym.name.as_str()) else {
            continue; // New symbol, not a change
        };
        if *old_sig == sym.signature {
            continue; // Unchanged
        }

        // Signature changed — find callers
        let importers =
            index::find_importers(wp_dir, &sym.name, Some(file_path)).unwrap_or_default();
        if importers.is_empty() {
            continue; // No downstream impact
        }

        let file_list: Vec<String> = importers
            .iter()
            .take(MAX_IMPORTERS_SHOWN)
            .map(|(f, line)| format!("{f}:{line}"))
            .collect();
        let shown = file_list.join(", ");
        let total = importers.len();
        let suffix = if total > MAX_IMPORTERS_SHOWN {
            format!(" (+{} more)", total - MAX_IMPORTERS_SHOWN)
        } else {
            String::new()
        };

        warnings.push(format!(
            "[waypoint] signature changed: {} — {total} caller(s): {shown}{suffix}\n  → run: waypoint callers {}",
            sym.name, sym.name
        ));
    }

    warnings.join("\n")
}

/// Find map entries in the same directory that no longer exist on disk.
fn collect_stale_siblings(
    wp_dir: &Path,
    project_root: &Path,
    current_relative: &str,
) -> Vec<String> {
    let dir_prefix = Path::new(current_relative)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let Ok(siblings) = index::entries_in_dir(wp_dir, &dir_prefix) else {
        return Vec::new();
    };

    siblings
        .into_iter()
        .filter(|path| path != current_relative && !project_root.join(path).exists())
        .collect()
}

/// Remove stale entries from index and map. Returns paths successfully cleaned.
fn remove_stale_entries(wp_dir: &Path, stale_paths: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    for path in stale_paths {
        let _ = index::remove(wp_dir, path);
        let _ = index::remove_file_symbols(wp_dir, path);
        let _ = index::remove_file_imports(wp_dir, path);
        cleaned.push(path.clone());
    }

    // Batch-remove from map.md
    if !cleaned.is_empty()
        && let Ok(mut entries) = map::read_map(wp_dir)
    {
        let before = entries.len();
        entries.retain(|e| !cleaned.contains(&e.path));
        if entries.len() < before {
            let _ = map::write_map(wp_dir, &entries);
        }
    }

    cleaned
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::map::extract::{Import, Symbol};
    use tempfile::TempDir;

    fn setup_waypoint_project(tmp: &TempDir) -> std::path::PathBuf {
        let wp_dir = tmp.path().join(".waypoint");
        std::fs::create_dir_all(&wp_dir).unwrap();
        // Initialize with empty map
        map::write_map(&wp_dir, &[]).unwrap();
        wp_dir
    }

    #[test]
    fn update_entry_adds_to_map() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let entry = MapEntry {
            path: "src/main.rs".into(),
            description: "entry point".into(),
            token_estimate: 100,
            ..Default::default()
        };
        map::update_entry(&wp_dir, entry).unwrap();

        let entries = map::read_map(&wp_dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/main.rs");
        assert_eq!(entries[0].description, "entry point");
    }

    #[test]
    fn update_entry_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let original = MapEntry {
            path: "src/lib.rs".into(),
            description: "original".into(),
            token_estimate: 50,
            ..Default::default()
        };
        map::update_entry(&wp_dir, original).unwrap();

        let updated = MapEntry {
            path: "src/lib.rs".into(),
            description: "updated".into(),
            token_estimate: 200,
            ..Default::default()
        };
        map::update_entry(&wp_dir, updated).unwrap();

        let entries = map::read_map(&wp_dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "updated");
        assert_eq!(entries[0].token_estimate, 200);
    }

    #[test]
    fn remove_entry_removes_from_map() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let entry = MapEntry {
            path: "src/foo.rs".into(),
            description: "foo".into(),
            token_estimate: 50,
            ..Default::default()
        };
        map::update_entry(&wp_dir, entry).unwrap();
        map::remove_entry(&wp_dir, "src/foo.rs").unwrap();

        let entries = map::read_map(&wp_dir).unwrap();
        assert!(entries.is_empty());
    }

    fn sample_symbol(file: &str, name: &str, sig: &str, exported: bool) -> Symbol {
        Symbol {
            file_path: file.into(),
            name: name.into(),
            kind: "fn".into(),
            signature: sig.into(),
            line_start: 1,
            line_end: 10,
            exported,
        }
    }

    #[test]
    fn update_file_symbols_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let old_syms = vec![sample_symbol("src/a.rs", "old_fn", "fn old_fn()", true)];
        index::update_file_symbols(&wp_dir, "src/a.rs", &old_syms).unwrap();

        let new_syms = vec![sample_symbol("src/a.rs", "new_fn", "fn new_fn()", true)];
        index::update_file_symbols(&wp_dir, "src/a.rs", &new_syms).unwrap();

        let results = index::sketch(&wp_dir, "old_fn").unwrap();
        assert!(results.is_empty(), "old symbol should be gone");

        let results = index::sketch(&wp_dir, "new_fn").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn update_file_symbols_does_not_affect_other_files() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let syms_a = vec![sample_symbol("src/a.rs", "func_a", "fn func_a()", true)];
        let syms_b = vec![sample_symbol("src/b.rs", "func_b", "fn func_b()", true)];
        index::update_file_symbols(&wp_dir, "src/a.rs", &syms_a).unwrap();
        index::update_file_symbols(&wp_dir, "src/b.rs", &syms_b).unwrap();

        // Replace a.rs symbols — b.rs should be untouched
        let new_a = vec![sample_symbol("src/a.rs", "func_a2", "fn func_a2()", true)];
        index::update_file_symbols(&wp_dir, "src/a.rs", &new_a).unwrap();

        let results = index::sketch(&wp_dir, "func_b").unwrap();
        assert_eq!(results.len(), 1, "other file's symbols should survive");
    }

    #[test]
    fn remove_file_symbols_clears() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let syms = vec![sample_symbol("src/a.rs", "target", "fn target()", true)];
        index::update_file_symbols(&wp_dir, "src/a.rs", &syms).unwrap();
        index::remove_file_symbols(&wp_dir, "src/a.rs").unwrap();

        let results = index::sketch(&wp_dir, "target").unwrap();
        assert!(results.is_empty());
    }

    fn sample_import(source: &str, name: &str, target: &str) -> Import {
        Import {
            source_file: source.into(),
            imported_name: name.into(),
            target_path: target.into(),
            raw_path: format!("./{target}"),
            line_number: 1,
        }
    }

    #[test]
    fn update_file_imports_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        // Seed symbols so find_importers join works
        let syms = vec![
            sample_symbol("src/utils.rs", "helper", "fn helper()", true),
            sample_symbol("src/utils.rs", "other", "fn other()", true),
        ];
        index::update_file_symbols(&wp_dir, "src/utils.rs", &syms).unwrap();

        let old_imports = vec![sample_import("src/main.rs", "helper", "src/utils.rs")];
        index::update_file_imports(&wp_dir, "src/main.rs", &old_imports).unwrap();

        let new_imports = vec![sample_import("src/main.rs", "other", "src/utils.rs")];
        index::update_file_imports(&wp_dir, "src/main.rs", &new_imports).unwrap();

        // Old import should be gone
        let results = index::find_importers(&wp_dir, "helper", Some("src/utils.rs")).unwrap();
        assert!(results.is_empty());

        // New import should exist
        let results = index::find_importers(&wp_dir, "other", Some("src/utils.rs")).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn remove_file_imports_clears() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let syms = vec![sample_symbol("src/utils.rs", "helper", "fn helper()", true)];
        index::update_file_symbols(&wp_dir, "src/utils.rs", &syms).unwrap();

        let imports = vec![sample_import("src/main.rs", "helper", "src/utils.rs")];
        index::update_file_imports(&wp_dir, "src/main.rs", &imports).unwrap();
        index::remove_file_imports(&wp_dir, "src/main.rs").unwrap();

        let results = index::find_importers(&wp_dir, "helper", Some("src/utils.rs")).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn exported_symbols_for_file_returns_only_exported() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let syms = vec![
            sample_symbol("src/lib.rs", "pub_fn", "pub fn pub_fn()", true),
            sample_symbol("src/lib.rs", "priv_fn", "fn priv_fn()", false),
        ];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &syms).unwrap();

        let exported = index::exported_symbols_for_file(&wp_dir, "src/lib.rs").unwrap();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name, "pub_fn");
    }

    #[test]
    fn entries_in_dir_top_level() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let entries = vec![
            MapEntry {
                path: "README.md".into(),
                description: "readme".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/main.rs".into(),
                description: "main".into(),
                token_estimate: 50,
                ..Default::default()
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();

        let top = index::entries_in_dir(&wp_dir, ".").unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0], "README.md");
    }

    #[test]
    fn entries_in_dir_direct_children_only() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let entries = vec![
            MapEntry {
                path: "src/a.rs".into(),
                description: "a".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/b.rs".into(),
                description: "b".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/sub/c.rs".into(),
                description: "c".into(),
                token_estimate: 10,
                ..Default::default()
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();

        let children = index::entries_in_dir(&wp_dir, "src").unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&"src/a.rs".to_string()));
        assert!(children.contains(&"src/b.rs".to_string()));
        assert!(!children.contains(&"src/sub/c.rs".to_string()));
    }

    #[test]
    fn detect_signature_changes_warns_on_changed_export() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        // Old exported symbol
        let old_syms = vec![sample_symbol(
            "src/lib.rs",
            "process",
            "pub fn process(x: i32)",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &old_syms).unwrap();

        // Set up an importer
        let imports = vec![sample_import("src/main.rs", "process", "src/lib.rs")];
        index::update_file_imports(&wp_dir, "src/main.rs", &imports).unwrap();

        let old_exported = index::exported_symbols_for_file(&wp_dir, "src/lib.rs").unwrap();

        // New symbols with changed signature
        let new_syms = vec![sample_symbol(
            "src/lib.rs",
            "process",
            "pub fn process(x: i32, y: i32)",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &new_syms).unwrap();

        let warnings = detect_signature_changes(&wp_dir, "src/lib.rs", &old_exported, &new_syms);
        assert!(
            warnings.contains("signature changed: process"),
            "got: {warnings}"
        );
        assert!(warnings.contains("src/main.rs"), "got: {warnings}");
        assert!(
            warnings.contains("waypoint callers process"),
            "got: {warnings}"
        );
    }

    #[test]
    fn detect_signature_changes_skips_non_exported() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let old = vec![sample_symbol("src/lib.rs", "helper", "fn helper()", true)];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &old).unwrap();
        let old_exported = index::exported_symbols_for_file(&wp_dir, "src/lib.rs").unwrap();

        // Changed signature but now non-exported
        let new_syms = vec![sample_symbol(
            "src/lib.rs",
            "helper",
            "fn helper(x: i32)",
            false,
        )];

        let warnings = detect_signature_changes(&wp_dir, "src/lib.rs", &old_exported, &new_syms);
        assert!(warnings.is_empty());
    }

    #[test]
    fn detect_signature_changes_skips_common_names() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let old = vec![sample_symbol(
            "src/lib.rs",
            "new",
            "pub fn new() -> Self",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &old).unwrap();

        let imports = vec![sample_import("src/main.rs", "new", "src/lib.rs")];
        index::update_file_imports(&wp_dir, "src/main.rs", &imports).unwrap();

        let old_exported = index::exported_symbols_for_file(&wp_dir, "src/lib.rs").unwrap();

        let new_syms = vec![sample_symbol(
            "src/lib.rs",
            "new",
            "pub fn new(config: Config) -> Self",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &new_syms).unwrap();

        let warnings = detect_signature_changes(&wp_dir, "src/lib.rs", &old_exported, &new_syms);
        assert!(warnings.is_empty(), "common name 'new' should be skipped");
    }

    #[test]
    fn detect_signature_changes_no_warning_without_importers() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let old = vec![sample_symbol(
            "src/lib.rs",
            "isolated_fn",
            "pub fn isolated_fn()",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &old).unwrap();
        let old_exported = index::exported_symbols_for_file(&wp_dir, "src/lib.rs").unwrap();

        let new_syms = vec![sample_symbol(
            "src/lib.rs",
            "isolated_fn",
            "pub fn isolated_fn(x: i32)",
            true,
        )];
        index::update_file_symbols(&wp_dir, "src/lib.rs", &new_syms).unwrap();

        let warnings = detect_signature_changes(&wp_dir, "src/lib.rs", &old_exported, &new_syms);
        assert!(warnings.is_empty(), "no importers means no warning");
    }

    #[test]
    fn collect_stale_siblings_finds_renamed_file() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        // Create src/ directory with one real file
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("new_name.rs"), "fn main() {}").unwrap();
        // old_name.rs does NOT exist on disk

        let entries = vec![
            MapEntry {
                path: "src/old_name.rs".into(),
                description: "old".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/new_name.rs".into(),
                description: "new".into(),
                token_estimate: 10,
                ..Default::default()
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();

        let stale = collect_stale_siblings(&wp_dir, tmp.path(), "src/new_name.rs");
        assert_eq!(stale, vec!["src/old_name.rs"]);
    }

    #[test]
    fn collect_stale_siblings_ignores_existing_files() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(src_dir.join("b.rs"), "fn b() {}").unwrap();

        let entries = vec![
            MapEntry {
                path: "src/a.rs".into(),
                description: "a".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/b.rs".into(),
                description: "b".into(),
                token_estimate: 10,
                ..Default::default()
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();

        let stale = collect_stale_siblings(&wp_dir, tmp.path(), "src/a.rs");
        assert!(stale.is_empty());
    }

    #[test]
    fn remove_stale_entries_cleans_map_and_index() {
        let tmp = TempDir::new().unwrap();
        let wp_dir = setup_waypoint_project(&tmp);

        let entries = vec![
            MapEntry {
                path: "src/keep.rs".into(),
                description: "keep".into(),
                token_estimate: 10,
                ..Default::default()
            },
            MapEntry {
                path: "src/stale.rs".into(),
                description: "stale".into(),
                token_estimate: 10,
                ..Default::default()
            },
        ];
        map::write_map(&wp_dir, &entries).unwrap();

        let syms = vec![sample_symbol("src/stale.rs", "old_fn", "fn old_fn()", true)];
        index::update_file_symbols(&wp_dir, "src/stale.rs", &syms).unwrap();

        let cleaned = remove_stale_entries(&wp_dir, &["src/stale.rs".to_string()]);
        assert_eq!(cleaned, vec!["src/stale.rs"]);

        // Map should only have keep.rs
        let remaining = map::read_map(&wp_dir).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].path, "src/keep.rs");

        // Symbols for stale file should be gone
        let results = index::sketch(&wp_dir, "old_fn").unwrap();
        assert!(results.is_empty());
    }
}
