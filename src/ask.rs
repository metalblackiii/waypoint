//! Natural-language file ranking for task descriptions.
//!
//! A file's score is the share of the query it covers: the IDF-weighted fraction of query
//! terms found anywhere in the file's path, map description, or symbol names. Scores are
//! absolute, so a low score means a weak match, never "best of a weak lot" — callers can
//! treat a low top score as "no answer".

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::AppError;
use crate::map::index::open_index;

/// Ranked file result from the ask pipeline.
#[derive(Debug, serde::Serialize)]
pub struct AskResult {
    /// Relative file path within the project.
    pub path: String,
    /// IDF-weighted share of query terms the file covers, in [0, 1].
    pub score: f64,
    /// Human-readable reason for the ranking.
    pub reason: String,
    /// Query terms the file covers (for `--explain`).
    pub matched_terms: Vec<String>,
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "for", "from", "has", "have", "how",
    "if", "in", "is", "it", "its", "my", "no", "not", "of", "on", "or", "so", "the", "this",
    "that", "to", "was", "we", "what", "when", "which", "with",
];

/// Maximum length of the reason string in output.
const REASON_MAX_LEN: usize = 60;

/// Share of the score earned by single-term coverage; the rest comes from term pairs.
const TERM_WEIGHT: f64 = 0.75;

/// WHY: `find` shows ranked files only past this bar, because a wrong file costs an agent
/// more than a miss. Chosen 2026-10-05 with `scripts/ask-eval.mjs` on 104 labelled real
/// `find` misses: this gate was right on 18 of 20 answers and silent on all 25 queries with
/// no right answer. Both halves informed the choice, so confirm on misses collected after
/// that date. Rerun `--cutoff 0.64,0.02` after any change to scoring or file descriptions;
/// retune if precision drops below 80%.
const CONFIDENT_SCORE: f64 = 0.64;
const CONFIDENT_LEAD: f64 = 0.02;

/// Every term a file can be found by — path segments, description words, symbol names — and
/// every pair of terms adjacent within one of those phrases.
struct FileTerms {
    path: String,
    description: String,
    terms: HashSet<String>,
    pairs: HashSet<(String, String)>,
}

impl FileTerms {
    fn new(path: String, description: String) -> Self {
        let mut file = Self {
            path: String::new(),
            description: String::new(),
            terms: HashSet::new(),
            pairs: HashSet::new(),
        };
        // WARNING: pairs must not span `/` — `skills/writing-subagents` would otherwise hold
        // "skills writing" and tie with `writing-skills`. `query_pairs` splits the same way.
        for segment in path.split('/') {
            file.add_phrase(segment);
        }
        // Code descriptions list declarations ("fn a(), fn b()"); adjacency across a comma
        // would pair unrelated names, so each item is its own phrase.
        for item in description.split(',') {
            file.add_phrase(item);
        }
        file.path = path;
        file.description = description;
        file
    }

    fn add_phrase(&mut self, text: &str) {
        let sequence = phrase_terms(text);
        for pair in sequence.windows(2) {
            if let Some(key) = pair_key(&pair[0], &pair[1]) {
                self.pairs.insert(key);
            }
        }
        self.terms.extend(sequence);
    }
}

/// Rank project files by relevance to a natural-language task description.
///
/// Returns at most `limit` files with a nonzero score, highest first; ties break by path.
pub fn ask(waypoint_dir: &Path, query: &str, limit: usize) -> Result<Vec<AskResult>, AppError> {
    let query_terms = tokenize(query);
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let conn = open_index(waypoint_dir)?;
    let files = load_file_terms(&conn)?;
    let mut results = rank_files(&files, &query_terms, &query_pairs(query));
    results.truncate(limit);
    Ok(results)
}

