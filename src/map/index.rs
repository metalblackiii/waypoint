use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::MapEntry;
use super::extract::{Import, Symbol};
use crate::AppError;

const INDEX_FILENAME: &str = "map_index.db";

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS map_entries (
    path TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    density REAL,
    content_hash INTEGER,
    mtime_secs INTEGER
);
CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    signature TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    exported INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_file TEXT NOT NULL,
    imported_name TEXT NOT NULL,
    target_path TEXT NOT NULL,
    raw_path TEXT NOT NULL,
    line_number INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_imports_source ON imports(source_file);
CREATE INDEX IF NOT EXISTS idx_imports_target ON imports(target_path);
CREATE INDEX IF NOT EXISTS idx_imports_name ON imports(imported_name);";

const FTS_SCHEMA: &str = "\
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(\
    name, kind, signature, file_path\
);";

const ARCH_SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS arch_summary (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    lang_dist TEXT NOT NULL,
    hotspots TEXT NOT NULL,
    file_count INTEGER NOT NULL
);";

/// Top-N languages shown in the arch summary language distribution line.
const ARCH_LANG_DISPLAY_LIMIT: usize = 4;
/// Top-N hotspot directories shown in the arch summary hotspots line.
const ARCH_HOTSPOT_DISPLAY_LIMIT: usize = 3;

pub(crate) fn open_index(waypoint_dir: &Path) -> Result<Connection, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(SCHEMA)?;
    // Migrate existing databases: add columns introduced in v0.10.
    // ALTER TABLE errors are silently ignored when columns already exist.
    for col in ["density REAL", "content_hash INTEGER", "mtime_secs INTEGER"] {
        let _ = conn.execute_batch(&format!("ALTER TABLE map_entries ADD COLUMN {col}"));
    }
    // FTS5 is best-effort — skip silently if unavailable
    let _ = conn.execute_batch(FTS_SCHEMA);
    let _ = conn.execute_batch(ARCH_SCHEMA);
    Ok(conn)
}

/// Row returned by sketch/find queries.
#[derive(Debug)]
pub struct SymbolRow {
    pub file_path: String,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub line_start: i64,
    pub line_end: i64,
    pub exported: bool,
}

/// O(1) lookup of a single map entry by relative path.
/// Returns `Err` if the index does not exist, signalling the caller to fall back.
pub fn lookup(waypoint_dir: &Path, relative_path: &str) -> Result<Option<MapEntry>, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    if !db_path.exists() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "map index not built yet",
        )));
    }
    let conn = open_index(waypoint_dir)?;
    let mut stmt = conn.prepare(
        "SELECT path, description, token_estimate, density, content_hash, mtime_secs \
         FROM map_entries WHERE path = ?1",
    )?;

    let entry = stmt
        .query_row(params![relative_path], |row| {
            Ok(MapEntry {
                path: row.get(0)?,
                description: row.get(1)?,
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                // token_estimate is always non-negative and fits in usize
                token_estimate: row.get::<_, i64>(2)? as usize,
                density: row.get(3)?,
                content_hash: row.get(4)?,
                mtime_ms: row.get(5)?,
            })
        })
        .optional()?;

    Ok(entry)
}

/// Insert or update a single entry in the index.
pub fn upsert(waypoint_dir: &Path, entry: &MapEntry) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;
    upsert_with(&conn, entry)
}

fn upsert_with(conn: &Connection, entry: &MapEntry) -> Result<(), AppError> {
    #[allow(clippy::cast_possible_wrap)]
    conn.execute(
        "INSERT OR REPLACE INTO map_entries \
         (path, description, token_estimate, density, content_hash, mtime_secs) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.path,
            entry.description,
            entry.token_estimate as i64,
            entry.density,
            entry.content_hash,
            entry.mtime_ms,
        ],
    )?;
    Ok(())
}

/// Remove an entry from the index by path.
pub fn remove(waypoint_dir: &Path, relative_path: &str) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;
    conn.execute(
        "DELETE FROM map_entries WHERE path = ?1",
        params![relative_path],
    )?;
    Ok(())
}

