//! P6.3: protected-terms guard.
//!
//! Belt-and-braces protection for the LLM polish path. After the model
//! produces a rewrite, the guard verifies that protected tokens (lexicon
//! terms, boost terms, numbers, URLs, emails, code identifiers) survived
//! the round-trip. Changed protected tokens are restored in place; a removed
//! protected token rejects the entire model output and the rules-only input is
//! returned. Reinsertion was tried and removed: an unmatched polished word
//! near the gap is indistinguishable from a genuine substitution, so
//! reinserting either duplicated it or stole an inserted word. If more than
//! three were changed, the entire model output is rejected as before.
//!
//! The word-level diff is a hand-rolled O(n·m) LCS over word vectors,
//! comparing words case-insensitively (punctuation-stripped) so that a
//! capitalization-only change is never treated as a substitution. No new
//! dependency.
//!
//! A hallucination guard rejects output that is substantially longer than
//! the input or contains a long phrase absent from the input.

// ── Protected span collection ────────────────────────────────────────────────

/// A protected word span: `(start_word_index, end_word_index_exclusive,
/// original_word)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSpan {
    pub start: usize,
    pub end: usize,
    pub word: String,
}

/// Collects protected word-index spans from the input.
///
/// Protected tokens are:
/// - lexicon terms present in `terms` (whole-word, case-insensitive match)
/// - boost-term text (whole-word, case-insensitive match)
/// - numbers (integer or decimal)
/// - URLs (`http://`, `https://`, `www.`)
/// - email addresses
/// - code identifiers: `snake_case` (≥ 2 words joined by `_`), `CamelCase`
///   (≥ 2 humps), `ALL_CAPS` (≥ 2 chars), or containing `.rs` / `.cpp` / `::`
///
/// Spans are returned in input order, non-overlapping.
pub fn collect_protected(
    input: &str,
    terms: &[String],
    boost_terms: &[&str],
) -> Vec<ProtectedSpan> {
    let words: Vec<&str> = input.split_whitespace().collect();
    let mut spans: Vec<ProtectedSpan> = Vec::new();

    // Build a set of protected lexicon/boost terms (lowercased).
    let mut lexicon: std::collections::HashSet<String> = std::collections::HashSet::new();
    for t in terms {
        lexicon.insert(t.to_lowercase());
    }
    for t in boost_terms {
        lexicon.insert(t.to_lowercase());
    }

    for (i, w) in words.iter().enumerate() {
        // Keep URL/email/code characters inside the token; strip everything
        // else (quotes, parentheses, terminal punctuation).
        let bare = w.trim_matches(|c: char| {
            !c.is_alphanumeric() && !matches!(c, '.' | ':' | '/' | '@' | '-')
        });
        if bare.is_empty() {
            continue;
        }
        let lower = bare.to_lowercase();

        let is_protected = lexicon.contains(&lower)
            || is_number(bare)
            || is_url(bare)
            || is_email(bare)
            || is_code_identifier(bare);

        if is_protected {
            spans.push(ProtectedSpan {
                start: i,
                end: i + 1,
                word: w.to_string(),
            });
        }
    }

    spans
}

/// True when the token is a number (integer or decimal, optional minus).
fn is_number(w: &str) -> bool {
    let s = w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
    if s.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if c == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if c == '-' {
            // Only valid at the start.
            if s.len() > 1 && !s.starts_with('-') {
                return false;
            }
        } else {
            return false;
        }
    }
    seen_digit
}

/// True when the token looks like a URL.
fn is_url(w: &str) -> bool {
    let lower = w.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.")
}

/// True when the token looks like an email address.
fn is_email(w: &str) -> bool {
    let at_count = w.matches('@').count();
    if at_count != 1 {
        return false;
    }
    let Some((local, domain)) = w.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    domain.contains('.')
}

