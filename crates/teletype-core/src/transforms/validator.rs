//! Validation of model output before it reaches the user.
//!
//! Catches the obvious failure modes of a small local model: empty output,
//! echoing the prompt or its own instructions, unwanted preambles, markdown
//! fences, and runaway expansion. On failure the pipeline falls back to the
//! original input — never to the model's raw output.
//!
//! Language drift is caught by `Failure::LanguageDrift` (added by U4b,
//! closing the gap P1-17 of commit 9163631 left open: that commit claimed
//! an "expanded failure taxonomy (language drift, truncation)" but only
//! added `Truncated`). The check is one-sided and script-based, not
//! language-based: when the input is Latin-script (English, German, French,
//! Spanish, accented Latin — the app's supported dictation languages) and
//! the output is not (`families::is_latin_script`), the output is rejected
//! and the pipeline falls back to the original. A non-Latin input (Japanese,
//! Chinese, Hindi, …) is never checked: the model is expected to stay in the
//! user's script, and this heuristic cannot tell one non-Latin language
//! from another.
//!
//! The `OffTopic` check is a separate content-overlap check, not a language
//! check: it catches refusals, off-topic answers, and English-but-unrelated
//! output. It cannot by itself catch a Latin-script rewrite into another
//! Latin language (French output for an English input), because the overlap
//! of content words can still be high; `LanguageDrift` is the one that
//! rejects a script change, and `OffTopic` is what still fires for
//! unrelated output.

use super::TransformDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Empty,
    PromptEcho,
    InstructionEcho,
    Preamble,
    MarkdownFence,
    MassivelyExpanded,
    /// Output is far shorter than a substantial input: the model hit its
    /// token cap and returned a fragment, not a full transform.
    Truncated,
    /// The input is Latin-script but the output is not (Cyrillic, CJK,
    /// Arabic, Devanagari, …): the model drifted to a script the user did
    /// not dictate in. Caught by `families::is_latin_script` on both sides;
    /// a non-Latin input is never checked (the model is expected to stay in
    /// the user's script, and the heuristic cannot tell one non-Latin
    /// language from another).
    LanguageDrift,
    /// The output shares almost no content with the input: the model
    /// refused, or answered a question instead of rewriting. This is a
    /// content-overlap check, not a language check: a Latin-script rewrite
    /// into another Latin language (French for an English input) can still
    /// share enough content words to pass here, and is caught by
    /// [`Failure::LanguageDrift`] only when the script itself changes.
    OffTopic,
    /// A genuine rewrite that still replaces more of the input than the
    /// user's change limit allows. Unlike [`Failure::OffTopic`], the output
    /// is on topic; it simply strayed further from what the user said than
    /// they asked the guard to accept.
    TooMuchChange,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Empty => write!(f, "model returned no text"),
            Failure::PromptEcho => write!(f, "model echoed the input"),
            Failure::InstructionEcho => write!(f, "model echoed the instructions"),
            Failure::Preamble => write!(f, "model added a preamble"),
            Failure::MarkdownFence => write!(f, "model wrapped output in markdown"),
            Failure::MassivelyExpanded => write!(f, "model expanded the text excessively"),
            Failure::Truncated => write!(f, "model returned a truncated fragment"),
            Failure::LanguageDrift => write!(f, "model output drifted to a different script"),
            Failure::OffTopic => write!(f, "model returned unrelated text"),
            Failure::TooMuchChange => write!(f, "model rewrote too much of the text"),
        }
    }
}

/// Output after validation: either cleaned model text, or the fallback.
#[derive(Debug, Clone)]
pub enum ValidatedOutput {
    Transformed(String),
    /// `reason` is `None` for a clean pass.
    Fallback(String, Option<Failure>),
}

const MAX_GROWTH: f64 = 4.0;
const MIN_INPUT_LEN_TO_CHECK_ECHO: usize = 24;
/// Above this input length, an output shorter than input/5 is treated as a
/// truncation. Short inputs legitimately shrink (filler removal), so the
/// floor only applies to substantial text.
const MIN_INPUT_FOR_TRUNCATION_CHECK: usize = 80;

const PREAMBLES: &[&str] = &[
    "here is",
    "here's",
    "sure",
    "of course",
    "certainly",
    "no problem",
    "the polished",
    "the revised",
    "the transformed",
    "the rewritten",
    "the professional",
    "the final",
    "changes made",
    "i hope this",
    "polished version",
    "revised version",
    "transformed version",
];

