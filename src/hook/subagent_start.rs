use crate::{AppError, ledger};

/// `SubagentStart` — deliver the command digest to Task-tool subagents.
///
/// Each subagent starts with its own fresh, isolated context window and never
/// sees the parent session's `SessionStart` output, so the pointer to the
/// `waypoint` skill in `COMMAND_DIGEST` has to be re-delivered here.
/// This is the fix for the adoption gap identified in the
/// waypoint-guidance-2026-06-01 assessment: subagents skipping `waypoint find`
/// because guidance never reached them, not because the tool lacked value.
///
/// No map rescan and no arch-summary lookup here (unlike `session_start::run`)
/// — those are session-scoped concerns, and subagents can be dispatched
/// frequently/concurrently, so repeating that work per subagent would be
/// wasteful without adding steering value.
pub fn run() -> Result<(), AppError> {
    let ctx = super::HookContext::from_stdin()?;

    let _ = ledger::record_event(
        ledger::EventKind::SubagentStart,
        &ctx.project_root.to_string_lossy(),
        0,
    );

    super::emit_hook_output(
        super::HookEvent::SubagentStart,
        None,
        super::session_start::COMMAND_DIGEST,
    );
    Ok(())
}
