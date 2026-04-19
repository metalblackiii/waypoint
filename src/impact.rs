use std::path::Path;
use std::process::Command;

use crate::AppError;
use crate::map::index;

/// Risk classification thresholds based on importer count.
fn classify_risk(importer_count: i64) -> &'static str {
    match importer_count {
        n if n >= 10 => "CRITICAL",
        5..=9 => "HIGH",
        2..=4 => "MEDIUM",
        _ => "LOW",
    }
}

/// A symbol affected by the diff.
struct AffectedSymbol {
    kind: String,
    name: String,
    file_path: String,
    line_start: i64,
    exported: bool,
    importer_count: i64,
    risk: &'static str,
}

/// A changed file with its modified line ranges.
struct ChangedFile {
    path: String,
    ranges: Vec<(i64, i64)>,
}

/// Where to source the diff from.
enum DiffSource {
    /// Uncommitted changes (working tree + staged vs HEAD).
    Uncommitted,
    /// Branch diff against a base ref.
    Branch(String),
}

/// Run impact analysis: map git diffs to affected symbols and importer blast radius.
pub fn run(project_root: &Path, wp_dir: &Path, base: Option<&str>) -> Result<(), AppError> {
    // FR-24: Non-git graceful exit
    if !project_root.join(".git").exists() {
        eprintln!("Not a git repository.");
        std::process::exit(1);
    }

    // FR-23: Stale map warning
    check_staleness(wp_dir, project_root);

    let diff_source = detect_diff_source(project_root, base)?;
    let diff_output = run_git_diff(project_root, &diff_source)?;
    let changed_files = parse_diff_hunks(&diff_output);

    // git diff HEAD omits untracked files entirely — surface them separately
    let untracked = if matches!(diff_source, DiffSource::Uncommitted) {
        collect_untracked(project_root)?
    } else {
        Vec::new()
    };

    if changed_files.is_empty() {
        if untracked.is_empty() {
            println!("No symbol changes detected.");
        } else {
            println!("No tracked symbol changes detected.");
        }
        for path in &untracked {
            println!(
                "  New (untracked): {path} — run `git add` and `waypoint scan` for symbol detail"
            );
        }
        return Ok(());
    }

    let mut affected: Vec<AffectedSymbol> = Vec::new();
    let mut affected_files: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for changed in &changed_files {
        let symbols = index::find_symbols_in_ranges(wp_dir, &changed.path, &changed.ranges)?;
        for sym in symbols {
            let importer_count = if sym.exported {
                let importers = index::find_importers(wp_dir, &sym.name, Some(&sym.file_path))?;
                for (file, _) in &importers {
                    affected_files.insert(file.clone());
                }
                #[allow(clippy::cast_possible_wrap)]
                let count = importers.len() as i64;
                count
            } else {
                0
            };
            let risk = classify_risk(importer_count);
            affected.push(AffectedSymbol {
                kind: sym.kind,
                name: sym.name,
                file_path: sym.file_path,
                line_start: sym.line_start,
                exported: sym.exported,
                importer_count,
                risk,
            });
        }
    }

    // FR-22: No-change exit
    if affected.is_empty() {
        if untracked.is_empty() {
            println!("No symbol changes detected.");
        } else {
            println!("No tracked symbol changes detected.");
        }
        for path in &untracked {
            println!(
                "  New (untracked): {path} — run `git add` and `waypoint scan` for symbol detail"
            );
        }
        return Ok(());
    }

    // Sort by importer count descending (highest impact first)
    affected.sort_by_key(|b| std::cmp::Reverse(b.importer_count));

    for sym in &affected {
        let export_marker = if sym.exported { "" } else { " (private)" };
        println!(
            "Changed: {} {}{} ({}:{}) — {} importers | Risk: {}",
            sym.kind,
            sym.name,
            export_marker,
            sym.file_path,
            sym.line_start,
            sym.importer_count,
            sym.risk,
        );
    }

    if !affected_files.is_empty() {
        println!("\nAffected files (deduplicated):");
        for file in &affected_files {
            println!("  {file}");
        }
    }

    if !untracked.is_empty() {
        println!(
            "\nUntracked new files (not in diff — run `git add` and `waypoint scan` for symbol detail):"
        );
        for path in &untracked {
            println!("  New: {path}");
        }
    }

    Ok(())
}

/// Collect untracked files unknown to git. Used to surface new additions that
/// `git diff HEAD` omits entirely.
fn collect_untracked(project_root: &Path) -> Result<Vec<String>, AppError> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(project_root)
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().map(str::to_owned).collect())
}