/// Validates `raw` model output for `transform` applied to `input`.
pub fn validate(
    raw: &str,
    input: &str,
    transform: &TransformDefinition,
    prompt: &str,
) -> ValidatedOutput {
    validate_with_off_topic_floor(raw, input, transform, prompt, OFF_TOPIC_MIN_SHARED)
}

/// Validation for an editor-style transform (the 2B Polish prompt), where
/// "make it more concise" legitimately removes most of the filler words that
/// the default overlap floor counts as content. A refusal or an answer still
/// shares almost nothing with the input, so the floor moves down rather than
/// disappearing.
pub fn validate_editor(
    raw: &str,
    input: &str,
    transform: &TransformDefinition,
    prompt: &str,
) -> ValidatedOutput {
    validate_with_off_topic_floor(raw, input, transform, prompt, OFF_TOPIC_MIN_SHARED_EDITOR)
}

/// Default ceiling on how much of a polished take may differ from the input,
/// as a percentage of its words. The user can change it in Settings; 100 (or
/// more) disables the ceiling.
pub const DEFAULT_MAX_CHANGE_PERCENT: u8 = 40;

/// The Polish path: the default off-topic floor plus the user's change
/// ceiling. A rewrite that stays on topic but replaces more than
/// `max_change_percent` of the words falls back to the input instead.
pub fn validate_polish(
    raw: &str,
    input: &str,
    transform: &TransformDefinition,
    prompt: &str,
    max_change_percent: u8,
) -> ValidatedOutput {
    enforce_change_limit(
        validate(raw, input, transform, prompt),
        input,
        max_change_percent,
    )
}

/// [`validate_polish`] with the editor's lower off-topic floor.
pub fn validate_editor_polish(
    raw: &str,
    input: &str,
    transform: &TransformDefinition,
    prompt: &str,
    max_change_percent: u8,
) -> ValidatedOutput {
    enforce_change_limit(
        validate_editor(raw, input, transform, prompt),
        input,
        max_change_percent,
    )
}

fn enforce_change_limit(
    out: ValidatedOutput,
    input: &str,
    max_change_percent: u8,
) -> ValidatedOutput {
    let ValidatedOutput::Transformed(text) = &out else {
        return out;
    };
    if max_change_percent >= 100 {
        return out;
    }
    if content_change_share(input, text) > f64::from(max_change_percent) / 100.0 {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::TooMuchChange));
    }
    out
}

