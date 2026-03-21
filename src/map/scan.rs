use std::path::Path;

use ignore::WalkBuilder;

use super::extract::extract_description;
use super::{MapEntry, estimate_tokens};
use crate::AppError;

/// Walk the project directory respecting .gitignore, parse files, return map entries.
pub fn scan_project(project_root: &Path) -> Result<Vec<MapEntry>, AppError> {
    let mut entries = Vec::new();

    let walker = WalkBuilder::new(project_root)
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
        .build();

    for result in walker {
        let Ok(entry) = result else { continue };

        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }

        let path = entry.path();

        if !is_scannable(path) {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(path) else { continue };

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

        entries.push(MapEntry {
            path: relative,
            description,
            token_estimate,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
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
fn is_scannable(path: &Path) -> bool {
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
