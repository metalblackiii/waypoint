//! Natural-language file ranking for task descriptions.
//!
//! Given a task description, scores all project files by relevance using:
//! - IDF-weighted token coverage over map entry descriptions (primary signal)
//! - FTS5 symbol name matching (secondary signal)
//! - Query-shape detection to adjust signal weights

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::AppError;
use crate::map::index::open_index;

/// Ranked file result from the ask pipeline.
#[derive(Debug)]
pub struct AskResult {
    /// Relative file path within the project.
    pub path: String,
    /// Combined relevance score in [0, 1].
    pub score: f64,
    /// Human-readable reason for the ranking.
    pub reason: String,
    /// Description-axis score (for `--explain`).
    pub desc_score: f64,
    /// Symbol-axis score (for `--explain`).
    pub symbol_score: f64,
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "for", "from", "has", "have", "how",
    "if", "in", "is", "it", "its", "my", "no", "not", "of", "on", "or", "so", "the", "this",
    "that", "to", "was", "we", "what", "when", "which", "with",
];

/// Description weight for prose-style queries.
const DESC_WEIGHT: f64 = 0.6;
/// Symbol weight for prose-style queries.
const SYMBOL_WEIGHT: f64 = 0.4;
/// Description weight when query contains code identifiers.
const CODE_DESC_WEIGHT: f64 = 0.35;
/// Symbol weight when query contains code identifiers.
const CODE_SYMBOL_WEIGHT: f64 = 0.65;
/// Maximum length of the reason string in output.
const REASON_MAX_LEN: usize = 60;

/// Rank project files by relevance to a natural-language task description.
///
/// Loads all file descriptions from `map_index.db`, scores each against the
/// tokenized query using IDF-weighted token coverage, fuses with FTS5 symbol matches,
/// and returns the top `limit` results sorted by descending relevance.
pub fn ask(waypoint_dir: &Path, query: &str, limit: usize) -> Result<Vec<AskResult>, AppError> {
    let conn = open_index(waypoint_dir)?;

    let entries = load_descriptions(&conn)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let tokens = tokenize_query(query);
    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let code_heavy = is_code_heavy(query);

    // Lowercase descriptions once — reused for IDF computation and scoring.
    let lower_descs: Vec<String> = entries.iter().map(|(_, d)| d.to_lowercase()).collect();

    let idf = compute_idf(&lower_descs, &tokens);
    let total_idf: f64 = tokens
        .iter()
        .map(|t| idf.get(t).copied().unwrap_or(0.0))
        .sum();
    let desc_scores = score_descriptions(&lower_descs, &tokens, &idf, total_idf);
    let symbol_scores = score_symbols(&conn, &tokens);

    let (dw, sw) = if code_heavy {
        (CODE_DESC_WEIGHT, CODE_SYMBOL_WEIGHT)
    } else {
        (DESC_WEIGHT, SYMBOL_WEIGHT)
    };

    let mut results = combine_scores(&entries, &desc_scores, &symbol_scores, dw, sw);
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(limit);

    Ok(results)
}

// ---------------------------------------------------------------------------
// Query processing
// ---------------------------------------------------------------------------

/// Tokenize a query into lowercase terms, splitting on word boundaries and
/// `camelCase`. Removes stop words and short tokens (<2 chars). Deduplicates.
fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    for segment in query.split(|c: char| !c.is_alphanumeric()) {
        if segment.is_empty() {
            continue;
        }
        for word in split_camel_case(segment) {
            let lower = word.to_lowercase();
            if lower.len() >= 2 && !STOP_WORDS.contains(&lower.as_str()) && !tokens.contains(&lower)
            {
                tokens.push(lower);
            }
        }
    }

    tokens
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
        // lowercase → UPPERCASE: "camelCase" → "camel" | "Case"
        let lc_to_uc = bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase();

        // UPPER run ending: "HTTPServer" → "HTTP" | "Server"
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

