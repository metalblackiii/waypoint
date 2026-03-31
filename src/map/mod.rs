pub mod extract;
pub mod index;
pub mod scan;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::AppError;

/// Maps older than this many days are considered stale.
pub const MAP_STALE_DAYS: i64 = 14;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEntry {
    pub path: String,
    pub description: String,
    pub token_estimate: usize,
}

/// Parse map.md into a list of entries.
pub fn read_map(waypoint_dir: &Path) -> Result<Vec<MapEntry>, AppError> {
    let map_path = waypoint_dir.join("map.md");
    if !map_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&map_path)?;
    Ok(parse_map(&content))
}

fn parse_map(content: &str) -> Vec<MapEntry> {
    let mut entries = Vec::new();
    let mut current_dir = String::new();

    for line in content.lines() {
        if let Some(dir) = line.strip_prefix("## ") {
            current_dir = dir.trim_end_matches('/').to_string();
            if current_dir == "." {
                current_dir.clear();
            }
        } else if let Some(rest) = line.strip_prefix("- `")
            && let Some(backtick_end) = rest.find('`')
        {
            let filename = &rest[..backtick_end];
            let path = if current_dir.is_empty() {
                filename.to_string()
            } else {
                format!("{current_dir}/{filename}")
            };

            let after_backtick = &rest[backtick_end + 1..];
            let (description, token_estimate) = parse_entry_tail(after_backtick);

            entries.push(MapEntry {
                path,
                description,
                token_estimate,
            });
        }
    }

    entries
}

/// Parse " — description (~N tok)" from the tail of a map entry line.
fn parse_entry_tail(s: &str) -> (String, usize) {
    let s = s
        .strip_prefix(" — ")
        .or_else(|| s.strip_prefix(" - "))
        .unwrap_or(s);

    if let Some(paren_start) = s.rfind("(~") {
        let before_paren = s[..paren_start].trim();
        let in_paren = &s[paren_start + 2..];
        if let Some(tok_end) = in_paren.find(" tok)")
            && let Ok(tokens) = in_paren[..tok_end].trim().parse::<usize>()
        {
            return (before_paren.to_string(), tokens);
        }
    }

    (s.trim().to_string(), 0)
}

/// Write entries to map.md grouped by directory. Uses atomic write (temp + rename).
pub fn write_map(waypoint_dir: &Path, entries: &[MapEntry]) -> Result<(), AppError> {
    let map_path = waypoint_dir.join("map.md");

    let mut grouped: BTreeMap<String, Vec<&MapEntry>> = BTreeMap::new();
    for entry in entries {
        let dir = match entry.path.rfind('/') {
            Some(pos) => entry.path[..pos].to_string(),
            None => ".".to_string(),
        };
        grouped.entry(dir).or_default().push(entry);
    }

    crate::project::atomic_write_with(&map_path, |file| {
        writeln!(file, "# Waypoint Map")?;
        writeln!(file)?;
        writeln!(
            file,
            "<!-- Generated: {} | Files: {} -->",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            entries.len()
        )?;

        for (dir, dir_entries) in &grouped {
            writeln!(file)?;
            writeln!(file, "## {dir}")?;
            writeln!(file)?;
            for entry in dir_entries {
                let filename = entry.path.rsplit('/').next().unwrap_or(&entry.path);
                writeln!(
                    file,
                    "- `{filename}` — {} (~{} tok)",
                    entry.description, entry.token_estimate
                )?;
            }
        }

        Ok(())
    })?;

    // Rebuild SQLite index alongside map.md — if rebuild fails, remove
    // the stale DB so pre_read falls back to map.md instead of serving old data
    if index::rebuild(waypoint_dir, entries).is_err() {
        let _ = std::fs::remove_file(waypoint_dir.join("map_index.db"));
    }

    Ok(())
}

/// Metadata parsed from the map.md header comment.
/// Both fields are required — `parse_map_header` returns `None` if either fails to parse.
#[derive(Debug)]
pub struct MapHeader {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub file_count: usize,
}

/// Parse the `<!-- Generated: ... | Files: N -->` header from map.md.
/// Returns `None` if map.md doesn't exist, the header is missing, or either
/// the timestamp or file count fails to parse.
#[must_use]
pub fn parse_map_header(waypoint_dir: &Path) -> Option<MapHeader> {
    use std::io::BufRead;

    let map_path = waypoint_dir.join("map.md");
    let file = std::fs::File::open(map_path).ok()?;
    let reader = std::io::BufReader::new(file);

    for line in reader.lines().take(5) {
        let Ok(line) = line else { return None };
        if let Some(rest) = line.strip_prefix("<!-- Generated: ") {
            let rest = rest.strip_suffix(" -->")?;
            let (ts_str, count_str) = rest.split_once(" | Files: ")?;

            let generated_at = chrono::DateTime::parse_from_rfc3339(ts_str)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))?;
            let file_count: usize = count_str.parse().ok()?;

            return Some(MapHeader {
                generated_at,
                file_count,
            });
        }
    }

    None
}

