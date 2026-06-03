use std::sync::LazyLock;

use regex::Regex;

use crate::AppError;

/// Strong signals: terms that are unambiguously about code reach or structure.
/// These fire the nudge on their own — they essentially never appear in
/// non-code prose. Case-insensitive, stem-friendly.
static STRONG: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)\b(refactor|signature|blast radius|breaking change|caller|callers|who calls|what calls|call graph|call site|dependents)",
    )
    .unwrap()
});

/// Change verbs. These also saturate ordinary prose ("change the title",
/// "climate change", "delete this paragraph"), so alone they over-fire. They
/// only nudge when paired with a code-context signal (`CODE_NOUN` or
/// `CODE_IDENT`). `break` is right-bounded so "breakfast" can't trip it.
static CHANGE_VERB: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(r"(?i)\b(chang|renam|remov|delet|modif|deprecat|impact|affect|depend|break\b)")
        .unwrap()
});

/// Code-context nouns — one alongside a `CHANGE_VERB` means the change is about
/// code, not prose.
static CODE_NOUN: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)\b(fn|def|function|method|signature|symbol|import|export|class|interface|endpoint|api|param|parameter|field|variable|module|const|struct|enum|trait|component|hook|route|schema|column|table|dependenc|caller|selector|reducer|middleware|controller|service|model)\b",
    )
    .unwrap()
});

/// Code-shaped identifiers: backticked tokens, `camelCase`, `PascalCase`,
/// `snake_case`, or a `call(` form. Case-SENSITIVE — the capitalisation IS the
/// signal, so no `(?i)`.
static CODE_IDENT: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"`[^`]+`|\b[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*\b|\b[A-Z][a-z0-9]+[A-Z][a-zA-Z0-9]*\b|\b\w+_\w+\b|\b[a-zA-Z_]\w*\(",
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
    STRONG.is_match(prompt)
        || (CHANGE_VERB.is_match(prompt)
            && (CODE_NOUN.is_match(prompt) || CODE_IDENT.is_match(prompt)))
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
    fn fires_on_strong_signals_alone() {
        // Strong terms fire with no code-context token required.
        assert!(should_nudge("refactor the parser"));
        assert!(should_nudge("what depends on this module"));
    }

    #[test]
    fn fires_on_change_verb_plus_code_context() {
        // verb + camelCase identifier
        assert!(should_nudge("rename getTenant to resolveTenant"));
        // verb + code noun + camelCase
        assert!(should_nudge("remove the validateUser function"));
        // verb + code noun
        assert!(should_nudge("modify the user schema"));
        // verb + backticked identifier + noun
        assert!(should_nudge("change the `MAX_RETRIES` const"));
    }

    #[test]
    fn silent_on_change_verbs_in_prose() {
        // The over-trigger class this gate exists to kill: change verbs with
        // no code-context signal nearby.
        assert!(!should_nudge("change the button color to blue"));
        assert!(!should_nudge("delete this paragraph from the doc"));
        assert!(!should_nudge("summarize this article about climate change"));
        assert!(!should_nudge("break this into smaller sentences"));
    }

    #[test]
    fn silent_on_neutral_prompts() {
        assert!(!should_nudge("explain this function"));
        assert!(!should_nudge("write me a haiku about the sea"));
        assert!(!should_nudge("summarize the README"));
    }

    #[test]
    fn silent_on_near_miss_wording() {
        // `caller(s)` is bounded so compound words don't trip the strong gate.
        assert!(!should_nudge("wire up a callback handler"));
        assert!(!should_nudge("explain the calling convention"));
        // stems are bounded so they don't match mid-word.
        assert!(!should_nudge("what are the current exchange rates"));
        assert!(!should_nudge("make breakfast for the team"));
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
