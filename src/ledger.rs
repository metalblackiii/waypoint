use std::fmt;
use std::path::PathBuf;

use chrono::Utc;
use colored::{Color, Colorize};
use rusqlite::{Connection, params};

use crate::AppError;

const RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy)]
pub enum EventKind {
    SessionStart,
    MapHit,
    MapMiss,
    TrapHit,
    FirstEdit,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::MapHit => "map_hit",
            Self::MapMiss => "map_miss",
            Self::TrapHit => "trap_hit",
            Self::FirstEdit => "first_edit",
        }
    }
}

/// Token savings statistics.
#[derive(Debug)]
pub struct GainStats {
    pub total_events: i64,
    pub map_hits: i64,
    pub map_misses: i64,
    pub trap_hits: i64,
    pub first_edit_count: i64,
    pub avg_first_edit_secs: f64,
    pub map_hit_rate: f64,
    pub estimated_tokens_saved: i64,
    pub daily: Vec<DayStats>,
}

#[derive(Debug)]
pub struct DayStats {
    pub date: String,
    pub events: i64,
    pub tokens_saved: i64,
}

const DISPLAY_WIDTH: usize = 68;
const METER_WIDTH: usize = 24;
const IMPACT_BAR_WIDTH: usize = 10;
const LABEL_PAD: usize = 16;
const VALUE_PAD: usize = 7;

/// Format a token count for human display (e.g., 1.5M, 350.2K, 42).
fn format_tokens(n: i64) -> String {
    let abs = n.unsigned_abs();
    #[allow(clippy::cast_precision_loss)]
    let value = n as f64;
    // 999_950+ rounds to "1000.0K" at one decimal, so promote to M
    if abs >= 999_950 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Format a duration in seconds for human display (e.g., "42s", "2m 15s", "5m").
fn format_duration(secs: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let s = secs.round() as i64;
    if s < 60 {
        format!("{s}s")
    } else {
        let m = s / 60;
        let remainder = s % 60;
        if remainder == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {remainder}s")
        }
    }
}

