use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;

use super::extract::{
    Import, Symbol, extract_description, extract_imports, extract_symbols, resolve_import_path,
};
use super::{MapEntry, estimate_tokens};
use crate::AppError;

/// Minimum file size for meaningful density measurement (gzip header skews small files).
const DENSITY_MIN_BYTES: usize = 256;

/// Gzip compression ratio (compressed / original). Lower = more repetitive.
fn gzip_density(content: &[u8]) -> Option<f32> {
    if content.len() < DENSITY_MIN_BYTES {
        return None;
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(content).ok()?;
    let compressed = encoder.finish().ok()?;
    #[allow(clippy::cast_precision_loss)]
    Some(compressed.len() as f32 / content.len() as f32)
}

/// `SipHash` of file content for change detection.
fn content_hash(content: &[u8]) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    #[allow(clippy::cast_possible_wrap)]
    {
        hasher.finish() as i64
    }
}

/// File mtime as Unix epoch milliseconds for sub-second change detection.
///
/// # Migration from seconds (v0.9 → v0.10)
///
/// The `SQLite` column is still named `mtime_secs` but now stores milliseconds.
/// Pre-upgrade indexes contain second-resolution values that will mismatch
/// against millis, triggering a one-time rescan. This is intentional — the
/// rescan writes millis, making the index self-correcting. No version gate
/// is needed because `map_index.db` is a rebuildable cache.
fn file_mtime(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let duration = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    // Unix millis (~1.7 trillion) fits comfortably in i64 (~9.2 quintillion)
    Some(duration.as_millis() as i64)
}

/// Output from a full project scan: file-level entries + symbol-level data.
pub struct ScanOutput {
    pub entries: Vec<MapEntry>,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
}

/// Configured walker shared by `count_scannable_files`, `scan_project`, and mtime staleness.
pub(crate) fn project_walker(project_root: &Path) -> ignore::Walk {
    WalkBuilder::new(project_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".waypoint" | ".git" | "node_modules" | "__pycache__"
            )
        })
        .build()
}

/// Stat-only file count — same walk/filter as `scan_project` minus file reads.
/// Used by `session_start` to detect map staleness cheaply. Slightly overcounts
/// vs a full scan (includes empty/whitespace-only files) but the 10% drift
/// threshold absorbs that.
pub fn count_scannable_files(project_root: &Path) -> usize {
    project_walker(project_root)
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_type().is_some_and(|ft| ft.is_dir()))
        .filter(|entry| is_scannable(entry.path()))
        .count()
}

/// Walk the project directory respecting .gitignore, parse files, return map entries and symbols.
pub fn scan_project(project_root: &Path) -> Result<ScanOutput, AppError> {
    let mut entries = Vec::new();
    let mut symbols = Vec::new();
    let mut imports = Vec::new();

    let walker = project_walker(project_root);

    for result in walker {
        let Ok(entry) = result else { continue };

        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }

        let path = entry.path();

        if !is_scannable(path) {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        if content.trim().is_empty() {
            continue;
        }

        let relative = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let description = extract_description(path, &content);
        let token_estimate = estimate_tokens(&content, path);
        let density = gzip_density(content.as_bytes());
        let hash = content_hash(content.as_bytes());
        let mtime = file_mtime(path);

        // Extract structured symbols and imports from tree-sitter-supported files
        let ext = path.extension().and_then(|e| e.to_str());
        if matches!(
            ext,
            Some("rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go")
        ) {
            let mut file_symbols = extract_symbols(path, &content);
            for sym in &mut file_symbols {
                sym.file_path.clone_from(&relative);
            }
            symbols.extend(file_symbols);

            let ext_str = ext.unwrap_or("");
            let mut file_imports = extract_imports(path, &content);
            for imp in &mut file_imports {
                imp.source_file.clone_from(&relative);
                if let Some(resolved) =
                    resolve_import_path(&relative, &imp.raw_path, ext_str, project_root)
                {
                    imp.target_path = resolved;
                }
            }
            imports.extend(file_imports);
        }

        entries.push(MapEntry {
            path: relative,
            description,
            token_estimate,
            density,
            content_hash: Some(hash),
            mtime_ms: mtime,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanOutput {
        entries,
        symbols,
        imports,
    })
}

/// Check if a file should appear in the waypoint map: scannable type and no hidden components.
/// Accepts either absolute or relative paths — only the filename/extension and component names matter.
#[must_use]
pub fn should_map_file(path: &Path) -> bool {
    is_scannable(path) && !has_hidden_component(path)
}

/// Returns true if any path component starts with `.` (hidden file or directory).
fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|c| {
        let bytes = c.as_os_str().as_encoded_bytes();
        bytes.first() == Some(&b'.')
    })
}

/// Check if a file is a text file we should scan, based on extension or known name.
pub(crate) fn is_scannable(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        return matches!(
            name,
            "Makefile"
                | "Dockerfile"
                | "Justfile"
                | "justfile"
                | "Rakefile"
                | "Gemfile"
                | "Brewfile"
                | ".gitignore"
                | ".gitattributes"
                | ".editorconfig"
                | ".eslintrc"
                | ".prettierrc"
                | ".babelrc"
                | "LICENSE"
                | "CHANGELOG"
        );
    };

    matches!(
        ext,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "go"
            | "rb"
            | "java"
            | "kt"
            | "swift"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "ini"
            | "conf"
            | "cfg"
            | "md"
            | "mdx"
            | "txt"
            | "rst"
            | "adoc"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "sql"
            | "graphql"
            | "gql"
            | "tf"
            | "tfvars"
            | "hcl"
            | "proto"
            | "thrift"
            | "avsc"
            | "vue"
            | "svelte"
            | "lock"
    )
}
