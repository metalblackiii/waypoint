use std::path::PathBuf;

use chrono::Utc;
use rusqlite::{Connection, params};

use crate::AppError;

const RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy)]
pub enum EventKind {
    SessionStart,
    MapHit,
    MapMiss,
    TrapHit,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::MapHit => "map_hit",
            Self::MapMiss => "map_miss",
            Self::TrapHit => "trap_hit",
        }
    }
}

/// Token savings statistics.
#[derive(Debug)]
pub struct GainStats {
    pub total_events: usize,
    pub map_hits: usize,
    pub map_misses: usize,
    pub trap_hits: usize,
    pub map_hit_rate: f64,
    pub estimated_tokens_saved: i64,
    pub daily: Vec<DayStats>,
}

#[derive(Debug)]
pub struct DayStats {
    pub date: String,
    pub events: usize,
    pub tokens_saved: i64,
}

/// Get the platform-native database path.
fn db_path() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    let dir = data_dir.join("waypoint");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("ledger.db"))
}

/// Open a connection and ensure the schema exists.
fn open_db() -> Result<Connection, AppError> {
    let path =
        db_path().ok_or_else(|| AppError::Ledger("cannot determine data directory".into()))?;
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            event_kind TEXT NOT NULL,
            project_path TEXT NOT NULL DEFAULT '',
            token_impact INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_project ON events(project_path, timestamp);",
    )?;
    Ok(conn)
}

/// Record a hook event. Silent failure — never crashes the hook.
pub fn record_event(
    kind: EventKind,
    project_path: &str,
    token_impact: i64,
) -> Result<(), AppError> {
    let conn = open_db()?;
    conn.execute(
        "INSERT INTO events (timestamp, event_kind, project_path, token_impact) VALUES (?1, ?2, ?3, ?4)",
        params![Utc::now().to_rfc3339(), kind.as_str(), project_path, token_impact],
    )?;

    // Auto-purge old records (FR-19)
    let cutoff = Utc::now() - chrono::TimeDelta::days(RETENTION_DAYS);
    let _ = conn.execute(
        "DELETE FROM events WHERE timestamp < ?1",
        params![cutoff.to_rfc3339()],
    );

    Ok(())
}

/// Get gain statistics, optionally filtered by project.
pub fn gain_stats(project_path: Option<&str>) -> Result<GainStats, AppError> {
    let conn = open_db()?;

    let (filter, param): (&str, Option<String>) = match project_path {
        Some(p) => ("WHERE project_path = ?1", Some(p.to_string())),
        None => ("", None),
    };

    let total_events = query_count(
        &conn,
        &format!("SELECT COUNT(*) FROM events {filter}"),
        &param,
    )?;

    let map_hits = query_count_kind(&conn, "map_hit", &param)?;
    let map_misses = query_count_kind(&conn, "map_miss", &param)?;
    let trap_hits = query_count_kind(&conn, "trap_hit", &param)?;

    let map_hit_rate = if map_hits + map_misses > 0 {
        map_hits as f64 / (map_hits + map_misses) as f64 * 100.0
    } else {
        0.0
    };

    let estimated_tokens_saved = {
        let sql = format!("SELECT COALESCE(SUM(token_impact), 0) FROM events {filter}");
        let mut stmt = conn.prepare(&sql)?;
        match &param {
            Some(p) => stmt.query_row(params![p], |row| row.get(0))?,
            None => stmt.query_row([], |row| row.get(0))?,
        }
    };

    let daily = query_daily(&conn, &param)?;

    Ok(GainStats {
        total_events,
        map_hits,
        map_misses,
        trap_hits,
        map_hit_rate,
        estimated_tokens_saved,
        daily,
    })
}

fn query_count(conn: &Connection, sql: &str, param: &Option<String>) -> Result<usize, AppError> {
    let mut stmt = conn.prepare(sql)?;
    let count = match param {
        Some(p) => stmt.query_row(params![p], |row| row.get(0))?,
        None => stmt.query_row([], |row| row.get(0))?,
    };
    Ok(count)
}

fn query_count_kind(
    conn: &Connection,
    kind: &str,
    param: &Option<String>,
) -> Result<usize, AppError> {
    let (sql, values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match param {
        Some(p) => (
            "SELECT COUNT(*) FROM events WHERE event_kind = ?1 AND project_path = ?2".into(),
            vec![Box::new(kind.to_string()), Box::new(p.clone())],
        ),
        None => (
            "SELECT COUNT(*) FROM events WHERE event_kind = ?1".into(),
            vec![Box::new(kind.to_string())],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let count = stmt.query_row(params.as_slice(), |row| row.get(0))?;
    Ok(count)
}

fn query_daily(conn: &Connection, param: &Option<String>) -> Result<Vec<DayStats>, AppError> {
    let (sql, values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match param {
        Some(p) => (
            "SELECT DATE(timestamp) as d, COUNT(*), COALESCE(SUM(token_impact), 0) \
             FROM events WHERE project_path = ?1 \
             GROUP BY d ORDER BY d DESC LIMIT 30"
                .into(),
            vec![Box::new(p.clone())],
        ),
        None => (
            "SELECT DATE(timestamp) as d, COUNT(*), COALESCE(SUM(token_impact), 0) \
             FROM events GROUP BY d ORDER BY d DESC LIMIT 30"
                .into(),
            vec![],
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok(DayStats {
            date: row.get(0)?,
            events: row.get(1)?,
            tokens_saved: row.get(2)?,
        })
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}
