use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEntry {
    pub id: String,
    pub entry: String,
    pub tags: Vec<String>,
    pub logged_at: String,
}

/// Read all learnings from learnings.json. Returns empty vec if file doesn't exist.
pub fn read_learnings(waypoint_dir: &Path) -> Result<Vec<LearningEntry>, AppError> {
    let path = waypoint_dir.join("learnings.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let learnings: Vec<LearningEntry> = serde_json::from_str(&content)?;
    Ok(learnings)
}

/// Write learnings to learnings.json atomically. Deletes the file if empty.
fn write_learnings(waypoint_dir: &Path, learnings: &[LearningEntry]) -> Result<(), AppError> {
    let path = waypoint_dir.join("learnings.json");
    if learnings.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    let content = serde_json::to_string_pretty(learnings)?;
    crate::project::atomic_write(&path, &content)
}

/// Fields for adding a new learning entry.
pub struct NewLearning<'a> {
    pub entry: &'a str,
    pub tags_str: &'a str,
}

/// Add a new learning to the store. Returns an error if no valid tags are provided.
pub fn add_learning(waypoint_dir: &Path, learning: &NewLearning<'_>) -> Result<(), AppError> {
    let mut learnings = read_learnings(waypoint_dir)?;
    let tags: Vec<String> = learning
        .tags_str
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    if tags.is_empty() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "At least one non-empty tag is required",
        )));
    }

    let short_uuid = &uuid::Uuid::new_v4().as_simple().to_string()[..8];
    let entry = LearningEntry {
        id: format!("learn-{short_uuid}"),
        entry: learning.entry.to_string(),
        tags,
        logged_at: Utc::now().to_rfc3339(),
    };

    learnings.push(entry);
    write_learnings(waypoint_dir, &learnings)
}

/// Search learnings by keyword across entry text and tags.
#[must_use]
pub fn search<'a>(learnings: &'a [LearningEntry], query: &str) -> Vec<&'a LearningEntry> {
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(&LearningEntry, usize)> = learnings
        .iter()
        .filter_map(|learning| {
            let searchable = format!(
                "{} {} {}",
                learning.entry,
                learning.tags.join(" "),
                learning.id
            )
            .to_lowercase();

            let score = query_terms
                .iter()
                .filter(|term| searchable.contains(**term))
                .count();

            if score > 0 {
                Some((learning, score))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(learning, _)| learning).collect()
}

/// Find learnings relevant to a specific file via tag prefix matching.
/// Directory tags must end with `/` — `src/hook/` matches `src/hook/pre_read.rs`
/// but not `src/hookutils.rs`. Exact file tags match exactly.
#[must_use]
pub fn learnings_for_file<'a>(
    learnings: &'a [LearningEntry],
    file_path: &str,
) -> Vec<&'a LearningEntry> {
    learnings
        .iter()
        .filter(|l| {
            l.tags.iter().any(|tag| {
                if tag.ends_with('/') {
                    file_path.starts_with(tag.as_str())
                } else {
                    file_path == tag
                }
            })
        })
        .collect()
}

/// Prune learnings older than `max_age_days`. Returns the removed entries.
/// Deletes learnings.json if all entries are pruned.
pub fn prune(waypoint_dir: &Path, max_age_days: i64) -> Result<Vec<LearningEntry>, AppError> {
    let learnings = read_learnings(waypoint_dir)?;
    let cutoff = Utc::now() - chrono::Duration::days(max_age_days);

    let (keep, pruned): (Vec<_>, Vec<_>) = learnings.into_iter().partition(|l| {
        chrono::DateTime::parse_from_rfc3339(&l.logged_at)
            .map(|dt| dt >= cutoff)
            .unwrap_or(true) // keep entries with unparseable dates
    });

    write_learnings(waypoint_dir, &keep)?;
    Ok(pruned)
}

