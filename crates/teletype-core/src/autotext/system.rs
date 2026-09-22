//! Built-in System AutoText entries.
//!
//! These are deterministic, local, spoken-phrase replacements (e.g. saying
//! "comma" inserts `,`). They reuse the exact same snippet-matching engine as
//! custom AutoText — there is no separate engine. Custom entries override
//! System entries with the same trigger/phrase.
//!
//! No LLM, no API, no semantic matching: each is a fixed phrase → value pair.

use std::sync::OnceLock;

use super::AutoTextEntry;

/// How the replacement should be spaced relative to the surrounding text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// Keep normal spaces on both sides (default for most entries).
    Normal,
    /// Attach to the preceding word with no space before (e.g. `,` `.` `?`).
    AttachLeft,
    /// Attach to the following word with no space after (e.g. `(` `[`).
    AttachRight,
    /// No space on either side (e.g. inline operators like `->`).
    AttachBoth,
}

/// A System AutoText definition: spoken phrase, replacement value, and the
/// spacing rule applied around the inserted value.
struct Def {
    phrase: &'static str,
    replacement: &'static str,
    spacing: Spacing,
}

const DEFS: &[Def] = &[
    // Punctuation
    Def { phrase: "comma", replacement: ",", spacing: Spacing::AttachLeft },
    Def { phrase: "period", replacement: ".", spacing: Spacing::AttachLeft },
    Def { phrase: "full stop", replacement: ".", spacing: Spacing::AttachLeft },
    Def { phrase: "question mark", replacement: "?", spacing: Spacing::AttachLeft },
    Def { phrase: "exclamation mark", replacement: "!", spacing: Spacing::AttachLeft },
    Def { phrase: "exclamation point", replacement: "!", spacing: Spacing::AttachLeft },
    Def { phrase: "colon", replacement: ":", spacing: Spacing::AttachLeft },
    Def { phrase: "semicolon", replacement: ";", spacing: Spacing::AttachLeft },
    // Quotes
    Def { phrase: "quote", replacement: "\"", spacing: Spacing::Normal },
    Def { phrase: "quotation mark", replacement: "\"", spacing: Spacing::Normal },
    Def { phrase: "apostrophe", replacement: "'", spacing: Spacing::Normal },
    Def { phrase: "single quote", replacement: "'", spacing: Spacing::Normal },
    // Parentheses
    Def { phrase: "open parenthesis", replacement: "(", spacing: Spacing::AttachRight },
    Def { phrase: "close parenthesis", replacement: ")", spacing: Spacing::AttachLeft },
    Def { phrase: "open paren", replacement: "(", spacing: Spacing::AttachRight },
    Def { phrase: "close paren", replacement: ")", spacing: Spacing::AttachLeft },
    // Brackets
    Def { phrase: "open bracket", replacement: "[", spacing: Spacing::AttachRight },
    Def { phrase: "close bracket", replacement: "]", spacing: Spacing::AttachLeft },
    Def { phrase: "open square bracket", replacement: "[", spacing: Spacing::AttachRight },
    Def { phrase: "close square bracket", replacement: "]", spacing: Spacing::AttachLeft },
    Def { phrase: "open curly bracket", replacement: "{", spacing: Spacing::AttachRight },
    Def { phrase: "close curly bracket", replacement: "}", spacing: Spacing::AttachLeft },
    Def { phrase: "open brace", replacement: "{", spacing: Spacing::AttachRight },
    Def { phrase: "close brace", replacement: "}", spacing: Spacing::AttachLeft },
    // Line formatting
    Def { phrase: "new line", replacement: "\n", spacing: Spacing::Normal },
    Def { phrase: "next line", replacement: "\n", spacing: Spacing::Normal },
    Def { phrase: "line break", replacement: "\n", spacing: Spacing::Normal },
    Def { phrase: "new paragraph", replacement: "\n\n", spacing: Spacing::Normal },
    // Common symbols
    Def { phrase: "ampersand", replacement: "&", spacing: Spacing::Normal },
    Def { phrase: "asterisk", replacement: "*", spacing: Spacing::Normal },
    Def { phrase: "star", replacement: "*", spacing: Spacing::Normal },
    Def { phrase: "at sign", replacement: "@", spacing: Spacing::Normal },
    Def { phrase: "at symbol", replacement: "@", spacing: Spacing::Normal },
    Def { phrase: "hash", replacement: "#", spacing: Spacing::Normal },
    Def { phrase: "hashtag", replacement: "#", spacing: Spacing::Normal },
    Def { phrase: "percent sign", replacement: "%", spacing: Spacing::AttachLeft },
    Def { phrase: "slash", replacement: "/", spacing: Spacing::Normal },
    Def { phrase: "forward slash", replacement: "/", spacing: Spacing::Normal },
    Def { phrase: "backslash", replacement: "\\", spacing: Spacing::Normal },
    Def { phrase: "underscore", replacement: "_", spacing: Spacing::Normal },
    Def { phrase: "tilde", replacement: "~", spacing: Spacing::Normal },
    Def { phrase: "pipe", replacement: "|", spacing: Spacing::Normal },
    Def { phrase: "vertical bar", replacement: "|", spacing: Spacing::Normal },
    // Mathematical symbols
    Def { phrase: "plus", replacement: "+", spacing: Spacing::Normal },
    Def { phrase: "plus sign", replacement: "+", spacing: Spacing::Normal },
    Def { phrase: "minus", replacement: "-", spacing: Spacing::Normal },
    Def { phrase: "minus sign", replacement: "-", spacing: Spacing::Normal },
    Def { phrase: "equals", replacement: "=", spacing: Spacing::Normal },
    Def { phrase: "equals sign", replacement: "=", spacing: Spacing::Normal },
    Def { phrase: "less than", replacement: "<", spacing: Spacing::Normal },
    Def { phrase: "greater than", replacement: ">", spacing: Spacing::Normal },
    // Developer / coding symbols
    Def { phrase: "double equals", replacement: "==", spacing: Spacing::Normal },
    Def { phrase: "triple equals", replacement: "===", spacing: Spacing::Normal },
    Def { phrase: "not equals", replacement: "!=", spacing: Spacing::Normal },
    Def { phrase: "arrow", replacement: "->", spacing: Spacing::Normal },
    Def { phrase: "fat arrow", replacement: "=>", spacing: Spacing::Normal },
    Def { phrase: "double colon", replacement: "::", spacing: Spacing::Normal },
    Def { phrase: "double slash", replacement: "//", spacing: Spacing::Normal },
    Def { phrase: "question dot", replacement: "?.", spacing: Spacing::Normal },
    Def { phrase: "question question", replacement: "??", spacing: Spacing::Normal },
    Def { phrase: "and and", replacement: "&&", spacing: Spacing::Normal },
    Def { phrase: "or or", replacement: "||", spacing: Spacing::Normal },
    Def { phrase: "colon equals", replacement: ":=", spacing: Spacing::Normal },
    // Dash / ellipsis
    Def { phrase: "hyphen", replacement: "-", spacing: Spacing::Normal },
    Def { phrase: "dash", replacement: "-", spacing: Spacing::Normal },
    Def { phrase: "em dash", replacement: "—", spacing: Spacing::Normal },
    Def { phrase: "en dash", replacement: "–", spacing: Spacing::Normal },
    Def { phrase: "ellipsis", replacement: "…", spacing: Spacing::AttachLeft },
    Def { phrase: "dot dot dot", replacement: "…", spacing: Spacing::AttachLeft },
    // Currency / common symbols
    Def { phrase: "dollar sign", replacement: "$", spacing: Spacing::Normal },
    Def { phrase: "euro sign", replacement: "€", spacing: Spacing::Normal },
    Def { phrase: "pound sign", replacement: "£", spacing: Spacing::Normal },
    Def { phrase: "yen sign", replacement: "¥", spacing: Spacing::Normal },
    Def { phrase: "rupee sign", replacement: "₹", spacing: Spacing::Normal },
    Def { phrase: "degree sign", replacement: "°", spacing: Spacing::AttachLeft },
    Def { phrase: "degree symbol", replacement: "°", spacing: Spacing::AttachLeft },
    Def { phrase: "copyright", replacement: "©", spacing: Spacing::Normal },
    Def { phrase: "trademark", replacement: "™", spacing: Spacing::Normal },
    Def { phrase: "registered trademark", replacement: "®", spacing: Spacing::Normal },
];

