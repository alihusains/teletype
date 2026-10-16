//! Screen context captured at hotkey-down: what the user can see, reduced to
//! the shape the engine and the polish prompt can use.
//!
//! Privacy contract: raw context text never leaves the device and never
//! enters a trace. Traces carry [`TraceContextSummary`] — lengths and counts
//! only. [`ephemeral_terms`] extracts the capitalized tokens, @handles,
//! CamelCase/snake_case identifiers, and email local-parts that the
//! per-take boost list, the protected-terms guard, and the polish prompt
//! rely on. Extraction is self-contained on purpose: it must not depend on
//! `learning/**` (written in parallel) or anything platform-specific.

use crate::app_category::AppCategory;
use serde::{Deserialize, Serialize};

/// What was on screen when the hotkey went down.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenContext {
    /// Bundle id of the focused app, when the platform exposes one.
    pub app_id: Option<String>,
    /// Window title, when readable.
    pub window_title: Option<String>,
    /// Field text before the caret, bounded by the caller.
    pub focused_before: Option<String>,
    /// Field text after the caret, bounded by the caller.
    pub focused_after: Option<String>,
    /// Selected text, if any.
    pub selection: Option<String>,
    /// Category of the focused app (see [`app_category`]).
    pub app_category: AppCategory,
}

/// Hard cap on extracted terms; the boost list and prompt must stay small.
const MAX_TERMS: usize = 50;

/// A deliberately small, embedded English stopword + calendar list. Terms
/// that reduce to one of these (case-insensitively) are dropped. The list is
/// local and fixed so extraction stays deterministic and dependency-free.
const STOPWORDS: &[&str] = &[
    // function words
    "the",
    "and",
    "for",
    "are",
    "but",
    "not",
    "you",
    "all",
    "any",
    "can",
    "her",
    "was",
    "one",
    "our",
    "out",
    "get",
    "has",
    "him",
    "his",
    "how",
    "its",
    "may",
    "new",
    "now",
    "old",
    "see",
    "two",
    "way",
    "who",
    "did",
    "let",
    "put",
    "say",
    "she",
    "too",
    "use",
    "will",
    "with",
    "this",
    "that",
    "from",
    "they",
    "know",
    "want",
    "been",
    "have",
    "just",
    "over",
    "such",
    "take",
    "come",
    "into",
    "more",
    "other",
    "some",
    "time",
    "very",
    "when",
    "your",
    "which",
    "while",
    "their",
    "there",
    "would",
    "could",
    "should",
    "about",
    "after",
    "before",
    "because",
    "between",
    "during",
    "without",
    "within",
    "upon",
    "then",
    "than",
    "also",
    "only",
    "very",
    "well",
    "here",
    "where",
    "what",
    "when",
    "why",
    "each",
    "even",
    "both",
    "much",
    "many",
    "most",
    "much",
    // months
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    // days
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    // common title-ish words
    "draft",
    "reply",
    "forward",
    "sent",
    "inbox",
    "unread",
    "message",
    "messages",
    "document",
    "untitled",
    "project",
    "task",
    "tasks",
];

fn is_stopword(word: &str) -> bool {
    STOPWORDS
        .iter()
        .any(|s| *s == word.to_ascii_lowercase().as_str())
}

/// Strip surrounding punctuation from a raw token, but keep a leading `@`:
/// it is the marker that separates an @handle from an email local-part, and
/// it is not punctuation to be stripped. Everything else non-alphanumeric
/// at either end goes.
fn strip_punct(token: &str) -> &str {
    let leading = token.starts_with('@');
    token.trim_matches(|c: char| !c.is_alphanumeric() && !(leading && c == '@'))
}

/// Classify one whitespace-free raw token. Returns the canonical term when
/// it is worth keeping, or `None`.
fn classify_token(raw: &str) -> Option<String> {
    let token = strip_punct(raw);
    if token.len() < 2 {
        return None;
    }
    if token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    // Email local-part (before @): "jane.doe@x.com" -> "jane.doe".
    // @handles are handled below so "@jdoe" never degrades to empty.
    if token.contains('@') && !token.starts_with('@') {
        let local = strip_punct(token.split('@').next().unwrap_or(""));
        if local.len() >= 2 && !local.chars().all(|c| c.is_ascii_digit()) {
            return Some(local.to_string());
        }
        return None;
    }

    // @handle: keep the part after the @, punctuation-stripped.
    // The @ is kept in the term: it is what separates @jdoe from the
    // email local-part jdoe, and the boost list wants the handle form.
    if let Some(handle) = token.strip_prefix('@') {
        let handle = strip_punct(handle);
        if handle.len() >= 2 && !handle.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("@{handle}"));
        }
        return None;
    }

    // CamelCase: an internal uppercase letter (not the first char) and at
    // least two alphabetic runs. "iPhone15" counts.
    let chars: Vec<char> = token.chars().collect();
    let has_internal_upper = chars
        .iter()
        .enumerate()
        .any(|(i, c)| i > 0 && c.is_uppercase());
    let starts_upper = chars.first().is_some_and(|c| c.is_uppercase());

    if has_internal_upper {
        return Some(token.to_string());
    }

    // snake_case identifiers: internal underscore with word-ish parts.
    if token.contains('_') && token.chars().filter(|&c| c == '_').count() >= 1 {
        let parts_ok = token
            .split('_')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric()));
        if parts_ok && token.chars().any(|c| c.is_alphabetic()) {
            return Some(token.to_string());
        }
    }

    // Mid-text capitalized word: starts uppercase, not at phrase start.
    // Phrase-start position is decided by the caller; here we only classify
    // "looks like a name" tokens. The phrase-start filter is applied in
    // `ephemeral_terms` because it needs the preceding character.
    if starts_upper && token.chars().filter(|c| c.is_alphabetic()).count() >= 2 {
        return Some(token.to_string());
    }

    None
}

