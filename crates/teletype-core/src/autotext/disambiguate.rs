//! Deciding whether a System AutoText match was a symbol request or a word.
//!
//! # The problem
//!
//! The System AutoText table maps spoken phrases to characters, and every entry
//! is offered to the matcher on every dictation. Most phrases are names a user
//! can only mean literally ("full stop", "close paren", "amperstand"), so
//! matching them is safe. A few are ordinary English, and matching those
//! unconditionally corrupted running text before the model ever saw it:
//!
//! | the user says | what used to be typed |
//! |---|---|
//! | the payment period ends in March | `the payment. ends in March` |
//! | use a comma to separate the fields | `use a, to separate the fields` |
//! | the star of the show | `the * of the show` |
//! | the quote in the article was wrong | `the " in the article was wrong` |
//! | that is a plus for the team | `that is a + for the team` |
//! | start a new line for the address | `start a\nfor the address` |
//!
//! The last column is worse than a stray character. `protect_snippets_with`
//! runs before the LLM, so the model was asked to polish
//! `the payment{{AUTOTEXT_0}} ends in March` and did the best it could with it.
//!
//! # The rule
//!
//! An ambiguous phrase expands only when the surrounding words show the user
//! was naming a character:
//!
//! 1. the utterance is one or two words (`"period"`, `"at sign"`), or
//! 2. a naming cue appears within two words before it (`"insert a comma"`,
//!    `"the symbol for a period"`), or
//! 3. it opens or closes the utterance (`"a period"`, `"period please"`), or
//! 4. an insertion marker follows it (`"I need a comma here"`).
//!
//! When none of those hold, the text is keeping the word. Being wrong that way
//! loses a symbol the user can ask for again; being wrong the other way mangles
//! their sentence.
//!
//! `use` is deliberately **not** a naming cue, because "use a comma to separate
//! the fields" is ordinary prose and is the case that has to keep the word.
//!
//! # Nothing is removed
//!
//! Every way of asking that already worked keeps working: `"full stop"` is
//! safe anywhere in a sentence, as is `"amperstand"` or `"close paren"`. The
//! only things that stop expanding are the mid-sentence coincidences, which
//! were the bug. That is why this needs no setting.
//!
//! # Why not a language model
//!
//! A discriminative decision model (Jev, or the open-source Kev family) is the
//! right *shape* for this: it returns a calibrated probability instead of a
//! hardcoded rule, and it can be fine-tuned on real corrections. Measured
//! latency would be fine (Kev-0.8B is 149 ms on an M5). It is not used here
//! because its only implementation is a Python + torch/MLX runtime, and
//! Teletype's LLM story is built on not having one (see the linked-llama.cpp
//! prohibition, D002, and the local-first, no-cloud contract, D003).
//!
//! So the decision stays a pure function behind one function, and it is small
//! enough that a real model can replace it later without touching the AutoText
//! engine. See `docs/research/jev-kev-for-symbol-disambiguation.md`.

/// Ordinary English words, used to classify *single-word* System entries.
///
/// A one-word System entry that is also a common English word is ambiguous by
/// construction: there is no context in the phrase itself to tell the two
/// apart, so the surrounding sentence has to. Deriving this from a word list
/// means a future single-word System entry is classified automatically instead
/// of depending on someone remembering to update a hand-written list.
///
/// "colon" and "semicolon" are in here even though they are less common in
/// speech than "period", because "the colon between the two" is a real
/// sentence and the same corruption applies.
const PROSE_WORDS: &[&str] = &[
    // Function words and filler that make any sentence.
    "a",
    "an",
    "the",
    "and",
    "or",
    "of",
    "to",
    "in",
    "on",
    "at",
    "for",
    "with",
    "from",
    "by",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "it",
    "its",
    "this",
    "that",
    "these",
    "those",
    "my",
    "your",
    "our",
    "as",
    "but",
    "not",
    "if",
    "so",
    "we",
    "you",
    "he",
    "she",
    "they",
    "i",
    "me",
    "do",
    "does",
    "did",
    "have",
    "has",
    "had",
    "will",
    "would",
    "can",
    "could",
    "should",
    "may",
    "might",
    "there",
    // Nouns and verbs that sit mid-sentence around a character name.
    "new",
    "line",
    "lines",
    "break",
    "breaks",
    "sign",
    "signs",
    "dot",
    "dots",
    "left",
    "right",
    "up",
    "down",
    "star",
    "stars",
    "plus",
    "minus",
    "quote",
    "quotes",
    "comma",
    "colon",
    "colons",
    "semicolon",
    "period",
    "periods",
    "hash",
    "pipe",
    "letter",
    "letters",
    "char",
    "character",
    "symbol",
    "mark",
    "point",
    "end",
    "start",
    "open",
    "close",
    "first",
    "second",
    "third",
    "word",
    "words",
    "space",
    "spaces",
    "bar",
    "bars",
    "tick",
    "ticks",
    "check",
    "cross",
    "after",
    "before",
    "between",
    "during",
    "next",
    "now",
    "here",
    "then",
    "one",
    "two",
    "three",
    "place",
    "above",
    "below",
    "back",
    "over",
    "under",
    "no",
    "yes",
    "if",
    "so",
    "very",
    "more",
    "most",
    "same",
    "other",
    "each",
    "every",
    "all",
    "some",
    "any",
    "all",
    "own",
    "just",
    "also",
    "only",
    "even",
];

