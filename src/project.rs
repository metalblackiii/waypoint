use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::AppError;

/// Resolved foreign project context — project root, waypoint dir, and relative file path.
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub root: PathBuf,
    pub wp_dir: PathBuf,
    pub relative_path: String,
}

/// Resolve a file's project from its absolute path.
///
/// Walks up from the file path to find a project root with a `.waypoint/` directory.
/// Returns `None` if no waypoint-managed project is found.
#[must_use]
pub fn resolve_foreign(file_path: &str) -> Option<ResolvedProject> {
    let path = Path::new(file_path);
    let root = find_root(path)?;
    let wp_dir = waypoint_dir(&root);
    if !wp_dir.exists() {
        return None;
    }
    let relative = path.strip_prefix(&root).ok()?;
    Some(ResolvedProject {
        root,
        wp_dir,
        relative_path: relative.to_string_lossy().into_owned(),
    })
}

/// Resolve a project from an optional `-C` path override, falling back to cwd.
///
/// If `context_path` is `Some`, resolves from that path.
/// If `context_path` is `None`, falls back to cwd resolution.
/// Does NOT check for `.waypoint/` existence — callers that need it should
/// use `require_waypoint_dir` or `ensure_initialized`.
pub fn resolve_with_context(context_path: Option<&str>) -> Result<PathBuf, AppError> {
    if let Some(path) = context_path {
        let abs = if Path::new(path).is_relative() {
            std::env::current_dir()?.join(path)
        } else {
            PathBuf::from(path)
        };
        let root = find_root(&abs).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no project root found for: {path}"),
            )
        })?;
        Ok(root)
    } else {
        let cwd = std::env::current_dir()?;
        Ok(find_root(&cwd).unwrap_or(cwd))
    }
}

/// Discover git repositories under `base` for batch scanning.
///
/// If `base` is a git repo with child git repos, scans children (monorepo parent case).
/// If `base` is a git repo without child git repos, walks up to parent and scans siblings.
/// If `base` is not a git repo, scans its immediate children.
pub fn discover_projects(base: &Path) -> Result<Vec<PathBuf>, AppError> {
    let base_is_repo = base.join(".git").exists();
    let children = child_git_repos(base)?;

    // Base has .git AND child .git repos → it's a parent of repos (e.g., ~/repos that's also a git repo)
    // Base has .git but NO child repos → it's a single project; walk up to scan siblings
    // Base has no .git → it's a plain parent dir; scan children
    let scan_dir = if base_is_repo && children.is_empty() {
        base.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("cannot determine parent of: {}", base.display()),
            )
        })?
    } else {
        base
    };

    let mut projects = child_git_repos(scan_dir)?;
    // Include scan_dir itself if it's a repo (covers the "parent is also a repo" edge case)
    if scan_dir.join(".git").exists() && !projects.contains(&scan_dir.to_path_buf()) {
        projects.push(scan_dir.to_path_buf());
    }
    projects.sort();
    Ok(projects)
}

/// List immediate child directories of `dir` that contain `.git`.
fn child_git_repos(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut repos = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(repos),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join(".git").exists() {
            repos.push(path);
        }
    }
    Ok(repos)
}

/// Ensure the project has a `.waypoint/` directory, returning an error if not.
/// Use for read-only commands (`sketch`, `find`) that can't create it.
pub fn require_waypoint_dir(project_root: &Path) -> Result<PathBuf, AppError> {
    let wp_dir = waypoint_dir(project_root);
    if !wp_dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "no .waypoint/ directory in project: {}",
                project_root.display()
            ),
        )
        .into());
    }
    Ok(wp_dir)
}

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

        assert!(wp.exists());
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

    #[test]
    fn resolve_foreign_finds_waypoint_project() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::create_dir(tmp.path().join(".waypoint")).unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let file = tmp.path().join("src/main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();

        let resolved = resolve_foreign(file.to_str().unwrap()).unwrap();
        assert_eq!(resolved.root, tmp.path());
        assert_eq!(resolved.wp_dir, tmp.path().join(".waypoint"));
        assert_eq!(resolved.relative_path, "src/main.rs");
    }

    #[test]
    fn resolve_foreign_returns_none_without_waypoint() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let file = tmp.path().join("foo.rs");
        std::fs::write(&file, "").unwrap();

        assert!(resolve_foreign(file.to_str().unwrap()).is_none());
    }

    #[test]
    fn resolve_with_context_none_uses_cwd() {
        // Just verifying it doesn't panic — actual root depends on test runner cwd
        let result = resolve_with_context(None);
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_with_context_errors_on_nonexistent() {
        let result = resolve_with_context(Some("/nonexistent/deeply/nested/path"));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_with_context_succeeds_without_waypoint() {
        // resolve_with_context does not require .waypoint/ — callers check separately
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();

        let result = resolve_with_context(Some(tmp.path().to_str().unwrap()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), tmp.path());
    }

    #[test]
    fn require_waypoint_dir_errors_without_waypoint() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();

        let result = require_waypoint_dir(tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(".waypoint"),
            "error should mention .waypoint: {err}"
        );
    }

    #[test]
    fn require_waypoint_dir_succeeds_with_waypoint() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".waypoint")).unwrap();

        let result = require_waypoint_dir(tmp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), tmp.path().join(".waypoint"));
    }

    #[test]
    fn resolve_with_context_resolves_from_file_path() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::create_dir(tmp.path().join(".waypoint")).unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let file = tmp.path().join("src/main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();

        let root = resolve_with_context(Some(file.to_str().unwrap())).unwrap();
        assert_eq!(root, tmp.path());
    }

    /// Helper: create a child git repo under `parent`.
    fn make_child_repo(parent: &Path, name: &str) -> PathBuf {
        let child = parent.join(name);
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir(child.join(".git")).unwrap();
        child
    }

    #[test]
    fn discover_from_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let a = make_child_repo(tmp.path(), "alpha");
        let b = make_child_repo(tmp.path(), "beta");
        // non-repo dir should be ignored
        std::fs::create_dir(tmp.path().join("not-a-repo")).unwrap();

        let projects = discover_projects(tmp.path()).unwrap();
        assert_eq!(projects, vec![a, b]);
    }

    #[test]
    fn discover_from_inside_project_walks_up() {
        let tmp = TempDir::new().unwrap();
        let a = make_child_repo(tmp.path(), "alpha");
        let b = make_child_repo(tmp.path(), "beta");

        // Calling from inside "alpha" should find siblings
        let projects = discover_projects(&a).unwrap();
        assert!(projects.contains(&a));
        assert!(projects.contains(&b));
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn discover_parent_is_also_repo() {
        let tmp = TempDir::new().unwrap();
        // Parent is a git repo AND has child repos
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let a = make_child_repo(tmp.path(), "alpha");
        let b = make_child_repo(tmp.path(), "beta");

        let projects = discover_projects(tmp.path()).unwrap();
        // Should include parent and both children
        assert!(projects.contains(&tmp.path().to_path_buf()));
        assert!(projects.contains(&a));
        assert!(projects.contains(&b));
        assert_eq!(projects.len(), 3);
    }

    #[test]
    fn discover_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let projects = discover_projects(tmp.path()).unwrap();
        assert!(projects.is_empty());
    }
}