/// Detect whether a query contains code-like patterns (`::`, `_`, camelCase).
///
/// When true, symbol matching weight is boosted over description matching.
fn is_code_heavy(query: &str) -> bool {
    query.contains("::")
        || query.contains('_')
        || query
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Check whether a description contains a token as a whole word.
///
/// Splits on non-alphanumeric boundaries to avoid substring false positives
/// (e.g., token `"rs"` must not match the word `"first"`).
fn desc_has_word(desc: &str, token: &str) -> bool {
    desc.split(|c: char| !c.is_alphanumeric())
        .any(|word| word == token)
}

/// Load `(path, description)` pairs from the `map_entries` table.
fn load_descriptions(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn.prepare("SELECT path, description FROM map_entries")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// Compute inverse document frequency for each query token.
///
/// `IDF = ln(1 + total_docs / (1 + docs_containing_token))`. Higher values
/// indicate rarer, more discriminative terms. The `1 +` ensures IDF is always
/// positive even when a term appears in every document.
#[allow(clippy::cast_precision_loss)]
fn compute_idf(lower_descs: &[String], tokens: &[String]) -> HashMap<String, f64> {
    let total = lower_descs.len() as f64;
    let mut idf = HashMap::with_capacity(tokens.len());

    for token in tokens {
        let doc_freq = lower_descs
            .iter()
            .filter(|desc| desc_has_word(desc, token))
            .count() as f64;
        idf.insert(token.clone(), (1.0 + total / (1.0 + doc_freq)).ln());
    }

    idf
}

/// Score each file's description by summing IDF of matched query tokens
/// (binary coverage, not term-frequency), normalized by total query IDF
/// so the result is in [0, 1].
fn score_descriptions(
    lower_descs: &[String],
    tokens: &[String],
    idf: &HashMap<String, f64>,
    total_idf: f64,
) -> Vec<f64> {
    if total_idf <= 0.0 {
        return vec![0.0; lower_descs.len()];
    }

    lower_descs
        .iter()
        .map(|desc| {
            let matched_idf: f64 = tokens
                .iter()
                .filter(|t| desc_has_word(desc, t))
                .map(|t| idf.get(t).copied().unwrap_or(0.0))
                .sum();
            matched_idf / total_idf
        })
        .collect()
}

/// Score files by FTS5 symbol matches, normalized to [0, 1].
///
/// Returns an empty map if FTS5 is unavailable or the query fails —
/// the pipeline degrades to description-only scoring.
fn score_symbols(conn: &Connection, tokens: &[String]) -> HashMap<String, f64> {
    let fts_query = build_fts_query(tokens);
    if fts_query.is_empty() {
        return HashMap::new();
    }

    // FTS5 is best-effort — degrade gracefully on error
    #[allow(clippy::cast_precision_loss)]
    let result: Result<HashMap<String, f64>, rusqlite::Error> = (|| {
        let mut stmt = conn.prepare(
            "SELECT file_path, COUNT(*) as hits \
             FROM symbols_fts \
             WHERE symbols_fts MATCH ?1 \
             GROUP BY file_path",
        )?;
        let rows = stmt.query_map(params![fts_query], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let file_hits: Vec<(String, i64)> = rows.collect::<Result<Vec<_>, _>>()?;

        let max_hits = file_hits.iter().map(|(_, h)| *h).max().unwrap_or(1).max(1);
        let max_f64 = max_hits as f64;

        Ok(file_hits
            .into_iter()
            .map(|(path, hits)| (path, hits as f64 / max_f64))
            .collect())
    })();

    // Intentional: FTS5 failures degrade to description-only scoring rather than
    // failing the entire ask pipeline. Symbol matching is a secondary signal.
    result.unwrap_or_default()
}

/// Build an FTS5 OR query from tokenized terms, quoting each to escape specials.
fn build_fts_query(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| {
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Fuse description and symbol scores into ranked `AskResult`s.
///
/// Entries with zero combined score are filtered out.
fn combine_scores(
    entries: &[(String, String)],
    desc_scores: &[f64],
    symbol_scores: &HashMap<String, f64>,
    desc_weight: f64,
    symbol_weight: f64,
) -> Vec<AskResult> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, (path, description))| {
            let ds = desc_scores.get(i).copied().unwrap_or(0.0);
            let ss = symbol_scores.get(path).copied().unwrap_or(0.0);
            let combined = ds * desc_weight + ss * symbol_weight;

            if combined <= 0.0 {
                return None;
            }

            Some(AskResult {
                path: path.clone(),
                score: combined,
                reason: truncate_description(description, REASON_MAX_LEN),
                desc_score: ds,
                symbol_score: ss,
            })
        })
        .collect()
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

    // -- tokenizer ----------------------------------------------------------

    #[test]
    fn tokenize_simple_prose() {
        let tokens = tokenize_query("add retry logic to billing");
        assert_eq!(tokens, vec!["add", "retry", "logic", "billing"]);
    }

    #[test]
    fn tokenize_removes_stop_words() {
        let tokens = tokenize_query("the quick brown fox");
        assert_eq!(tokens, vec!["quick", "brown", "fox"]);
    }

    #[test]
    fn tokenize_splits_camel_case() {
        let tokens = tokenize_query("fix CamelCase handling");
        assert_eq!(tokens, vec!["fix", "camel", "case", "handling"]);
    }

    #[test]
    fn tokenize_splits_snake_case() {
        let tokens = tokenize_query("update retry_logic function");
        assert_eq!(tokens, vec!["update", "retry", "logic", "function"]);
    }

    #[test]
    fn tokenize_splits_paths() {
        let tokens = tokenize_query("fix src/billing/webhook.rs");
        assert_eq!(tokens, vec!["fix", "src", "billing", "webhook", "rs"]);
    }

    #[test]
    fn tokenize_deduplicates() {
        let tokens = tokenize_query("retry retry retry");
        assert_eq!(tokens, vec!["retry"]);
    }

    #[test]
    fn tokenize_empty_returns_empty() {
        assert!(tokenize_query("").is_empty());
    }

    #[test]
    fn tokenize_only_stop_words_returns_empty() {
        assert!(tokenize_query("the and or is").is_empty());
    }

    #[test]
    fn tokenize_short_tokens_filtered() {
        // Single-char tokens are dropped
        let tokens = tokenize_query("a b cd ef");
        assert_eq!(tokens, vec!["cd", "ef"]);
    }

    #[test]
    fn tokenize_uppercase_acronym_split() {
        let tokens = tokenize_query("HTTPServer");
        assert_eq!(tokens, vec!["http", "server"]);
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

    // -- code detection -----------------------------------------------------

    #[test]
    fn code_heavy_double_colon() {
        assert!(is_code_heavy("std::path::Path"));
    }

    #[test]
    fn code_heavy_underscore() {
        assert!(is_code_heavy("retry_logic"));
    }

    #[test]
    fn code_heavy_camel_case() {
        assert!(is_code_heavy("add retryLogic handler"));
    }

    #[test]
    fn code_heavy_false_for_prose() {
        assert!(!is_code_heavy("add retry logic to billing"));
    }

    // -- IDF ----------------------------------------------------------------

    #[test]
    fn idf_rare_terms_score_higher() {
        let descs = vec![
            "handles billing events".to_string(),
            "billing configuration".to_string(),
            "retry logic for failures".to_string(),
        ];
        let tokens = vec!["billing".to_string(), "retry".to_string()];
        let idf = compute_idf(&descs, &tokens);

        // "billing" in 2/3 docs, "retry" in 1/3 — retry should have higher IDF
        assert!(idf["retry"] > idf["billing"]);
    }

    #[test]
    fn idf_absent_term_gets_max_weight() {
        let descs = vec!["alpha".to_string(), "beta".to_string()];
        let tokens = vec!["alpha".to_string(), "gamma".to_string()];
        let idf = compute_idf(&descs, &tokens);

        // "gamma" appears in 0 docs → highest IDF
        assert!(idf["gamma"] > idf["alpha"]);
    }

    // -- description scoring ------------------------------------------------

    #[test]
    fn description_scoring_all_tokens_match() {
        let descs = vec!["billing webhook retry handler".to_string()];
        let tokens = vec![
            "billing".to_string(),
            "webhook".to_string(),
            "retry".to_string(),
        ];
        let idf = compute_idf(&descs, &tokens);
        let total_idf: f64 = tokens.iter().map(|t| idf[t]).sum();
        let scores = score_descriptions(&descs, &tokens, &idf, total_idf);

        // All tokens match → score = 1.0
        assert!((scores[0] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn description_scoring_partial_match() {
        let descs = vec![
            "billing webhook retry handler".to_string(),
            "user authentication module".to_string(),
            "billing configuration defaults".to_string(),
        ];
        let tokens = vec![
            "billing".to_string(),
            "webhook".to_string(),
            "retry".to_string(),
        ];
        let idf = compute_idf(&descs, &tokens);
        let total_idf: f64 = tokens.iter().map(|t| idf[t]).sum();
        let scores = score_descriptions(&descs, &tokens, &idf, total_idf);

        // First matches all 3, third matches 1, second matches 0
        assert!(scores[0] > scores[2]);
        assert!(scores[2] > scores[1]);
        assert!(scores[1].abs() < f64::EPSILON);
    }

    #[test]
    fn description_scoring_no_substring_false_positives() {
        // "rs" must NOT match "first" or "errors" — only whole-word "rs"
        let descs = vec![
            "first errors in parsing".to_string(), // contains "rs" as substring, not word
            "webhook.rs — handler".to_string(),    // contains "rs" as word boundary
        ];
        let tokens = vec!["rs".to_string()];
        let idf = compute_idf(&descs, &tokens);
        let total_idf: f64 = tokens.iter().map(|t| idf[t]).sum();
        let scores = score_descriptions(&descs, &tokens, &idf, total_idf);

        // "first errors" should NOT match on "rs"
        assert!(
            scores[0].abs() < f64::EPSILON,
            "substring match on 'first'/'errors' is a false positive"
        );
        // "webhook.rs" should match on "rs"
        assert!(
            scores[1] > 0.0,
            "'rs' should match as whole word in 'webhook.rs'"
        );
    }

    #[test]
    fn desc_has_word_rejects_substrings() {
        assert!(!desc_has_word("first errors bitmap", "rs"));
        assert!(!desc_has_word("bitmap manager", "map"));
        assert!(desc_has_word("webhook rs handler", "rs"));
        assert!(desc_has_word("the map module", "map"));
    }

    #[test]
    fn description_scoring_zero_total_idf() {
        let descs = vec!["anything".to_string()];
        let tokens = vec!["test".to_string()];
        let idf = HashMap::new(); // empty — simulates zero total
        let scores = score_descriptions(&descs, &tokens, &idf, 0.0);
        assert!(scores[0].abs() < f64::EPSILON);
    }

    // -- combine ------------------------------------------------------------

    #[test]
    fn combine_filters_zero_scores() {
        let entries = vec![
            ("a.rs".to_string(), "matching file".to_string()),
            ("b.rs".to_string(), "unrelated file".to_string()),
        ];
        let desc_scores = vec![0.8, 0.0];
        let symbol_scores = HashMap::new();

        let results = combine_scores(
            &entries,
            &desc_scores,
            &symbol_scores,
            DESC_WEIGHT,
            SYMBOL_WEIGHT,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "a.rs");
    }

    #[test]
    fn combine_weights_signals_correctly() {
        let entries = vec![
            ("desc_heavy.rs".to_string(), "description match".to_string()),
            ("sym_heavy.rs".to_string(), "no match".to_string()),
        ];
        let desc_scores = vec![1.0, 0.0];
        let mut symbol_scores = HashMap::new();
        symbol_scores.insert("sym_heavy.rs".to_string(), 1.0);

        let results = combine_scores(
            &entries,
            &desc_scores,
            &symbol_scores,
            DESC_WEIGHT,
            SYMBOL_WEIGHT,
        );
        assert_eq!(results.len(), 2);

        let desc_result = results.iter().find(|r| r.path == "desc_heavy.rs").unwrap();
        let sym_result = results.iter().find(|r| r.path == "sym_heavy.rs").unwrap();

        // With default weights (0.6/0.4), desc-only scores higher
        assert!(desc_result.score > sym_result.score);
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
        // "this is a " (10 chars) + "…" (3 bytes)
        assert!(result.len() <= 13);
    }

    // -- FTS query building -------------------------------------------------

    #[test]
    fn fts_query_basic() {
        let tokens = vec!["retry".to_string(), "billing".to_string()];
        assert_eq!(build_fts_query(&tokens), r#""retry" OR "billing""#);
    }

    #[test]
    fn fts_query_escapes_quotes() {
        let tokens = vec!["say\"hello".to_string()];
        assert_eq!(build_fts_query(&tokens), r#""say""hello""#);
    }

    #[test]
    fn fts_query_empty_tokens() {
        let tokens: Vec<String> = Vec::new();
        assert_eq!(build_fts_query(&tokens), "");
    }
}