/// Render a filled/empty bar chart at the given width.
fn bar(ratio: f64, width: usize) -> String {
    let clamped = ratio.clamp(0.0, 1.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = (clamped * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Choose a meter color based on hit-rate percentage.
fn rate_color(pct: f64) -> Color {
    if pct >= 75.0 {
        Color::Green
    } else if pct >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

impl fmt::Display for GainStats {
    #[allow(clippy::cast_precision_loss)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sep_double = "═".repeat(DISPLAY_WIDTH);
        let sep_single = "─".repeat(DISPLAY_WIDTH);

        writeln!(f, "{}", sep_double.dimmed())?;
        writeln!(f)?;

        // Summary stats — pad label, right-align value
        let lines: &[(&str, String, Option<Color>)] = &[
            ("Total events:", self.total_events.to_string(), None),
            ("Map hits:", self.map_hits.to_string(), Some(Color::Green)),
            (
                "Map misses:",
                self.map_misses.to_string(),
                Some(Color::Yellow),
            ),
            ("Trap hits:", self.trap_hits.to_string(), Some(Color::Cyan)),
            (
                "Tokens saved:",
                format_tokens(self.estimated_tokens_saved),
                Some(Color::BrightGreen),
            ),
        ];

        for (label, value, color) in lines {
            let padded_label = format!("{label:<LABEL_PAD$}");
            let padded_val = format!("{value:>VALUE_PAD$}");
            let styled_val = match color {
                Some(c) => format!("{}", padded_val.color(*c)),
                None => padded_val,
            };
            writeln!(f, "{} {styled_val}", padded_label.bold())?;
        }

        // Hit rate meter
        let ratio = self.map_hit_rate / 100.0;
        let meter = bar(ratio, METER_WIDTH);
        let color = rate_color(self.map_hit_rate);
        let padded_label = format!("{:<LABEL_PAD$}", "Hit rate:");
        writeln!(
            f,
            "{} {} {}",
            padded_label.bold(),
            meter.color(color),
            format!("{:.1}%", self.map_hit_rate).bold(),
        )?;

        // First-edit timing
        if self.first_edit_count > 0 {
            let padded_label = format!("{:<LABEL_PAD$}", "First edit:");
            let time_str = format_duration(self.avg_first_edit_secs);
            let count_str = format!("({} samples)", self.first_edit_count);
            writeln!(
                f,
                "{} {:>VALUE_PAD$}  {}",
                padded_label.bold(),
                time_str.color(Color::Cyan),
                count_str.dimmed(),
            )?;
        }

        // Daily breakdown table
        if !self.daily.is_empty() {
            writeln!(f)?;
            writeln!(f, "{}", "Daily Breakdown".bold())?;
            writeln!(f, "{}", sep_single.dimmed())?;
            writeln!(
                f,
                "  {}  {:<12} {:>7}  {:>8}  {}",
                "#".dimmed(),
                "Date".dimmed(),
                "Events".dimmed(),
                "Saved".dimmed(),
                "Impact".dimmed(),
            )?;
            writeln!(f, "{}", sep_single.dimmed())?;

            let max_saved = self
                .daily
                .iter()
                .map(|d| d.tokens_saved)
                .max()
                .unwrap_or(1)
                .max(1);

            for (i, day) in self.daily.iter().enumerate() {
                let impact_ratio = day.tokens_saved as f64 / max_saved as f64;
                writeln!(
                    f,
                    " {:>2}.  {:<12} {:>7}  {:>8}  {}",
                    (i + 1).to_string().dimmed(),
                    day.date,
                    day.events,
                    format_tokens(day.tokens_saved),
                    bar(impact_ratio, IMPACT_BAR_WIDTH).green(),
                )?;
            }
            writeln!(f, "{}", sep_single.dimmed())?;
        }

        Ok(())
    }
}

impl GainStats {
    /// One-line summary for status display.
    #[must_use]
    pub fn summary_line(&self) -> String {
        format!(
            "{} events, {:.1}% hit rate, ~{} tokens saved",
            self.total_events,
            self.map_hit_rate,
            format_tokens(self.estimated_tokens_saved)
        )
    }
}

/// Get the platform-native database path.
fn db_path() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    let dir = data_dir.join("waypoint");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("ledger.db"))
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    project_path TEXT NOT NULL DEFAULT '',
    token_impact INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_project ON events(project_path, timestamp);";

fn init_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// Open a connection and ensure the schema exists.
fn open_db() -> Result<Connection, AppError> {
    let path =
        db_path().ok_or_else(|| AppError::Ledger("cannot determine data directory".into()))?;
    let conn = Connection::open(&path)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Record a hook event. Silent failure — never crashes the hook.
pub fn record_event(
    kind: EventKind,
    project_path: &str,
    token_impact: i64,
) -> Result<(), AppError> {
    let conn = open_db()?;
    record_event_with(&conn, kind, project_path, token_impact)
}

/// Purge events older than the retention window. Called once per session, not per event.
pub fn purge_old_events() -> Result<(), AppError> {
    let conn = open_db()?;
    let cutoff = Utc::now() - chrono::TimeDelta::days(RETENTION_DAYS);
    conn.execute(
        "DELETE FROM events WHERE timestamp < ?1",
        params![cutoff.to_rfc3339()],
    )?;
    Ok(())
}

fn record_event_with(
    conn: &Connection,
    kind: EventKind,
    project_path: &str,
    token_impact: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO events (timestamp, event_kind, project_path, token_impact) VALUES (?1, ?2, ?3, ?4)",
        params![Utc::now().to_rfc3339(), kind.as_str(), project_path, token_impact],
    )?;
    Ok(())
}

/// Record a first-edit event if one hasn't been logged for the current session.
///
/// Stores elapsed seconds since session start in `token_impact`.
/// Idempotent within a session — second and subsequent calls are no-ops.
pub fn record_first_edit_if_needed(project_path: &str) -> Result<(), AppError> {
    let conn = open_db()?;
    record_first_edit_if_needed_with(&conn, project_path)
}

fn record_first_edit_if_needed_with(conn: &Connection, project_path: &str) -> Result<(), AppError> {
    // Use the most recent session_start globally — a session has one start
    // time regardless of which (possibly foreign) project files are edited.
    let session_start: Option<String> = conn.query_row(
        "SELECT MAX(timestamp) FROM events WHERE event_kind = 'session_start'",
        [],
        |row| row.get(0),
    )?;

    // Check if already logged since that session_start — globally, not per-project.
    // A session has one first edit regardless of which project the file belongs to.
    let cutoff = session_start.as_deref().unwrap_or("1970-01-01");
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events WHERE event_kind = 'first_edit' \
         AND timestamp > ?1",
        params![cutoff],
        |row| row.get(0),
    )?;

    if count > 0 {
        return Ok(());
    }

    let elapsed_secs = match session_start {
        Some(ref ts) => chrono::DateTime::parse_from_rfc3339(ts)
            .map(|start| (Utc::now() - start.with_timezone(&Utc)).num_seconds())
            .unwrap_or(0),
        None => 0,
    };

    record_event_with(conn, EventKind::FirstEdit, project_path, elapsed_secs)
}

/// Get gain statistics, optionally filtered by project.
pub fn gain_stats(project_path: Option<&str>) -> Result<GainStats, AppError> {
    let conn = open_db()?;
    gain_stats_with(&conn, project_path)
}

fn gain_stats_with(conn: &Connection, project_path: Option<&str>) -> Result<GainStats, AppError> {
    let (filter, param): (&str, Option<String>) = match project_path {
        Some(p) => ("WHERE project_path = ?1", Some(p.to_string())),
        None => ("", None),
    };

    let param_ref = param.as_deref();

    let total_events = query_count(
        conn,
        &format!("SELECT COUNT(*) FROM events {filter}"),
        param_ref,
    )?;

    let map_hits = query_count_kind(conn, "map_hit", param_ref)?;
    let map_misses = query_count_kind(conn, "map_miss", param_ref)?;
    let trap_hits = query_count_kind(conn, "trap_hit", param_ref)?;

    #[allow(clippy::cast_precision_loss)] // ratio of small counters — precision loss irrelevant
    let map_hit_rate = if map_hits + map_misses > 0 {
        map_hits as f64 / (map_hits + map_misses) as f64 * 100.0
    } else {
        0.0
    };

    let estimated_tokens_saved = {
        let sql = if filter.is_empty() {
            "SELECT COALESCE(SUM(token_impact), 0) FROM events WHERE event_kind != 'first_edit'"
                .to_string()
        } else {
            format!("SELECT COALESCE(SUM(token_impact), 0) FROM events {filter} AND event_kind != 'first_edit'")
        };
        let mut stmt = conn.prepare(&sql)?;
        match param_ref {
            Some(p) => stmt.query_row(params![p], |row| row.get(0))?,
            None => stmt.query_row([], |row| row.get(0))?,
        }
    };

    let first_edit_count = query_count_kind(conn, "first_edit", param_ref)?;

    #[allow(clippy::cast_precision_loss)]
    let avg_first_edit_secs: f64 = if first_edit_count > 0 {
        let sql = match param_ref {
            Some(_) => "SELECT AVG(token_impact) FROM events WHERE event_kind = 'first_edit' AND project_path = ?1",
            None => "SELECT AVG(token_impact) FROM events WHERE event_kind = 'first_edit'",
        };
        let mut stmt = conn.prepare(sql)?;
        match param_ref {
            Some(p) => stmt.query_row(params![p], |row| row.get(0))?,
            None => stmt.query_row([], |row| row.get(0))?,
        }
    } else {
        0.0
    };

    let daily = query_daily(conn, param_ref)?;

    Ok(GainStats {
        total_events,
        map_hits,
        map_misses,
        trap_hits,
        first_edit_count,
        avg_first_edit_secs,
        map_hit_rate,
        estimated_tokens_saved,
        daily,
    })
}

fn query_count(conn: &Connection, sql: &str, param: Option<&str>) -> Result<i64, AppError> {
    let mut stmt = conn.prepare(sql)?;
    let count = match param {
        Some(p) => stmt.query_row(params![p], |row| row.get(0))?,
        None => stmt.query_row([], |row| row.get(0))?,
    };
    Ok(count)
}

fn query_count_kind(conn: &Connection, kind: &str, param: Option<&str>) -> Result<i64, AppError> {
    let (sql, values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match param {
        Some(p) => (
            "SELECT COUNT(*) FROM events WHERE event_kind = ?1 AND project_path = ?2".into(),
            vec![Box::new(kind.to_string()), Box::new(p.to_string())],
        ),
        None => (
            "SELECT COUNT(*) FROM events WHERE event_kind = ?1".into(),
            vec![Box::new(kind.to_string())],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let params_vec: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(AsRef::as_ref).collect();
    let count = stmt.query_row(params_vec.as_slice(), |row| row.get(0))?;
    Ok(count)
}

fn query_daily(conn: &Connection, param: Option<&str>) -> Result<Vec<DayStats>, AppError> {
    let (sql, values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match param {
        Some(p) => (
            "SELECT DATE(timestamp) as d, COUNT(*), \
             COALESCE(SUM(CASE WHEN event_kind = 'first_edit' THEN 0 ELSE token_impact END), 0) \
             FROM events WHERE project_path = ?1 \
             GROUP BY d ORDER BY d DESC LIMIT 30"
                .into(),
            vec![Box::new(p.to_string())],
        ),
        None => (
            "SELECT DATE(timestamp) as d, COUNT(*), \
             COALESCE(SUM(CASE WHEN event_kind = 'first_edit' THEN 0 ELSE token_impact END), 0) \
             FROM events GROUP BY d ORDER BY d DESC LIMIT 30"
                .into(),
            vec![],
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let params_vec: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(AsRef::as_ref).collect();
    let rows = stmt.query_map(params_vec.as_slice(), |row| {
        Ok(DayStats {
            date: row.get(0)?,
            events: row.get(1)?,
            tokens_saved: row.get(2)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn record_and_query_events() {
        let conn = test_db();

        record_event_with(&conn, EventKind::MapHit, "/tmp/project", 150).unwrap();
        record_event_with(&conn, EventKind::MapMiss, "/tmp/project", 0).unwrap();
        record_event_with(&conn, EventKind::TrapHit, "/tmp/project", 50).unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/project")).unwrap();

        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.map_hits, 1);
        assert_eq!(stats.map_misses, 1);
        assert_eq!(stats.trap_hits, 1);
        assert!((stats.map_hit_rate - 50.0).abs() < f64::EPSILON);
        assert_eq!(stats.estimated_tokens_saved, 200);
    }

    #[test]
    fn gain_stats_empty_db() {
        let conn = test_db();

        let stats = gain_stats_with(&conn, None).unwrap();

        assert_eq!(stats.total_events, 0);
        assert_eq!(stats.map_hits, 0);
        assert!((stats.map_hit_rate).abs() < f64::EPSILON);
        assert_eq!(stats.estimated_tokens_saved, 0);
        assert!(stats.daily.is_empty());
    }

    #[test]
    fn project_filter_isolates_events() {
        let conn = test_db();

        record_event_with(&conn, EventKind::MapHit, "/project-a", 100).unwrap();
        record_event_with(&conn, EventKind::MapHit, "/project-b", 200).unwrap();

        let stats_a = gain_stats_with(&conn, Some("/project-a")).unwrap();
        let stats_b = gain_stats_with(&conn, Some("/project-b")).unwrap();
        let stats_all = gain_stats_with(&conn, None).unwrap();

        assert_eq!(stats_a.total_events, 1);
        assert_eq!(stats_a.estimated_tokens_saved, 100);
        assert_eq!(stats_b.total_events, 1);
        assert_eq!(stats_b.estimated_tokens_saved, 200);
        assert_eq!(stats_all.total_events, 2);
        assert_eq!(stats_all.estimated_tokens_saved, 300);
    }

    #[test]
    fn daily_breakdown_present() {
        let conn = test_db();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_event_with(&conn, EventKind::MapHit, "/tmp/p", 500).unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/p")).unwrap();

        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.daily[0].events, 2);
        assert_eq!(stats.daily[0].tokens_saved, 500);
    }

    #[test]
    fn daily_breakdown_excludes_first_edit_seconds() {
        let conn = test_db();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_event_with(&conn, EventKind::MapHit, "/tmp/p", 500).unwrap();
        record_first_edit_if_needed_with(&conn, "/tmp/p").unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/p")).unwrap();

        assert_eq!(stats.daily.len(), 1);
        assert_eq!(stats.daily[0].events, 3);
        assert_eq!(stats.daily[0].tokens_saved, 500);
    }

    #[test]
    fn event_kind_as_str() {
        assert_eq!(EventKind::SessionStart.as_str(), "session_start");
        assert_eq!(EventKind::MapHit.as_str(), "map_hit");
        assert_eq!(EventKind::MapMiss.as_str(), "map_miss");
        assert_eq!(EventKind::TrapHit.as_str(), "trap_hit");
        assert_eq!(EventKind::FirstEdit.as_str(), "first_edit");
    }

    #[test]
    fn first_edit_tracking() {
        let conn = test_db();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_event_with(&conn, EventKind::MapHit, "/tmp/p", 100).unwrap();
        record_event_with(&conn, EventKind::FirstEdit, "/tmp/p", 45).unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/p")).unwrap();

        assert_eq!(stats.first_edit_count, 1);
        assert!((stats.avg_first_edit_secs - 45.0).abs() < f64::EPSILON);
        // first_edit token_impact (seconds) must not pollute tokens saved
        assert_eq!(stats.estimated_tokens_saved, 100);
    }

    #[test]
    fn first_edit_avg_across_sessions() {
        let conn = test_db();

        // Session 1: first edit at 30s
        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_event_with(&conn, EventKind::FirstEdit, "/tmp/p", 30).unwrap();

        // Session 2: first edit at 60s
        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_event_with(&conn, EventKind::FirstEdit, "/tmp/p", 60).unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/p")).unwrap();

        assert_eq!(stats.first_edit_count, 2);
        assert!((stats.avg_first_edit_secs - 45.0).abs() < f64::EPSILON);
    }

    #[test]
    fn first_edit_helper_is_idempotent_within_session() {
        let conn = test_db();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_first_edit_if_needed_with(&conn, "/tmp/p").unwrap();
        record_first_edit_if_needed_with(&conn, "/tmp/p").unwrap();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/p", 0).unwrap();
        record_first_edit_if_needed_with(&conn, "/tmp/p").unwrap();

        let stats = gain_stats_with(&conn, Some("/tmp/p")).unwrap();

        assert_eq!(stats.first_edit_count, 2);
        assert!(stats.avg_first_edit_secs >= 0.0);
    }

    #[test]
    fn first_edit_is_session_global_not_per_project() {
        let conn = test_db();

        record_event_with(&conn, EventKind::SessionStart, "/tmp/a", 0).unwrap();
        // First edit in project A — recorded
        record_first_edit_if_needed_with(&conn, "/tmp/a").unwrap();
        // First edit in project B — should be a no-op (session already has a first edit)
        record_first_edit_if_needed_with(&conn, "/tmp/b").unwrap();

        let stats_all = gain_stats_with(&conn, None).unwrap();
        assert_eq!(stats_all.first_edit_count, 1);

        let stats_a = gain_stats_with(&conn, Some("/tmp/a")).unwrap();
        assert_eq!(stats_a.first_edit_count, 1);

        let stats_b = gain_stats_with(&conn, Some("/tmp/b")).unwrap();
        assert_eq!(stats_b.first_edit_count, 0);
    }

    #[test]
    fn format_tokens_millions() {
        assert_eq!(format_tokens(1_500_000), "1.5M");
        assert_eq!(format_tokens(1_000_000), "1.0M");
        assert_eq!(format_tokens(42_300_000), "42.3M");
    }

    #[test]
    fn format_tokens_thousands() {
        assert_eq!(format_tokens(250_000), "250.0K");
        assert_eq!(format_tokens(1_000), "1.0K");
        assert_eq!(format_tokens(999_999), "1.0M");
    }

    #[test]
    fn format_tokens_small() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(1), "1");
    }

    #[test]
    fn bar_full_half_empty() {
        assert_eq!(bar(1.0, 10), "██████████");
        assert_eq!(bar(0.0, 10), "░░░░░░░░░░");
        assert_eq!(bar(0.5, 10), "█████░░░░░");
    }

    #[test]
    fn bar_clamps_out_of_range() {
        assert_eq!(bar(1.5, 10), "██████████");
        assert_eq!(bar(-0.5, 10), "░░░░░░░░░░");
    }

    #[test]
    fn rate_color_thresholds() {
        assert_eq!(rate_color(80.0), Color::Green);
        assert_eq!(rate_color(75.0), Color::Green);
        assert_eq!(rate_color(60.0), Color::Yellow);
        assert_eq!(rate_color(50.0), Color::Yellow);
        assert_eq!(rate_color(25.0), Color::Red);
        assert_eq!(rate_color(0.0), Color::Red);
    }

    #[test]
    fn summary_line_format() {
        let stats = GainStats {
            total_events: 100,
            map_hits: 75,
            map_misses: 25,
            trap_hits: 3,
            first_edit_count: 0,
            avg_first_edit_secs: 0.0,
            map_hit_rate: 75.0,
            estimated_tokens_saved: 500_000,
            daily: vec![],
        };
        let line = stats.summary_line();
        assert!(line.contains("100 events"));
        assert!(line.contains("75.0% hit rate"));
        assert!(line.contains("500.0K tokens saved"));
    }

    #[test]
    fn display_includes_all_sections() {
        let stats = GainStats {
            total_events: 452,
            map_hits: 325,
            map_misses: 101,
            trap_hits: 7,
            first_edit_count: 0,
            avg_first_edit_secs: 0.0,
            map_hit_rate: 76.3,
            estimated_tokens_saved: 1_025_558,
            daily: vec![DayStats {
                date: "2026-03-21".into(),
                events: 452,
                tokens_saved: 1_025_558,
            }],
        };
        let output = format!("{stats}");

        assert!(output.contains("Total events:"));
        assert!(output.contains("452"));
        assert!(output.contains("Map hits:"));
        assert!(output.contains("325"));
        assert!(output.contains("Tokens saved:"));
        assert!(output.contains("1.0M"));
        assert!(output.contains("76.3%"));
        assert!(output.contains("Daily Breakdown"));
        assert!(output.contains("2026-03-21"));
        assert!(output.contains("█"));
    }

    #[test]
    fn display_no_daily_omits_table() {
        let stats = GainStats {
            total_events: 10,
            map_hits: 8,
            map_misses: 2,
            trap_hits: 0,
            first_edit_count: 0,
            avg_first_edit_secs: 0.0,
            map_hit_rate: 80.0,
            estimated_tokens_saved: 5_000,
            daily: vec![],
        };
        let output = format!("{stats}");

        assert!(output.contains("Total events:"));
        assert!(!output.contains("Daily Breakdown"));
    }

    #[test]
    fn display_uses_first_edit_samples_label() {
        let stats = GainStats {
            total_events: 12,
            map_hits: 8,
            map_misses: 2,
            trap_hits: 1,
            first_edit_count: 3,
            avg_first_edit_secs: 42.0,
            map_hit_rate: 80.0,
            estimated_tokens_saved: 5_000,
            daily: vec![],
        };
        let output = format!("{stats}");

        assert!(output.contains("(3 samples)"));
        assert!(!output.contains("sessions"));
    }
}
