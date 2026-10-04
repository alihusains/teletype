//! Smart insertion: fitting dictated text into the text it lands in.
//!
//! Dictation always produces a standalone sentence — capitalised, spaced as if
//! it were the whole message. Dropped into the middle of a paragraph, that
//! produces the two artefacts users actually notice: a doubled space, and a
//! capital letter mid-sentence.
//!
//! Deliberately one-directional. [`merge`] may raise the dictated text to a
//! capital or remove whitespace that would double up against text already
//! there, but it never lowercases: there is no way to tell "I said IBM" from
//! "I said Ibm", and quietly rewriting a proper noun into a lowercase word
//! destroys more than it repairs. Anything these rules cannot improve is left
//! exactly as dictated.

/// Text surrounding the caret: everything before it, and everything after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Surroundings<'a> {
    pub before: &'a str,
    pub after: &'a str,
}

/// Punctuation that ends a sentence, so the next word starts a new one.
const SENTENCE_END: [char; 3] = ['.', '!', '?'];

fn is_space(c: char) -> bool {
    c.is_whitespace()
}

fn trim_start_ws(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

fn trim_end_ws(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// The last character of `before` that is not whitespace, which is what
/// actually decides the capitalisation.
fn last_meaningful(before: &str) -> Option<char> {
    before.chars().rev().find(|c| !is_space(*c))
}

/// Uppercases the first alphabetic character of `s`, leaving every other byte
/// alone. A no-op when there is no alphabetic character, so punctuation-only
/// or numeric dictated text passes through untouched.
fn capitalize_first(s: &str) -> String {
    let Some((idx, c)) = s.char_indices().find(|(_, c)| c.is_alphabetic()) else {
        return s.to_string();
    };
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..idx]);
    out.extend(c.to_uppercase());
    out.push_str(&s[idx + c.len_utf8()..]);
    out
}

/// Whether the dictated text should start with a capital, judged only from what
/// precedes the caret.
fn needs_capital(before: &str) -> bool {
    match last_meaningful(before) {
        // Start of the field, or a full stop: a new sentence.
        None => true,
        Some(c) => SENTENCE_END.contains(&c),
    }
}

fn before_ends_in_space(before: &str) -> bool {
    before.chars().last().is_some_and(is_space)
}

/// Fits `dictated` into `surroundings`, returning the text to actually insert.
///
/// Whitespace is only removed when the surrounding text already supplies the
/// gap. A leading space is stripped when the caret already sits after one, and
/// a trailing space is stripped when the text that follows already starts with
/// one — trimming both unconditionally would run words together.
pub fn merge(surroundings: &Surroundings<'_>, dictated: &str) -> String {
    merge_with(surroundings, dictated, true)
}