/// The one file confident enough to answer a query `find` could not match by name: the top
/// file, when it clears `CONFIDENT_SCORE` and leads the runner-up by `CONFIDENT_LEAD`.
///
/// Only the top file is returned because only the top file's precision was measured;
/// runners-up clearing the score bar were mostly wrong in real repos.
pub fn confident(waypoint_dir: &Path, query: &str) -> Result<Option<AskResult>, AppError> {
    let ranked = ask(waypoint_dir, query, 2)?;
    Ok(if is_confident(&ranked) {
        ranked.into_iter().next()
    } else {
        None
    })
}

fn is_confident(ranked: &[AskResult]) -> bool {
    match ranked {
        [] => false,
        [top] => top.score >= CONFIDENT_SCORE,
        [top, runner_up, ..] => {
            top.score >= CONFIDENT_SCORE && top.score - runner_up.score >= CONFIDENT_LEAD
        }
    }
}

// ---------------------------------------------------------------------------
// Terms
// ---------------------------------------------------------------------------

/// All terms in `text`, in order, with repeats — the sequence adjacency is read from.
fn phrase_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for_each_term(text, |term| terms.push(term));
    terms
}

/// Order-free key for two adjacent terms, so "config default" matches `default_config`.
/// `None` for a term repeated next to itself, which says nothing about the phrase.
fn pair_key(a: &str, b: &str) -> Option<(String, String)> {
    match a.cmp(b) {
        std::cmp::Ordering::Less => Some((a.to_string(), b.to_string())),
        std::cmp::Ordering::Greater => Some((b.to_string(), a.to_string())),
        std::cmp::Ordering::Equal => None,
    }
}

/// Unique adjacent term pairs in the query, never spanning `/` (file paths pair the same way).
fn query_pairs(query: &str) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    query
        .split('/')
        .flat_map(|segment| {
            phrase_terms(segment)
                .windows(2)
                .filter_map(|pair| pair_key(&pair[0], &pair[1]))
                .collect::<Vec<_>>()
        })
        .filter(|key| seen.insert(key.clone()))
        .collect()
}

/// Tokenize text into unique terms, in first-seen order.
fn tokenize(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for_each_term(text, |term| {
        if seen.insert(term.clone()) {
            terms.push(term);
        }
    });
    terms
}

/// Emit each term in `text`: split on non-alphanumerics and `camelCase`, lowercase, drop stop
/// words and single characters, and fold a trailing plural `s` so "skills" finds "skill".
fn for_each_term(text: &str, mut emit: impl FnMut(String)) {
    for segment in text.split(|c: char| !c.is_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }
        for word in split_camel_case(segment) {
            let lower = word.to_lowercase();
            if lower.len() < 2 || STOP_WORDS.contains(&lower.as_str()) {
                continue;
            }
            emit(singular(lower));
        }
    }
}

/// SHORTCUT: strips one trailing `s` (not `ss`) from words over three letters. Mangles words
/// like "status", harmlessly, because queries and files fold the same way. Upgrade to a real
/// stemmer if the ask eval shows misses on other inflections ("-ing", "-ed").
fn singular(word: String) -> String {
    if word.len() > 3
        && !word.ends_with("ss")
        && let Some(stem) = word.strip_suffix('s')
    {
        return stem.to_string();
    }
    word
}

/// Split a string on `camelCase` / `PascalCase` boundaries.
///
/// `"camelCase"` → `["camel", "Case"]`, `"HTTPServer"` → `["HTTP", "Server"]`.
/// Non-ASCII input is returned as a single element (code identifiers are ASCII).
fn split_camel_case(s: &str) -> Vec<&str> {
    if !s.is_ascii() || s.len() <= 1 {
        return vec![s];
    }

    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut start = 0;

    for i in 1..bytes.len() {
        let lc_to_uc = bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase();
        let uc_run_end = i + 1 < bytes.len()
            && bytes[i - 1].is_ascii_uppercase()
            && bytes[i].is_ascii_uppercase()
            && bytes[i + 1].is_ascii_lowercase();

        if lc_to_uc || uc_run_end {
            if start < i {
                result.push(&s[start..i]);
            }
            start = i;
        }
    }

    if start < s.len() {
        result.push(&s[start..]);
    }

    result
}

