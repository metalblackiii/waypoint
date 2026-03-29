use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LearningType {
    /// User style/workflow preference. Permanent, surfaced at session start.
    Preference,
    /// Past mistake / gotcha. Surfaced at session start.
    Correction,
    /// Contextual project knowledge. Surfaced on pre-read via tag match.
    #[default]
    Discovery,
}

impl std::fmt::Display for LearningType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preference => write!(f, "preference"),
            Self::Correction => write!(f, "correction"),
            Self::Discovery => write!(f, "discovery"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEntry {
    pub id: String,
    #[serde(default)]
    pub r#type: LearningType,
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
    pub r#type: LearningType,
}

/// Add a new learning to the store.
/// Tags are required for `Discovery` type, optional for `Preference`/`Correction`.
pub fn add_learning(waypoint_dir: &Path, learning: &NewLearning<'_>) -> Result<(), AppError> {
    let mut learnings = read_learnings(waypoint_dir)?;
    let tags: Vec<String> = learning
        .tags_str
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    if tags.is_empty() && learning.r#type == LearningType::Discovery {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "At least one non-empty tag is required for discovery learnings",
        )));
    }

    let prefix = match learning.r#type {
        LearningType::Preference => "pref",
        LearningType::Correction => "corr",
        LearningType::Discovery => "learn",
    };
    let short_uuid = &uuid::Uuid::new_v4().as_simple().to_string()[..8];
    let entry = LearningEntry {
        id: format!("{prefix}-{short_uuid}"),
        r#type: learning.r#type,
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
            l.r#type == LearningType::Discovery
                && l.tags.iter().any(|tag| {
                    if tag.ends_with('/') {
                        file_path.starts_with(tag.as_str())
                    } else {
                        file_path == tag
                    }
                })
        })
        .collect()
}

/// Filter learnings by type.
#[must_use]
pub fn learnings_by_type(learnings: &[LearningEntry], typ: LearningType) -> Vec<&LearningEntry> {
    learnings.iter().filter(|l| l.r#type == typ).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn add_discovery_and_read() {
        let tmp = TempDir::new().unwrap();

        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "FTS is best-effort",
                tags_str: "src/map/index.rs,sqlite",
                r#type: LearningType::Discovery,
            },
        )
        .unwrap();

        let learnings = read_learnings(tmp.path()).unwrap();
        assert_eq!(learnings.len(), 1);
        assert_eq!(learnings[0].entry, "FTS is best-effort");
        assert_eq!(learnings[0].tags, vec!["src/map/index.rs", "sqlite"]);
        assert!(learnings[0].id.starts_with("learn-"));
        assert_eq!(learnings[0].r#type, LearningType::Discovery);
    }

    #[test]
    fn add_preference_without_tags() {
        let tmp = TempDir::new().unwrap();

        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "Use conventional commits",
                tags_str: "",
                r#type: LearningType::Preference,
            },
        )
        .unwrap();

        let learnings = read_learnings(tmp.path()).unwrap();
        assert_eq!(learnings.len(), 1);
        assert!(learnings[0].id.starts_with("pref-"));
        assert!(learnings[0].tags.is_empty());
    }

    #[test]
    fn add_correction_without_tags() {
        let tmp = TempDir::new().unwrap();

        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "Never use .unwrap() in this codebase",
                tags_str: "",
                r#type: LearningType::Correction,
            },
        )
        .unwrap();

        let learnings = read_learnings(tmp.path()).unwrap();
        assert!(learnings[0].id.starts_with("corr-"));
    }

    #[test]
    fn discovery_rejects_empty_tags() {
        let tmp = TempDir::new().unwrap();
        let result = add_learning(
            tmp.path(),
            &NewLearning {
                entry: "no tags",
                tags_str: " , ",
                r#type: LearningType::Discovery,
            },
        );
        assert!(result.is_err());
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
                r#type: LearningType::Discovery,
                entry: "FTS is best-effort".into(),
                tags: vec!["sqlite".into()],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
            LearningEntry {
                id: "learn-002".into(),
                r#type: LearningType::Discovery,
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
            r#type: LearningType::Discovery,
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
            r#type: LearningType::Discovery,
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
            r#type: LearningType::Discovery,
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
            r#type: LearningType::Discovery,
            entry: "hook stuff".into(),
            tags: vec!["src/hook/".into()],
            logged_at: "2026-01-01T00:00:00Z".into(),
        }];

        // "src/hook/" should NOT match "src/hookutils.rs"
        assert_eq!(learnings_for_file(&learnings, "src/hookutils.rs").len(), 0);
    }

    #[test]
    fn add_preserves_tags_as_given() {
        let tmp = TempDir::new().unwrap();
        add_learning(
            tmp.path(),
            &NewLearning {
                entry: "hook stuff",
                tags_str: "src/hook/,src/trap.rs,sqlite",
                r#type: LearningType::Discovery,
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

    #[test]
    fn learnings_by_type_filters_correctly() {
        let learnings = vec![
            LearningEntry {
                id: "pref-001".into(),
                r#type: LearningType::Preference,
                entry: "Use snake_case".into(),
                tags: vec![],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
            LearningEntry {
                id: "corr-001".into(),
                r#type: LearningType::Correction,
                entry: "Don't use unwrap".into(),
                tags: vec![],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
            LearningEntry {
                id: "learn-001".into(),
                r#type: LearningType::Discovery,
                entry: "FTS uses BM25".into(),
                tags: vec!["src/".into()],
                logged_at: "2026-01-01T00:00:00Z".into(),
            },
        ];

        assert_eq!(
            learnings_by_type(&learnings, LearningType::Preference).len(),
            1
        );
        assert_eq!(
            learnings_by_type(&learnings, LearningType::Correction).len(),
            1
        );
        assert_eq!(
            learnings_by_type(&learnings, LearningType::Discovery).len(),
            1
        );
    }

    #[test]
    fn deserialize_legacy_entry_defaults_to_discovery() {
        let json = r#"[{"id":"learn-abc","entry":"old entry","tags":["src/"],"logged_at":"2026-01-01T00:00:00Z"}]"#;
        let learnings: Vec<LearningEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(learnings[0].r#type, LearningType::Discovery);
    }
}