/// [`merge`] with the capitalisation rule switchable, so tests can pin both
/// sides of the decision instead of only the enabled path.
pub fn merge_with(surroundings: &Surroundings<'_>, dictated: &str, smart_case: bool) -> String {
    // Nothing to fit: whitespace-only dictation is handed back untouched so
    // the caller can tell "nothing was said" from "nothing to say".
    if dictated.trim().is_empty() {
        return dictated.to_string();
    }

    // Leading gap. If whitespace already precedes the caret, ours is a second
    // one, so drop ours and leave theirs — which also preserves a tab or a
    // longer run the user chose.
    let mut body = if before_ends_in_space(surroundings.before) {
        trim_start_ws(dictated).to_string()
    } else {
        dictated.to_string()
    };

    // Capitalise before trimming the trailing side, so a dictated fragment
    // that is only punctuation still gets the chance.
    if smart_case && needs_capital(surroundings.before) {
        body = capitalize_first(&body);
    }

    // Trailing gap, mirrored.
    if surroundings.after.starts_with(is_space) {
        body = trim_end_ws(&body).to_string();
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s<'a>(before: &'a str, after: &'a str) -> Surroundings<'a> {
        Surroundings { before, after }
    }

    #[test]
    fn reuses_a_space_that_is_already_there() {
        // "hello |world" — dictated " there" must not produce "hello  there".
        assert_eq!(merge(&s("hello ", ""), " there"), "there");
    }

    #[test]
    fn keeps_the_dictated_space_when_there_is_none() {
        // Nothing precedes the caret, so the dictated space is the only gap.
        // Trimming it unconditionally would produce "hellothere".
        assert_eq!(merge(&s("hello", "world"), " there"), " there");
        assert_eq!(merge(&s("hello", ""), " there"), " there");
    }

    #[test]
    fn drops_trailing_space_when_the_next_text_already_has_one() {
        assert_eq!(merge(&s("hello ", " world"), "there "), "there");
    }

    #[test]
    fn keeps_a_trailing_space_that_is_doing_real_work() {
        // No space follows the caret, so this one separates "there" from
        // "world".
        assert_eq!(merge(&s("hello ", "world"), "there "), "there ");
    }

    #[test]
    fn keeps_a_trailing_space_at_the_end_of_a_field() {
        assert_eq!(merge(&s("hello ", ""), "there "), "there ");
    }

    #[test]
    fn capitalises_after_a_sentence_end() {
        assert_eq!(merge(&s("Done. ", ""), "next thing"), "Next thing");
        assert_eq!(merge(&s("Really? ", ""), "yes"), "Yes");
        assert_eq!(merge(&s("Wow! ", ""), "really"), "Really");
    }

    #[test]
    fn capitalises_at_the_start_of_an_empty_field() {
        assert_eq!(merge(&s("", ""), "hello"), "Hello");
        assert_eq!(merge(&s("   \n ", ""), "hello"), "Hello");
    }

    #[test]
    fn leaves_capitalisation_alone_mid_sentence() {
        // Never lowercases: "Ibm" and "IBM" are indistinguishable here, and a
        // proper noun rewritten to lowercase is worse than a stray capital.
        assert_eq!(merge(&s("the ", "rest"), "IBM server"), "IBM server");
        assert_eq!(merge(&s("the ", ""), "ibm server"), "ibm server");
    }

    #[test]
    fn never_lowercases_a_capital_the_user_dictated() {
        assert_eq!(merge(&s("call ", ""), "Alice"), "Alice");
    }

    #[test]
    fn preserves_the_users_own_whitespace_run() {
        // The existing run is left alone rather than normalised to one space.
        assert_eq!(merge(&s("hello  ", ""), " there"), "there");
        assert_eq!(merge(&s("hello\t", ""), "\tthere"), "there");
    }

    #[test]
    fn empty_and_whitespace_dictation_is_passed_through() {
        assert_eq!(merge(&s("hello ", ""), ""), "");
        assert_eq!(merge(&s("hello ", ""), "   "), "   ");
    }

    #[test]
    fn capitalization_can_be_switched_off_independently() {
        // Spacing still adapts; only the capital rule is disabled.
        assert_eq!(merge_with(&s("Done. ", ""), "next", false), "next");
        assert_eq!(merge_with(&s("Done. ", ""), "next", true), "Next");
    }

    #[test]
    fn only_the_first_letter_is_touched() {
        // "ebay" must not become "Ebay" -> "EBAY" or "eBay": the rule is a
        // capital on the first character, not title-casing the word.
        assert_eq!(merge(&s("", ""), "ebay account"), "Ebay account");
        assert_eq!(merge(&s("new. ", ""), "ebay account"), "Ebay account");
        // Mid-sentence there is no capital to add, and none is invented.
        assert_eq!(merge(&s("buy ", ""), "ebay account"), "ebay account");
    }

    #[test]
    fn non_alphabetic_dictation_survives() {
        assert_eq!(merge(&s("", ""), "123"), "123");
        assert_eq!(merge(&s("", ""), "..."), "...");
    }

    #[test]
    fn multiline_and_non_ascii_context_is_handled() {
        assert_eq!(merge(&s("line one\n", ""), "line two"), "line two");
        assert_eq!(merge(&s("naïve. ", ""), "café"), "Café");
    }
}
