//! Emoji restore: turn spoken emoji phrases back into emoji.
//!
//! ASR transcribes "thumbs up" or "smiley face" as words; the LLM polish
//! layer keeps them as words. This pass restores a small curated set of
//! emoji after the transform so output feels human.
//!
//! House pattern (see [`crate::itn`]): a pure, stateless
//! `restore(input) -> String` with table-driven tests. The dictionary is a
//! curated ~120-entry list we wrote ourselves (Apache-2.0, shipped in this
//! repo — deliberately NOT the 127 KB third-party list):
//! `emoji-dictionary.json`, format `[{"trigger": "thumbs up", "emoji": "👍"}]`.
//!
//! Matching rule (documented per the task):
//! - Word-boundary, case-insensitive, whitespace-tolerant: the trigger must
//!   match whole words ("thumbs up" matches "thumbs, up" but not
//!   "thumbsticks"), never inside a longer word.
//! - Longest trigger first: "heart emoji" beats a hypothetical shorter
//!   trigger sharing its prefix.
//! - Ambiguity rule: a bare common noun is only a trigger when it is
//!   unambiguous on its own (e.g. "sparkles", "tada"). For words that are
//!   ordinary vocabulary ("heart", "fire", "star", "check mark" as a UI
//!   label is fine, but a bare "heart" is not), the trigger requires the
//!   "emoji"/"emoticon" suffix — "my heart" must stay "my heart".
//!
//! Idempotent: the output contains emoji, which no trigger matches, so
//! `restore(restore(x)) == restore(x)`.

use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, serde::Deserialize)]
struct Entry {
    trigger: String,
    emoji: String,
}

/// The curated dictionary, loaded at compile time (no runtime IO).
fn entries() -> &'static [Entry] {
    static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("emoji-dictionary.json"))
            .expect("emoji dictionary is valid JSON")
    });
    &ENTRIES
}

/// Triggers sorted longest-first so multi-word phrases win over their
/// sub-phrases.
fn sorted_triggers() -> &'static [(&'static str, &'static str)] {
    static TRIGGERS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
        let mut v: Vec<(&'static str, &'static str)> = entries()
            .iter()
            .map(|e| (e.trigger.as_str(), e.emoji.as_str()))
            .collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        v
    });
    &TRIGGERS
}

/// A word boundary for trigger matching: start/end of text, or a run of
/// non-alphanumeric characters (space, punctuation).
fn is_boundary(c: Option<char>) -> bool {
    !c.is_some_and(|c| c.is_alphanumeric())
}

fn find_trigger(text: &str, pos: usize) -> Option<(&str, &'static str)> {
    // Longest trigger first. A trigger must start exactly at `pos` (modulo
    // letter case); the caller advances `pos` one char at a time until one
    // matches, so this scans forward from `pos` to find a candidate whose
    // letter content equals the trigger's.
    for (trigger, emoji) in sorted_triggers() {
        let trigger_letters: String = trigger
            .chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        // Walk forward from `pos`, accumulating letters. Punctuation and
        // whitespace between words are allowed (a multi-word trigger spans
        // exactly the gaps in its own words), but the candidate must not
        // run past a boundary once its letters no longer prefix the
        // trigger's letters.
        let mut end = pos;
        let mut letters: String = String::new();
        let mut found = false;
        while end < text.len() {
            let c = text[end..].chars().next().unwrap();
            let len = c.len_utf8();
            if c.is_alphanumeric() {
                letters.push(c.to_ascii_lowercase());
                if letters.len() > trigger_letters.len() {
                    break; // candidate longer than the trigger
                }
            } else {
                // Punctuation/whitespace: only continue if the letters so far
                // still prefix the trigger's letters (i.e. we are inside a
                // multi-word trigger's own word gap). A leading gap is not
                // allowed for a bare trigger.
                if letters.is_empty() || !trigger_letters.starts_with(&letters) {
                    break;
                }
            }
            end += len;
            if letters == trigger_letters {
                // Candidate complete; require a boundary right after it.
                let after = text[end..].chars().next();
                if is_boundary(after) {
                    found = true;
                }
                break;
            }
        }
        if found {
            return Some((&text[pos..end], emoji));
        }
    }
    None
}