/// Look up a file in the map by relative path.
#[must_use]
pub fn lookup<'a>(entries: &'a [MapEntry], relative_path: &str) -> Option<&'a MapEntry> {
    entries.iter().find(|e| e.path == relative_path)
}

/// Result of comparing a current scan against the existing map.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StalenessReport {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}

impl StalenessReport {
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.added > 0 || self.removed > 0 || self.modified > 0
    }
}

impl fmt::Display for StalenessReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_stale() {
            return write!(f, "up to date");
        }
        let mut first = true;
        for (count, label) in [
            (self.added, "added"),
            (self.removed, "removed"),
            (self.modified, "modified"),
        ] {
            if count > 0 {
                if !first {
                    write!(f, ", ")?;
                }
                write!(f, "{count} {label}")?;
                first = false;
            }
        }
        Ok(())
    }
}

/// Compare current scan against existing map.
#[must_use]
pub fn check_staleness(current: &[MapEntry], existing: &[MapEntry]) -> StalenessReport {
    use std::collections::HashMap;

    let current_map: HashMap<&str, &MapEntry> =
        current.iter().map(|e| (e.path.as_str(), e)).collect();
    let existing_map: HashMap<&str, &MapEntry> =
        existing.iter().map(|e| (e.path.as_str(), e)).collect();

    let added = current_map
        .keys()
        .filter(|k| !existing_map.contains_key(*k))
        .count();
    let removed = existing_map
        .keys()
        .filter(|k| !current_map.contains_key(*k))
        .count();
    let modified = current_map
        .iter()
        .filter(|(path, entry)| {
            existing_map.get(*path).is_some_and(|e| {
                e.description != entry.description || e.token_estimate != entry.token_estimate
            })
        })
        .count();

    StalenessReport {
        added,
        removed,
        modified,
    }
}

/// Update a single entry in `map.md` (parse, replace or insert, write).
/// The `SQLite` index is rebuilt via `write_map`.
pub fn update_entry(waypoint_dir: &Path, new_entry: MapEntry) -> Result<(), AppError> {
    let mut entries = read_map(waypoint_dir)?;

    if let Some(existing) = entries.iter_mut().find(|e| e.path == new_entry.path) {
        existing.description = new_entry.description;
        existing.token_estimate = new_entry.token_estimate;
    } else {
        entries.push(new_entry);
        entries.sort_by(|a, b| a.path.cmp(&b.path));
    }

    write_map(waypoint_dir, &entries)
}

