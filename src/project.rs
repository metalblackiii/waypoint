use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::AppError;

/// Find the project root by walking up from `start` looking for .git (primary) or .waypoint/ (secondary).
#[must_use]
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
#[must_use]
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

/// Derive a sibling `.tmp` path for atomic writes.
fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Atomically write string content via temp-file-then-rename.
pub fn atomic_write(path: &Path, content: &str) -> Result<(), AppError> {
    let tmp_path = tmp_path_for(path);
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Atomically write to a file using a callback that receives a `BufWriter`.
pub fn atomic_write_with<F>(path: &Path, f: F) -> Result<(), AppError>
where
    F: FnOnce(&mut BufWriter<std::fs::File>) -> Result<(), AppError>,
{
    let tmp_path = tmp_path_for(path);
    let mut writer = BufWriter::new(std::fs::File::create(&tmp_path)?);
    f(&mut writer)?;
    writer.flush()?;
    drop(writer);
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

    #[test]
    fn atomic_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write(&path, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_no_leftover_tmp() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write(&path, "hello").unwrap();
        assert!(!tmp.path().join("test.txt.tmp").exists());
    }

    #[test]
    fn atomic_write_with_bufwriter() {
        use std::io::Write;
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write_with(&path, |w| {
            writeln!(w, "line 1")?;
            writeln!(w, "line 2")?;
            Ok(())
        })
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("line 1"));
        assert!(content.contains("line 2"));
    }
}
