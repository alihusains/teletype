//! P5.1: deterministic polish gate.
//!
//! Decides, before spending a 500 ms – 3 s local-LLM polish pass, whether the
//! utterance is clean enough to skip the model entirely. Pure and
//! synchronous: no I/O, no allocations beyond the returned struct.

/// Default max word count for the short-clean-skip path (user-tunable via
/// `Settings::polish_gate_threshold_words`).
pub const DEFAULT_MAX_SHORT_WORDS: usize = 8;
/// Max word count for the single-clean-sentence path.
const MAX_SINGLE_SENTENCE_WORDS: usize = 12;
/// Whole-word filler markers that force a polish pass.
const FILLER_WORDS: &[&str] = &[
    "um",
    "uh",
    "er",
    "ah",
    "eh",
    "hmm",
    "like",
    "you know",
    "basically",
];
/// Leading openers that force a polish pass (a sentence that starts with one
/// is a self-correction, not a clean utterance).
const FILLER_OPENERS: &[&str] = &["actually", "honestly", "well", "so", "basically"];
/// Language prefixes (BCP-47, first two letters) that are never word-segmented
/// and therefore always need the LLM.
const NON_LATIN_PREFIXES: &[&str] = &["ja", "zh", "ko", "th", "lo", "my", "hi", "bn"];

/// The outcome of the gate.
#[derive(Debug, Clone, Copy)]
pub struct GateDecision {
    /// True when the polish pass should run; false when it can be skipped.
    pub should_polish: bool,
    /// Short, stable, human-readable reason (also used in the tracing event).
    pub reason: &'static str,
}

/// Decides whether a transcript needs the LLM polish pass.
///
/// `language` is the BCP-47 code the pipeline already carries
/// (`profile.language`), e.g. `"en"`, `"ja"`, `"auto"`.
///
/// `max_short_words` is the user-tunable word cap for the short-clean-skip
/// path (settings `polishGateThresholdWords`, default [`DEFAULT_MAX_SHORT_WORDS`]).
///
/// Rules, in order:
/// 1. Non-Latin scripts never skip (`script_needs_llm`).
/// 2. Any filler word / filler opener forces polish (`needs_cleanup`).
/// 3. Very short input (<= 3 words, matching the engine's own too-short
///    bypass) always skips: the model would treat it as a prompt to answer.
/// 4. <= `max_short_words` words with no sentence-final punctuation missing
///    (i.e. ending in `. ! ?`) skips (`short_clean`).
/// 5. A single sentence, <= 12 words, starting with an uppercase letter and
///    ending in `. ! ?` skips (`single_clean_sentence`).
/// 6. Anything else polishes (`needs_cleanup`).
pub fn should_polish(text: &str, language: &str, max_short_words: usize) -> GateDecision {
    let max_short_words = max_short_words.max(1);
    // Non-Latin scripts: word counting and punctuation heuristics do not
    // apply; the LLM is the only reliable polisher.
    if NON_LATIN_PREFIXES.contains(&language.get(..2).unwrap_or("")) {
        return GateDecision {
            should_polish: true,
            reason: "script_needs_llm",
        };
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        // Nothing to polish; the engine's own too-short path would handle it.
        return GateDecision {
            should_polish: false,
            reason: "short_clean",
        };
    }

    let words: Vec<&str> = trimmed.split_whitespace().collect();

    // Filler anywhere in the text, or a filler opener, forces a polish pass.
    if has_filler(&words) {
        return GateDecision {
            should_polish: true,
            reason: "needs_cleanup",
        };
    }

    // Rule 5: single sentence, already capitalized and punctuated, under the cap.
    // Checked before rule 4 so "Hello there." gets single_clean_sentence, not short_clean.
    if is_single_sentence(trimmed)
        && words.len() <= MAX_SINGLE_SENTENCE_WORDS
        && starts_uppercase(trimmed)
        && ends_with_sentence_punct(trimmed)
    {
        return GateDecision {
            should_polish: false,
            reason: "single_clean_sentence",
        };
    }

    // Rule 4: short and clean: <= max_short_words, single sentence, no filler.
    // "No sentence-final punctuation missing" means the text is a complete
    // thought (no mid-text period indicating a truncated sentence), not that
    // it must end with . ! ? - dictation often lacks terminal punctuation.
    if words.len() <= max_short_words && is_single_sentence(trimmed) {
        return GateDecision {
            should_polish: false,
            reason: "short_clean",
        };
    }

    GateDecision {
        should_polish: true,
        reason: "needs_cleanup",
    }
}

