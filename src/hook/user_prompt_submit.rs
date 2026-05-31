use std::sync::LazyLock;

use regex::Regex;

use crate::AppError;

/// Keyword gate: prompts that change, rename, or assess the reach of code.
/// Stems (not whole words) so `change`/`changing`, `caller`/`calls`,
/// `depend`/`dependents` all match. Case-insensitive. Intentionally broad —
/// the nudge only advises (never forces a command), so a false fire costs one
/// line of context; a miss costs the whole win.
static GATE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)\b(chang|renam|refactor|remov|delet|signature|break|impact|blast radius|affect|depend|call(s|er|ers)?\b)",
    )
    .unwrap()
});

/// v2 nudge (A/B-confirmed 2026-05-31). Frames `callers` as *prune-and-precise*,
/// NOT complete — `callers` is import-graph-only and blind to string-keyed,
/// dynamic, namespace, and same-file references — so it mandates a broad `rg`
/// sweep. The v1 "prefer callers for completeness" wording caused a
/// completeness regression; do not soften this back toward it.
const NUDGE: &str = "[waypoint] This task changes or assesses code reach. Run `waypoint callers <symbol>` for the import-precise set and prune `rg`'s noise — but it is import-graph-only and CANNOT see string-keyed, dynamic, namespace, or same-file references, so ALSO run a broad `rg` for the bare symbol name to catch those. For pre-push blast radius, run `waypoint impact --base <ref>`.";

/// True when the prompt looks like a change/impact task that benefits from
/// `callers`/`impact`. Pure (no I/O) so the gate is unit-testable.
#[must_use]
pub fn should_nudge(prompt: &str) -> bool {
    GATE.is_match(prompt)
}

/// `UserPromptSubmit` — steer toward `waypoint callers`/`impact` on change tasks.
///
/// Reads the `prompt` field from the hook payload on stdin. On a keyword match,
/// injects the v2 advice as `additionalContext`; otherwise stays silent.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let prompt = payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

    let context = if should_nudge(prompt) { NUDGE } else { "" };
    super::emit_hook_output(super::HookEvent::UserPromptSubmit, None, context);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_on_signature_change() {
        assert!(should_nudge("rename getTenant and update its signature"));
    }

    #[test]
    fn fires_on_who_calls_query() {
        assert!(should_nudge("what calls validateProviderFields?"));
    }

    #[test]
    fn fires_on_blast_radius_query() {
        assert!(should_nudge("assess the blast radius of this change"));
    }

    #[test]
    fn fires_on_refactor_and_delete() {
        assert!(should_nudge("refactor the parser"));
        assert!(should_nudge("delete the legacy adapter"));
        assert!(should_nudge("what depends on this module"));
    }

    #[test]
    fn silent_on_neutral_prompts() {
        assert!(!should_nudge("explain this function"));
        assert!(!should_nudge("write me a haiku about the sea"));
        assert!(!should_nudge("summarize the README"));
    }

    #[test]
    fn silent_on_near_miss_wording() {
        // `call` is bounded so compound words don't trip the gate.
        assert!(!should_nudge("wire up a callback handler"));
        assert!(!should_nudge("explain the calling convention"));
        // `\b` keeps stems from matching mid-word.
        assert!(!should_nudge("what are the current exchange rates"));
        // ...but genuine call-graph phrasing still fires.
        assert!(should_nudge("who calls this"));
        assert!(should_nudge("list the callers of parse"));
    }

    #[test]
    fn silent_on_empty_prompt() {
        assert!(!should_nudge(""));
    }

    #[test]
    fn nudge_text_names_both_commands_and_uses_rg() {
        assert!(NUDGE.contains("waypoint callers"));
        assert!(NUDGE.contains("waypoint impact"));
        assert!(NUDGE.contains("`rg`"));
        // v2 must not anchor on callers as complete (the v1 regression).
        assert!(!NUDGE.contains("for completeness"));
    }
}
