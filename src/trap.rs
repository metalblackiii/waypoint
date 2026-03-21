use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrapEntry {
    pub id: String,
    pub error_message: String,
    pub file: String,
    pub root_cause: String,
    pub fix: String,
    pub tags: Vec<String>,
    pub logged_at: String,
    #[serde(default = "default_occurrences")]
    pub occurrences: u32,
}

fn default_occurrences() -> u32 {
    1
}

/// Read all traps from traps.json.
pub fn read_traps(waypoint_dir: &Path) -> Result<Vec<TrapEntry>, AppError> {
    let path = waypoint_dir.join("traps.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let traps: Vec<TrapEntry> = serde_json::from_str(&content)?;
    Ok(traps)
}

/// Write traps to traps.json atomically.
fn write_traps(waypoint_dir: &Path, traps: &[TrapEntry]) -> Result<(), AppError> {
    let path = waypoint_dir.join("traps.json");
    let tmp_path = waypoint_dir.join("traps.json.tmp");
    let content = serde_json::to_string_pretty(traps)?;
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Fields for logging a new trap entry.
pub struct NewTrap<'a> {
    pub error_message: &'a str,
    pub file: &'a str,
    pub root_cause: &'a str,
    pub fix: &'a str,
    pub tags_str: &'a str,
}

/// Log a new trap. Returns a warning message if a duplicate was detected.
pub fn log_trap(
    waypoint_dir: &Path,
    trap: &NewTrap<'_>,
) -> Result<Option<String>, AppError> {
    let mut traps = read_traps(waypoint_dir)?;
    let tags: Vec<String> = trap
        .tags_str
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    // FR-14: Dedup guard — only dedup within the same file
    if let Some(dup_idx) = find_duplicate_idx(&traps, trap.error_message, trap.file) {
        traps[dup_idx].occurrences += 1;
        traps[dup_idx].logged_at = Utc::now().to_rfc3339();
        let id = traps[dup_idx].id.clone();
        let count = traps[dup_idx].occurrences;
        write_traps(waypoint_dir, &traps)?;
        return Ok(Some(format!(
            "Duplicate detected: {id} has similar error (occurrence #{count})"
        )));
    }

    let short_uuid = &uuid::Uuid::new_v4().as_simple().to_string()[..8];
    let entry = TrapEntry {
        id: format!("trap-{short_uuid}"),
        error_message: trap.error_message.to_string(),
        file: trap.file.to_string(),
        root_cause: trap.root_cause.to_string(),
        fix: trap.fix.to_string(),
        tags,
        logged_at: Utc::now().to_rfc3339(),
        occurrences: 1,
    };

    traps.push(entry);
    write_traps(waypoint_dir, &traps)?;
    Ok(None)
}

/// Search traps by keyword across all text fields.
#[must_use]
pub fn search<'a>(traps: &'a [TrapEntry], query: &str) -> Vec<&'a TrapEntry> {
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(&TrapEntry, usize)> = traps
        .iter()
        .filter_map(|trap| {
            let searchable = format!(
                "{} {} {} {} {} {}",
                trap.error_message,
                trap.root_cause,
                trap.fix,
                trap.file,
                trap.tags.join(" "),
                trap.id
            )
            .to_lowercase();

            let score = query_terms
                .iter()
                .filter(|term| searchable.contains(**term))
                .count();

            if score > 0 { Some((trap, score)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(trap, _)| trap).collect()
}

/// Find traps relevant to a specific file.
#[must_use]
pub fn traps_for_file<'a>(traps: &'a [TrapEntry], file_path: &str) -> Vec<&'a TrapEntry> {
    traps.iter().filter(|t| t.file == file_path).collect()
}

/// Find the index of a duplicate trap using Jaccard similarity on normalized error messages,
/// scoped to the same file to avoid merging distinct per-file traps.
fn find_duplicate_idx(traps: &[TrapEntry], error_message: &str, file: &str) -> Option<usize> {
    let new_terms = normalize_terms(error_message);
    if new_terms.is_empty() {
        return None;
    }

    for (i, trap) in traps.iter().enumerate() {
        if trap.file != file {
            continue;
        }
        let existing_terms = normalize_terms(&trap.error_message);
        if jaccard_similarity(&new_terms, &existing_terms) > 0.7 {
            return Some(i);
        }
    }
    None
}

