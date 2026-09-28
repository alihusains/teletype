//! Validation of model output before it reaches the user.
//!
//! Catches the obvious failure modes of a small local model: empty output,
//! echoing the prompt or its own instructions, unwanted preambles, markdown
//! fences, and runaway expansion. On failure the pipeline falls back to the
//! original input — never to the model's raw output.
//!
//! NOTE: Commit 9163631 (2026-09-25) claims "Validator: expanded failure
//! taxonomy (language drift, truncation)" but the diff only adds the
//! `Truncated` variant. No language-drift check was implemented. This is
//! tracked as P1-17 in findings.md and remains future work.
//!
//! `Failure::OffTopic` (added 2026-09-28) is the one check that can express
//! "the model produced something unrelated to the input", which is what a
//! refusal and an off-topic answer both look like. It is a content-overlap
//! check, not a language check, so a genuine non-English rewrite is still
//! accepted.

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
    /// The output shares almost no content with the input: the model refused,
    /// answered a question instead of rewriting, or drifted to another
    /// language. Nothing in the taxonomy above can express this, so a refusal
    /// passed validation and was typed into the user's document in place of
    /// every word they had said.
    OffTopic,
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
            Failure::OffTopic => write!(f, "model returned unrelated text"),
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

    // The transform must not have dropped protected placeholders.
    if let Some(count) = placeholder_count(input) {
        if placeholder_count(cleaned).unwrap_or(0) != count {
            return ValidatedOutput::Fallback(input.to_string(), Some(Failure::PromptEcho));
        }
    }

    if let Some(reason) = off_topic_reason(cleaned, input) {
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
const OFF_TOPIC_MIN_SHARED: f64 = 0.34;

/// Inputs shorter than this are exempt: below it there are too few content
/// words for the ratio to mean anything, and a short utterance can legitimately
/// become a short answer.
const OFF_TOPIC_MIN_WORDS: usize = 4;

fn off_topic_reason(cleaned: &str, input: &str) -> Option<Failure> {
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
    if ratio < OFF_TOPIC_MIN_SHARED {
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
fn longest_common_substring(a: &str, b: &str) -> String {
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
}
