use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::MapEntry;
use crate::AppError;

const INDEX_FILENAME: &str = "map_index.db";

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS map_entries (
    path TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    token_estimate INTEGER NOT NULL
);";

fn open_index(waypoint_dir: &Path) -> Result<Connection, AppError> {
    let db_path = waypoint_dir.join(INDEX_FILENAME);
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
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
}
