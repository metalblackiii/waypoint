use std::path::Path;

use crate::{AppError, journal, ledger, map, project, trap};

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

    // Journal
    let entry_count = journal::entry_count(&wp_dir)?;
    println!("Journal: {} entries", entry_count);

    // Traps
    let traps = trap::read_traps(&wp_dir)?;
    println!("Traps:   {} logged", traps.len());

    // Ledger (silent failure)
    match ledger::gain_stats(Some(&project_root.to_string_lossy())) {
        Ok(stats) => {
            println!(
                "Ledger:  {} events, {:.0}% map hit rate, ~{} tokens saved",
                stats.total_events, stats.map_hit_rate, stats.estimated_tokens_saved
            );
        }
        Err(e) => {
            println!("Ledger:  unavailable ({e})");
        }
    }

    Ok(())
}

#[cfg(test)]
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
        // journal.md and traps.json exist, but no map.md
        assert!(run(tmp.path()).is_ok());
    }
}
