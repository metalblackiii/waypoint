use std::path::Path;

use crate::AppError;
use crate::cli::JournalSection;

/// Return the initial journal.md content.
pub fn empty_journal() -> String {
    "# Waypoint Journal\n\n## Preferences\n\n## Learnings\n\n## Do-Not-Repeat\n".to_string()
}

/// Read the full journal.md content.
pub fn read_journal(waypoint_dir: &Path) -> Result<String, AppError> {
    let path = waypoint_dir.join("journal.md");
    if !path.exists() {
        return Ok(empty_journal());
    }
    Ok(std::fs::read_to_string(&path)?)
}

/// Add a timestamped entry to the specified section.
pub fn add_entry(
    waypoint_dir: &Path,
    section: JournalSection,
    entry: &str,
) -> Result<(), AppError> {
    let path = waypoint_dir.join("journal.md");
    let content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        empty_journal()
    };

    let section_header = match section {
        JournalSection::Preferences => "## Preferences",
        JournalSection::Learnings => "## Learnings",
        JournalSection::DoNotRepeat => "## Do-Not-Repeat",
    };

    let date = chrono::Utc::now().format("%Y-%m-%d");
    let new_entry = format!("- {date}: {entry}");

    let updated = insert_after_section(&content, section_header, &new_entry);

    let tmp_path = waypoint_dir.join("journal.md.tmp");
    std::fs::write(&tmp_path, &updated)?;
    std::fs::rename(&tmp_path, &path)?;

    Ok(())
}

/// Insert a line after the given section header.
fn insert_after_section(content: &str, header: &str, entry: &str) -> String {
    let mut result = String::with_capacity(content.len() + entry.len() + 2);
    let mut found = false;

    for line in content.lines() {
        result.push_str(line);
        result.push('\n');

        if !found && line.trim() == header {
            found = true;
            result.push_str(entry);
            result.push('\n');
        }
    }

    if !found {
        result.push('\n');
        result.push_str(header);
        result.push('\n');
        result.push_str(entry);
        result.push('\n');
    }

    result
}

/// Count entries across all sections.
pub fn entry_count(waypoint_dir: &Path) -> Result<usize, AppError> {
    let content = read_journal(waypoint_dir)?;
    Ok(content.lines().filter(|l| l.starts_with("- ")).count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn add_entry_to_preferences() {
        let tmp = TempDir::new().unwrap();
        let wp = tmp.path();
        std::fs::write(wp.join("journal.md"), empty_journal()).unwrap();

        add_entry(wp, JournalSection::Preferences, "Use snake_case").unwrap();

        let content = std::fs::read_to_string(wp.join("journal.md")).unwrap();
        assert!(content.contains("Use snake_case"), "got:\n{content}");
        assert_eq!(entry_count(wp).unwrap(), 1);
    }

    #[test]
    fn add_entry_to_do_not_repeat() {
        let tmp = TempDir::new().unwrap();
        let wp = tmp.path();
        std::fs::write(wp.join("journal.md"), empty_journal()).unwrap();

        add_entry(wp, JournalSection::DoNotRepeat, "Never use var").unwrap();

        let content = std::fs::read_to_string(wp.join("journal.md")).unwrap();
        assert!(content.contains("Never use var"), "got:\n{content}");

        // Verify entry appears after Do-Not-Repeat header, not after Preferences
        let dnr_pos = content.find("## Do-Not-Repeat").unwrap();
        let entry_pos = content.find("Never use var").unwrap();
        assert!(entry_pos > dnr_pos);
    }

    #[test]
    fn entry_count_empty() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("journal.md"), empty_journal()).unwrap();
        assert_eq!(entry_count(tmp.path()).unwrap(), 0);
    }
}