/// Share of the larger text that did not survive the rewrite, 0.0-1.0.
///
/// Words are compared case- and punctuation-insensitively as a multiset, so
/// fixing capitalisation or a comma costs nothing while replacing a word
/// costs its share. Order and line breaks are free: moving a clause is a
/// legitimate edit, and this guard is about how much of the user's wording
/// was replaced, not where it ended up.
///
/// Every substitution counts, including grammatical ones (`a` → `an`). This
/// is the strict accounting; [`content_change_share`] is what the user's
/// change ceiling uses, because a grammar fix is not a rewrite.
pub fn change_share(input: &str, output: &str) -> f64 {
    let mut before = words(input);
    let mut after = words(output);
    let larger = before.len().max(after.len());
    if larger == 0 {
        return 0.0;
    }
    before.sort_unstable();
    after.sort_unstable();
    let (mut i, mut j, mut shared) = (0usize, 0usize, 0usize);
    while i < before.len() && j < after.len() {
        match before[i].cmp(&after[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    1.0 - shared as f64 / larger as f64
}

/// Like [`change_share`], but a substitution between two function words that
/// sit in the same [`families::FAMILIES`] family or are near-spellings of
/// each other (`a` → `an`, `is` → `are`, `dont` → `don't`) is a *grammar*
/// edit and does not count against the user's change ceiling. Content
/// substitutions, insertions and deletions keep counting exactly as in
/// [`change_share`].
///
/// Monotone: `content_change_share(a, b) <= change_share(a, b)` for all
/// inputs, so an output accepted under the old accounting is still accepted
/// under this one, and a pure-grammar fix that the old accounting rejected
/// can now pass.
pub fn content_change_share(input: &str, output: &str) -> f64 {
    let before = words(input);
    let after = words(output);
    let larger = before.len().max(after.len());
    if larger == 0 {
        return 0.0;
    }
    let families = super::families::FamilyMap::new();
    let mut bag_in: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut bag_out: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for w in &before {
        *bag_in.entry(w.clone()).or_insert(0) += 1;
    }
    for w in &after {
        *bag_out.entry(w.clone()).or_insert(0) += 1;
    }
    let mut matched: usize = 0;
    // Pass 1: exact matches, as in change_share.
    for (w, n) in &bag_in {
        matched += (*n).min(*bag_out.get(w).unwrap_or(&0));
    }
    // Pass 2: grammatical substitutions. For each input word still unmatched,
    // pair it with an unmatched output word in the same family (or a
    // near-spelling of it) when both are function words.
    let mut pending_in: Vec<String> = Vec::new();
    for (w, n) in &bag_in {
        let rem = n.saturating_sub(*bag_out.get(w).unwrap_or(&0));
        pending_in.extend(std::iter::repeat_n(w.clone(), rem));
    }
    let mut pending_out: Vec<String> = Vec::new();
    for (w, n) in &bag_out {
        let rem = n.saturating_sub(*bag_in.get(w).unwrap_or(&0));
        pending_out.extend(std::iter::repeat_n(w.clone(), rem));
    }
    // HashMap iteration order is randomised per process; the greedy pairing
    // below must not make the guard's verdict vary run to run.
    pending_in.sort();
    pending_out.sort();
    for w in &pending_in {
        if let Some(pos) = pending_out
            .iter()
            .position(|o| super::families::is_grammar_edit(w, o, &families))
        {
            pending_out.remove(pos);
            matched += 1;
        }
    }
    1.0 - matched as f64 / larger as f64
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn validate_with_off_topic_floor(
    raw: &str,
    input: &str,
    transform: &TransformDefinition,
    prompt: &str,
    off_topic_floor: f64,
) -> ValidatedOutput {
    // Check for markdown fence BEFORE stripping, since strip_wrapping removes it.
    let raw_trimmed = raw.trim();
    if raw_trimmed.starts_with("```") || raw_trimmed.ends_with("```") {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::MarkdownFence));
    }

    let cleaned = strip_wrapping(raw);
    let cleaned = cleaned.trim();

    if cleaned.is_empty() {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::Empty));
    }

    // Prompt / instruction echo: the output contains a sizeable chunk of the
    // system prompt or the input verbatim.
    if cleaned.len() > MIN_INPUT_LEN_TO_CHECK_ECHO {
        let lower = cleaned.to_ascii_lowercase();
        for probe in [
            "you are a text transformation engine",
            "output contract",
            "text to transform",
            "rules that override",
        ] {
            if lower.contains(probe) {
                return ValidatedOutput::Fallback(
                    input.to_string(),
                    Some(Failure::InstructionEcho),
                );
            }
        }
        let echo = longest_common_substring(input, cleaned);
        if echo.len() as f64 > cleaned.len() as f64 * 0.9
            && echo.len() as f64 > input.len() as f64 * 0.7
        {
            // A no-op is not an echo. When the model returns the input
            // essentially unchanged, it is agreeing the text is already clean,
            // which is a valid pass-through, not a failure. Only treat it as
            // an echo when the output is a *substring* of the input (the model
            // dropped content and parroted part of it back) — that is the real
            // failure. An output that covers at least 90% of the input's own
            // length is a faithful (possibly lightly polished) restatement.
            let covers_most_of_input = cleaned.len() as f64 >= input.len() as f64 * 0.9;
            if !covers_most_of_input {
                return ValidatedOutput::Fallback(input.to_string(), Some(Failure::PromptEcho));
            }
        }
    }

    if let Some((_start, end)) = find_preamble(cleaned) {
        // A preamble is a failure when the model added an introductory
        // phrase. User text that happens to start with "Sure" or "Here is"
        // as its own content is NOT a preamble.
        let rest = &cleaned[end..].trim_start();
        let preamble_ends_with_colon = cleaned[..end].ends_with(':');
        let rest_starts_with_label = rest.starts_with(['"', '\'', '-', ':', '(', '“']);
        if rest_starts_with_label || (preamble_ends_with_colon && !rest.is_empty()) {
            return ValidatedOutput::Fallback(input.to_string(), Some(Failure::Preamble));
        }
    }

    if cleaned.starts_with("```") || cleaned.ends_with("```") {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::MarkdownFence));
    }

    if input.len() > 20 && cleaned.len() as f64 > input.len() as f64 * MAX_GROWTH {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::MassivelyExpanded));
    }

    // Truncation floor: a substantial input that came back a fifth of its
    // size is a token-cap fragment, not a real transform. The reference app
    // uses the same 1/5 ratio with the same 80-char input threshold.
    if input.len() >= MIN_INPUT_FOR_TRUNCATION_CHECK && cleaned.len() * 5 < input.len() {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::Truncated));
    }

    // Language drift: the user dictated in a Latin script and the model came
    // back in another script (Cyrillic, CJK, Arabic, Devanagari, …). One-
    // sided on purpose: a non-Latin input is never checked, because the
    // model is expected to stay in the user's script and the heuristic
    // cannot tell one non-Latin language from another. Accented Latin
    // ("Café au lait", "C'est fini") is Latin-script and passes.
    if super::families::is_latin_script(input) && !super::families::is_latin_script(cleaned) {
        return ValidatedOutput::Fallback(input.to_string(), Some(Failure::LanguageDrift));
    }

    // The transform must not have dropped protected placeholders.
    if let Some(count) = placeholder_count(input) {
        if placeholder_count(cleaned).unwrap_or(0) != count {
            return ValidatedOutput::Fallback(input.to_string(), Some(Failure::PromptEcho));
        }
    }

    if let Some(reason) = off_topic_reason(cleaned, input, off_topic_floor) {
        return ValidatedOutput::Fallback(input.to_string(), Some(reason));
    }

    let _ = (prompt, transform);
    ValidatedOutput::Transformed(cleaned.to_string())
}