/// Restores curated emoji phrases in `text`. See the module docs for the
/// matching rule.
pub fn restore(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while pos < text.len() {
        if let Some((matched, emoji)) = find_trigger(text, pos) {
            out.push_str(emoji);
            pos += matched.len();
        } else {
            let c = text[pos..].chars().next().unwrap();
            out.push(c);
            pos += c.len_utf8();
        }
    }
    out
}

/// The trigger -> emoji table, exposed for tests and future UI.
pub fn dictionary() -> HashMap<String, String> {
    entries()
        .iter()
        .map(|e| (e.trigger.clone(), e.emoji.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_thumbs_up() {
        assert_eq!(restore("thanks, thumbs up!"), "thanks, 👍!");
        assert_eq!(restore("Thumbs Up, great job"), "👍, great job");
    }

    #[test]
    fn bare_heart_is_ambiguous() {
        // "heart" alone is ordinary vocabulary: never replaced.
        assert_eq!(restore("my heart"), "my heart");
        assert_eq!(restore("Heart of the city"), "Heart of the city");
        // But the explicit phrase is restored.
        assert_eq!(restore("send a heart emoji"), "send a ❤️");
        assert_eq!(restore("heart emoticon please"), "❤️ please");
    }

    #[test]
    fn longest_trigger_wins() {
        // "broken heart emoji" (longer) must beat "heart emoji" (shorter).
        assert_eq!(
            restore("the broken heart emoji and a blue heart emoji"),
            "the 💔 and a 💙"
        );
        // Bare "heart" is ambiguous and stays, even with a colour adjective.
        assert_eq!(
            restore("the broken heart emoji and a red heart"),
            "the 💔 and a red heart"
        );
    }

    #[test]
    fn never_inside_longer_word() {
        // "sparkles" is a bare trigger, but "sparklesome" is not a match.
        assert_eq!(restore("sparkles everywhere"), "✨ everywhere");
        assert_eq!(restore("sparklesome"), "sparklesome");
        // "tada" must not match inside "tadadum".
        assert_eq!(restore("tadadum"), "tadadum");
    }

    #[test]
    fn idempotent() {
        let cases = [
            "thanks, thumbs up!",
            "heart emoji",
            "my heart",
            "fire emoji and sparkles",
            "👍 already an emoji",
            "x mark and check mark",
        ];
        for x in cases {
            let once = restore(x);
            assert_eq!(restore(&once), once, "not idempotent for {x:?}");
        }
    }

    #[test]
    fn cjk_unchanged() {
        let cjk = "谢谢，点赞！心形表情";
        assert_eq!(restore(cjk), cjk);
        // Mixed text: the CJK part is untouched, the Latin trigger fires.
        assert_eq!(restore("谢谢 thumbs up"), "谢谢 👍");
    }

    /// Punctuation-tolerant for multi-word triggers: a comma between the
    /// words of "thumbs up" is fine (ASR inserts commas freely).
    #[test]
    fn punctuation_tolerant() {
        assert_eq!(restore("thumbs, up"), "👍");
        assert_eq!(restore("(thumbs up)"), "(👍)");
        assert_eq!(restore("a thumbs-up!"), "a 👍!");
    }

    #[test]
    fn multiple_in_one_text() {
        assert_eq!(
            restore("great work, thumbs up and clap emoji"),
            "great work, 👍 and 👏"
        );
    }

    #[test]
    fn dictionary_is_curated() {
        // We ship our own list, not a scraped mega-dictionary.
        let dict = dictionary();
        assert!(
            (100..=200).contains(&dict.len()),
            "expected 100-150 entries, got {}",
            dict.len()
        );
        assert_eq!(dict.get("thumbs up").unwrap(), "👍");
    }
}
