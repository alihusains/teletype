//! Bounded, high-confidence subject/verb agreement repairs for Polish.
//!
//! Cleanup fine-tunes (S1-mini, SpeakoFlow Mini) are card-locked: their system
//! prompt cannot be edited without breaking the model contract, and a small
//! instruct model still misses obvious agreement errors. These are the
//! unambiguous English cases where a correction cannot change the speaker's
//! claim ("she don't" -> "she doesn't"). Anything with a semantic choice
//! ("if I were", "yesterday we go") is left to the model prompt.

const FIXES: &[(&str, &str)] = &[
    ("i has", "i have"),
    ("i is", "i am"),
    ("i are", "i am"),
    ("you is", "you are"),
    ("you has", "you have"),
    ("we is", "we are"),
    ("we has", "we have"),
    ("they is", "they are"),
    ("they has", "they have"),
    ("he have", "he has"),
    ("she have", "she has"),
    ("it have", "it has"),
    ("i doesn't", "i don't"),
    ("you doesn't", "you don't"),
    ("we doesn't", "we don't"),
    ("they doesn't", "they don't"),
    ("he don't", "he doesn't"),
    ("she don't", "she doesn't"),
    ("it don't", "it doesn't"),
    ("we was", "we were"),
    ("they was", "they were"),
    ("you was", "you were"),
    ("i doesn’t", "i don’t"),
    ("you doesn’t", "you don’t"),
    ("we doesn’t", "we don’t"),
    ("they doesn’t", "they don’t"),
    ("he don’t", "he doesn’t"),
    ("she don’t", "she doesn’t"),
    ("it don’t", "it doesn’t"),
];

fn copy_case(src: &str, replacement: &str) -> String {
    let src_letters: Vec<char> = src.chars().filter(|c| c.is_alphabetic()).collect();
    if !src_letters.is_empty() && src_letters.iter().all(|c| c.is_uppercase()) {
        return replacement.to_uppercase();
    }
    let mut chars = replacement.chars();
    match chars.next() {
        Some(first) if src.starts_with(|c: char| c.is_uppercase()) => {
            let mut out: String = first.to_uppercase().collect();
            out.extend(chars);
            out
        }
        Some(first) => {
            let mut out = String::new();
            out.push(first);
            out.extend(chars);
            out
        }
        None => String::new(),
    }
}

fn boundary_ok(lower: &[char], at: usize, len: usize) -> bool {
    let before = at.checked_sub(1).and_then(|i| lower.get(i));
    let after = lower.get(at + len);
    !before.is_some_and(|c| c.is_alphanumeric() || *c == '’')
        && !after.is_some_and(|c| c.is_alphanumeric() || *c == '’')
}

fn replace_phrase(text: &str, from: &str, to: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
    let needle: Vec<char> = from.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if i + needle.len() <= lower.len()
            && lower[i..i + needle.len()] == needle[..]
            && boundary_ok(&lower, i, needle.len())
        {
            let src: String = chars[i..i + needle.len()].iter().collect();
            out.push_str(&copy_case(&src, to));
            i += needle.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

pub fn fix_common_agreement(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in FIXES {
        out = replace_phrase(&out, from, to);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_unambiguous_agreement_errors() {
        assert_eq!(fix_common_agreement("she don't know"), "she doesn't know");
        assert_eq!(fix_common_agreement("I has a plan"), "I have a plan");
        assert_eq!(fix_common_agreement("we was there"), "we were there");
        assert_eq!(fix_common_agreement("they doesn't care"), "they don't care");
        assert_eq!(fix_common_agreement("she don’t know"), "she doesn’t know");
    }

    #[test]
    fn keeps_correct_grammar_and_content_words() {
        for text in [
            "i was there",
            "if i were you",
            "she doesn't know",
            "he has a plan",
            "they are here",
            "the word doesn't change",
            "youtube is here",
        ] {
            assert_eq!(fix_common_agreement(text), text, "changed {text:?}");
        }
    }

    #[test]
    fn is_idempotent() {
        let once = fix_common_agreement("she don't have it. i has none");
        assert_eq!(once, "she doesn't have it. i have none");
        assert_eq!(fix_common_agreement(&once), once);
    }
}