/// Rebuild the entire index from a set of entries. Uses a transaction for performance.
pub fn rebuild(waypoint_dir: &Path, entries: &[MapEntry]) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;
    let tx = conn.unchecked_transaction()?;

    tx.execute_batch("DELETE FROM map_entries")?;

    let mut stmt = tx.prepare(
        "INSERT INTO map_entries \
         (path, description, token_estimate, density, content_hash, mtime_secs) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for entry in entries {
        #[allow(clippy::cast_possible_wrap)]
        stmt.execute(params![
            entry.path,
            entry.description,
            entry.token_estimate as i64,
            entry.density,
            entry.content_hash,
            entry.mtime_ms,
        ])?;
    }

    drop(stmt);
    tx.commit()?;
    Ok(())
}

/// Rebuild the symbols and FTS tables from a full scan.
///
/// Clears stale data first (committed separately) so a mid-rebuild failure
/// leaves the tables empty rather than serving outdated symbols.
pub fn rebuild_symbols(waypoint_dir: &Path, symbols: &[Symbol]) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;

    // Clear stale data in a committed step — if the insert phase fails,
    // queries return empty rather than outdated results.
    conn.execute_batch("DELETE FROM symbols")?;
    let _ = conn.execute_batch("DELETE FROM symbols_fts");

    let tx = conn.unchecked_transaction()?;

    let mut stmt = tx.prepare(
        "INSERT INTO symbols (file_path, name, kind, signature, line_start, line_end, exported) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut fts_stmt = tx.prepare(
        "INSERT INTO symbols_fts (name, kind, signature, file_path) VALUES (?1, ?2, ?3, ?4)",
    );

    for sym in symbols {
        stmt.execute(params![
            sym.file_path,
            sym.name,
            sym.kind,
            sym.signature,
            sym.line_start,
            sym.line_end,
            i64::from(sym.exported)
        ])?;
        if let Ok(ref mut fts) = fts_stmt {
            let _ = fts.execute(params![sym.name, sym.kind, sym.signature, sym.file_path]);
        }
    }

    drop(stmt);
    drop(fts_stmt);
    tx.commit()?;
    Ok(())
}

/// Rebuild the imports table from a full scan.
pub fn rebuild_imports(waypoint_dir: &Path, imports: &[Import]) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;

    conn.execute_batch("DELETE FROM imports")?;

    let tx = conn.unchecked_transaction()?;
    let mut stmt = tx.prepare(
        "INSERT INTO imports (source_file, imported_name, target_path, raw_path, line_number) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for imp in imports {
        stmt.execute(params![
            imp.source_file,
            imp.imported_name,
            imp.target_path,
            imp.raw_path,
            imp.line_number
        ])?;
    }

    drop(stmt);
    tx.commit()?;
    Ok(())
}

/// Cached architecture summary for a project.
#[derive(Debug)]
pub struct ArchSummary {
    pub lang_dist: String,
    pub hotspots: String,
    pub file_count: i64,
}

/// Compute and cache architecture summary from scan results.
///
/// Language distribution: group entries by extension, top 4 by file count as percentages.
/// Hotspots: directories with highest inbound import fan-in (top 3).
pub fn rebuild_arch_summary(
    waypoint_dir: &Path,
    entries: &[super::MapEntry],
    imports: &[super::extract::Import],
) -> Result<ArchSummary, AppError> {
    let conn = open_index(waypoint_dir)?;
    let summary = rebuild_arch_summary_with(&conn, entries, imports)?;
    mirror_arch_section_in_map(waypoint_dir, &summary)?;
    Ok(summary)
}

fn rebuild_arch_summary_with(
    conn: &Connection,
    entries: &[super::MapEntry],
    imports: &[super::extract::Import],
) -> Result<ArchSummary, AppError> {
    // Language distribution by file count
    let mut ext_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for entry in entries {
        let ext = std::path::Path::new(&entry.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("other");
        *ext_counts.entry(ext).or_default() += 1;
    }
    let total = entries.len().max(1);
    let mut ext_vec: Vec<_> = ext_counts.into_iter().collect();
    ext_vec.sort_by_key(|b| std::cmp::Reverse(b.1));

    let lang_parts: Vec<String> = ext_vec
        .iter()
        .take(ARCH_LANG_DISPLAY_LIMIT)
        .map(|(ext, count)| {
            let name = ext_to_lang(ext);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let pct = (*count as f64 / total as f64 * 100.0).round() as u32;
            format!("{name} {pct}%")
        })
        .collect();
    let lang_dist = format!("[waypoint] arch: {}", lang_parts.join(", "));

    // Hotspots: directory-level inbound import fan-in
    let mut dir_fan_in: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for imp in imports {
        let dir = std::path::Path::new(&imp.target_path)
            .parent()
            .map_or_else(String::new, |p| {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() { ".".to_string() } else { s }
            });
        *dir_fan_in.entry(dir).or_default() += 1;
    }
    let mut dir_vec: Vec<_> = dir_fan_in.into_iter().collect();
    dir_vec.sort_by_key(|b| std::cmp::Reverse(b.1));

    let hotspot_parts: Vec<String> = dir_vec
        .iter()
        .take(ARCH_HOTSPOT_DISPLAY_LIMIT)
        .map(|(dir, count)| format!("{dir}/ ({count} imports-in)"))
        .collect();
    let hotspots = if hotspot_parts.is_empty() {
        String::new()
    } else {
        format!("[waypoint] arch: hotspots: {}", hotspot_parts.join(", "))
    };

    #[allow(clippy::cast_possible_wrap)]
    let file_count = entries.len() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO arch_summary (id, lang_dist, hotspots, file_count) VALUES (1, ?1, ?2, ?3)",
        params![lang_dist, hotspots, file_count],
    )?;

    Ok(ArchSummary {
        lang_dist,
        hotspots,
        file_count,
    })
}