/// Load every mapped file with the terms from its path, description, and symbol names.
fn load_file_terms(conn: &Connection) -> Result<Vec<FileTerms>, AppError> {
    let mut stmt = conn.prepare("SELECT path, description FROM map_entries")?;
    let mut files = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .map(|row| row.map(|(path, description)| FileTerms::new(path, description)))
        .collect::<Result<Vec<_>, _>>()?;

    let position: HashMap<String, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.clone(), i))
        .collect();

    let mut stmt = conn.prepare("SELECT file_path, name FROM symbols")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (file_path, name) = row?;
        if let Some(&i) = position.get(&file_path) {
            files[i].add_phrase(&name);
        }
    }

    Ok(files)
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Inverse document frequency of each query term across files, aligned with `query_terms`.
///
/// `IDF = ln(1 + total_files / (1 + files_containing_term))`. A term no file contains gets
/// the highest weight, which lowers every file's coverage — a query about something absent
/// from the project should score low everywhere.
#[allow(clippy::cast_precision_loss)] // file counts are far below f64's exact-integer range
fn compute_idf(files: &[FileTerms], query_terms: &[String]) -> Vec<f64> {
    let total = files.len() as f64;
    query_terms
        .iter()
        .map(|term| {
            let doc_freq = files.iter().filter(|f| f.terms.contains(term)).count() as f64;
            (1.0 + total / (1.0 + doc_freq)).ln()
        })
        .collect()
}