/// Parse a duration string like "90d" into days. Only supports `Nd` format.
#[must_use]
pub fn parse_duration_days(s: &str) -> Option<i64> {
    s.trim()
        .strip_suffix('d')
        .and_then(|n| n.parse::<i64>().ok())
        .filter(|&d| d > 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn add_and_read() {
        let tmp = TempDir::new().unwrap();

        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "FTS is best-effort",
                tags_str: "src/map/index.rs,sqlite",
            },
        )
        .unwrap();

        let learnings = read_learnings(tmp.path()).unwrap();
        assert_eq!(learnings.len(), 1);
        assert_eq!(learnings[0].entry, "FTS is best-effort");
        assert_eq!(learnings[0].tags, vec!["src/map/index.rs", "sqlite"]);
        assert!(learnings[0].id.starts_with("learn-"));
    }

    #[test]
    fn read_missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let learnings = read_learnings(tmp.path()).unwrap();
        assert!(learnings.is_empty());
    }

    #[test]
    fn search_by_term() {
        let learnings = vec![
            LearningEntry {
                id: "learn-001".into(),
                entry: "FTS is best-effort".into(),
                tags: vec!["sqlite".into()],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
            LearningEntry {
                id: "learn-002".into(),
                entry: "BufWriter is essential".into(),
                tags: vec!["io".into()],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
        ];

        let results = search(&learnings, "FTS");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "learn-001");
    }

    #[test]
    fn search_by_tag() {
        let learnings = vec![LearningEntry {
            id: "learn-001".into(),
            entry: "some learning".into(),
            tags: vec!["sqlite".into(), "fts".into()],
            logged_at: "2026-01-01T00:00:00Z".into(),
        }];

        let results = search(&learnings, "sqlite");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn learnings_for_file_exact_match() {
        let learnings = vec![LearningEntry {
            id: "learn-001".into(),
            entry: "trap dedup uses Jaccard".into(),
            tags: vec!["src/trap.rs".into()],
            logged_at: "2026-01-01T00:00:00Z".into(),
        }];

        assert_eq!(learnings_for_file(&learnings, "src/trap.rs").len(), 1);
        assert_eq!(learnings_for_file(&learnings, "src/other.rs").len(), 0);
    }

    #[test]
    fn learnings_for_file_prefix_match() {
        let learnings = vec![LearningEntry {
            id: "learn-001".into(),
            entry: "hooks resolve foreign projects".into(),
            tags: vec!["src/hook/".into()],
            logged_at: "2026-01-01T00:00:00Z".into(),
        }];

        assert_eq!(
            learnings_for_file(&learnings, "src/hook/pre_read.rs").len(),
            1
        );
        assert_eq!(
            learnings_for_file(&learnings, "src/hook/session_start.rs").len(),
            1
        );
    }

    #[test]
    fn learnings_for_file_no_false_prefix() {
        let learnings = vec![LearningEntry {
            id: "learn-001".into(),
            entry: "hook stuff".into(),
            tags: vec!["src/hook/".into()],
            logged_at: "2026-01-01T00:00:00Z".into(),
        }];

        // "src/hook/" should NOT match "src/hookutils.rs"
        assert_eq!(learnings_for_file(&learnings, "src/hookutils.rs").len(), 0);
    }

    #[test]
    fn prune_removes_old_entries() {
        let tmp = TempDir::new().unwrap();
        let old_date = (Utc::now() - chrono::Duration::days(100)).to_rfc3339();
        let new_date = Utc::now().to_rfc3339();

        let learnings = vec![
            LearningEntry {
                id: "learn-old".into(),
                entry: "old learning".into(),
                tags: vec!["old".into()],
                logged_at: old_date,
            },
            LearningEntry {
                id: "learn-new".into(),
                entry: "new learning".into(),
                tags: vec!["new".into()],
                logged_at: new_date,
            },
        ];

        let content = serde_json::to_string_pretty(&learnings).unwrap();
        std::fs::write(tmp.path().join("learnings.json"), content).unwrap();

        let pruned = prune(tmp.path(), 90).unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].id, "learn-old");

        let remaining = read_learnings(tmp.path()).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "learn-new");
    }

    #[test]
    fn prune_all_deletes_file() {
        let tmp = TempDir::new().unwrap();
        let old_date = (Utc::now() - chrono::Duration::days(100)).to_rfc3339();

        let learnings = vec![LearningEntry {
            id: "learn-old".into(),
            entry: "old".into(),
            tags: vec!["old".into()],
            logged_at: old_date,
        }];

        let content = serde_json::to_string_pretty(&learnings).unwrap();
        let file_path = tmp.path().join("learnings.json");
        std::fs::write(&file_path, content).unwrap();

        let pruned = prune(tmp.path(), 90).unwrap();
        assert_eq!(pruned.len(), 1);
        assert!(
            !file_path.exists(),
            "file should be deleted when all entries pruned"
        );
    }

    #[test]
    fn parse_duration_days_valid() {
        assert_eq!(parse_duration_days("90d"), Some(90));
        assert_eq!(parse_duration_days("1d"), Some(1));
        assert_eq!(parse_duration_days("365d"), Some(365));
    }

    #[test]
    fn parse_duration_days_invalid() {
        assert_eq!(parse_duration_days("90"), None);
        assert_eq!(parse_duration_days("d"), None);
        assert_eq!(parse_duration_days("0d"), None);
        assert_eq!(parse_duration_days("-5d"), None);
        assert_eq!(parse_duration_days("abc"), None);
    }

    #[test]
    fn add_rejects_empty_tags() {
        let tmp = TempDir::new().unwrap();
        let result = add_learning(
            tmp.path(),
            &NewLearning {
                entry: "no tags",
                tags_str: " , ",
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn add_preserves_tags_as_given() {
        let tmp = TempDir::new().unwrap();
        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "hook stuff",
                tags_str: "src/hook/,src/trap.rs,sqlite",
            },
        )
        .unwrap();

        let learnings = read_learnings(tmp.path()).unwrap();
        assert_eq!(
            learnings[0].tags,
            vec!["src/hook/", "src/trap.rs", "sqlite"],
            "tags should be stored exactly as provided"
        );
    }
}
