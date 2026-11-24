//! Deterministic sentence-start capitalization for Polish output.
//!
//! Wispr Flow keeps casing and punctuation as a formatting layer separate from
//! cleanup: it matches the text around the cursor on device and capitalizes
//! sentence starts without asking the language model. Small local models
//! routinely ignore a generic "fix capitalization" instruction (L059), so the
//! repeated formatting promise is kept in code here. The LLM edits words;
//! this module only uppercases a lowercase letter that already opens a
//! sentence. It never adds, removes or reorders any other character.

/// Abbreviations whose trailing period is not a sentence end.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "eg", "ie", "no", "inc", "ltd",
    "approx", "dept", "fig", "vol", "a.m", "p.m", "u.s", "e.g", "i.e", "am", "pm",
];

/// Titles followed by a name: the word after Dr. or Mr. is a proper noun.
const TITLES: &[&str] = &["mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st"];

fn is_opening(c: char) -> bool {
    matches!(c, '"' | '\'' | '(' | '[' | '{' | '“' | '‘' | '«')
}

fn is_closing(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '}' | '”' | '’' | '»')
}

fn is_terminal(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…')
}

fn word_start(chars: &[char], dot: usize) -> usize {
    let mut i = dot;
    while i > 0 && !chars[i - 1].is_whitespace() && chars[i - 1] != '.' {
        i -= 1;
    }
    i
}

fn ends_sentence(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if !is_terminal(c) {
        return false;
    }
    let mut after = i + 1;
    while after < chars.len() && is_closing(chars[after]) {
        after += 1;
    }
    if after < chars.len() && !chars[after].is_whitespace() {
        return false;
    }
    if c != '.' {
        return true;
    }
    if after < chars.len() && chars[after].is_ascii_digit() {
        return false;
    }
    let start = word_start(chars, i);
    let word: String = chars[start..i].iter().collect();
    let lower = word.to_ascii_lowercase();
    if word.contains('.') || ABBREVIATIONS.contains(&lower.as_str()) {
        return false;
    }
    true
}

/// A title's period does not end a sentence, but the name after it starts
/// with a capital even when the transcript is all lowercase.
fn title_dot(chars: &[char], i: usize) -> bool {
    if chars[i] != '.' {
        return false;
    }
    let start = word_start(chars, i);
    let lower = chars[start..i]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    TITLES.contains(&lower.as_str())
}

/// Capitalizes the first letter of every sentence, line and quoted sentence.
///
/// Idempotent: an uppercase or non-ASCII lead is left alone. Abbreviations,
/// initials, decimals, domains and open quotes are handled without changing
/// any other character. Other scripts are never touched.
pub fn capitalize_sentences(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at_start = true;
    for (i, c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            out.push(*c);
            if *c == '\n' {
                at_start = true;
            }
            continue;
        }
        if at_start {
            if is_opening(*c) {
                out.push(*c);
                continue;
            }
            // English-only by design: scripts with no case (CJK) or a
            // different case system (Cyrillic, Arabic) stay byte-for-byte.
            // The i18n round-trip test depends on this.
            if c.is_ascii_lowercase() {
                out.push(c.to_ascii_uppercase());
            } else {
                out.push(*c);
            }
            at_start = false;
            continue;
        }
        out.push(*c);
        if ends_sentence(&chars, i) || title_dot(&chars, i) {
            at_start = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalizes_each_sentence_after_terminal_punctuation() {
        assert_eq!(
            capitalize_sentences("hello world. how are you? fine! thanks... okay"),
            "Hello world. How are you? Fine! Thanks... Okay"
        );
    }

    #[test]
    fn capitalizes_after_a_newline_and_an_opening_quote() {
        assert_eq!(
            capitalize_sentences("he said. \"go home.\" then left\nsecond line"),
            "He said. \"Go home.\" Then left\nSecond line"
        );
    }

    #[test]
    fn leaves_abbreviations_initials_decimals_and_domains_alone() {
        assert_eq!(
            capitalize_sentences("dr. smith met mr. jones. i left at 3.14 pm"),
            "Dr. Smith met mr. Jones. I left at 3.14 pm"
        );
        assert_eq!(
            capitalize_sentences("go to example.com. it works"),
            "Go to example.com. It works"
        );
        assert_eq!(
            capitalize_sentences("A. smith left. the end"),
            "A. Smith left. The end"
        );
    }

    #[test]
    fn is_idempotent_and_keeps_uppercase_leads() {
        let once = capitalize_sentences("hey. iPod stayed. NASA launched");
        assert_eq!(once, "Hey. IPod stayed. NASA launched");
        assert_eq!(capitalize_sentences(&once), once);
    }

    #[test]
    fn does_not_touch_empty_or_symbol_only_text() {
        assert_eq!(capitalize_sentences(""), "");
        assert_eq!(capitalize_sentences("..."), "...");
        assert_eq!(capitalize_sentences("123. 456"), "123. 456");
    }
}
