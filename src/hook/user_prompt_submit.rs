//! `UserPromptSubmit` — steer toward the right waypoint command for the prompt.
//!
//! Two-stage design (the "graduated A→B" nudge):
//!   A. Classify the prompt's intent (`callers` / `sketch` / `find` / `ask`) with
//!      pure regex tiers and emit advice naming the command.
//!   B. When a concrete argument is extractable with confidence, actually run the
//!      command in-process and inject its output inline — so the agent gets the
//!      answer, not just a suggestion. Injection always falls back to advice on a
//!      miss, a missing `.waypoint` index, or a failed size gate; the hook never
//!      errors (a crashing hook would block the user's prompt).

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::AppError;

/// Strong signals: terms that are unambiguously about code reach or structure.
/// These fire the `callers` nudge on their own — they essentially never appear
/// in non-code prose. Case-insensitive, stem-friendly.
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

/// `sketch` intent — "show me the shape/structure of a named symbol".
static SKETCH_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)\b(structure of|shape of|outline of|signature of|show me the (shape|structure|signature)|what'?s in|where is .* (defined|declared)|which file (defines|declares))\b",
    )
    .unwrap()
});

/// `find` intent — "locate a symbol by name/fragment". Bare `find` is included
/// (not just `find the`) because `find getFoo` is a high-probability prompt
/// shape; over-firing on prose `find` is harmless since injection needs a token.
static FIND_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)\b(where is|where'?s|find|locate|which file (has|contains)|definition of|look up)\b",
    )
    .unwrap()
});

/// `ask` intent — "where do I start / how does this work" orientation.
static ASK_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"(?i)(how (does|do|is|are).{0,40}\bwork|where (do|should) i (start|begin|look)|where to start|walk me through|what handles|which files? (are )?relevant)",
    )
    .unwrap()
});

/// A single backticked token, captured for symbol extraction.
static BACKTICK: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(r"`([^`]+)`").unwrap()
});

/// Code-identifier tokens for extraction: `camelCase`, multi-cap `PascalCase`,
/// or `snake_case`. The strongest symbol signal. Plain lowercase words (e.g. a
/// single-word `fn classify`) are handled separately as a content-word fallback.
static IDENT_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(
        r"\b[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*\b|\b[A-Z][a-z0-9]+[A-Z][a-zA-Z0-9]*\b|\b[a-z]+_[a-z0-9_]+\b",
    )
    .unwrap()
});

/// Any word-shaped token, for the lowercase content-word fallback.
static WORD: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern, validated by unit tests
    Regex::new(r"[A-Za-z][A-Za-z0-9_]*").unwrap()
});

/// Framing vocabulary: stopwords plus the intent-trigger and meta-descriptor
/// lexicon (`find`, `where`, `structure`, `function`, `file`, …). Stripped before
/// the lowercase content-word fallback so that a prompt like "where is classify"
/// or "the parse function" leaves exactly one content word to query on. Domain
/// nouns (`model`, `service`, `controller`) are deliberately NOT framing — they
/// are often the symbol itself, so prompts naming two of them stay ambiguous.
static FRAMING: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "the",
        "and",
        "or",
        "are",
        "was",
        "were",
        "be",
        "this",
        "that",
        "its",
        "for",
        "from",
        "with",
        "into",
        "your",
        "our",
        "their",
        "you",
        "we",
        "us",
        "can",
        "could",
        "should",
        "would",
        "please",
        "just",
        "need",
        "want",
        "about",
        "find",
        "locate",
        "look",
        "where",
        "what",
        "which",
        "who",
        "how",
        "show",
        "tell",
        "give",
        "get",
        "see",
        "structure",
        "shape",
        "outline",
        "signature",
        "definition",
        "define",
        "defined",
        "declare",
        "declared",
        "declaration",
        "module",
        "file",
        "function",
        "method",
        "symbol",
        "class",
        "code",
        "name",
        "named",
        "called",
        "start",
        "begin",
        "walk",
        "through",
        "handles",
        "handle",
        "relevant",
        "help",
        "helper",
        "thing",
        "stuff",
    ]
    .into_iter()
    .collect()
});

