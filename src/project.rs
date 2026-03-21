use std::path::{Path, PathBuf};

use crate::AppError;

/// Find the project root by walking up from `start` looking for .git (primary) or .waypoint/ (secondary).
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if current.join(".waypoint").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Return the .waypoint directory path for the given project root.
pub fn waypoint_dir(project_root: &Path) -> PathBuf {
    project_root.join(".waypoint")
}

/// Ensure .waypoint/ exists with initial files. Returns the waypoint dir path.
pub fn ensure_initialized(project_root: &Path) -> Result<PathBuf, AppError> {
    let dir = waypoint_dir(project_root);
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }

    let journal_path = dir.join("journal.md");
    if !journal_path.exists() {
        std::fs::write(&journal_path, crate::journal::empty_journal())?;
    }

    let traps_path = dir.join("traps.json");
    if !traps_path.exists() {
        std::fs::write(&traps_path, "[]")?;
    }

    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_root_from_git_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let sub = tmp.path().join("src/deep");
        std::fs::create_dir_all(&sub).unwrap();

        assert_eq!(find_root(&sub), Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_root_from_waypoint_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".waypoint")).unwrap();

        assert_eq!(find_root(tmp.path()), Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn ensure_initialized_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let wp = ensure_initialized(tmp.path()).unwrap();

        assert!(wp.join("journal.md").exists());
        assert!(wp.join("traps.json").exists());
    }
}