/// Multi-word System phrases that read like a fragment of an English sentence,
/// and so can be swallowed by one.
///
/// This list is explicit rather than derived, because the derivation does not
/// separate the two cases. "at sign" and "new line" are both two ordinary
/// words, but only one of them is a thing people say mid-sentence:
///
/// - "start a new line", "on the next line", "a line break", "the plus sign",
///   "a minus sign" are all things a person says while writing. These are
///   checked against the context rule like a single word.
/// - "at sign", "at symbol", "and and", "or or", "dot dot dot", "colon
///   equals" are names a person chose deliberately and does not use in
///   ordinary prose. They keep working anywhere in a sentence, which is what
///   makes "email at sign example dot com" still type `email @ example dot com`.
///
/// The criterion for adding a phrase here: it should be a complete English
/// fragment on its own ("a new line", "the plus sign"), because that is the
/// shape that collides with dictation.
const AMBIGUOUS_PHRASES: &[&str] = &[
    "new line",
    "next line",
    "line break",
    "plus sign",
    "minus sign",
];

/// Words that mean "I am naming a character" when they sit just before the
/// ambiguous word.
///
/// `use` is intentionally absent: "use a comma to separate the fields" is prose,
/// and that is the case that has to keep the word. Neither is `a`, which is an
/// article rather than an instruction.
const NAMING_CUES: &[&str] = &[
    "symbol",
    "character",
    "sign",
    "type",
    "typing",
    "press",
    "insert",
    "give",
    "add",
    "put",
    "write",
    "spell",
    "code",
    "enter",
];

/// Words that mean the character goes *here*, which is how a request ends
/// without naming a cue first: "I need a comma here".
///
/// Deliberately short. "in", "on", "after" and "first" were tried here and
/// removed: they are ordinary continuations, so "the quote in the article" and
/// "a new line for the address" counted as requests and the bug came back.
const INSERTION_MARKERS: &[&str] = &["here", "please", "now", "next"];