/// v2 `callers`/`impact` nudge (A/B-confirmed 2026-05-31). Frames `callers` as
/// *prune-and-precise*, NOT complete — `callers` is import-graph-only and blind
/// to string-keyed, dynamic, namespace, and same-file references — so it mandates
/// a broad `rg` sweep. Do not soften back toward the v1 "for completeness" wording.
const NUDGE: &str = "[waypoint] This task changes or assesses code reach. Run `waypoint callers <symbol>` for the import-precise set and prune `rg`'s noise — but it is import-graph-only and CANNOT see string-keyed, dynamic, namespace, or same-file references, so ALSO run a broad `rg` for the bare symbol name to catch those. For pre-push blast radius, run `waypoint impact --base <ref>`.";

/// The same blindness caveat, appended to injected `find` results.
const GREP_CAVEAT: &str = "Note: the symbol graph is import/definition-precise but blind to string-keyed, dynamic, namespace, and same-file references — ALSO run a broad `rg` for the bare name to catch those.";

/// Caps on injected output: keep the context cheap and skimmable.
const MAX_LINES: usize = 12;
const MAX_CHARS: usize = 1500;

/// Sketch size gate: only inject when reading the target raw would be costly.
/// A symbol spanning more than this many lines, OR living in a file larger than
/// `SKETCH_MIN_BYTES`, is worth pinpointing; a small target is cheaper to just read.
const SKETCH_MIN_SPAN: i64 = 60;
const SKETCH_MIN_BYTES: u64 = 8_000; // ~200 lines

/// Which waypoint command a prompt is steering toward, if any.
/// Priority on multi-match is the declaration order below: `Callers` (highest-
/// precision existing tier) wins, `Ask` (loosest) loses every tie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Callers,
    Sketch,
    Find,
    Ask,
}

/// Classify a prompt's command intent. Pure (no I/O) so the gate is unit-testable.
#[must_use]
pub fn classify(prompt: &str) -> Option<Intent> {
    if is_callers_intent(prompt) {
        Some(Intent::Callers)
    } else if SKETCH_RE.is_match(prompt) {
        Some(Intent::Sketch)
    } else if FIND_RE.is_match(prompt) {
        Some(Intent::Find)
    } else if ASK_RE.is_match(prompt) {
        Some(Intent::Ask)
    } else {
        None
    }
}

fn is_callers_intent(prompt: &str) -> bool {
    STRONG.is_match(prompt)
        || (CHANGE_VERB.is_match(prompt)
            && (CODE_NOUN.is_match(prompt) || CODE_IDENT.is_match(prompt)))
}

/// True when the prompt is a change/impact task that benefits from
/// `callers`/`impact`. Retained for the original callers nudge tests.
#[must_use]
pub fn should_nudge(prompt: &str) -> bool {
    matches!(classify(prompt), Some(Intent::Callers))
}

/// Extract a single, confident symbol argument from the prompt — the confidence
/// gate for data injection. Tiered by confidence:
///
/// 1. a backticked identifier wins outright;
/// 2. else exactly one code-shaped token (`camelCase`/`PascalCase`/`snake_case`);
/// 3. else exactly one lowercase content word after stripping framing vocab —
///    this is what lets `where is classify` / `the parse function` resolve.
///
/// More than one candidate at any tier → `None` (ambiguous → advice). A wrong
/// guess is cheap: `find`/`sketch` self-validate (no match → advice fallback).
#[must_use]
pub fn extract_symbol(prompt: &str) -> Option<String> {
    if let Some(inner) = BACKTICK.captures(prompt).and_then(|c| c.get(1)) {
        let token = inner.as_str().trim();
        let is_identifier =
            !token.is_empty() && token.chars().all(|ch| ch.is_alphanumeric() || ch == '_');
        if is_identifier {
            return Some(token.to_string());
        }
    }

    let code: BTreeSet<&str> = IDENT_TOKEN.find_iter(prompt).map(|m| m.as_str()).collect();
    if !code.is_empty() {
        // Any code-shaped token present: inject only if it is unambiguously the
        // one. More than one distinct code token → ambiguous → advice.
        return (code.len() == 1)
            .then(|| code.into_iter().next().map(str::to_string))
            .flatten();
    }

    let content: BTreeSet<&str> = WORD
        .find_iter(prompt)
        .map(|m| m.as_str())
        .filter(|w| w.len() >= 3 && !FRAMING.contains(w.to_lowercase().as_str()))
        .collect();
    if content.len() == 1 {
        return content.into_iter().next().map(str::to_string);
    }
    None
}