/// Extract the ephemeral terms from a screen context.
///
/// Deterministic first-seen order, deduped case-insensitively (first
/// spelling wins), capped at [`MAX_TERMS`].
pub fn ephemeral_terms(ctx: &ScreenContext) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let push = |term: String, seen: &mut Vec<String>| {
        let lower = term.to_ascii_lowercase();
        if is_stopword(&lower) {
            return;
        }
        if seen.iter().any(|t| t.to_ascii_lowercase() == lower) {
            return;
        }
        seen.push(term);
    };

    let scan = |text: &str, seen: &mut Vec<String>| {
        let mut prev_word = false;
        for raw in text.split_whitespace() {
            // A token is "at phrase start" when it is the first token of the scanned
            // text: there, a capitalized word is ordinary sentence case, not a name,
            // so it is skipped. (The preceding character is not tracked, so a capital
            // right after a period mid-field is still kept — the conservative
            // direction for a name list.)
            let phrase_start = !prev_word;
            prev_word = true;
            let Some(term) = classify_token(raw) else {
                continue;
            };
            if phrase_start && is_plain_capitalized_word(&term) {
                continue;
            }
            if seen.len() >= MAX_TERMS {
                return;
            }
            push(term, seen);
        }
    };

    if let Some(t) = &ctx.window_title {
        scan(t, &mut seen);
    }
    if let Some(t) = &ctx.focused_before {
        scan(t, &mut seen);
    }
    if let Some(t) = &ctx.selection {
        scan(t, &mut seen);
    }
    if let Some(t) = &ctx.focused_after {
        scan(t, &mut seen);
    }

    seen.into_iter().take(MAX_TERMS).collect()
}

/// A single all-caps-first word like "The" or "Apple" with no internal
/// capitals: sentence-initial, not a name. CamelCase and acronyms pass.
/// @handles are never treated as sentence-initial.
fn is_plain_capitalized_word(term: &str) -> bool {
    if term.starts_with('@') {
        return false;
    }
    let chars: Vec<char> = term.chars().collect();
    if chars.len() < 2 {
        return true;
    }
    chars.first().is_some_and(|c| c.is_uppercase()) && chars[1..].iter().all(|c| !c.is_uppercase())
}

/// The non-S1 polish prompt section: the human-readable line shown to the
/// polish model. `None` when there are no terms (nothing worth saying).
pub fn context_section(_ctx: &ScreenContext, terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    Some(format!(
        "Names and terms visible on screen: {}",
        terms.join(", ")
    ))
}

/// What a trace is allowed to carry about the screen context: lengths and
/// counts only. No field holds raw text; a trace that needed raw context
/// would be a privacy bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContextSummary {
    pub app_category: AppCategory,
    pub before_len: usize,
    pub after_len: usize,
    pub selection_len: usize,
    pub term_count: usize,
}