/// FR-23: Warn if index is older than the most recent commit.
fn check_staleness(wp_dir: &Path, project_root: &Path) {
    let Some(index_mtime) = index::index_mtime(wp_dir) else {
        return;
    };
    let output = Command::new("git")
        .arg("log")
        .arg("-1")
        .arg("--format=%ct")
        .current_dir(project_root)
        .output();
    let Ok(output) = output else { return };
    let timestamp_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let Ok(commit_epoch) = timestamp_str.parse::<u64>() else {
        return;
    };
    let commit_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(commit_epoch);
    if index_mtime < commit_time {
        eprintln!("⚠ Map may be stale — run `waypoint scan` for accurate results.");
    }
}

/// FR-15/FR-16: Determine diff source.
fn detect_diff_source(project_root: &Path, base: Option<&str>) -> Result<DiffSource, AppError> {
    // FR-16: Explicit base override
    if let Some(base_ref) = base {
        return Ok(DiffSource::Branch(base_ref.to_string()));
    }

    // FR-15: Check for uncommitted changes
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_root)
        .output()?;
    let status_text = String::from_utf8_lossy(&status.stdout);
    if !status_text.trim().is_empty() {
        return Ok(DiffSource::Uncommitted);
    }

    // No uncommitted changes — diff current branch against default branch
    let default_branch = detect_default_branch(project_root)?;
    Ok(DiffSource::Branch(default_branch))
}

/// Detect the default branch: origin/HEAD → main → master → error.
fn detect_default_branch(project_root: &Path) -> Result<String, AppError> {
    // Try origin/HEAD
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(project_root)
        .output()?;
    if output.status.success() {
        let full_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // refs/remotes/origin/main → origin/main
        if let Some(branch) = full_ref.strip_prefix("refs/remotes/") {
            return Ok(branch.to_string());
        }
    }

    // Try main
    let check_main = Command::new("git")
        .args(["rev-parse", "--verify", "origin/main"])
        .current_dir(project_root)
        .output()?;
    if check_main.status.success() {
        return Ok("origin/main".to_string());
    }

    // Try origin/master
    let check_master = Command::new("git")
        .args(["rev-parse", "--verify", "origin/master"])
        .current_dir(project_root)
        .output()?;
    if check_master.status.success() {
        return Ok("origin/master".to_string());
    }

    // Try configured default branch name (git config init.defaultBranch)
    let configured = Command::new("git")
        .args(["config", "init.defaultBranch"])
        .current_dir(project_root)
        .output()?;
    if configured.status.success() {
        let branch_name = String::from_utf8_lossy(&configured.stdout)
            .trim()
            .to_string();
        if !branch_name.is_empty() {
            let check = Command::new("git")
                .args(["rev-parse", "--verify", &branch_name])
                .current_dir(project_root)
                .output()?;
            if check.status.success() {
                return Ok(branch_name);
            }
        }
    }

    // Try local main (no remote, no config)
    let check_local_main = Command::new("git")
        .args(["rev-parse", "--verify", "main"])
        .current_dir(project_root)
        .output()?;
    if check_local_main.status.success() {
        return Ok("main".to_string());
    }

    // Try local master (no remote, no config)
    let check_local_master = Command::new("git")
        .args(["rev-parse", "--verify", "master"])
        .current_dir(project_root)
        .output()?;
    if check_local_master.status.success() {
        return Ok("master".to_string());
    }

    Err(AppError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Cannot detect default branch. Use --base <ref> to specify.",
    )))
}