/// True when a sketch target is expensive enough to read raw that pinpointing it
/// pays. Pure (takes the span and file size as args) so it is unit-testable.
#[must_use]
pub fn sketch_pays(span_lines: i64, file_bytes: u64) -> bool {
    span_lines > SKETCH_MIN_SPAN || file_bytes > SKETCH_MIN_BYTES
}

/// Truncate an injected block to at most `max_lines` lines and `max_chars`
/// characters, appending a marker when anything is dropped.
#[must_use]
pub fn cap_block(body: &str, max_lines: usize, max_chars: usize) -> String {
    let mut out = String::new();
    let mut truncated = false;
    for (i, line) in body.lines().enumerate() {
        if i >= max_lines || out.len() + line.len() + 1 > max_chars {
            truncated = true;
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    if truncated {
        out.push_str("\n  … (truncated; run the command for the full list)");
    }
    out
}

/// Resolve `(project_root, waypoint_dir)` from the hook payload's cwd. Returns
/// `None` when there is no project root or no `.waypoint` index — in which case
/// the caller falls back to advice rather than injection.
fn resolve_paths(payload: &serde_json::Value) -> Option<(PathBuf, PathBuf)> {
    let cwd = super::extract_cwd(payload).unwrap_or(".");
    let root = crate::project::find_root(Path::new(cwd))?;
    let wp_dir = crate::project::waypoint_dir(&root);
    wp_dir.is_dir().then_some((root, wp_dir))
}

fn advice_find(query: Option<&str>) -> String {
    match query {
        Some(q) => format!(
            "[waypoint] To locate this, run `waypoint find \"{q}\"` (symbol/name search) instead of grepping blind. {GREP_CAVEAT}"
        ),
        None => format!(
            "[waypoint] To locate this by name or behavior, run `waypoint find \"<keywords>\"` instead of grepping blind. {GREP_CAVEAT}"
        ),
    }
}

fn advice_sketch(symbol: Option<&str>) -> String {
    match symbol {
        Some(s) => format!(
            "[waypoint] To pinpoint this without reading whole files, run `waypoint sketch {s}` — it returns the definition's file, line span, and signature, disambiguating barrel re-exports."
        ),
        None => "[waypoint] To pinpoint a symbol's definition (file, line span, signature) without reading whole files, run `waypoint sketch <name>`.".to_string(),
    }
}

const NUDGE_ASK: &str = "[waypoint] For a where-do-I-start question, run `waypoint ask \"<task>\"` to rank files by relevance to the task instead of grepping or reading blind.";

/// Build the `additionalContext` string for a prompt: classify, then inject data
/// where confident or fall back to advice. Returns `""` when no intent matches.
fn build_context(prompt: &str, payload: &serde_json::Value) -> String {
    match classify(prompt) {
        None => String::new(),
        Some(Intent::Callers) => NUDGE.to_string(),
        Some(Intent::Find) => inject_find(prompt, payload),
        Some(Intent::Sketch) => inject_sketch(prompt, payload),
        Some(Intent::Ask) => inject_ask(prompt, payload),
    }
}

fn inject_find(prompt: &str, payload: &serde_json::Value) -> String {
    let Some(query) = extract_symbol(prompt) else {
        return advice_find(None);
    };
    let Some((_, wp_dir)) = resolve_paths(payload) else {
        return advice_find(Some(&query));
    };
    match crate::map::index::find_symbols(&wp_dir, &query, MAX_LINES) {
        Ok(rows) if !rows.is_empty() => {
            let body = cap_block(&crate::render::find_rows(&rows), MAX_LINES, MAX_CHARS);
            format!("[waypoint] Ran `waypoint find \"{query}\"`:\n{body}\n{GREP_CAVEAT}")
        }
        _ => advice_find(Some(&query)),
    }
}

fn inject_sketch(prompt: &str, payload: &serde_json::Value) -> String {
    let Some(symbol) = extract_symbol(prompt) else {
        return advice_sketch(None);
    };
    let Some((root, wp_dir)) = resolve_paths(payload) else {
        return advice_sketch(Some(&symbol));
    };
    match crate::map::index::sketch(&wp_dir, &symbol) {
        Ok(rows) if !rows.is_empty() => {
            let span = rows
                .iter()
                .map(|r| r.line_end - r.line_start)
                .max()
                .unwrap_or(0);
            let file_bytes = rows
                .iter()
                .filter_map(|r| std::fs::metadata(root.join(&r.file_path)).ok())
                .map(|m| m.len())
                .max()
                .unwrap_or(0);
            if !sketch_pays(span, file_bytes) {
                return advice_sketch(Some(&symbol));
            }
            let body = cap_block(&crate::render::sketch_rows(&rows), MAX_LINES, MAX_CHARS);
            format!("[waypoint] Ran `waypoint sketch {symbol}`:\n{body}")
        }
        _ => advice_sketch(Some(&symbol)),
    }
}

fn inject_ask(prompt: &str, payload: &serde_json::Value) -> String {
    let Some((_, wp_dir)) = resolve_paths(payload) else {
        return NUDGE_ASK.to_string();
    };
    match crate::ask::ask(&wp_dir, prompt, 10) {
        Ok(rows) if !rows.is_empty() => {
            let body = cap_block(&crate::render::ask_rows(&rows, false), MAX_LINES, MAX_CHARS);
            format!("[waypoint] Ran `waypoint ask` to rank files for this task:\n{body}")
        }
        _ => NUDGE_ASK.to_string(),
    }
}

/// `UserPromptSubmit` — classify intent and inject the relevant waypoint output
/// (or advice). Reads the `prompt` and `cwd` fields from the hook payload.
pub fn run() -> Result<(), AppError> {
    let payload = super::read_stdin()?;
    let prompt = payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

    let context = build_context(prompt, &payload);
    super::emit_hook_output(super::HookEvent::UserPromptSubmit, None, &context);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- callers tier (unchanged behavior) ----

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
        assert!(should_nudge("refactor the parser"));
        assert!(should_nudge("what depends on this module"));
    }

    #[test]
    fn fires_on_change_verb_plus_code_context() {
        assert!(should_nudge("rename getTenant to resolveTenant"));
        assert!(should_nudge("remove the validateUser function"));
        assert!(should_nudge("modify the user schema"));
        assert!(should_nudge("change the `MAX_RETRIES` const"));
    }

    #[test]
    fn silent_on_change_verbs_in_prose() {
        assert!(!should_nudge("change the button color to blue"));
        assert!(!should_nudge("delete this paragraph from the doc"));
        assert!(!should_nudge("summarize this article about climate change"));
        assert!(!should_nudge("break this into smaller sentences"));
    }

    #[test]
    fn silent_on_neutral_prompts() {
        assert!(classify("write me a haiku about the sea").is_none());
        assert!(classify("summarize the README").is_none());
    }

    #[test]
    fn silent_on_empty_prompt() {
        assert!(classify("").is_none());
    }

    #[test]
    fn nudge_text_names_both_commands_and_uses_rg() {
        assert!(NUDGE.contains("waypoint callers"));
        assert!(NUDGE.contains("waypoint impact"));
        assert!(NUDGE.contains("`rg`"));
        assert!(!NUDGE.contains("for completeness"));
    }

    // ---- classifier tiers ----

    #[test]
    fn classifies_sketch_intent() {
        assert_eq!(
            classify("show me the structure of MessagingClient"),
            Some(Intent::Sketch)
        );
        assert_eq!(
            classify("what's in the `controllerWrapper` module"),
            Some(Intent::Sketch)
        );
    }

    #[test]
    fn classifies_find_intent() {
        assert_eq!(classify("where is getTenantConfig"), Some(Intent::Find));
        assert_eq!(classify("locate the retry helper"), Some(Intent::Find));
        // bare `find <symbol>`, not just `find the` — a core prompt shape.
        assert_eq!(classify("find GainStats"), Some(Intent::Find));
    }

    #[test]
    fn classifies_ask_intent() {
        assert_eq!(
            classify("where do I start to add a new middleware"),
            Some(Intent::Ask)
        );
        assert_eq!(
            classify("how does the rate limiter work"),
            Some(Intent::Ask)
        );
    }

    #[test]
    fn callers_wins_ties_over_other_intents() {
        // "refactor" (STRONG) + "where is" (FIND) → callers takes priority.
        assert_eq!(
            classify("where is the code I need to refactor"),
            Some(Intent::Callers)
        );
    }

    // ---- symbol extraction (confidence gate) ----

    #[test]
    fn extracts_backticked_identifier() {
        assert_eq!(
            extract_symbol("show me `getTenantDetails` please").as_deref(),
            Some("getTenantDetails")
        );
    }

    #[test]
    fn rejects_backticked_path_or_phrase() {
        // A path is not a symbol arg we trust.
        assert_eq!(extract_symbol("open `src/messaging/client.js`"), None);
    }

    #[test]
    fn extracts_single_code_token() {
        assert_eq!(
            extract_symbol("where is MessagingClient").as_deref(),
            Some("MessagingClient")
        );
        assert_eq!(
            extract_symbol("find get_tenant_config").as_deref(),
            Some("get_tenant_config")
        );
    }

    #[test]
    fn rejects_ambiguous_multiple_tokens() {
        assert_eq!(extract_symbol("rename getTenant to resolveTenant"), None);
    }

    #[test]
    fn extracts_single_lowercase_content_word() {
        // Plain lowercase symbol names resolve once framing vocab is stripped.
        assert_eq!(
            extract_symbol("where is classify").as_deref(),
            Some("classify")
        );
        assert_eq!(
            extract_symbol("show me the shape of run").as_deref(),
            Some("run")
        );
        assert_eq!(extract_symbol("find config").as_deref(), Some("config"));
    }

    #[test]
    fn rejects_multiple_content_words() {
        // Two+ non-framing content words → ambiguous → advice, not a wrong guess.
        assert_eq!(extract_symbol("where is the middleware chain built"), None);
        assert_eq!(extract_symbol("find the user model record"), None);
    }

    // ---- size gate ----

    #[test]
    fn sketch_pays_on_large_span_or_file() {
        assert!(sketch_pays(120, 1000)); // big symbol
        assert!(sketch_pays(10, 9000)); // big file
        assert!(!sketch_pays(10, 1000)); // small both → read it
    }

    // ---- caps ----

    #[test]
    fn cap_block_truncates_excess_lines() {
        let body = (0..20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let capped = cap_block(&body, 5, 10_000);
        assert_eq!(capped.lines().count(), 6); // 5 + marker line
        assert!(capped.contains("truncated"));
    }

    #[test]
    fn cap_block_keeps_short_body_intact() {
        let body = "a\nb\nc";
        assert_eq!(cap_block(body, 12, 10_000), body);
    }

    // ---- advice fallback text ----

    #[test]
    fn find_advice_names_command_and_caveat() {
        let a = advice_find(Some("foo"));
        assert!(a.contains("waypoint find \"foo\""));
        assert!(a.contains("`rg`"));
    }

    #[test]
    fn sketch_advice_names_command() {
        assert!(advice_sketch(Some("Bar")).contains("waypoint sketch Bar"));
    }
}
