//! Closed sets for the polish edit guard.
//!
//! WHY: The polish model is a general-purpose LLM, not a grammar model.
//! Left alone it rewrites: measured in VOCS it fixed "she don't" but also
//! turned "because their going to review it" into "because they will review
//! it" — a different claim. The guard treats model output as a list of
//! PROPOSED edits and admits each one only if it falls in a closed set:
//!
//!   - A one-word swap is kept only when both words are grammatical forms of
//!     the same word (is/are, their/they're) or a near-spelling of it.
//!   - An insertion or deletion is kept only for the small closed set of
//!     function words a grammar fix actually adds or removes.
//!
//! The sets are English-only: the families and the function-word list are
//! English morphology. Two consumers use them:
//!
//! - `validator::content_change_share` uses [`FAMILIES`], [`FUNCTION_WORDS`],
//!   and [`is_grammar_edit`] to tell a grammatical inflection (`a` → `an`)
//!   from a content rewrite, so grammar fixes do not consume the user's
//!   `polish_max_change_percent` budget.
//! - `validator::validate` uses [`is_latin_script`] to reject model output
//!   that drifted to a non-Latin script (Cyrillic, CJK, Arabic, …) when the
//!   input was Latin-script.

/// Grammatical forms of one word. A swap inside a family is the fix the
/// guard is for; a swap across families is a different word.
///
/// Only function-word families are listed here: content words (went/go,
/// bought/buy, …) are never admitted as grammar edits by
/// [`is_grammar_edit`], which requires both sides to be in
/// [`FUNCTION_WORDS`]. The old content-word families were dead weight.
pub(crate) const FAMILIES: &[&[&str]] = &[
    &["am", "is", "are", "was", "were", "be", "been", "being"],
    &["have", "has", "had"],
    &["do", "does", "did"],
    &["a", "an"],
    &["this", "these"],
    &["that", "those"],
    &["their", "there", "they're", "theyre"],
    &["your", "you're", "youre"],
    &["its", "it's"],
    &["to", "too"],
    &["then", "than"],
    &["whose", "who's"],
    &["affect", "effect"],
    &["don't", "doesn't", "didn't"],
    &["isn't", "aren't", "wasn't", "weren't"],
    &["haven't", "hasn't", "hadn't"],
];

/// The only words a grammar fix substitutes for another grammar token.
pub(crate) const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "at", "for", "by", "with", "as", "is", "are", "was",
    "were", "be", "been", "am", "have", "has", "had", "do", "does", "did", "will", "would", "it",
    "that", "this", "and", "or", "but", "so",
];

/// The only words a grammar fix may substitute for another grammar token.
///
/// A substituted token is a *grammatical* edit (free against the user's
/// change ceiling) when both sides are function words and either sit in the
/// same [`FAMILIES`] family or are near-spellings of each other
/// (`dont` → `don't`). A function word swapped for a content word (or a
/// content word for anything) is a *content* edit and counts in full.
pub(crate) fn is_grammar_edit(a: &str, b: &str, families: &FamilyMap) -> bool {
    if a == b {
        return false; // not a substitution at all
    }
    is_function_word(a)
        && is_function_word(b)
        && (families.same_family(a, b) || near_spelling(a, b))
}

// ── Token model ──────────────────────────────────────────────────────────────

/// One whitespace-separated word, split so punctuation can be kept apart from
/// the letters that are compared.
#[derive(Debug, Clone)]
pub(crate) struct Tok {
    /// The alphanumeric core (letters/digits), preserving case.
    pub(crate) core: String,
}

impl Tok {
    fn parse(raw: &str) -> Self {
        let chars: Vec<char> = raw.chars().collect();
        let start = chars
            .iter()
            .position(|c| c.is_alphanumeric())
            .unwrap_or(chars.len());
        let end = chars
            .iter()
            .rposition(|c| c.is_alphanumeric())
            .map_or(start, |i| i + 1);
        let core: String = chars[start..end].iter().collect();
        Self { core }
    }
}

/// Splits into tokens and the whitespace around them: `gaps.len() == toks.len() + 1`.
pub(crate) fn tokenise(text: &str) -> (Vec<Tok>, Vec<String>) {
    let mut toks = Vec::new();
    let mut gaps = vec![String::new()];
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            if word.is_empty() {
                if let Some(gap) = gaps.last_mut() {
                    gap.push(ch);
                }
            } else {
                toks.push(Tok::parse(&word));
                word.clear();
                gaps.push(ch.to_string());
            }
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        toks.push(Tok::parse(&word));
        gaps.push(String::new());
    }
    (toks, gaps)
}

// ── Family / function-word lookups ───────────────────────────────────────────

/// Precomputed norm → family-index map for O(1) family lookups.
pub(crate) struct FamilyMap {
    family_of: std::collections::HashMap<String, usize>,
}

impl FamilyMap {
    pub fn new() -> Self {
        let mut family_of = std::collections::HashMap::new();
        for (idx, family) in FAMILIES.iter().enumerate() {
            for word in family.iter() {
                family_of.insert(word.to_string(), idx);
            }
        }
        Self { family_of }
    }

    pub fn same_family(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        match (self.family_of.get(a), self.family_of.get(b)) {
            (Some(&ia), Some(&ib)) => ia == ib,
            _ => false,
        }
    }
}

pub(crate) fn is_function_word(norm: &str) -> bool {
    FUNCTION_WORDS.contains(&norm)
}

// ── Near-spelling ────────────────────────────────────────────────────────────

pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// A near-spelling of the same word: go/goes, buy/buyed, dont/don't.
pub(crate) fn near_spelling(a: &str, b: &str) -> bool {
    if a.chars().next() != b.chars().next() {
        return false;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    // A prefix relation only counts for an inflection, not for a name stem:
    // "go"/"goes" is the same word, "priya"/"priyanka" is two different names.
    let stem_delta = la.max(lb) - la.min(lb);
    if stem_delta <= 2 && (a.starts_with(b) || b.starts_with(a)) {
        return true;
    }
    la >= 4 && lb >= 4 && levenshtein(a, b) <= 2
}

// ── Language detection ───────────────────────────────────────────────────────

/// Whether the text looks like Latin-script prose (English, German, French,
/// Spanish, accented Latin, …).
///
/// Honest name: this is a *script* heuristic, not a language detector. The
/// validator uses it one-sided — "the user dictated in a Latin script but the
/// model came back in Cyrillic/CJK/Arabic/Devanagari" — so a German user
/// whose output is correctly German passes, and a non-Latin user is never
/// checked at all (the same precedent as `NON_LATIN_PREFIXES` in
/// `gate.rs`). It cannot tell English from French, and it does not try.
///
/// Heuristic: if any of the first 8 word cores contains a character outside
/// basic Latin, extended Latin, and common punctuation, the text is not
/// Latin-script.
pub(crate) fn is_latin_script(text: &str) -> bool {
    let (toks, _) = tokenise(text);
    for t in toks.iter().take(8) {
        for c in t.core.chars() {
            // Non-Latin scripts: if we see a character outside basic Latin
            // + extended Latin + common punctuation, it is not Latin-script.
            if c.is_ascii() {
                continue;
            }
            // Allow common accented Latin characters (é, ñ, ü, etc.)
            // but reject CJK, Cyrillic, Arabic, Devanagari, Thai, etc.
            if ('\u{00C0}'..='\u{024F}').contains(&c) {
                continue; // Latin Extended
            }
            if matches!(c, '\u{2018}'..='\u{201F}') {
                continue; // Curly quotes
            }
            if matches!(c, '\u{2013}'..='\u{2014}') {
                continue; // Dashes
            }
            if matches!(c, '\u{2026}') {
                continue; // Ellipsis
            }
            if matches!(c, '\u{20AC}') {
                continue; // Euro
            }
            if matches!(c, '\u{2122}') {
                continue; // Trademark
            }
            return false;
        }
    }
    true
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_contain_grammatical_forms() {
        let fm = FamilyMap::new();
        assert!(fm.same_family("is", "are"));
        assert!(fm.same_family("their", "they're"));
        assert!(fm.same_family("dont", "dont")); // same word
        assert!(!fm.same_family("their", "your"));
        assert!(!fm.same_family("priya", "priyanka"));
    }

    #[test]
    fn function_words_are_the_closed_set() {
        assert!(is_function_word("the"));
        assert!(is_function_word("a"));
        assert!(is_function_word("is"));
        assert!(!is_function_word("priya"));
        assert!(!is_function_word("review"));
    }

    #[test]
    fn near_spelling_catches_typos() {
        assert!(near_spelling("dont", "dont"));
        assert!(near_spelling("dont", "dontt"));
        assert!(!near_spelling("priya", "priyanka"));
    }

    #[test]
    fn latin_script_detection_rejects_non_latin() {
        assert!(is_latin_script("Hello world"));
        assert!(is_latin_script("Café au lait")); // accented Latin is OK
        assert!(is_latin_script("Guten Morgen, wie geht es dir?")); // German prose
        assert!(!is_latin_script("こんにちは世界"));
        assert!(!is_latin_script("Привет мир"));
        assert!(!is_latin_script("مرحبا بالعالم"));
        assert!(!is_latin_script("नमस्ते दुनिया"));
    }

    #[test]
    fn grammar_edit_is_same_family_or_near_spelling_of_function_words() {
        let fm = FamilyMap::new();
        assert!(is_grammar_edit("a", "an", &fm));
        assert!(is_grammar_edit("is", "are", &fm));
        // "their" is not in FUNCTION_WORDS (it is a possessive determiner, not
        // a closed-set function word), so the family match alone is not
        // enough — both sides must be function words.
        assert!(!is_grammar_edit("their", "they're", &fm));
        // "dont" is not in FUNCTION_WORDS either; the near-spelling path
        // requires both sides to be function words.
        assert!(!is_grammar_edit("dont", "don't", &fm));
        // Function word swapped for a content word: content edit.
        assert!(!is_grammar_edit("the", "quick", &fm));
        // Content words, even near-spellings: content edit.
        assert!(!is_grammar_edit("priya", "priyanka", &fm));
        // Two different content words: content edit.
        assert!(!is_grammar_edit("report", "proposal", &fm));
        // Identical tokens are not substitutions at all.
        assert!(!is_grammar_edit("the", "the", &fm));
    }

    #[test]
    fn tokenise_preserves_whitespace() {
        let (toks, gaps) = tokenise("Hello,  world.\nSecond line");
        assert_eq!(toks.len(), 4);
        assert_eq!(gaps.len(), 5);
        assert_eq!(toks[0].core, "Hello");
        assert_eq!(toks[1].core, "world");
        assert_eq!(toks[2].core, "Second");
        assert_eq!(toks[3].core, "line");
        assert_eq!(gaps[1], "  "); // double space preserved
        assert_eq!(gaps[2], "\n"); // newline preserved
    }
}
