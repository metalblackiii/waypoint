use std::path::Path;

use colored::Colorize;

use crate::{AppError, ledger, map, project, trap};

pub fn run(project_root: &Path) -> Result<(), AppError> {
    let wp_dir = project::waypoint_dir(project_root);

    if !wp_dir.exists() {
        println!(
            "No .waypoint/ directory found at {}",
            project_root.display()
        );
        println!("Start a Claude Code session to auto-initialize, or run: waypoint scan");
        return Ok(());
    }

    println!("Waypoint status: {}\n", project_root.display());

    // Map
    let map_path = wp_dir.join("map.md");
    if map_path.exists() {
        let entries = map::read_map(&wp_dir)?;
        let metadata = std::fs::metadata(&map_path)?;
        let modified: chrono::DateTime<chrono::Utc> = metadata.modified()?.into();
        println!(
            "Map:     {} files (last scan: {})",
            entries.len(),
            modified.format("%Y-%m-%d %H:%M")
        );
    } else {
        println!("Map:     not generated (run: waypoint scan)");
    }

    // Traps
    let traps = trap::read_traps(&wp_dir)?;
    println!("Traps:   {} logged", traps.len());

    // Ledger (silent failure)
    match ledger::gain_stats(Some(&project_root.to_string_lossy())) {
        Ok(stats) => {
            println!("Ledger:  {}", stats.summary_line());
        }
        Err(e) => {
            println!("Ledger:  unavailable ({e})");
        }
    }

    Ok(())
}

pub fn run_all(base: &Path) -> Result<(), AppError> {
    let projects = project::discover_projects(base)?;
    if projects.is_empty() {
        println!("No git repos found under {}", base.display());
        return Ok(());
    }

    println!("Waypoint status: {} projects\n", projects.len());

    let name_width = projects
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().len())
        .max()
        .unwrap_or(10)
        .max(10);

    let now = chrono::Utc::now();

    for root in &projects {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let wp_dir = project::waypoint_dir(root);

        if !wp_dir.exists() {
            println!("  {} {name:<name_width$}  no .waypoint/", "-".dimmed());
            continue;
        }

        let trap_display = match trap::read_traps(&wp_dir) {
            Ok(traps) => traps.len().to_string(),
            Err(_) => "?".to_string(),
        };

        match map::parse_map_header(&wp_dir) {
            None => {
                println!(
                    "  {} {name:<name_width$}  map: not generated  traps: {trap_display}",
                    "?".yellow()
                );
            }
            Some(header) => {
                let age_days = (now - header.generated_at).num_days();
                let stale = age_days >= map::MAP_STALE_DAYS;
                let age_str = if age_days == 0 {
                    "today".to_string()
                } else {
                    format!("{age_days}d ago")
                };
                let indicator = if stale { "!".yellow() } else { "✓".green() };
                let stale_tag = if stale {
                    format!("  {}", "[stale]".yellow())
                } else {
                    String::new()
                };
                println!(
                    "  {indicator} {name:<name_width$}  map: {} files ({age_str:<8})  traps: {trap_display}{stale_tag}",
                    header.file_count,
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn no_waypoint_dir_succeeds() {
        let tmp = TempDir::new().unwrap();
        // No .waypoint/ directory — should print guidance and return Ok
        assert!(run(tmp.path()).is_ok());
    }

    #[test]
    fn initialized_project_succeeds() {
        let tmp = TempDir::new().unwrap();
        let wp = project::ensure_initialized(tmp.path()).unwrap();

        // Write a minimal map.md
        std::fs::write(
            wp.join("map.md"),
            "# Waypoint Map\n\n## src\n\n- `main.rs` — fn main() (~45 tok)\n",
        )
        .unwrap();

        assert!(run(tmp.path()).is_ok());
    }

    #[test]
    fn no_map_file_succeeds() {
        let tmp = TempDir::new().unwrap();
        project::ensure_initialized(tmp.path()).unwrap();
        // .waypoint/ exists but no map.md
        assert!(run(tmp.path()).is_ok());
    }

    #[test]
    fn run_all_no_repos_succeeds() {
        let tmp = TempDir::new().unwrap();
        // No child .git dirs — discover_projects returns empty list
        assert!(run_all(tmp.path()).is_ok());
    }

    #[test]
    fn run_all_project_without_waypoint() {
        let parent = TempDir::new().unwrap();
        std::fs::create_dir_all(parent.path().join("repo-a/.git")).unwrap();
        // Child has .git but no .waypoint/ — should print "no .waypoint/" line
        assert!(run_all(parent.path()).is_ok());
    }

    #[test]
    fn run_all_project_with_waypoint() {
        let parent = TempDir::new().unwrap();
        let child = parent.path().join("repo-a");
        std::fs::create_dir_all(child.join(".git")).unwrap();
        project::ensure_initialized(&child).unwrap();
        // Child has .waypoint/ but no map.md — should print "map: not generated"
        assert!(run_all(parent.path()).is_ok());
    }
}