/// True when the token looks like a code identifier:
/// - `snake_case` (≥ 2 segments joined by `_`)
/// - `CamelCase` (≥ 2 humps)
/// - `ALL_CAPS` (≥ 2 chars, all alphabetic)
/// - contains `.rs`, `.cpp`, or `::`
fn is_code_identifier(w: &str) -> bool {
    if w.contains("::") || w.contains(".rs") || w.contains(".cpp") {
        return true;
    }
    // snake_case: at least one underscore, ≥ 2 alpha segments.
    if w.contains('_') {
        let segments: Vec<&str> = w.split('_').filter(|s| !s.is_empty()).collect();
        if segments.len() >= 2
            && segments
                .iter()
                .all(|s| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        {
            return true;
        }
    }
    // Capitalized word (starts uppercase, length ≥ 4): covers CamelCase
    // ("CherryPick"), single-hump ("Siobhan"), and all-lowercase-after-first
    // ("Kubernetes"). Length ≥ 4 avoids flagging short words like "Ok".
    if w.len() >= 4 && w.chars().next().is_some_and(|c| c.is_uppercase()) {
        return true;
    }
    // ALL_CAPS: ≥ 2 chars, all alphabetic uppercase.
    if w.len() >= 2 && w.chars().all(|c| c.is_ascii_uppercase()) {
        return true;
    }
    false
}

// ── LCS diff ─────────────────────────────────────────────────────────────────

/// Normalise a word for diff/alignment: strip surrounding punctuation and
/// lowercase. A capitalization-only change is not a protected change.
fn bare_lower(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Hand-rolled O(n·m) LCS over word vectors, comparing words by their
/// [`bare_lower`] form so that case-only differences do not create spurious
/// substitutions. Returns the LCS length and the mapping of polished-word
/// indices to input-word indices for matched words.
///
/// `matched_polished[i]` is `Some(input_idx)` when polished word `i` matches
/// an input word; `None` when it is an insertion.
fn lcs_word_diff(input: &[&str], polished: &[&str]) -> (usize, Vec<Option<usize>>) {
    let n = input.len();
    let m = polished.len();
    if n == 0 || m == 0 {
        return (0, vec![None; m]);
    }

    // Pre-compute bare forms for both vectors.
    let input_bare: Vec<String> = input.iter().map(|w| bare_lower(w)).collect();
    let polished_bare: Vec<String> = polished.iter().map(|w| bare_lower(w)).collect();

    // dp[i][j] = LCS length of input[i..] and polished[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            if input_bare[i] == polished_bare[j] {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    // Backtrack to build the mapping.
    let mut matched: Vec<Option<usize>> = vec![None; m];
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if input_bare[i] == polished_bare[j] {
            matched[j] = Some(i);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    (dp[0][0], matched)
}

// ── Guard result ─────────────────────────────────────────────────────────────

/// The outcome of the guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    /// The polished text is safe to use (possibly with protected tokens
    /// restored in place).
    Accept(String),
    /// The polished text was rejected; use the rules-only input.
    Reject { reason: String },
}

/// The maximum number of protected tokens that may be changed/removed before
/// the entire model output is rejected.
const MAX_PROTECTED_CHANGES: usize = 3;

/// Hallucination threshold: polished word count > 1.3 × input word count + 5.
fn is_too_long(input_words: usize, polished_words: usize) -> bool {
    polished_words > (input_words as f32 * 1.3 + 5.0) as usize
}

/// Greeting words excluded from the hallucination-phrase check.
const GREETINGS: &[&str] = &["hello", "hi", "hey"];

/// List-marker tokens excluded from the hallucination-phrase check.
const LIST_MARKERS: &[&str] = &["-", "•", "•", "1.", "2.", "3.", "4.", "5."];

/// Checks whether a sequence of > 6 consecutive words in `polished` is absent
/// from `input` (case-insensitive, punctuation-stripped).
fn has_hallucinated_phrase(input: &[&str], polished: &[&str]) -> bool {
    if polished.len() <= 6 {
        return false;
    }
    // Build a set of input words (lowercased, punctuation-stripped).
    let input_set: std::collections::HashSet<String> = input
        .iter()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();

    // Sliding window of 7 consecutive words in polished.
    let window = 7;
    if polished.len() < window {
        return false;
    }
    for start in 0..=(polished.len() - window) {
        let slice = &polished[start..start + window];
        // Exclude list markers and greetings from the window.
        let content: Vec<&str> = slice
            .iter()
            .filter(|w| {
                let bare = w
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                !LIST_MARKERS.contains(*w) && !GREETINGS.contains(&bare.as_str())
            })
            .copied()
            .collect();
        if content.len() < 6 {
            continue;
        }
        // If ALL content words in the window are absent from input, it is a
        // hallucinated phrase.
        let all_absent = content.iter().all(|w| {
            let bare = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            !input_set.contains(&bare)
        });
        if all_absent {
            return true;
        }
    }
    false
}

/// Restores protected tokens in the polished text, or rejects the output.
///
/// `input` is the original (rules-only) text. `polished` is the model output.
/// `protected` is the list of protected spans in `input` (from
/// [`collect_protected`]).
pub fn restore_or_reject(input: &str, polished: &str, protected: &[ProtectedSpan]) -> GuardResult {
    let input_words: Vec<&str> = input.split_whitespace().collect();
    let polished_words: Vec<&str> = polished.split_whitespace().collect();

    // Hallucination guard: too long.
    if is_too_long(input_words.len(), polished_words.len()) {
        return GuardResult::Reject {
            reason: format!(
                "hallucination: {} polished words > 1.3×{} + 5",
                polished_words.len(),
                input_words.len()
            ),
        };
    }

    // Hallucination guard: long absent phrase.
    if has_hallucinated_phrase(&input_words, &polished_words) {
        return GuardResult::Reject {
            reason: "hallucination: phrase absent from input".into(),
        };
    }

    if protected.is_empty() {
        return GuardResult::Accept(polished.to_string());
    }

    // LCS diff: map polished tokens back to input indices.
    let (_lcs_len, matched) = lcs_word_diff(&input_words, &polished_words);

    // For each protected span, find the polished token that maps to it.
    // A protected token is "changed" when an unmatched polished token sits
    // exactly at the span's input position (a substitution). It is "removed"
    // when no polished token maps to it at all. Changed tokens are restored in
    // place; a removed token rejects the whole output, because the model had
    // no word at that position to restore and reinsertion would either
    // duplicate a word or steal an adjacent insertion.
    let mut changes = 0usize;
    let mut restored: Vec<String> = polished_words.iter().map(|w| w.to_string()).collect();

    for span in protected {
        let mut found = None;
        for (pj, &mapped_i) in matched.iter().enumerate() {
            if mapped_i == Some(span.start) {
                found = Some(pj);
                break;
            }
        }
        match found {
            Some(pj) => {
                let polished_token = polished_words[pj];
                let original = span.word.as_str();
                if !bare_lower(polished_token).eq(&bare_lower(original)) {
                    let leading_punct: String = polished_token
                        .chars()
                        .take_while(|c| !c.is_alphanumeric())
                        .collect();
                    let trailing_punct: String = polished_token
                        .chars()
                        .rev()
                        .take_while(|c| !c.is_alphanumeric())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    restored[pj] = format!("{leading_punct}{original}{trailing_punct}");
                    changes += 1;
                }
            }
            None => {
                // The LCS didn't match this input token. Check if there's an
                // unmatched polished token near the span's position — that's
                // a substitution (the model changed the token). Otherwise it
                // was removed.
                let mut substituted = false;
                for (pj, &mapped_i) in matched.iter().enumerate() {
                    if mapped_i.is_none() {
                        // This polished token is an insertion/substitution.
                        // It is a substitution only when it sits exactly at
                        // the protected span's input position. A word one
                        // position early is an insertion before the span
                        // ("Please definitely ask ..."), not a replacement
                        // for the missing token; treating it as one steals
                        // the inserted word and loses it.
                        let matched_before: usize =
                            matched[..pj].iter().filter(|m| m.is_some()).count();
                        if matched_before == span.start {
                            let polished_token = polished_words[pj];
                            let original = span.word.as_str();
                            if !bare_lower(polished_token).eq(&bare_lower(original)) {
                                let leading_punct: String = polished_token
                                    .chars()
                                    .take_while(|c| !c.is_alphanumeric())
                                    .collect();
                                let trailing_punct: String = polished_token
                                    .chars()
                                    .rev()
                                    .take_while(|c| !c.is_alphanumeric())
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .rev()
                                    .collect();
                                restored[pj] = format!("{leading_punct}{original}{trailing_punct}");
                                changes += 1;
                                substituted = true;
                            }
                            break;
                        }
                    }
                }
                if !substituted {
                    // Removed: reject. The polished output has no word at
                    // this position, so there is nothing safe to restore
                    // into; reinserting was tried and could duplicate the
                    // word or steal an adjacent insertion. Rejecting returns
                    // the rules-only input, which still contains the
                    // protected token exactly.
                    return GuardResult::Reject {
                        reason: format!("protected token removed: {}", span.word),
                    };
                }
            }
        }
    }

    if changes > MAX_PROTECTED_CHANGES {
        return GuardResult::Reject {
            reason: format!("protected: {changes} tokens changed > {MAX_PROTECTED_CHANGES}"),
        };
    }

    if changes > 0 {
        tracing::info!(
            changes,
            "guard: restored protected tokens in polished output"
        );
    }

    GuardResult::Accept(restored.join(" "))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_protected_finds_numbers_urls_emails_code() {
        let input = "call 42 at https://example.com or dev@example.com or use parse_file or Kubernetes or API or main.rs";
        let spans = collect_protected(input, &[], &[]);
        let words: Vec<&str> = spans.iter().map(|s| s.word.as_str()).collect();
        assert!(words.contains(&"42"), "number: {words:?}");
        assert!(words.contains(&"https://example.com"), "url: {words:?}");
        assert!(words.contains(&"dev@example.com"), "email: {words:?}");
        assert!(words.contains(&"parse_file"), "snake_case: {words:?}");
        assert!(words.contains(&"Kubernetes"), "CamelCase: {words:?}");
        assert!(words.contains(&"API"), "ALL_CAPS: {words:?}");
        assert!(words.contains(&"main.rs"), "file ext: {words:?}");
    }

    #[test]
    fn collect_protected_finds_lexicon_terms() {
        let terms = vec!["Siobhan".to_string(), "Teletype".to_string()];
        let input = "Siobhan is using Teletype daily";
        let spans = collect_protected(input, &terms, &[]);
        let words: Vec<&str> = spans.iter().map(|s| s.word.as_str()).collect();
        assert!(words.contains(&"Siobhan"), "{words:?}");
        assert!(words.contains(&"Teletype"), "{words:?}");
    }

    #[test]
    fn collect_protected_finds_boost_terms() {
        let input = "the CherryPick feature is ready";
        let spans = collect_protected(input, &[], &["CherryPick"]);
        let words: Vec<&str> = spans.iter().map(|s| s.word.as_str()).collect();
        assert!(words.contains(&"CherryPick"), "{words:?}");
    }

    #[test]
    fn collect_protected_ignores_plain_words() {
        let input = "the quick brown fox";
        let spans = collect_protected(input, &[], &[]);
        assert!(spans.is_empty(), "{spans:?}");
    }

    #[test]
    fn name_not_fixable() {
        // "Siobhan" is protected; the model changes it to "Siobhan" (same) → no change.
        let terms = vec!["Siobhan".to_string()];
        let input = "Siobhan is here";
        let polished = "Siobhan is here";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(matches!(result, GuardResult::Accept(_)), "{result:?}");
    }

    #[test]
    fn name_restored_when_changed() {
        let terms = vec!["Siobhan".to_string()];
        let input = "Siobhan is here";
        // The model "corrects" the name.
        let polished = "Shivon is here";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(text.contains("Siobhan"), "name must be restored: {text}");
                assert!(
                    !text.contains("Shivon"),
                    "misheard name must not survive: {text}"
                );
            }
            other => panic!("expected Accept with restore, got {other:?}"),
        }
    }

    #[test]
    fn number_restored_when_changed() {
        let input = "call 42 now";
        let polished = "call 43 now";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(text.contains("42"), "number must be restored: {text}");
                assert!(
                    !text.contains("43"),
                    "changed number must not survive: {text}"
                );
            }
            other => panic!("expected Accept with restore, got {other:?}"),
        }
    }

    #[test]
    fn url_restored_when_changed() {
        let input = "visit https://example.com today";
        let polished = "visit https://example.org today";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(
                    text.contains("https://example.com"),
                    "URL must be restored: {text}"
                );
            }
            other => panic!("expected Accept with restore, got {other:?}"),
        }
    }

    #[test]
    fn email_restored_when_changed() {
        let input = "email dev@example.com";
        let polished = "email dev@example.org";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(
                    text.contains("dev@example.com"),
                    "email must be restored: {text}"
                );
            }
            other => panic!("expected Accept with restore, got {other:?}"),
        }
    }

    #[test]
    fn code_identifier_restored_when_changed() {
        let input = "use parse_file in main.rs";
        let polished = "use parsefile in main.rs";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(
                    text.contains("parse_file"),
                    "snake_case must be restored: {text}"
                );
            }
            other => panic!("expected Accept with restore, got {other:?}"),
        }
    }

    #[test]
    fn more_than_three_protected_changes_rejects() {
        let terms = vec![
            "Siobhan".to_string(),
            "Teletype".to_string(),
            "CherryPick".to_string(),
            "Kubernetes".to_string(),
        ];
        let input = "Siobhan Teletype CherryPick Kubernetes";
        // The model changes all four to genuinely different words (not
        // case-only variants).
        let polished = "Shivon Telltype CherryPicked Kuber";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "4 protected changes must reject: {result:?}"
        );
    }

    #[test]
    fn hallucinated_seven_word_phrase_rejects() {
        let input = "the file is in downloads";
        // A 7-word phrase entirely absent from the input.
        let polished =
            "the file is in downloads and the quarterly earnings report shows strong growth";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "7-word hallucinated phrase must reject: {result:?}"
        );
    }

    #[test]
    fn too_long_output_rejects() {
        let input = "hi";
        // 1.3 × 1 + 5 = 6.3 → 6. So > 6 words rejects.
        let polished = "one two three four five six seven";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "7 words from 1-word input must reject: {result:?}"
        );
    }

    #[test]
    fn benign_paraphrase_passes() {
        let input = "hello john can you send the proposal";
        let polished = "Hi John, could you send the proposal?";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Accept(_)),
            "benign paraphrase must pass: {result:?}"
        );
    }

    /// 50 benign paraphrase sentences must all pass the guard.
    #[test]
    fn benign_paraphrases_pass_50_sentences() {
        let cases: &[(&str, &str)] = &[
            ("the file is in downloads", "The file is in Downloads."),
            ("sounds good", "Sounds good."),
            ("on my way", "On my way."),
            ("yes let us do that", "Yes, let's do that."),
            (
                "the meeting is at three today",
                "The meeting is at 3 today.",
            ),
            ("i will call you later", "I will call you later."),
            ("thanks for the update", "Thanks for the update."),
            ("the report is ready", "The report is ready."),
            ("see you tomorrow", "See you tomorrow."),
            ("that works for me", "That works for me."),
            ("the deployment is complete", "The deployment is complete."),
            ("we should ship on friday", "We should ship on Friday."),
            ("the client wants a demo", "The client wants a demo."),
            (
                "i need the invoice by monday",
                "I need the invoice by Monday.",
            ),
            ("the server is down again", "The server is down again."),
            (
                "please review the pull request",
                "Please review the pull request.",
            ),
            ("the build failed on ci", "The build failed on CI."),
            (
                "can you pick up the dry cleaning",
                "Can you pick up the dry cleaning?",
            ),
            ("the presentation is at two", "The presentation is at 2."),
            ("i forgot the charger", "I forgot the charger."),
            ("the road is closed today", "The road is closed today."),
            ("we need more coffee", "We need more coffee."),
            ("the wifi is not working", "The Wi-Fi is not working."),
            ("please send the agenda", "Please send the agenda."),
            ("the train leaves at nine", "The train leaves at 9."),
            ("i am running late", "I am running late."),
            ("the package was delivered", "The package was delivered."),
            (
                "we should reschedule the call",
                "We should reschedule the call.",
            ),
            ("the airport is very busy", "The airport is very busy."),
            (
                "i will email you the details",
                "I will email you the details.",
            ),
            ("the store closes at ten", "The store closes at 10."),
            ("can you water the plants", "Can you water the plants?"),
            ("the concert starts at eight", "The concert starts at 8."),
            ("i need to buy some bread", "I need to buy some bread."),
            ("the meeting was cancelled", "The meeting was cancelled."),
            (
                "please confirm the reservation",
                "Please confirm the reservation.",
            ),
            ("the library opens at nine", "The library opens at 9."),
            ("i left my keys in the car", "I left my keys in the car."),
            (
                "the restaurant is fully booked",
                "The restaurant is fully booked.",
            ),
            (
                "we should leave before noon",
                "We should leave before noon.",
            ),
            ("the flight lands at six", "The flight lands at 6."),
            ("i will pick up the kids", "I will pick up the kids."),
            ("the gym is open until ten", "The gym is open until 10."),
            ("please forward the email", "Please forward the email."),
            ("the show starts at seven", "The show starts at 7."),
            (
                "i need to renew my passport",
                "I need to renew my passport.",
            ),
            (
                "the pharmacy is closed on sundays",
                "The pharmacy is closed on Sundays.",
            ),
            ("we can meet at the cafe", "We can meet at the cafe."),
            (
                "the train is delayed by an hour",
                "The train is delayed by an hour.",
            ),
        ];
        for (input, polished) in cases {
            let protected = collect_protected(input, &[], &[]);
            let result = restore_or_reject(input, polished, &protected);
            assert!(
                matches!(result, GuardResult::Accept(_)),
                "benign paraphrase must pass: input={input:?} polished={polished:?} got {result:?}"
            );
        }
    }

    #[test]
    fn case_only_change_is_not_a_protected_change() {
        // Sentence-initial lowercase word + a protected name. The polished
        // version capitalises the first word and keeps the name. The
        // capitalisation-only change must not be counted as a protected
        // change, and both words must be present in the output.
        let terms = vec!["Siobhan".to_string()];
        let input = "ask Siobhan to review the plan";
        let polished = "Ask Siobhan to review the plan.";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        match result {
            GuardResult::Accept(text) => {
                assert!(text.contains("Siobhan"), "name must be present: {text}");
                assert!(
                    text.to_lowercase().contains("ask"),
                    "verb must be present: {text}"
                );
            }
            other => panic!("case-only change must be accepted, got {other:?}"),
        }
    }

    #[test]
    fn removed_name_rejects() {
        // Stable lowercase prefix, name deleted.
        let terms = vec!["Siobhan".to_string()];
        let input = "please ask Siobhan to review the plan";
        let polished = "Please ask to review the plan.";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "a removed protected name must reject, got {result:?}"
        );
    }

    #[test]
    fn removed_number_rejects() {
        // Stable prefix, number deleted.
        let input = "call 42 now";
        let polished = "Call now.";
        let protected = collect_protected(input, &[], &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "a removed protected number must reject, got {result:?}"
        );
    }

    #[test]
    fn sentence_initial_case_plus_removed_name_does_not_displace() {
        // The exact G9 case: input `ask Siobhan to review the plan` →
        // `Ask to review the plan.`. The result must NOT be
        // `Siobhan to review the plan.` (which drops the verb). A removed
        // token has no safe output position, so reject.
        let terms = vec!["Siobhan".to_string()];
        let input = "ask Siobhan to review the plan";
        let polished = "Ask to review the plan.";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "removed name after case-only change must reject, got {result:?}"
        );
    }

    #[test]
    fn removed_name_after_insertion_rejects_instead_of_stealing_it() {
        // The model inserts "definitely" before the verb and drops the name.
        // "definitely" must not be treated as a substitution for "Siobhan":
        // doing that loses the inserted word and corrupts the sentence.
        let terms = vec!["Siobhan".to_string()];
        let input = "please ask Siobhan to review the plan";
        let polished = "Please definitely ask to review the plan.";
        let protected = collect_protected(input, &terms, &[]);
        let result = restore_or_reject(input, polished, &protected);
        assert!(
            matches!(result, GuardResult::Reject { .. }),
            "removed name after an insertion must reject, got {result:?}"
        );
    }

    #[test]
    fn lcs_word_diff_basic() {
        let input = vec!["a", "b", "c"];
        let polished = vec!["a", "x", "c", "d"];
        let (lcs, matched) = lcs_word_diff(&input, &polished);
        assert_eq!(lcs, 2, "LCS of [a,b,c] and [a,x,c,d] is 2");
        assert_eq!(matched, vec![Some(0), None, Some(2), None]);
    }

    #[test]
    fn lcs_word_diff_empty() {
        let (lcs, matched) = lcs_word_diff(&[], &["a", "b"]);
        assert_eq!(lcs, 0);
        assert_eq!(matched, vec![None, None]);
    }

    #[test]
    fn no_protected_tokens_accepts_polished() {
        let input = "the quick brown fox";
        let polished = "The quick brown fox.";
        let protected = collect_protected(input, &[], &[]);
        assert!(protected.is_empty());
        let result = restore_or_reject(input, polished, &protected);
        assert!(matches!(result, GuardResult::Accept(_)), "{result:?}");
    }
}