/// Builds the full set of System AutoText entries. Each is a snippet-based
/// entry (the spoken phrase is the trigger) so it flows through the existing
/// snippet engine. Longest-phrase-first ordering is handled by the engine.
fn build() -> Vec<AutoTextEntry> {
    DEFS.iter()
        .map(|d| {
            let mut e = AutoTextEntry::new(String::new(), d.replacement);
            e.snippet = d.phrase.to_string();
            e.system = true;
            e
        })
        .collect()
}

/// The predefined System AutoText entries (built once).
pub fn entries() -> &'static [AutoTextEntry] {
    static ENTRIES: OnceLock<Vec<AutoTextEntry>> = OnceLock::new();
    ENTRIES.get_or_init(build)
}

/// The spacing rule for a System entry's phrase, or `Normal` if the phrase is
/// not a System entry.
pub fn spacing_for(phrase: &str) -> Spacing {
    DEFS
        .iter()
        .find(|d| d.phrase == phrase)
        .map(|d| d.spacing)
        .unwrap_or(Spacing::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_all_system_and_snippet_based() {
        let entries = entries();
        assert!(!entries.is_empty());
        for e in entries {
            assert!(e.system);
            assert!(e.enabled);
            assert!(!e.snippet.trim().is_empty());
        }
    }

    #[test]
    fn phrases_are_unique() {
        let entries = entries();
        let mut seen = std::collections::HashSet::new();
        for e in entries {
            assert!(seen.insert(e.snippet.trim().to_lowercase()), "dup: {}", e.snippet);
        }
    }

    #[test]
    fn spacing_lookup() {
        assert_eq!(spacing_for("comma"), Spacing::AttachLeft);
        assert_eq!(spacing_for("open paren"), Spacing::AttachRight);
        assert_eq!(spacing_for("unknown"), Spacing::Normal);
    }
}