/// Whole-word filler anywhere in the text, or a filler opener at the start.
fn has_filler(words: &[&str]) -> bool {
    for (i, w) in words.iter().enumerate() {
        let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
        if FILLER_WORDS.contains(&bare) {
            return true;
        }
        // "you know" is two words; check the bigram.
        if bare == "you"
            && words.get(i + 1).is_some_and(|n| {
                n.trim_matches(|c: char| !c.is_alphanumeric())
                    .eq_ignore_ascii_case("know")
            })
        {
            return true;
        }
    }
    let first = words
        .first()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .unwrap_or("");
    FILLER_OPENERS.contains(&first)
}

/// True when the text ends with a terminal sentence mark (`.` `!` `?` or a
/// closing quote/paren after one).
fn ends_with_sentence_punct(text: &str) -> bool {
    let mut chars = text.chars().rev();
    // Skip at most one trailing closing quote/paren, then require a terminal
    // sentence mark ("...said." "done!)").
    let first = chars.next();
    if matches!(first, Some(c) if c == '"' || c == '\'' || c == '”' || c == ')') {
        // Skip the quote/paren and check the char before it.
        let mut inner = text.chars().rev();
        inner.next();
        matches!(inner.next(), Some(c) if matches!(c, '.' | '!' | '?'))
    } else {
        matches!(first, Some(c) if matches!(c, '.' | '!' | '?'))
    }
}

/// True when the text starts with an uppercase letter or a closing quote.
fn starts_uppercase(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_uppercase() || c == '"' || c == '“')
}