/// Build the privacy-safe summary for a trace.
pub fn privacy_safe_summary(ctx: &ScreenContext, terms: &[String]) -> TraceContextSummary {
    TraceContextSummary {
        app_category: ctx.app_category,
        before_len: ctx
            .focused_before
            .as_deref()
            .map_or(0, |s| s.chars().count()),
        after_len: ctx
            .focused_after
            .as_deref()
            .map_or(0, |s| s.chars().count()),
        selection_len: ctx.selection.as_deref().map_or(0, |s| s.chars().count()),
        term_count: terms.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        before: &str,
        after: &str,
        selection: Option<&str>,
        title: Option<&str>,
    ) -> ScreenContext {
        ScreenContext {
            app_id: Some("com.example.test".into()),
            window_title: title.map(String::from),
            focused_before: Some(before.into()),
            focused_after: Some(after.into()),
            selection: selection.map(String::from),
            app_category: AppCategory::Chat,
        }
    }

    #[test]
    fn extracts_at_handles() {
        let terms = ephemeral_terms(&ctx("cc @jdsmith and @alice_smith please", "", None, None));
        assert!(terms.contains(&"@jdsmith".to_string()));
        assert!(terms.contains(&"@alice_smith".to_string()));
    }

    #[test]
    fn extracts_camel_case() {
        let terms = ephemeral_terms(&ctx("use the parseConfig function", "", None, None));
        assert!(terms.contains(&"parseConfig".to_string()));
    }

    #[test]
    fn extracts_snake_case() {
        let terms = ephemeral_terms(&ctx("set the max_retries value", "", None, None));
        assert!(terms.contains(&"max_retries".to_string()));
    }

    #[test]
    fn extracts_email_local_part() {
        let terms = ephemeral_terms(&ctx("ask jane.doe@example.com", "", None, None));
        assert!(terms.contains(&"jane.doe".to_string()));
    }

    #[test]
    fn mid_sentence_caps_are_kept_phrase_start_dropped() {
        let terms = ephemeral_terms(&ctx("The meeting with Alice is at 3", "", None, None));
        assert!(
            !terms.contains(&"The".to_string()),
            "phrase-start 'The' is sentence case"
        );
        assert!(terms.contains(&"Alice".to_string()));
    }

    #[test]
    fn dedupe_case_insensitive_first_spelling_wins() {
        let terms = ephemeral_terms(&ctx("talked to Alice then alice and ALICE", "", None, None));
        let alice: Vec<_> = terms
            .iter()
            .filter(|t| t.eq_ignore_ascii_case("alice"))
            .collect();
        assert_eq!(alice, vec!["Alice"]);
    }

    #[test]
    fn pure_numbers_and_single_chars_dropped() {
        let terms = ephemeral_terms(&ctx("the 2026 budget is 42 b", "", None, None));
        assert!(!terms.contains(&"2026".to_string()));
        assert!(!terms.contains(&"42".to_string()));
        assert!(!terms.contains(&"b".to_string()));
    }

    #[test]
    fn stopwords_and_calendar_words_dropped() {
        let terms = ephemeral_terms(&ctx(
            "We scheduled it for January on Monday",
            "",
            None,
            None,
        ));
        assert!(!terms.contains(&"January".to_string()));
        assert!(!terms.contains(&"Monday".to_string()));
        assert!(!terms.contains(&"We".to_string()));
    }

    #[test]
    fn caps_at_fifty_terms() {
        let text: String = (0..80).map(|i| format!("TermX{i} ")).collect();
        let terms = ephemeral_terms(&ctx(&text, "", None, None));
        assert_eq!(terms.len(), 50);
        // First-seen order is deterministic.
        assert_eq!(terms.first().map(String::as_str), Some("TermX0"));
        assert_eq!(terms.last().map(String::as_str), Some("TermX49"));
    }

    #[test]
    fn punctuation_is_stripped() {
        let terms = ephemeral_terms(&ctx(
            "see @jdsmith, (CamelCase) and snake_case ok",
            "",
            None,
            None,
        ));
        assert!(terms.contains(&"@jdsmith".to_string()));
        assert!(terms.contains(&"CamelCase".to_string()));
        assert!(terms.contains(&"snake_case".to_string()));
    }

    #[test]
    fn window_title_is_scanned() {
        let terms = ephemeral_terms(&ctx("", "", None, Some("Q3 Roadmap - Obsidian Vault")));
        assert!(terms.contains(&"Obsidian".to_string()));
        assert!(terms.contains(&"Roadmap".to_string()));
    }

    #[test]
    fn empty_context_yields_no_terms() {
        let terms = ephemeral_terms(&ctx("", "", None, None));
        assert!(terms.is_empty());
    }

    #[test]
    fn context_section_none_when_empty() {
        assert_eq!(context_section(&ctx("", "", None, None), &[]), None);
    }

    #[test]
    fn context_section_joins_terms() {
        let terms = vec!["Alice".to_string(), "parseConfig".to_string()];
        assert_eq!(
            context_section(&ctx("", "", None, None), &terms).as_deref(),
            Some("Names and terms visible on screen: Alice, parseConfig")
        );
    }

    #[test]
    fn privacy_summary_carries_only_lengths_and_counts() {
        let c = ctx("hello world", "next line", Some("sel"), Some("Title"));
        let terms = ephemeral_terms(&c);
        let s = privacy_safe_summary(&c, &terms);
        assert_eq!(s.before_len, "hello world".chars().count());
        assert_eq!(s.after_len, "next line".chars().count());
        assert_eq!(s.selection_len, 3);
        assert_eq!(s.term_count, terms.len());
        assert_eq!(s.app_category, AppCategory::Chat);
        // Prove no raw text can leak: the only fields are the enum and sizes.
        let debug = format!("{s:?}");
        assert!(!debug.contains("hello world"));
        assert!(!debug.contains("next line"));
        assert!(!debug.contains("Title"));
    }

    #[test]
    fn summary_handles_missing_fields() {
        let c = ScreenContext::default();
        let s = privacy_safe_summary(&c, &[]);
        assert_eq!(s.before_len, 0);
        assert_eq!(s.after_len, 0);
        assert_eq!(s.selection_len, 0);
        assert_eq!(s.term_count, 0);
        assert_eq!(s.app_category, AppCategory::Other);
    }
}
