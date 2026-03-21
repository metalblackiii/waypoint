use crate::{AppError, journal, ledger, map, project};

/// FR-7: `SessionStart` — inject journal context and auto-scan.
///
/// `SessionStart` hooks use plain stdout for context injection.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;
    let wp_dir = project::ensure_initialized(&ctx.project_root)?;

    // FR-22: Auto-scan if map.md doesn't exist
    if !wp_dir.join("map.md").exists() {
        let entries = map::scan::scan_project(&ctx.project_root)?;
        map::write_map(&wp_dir, &entries)?;
    }

    // FR-7: Inject journal contents
    let journal_content = journal::read_journal(&wp_dir)?;

    let mut output = String::new();

    if !journal_content.trim().is_empty() {
        output.push_str(&journal_content);
        output.push('\n');
    }

    // FR-10: Invocation prompt for journal
    output.push_str(
        "To log a correction or preference: \
         waypoint journal add --section <preferences|learnings|do-not-repeat> \"<entry>\"\n",
    );

    // FR-15: Invocation prompt for traps
    output.push_str(
        "To log a bug fix: \
         waypoint trap log --error \"<msg>\" --file \"<path>\" \
         --cause \"<root cause>\" --fix \"<what you did>\" --tags \"<comma-separated>\"\n",
    );

    // FR-17: Record session start (silent failure)
    let _ = ledger::record_event(
        ledger::EventKind::SessionStart,
        &ctx.project_root.to_string_lossy(),
        0,
    );

    // FR-19: Purge old ledger events once per session, not per hook
    let _ = ledger::purge_old_events();

    print!("{output}");
    Ok(())
}