fn mirror_arch_section_in_map(waypoint_dir: &Path, summary: &ArchSummary) -> Result<(), AppError> {
    let map_path = waypoint_dir.join("map.md");
    if !map_path.exists() {
        return Ok(());
    }
    let current = std::fs::read_to_string(&map_path)?;
    let updated = upsert_architecture_section(&current, summary);
    if updated != current {
        crate::project::atomic_write(&map_path, &updated)?;
    }
    Ok(())
}

fn upsert_architecture_section(map_content: &str, summary: &ArchSummary) -> String {
    let stripped = strip_existing_architecture_section(map_content);
    let mut lines: Vec<String> = stripped.lines().map(ToString::to_string).collect();

    let insert_at = lines
        .iter()
        .position(|line| line.starts_with("## "))
        .unwrap_or(lines.len());
    let mut section = vec![
        "### Architecture".to_string(),
        format!("- Languages: {}", map_languages_line(summary)),
    ];
    if let Some(hotspots) = map_hotspots_line(summary) {
        section.push(format!("- Hotspots: {hotspots}"));
    }
    section.push(String::new());

    if insert_at > 0 && !lines[insert_at - 1].is_empty() {
        lines.insert(insert_at, String::new());
    }
    lines.splice(insert_at..insert_at, section);

    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn strip_existing_architecture_section(map_content: &str) -> String {
    let mut cleaned = Vec::new();
    let mut skipping_arch_section = false;

    for line in map_content.lines() {
        if line == "### Architecture" {
            skipping_arch_section = true;
            continue;
        }
        if skipping_arch_section {
            if line.starts_with("## ") {
                skipping_arch_section = false;
            } else {
                continue;
            }
        }
        cleaned.push(line.to_string());
    }

    cleaned.join("\n")
}

fn map_languages_line(summary: &ArchSummary) -> &str {
    summary
        .lang_dist
        .strip_prefix("[waypoint] arch: ")
        .unwrap_or(summary.lang_dist.as_str())
}

fn map_hotspots_line(summary: &ArchSummary) -> Option<&str> {
    if summary.hotspots.trim().is_empty() {
        return None;
    }
    Some(
        summary
            .hotspots
            .strip_prefix("[waypoint] arch: hotspots: ")
            .unwrap_or(summary.hotspots.as_str()),
    )
}

/// Read cached architecture summary.
pub fn get_arch_summary(waypoint_dir: &Path) -> Result<Option<ArchSummary>, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_index(waypoint_dir)?;
    let result = conn
        .query_row(
            "SELECT lang_dist, hotspots, file_count FROM arch_summary WHERE id = 1",
            [],
            |row| {
                Ok(ArchSummary {
                    lang_dist: row.get(0)?,
                    hotspots: row.get(1)?,
                    file_count: row.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(result)
}

/// Map file extension to human-readable language name.
fn ext_to_lang(ext: &str) -> &str {
    match ext {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "rb" => "Ruby",
        "java" => "Java",
        "kt" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cpp" | "hpp" => "C++",
        "sh" | "bash" | "zsh" | "fish" => "Shell",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "md" | "mdx" => "Markdown",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "sql" => "SQL",
        "tf" | "tfvars" | "hcl" => "Terraform",
        "proto" => "Protobuf",
        "vue" => "Vue",
        "svelte" => "Svelte",
        other => other,
    }
}

/// Find symbols in a file whose line ranges overlap any of the given changed ranges.
/// Used by `waypoint impact` to map diff hunks to affected symbols.
pub fn find_symbols_in_ranges(
    waypoint_dir: &Path,
    file_path: &str,
    ranges: &[(i64, i64)],
) -> Result<Vec<SymbolRow>, AppError> {
    let conn = open_index(waypoint_dir)?;
    find_symbols_in_ranges_with(&conn, file_path, ranges)
}

/// Connection-reuse variant — avoids opening a new DB connection per call.
pub(crate) fn find_symbols_in_ranges_with(
    conn: &Connection,
    file_path: &str,
    ranges: &[(i64, i64)],
) -> Result<Vec<SymbolRow>, AppError> {
    if ranges.is_empty() {
        return Ok(vec![]);
    }
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stmt = conn.prepare(
        "SELECT file_path, name, kind, signature, line_start, line_end, exported \
         FROM symbols WHERE file_path = ?1 AND line_start <= ?3 AND line_end >= ?2",
    )?;
    for &(range_start, range_end) in ranges {
        let rows = stmt.query_map(params![file_path, range_start, range_end], |row| {
            Ok(SymbolRow {
                file_path: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                signature: row.get(3)?,
                line_start: row.get(4)?,
                line_end: row.get(5)?,
                exported: row.get::<_, i64>(6)? != 0,
            })
        })?;
        for row in rows {
            let row = row?;
            let key = (row.name.clone(), row.file_path.clone(), row.line_start);
            if seen.insert(key) {
                results.push(row);
            }
        }
    }
    Ok(results)
}

/// Count distinct files that import a given symbol from a specific file.
pub fn count_importers(
    waypoint_dir: &Path,
    symbol_name: &str,
    target_file: &str,
) -> Result<i64, AppError> {
    let conn = open_index(waypoint_dir)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT source_file) FROM imports \
         WHERE imported_name = ?1 AND target_path = ?2",
        params![symbol_name, target_file],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Check if the index database file exists and return its modification time.
#[must_use]
pub fn index_mtime(waypoint_dir: &Path) -> Option<std::time::SystemTime> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    std::fs::metadata(&db_path).ok()?.modified().ok()
}

/// Enrich map entries parsed from map.md with stored `content_hash` and
/// `mtime_ms` from the `SQLite` index. Entries not found in the index are
/// left unchanged. No-op if the index file does not exist.
pub fn enrich_metadata(waypoint_dir: &Path, entries: &mut [MapEntry]) -> Result<(), AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_index(waypoint_dir)?;
    let mut stmt =
        conn.prepare("SELECT content_hash, mtime_secs FROM map_entries WHERE path = ?1")?;
    for entry in entries.iter_mut() {
        if let Some((hash, mtime)) = stmt
            .query_row(params![entry.path], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .optional()?
        {
            entry.content_hash = hash;
            entry.mtime_ms = mtime;
        }
    }
    Ok(())
}

/// Load all stored mtimes for mtime-based staleness detection.
/// Returns a map of `relative_path → mtime_ms` (milliseconds despite the
/// legacy `mtime_secs` column name in `SQLite`).
///
/// Pre-v0.10 indexes store seconds here. The unit mismatch causes a one-time
/// rescan on upgrade, which is intentional — see `scan::file_mtime` for details.
pub fn get_stored_mtimes(
    waypoint_dir: &Path,
) -> Result<std::collections::HashMap<String, i64>, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    if !db_path.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let conn = open_index(waypoint_dir)?;
    let mut stmt =
        conn.prepare("SELECT path, mtime_secs FROM map_entries WHERE mtime_secs IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (path, mtime) = row?;
        map.insert(path, mtime);
    }
    Ok(map)
}

/// Find all files that import a given symbol name, optionally filtered by target file.
/// Joins against the symbols table to validate the symbol exists at the target (FR-5).
/// Returns deduplicated `(source_file, line_number)` pairs.
pub fn find_importers(
    waypoint_dir: &Path,
    symbol_name: &str,
    target_file: Option<&str>,
) -> Result<Vec<(String, i64)>, AppError> {
    let conn = open_index(waypoint_dir)?;
    find_importers_with(&conn, symbol_name, target_file)
}

/// Connection-reuse variant — avoids opening a new DB connection per call.
pub(crate) fn find_importers_with(
    conn: &Connection,
    symbol_name: &str,
    target_file: Option<&str>,
) -> Result<Vec<(String, i64)>, AppError> {
    let results = if let Some(target) = target_file {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT i.source_file, i.line_number FROM imports i \
             INNER JOIN symbols s ON s.name = i.imported_name AND s.file_path = i.target_path \
             WHERE i.imported_name = ?1 AND i.target_path = ?2 \
             ORDER BY i.source_file",
        )?;
        let rows = stmt.query_map(params![symbol_name, target], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT i.source_file, i.line_number FROM imports i \
             INNER JOIN symbols s ON s.name = i.imported_name AND s.file_path = i.target_path \
             WHERE i.imported_name = ?1 \
             ORDER BY i.source_file",
        )?;
        let rows = stmt.query_map(params![symbol_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    Ok(results)
}

/// Look up symbols by exact name. Used by `waypoint sketch`.
pub fn sketch(waypoint_dir: &Path, name: &str) -> Result<Vec<SymbolRow>, AppError> {
    let conn = open_index(waypoint_dir)?;
    // Match exact name OR qualified name (e.g., "Type::method")
    // Escape LIKE wildcards in user input to prevent unintended pattern matching
    let escaped = name.replace('%', r"\%").replace('_', r"\_");
    let pattern = format!("%::{escaped}");
    let mut stmt = conn.prepare(
        "SELECT file_path, name, kind, signature, line_start, line_end, exported \
         FROM symbols WHERE name = ?1 OR name LIKE ?2 ESCAPE '\\' \
         ORDER BY exported DESC, file_path, line_start",
    )?;

    let rows = stmt.query_map(params![name, pattern], |row| {
        Ok(SymbolRow {
            file_path: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            signature: row.get(3)?,
            line_start: row.get(4)?,
            line_end: row.get(5)?,
            exported: row.get::<_, i64>(6)? != 0,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// Structural weight for symbol kind (tertiary ranking signal).
fn kind_weight(kind: &str) -> u8 {
    match kind {
        "fn" | "method" | "struct" | "enum" | "trait" | "class" | "type" | "interface" => 2,
        "const" | "static" | "var" | "let" => 1,
        _ => 0,
    }
}

/// Re-rank candidates by structural importance: fan-in (primary), exported (secondary), kind (tertiary).
/// Original order serves as tiebreaker so BM25/LIKE relevance still matters when structural signals are equal.
fn rank_candidates(candidates: &[SymbolRow], conn: &Connection, limit: usize) -> Vec<SymbolRow> {
    // Batch-enrich with fan-in counts
    let mut fan_ins: Vec<i64> = Vec::with_capacity(candidates.len());
    let stmt_result = conn.prepare(
        "SELECT COUNT(DISTINCT source_file) FROM imports \
         WHERE imported_name = ?1 AND target_path = ?2",
    );
    match stmt_result {
        Err(e) => {
            // Imports index unavailable — degrade to unranked truncation
            eprintln!("waypoint: fan-in query unavailable, results unranked: {e}");
            return candidates
                .iter()
                .take(limit)
                .map(|row| SymbolRow {
                    file_path: row.file_path.clone(),
                    name: row.name.clone(),
                    kind: row.kind.clone(),
                    signature: row.signature.clone(),
                    line_start: row.line_start,
                    line_end: row.line_end,
                    exported: row.exported,
                })
                .collect();
        }
        Ok(mut stmt) => {
            for row in candidates {
                let count: i64 = stmt
                    .query_row(params![row.name, row.file_path], |r| r.get(0))
                    .unwrap_or(0);
                fan_ins.push(count);
            }
        }
    }

    // Build sort indices to preserve original position as tiebreaker
    let mut indices: Vec<usize> = (0..candidates.len()).collect();
    indices.sort_by(|&a, &b| {
        fan_ins[b]
            .cmp(&fan_ins[a])
            .then_with(|| u8::from(candidates[b].exported).cmp(&u8::from(candidates[a].exported)))
            .then_with(|| kind_weight(&candidates[b].kind).cmp(&kind_weight(&candidates[a].kind)))
            .then_with(|| a.cmp(&b))
    });

    indices
        .into_iter()
        .take(limit)
        .map(|i| {
            let row = &candidates[i];
            SymbolRow {
                file_path: row.file_path.clone(),
                name: row.name.clone(),
                kind: row.kind.clone(),
                signature: row.signature.clone(),
                line_start: row.line_start,
                line_end: row.line_end,
                exported: row.exported,
            }
        })
        .collect()
}

/// Full-text search over symbols with structural ranking. Used by `waypoint find`.
///
/// Two-phase approach: FTS5/LIKE returns a widened candidate set, then Rust re-ranks
/// by import fan-in (primary), export status (secondary), and symbol kind (tertiary).
pub fn find_symbols(
    waypoint_dir: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SymbolRow>, AppError> {
    let conn = open_index(waypoint_dir)?;

    // Widen candidate pool for re-ranking
    #[allow(clippy::cast_possible_wrap)]
    let pool_size = (limit.saturating_mul(3)).clamp(20, 60) as i64;

    let fts_result: Result<Vec<SymbolRow>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT f.name, f.kind, f.signature, f.file_path \
             FROM symbols_fts f WHERE symbols_fts MATCH ?1 \
             ORDER BY f.rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, pool_size], |row| {
            Ok(SymbolRow {
                name: row.get(0)?,
                kind: row.get(1)?,
                signature: row.get(2)?,
                file_path: row.get(3)?,
                line_start: 0,
                line_end: 0,
                exported: false,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
    })();

    // If FTS works and returns results, enrich with line numbers then re-rank.
    // If FTS succeeds but returns nothing, fall through to LIKE for partial-name matching.
    if let Ok(fts_rows) = fts_result
        && !fts_rows.is_empty()
    {
        let mut enriched = Vec::with_capacity(fts_rows.len());
        let mut detail_stmt = conn.prepare(
            "SELECT line_start, line_end, exported FROM symbols \
             WHERE name = ?1 AND file_path = ?2 LIMIT 1",
        )?;
        for row in fts_rows {
            let detail: Option<(i64, i64, bool)> = detail_stmt
                .query_row(params![row.name, row.file_path], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))
                })
                .optional()?;
            let (ls, le, exp) = detail.unwrap_or((0, 0, false));
            enriched.push(SymbolRow {
                line_start: ls,
                line_end: le,
                exported: exp,
                ..row
            });
        }
        return Ok(rank_candidates(&enriched, &conn, limit));
    }

    // Fallback: LIKE search with widened pool, then re-rank
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT file_path, name, kind, signature, line_start, line_end, exported \
         FROM symbols WHERE name LIKE ?1 OR signature LIKE ?1 \
         ORDER BY exported DESC, name LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, pool_size], |row| {
        Ok(SymbolRow {
            file_path: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            signature: row.get(3)?,
            line_start: row.get(4)?,
            line_end: row.get(5)?,
            exported: row.get::<_, i64>(6)? != 0,
        })
    })?;
    let candidates: Vec<SymbolRow> = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(rank_candidates(&candidates, &conn, limit))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_entry(path: &str) -> MapEntry {
        MapEntry {
            path: path.into(),
            description: format!("desc for {path}"),
            token_estimate: 100,
            ..Default::default()
        }
    }

    #[test]
    fn lookup_without_index_returns_err() {
        let tmp = TempDir::new().unwrap();
        assert!(lookup(tmp.path(), "nonexistent.rs").is_err());
    }

    #[test]
    fn lookup_missing_entry_returns_none() {
        let tmp = TempDir::new().unwrap();
        // Build the index so the DB file exists
        rebuild(tmp.path(), &[]).unwrap();
        let result = lookup(tmp.path(), "nonexistent.rs").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn upsert_then_lookup() {
        let tmp = TempDir::new().unwrap();
        let entry = sample_entry("src/main.rs");

        upsert(tmp.path(), &entry).unwrap();
        let found = lookup(tmp.path(), "src/main.rs").unwrap().unwrap();

        assert_eq!(found.path, "src/main.rs");
        assert_eq!(found.description, "desc for src/main.rs");
        assert_eq!(found.token_estimate, 100);
    }

    #[test]
    fn upsert_overwrites_existing() {
        let tmp = TempDir::new().unwrap();

        upsert(tmp.path(), &sample_entry("a.rs")).unwrap();

        let updated = MapEntry {
            path: "a.rs".into(),
            description: "updated".into(),
            token_estimate: 999,
            ..Default::default()
        };
        upsert(tmp.path(), &updated).unwrap();

        let found = lookup(tmp.path(), "a.rs").unwrap().unwrap();
        assert_eq!(found.description, "updated");
        assert_eq!(found.token_estimate, 999);
    }

    #[test]
    fn remove_deletes_entry() {
        let tmp = TempDir::new().unwrap();

        upsert(tmp.path(), &sample_entry("a.rs")).unwrap();
        remove(tmp.path(), "a.rs").unwrap();

        assert!(lookup(tmp.path(), "a.rs").unwrap().is_none());
    }

    #[test]
    fn remove_nonexistent_is_ok() {
        let tmp = TempDir::new().unwrap();
        remove(tmp.path(), "nonexistent.rs").unwrap();
    }

    #[test]
    fn rebuild_replaces_all_entries() {
        let tmp = TempDir::new().unwrap();

        // Seed with one entry
        upsert(tmp.path(), &sample_entry("old.rs")).unwrap();

        // Rebuild with different entries
        let entries = vec![sample_entry("a.rs"), sample_entry("b.rs")];
        rebuild(tmp.path(), &entries).unwrap();

        assert!(lookup(tmp.path(), "old.rs").unwrap().is_none());
        assert!(lookup(tmp.path(), "a.rs").unwrap().is_some());
        assert!(lookup(tmp.path(), "b.rs").unwrap().is_some());
    }

    #[test]
    fn rebuild_empty_clears_index() {
        let tmp = TempDir::new().unwrap();

        upsert(tmp.path(), &sample_entry("a.rs")).unwrap();
        rebuild(tmp.path(), &[]).unwrap();

        assert!(lookup(tmp.path(), "a.rs").unwrap().is_none());
    }

    fn sample_symbol(file: &str, name: &str, kind: &str) -> Symbol {
        Symbol {
            file_path: file.into(),
            name: name.into(),
            kind: kind.into(),
            signature: format!("pub {kind} {name}"),
            line_start: 1,
            line_end: 5,
            exported: true,
        }
    }

    #[test]
    fn rebuild_symbols_then_sketch() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![
            sample_symbol("src/lib.rs", "AppError", "enum"),
            sample_symbol("src/lib.rs", "run", "fn"),
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        let results = sketch(tmp.path(), "AppError").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "AppError");
        assert_eq!(results[0].kind, "enum");
    }

    #[test]
    fn sketch_finds_qualified_methods() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![
            sample_symbol("src/foo.rs", "Foo", "struct"),
            Symbol {
                name: "Foo::new".into(),
                kind: "method".into(),
                signature: "pub fn new() -> Self".into(),
                ..sample_symbol("src/foo.rs", "", "")
            },
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        // Searching "new" should find Foo::new
        let results = sketch(tmp.path(), "new").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Foo::new");
    }

    #[test]
    fn find_symbols_fallback_like() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![
            sample_symbol("src/map.rs", "extract_description", "fn"),
            sample_symbol("src/map.rs", "write_map", "fn"),
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        let results = find_symbols(tmp.path(), "extract", 10).unwrap();
        assert!(results.iter().any(|r| r.name == "extract_description"));
    }

    #[test]
    fn find_symbols_ranks_by_fan_in() {
        let tmp = TempDir::new().unwrap();
        // Two exported fns matching "process": one with 3 importers, one with 0
        let syms = vec![
            Symbol {
                file_path: "src/a.rs".into(),
                name: "process_data".into(),
                kind: "fn".into(),
                signature: "pub fn process_data()".into(),
                line_start: 1,
                line_end: 10,
                exported: true,
            },
            Symbol {
                file_path: "src/b.rs".into(),
                name: "process_request".into(),
                kind: "fn".into(),
                signature: "pub fn process_request()".into(),
                line_start: 1,
                line_end: 10,
                exported: true,
            },
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        let imports = vec![
            sample_import("src/c.rs", "process_data", "src/a.rs"),
            sample_import("src/d.rs", "process_data", "src/a.rs"),
            sample_import("src/e.rs", "process_data", "src/a.rs"),
        ];
        rebuild_imports(tmp.path(), &imports).unwrap();

        let results = find_symbols(tmp.path(), "process", 10).unwrap();
        assert_eq!(results.len(), 2);
        // process_data has 3 importers, should rank first
        assert_eq!(results[0].name, "process_data");
        assert_eq!(results[1].name, "process_request");
    }

    #[test]
    fn find_symbols_exported_beats_non_exported() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![
            Symbol {
                file_path: "src/a.rs".into(),
                name: "handle_event".into(),
                kind: "fn".into(),
                signature: "fn handle_event()".into(),
                line_start: 1,
                line_end: 5,
                exported: false,
            },
            Symbol {
                file_path: "src/b.rs".into(),
                name: "handle_error".into(),
                kind: "fn".into(),
                signature: "pub fn handle_error()".into(),
                line_start: 1,
                line_end: 5,
                exported: true,
            },
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();
        rebuild_imports(tmp.path(), &[]).unwrap();

        let results = find_symbols(tmp.path(), "handle", 10).unwrap();
        assert_eq!(results.len(), 2);
        // exported symbol ranks first when fan-in is equal (both 0)
        assert!(results[0].exported);
        assert!(!results[1].exported);
    }

    #[test]
    fn find_symbols_kind_weight_tiebreak() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![
            Symbol {
                file_path: "src/a.rs".into(),
                name: "Config".into(),
                kind: "const".into(),
                signature: "pub const Config: &str".into(),
                line_start: 1,
                line_end: 1,
                exported: true,
            },
            Symbol {
                file_path: "src/b.rs".into(),
                name: "ConfigBuilder".into(),
                kind: "struct".into(),
                signature: "pub struct ConfigBuilder".into(),
                line_start: 1,
                line_end: 10,
                exported: true,
            },
        ];
        rebuild_symbols(tmp.path(), &syms).unwrap();
        rebuild_imports(tmp.path(), &[]).unwrap();

        // Use substring that won't match FTS5 exactly, forcing LIKE fallback
        let results = find_symbols(tmp.path(), "onfig", 10).unwrap();
        assert_eq!(results.len(), 2);
        // struct (weight 2) beats const (weight 1) when fan-in and exported are equal
        assert_eq!(results[0].kind, "struct");
        assert_eq!(results[1].kind, "const");
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
    fn rebuild_arch_summary_mirrors_architecture_section_into_map() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![sample_entry("src/main.rs"), sample_entry("README.md")];
        crate::map::write_map(tmp.path(), &entries).unwrap();
        let imports = vec![sample_import("src/main.rs", "helper", "src/lib.rs")];

        rebuild_arch_summary(tmp.path(), &entries, &imports).unwrap();

        let map = std::fs::read_to_string(tmp.path().join("map.md")).unwrap();
        assert!(map.contains("### Architecture"));
        assert!(map.contains("- Languages:"));
        assert!(map.contains("- Hotspots:"));
    }

    #[test]
    fn rebuild_arch_summary_replaces_existing_architecture_section() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![sample_entry("src/main.rs"), sample_entry("README.md")];
        crate::map::write_map(tmp.path(), &entries).unwrap();

        let first_imports = vec![sample_import("src/main.rs", "helper", "src/lib.rs")];
        rebuild_arch_summary(tmp.path(), &entries, &first_imports).unwrap();

        let second_imports = vec![
            sample_import("src/main.rs", "helper", "src/lib.rs"),
            sample_import("src/main.rs", "thing", "src/core.rs"),
        ];
        rebuild_arch_summary(tmp.path(), &entries, &second_imports).unwrap();

        let map = std::fs::read_to_string(tmp.path().join("map.md")).unwrap();
        let section_count = map.matches("### Architecture").count();
        assert_eq!(section_count, 1, "architecture section should be replaced");
    }

    /// Seed symbols so `find_importers` join succeeds.
    fn seed_symbols_for_imports(dir: &std::path::Path) {
        let syms = vec![
            sample_symbol("src/utils.js", "foo", "fn"),
            sample_symbol("src/utils.js", "new_fn", "fn"),
            sample_symbol("src/helpers.js", "bar", "fn"),
        ];
        rebuild_symbols(dir, &syms).unwrap();
    }

    #[test]
    fn rebuild_imports_then_find() {
        let tmp = TempDir::new().unwrap();
        seed_symbols_for_imports(tmp.path());
        let imports = vec![
            sample_import("src/a.js", "foo", "src/utils.js"),
            sample_import("src/b.js", "foo", "src/utils.js"),
            sample_import("src/a.js", "bar", "src/helpers.js"),
        ];
        rebuild_imports(tmp.path(), &imports).unwrap();

        let results = find_importers(tmp.path(), "foo", None).unwrap();
        assert_eq!(results.len(), 2);

        let results = find_importers(tmp.path(), "foo", Some("src/utils.js")).unwrap();
        assert_eq!(results.len(), 2);

        let results = find_importers(tmp.path(), "bar", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "src/a.js");
    }
}