/// Content words the check below should look for, ignoring AutoText
/// placeholders (whose value is not the user's words) and single characters.
fn content_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .collect()
}

/// True when the model produced text with essentially nothing in common with
/// what the user said.
///
/// A rewrite is allowed to add, remove and reorder: "um so the deploy went
/// out" can become "The deploy has gone out." What it may not do is answer a
/// different question. Without this check a refusal ("I'm sorry, I can't
/// help with that") or an off-topic answer passed every other rule and
/// replaced the user's dictation outright.
///
/// The threshold is deliberately low. A legitimate rewrite can drop a
/// connective ("so", "and") or compress hard, but it cannot plausibly discard
/// most of the content, so requiring a third of the input's content words to
/// survive costs nothing on real output and catches every refusal observed.
/// The editor path (`validate_editor`) uses a lower floor because conciseness
/// is an explicit instruction there.
const OFF_TOPIC_MIN_SHARED: f64 = 0.34;

/// The editor has licence to compress hard: "um so basically i think we should
/// probably move the meeting" can lose most of its content words while keeping
/// the only fact. A refusal or an answer still shares almost nothing, so this
/// floor stays above zero.
const OFF_TOPIC_MIN_SHARED_EDITOR: f64 = 0.15;

/// Inputs shorter than this are exempt: below it there are too few content
/// words for the ratio to mean anything, and a short utterance can legitimately
/// become a short answer.
const OFF_TOPIC_MIN_WORDS: usize = 4;

fn off_topic_reason(cleaned: &str, input: &str, min_shared: f64) -> Option<Failure> {
    let in_words = content_words(input);
    if in_words.len() < OFF_TOPIC_MIN_WORDS {
        return None;
    }
    // Count each input content word once, however often it appears in the
    // output, so a model that repeats one word cannot fake the ratio.
    let out_all: std::collections::HashSet<String> = content_words(cleaned).into_iter().collect();
    let unique_in: std::collections::HashSet<&String> = in_words.iter().collect();
    let shared = unique_in.iter().filter(|w| out_all.contains(**w)).count();
    let ratio = shared as f64 / unique_in.len() as f64;
    if ratio < min_shared {
        Some(Failure::OffTopic)
    } else {
        None
    }
}

fn strip_wrapping(raw: &str) -> String {
    let mut s = raw.trim();
    // Single markdown fence wrapping the whole output.
    if s.starts_with("```") {
        if let Some(nl) = s.find('\n') {
            s = &s[nl + 1..];
        }
        if let Some(end) = s.rfind("```") {
            s = &s[..end];
        }
        s = s.trim();
    }
    // Quoted output.
    if s.len() >= 2
        && (s.starts_with('"') && s.ends_with('"') || s.starts_with('“') && s.ends_with('”'))
    {
        s = s.trim_matches(|c| matches!(c, '"' | '“' | '”'));
    }
    // The generic prompt wraps the input in `<<<` / `>>>`. A model that
    // echoes the wrapper would otherwise pass validation with the delimiter
    // glued onto the user's text.
    s = s.strip_prefix("<<<").map(str::trim_start).unwrap_or(s);
    s = s.strip_suffix(">>>").map(str::trim_end).unwrap_or(s);
    s.to_string()
}

