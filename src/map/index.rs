use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::MapEntry;
use super::extract::Symbol;
use crate::AppError;

const INDEX_FILENAME: &str = "map_index.db";

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS map_entries (
    path TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    token_estimate INTEGER NOT NULL
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
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);";

const FTS_SCHEMA: &str = "\
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(\
    name, kind, signature, file_path\
);";

fn open_index(waypoint_dir: &Path) -> Result<Connection, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(SCHEMA)?;
    // FTS5 is best-effort — skip silently if unavailable
    let _ = conn.execute_batch(FTS_SCHEMA);
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
    let mut stmt =
        conn.prepare("SELECT path, description, token_estimate FROM map_entries WHERE path = ?1")?;

    let entry = stmt
        .query_row(params![relative_path], |row| {
            Ok(MapEntry {
                path: row.get(0)?,
                description: row.get(1)?,
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                // token_estimate is always non-negative and fits in usize
                token_estimate: row.get::<_, i64>(2)? as usize,
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
        "INSERT OR REPLACE INTO map_entries (path, description, token_estimate) VALUES (?1, ?2, ?3)",
        params![entry.path, entry.description, entry.token_estimate as i64],
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
        "INSERT INTO map_entries (path, description, token_estimate) VALUES (?1, ?2, ?3)",
    )?;

    for entry in entries {
        #[allow(clippy::cast_possible_wrap)]
        stmt.execute(params![
            entry.path,
            entry.description,
            entry.token_estimate as i64
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

/// Update symbols for a single file (incremental, used by `post_write` hook).
///
/// Commits the delete of old symbols first so a mid-insert failure leaves the
/// file's symbols empty rather than stale.
pub fn update_file_symbols(
    waypoint_dir: &Path,
    file_path: &str,
    symbols: &[Symbol],
) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;

    // Clear stale data in a committed step before inserting new symbols
    conn.execute(
        "DELETE FROM symbols WHERE file_path = ?1",
        params![file_path],
    )?;
    let _ = conn.execute(
        "DELETE FROM symbols_fts WHERE file_path = ?1",
        params![file_path],
    );

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
            file_path,
            sym.name,
            sym.kind,
            sym.signature,
            sym.line_start,
            sym.line_end,
            i64::from(sym.exported)
        ])?;
        if let Ok(ref mut fts) = fts_stmt {
            let _ = fts.execute(params![sym.name, sym.kind, sym.signature, file_path]);
        }
    }

    drop(stmt);
    drop(fts_stmt);
    tx.commit()?;
    Ok(())
}

/// Remove all symbols for a deleted file.
pub fn remove_file_symbols(waypoint_dir: &Path, file_path: &str) -> Result<(), AppError> {
    let conn = open_index(waypoint_dir)?;
    conn.execute(
        "DELETE FROM symbols WHERE file_path = ?1",
        params![file_path],
    )?;
    // Best-effort FTS cleanup
    let _ = conn.execute(
        "DELETE FROM symbols_fts WHERE file_path = ?1",
        params![file_path],
    );
    Ok(())
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

/// Full-text search over symbols. Used by `waypoint find`.
pub fn find_symbols(
    waypoint_dir: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SymbolRow>, AppError> {
    let conn = open_index(waypoint_dir)?;

    // Try FTS5 first
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    let fts_result: Result<Vec<SymbolRow>, _> = (|| {
        let mut stmt = conn.prepare(
            "SELECT f.name, f.kind, f.signature, f.file_path \
             FROM symbols_fts f WHERE symbols_fts MATCH ?1 \
             ORDER BY f.rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit_i64], |row| {
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

    // If FTS works and returns results, enrich with line numbers from the symbols table.
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
        return Ok(enriched);
    }

    // Fallback: LIKE search on the symbols table
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT file_path, name, kind, signature, line_start, line_end, exported \
         FROM symbols WHERE name LIKE ?1 OR signature LIKE ?1 \
         ORDER BY exported DESC, name LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit_i64], |row| {
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
    fn update_file_symbols_replaces() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![sample_symbol("src/a.rs", "old_fn", "fn")];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        let new_syms = vec![sample_symbol("src/a.rs", "new_fn", "fn")];
        update_file_symbols(tmp.path(), "src/a.rs", &new_syms).unwrap();

        assert!(sketch(tmp.path(), "old_fn").unwrap().is_empty());
        assert_eq!(sketch(tmp.path(), "new_fn").unwrap().len(), 1);
    }

    #[test]
    fn remove_file_symbols_clears() {
        let tmp = TempDir::new().unwrap();
        let syms = vec![sample_symbol("src/a.rs", "foo", "fn")];
        rebuild_symbols(tmp.path(), &syms).unwrap();

        remove_file_symbols(tmp.path(), "src/a.rs").unwrap();
        assert!(sketch(tmp.path(), "foo").unwrap().is_empty());
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
}