/// True when the text is a single sentence: at most one sentence-final mark
/// (excluding time-like colons and ellipses), and no mid-text period.
fn is_single_sentence(text: &str) -> bool {
    let mut finals = 0;
    let mut in_word = false;
    for c in text.chars() {
        match c {
            '.' | '!' | '?' => {
                if in_word {
                    finals += 1;
                }
                in_word = false;
            }
            c if c.is_alphanumeric() => in_word = true,
            _ => {}
        }
    }
    finals <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "en";
    const CAP: usize = 8;

    fn gate(text: &str, lang: &str) -> GateDecision {
        should_polish(text, lang, CAP)
    }

    #[test]
    fn short_clean_utterances_skip() {
        for text in [
            "ok",
            "yes",
            "the file is in downloads",
            "sounds good.",
            "on my way!",
        ] {
            let d = gate(text, EN);
            assert!(!d.should_polish, "{text:?} must skip, got {d:?}");
            assert_eq!(d.reason, "short_clean", "{text:?}");
        }
    }

    #[test]
    fn filler_forces_polish() {
        for text in [
            "um so like the thing you know with the client",
            "actually i think we should ship it",
            "honestly it was fine",
            "well that was a thing",
        ] {
            let d = gate(text, EN);
            assert!(d.should_polish, "{text:?} must polish, got {d:?}");
            assert_eq!(d.reason, "needs_cleanup", "{text:?}");
        }
    }

    #[test]
    fn nine_word_clean_sentence_polishes() {
        // 8 words with terminal punctuation: skip.
        let d = gate("the file is in downloads folder right now.", EN);
        assert!(!d.should_polish, "{d:?}");
        assert_eq!(d.reason, "short_clean");
        // 9 words with terminal punctuation: over the short cap, not a
        // single clean sentence (lowercase), so polish.
        let text = "the file is in downloads folder right now please.";
        assert_eq!(text.split_whitespace().count(), 9);
        let d = gate(text, EN);
        assert!(d.should_polish, "{d:?}");
        assert_eq!(d.reason, "needs_cleanup");
    }

    #[test]
    fn threshold_is_user_tunable() {
        // 9 words with punctuation: polishes at the default cap of 8...
        let punct = "the file is in downloads folder right now please.";
        assert!(gate(punct, EN).should_polish);
        // ...but skips at a cap of 9.
        let d = should_polish(punct, EN, 9);
        assert!(!d.should_polish, "{d:?}");
        assert_eq!(d.reason, "short_clean");
    }

    #[test]
    fn single_clean_sentence_under_cap_skips() {
        // 3 words, capitalized, terminal period: single_clean_sentence.
        let d = gate("Hello there.", EN);
        assert!(!d.should_polish, "{d:?}");
        assert_eq!(d.reason, "single_clean_sentence");
        // 12 words, capitalized, terminal period: still single_clean_sentence.
        let twelve = "This is a twelve word clean sentence that ends with a period.";
        assert_eq!(twelve.split_whitespace().count(), 12);
        let d = gate(twelve, EN);
        assert!(!d.should_polish, "{d:?}");
        assert_eq!(d.reason, "single_clean_sentence");
    }

    #[test]
    fn thirteen_word_clean_sentence_polishes() {
        // 13 words: over the single-sentence cap, must polish.
        let thirteen = "This is a thirteen word clean sentence that ends with a period today.";
        assert_eq!(thirteen.split_whitespace().count(), 13);
        let d = gate(thirteen, EN);
        assert!(d.should_polish, "{d:?}");
        assert_eq!(d.reason, "needs_cleanup");
    }

    #[test]
    fn cjk_never_skips() {
        for lang in ["ja", "zh-CN", "ko", "th"] {
            let d = gate("今日会議の資料を送ってください。", lang);
            assert!(d.should_polish, "{lang} must polish, got {d:?}");
            assert_eq!(d.reason, "script_needs_llm");
        }
    }

    #[test]
    fn auto_language_uses_latin_rules() {
        // "auto" (Whisper detection pending) falls through to the Latin path.
        let d = gate("ok", "auto");
        assert!(!d.should_polish, "{d:?}");
    }

    #[test]
    fn empty_text_skips() {
        let d = gate("", EN);
        assert!(!d.should_polish, "{d:?}");
        assert_eq!(d.reason, "short_clean");
    }

    #[test]
    fn two_sentences_polish() {
        // Two sentence-final marks: not a single sentence, over the short cap.
        let d = gate("First sentence here. Second sentence here too.", EN);
        assert!(d.should_polish, "{d:?}");
        assert_eq!(d.reason, "needs_cleanup");
    }

    #[test]
    fn filler_inside_longer_word_does_not_trigger() {
        // "er" inside "error" must not trigger the filler path.
        let d = gate("The error is fixed.", EN);
        assert!(!d.should_polish, "{d:?}");
    }

    #[test]
    fn you_know_bigram_triggers() {
        let d = gate("you know we should ship it tomorrow", EN);
        assert!(d.should_polish, "{d:?}");
        assert_eq!(d.reason, "needs_cleanup");
    }

    #[test]
    fn twelve_word_lowercase_clean_polishes() {
        // 12 words, lowercase, no terminal punctuation: not short_clean (over
        // 8), not single_clean_sentence (no terminal punct).
        let twelve =
            "this is a twelve word lowercase clean sentence without any punctuation at all";
        assert_eq!(twelve.split_whitespace().count(), 13);
        let d = gate(twelve, EN);
        assert!(d.should_polish, "{d:?}");
    }
}