fn find_preamble(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for p in PREAMBLES {
        if let Some(start) = lower.starts_with(p).then_some(0usize).or_else(|| {
            lower
                .find(p)
                .filter(|pos| *pos < 60 && lower[..*pos].chars().all(|c| c.is_whitespace()))
        }) {
            // The preamble must end at the first sentence boundary.
            let tail = &lower[start..];
            let end = tail
                .find(['.', ':', '!', '?'])
                .map(|i| start + i + 1)
                .unwrap_or(tail.len());
            if end <= 120 {
                best = Some((start, end));
                break;
            }
        }
    }
    best
}

fn placeholder_count(text: &str) -> Option<usize> {
    if !text.contains("{{AUTOTEXT_") {
        return None;
    }
    Some(text.matches("{{AUTOTEXT_").count())
}

/// Length of the longest common substring, capped early for speed.
pub(crate) fn longest_common_substring(a: &str, b: &str) -> String {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return String::new();
    }
    let mut best_len = 0usize;
    let mut best_end = 0usize;
    let mut prev = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        let mut cur = vec![0usize; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                cur[j] = prev[j - 1] + 1;
                if cur[j] > best_len {
                    best_len = cur[j];
                    best_end = i;
                }
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    a[best_end - best_len..best_end].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform() -> TransformDefinition {
        let mut t = TransformDefinition::new("Polish", "d", "Fix grammar.");
        t.id = "builtin-polish".into();
        t
    }

    fn prompt(input: &str) -> String {
        crate::transforms::prompt::build_prompt(
            &transform(),
            input,
            &crate::transforms::prompt::PromptContext::default(),
        )
    }

    #[test]
    fn accepts_clean_output() {
        let input = "hey john can you send the proposal";
        let out = validate(
            "Hi John, can you send the proposal?",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn rejects_empty() {
        let out = validate("   \n", "some input", &transform(), &prompt("some input"));
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::Empty))
        ));
    }

    #[test]
    fn rejects_preamble() {
        let out = validate(
            "Here is the polished version: Hi John",
            "hey john",
            &transform(),
            &prompt("hey john"),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::Preamble))
        ));
    }

    #[test]
    fn rejects_markdown_fence() {
        let out = validate(
            "```\nHi John\n```",
            "hey john",
            &transform(),
            &prompt("hey john"),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::MarkdownFence))
        ));
    }

    #[test]
    fn rejects_instruction_echo() {
        let out = validate(
            "You are a text transformation engine embedded in a desktop dictation app.",
            "hey john",
            &transform(),
            &prompt("hey john"),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::InstructionEcho))
        ));
    }

    #[test]
    fn rejects_massive_expansion() {
        let input = "send the report to finance by friday";
        let out = validate(
            &format!("{input} {}", "extra words ".repeat(60)),
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::MassivelyExpanded))
        ));
    }

    #[test]
    fn rejects_truncated_fragment_on_substantial_input() {
        let input = "Please send the quarterly financial report to the board no later than next Friday morning so they have time to review it before the meeting on Monday, and cc the finance team as well.";
        assert!(input.len() >= MIN_INPUT_FOR_TRUNCATION_CHECK);
        let out = validate(
            "Please send the report.",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::Truncated))
        ));
    }

    #[test]
    fn accepts_short_output_for_short_input() {
        // Below the 80-char threshold, shrinkage is legitimate (filler removal).
        let input = "um so basically i need the thing done by friday";
        let out = validate(
            "I need the thing done by Friday.",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn accepts_noop_on_already_clean_input() {
        // A short, clean sentence the model returns essentially unchanged is a
        // valid pass-through, not a prompt echo. This is the case that made
        // short dictations always fall back to raw.
        let input = "The weather is very hot today.";
        let out = validate(input, input, &transform(), &prompt(input));
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn accepts_lightly_polished_input() {
        // Model adds a period and capitalisation to an already-fine sentence.
        let input = "the weather is very hot today";
        let out = validate(
            "The weather is very hot today.",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn editor_floor_accepts_aggressive_compression_but_still_rejects_off_topic() {
        // The editor is told to make the text more concise, so a good edit can
        // drop most of the filler that the default floor counts as content.
        // Kept under 175 chars: on a longer input the truncation floor fires
        // first and this test would pass for the wrong reason.
        let input = "um so basically i think we should uh probably move the meeting to friday at like 2 pm because uh the current time doesn't work for everyone";
        let concise = "Move the meeting to Friday at 2 PM.";
        let off_topic = "The weather in Paris is sunny today.";
        let p = prompt(input);

        assert!(
            matches!(
                validate(concise, input, &transform(), &p),
                ValidatedOutput::Fallback(_, Some(Failure::OffTopic))
            ),
            "the default floor should keep rejecting this compression"
        );
        assert!(
            matches!(
                validate_editor(concise, input, &transform(), &p),
                ValidatedOutput::Transformed(_)
            ),
            "the editor floor should accept a factual concise rewrite"
        );
        assert!(
            matches!(
                validate_editor(off_topic, input, &transform(), &p),
                ValidatedOutput::Fallback(_, Some(Failure::OffTopic))
            ),
            "a lower floor must not accept an unrelated answer"
        );
    }

    #[test]
    fn still_rejects_truncated_fragment() {
        // The model returns a fragment well under a fifth of a long input: a
        // token-cap truncation, not a transform. Must fall back.
        let input = "The quarterly financial report needs to go to the board before Monday's meeting and the finance team should be copied on it as well for the record and the legal team too.";
        let out = validate(
            "The quarterly financial report.",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(
            out,
            ValidatedOutput::Fallback(_, Some(Failure::Truncated))
        ));
    }

    #[test]
    fn rejects_dropped_placeholder() {
        let input = "send to {{AUTOTEXT_0}}";
        let out = validate("send to the address", input, &transform(), &prompt(input));
        assert!(matches!(out, ValidatedOutput::Fallback(_, Some(_))));
    }

    #[test]
    fn accepts_kept_placeholder() {
        let input = "send to {{AUTOTEXT_0}} today";
        let out = validate(
            "Send it to {{AUTOTEXT_0}} today.",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn user_text_starting_with_sure_is_not_a_preamble() {
        let out = validate(
            "Sure, I can do that by Friday.",
            "sure i can do that",
            &transform(),
            &prompt("sure i can do that"),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)));
    }

    #[test]
    fn strips_echoed_input_delimiters() {
        let input = "they was ready";
        let out = validate(
            &format!("{input}\n>>>"),
            input,
            &transform(),
            &prompt(input),
        );
        match out {
            ValidatedOutput::Transformed(text) => assert_eq!(text, input),
            ValidatedOutput::Fallback(_, reason) => panic!("unexpected fallback: {reason:?}"),
        }
    }

    // ── Language drift (Failure::LanguageDrift) ──────────────────────────────

    #[test]
    fn rejects_latin_input_with_non_latin_output() {
        // English input, French output: French is Latin-script, so
        // LanguageDrift does NOT fire — this is caught by OffTopic if the
        // overlap is low enough, or passes if it's high. The script check
        // only fires when the output leaves the Latin script entirely.
        let input = "hey john can you send the proposal by friday";
        // Cyrillic output (model drifted to Russian): LanguageDrift fires.
        let out = validate(
            "Привет Джон, можете ли вы отправить предложение?",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(
            matches!(
                out,
                ValidatedOutput::Fallback(_, Some(Failure::LanguageDrift))
            ),
            "{out:?}"
        );
    }

    #[test]
    fn rejects_latin_input_with_cjk_output() {
        let input = "send the report to finance by friday";
        let out = validate(
            "请把报告在星期五之前发给财务部",
            input,
            &transform(),
            &prompt(input),
        );
        assert!(
            matches!(
                out,
                ValidatedOutput::Fallback(_, Some(Failure::LanguageDrift))
            ),
            "{out:?}"
        );
    }

    #[test]
    fn accepts_non_latin_input_with_non_latin_output() {
        // A non-Latin input is never checked: the model is expected to stay
        // in the user's script. Japanese input, Japanese output passes the
        // language-drift check (it may still fail OffTopic on content overlap,
        // but that's a separate check).
        let input = "金曜までに財務部にレポートを送ってください";
        let out = validate(
            "金曜までに財務部にレポートを送ってください。",
            input,
            &transform(),
            &prompt(input),
        );
        // The output is essentially the input (a no-op with a period), so it
        // passes the prompt-echo check (covers ≥ 90% of input length) and the
        // language-drift check (input is not Latin, so the check is skipped).
        assert!(matches!(out, ValidatedOutput::Transformed(_)), "{out:?}");
    }

    #[test]
    fn accepts_accented_latin_output() {
        // Accented Latin (French, Spanish, Portuguese) is still Latin-script
        // and must not trigger LanguageDrift.
        let input = "cafe au lait is my favourite breakfast";
        let out = validate(
            "Café au lait est mon petit-déjeuner préféré.",
            input,
            &transform(),
            &prompt(input),
        );
        // This may fail OffTopic (content overlap) but must NOT fail
        // LanguageDrift.
        match out {
            ValidatedOutput::Transformed(_) => {}
            ValidatedOutput::Fallback(_, Some(Failure::LanguageDrift)) => {
                panic!("accented Latin output must not trigger LanguageDrift")
            }
            ValidatedOutput::Fallback(_, other) => {
                // Other failures (OffTopic) are acceptable here.
                assert!(other.is_some(), "expected a specific failure, got none");
            }
        }
    }

    #[test]
    fn accepts_german_input_with_german_output() {
        // A German user whose output is correctly German must not be rejected
        // by LanguageDrift. German is Latin-script on both sides.
        let input = "kannst du den vorschlag bis freitag schicken";
        let out = validate(
            "Kannst du den Vorschlag bis Freitag schicken?",
            input,
            &transform(),
            &prompt(input),
        );
        match out {
            ValidatedOutput::Transformed(_) => {}
            ValidatedOutput::Fallback(_, Some(Failure::LanguageDrift)) => {
                panic!("German output must not trigger LanguageDrift")
            }
            ValidatedOutput::Fallback(_, _) => {
                // OffTopic or other failures are acceptable; LanguageDrift is not.
            }
        }
    }

    // ── Family-aware change accounting ─────────────────────────────────────

    #[test]
    fn content_change_share_ignores_grammar_edits() {
        // "a" → "an" is a grammatical edit within the same family:
        // content_change_share should be 0.0 (no content words changed),
        // while change_share counts it as 1/6 ≈ 0.1667.
        let input = "i saw a movie last night";
        let output = "I saw an movie last night.";
        assert!(
            (change_share(input, output) - 1.0 / 6.0).abs() < 1e-9,
            "strict accounting counts a→an, got {}",
            change_share(input, output)
        );
        assert_eq!(
            content_change_share(input, output),
            0.0,
            "grammar edit must not count against the ceiling"
        );
    }

    #[test]
    fn content_change_share_counts_content_edits() {
        // Three content words changed: report→proposal, finance→legal, friday→monday.
        // These are not in the same family and are not near-spellings.
        let input = "send the report to finance by friday";
        let output = "send the proposal to legal by monday";
        // 3 of 7 words changed (report, finance, friday) → 3/7 ≈ 0.4286
        let share = content_change_share(input, output);
        assert!(
            (share - 3.0 / 7.0).abs() < 1e-9,
            "expected 3/7, got {share}"
        );
    }

    #[test]
    fn content_change_share_is_monotone() {
        // content_change_share <= change_share for all inputs.
        let pairs: &[(&str, &str)] = &[
            ("i saw a movie", "I saw an movie"),
            ("send the report to finance", "send the proposal to legal"),
            (
                "hey john can you send the proposal",
                "Hi John, can you send the proposal?",
            ),
            ("dont worry about it", "Don't worry about it."),
            ("alpha bravo charlie delta", "alpha bravo mike november"),
        ];
        for (a, b) in pairs {
            let strict = change_share(a, b);
            let relaxed = content_change_share(a, b);
            assert!(
                relaxed <= strict + 1e-9,
                "content_change_share({a:?}, {b:?}) = {relaxed} > change_share = {strict}"
            );
        }
    }

    #[test]
    fn polish_cap_accepts_pure_grammar_fix() {
        // A polish that only fixes "a" → "an" (or similar) should pass the
        // change ceiling even at a low threshold, because grammar edits are
        // free.
        let input = "i saw a movie last night and it was a good one";
        let output = "I saw an movie last night and it was a good one.";
        // change_share: 2/10 = 0.2 (a→an twice)
        // content_change_share: 0.0 (both are grammar edits)
        let out = validate_polish(
            output,
            input,
            &transform(),
            &prompt(input),
            10, // very tight ceiling: 10%
        );
        assert!(
            matches!(out, ValidatedOutput::Transformed(_)),
            "pure-grammar fix must pass a 10% ceiling: {out:?}"
        );
    }

    #[test]
    fn polish_cap_still_rejects_content_rewrite() {
        // A rewrite that changes three content words must still be rejected
        // at a tight ceiling.
        let input = "send the report to finance by friday";
        let output = "send the proposal to legal by monday";
        let out = validate_polish(
            output,
            input,
            &transform(),
            &prompt(input),
            30, // 30% ceiling; 3/7 ≈ 43% content change
        );
        assert!(
            matches!(
                out,
                ValidatedOutput::Fallback(_, Some(Failure::TooMuchChange))
            ),
            "content rewrite must still be rejected: {out:?}"
        );
    }

    #[test]
    fn change_share_ignores_case_and_punctuation() {
        assert_eq!(
            change_share("hi john can you send it", "Hi John, can you send it?"),
            0.0
        );
    }

    #[test]
    fn change_share_counts_replaced_words() {
        // Five words each; only "the" and "to" survive -> 3/5 replaced.
        let share = change_share(
            "send the report to finance",
            "email the document to accounting",
        );
        assert!((share - 0.6).abs() < 1e-9, "share = {share}");
    }

    #[test]
    fn change_share_is_one_for_unrelated_text() {
        assert_eq!(change_share("alpha beta gamma", "one two three"), 1.0);
    }

    #[test]
    fn change_share_of_empty_text_is_zero() {
        assert_eq!(change_share("", ""), 0.0);
        assert_eq!(change_share("", "word"), 1.0);
    }

    /// On-topic (40% of content words survive, so the off-topic floor
    /// passes) but more than 40% of the words are replaced.
    const STRAYING_INPUT: &str = "alpha bravo charlie delta echo foxtrot golf hotel india juliet";
    const STRAYING_OUTPUT: &str = "alpha bravo charlie delta mike november oscar papa";

    #[test]
    fn polish_cap_rejects_a_rewrite_over_the_limit() {
        let out = validate_polish(
            STRAYING_OUTPUT,
            STRAYING_INPUT,
            &transform(),
            &prompt(STRAYING_INPUT),
            40,
        );
        assert!(
            matches!(
                out,
                ValidatedOutput::Fallback(_, Some(Failure::TooMuchChange))
            ),
            "{out:?}"
        );
    }

    #[test]
    fn polish_cap_accepts_the_same_rewrite_at_the_boundary() {
        // 60% replaced; a 60% ceiling allows it because the rule is "more
        // than", not "at least".
        let out = validate_polish(
            STRAYING_OUTPUT,
            STRAYING_INPUT,
            &transform(),
            &prompt(STRAYING_INPUT),
            60,
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)), "{out:?}");
    }

    #[test]
    fn polish_cap_of_100_disables_the_check() {
        let out = validate_polish(
            STRAYING_OUTPUT,
            STRAYING_INPUT,
            &transform(),
            &prompt(STRAYING_INPUT),
            100,
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)), "{out:?}");
    }

    #[test]
    fn polish_cap_accepts_light_edits() {
        let input = "hey john can you send the proposal by friday";
        let out = validate_polish(
            "Hi John, can you send the proposal by Friday?",
            input,
            &transform(),
            &prompt(input),
            40,
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)), "{out:?}");
    }

    #[test]
    fn editor_polish_cap_keeps_the_editor_floor() {
        // A disabled ceiling must not turn the editor into a text acceptor:
        // an unrelated answer still fails the off-topic floor.
        let input = "um so basically i think we should uh probably move the meeting to friday at like 2 pm because uh the current time doesn't work for everyone";
        let out = validate_editor_polish(
            "The weather in Paris is sunny today.",
            input,
            &transform(),
            &prompt(input),
            100,
        );
        assert!(
            matches!(out, ValidatedOutput::Fallback(_, Some(Failure::OffTopic))),
            "{out:?}"
        );
    }

    #[test]
    fn uncapped_validate_still_accepts_a_straying_rewrite() {
        // The ceiling is opt-in on the polish path; other transforms keep the
        // old behaviour.
        let out = validate(
            STRAYING_OUTPUT,
            STRAYING_INPUT,
            &transform(),
            &prompt(STRAYING_INPUT),
        );
        assert!(matches!(out, ValidatedOutput::Transformed(_)), "{out:?}");
    }
}