/// Score each file by IDF-weighted term coverage, blended with the share of the query's
/// adjacent term pairs it holds adjacent too; drop files that cover nothing.
///
/// Pairs separate files that merely contain the query's words from the one that names the
/// query's phrase: `writing-skills/SKILL.md` over `writing-subagents/SKILL.md`.
#[allow(clippy::cast_precision_loss)] // pair counts are tiny
fn rank_files(
    files: &[FileTerms],
    query_terms: &[String],
    query_pairs: &[(String, String)],
) -> Vec<AskResult> {
    let idf = compute_idf(files, query_terms);
    let total_idf: f64 = idf.iter().sum();
    if total_idf <= 0.0 {
        return Vec::new();
    }

    let mut results: Vec<AskResult> = files
        .iter()
        .filter_map(|file| {
            let mut covered = 0.0;
            let mut matched_terms = Vec::new();
            for (term, weight) in query_terms.iter().zip(&idf) {
                if file.terms.contains(term) {
                    covered += weight;
                    matched_terms.push(term.clone());
                }
            }
            let term_score = covered / total_idf;
            let score = if query_pairs.is_empty() {
                term_score
            } else {
                let held = query_pairs
                    .iter()
                    .filter(|p| file.pairs.contains(p))
                    .count();
                let pair_score = held as f64 / query_pairs.len() as f64;
                TERM_WEIGHT * term_score + (1.0 - TERM_WEIGHT) * pair_score
            };
            (!matched_terms.is_empty()).then(|| AskResult {
                path: file.path.clone(),
                score,
                reason: truncate_description(&file.description, REASON_MAX_LEN),
                matched_terms,
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.path.cmp(&b.path))
    });
    results
}

/// Truncate a description to `max_len` characters, appending `…` if shortened.
fn truncate_description(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }

    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn file(path: &str, description: &str, symbols: &[&str]) -> FileTerms {
        let mut file = FileTerms::new(path.to_string(), description.to_string());
        for name in symbols {
            file.add_phrase(name);
        }
        file
    }

    fn rank(files: &[FileTerms], query: &str) -> Vec<AskResult> {
        rank_files(files, &tokenize(query), &query_pairs(query))
    }

    // -- tokenizer ----------------------------------------------------------

    #[test]
    fn tokenize_simple_prose() {
        assert_eq!(
            tokenize("add retry logic to billing"),
            vec!["add", "retry", "logic", "billing"]
        );
    }

    #[test]
    fn tokenize_removes_stop_words() {
        assert_eq!(
            tokenize("the quick brown fox"),
            vec!["quick", "brown", "fox"]
        );
    }

    #[test]
    fn tokenize_splits_camel_case() {
        assert_eq!(
            tokenize("fix CamelCase handling"),
            vec!["fix", "camel", "case", "handling"]
        );
    }

    #[test]
    fn tokenize_splits_snake_case() {
        assert_eq!(
            tokenize("update retry_logic function"),
            vec!["update", "retry", "logic", "function"]
        );
    }

    #[test]
    fn tokenize_splits_paths() {
        assert_eq!(
            tokenize("fix src/billing/webhook.rs"),
            vec!["fix", "src", "billing", "webhook", "rs"]
        );
    }

    #[test]
    fn tokenize_deduplicates() {
        assert_eq!(tokenize("retry retry retry"), vec!["retry"]);
    }

    #[test]
    fn tokenize_empty_returns_empty() {
        assert_eq!(tokenize(""), Vec::<String>::new());
    }

    #[test]
    fn tokenize_only_stop_words_returns_empty() {
        assert_eq!(tokenize("the and or is"), Vec::<String>::new());
    }

    #[test]
    fn tokenize_short_tokens_filtered() {
        assert_eq!(tokenize("a b cd ef"), vec!["cd", "ef"]);
    }

    #[test]
    fn tokenize_uppercase_acronym_split() {
        assert_eq!(tokenize("HTTPServer"), vec!["http", "server"]);
    }

    #[test]
    fn tokenize_folds_plural_s() {
        assert_eq!(tokenize("skills plugins"), vec!["skill", "plugin"]);
    }

    #[test]
    fn tokenize_keeps_double_s_and_short_words() {
        assert_eq!(tokenize("process gas"), vec!["process", "gas"]);
    }

    // -- camelCase splitter -------------------------------------------------

    #[test]
    fn camel_case_basic() {
        assert_eq!(split_camel_case("camelCase"), vec!["camel", "Case"]);
    }

    #[test]
    fn camel_case_pascal() {
        assert_eq!(split_camel_case("PascalCase"), vec!["Pascal", "Case"]);
    }

    #[test]
    fn camel_case_acronym() {
        assert_eq!(split_camel_case("HTTPServer"), vec!["HTTP", "Server"]);
    }

    #[test]
    fn camel_case_single_word() {
        assert_eq!(split_camel_case("simple"), vec!["simple"]);
    }

    #[test]
    fn camel_case_all_upper() {
        assert_eq!(split_camel_case("HTTP"), vec!["HTTP"]);
    }

    // -- IDF ----------------------------------------------------------------

    #[test]
    fn idf_rare_terms_weigh_more() {
        let files = vec![
            file("a.md", "billing events", &[]),
            file("b.md", "billing configuration", &[]),
            file("c.md", "retry logic", &[]),
        ];
        let idf = compute_idf(&files, &tokenize("billing retry"));
        assert!(idf[1] > idf[0]);
    }

    // -- ranking ------------------------------------------------------------

    #[test]
    fn rank_matches_terms_in_path() {
        let files = vec![
            file(
                ".github/workflows/run-all-tests.yml",
                "GHA: Run All Tests",
                &[],
            ),
            file("src/billing.js", "fn charge()", &["charge"]),
        ];
        let results = rank(&files, "run all tests workflow");
        assert_eq!(results[0].path, ".github/workflows/run-all-tests.yml");
        assert!(results[0].score > 0.9, "score was {}", results[0].score);
    }

    #[test]
    fn rank_matches_terms_in_symbol_names() {
        let files = vec![
            file("src/guard.ts", "export function", &["acquireGuardDir"]),
            file("src/other.ts", "export function", &["releaseLock"]),
        ];
        let results = rank(&files, "acquire guard");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/guard.ts");
    }

    #[test]
    fn rank_score_is_absolute_not_relative_to_best() {
        // Only one of three rare query terms is covered anywhere: the best file must
        // still score well below 1.0.
        let files = vec![
            file("src/a.rs", "alpha", &[]),
            file("src/b.rs", "beta", &[]),
        ];
        let results = rank(&files, "alpha gamma delta");
        assert_eq!(results.len(), 1);
        assert!(results[0].score < 0.5, "score was {}", results[0].score);
    }

    #[test]
    fn rank_symbol_count_does_not_inflate_score() {
        let many: Vec<String> = (0..200).map(|i| format!("skillHelper{i}")).collect();
        let many_refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let files = vec![
            file("plugins/big/scripts/triage.ts", "export", &many_refs),
            file("plugins/ui-tester/skills/SKILL.md", "UI tester skill", &[]),
        ];
        let results = rank(&files, "ui tester skill");
        assert_eq!(results[0].path, "plugins/ui-tester/skills/SKILL.md");
    }

    #[test]
    fn rank_ties_break_by_path() {
        let files = vec![file("b.md", "retry", &[]), file("a.md", "retry", &[])];
        let results = rank(&files, "retry");
        assert_eq!(results[0].path, "a.md");
        assert!((results[0].score - results[1].score).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_reports_matched_terms() {
        let files = vec![file("src/retry.rs", "billing", &[])];
        let results = rank(&files, "billing retry webhook");
        assert_eq!(results[0].matched_terms, vec!["billing", "retry"]);
    }

    #[test]
    fn rank_empty_files_returns_empty() {
        assert!(rank(&[], "retry").is_empty());
    }

    #[test]
    fn rank_prefers_file_holding_query_phrase() {
        let files = vec![
            file(".codex/skills/writing-subagents/SKILL.md", "Writing", &[]),
            file(
                "codex/.agents/skills/writing-skills/SKILL.md",
                "Writing",
                &[],
            ),
        ];
        let results = rank(&files, "writing skills");
        assert_eq!(
            results[0].path,
            "codex/.agents/skills/writing-skills/SKILL.md"
        );
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn rank_pairs_ignore_word_order() {
        let files = vec![
            file("cli/config.py", "config module", &["load"]),
            file("cli/defaults.py", "defaults", &["DEFAULT_CONFIG"]),
        ];
        let results = rank(&files, "config default");
        assert_eq!(results[0].path, "cli/defaults.py");
    }

    fn scored(scores: &[f64]) -> Vec<AskResult> {
        scores
            .iter()
            .enumerate()
            .map(|(i, &score)| AskResult {
                path: format!("f{i}"),
                score,
                reason: String::new(),
                matched_terms: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn confident_needs_score_and_lead() {
        assert!(is_confident(&scored(&[0.9, 0.5])));
        assert!(is_confident(&scored(&[0.7])));
        assert!(!is_confident(&scored(&[0.6, 0.1])), "score below bar");
        assert!(!is_confident(&scored(&[0.9, 0.89])), "no clear lead");
        assert!(!is_confident(&[]));
    }

    #[test]
    fn rank_pairs_do_not_span_description_items() {
        let joined = file("a.rs", "fn retry(), fn billing()", &[]);
        assert!(
            !joined
                .pairs
                .contains(&("billing".to_string(), "retry".to_string()))
        );
    }

    // -- truncation ---------------------------------------------------------

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate_description("short", 10), "short");
    }

    #[test]
    fn truncate_at_limit_unchanged() {
        let s = "exactly ten";
        assert_eq!(truncate_description(s, s.len()), s);
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let result = truncate_description("this is a long description", 10);
        assert!(result.ends_with('…'));
        assert!(result.len() <= 13);
    }
}