/// Estimate token count for file content.
#[must_use]
pub fn estimate_tokens(content: &str, path: &Path) -> usize {
    let ratio = match path.extension().and_then(|e| e.to_str()) {
        Some("md" | "txt" | "rst" | "adoc") => 4.0,
        Some("json" | "yaml" | "yml" | "toml" | "xml") => 3.75,
        _ => 3.5,
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    // ceil() guarantees non-negative; content.len() / 3.5 fits in usize
    {
        (content.len() as f64 / ratio).ceil() as usize
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn roundtrip_map() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![
            MapEntry {
                path: "src/main.rs".into(),
                description: "entry point".into(),
                token_estimate: 45,
            },
            MapEntry {
                path: "src/lib.rs".into(),
                description: "library root".into(),
                token_estimate: 80,
            },
        ];

        write_map(tmp.path(), &entries).unwrap();
        let read_back = read_map(tmp.path()).unwrap();

        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back[0].path, "src/main.rs");
        assert_eq!(read_back[0].description, "entry point");
        assert_eq!(read_back[0].token_estimate, 45);
        assert_eq!(read_back[1].path, "src/lib.rs");
        assert_eq!(read_back[1].description, "library root");
        assert_eq!(read_back[1].token_estimate, 80);
    }

    #[test]
    fn parse_entry_tail_extracts_tokens() {
        let (desc, tok) = parse_entry_tail(" — fn main(), struct Config (~120 tok)");
        assert_eq!(desc, "fn main(), struct Config");
        assert_eq!(tok, 120);
    }

    #[test]
    fn lookup_finds_entry() {
        let entries = vec![MapEntry {
            path: "src/foo.rs".into(),
            description: "test".into(),
            token_estimate: 10,
        }];
        assert!(lookup(&entries, "src/foo.rs").is_some());
        assert!(lookup(&entries, "src/bar.rs").is_none());
    }

    #[test]
    fn update_entry_adds_new() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![MapEntry {
            path: "a.rs".into(),
            description: "first".into(),
            token_estimate: 10,
        }];
        write_map(tmp.path(), &entries).unwrap();

        update_entry(
            tmp.path(),
            MapEntry {
                path: "b.rs".into(),
                description: "second".into(),
                token_estimate: 20,
            },
        )
        .unwrap();

        let result = read_map(tmp.path()).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn check_staleness_detects_modified() {
        let existing = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "fn alpha()".into(),
            token_estimate: 50,
        }];
        let modified_desc = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "fn beta()".into(),
            token_estimate: 50,
        }];
        let modified_tokens = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "fn alpha()".into(),
            token_estimate: 999,
        }];
        let identical = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "fn alpha()".into(),
            token_estimate: 50,
        }];

        let r1 = check_staleness(&modified_desc, &existing);
        assert_eq!(r1.modified, 1);
        assert!(r1.is_stale());

        let r2 = check_staleness(&modified_tokens, &existing);
        assert_eq!(r2.modified, 1);

        let r3 = check_staleness(&identical, &existing);
        assert!(!r3.is_stale());
        assert_eq!(format!("{r3}"), "up to date");
    }

    #[test]
    fn check_staleness_detects_added_and_removed() {
        let existing = vec![MapEntry {
            path: "a.rs".into(),
            description: "a".into(),
            token_estimate: 10,
        }];
        let current = vec![MapEntry {
            path: "b.rs".into(),
            description: "b".into(),
            token_estimate: 20,
        }];

        let report = check_staleness(&current, &existing);
        assert_eq!(report.added, 1);
        assert_eq!(report.removed, 1);
        assert_eq!(report.modified, 0);
        assert_eq!(format!("{report}"), "1 added, 1 removed");
    }

    #[test]
    fn write_map_builds_index() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "entry point".into(),
            token_estimate: 45,
        }];

        write_map(tmp.path(), &entries).unwrap();

        // Index should be queryable after write_map
        let found = index::lookup(tmp.path(), "src/main.rs").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().description, "entry point");
    }

    #[test]
    fn write_map_removes_stale_index_on_rebuild_failure() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![MapEntry {
            path: "src/main.rs".into(),
            description: "original".into(),
            token_estimate: 45,
        }];
        write_map(tmp.path(), &entries).unwrap();

        // Corrupt the index by replacing the DB file with a directory
        let db_path = tmp.path().join("map_index.db");
        std::fs::remove_file(&db_path).unwrap();
        std::fs::create_dir(&db_path).unwrap();

        // write_map should succeed (map.md is written) even though rebuild fails
        let new_entries = vec![MapEntry {
            path: "src/new.rs".into(),
            description: "new file".into(),
            token_estimate: 100,
        }];
        write_map(tmp.path(), &new_entries).unwrap();

        // map.md has the new data
        let from_md = read_map(tmp.path()).unwrap();
        assert_eq!(from_md.len(), 1);
        assert_eq!(from_md[0].path, "src/new.rs");

        // Index lookup should fail, triggering fallback to map.md
        assert!(index::lookup(tmp.path(), "src/new.rs").is_err());
    }

    #[test]
    fn parse_map_header_from_written_map() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![
            MapEntry {
                path: "a.rs".into(),
                description: "a".into(),
                token_estimate: 10,
            },
            MapEntry {
                path: "b.rs".into(),
                description: "b".into(),
                token_estimate: 20,
            },
        ];
        write_map(tmp.path(), &entries).unwrap();

        let header = parse_map_header(tmp.path()).unwrap();
        assert_eq!(header.file_count, 2);
    }

    #[test]
    fn parse_map_header_missing_map() {
        let tmp = TempDir::new().unwrap();
        assert!(parse_map_header(tmp.path()).is_none());
    }

    #[test]
    fn parse_map_header_malformed_timestamp() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("map.md"),
            "# Waypoint Map\n\n<!-- Generated: NOT-A-DATE | Files: 10 -->\n",
        )
        .unwrap();
        // Malformed timestamp → None (triggers rescan)
        assert!(parse_map_header(tmp.path()).is_none());
    }

    #[test]
    fn parse_map_header_malformed_count() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("map.md"),
            "# Waypoint Map\n\n<!-- Generated: 2026-03-22T00:00:00Z | Files: abc -->\n",
        )
        .unwrap();
        // Malformed count → None (triggers rescan)
        assert!(parse_map_header(tmp.path()).is_none());
    }

    #[test]
    fn estimate_tokens_code_vs_prose() {
        let code = "fn main() { println!(\"hello\"); }";
        let prose = "This is a paragraph of documentation text.";

        let code_tokens = estimate_tokens(code, Path::new("main.rs"));
        let prose_tokens = estimate_tokens(prose, Path::new("README.md"));

        // Code uses 3.5 ratio, prose uses 4.0 — code should yield more tokens per char
        assert!(code_tokens > 0);
        assert!(prose_tokens > 0);
    }
}