/// Normalize a word for comparison: lowercase, letters and apostrophes only.
fn normalize(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `phrase` can be confused with ordinary English, and so needs the
/// surrounding text to be checked before it expands.
pub fn is_ambiguous(phrase: &str) -> bool {
    let words: Vec<String> = phrase.split_whitespace().map(normalize).collect();
    match words.as_slice() {
        // A single ordinary word is ambiguous by construction.
        [one] => PROSE_WORDS.contains(&one.as_str()),
        // Two words: only the explicit list, see its documentation. Anything
        // longer is a deliberate name nobody would say mid-sentence, so it is
        // left alone ("dot dot dot" is three words).
        // Both sides go through `normalize`, which drops the space, so
        // "new line" is compared as "newline".
        [a, b] => {
            let joined = normalize(&format!("{a} {b}"));
            AMBIGUOUS_PHRASES.iter().any(|p| normalize(p) == joined)
        }
        // Nothing to match, or a long deliberate name.
        _ => false,
    }
}

/// The next word after the match, lowercased.
fn word_after(text: &str, start_idx: usize, phrase: &str) -> Option<String> {
    let start = floor_boundary(text, start_idx);
    let after = start + phrase.len();
    if after >= text.len() || !text.is_char_boundary(after) {
        return None;
    }
    text[after..].split_whitespace().next().map(normalize)
}

/// A longer ambiguous System phrase that the text continues into at `start_idx`.
///
/// This exists to stop a half-expansion. "two plus sign two equals four"
/// contains the one-word entry "plus" *and* the two-word entry "plus sign" at
/// the same position, and the engine tries the longer phrase first. If the
/// phrase is rejected as prose but the word is not, the text comes out as
/// `two + sign two = four`, which is worse than either decision on its own.
///
/// So a one-word entry that the utterance continues into an ambiguous phrase is
/// judged by that phrase, not by itself.
pub fn continues_into_ambiguous_phrase(
    text: &str,
    start_idx: usize,
    phrase: &str,
) -> Option<String> {
    if phrase.split_whitespace().count() != 1 {
        return None;
    }
    let next = word_after(text, start_idx, phrase)?;
    let longer = format!("{} {}", normalize(phrase), next);
    is_ambiguous(&longer).then_some(longer)
}

/// Whether the match at `start_idx` should be expanded to its character.
///
/// `text` is the whole utterance, `start_idx` is the char index where `phrase`
/// begins, and `phrase` is the System entry's spoken phrase.
///
/// Always `true` for anything [`is_ambiguous`] rejects, so this can only ever
/// suppress a match that would have corrupted prose.
pub fn is_symbol_request(text: &str, start_idx: usize, phrase: &str) -> bool {
    if !is_ambiguous(phrase) {
        return true;
    }
    if text.is_empty() || start_idx >= text.len() {
        // Not a real match. Keep the word rather than guessing.
        return false;
    }
    let words: Vec<String> = text.split_whitespace().map(normalize).collect();
    if words.is_empty() {
        return false;
    }

    // Which word of the utterance the phrase starts at. `split_whitespace` on
    // the whole text and on a prefix agree on the count, so this stays valid
    // whatever the spacing, and `floor_boundary` keeps the slice from landing
    // inside a multi-byte char.
    let start = floor_boundary(text, start_idx);
    let first = text[..start].split_whitespace().count();
    if first >= words.len() {
        return false;
    }
    // The phrase's own words are not context. "new line" occupies two slots, so
    // the word after it is two along, not one.
    let span = phrase.split_whitespace().count().max(1);
    let end = first + span;
    let is_first = first == 0;
    let is_last = end >= words.len();

    // Rule 1: the whole utterance is the request.
    if words.len() <= 2 {
        return true;
    }
    // Rule 3: it opens or closes the utterance.
    if is_first || is_last {
        return true;
    }
    // Rule 4: an insertion marker right after it ("I need a comma here").
    if let Some(next) = words.get(end) {
        if INSERTION_MARKERS.contains(&next.as_str()) {
            return true;
        }
    }
    // Rule 2: a naming cue within two words before it ("insert a comma").
    if has_cue_before(&words, first) {
        return true;
    }
    // Rule 5: a naming cue within two words after it. "the pipe character",
    // "the comma symbol": the user is naming the character, just after it.
    // None of the prose cases below are followed by a cue, which is why this
    // does not undo the fix.
    if has_cue_after(&words, end) {
        return true;
    }
    false
}

/// A naming cue in the two words before index `from`.
fn has_cue_before(words: &[String], from: usize) -> bool {
    for back in 1..=2usize {
        let Some(j) = from.checked_sub(back) else {
            continue;
        };
        if NAMING_CUES.contains(&words[j].as_str()) {
            return true;
        }
        // "the symbol for a period": `for` sits between the cue and the word.
        if words[j] == "for" && j > 0 && NAMING_CUES.contains(&words[j - 1].as_str()) {
            return true;
        }
    }
    false
}

/// A naming cue in the two words starting at index `from`.
fn has_cue_after(words: &[String], from: usize) -> bool {
    for ahead in 0..2usize {
        let Some(j) = from.checked_add(ahead) else {
            continue;
        };
        let Some(word) = words.get(j) else { break };
        if NAMING_CUES.contains(&word.as_str()) {
            return true;
        }
        if word == "for"
            && words
                .get(j + 1)
                .is_some_and(|w| NAMING_CUES.contains(&w.as_str()))
        {
            return true;
        }
    }
    false
}

/// Whether a System entry at `start_idx` may expand. The one function both the
/// expansion and the protection path call.
pub fn entry_allowed(text: &str, start_idx: usize, phrase: &str) -> bool {
    match continues_into_ambiguous_phrase(text, start_idx, phrase) {
        // "plus" in "two plus sign two": the user said "plus sign", so that
        // phrase's verdict is the one that applies.
        Some(longer) => is_symbol_request(text, start_idx, &longer),
        None => is_symbol_request(text, start_idx, phrase),
    }
}

/// Snap `idx` back to the nearest char boundary at or before it.
///
/// `match_snippet_at` hands us a char index, so this is a no-op in practice,
/// but slicing a `&str` at a non-boundary panics and a pure function is worth
/// testing directly.
fn floor_boundary(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    let mut i = idx;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the phrase starts, in chars, mirroring how the engine calls in.
    fn start_of(text: &str, phrase: &str) -> usize {
        text.to_lowercase()
            .find(&phrase.to_lowercase())
            .unwrap_or(0)
    }

    fn fires(text: &str, phrase: &str) -> bool {
        is_symbol_request(text, start_of(text, phrase), phrase)
    }

    /// These used to be typed as a character, and are the bug.
    #[test]
    fn ordinary_prose_keeps_the_word() {
        for (text, phrase) in [
            ("the payment period ends in March", "period"),
            ("use a comma to separate the fields", "comma"),
            ("the star of the show", "star"),
            ("the quote in the article was wrong", "quote"),
            ("that is a plus for the team", "plus"),
            ("the colon between the two values", "colon"),
            ("start a new line for the address", "new line"),
            (
                "please move this down to the next line of the poem",
                "next line",
            ),
            ("there is a line break in the paragraph", "line break"),
            ("the plus sign shows the delta", "plus sign"),
            (
                "change the sign to a minus sign for the total",
                "minus sign",
            ),
        ] {
            assert!(
                !fires(text, phrase),
                "{text:?} should keep the word {phrase:?}"
            );
        }
    }

    /// A real request, in every shape a user might say it.
    #[test]
    fn a_symbol_request_still_expands() {
        for (text, phrase) in [
            ("period", "period"),
            ("comma", "comma"),
            ("plus", "plus"),
            ("insert a comma here", "comma"),
            ("type a period", "period"),
            ("press the colon", "colon"),
            ("the symbol for a period", "period"),
            ("give me a colon", "colon"),
            ("add a star", "star"),
            ("a period", "period"),
            ("period please", "period"),
            ("i need a comma here", "comma"),
            ("new line", "new line"),
            ("i need a new line now", "new line"),
        ] {
            assert!(fires(text, phrase), "{text:?} should expand {phrase:?}");
        }
    }

    /// Names a user chose deliberately keep working anywhere in a sentence.
    /// "email at sign example dot com" is the real case: spelling an address
    /// aloud is a symbol request in the middle of an otherwise long utterance.
    #[test]
    fn deliberate_names_are_never_suppressed() {
        for (text, phrase) in [
            ("email at sign example dot com", "at sign"),
            ("the payment period ends in March", "full stop"),
            ("use a comma to separate the fields", "full stop"),
            ("that is a plus for the team", "line break"),
            ("remind me to write full stop at the end", "full stop"),
            ("the semicolon ends it", "close paren"),
            ("start a new line for the address", "dot dot dot"),
            ("the payment period ends in March", "amperstand"),
            ("the star of the show", "exclamation mark"),
        ] {
            assert!(
                fires(text, phrase),
                "{text:?} should keep expanding {phrase:?}"
            );
        }
    }

    /// Anything that is not a common English word behaves exactly as before.
    #[test]
    fn other_single_words_are_never_suppressed() {
        for (text, phrase) in [
            ("the ampersand and", "ampersand"),
            ("an underscore in the name", "underscore"),
            ("a tilde is a squiggle", "tilde"),
            ("insert a pipe here", "pipe"),
            ("slash", "slash"),
            ("use an em dash there", "em dash"),
            ("the copyright sign", "copyright"),
        ] {
            assert!(fires(text, phrase), "{text:?} should expand {phrase:?}");
        }
    }

    /// A trailing marker means "put the character here", which is a request
    /// even with no naming cue before it.
    #[test]
    fn an_insertion_marker_after_the_word_counts_as_a_request() {
        assert!(fires("i need a comma here", "comma"));
        assert!(fires("add a period please", "period"));
        assert!(fires("put a star now", "star"));
        // ...but an ordinary continuation is not a request.
        assert!(!fires("the comma is wrong here", "comma"));
        assert!(!fires("the quote in the article was wrong", "quote"));
    }

    /// Being wrong in the safe direction: if we cannot tell, keep the word.
    /// A mangled sentence is worse than a missed symbol.
    #[test]
    fn an_empty_or_odd_match_keeps_the_word() {
        assert!(!is_symbol_request("", 0, "period"));
        assert!(!is_symbol_request(
            "some longer sentence here entirely",
            4,
            "period"
        ));
        // A match index past the end must not panic.
        assert!(!is_symbol_request(
            "the payment period ends in March",
            9999,
            "period"
        ));
        // Nor must a non-boundary index.
        assert!(!is_symbol_request(
            "the payment period ends in March",
            5,
            "period"
        ));
    }

    /// Pin the real System table, so a new entry is a deliberate decision
    /// rather than an accident.
    #[test]
    fn the_real_system_table_classifies_as_expected() {
        for phrase in [
            "comma",
            "period",
            "quote",
            "star",
            "plus",
            "minus",
            "hash",
            "pipe",
            "colon",
            "semicolon",
            "new line",
            "next line",
            "line break",
            "plus sign",
            "minus sign",
        ] {
            assert!(is_ambiguous(phrase), "{phrase:?} should be ambiguous");
        }
        for phrase in [
            "full stop",
            "close paren",
            "open bracket",
            "question mark",
            "exclamation point",
            "quotation mark",
            "percent sign",
            "dollar sign",
            "asterisk",
            "underscore",
            "tilde",
            "ellipsis",
            "em dash",
            "hyphen",
            "arrow",
            "at sign",
            "at symbol",
            "and and",
            "or or",
            "dot dot dot",
            "colon equals",
            "double colon",
            "single quote",
        ] {
            assert!(!is_ambiguous(phrase), "{phrase:?} should NOT be ambiguous");
        }
    }

    /// Every entry the rule actually catches, so a surprise is visible.
    #[test]
    fn the_rule_catches_every_ambiguous_entry_in_the_real_table() {
        let caught: Vec<String> = super::super::system::entries()
            .iter()
            .map(|e| e.snippet_phrase().to_string())
            .filter(|p| is_ambiguous(p))
            .collect();
        let mut expected = vec![
            "comma",
            "period",
            "quote",
            "star",
            "plus",
            "minus",
            "hash",
            "pipe",
            "colon",
            "semicolon",
            "new line",
            "next line",
            "line break",
            "plus sign",
            "minus sign",
        ];
        let mut sorted = caught.clone();
        sorted.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            sorted, expected,
            "the set of entries the rule touches changed; re-check each one"
        );
    }

    /// Every entry the rule touches must be reachable both ways: an explicit
    /// request expands it, and the prose it collides with does not. This is
    /// what "no capability is lost" means, checked against the real table
    /// rather than asserted in a comment.
    #[test]
    fn every_caught_entry_is_still_askable_for() {
        for phrase in [
            "comma",
            "period",
            "quote",
            "star",
            "plus",
            "minus",
            "hash",
            "pipe",
            "colon",
            "semicolon",
            "new line",
            "next line",
            "line break",
            "plus sign",
            "minus sign",
        ] {
            // Asked for plainly, three different ways.
            assert!(fires(phrase, phrase), "{phrase:?} alone should expand");
            assert!(
                fires(&format!("insert a {phrase} here"), phrase),
                "{phrase:?} + cue"
            );
            assert!(
                fires(&format!("a {phrase}"), phrase),
                "{phrase:?} at the end"
            );
            // And the prose case it collides with still keeps the words.
            assert!(
                !fires(&format!("that is a {phrase} for the report"), phrase),
                "{phrase:?} mid-sentence should keep the words"
            );
        }
    }

    /// The window either side of a match must skip the phrase's own words, or a
    /// two-word entry would read its own second word as a cue.
    #[test]
    fn the_context_window_skips_the_phrase_itself() {
        // "line" and "plus"/"minus" are not cues; if the window were off by the
        // phrase length, these would pass or fail for the wrong reason.
        assert!(!fires("start a new line for the address", "new line"));
        assert!(fires("i need a new line now", "new line"));
        assert!(!fires(
            "change the plus sign to a minus sign for the total",
            "minus sign"
        ));
        assert!(fires(
            "change the plus sign to a minus sign here",
            "minus sign"
        ));
    }
}