/// Normalize an error message into a set of meaningful terms.
fn normalize_terms(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| {
            let mut result = String::new();
            let mut chars = w.chars().peekable();
            while let Some(c) = chars.next() {
                if c.is_ascii_digit() {
                    result.push('N');
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                } else {
                    result.push(c);
                }
            }
            result
        })
        .collect()
}

/// Compute Jaccard similarity between two term sets.
fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // small set sizes — precision loss irrelevant
    { intersection as f64 / union as f64 }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn log_and_search() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("traps.json"), "[]").unwrap();

        let result = log_trap(
            tmp.path(),
            &NewTrap {
                error_message: "TypeError: Cannot read properties of undefined",
                file: "src/api/users.ts",
                root_cause: "API response was null",
                fix: "Added optional chaining",
                tags_str: "null-check, api",
            },
        )
        .unwrap();

        assert!(result.is_none(), "first log should not detect duplicate");

        let traps = read_traps(tmp.path()).unwrap();
        assert_eq!(traps.len(), 1);
        assert_eq!(traps[0].tags, vec!["null-check", "api"]);

        let results = search(&traps, "Cannot read properties");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn dedup_detection() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("traps.json"), "[]").unwrap();

        log_trap(
            tmp.path(),
            &NewTrap {
                error_message: "TypeError: Cannot read properties of undefined (reading 'map')",
                file: "src/users.ts",
                root_cause: "null response",
                fix: "added optional chaining",
                tags_str: "null-check",
            },
        )
        .unwrap();

        // Very similar error message — should trigger dedup
        let result = log_trap(
            tmp.path(),
            &NewTrap {
                error_message: "TypeError: Cannot read properties of undefined (reading 'filter')",
                file: "src/users.ts",
                root_cause: "null response again",
                fix: "added fallback array",
                tags_str: "null-check",
            },
        )
        .unwrap();

        assert!(result.is_some(), "should detect duplicate");
        let traps = read_traps(tmp.path()).unwrap();
        assert_eq!(traps.len(), 1, "should not create a second entry");
        assert_eq!(traps[0].occurrences, 2);
    }

    #[test]
    fn dedup_scoped_to_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("traps.json"), "[]").unwrap();

        log_trap(
            tmp.path(),
            &NewTrap {
                error_message: "TypeError: Cannot read properties of undefined (reading 'map')",
                file: "src/a.ts",
                root_cause: "null response",
                fix: "added optional chaining",
                tags_str: "null-check",
            },
        )
        .unwrap();

        // Same error but different file — should NOT trigger dedup
        let result = log_trap(
            tmp.path(),
            &NewTrap {
                error_message: "TypeError: Cannot read properties of undefined (reading 'map')",
                file: "src/b.ts",
                root_cause: "null response",
                fix: "added optional chaining",
                tags_str: "null-check",
            },
        )
        .unwrap();

        assert!(result.is_none(), "different files should not dedup");
        let traps = read_traps(tmp.path()).unwrap();
        assert_eq!(traps.len(), 2, "should create separate entries per file");
    }

    #[test]
    fn traps_for_file_filters() {
        let traps = vec![
            TrapEntry {
                id: "trap-001".into(),
                error_message: "err1".into(),
                file: "src/a.ts".into(),
                root_cause: "cause".into(),
                fix: "fix".into(),
                tags: vec![],
                logged_at: "2026-01-01".into(),
                occurrences: 1,
            },
            TrapEntry {
                id: "trap-002".into(),
                error_message: "err2".into(),
                file: "src/b.ts".into(),
                root_cause: "cause".into(),
                fix: "fix".into(),
                tags: vec![],
                logged_at: "2026-01-01".into(),
                occurrences: 1,
            },
        ];

        assert_eq!(traps_for_file(&traps, "src/a.ts").len(), 1);
        assert_eq!(traps_for_file(&traps, "src/c.ts").len(), 0);
    }

    #[test]
    fn jaccard_identical() {
        let a: HashSet<String> = ["foo", "bar", "baz"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!((jaccard_similarity(&a, &a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint() {
        let a: HashSet<String> = ["foo"].iter().map(ToString::to_string).collect();
        let b: HashSet<String> = ["bar"].iter().map(ToString::to_string).collect();
        assert!((jaccard_similarity(&a, &b)).abs() < f64::EPSILON);
    }
}