/// Run git diff and return raw output.
fn run_git_diff(project_root: &Path, source: &DiffSource) -> Result<String, AppError> {
    let output = match source {
        DiffSource::Uncommitted => Command::new("git")
            .args(["diff", "--unified=0", "--no-color", "HEAD"])
            .current_dir(project_root)
            .output()?,
        DiffSource::Branch(base) => Command::new("git")
            .args(["diff", "--unified=0", "--no-color"])
            .arg(format!("{base}...HEAD"))
            .current_dir(project_root)
            .output()?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Io(std::io::Error::other(format!(
            "git diff failed: {stderr}"
        ))));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse unified diff output into changed files with line ranges.
///
/// Handles three special cases beyond normal modifications:
/// - **Deletions** (`+++ /dev/null`): tracks under old path with old-side ranges.
/// - **Renames** (`--- a/old` + `+++ b/new`): emits both new path (new-side ranges)
///   and old path (old-side ranges) so deleted symbols remain findable in a stale index.
/// - **Additions** (`--- /dev/null` + `+++ b/new`): captured normally via new-side ranges.
fn parse_diff_hunks(diff: &str) -> Vec<ChangedFile> {
    let mut files: Vec<ChangedFile> = Vec::new();
    let mut current_path: Option<String> = None;
    let mut rename_old_path: Option<String> = None;
    let mut old_path: Option<String> = None;
    let mut current_ranges: Vec<(i64, i64)> = Vec::new();
    let mut old_side_ranges: Vec<(i64, i64)> = Vec::new();

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("--- a/") {
            old_path = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("+++ b/") {
            // Flush previous new-path file and any pending rename old-path file
            if let Some(path) = current_path.take()
                && !current_ranges.is_empty()
            {
                files.push(ChangedFile {
                    path,
                    ranges: std::mem::take(&mut current_ranges),
                });
            } else {
                current_ranges.clear();
            }
            if let Some(old) = rename_old_path.take()
                && !old_side_ranges.is_empty()
            {
                files.push(ChangedFile {
                    path: old,
                    ranges: std::mem::take(&mut old_side_ranges),
                });
            } else {
                old_side_ranges.clear();
            }
            let new_path = rest.to_string();
            if let Some(old) = old_path.take()
                && old != new_path
            {
                rename_old_path = Some(old);
            }
            current_path = Some(new_path);
        } else if line == "+++ /dev/null" {
            // Deleted file — flush previous file(s) exactly as +++ b/ does, then use old path
            if let Some(path) = current_path.take()
                && !current_ranges.is_empty()
            {
                files.push(ChangedFile {
                    path,
                    ranges: std::mem::take(&mut current_ranges),
                });
            } else {
                current_ranges.clear();
            }
            if let Some(old) = rename_old_path.take()
                && !old_side_ranges.is_empty()
            {
                files.push(ChangedFile {
                    path: old,
                    ranges: std::mem::take(&mut old_side_ranges),
                });
            } else {
                old_side_ranges.clear();
            }
            current_path = old_path.take();
        } else if line.starts_with("@@ ") {
            if let Some(range) = parse_hunk_header(line) {
                current_ranges.push(range);
            }
            if let Some(range) = parse_hunk_old_side(line) {
                old_side_ranges.push(range);
            }
        }
    }

    // Flush last file(s)
    if let Some(path) = current_path
        && !current_ranges.is_empty()
    {
        files.push(ChangedFile {
            path,
            ranges: current_ranges,
        });
    }
    if let Some(old) = rename_old_path
        && !old_side_ranges.is_empty()
    {
        files.push(ChangedFile {
            path: old,
            ranges: old_side_ranges,
        });
    }

    files
}

/// Parse the old-side line range from a unified diff hunk header.
/// Used alongside `parse_hunk_header` for rename tracking.
fn parse_hunk_old_side(line: &str) -> Option<(i64, i64)> {
    let minus_pos = line.find('-')?;
    let old_rest = &line[minus_pos + 1..];
    let old_end = old_rest.find(' ').unwrap_or(old_rest.len());
    let old_range = &old_rest[..old_end];

    let (old_start, old_count) = if let Some((s, c)) = old_range.split_once(',') {
        (s.parse::<i64>().ok()?, c.parse::<i64>().ok()?)
    } else {
        (old_range.parse::<i64>().ok()?, 1)
    };

    if old_count > 0 {
        Some((old_start, old_start + old_count - 1))
    } else {
        None
    }
}

/// Parse a hunk header line and extract the relevant line range.
/// Format: `@@ -old_start[,old_count] +new_start[,new_count] @@`
///
/// Returns the new-side range for additions/modifications. For pure deletions
/// (`new_count == 0`), falls back to the old-side range so callers can still
/// query deleted symbols from a stale index.
fn parse_hunk_header(line: &str) -> Option<(i64, i64)> {
    // Parse new-side: +new_start[,new_count]
    let plus_pos = line.find('+')?;
    let new_rest = &line[plus_pos + 1..];
    let new_end = new_rest.find(' ').unwrap_or(new_rest.len());
    let new_range = &new_rest[..new_end];

    let (new_start, new_count) = if let Some((s, c)) = new_range.split_once(',') {
        (s.parse::<i64>().ok()?, c.parse::<i64>().ok()?)
    } else {
        (new_range.parse::<i64>().ok()?, 1)
    };

    if new_count > 0 {
        return Some((new_start, new_start + new_count - 1));
    }

    // Pure deletion: fall back to old-side range to locate deleted symbols in the index
    let minus_pos = line.find('-')?;
    let old_rest = &line[minus_pos + 1..];
    let old_end = old_rest.find(' ').unwrap_or(old_rest.len());
    let old_range = &old_rest[..old_end];

    let (old_start, old_count) = if let Some((s, c)) = old_range.split_once(',') {
        (s.parse::<i64>().ok()?, c.parse::<i64>().ok()?)
    } else {
        (old_range.parse::<i64>().ok()?, 1)
    };

    if old_count > 0 {
        Some((old_start, old_start + old_count - 1))
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn classify_risk_thresholds() {
        assert_eq!(classify_risk(0), "LOW");
        assert_eq!(classify_risk(1), "LOW");
        assert_eq!(classify_risk(2), "MEDIUM");
        assert_eq!(classify_risk(4), "MEDIUM");
        assert_eq!(classify_risk(5), "HIGH");
        assert_eq!(classify_risk(9), "HIGH");
        assert_eq!(classify_risk(10), "CRITICAL");
        assert_eq!(classify_risk(50), "CRITICAL");
    }

    #[test]
    fn parse_hunk_header_single_line() {
        assert_eq!(parse_hunk_header("@@ -10 +15 @@"), Some((15, 15)));
    }

    #[test]
    fn parse_hunk_header_range() {
        assert_eq!(
            parse_hunk_header("@@ -10,5 +20,3 @@ fn foo()"),
            Some((20, 22))
        );
    }

    #[test]
    fn parse_hunk_header_deletion_only() {
        // Pure deletion: falls back to old-side range so deleted symbols can be queried
        assert_eq!(parse_hunk_header("@@ -10,3 +9,0 @@"), Some((10, 12)));
    }

    #[test]
    fn parse_hunk_header_both_zero() {
        // Both sides zero is a degenerate hunk — skip it
        assert_eq!(parse_hunk_header("@@ -0,0 +0,0 @@"), None);
    }

    #[test]
    fn parse_diff_hunks_deleted_file() {
        let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
--- a/src/old.rs
+++ /dev/null
@@ -5,3 +4,0 @@
-removed line";
        let files = parse_diff_hunks(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/old.rs");
        assert_eq!(files[0].ranges, vec![(5, 7)]);
    }

    #[test]
    fn parse_diff_hunks_multiple_files() {
        let diff = "\
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -10,3 +10,5 @@ fn old()
+new line
diff --git a/src/bar.rs b/src/bar.rs
--- a/src/bar.rs
+++ b/src/bar.rs
@@ -1 +1,2 @@
+another line
@@ -20,2 +21,3 @@
+more lines";

        let files = parse_diff_hunks(diff);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/foo.rs");
        assert_eq!(files[0].ranges, vec![(10, 14)]);
        assert_eq!(files[1].path, "src/bar.rs");
        assert_eq!(files[1].ranges, vec![(1, 2), (21, 23)]);
    }

    #[test]
    fn parse_diff_hunks_empty() {
        assert!(parse_diff_hunks("").is_empty());
    }

    #[test]
    fn parse_diff_hunks_rename_then_deletion() {
        // Rename followed by a deletion must not drop the rename's old-path entry
        let diff = "\
diff --git a/src/old.rs b/src/new.rs
--- a/src/old.rs
+++ b/src/new.rs
@@ -5,2 +5,3 @@
+added
diff --git a/src/gone.rs b/src/gone.rs
deleted file mode 100644
--- a/src/gone.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-removed";
        let files = parse_diff_hunks(diff);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"src/new.rs"), "new path missing: {paths:?}");
        assert!(
            paths.contains(&"src/old.rs"),
            "rename old path missing: {paths:?}"
        );
        assert!(
            paths.contains(&"src/gone.rs"),
            "deleted file missing: {paths:?}"
        );
    }

    #[test]
    fn parse_diff_hunks_rename_with_changes() {
        let diff = "\
diff --git a/src/old.rs b/src/new.rs
similarity index 80%
rename from src/old.rs
rename to src/new.rs
--- a/src/old.rs
+++ b/src/new.rs
@@ -10,2 +12,3 @@ fn foo()
+added line";
        let files = parse_diff_hunks(diff);
        // New path gets new-side range; old path gets old-side range
        assert_eq!(files.len(), 2);
        let new_file = files.iter().find(|f| f.path == "src/new.rs").unwrap();
        assert_eq!(new_file.ranges, vec![(12, 14)]);
        let old_file = files.iter().find(|f| f.path == "src/old.rs").unwrap();
        assert_eq!(old_file.ranges, vec![(10, 11)]);
    }
}
