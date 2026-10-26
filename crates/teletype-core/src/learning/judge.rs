//! Classify word-level replacements from user edits (Phase 7, P7.3).
//!
//! Given a `pasted` (dictated) vs `observed` (user-edited) pair, the diff
//! yields replacement spans; the judge classifies each span as a
//! mishearing (learn into the lexicon), a style preference, a rewrite
//! (learn as an example), or something to skip entirely (never learn).
//!
//! Pure logic + embedded data files — no IO, no DB.

use std::collections::HashSet;
use std::sync::OnceLock;

use rphonetic::DoubleMetaphone;

use super::diff::{changed_pairs, diff_words};

/// Classification of one replacement span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// `from` was a mishearing of `to` (learn `to` as a term, `from` as alias).
    Mishearing { term: String, alias: String },
    /// A stylistic preference (casing, punctuation, greeting, …).
    Style { kind: StyleKind },
    /// A larger rewording — candidate for example-based learning.
    Rewrite,
    /// In the 0.3–0.6 similarity band; a later LLM judge unit decides.
    Ambiguous,
    /// Never learn this span.
    Skip { reason: SkipReason },
}

/// Flavour of a style edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    Casing,
    Punctuation,
    Contraction,
    Greeting,
    Signoff,
    ListFormat,
}

/// Why a span must never be learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    FilePathLike,
    UrlLike,
    EmailPartial,
    CodeSymbol,
    KeyOrHashLike,
    NumberOrDate,
    /// A number word corrected to the digits it reads as ("two" → "2") — a
    /// number formatting preference, never a mishearing to learn.
    NumberWordReading,
    MarkdownOrEmoji,
    EverydayWord,
    CommonFirstName,
    EverydayAcronym,
}

/// Context knobs for the judge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JudgeContext {
    /// When set, a mishearing requires `to` to be proper-noun-like unless
    /// the confidence is below 0.9.
    pub min_conf: Option<f32>,
    /// Foreground app category hint (reserved for later units).
    pub app_category: Option<String>,
    /// When true, a phonetically close correction may be learned even when
    /// `to` is not proper-noun-shaped (all-lowercase vocabulary such as
    /// "tauri" or "webhook"). Restricted to spans of one or two words by the
    /// judge so a lowercase rewrite cannot become a term.
    pub allow_lowercase_terms: bool,
}

// ── Data files ─────────────────────────────────────────────────────────────

fn common_words() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| parse_words(include_str!("data/common_words.txt")))
}

fn first_names() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| parse_words(include_str!("data/first_names.txt")))
}

fn acronyms() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| parse_words(include_str!("data/acronyms.txt")))
}

fn parse_words(raw: &str) -> HashSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_lowercase())
        .collect()
}

// ── Phonetic similarity ────────────────────────────────────────────────────

/// Combined phonetic + orthographic similarity in `0.0..=1.0`.
///
/// Formula (mirrors `rescoring.rs` pass 2, adapted for single words):
/// `0.5 * metaphone_sim + 0.5 * jaro_winkler`, where `metaphone_sim` is
/// `1 - levenshtein(dm_primary(a), dm_primary(b)) / max(len_a, len_b)`
/// (1.0 when both codes are empty) and `jaro_winkler` is computed on the
/// lowercased inputs. Identical strings score 1.0; unrelated words land
/// well below 0.6.
pub fn phon_sim(a: &str, b: &str) -> f32 {
    static DM: OnceLock<DoubleMetaphone> = OnceLock::new();
    let dm = DM.get_or_init(|| DoubleMetaphone::new(None));
    let (la, lb) = (a.to_lowercase(), b.to_lowercase());
    let (ca, cb) = (
        dm.double_metaphone(&la).primary(),
        dm.double_metaphone(&lb).primary(),
    );
    let max_len = ca.len().max(cb.len());
    let lev = strsim::levenshtein(&ca, &cb);
    let meta = if max_len == 0 {
        1.0
    } else {
        1.0 - lev as f64 / max_len as f64
    };
    let jw = strsim::jaro_winkler(&la, &lb);
    (0.5 * meta + 0.5 * jw) as f32
}

// ── Skip-list checks ───────────────────────────────────────────────────────

/// Returns `Some(reason)` if the span must never be learned.
/// Checks the `to` side first (what we would learn), then `from`.
/// Structural patterns (paths, URLs, code, numbers, …) are checked
/// first so they always win; the word lists (acronyms, first names,
/// everyday words) only apply when neither side is structural.
fn skip_reason(from: &str, to: &str) -> Option<SkipReason> {
    for w in [to, from] {
        if let Some(r) = structural_skip(w) {
            return Some(r);
        }
    }
    // Category-major over both sides: an acronym on either side beats a
    // first name, which beats an everyday word ("URL" → "URLS" is an
    // acronym edit, not an everyday word, even though "urls" is a word).
    let lows: [String; 2] = [to.to_lowercase(), from.to_lowercase()];
    if lows.iter().any(|l| acronyms().contains(l)) {
        return Some(SkipReason::EverydayAcronym);
    }
    if lows.iter().any(|l| first_names().contains(l)) {
        return Some(SkipReason::CommonFirstName);
    }
    if lows.iter().any(|l| common_words().contains(l)) {
        return Some(SkipReason::EverydayWord);
    }
    None
}

/// Word-list skip only (no structural checks). Used for mishearing
/// classification: a learned `to` must not be an everyday word, common
/// first name, or everyday acronym.
fn word_list_skip(w: &str) -> Option<SkipReason> {
    let lower = w.to_lowercase();
    if acronyms().contains(&lower) {
        return Some(SkipReason::EverydayAcronym);
    }
    if first_names().contains(&lower) {
        return Some(SkipReason::CommonFirstName);
    }
    if common_words().contains(&lower) {
        return Some(SkipReason::EverydayWord);
    }
    None
}

/// Structural (pattern-based) skip checks: paths, URLs, emails, code,
/// keys/hashes, numbers/dates, markdown/emoji.
fn structural_skip(w: &str) -> Option<SkipReason> {
    let lower = w.to_lowercase();
    // URLs first: a URL contains '/' and must not be classified as a path.
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || looks_like_domain(&lower)
    {
        return Some(SkipReason::UrlLike);
    }
    if lower.contains('/') || lower.contains('\\') {
        return Some(SkipReason::FilePathLike);
    }
    if lower.contains('@') {
        return Some(SkipReason::EmailPartial);
    }
    if hex_key_like(&lower) || key_or_hash_like(&lower) {
        return Some(SkipReason::KeyOrHashLike);
    }
    if lower.contains("::")
        || lower.contains("->")
        || (lower.contains('(') && lower.contains(')'))
        || (lower.contains('{') && lower.contains('}'))
        || snake_case_heavy(&lower)
    {
        return Some(SkipReason::CodeSymbol);
    }
    if number_or_date_like(&lower) {
        return Some(SkipReason::NumberOrDate);
    }
    if markdown_or_emoji(&lower) {
        return Some(SkipReason::MarkdownOrEmoji);
    }
    None
}

fn looks_like_domain(s: &str) -> bool {
    let s = s.trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && parts.last().is_some_and(|tld| tld.len() >= 2)
        && s.contains('.')
}

fn snake_case_heavy(s: &str) -> bool {
    // `foo_bar_baz` — at least two underscores joining alpha runs.
    let n = s.matches('_').count();
    n >= 2 && s.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 3
}

/// All-hex blobs (keys, hashes): 12+ hex characters with at least one
/// digit. The vowel heuristic in [`key_or_hash_like`] can reject short
/// letter/digit mixes ("0123456789ab" has one vowel in two letters), so
/// all-hex strings get their own exact check — and they must win over
/// `NumberOrDate`, because a digit-heavy hex blob is a key, not a number.
fn hex_key_like(s: &str) -> bool {
    s.len() >= 12
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn key_or_hash_like(s: &str) -> bool {
    if s.len() < 12 {
        return false;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return false;
    }
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    let digits: Vec<char> = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let vowels = letters
        .iter()
        .filter(|c| matches!(**c, 'a' | 'e' | 'i' | 'o' | 'u'))
        .count();
    // hex/base64-ish: digits present, vowels rare relative to letters
    !digits.is_empty() && vowels <= letters.len() / 4
}

/// The integer that English number-word phrases spell, if one. Covers the
/// forms ASR actually emits: units 0–19, hundreds ("two hundred [and] 10"),
/// tens ("twenty", "thirty"), thousands ("one thousand", "thousand"), and
/// "million"/"billion". Hyphens, commas, spaces and surrounding
/// punctuation are ignored.
///
/// Port of EnviousWispr's `numberWordReadings` (`CorrectionCandidateFilter
/// .swift:232-236,306`, commit `faad17d1`): when the user's correction turns
/// a spelled number into its digit form ("two" → "2"), that is number
/// formatting, not a mishearing — learning it would teach the engine to
/// write digits for words the user wanted as words.
pub fn number_word_reading(s: &str) -> Option<u64> {
    // Commas separate number words ("two, three" is not a phrase); hyphens
    // bind tens and units into a single word ("twenty-three" → 23).
    let normalized = s
        .to_lowercase()
        .replace([',', '-'], " ");
    let words: Vec<&str> = normalized
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }

    const UNITS: [(&str, u64); 20] = [
        ("zero", 0),
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
    ];
    const TENS: [(&str, u64); 8] = [
        ("twenty", 20),
        ("thirty", 30),
        ("forty", 40),
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
    ];
    const SCALE: [(&str, u64); 4] = [
        ("hundred", 100),
        ("thousand", 1_000),
        ("million", 1_000_000),
        ("billion", 1_000_000_000),
    ];

    fn unit(w: &str) -> Option<u64> {
        if w == "zero" {
            return Some(0);
        }
        UNITS.iter()
            .find(|(n, _)| *n == w)
            .map(|(_, v)| *v)
            .filter(|&v| v > 0)
    }

    fn scale_rank(v: u64) -> usize {
        match v {
            100 => 0,
            1_000 => 1,
            1_000_000 => 2,
            _ => 3,
        }
    }

    // Parse low-to-high: "two hundred [and] ten" → 210, "one thousand" →
    // 1000, "a hundred" → 100. `current` holds the unit part that `hundred`
    // scales later, so "two hundred thousand" folds as (2 × 100) × 1000.
    // Rejects non-number phrases ("twenty thirty", "hundred thousand").
    let mut total: u64 = 0;
    let mut current: u64 = 0;
    let mut saw_any = false;
    let mut last_scale: Option<usize> = None;
    // Set when `hundred` was bare ("hundred" / "a hundred", no multiplier),
    // so a later larger scale cannot multiply it ("hundred thousand" is not
    // a plain phrase, "two hundred thousand" is).
    let mut bare_hundred: bool = false;
    for w in &words {
        if *w == "and" || *w == "a" || *w == "an" {
            continue;
        }
        if let Some(v) = SCALE.iter().find(|(n, _)| *n == *w).map(|(_, v)| *v) {
            let rank = scale_rank(v);
            if v == 100 {
                // `hundred` scales the current unit part ("two hundred" →
                // 2 × 100); bare "hundred" / "a hundred" → 100. It may only
                // appear once per phrase, after a small part.
                let bare = current == 0;
                if current == 0 {
                    if total != 0 {
                        return None;
                    }
                    current = 1;
                }
                if current >= 100 {
                    return None; // "a hundred hundred" is not a phrase
                }
                // "hundred" is the smallest scale, so a larger group before
                // it is valid ("one thousand two hundred" → 1200), but a
                // hundred never follows another hundred ("million hundred" /
                // "thousand hundred" are not phrases). Reject only when a
                // scale of rank <= this one was already placed.
                if let Some(s) = last_scale {
                    if s <= rank {
                        return None; // "hundred hundred" family
                    }
                }
                if bare {
                    bare_hundred = true;
                }
                current *= 100;
                saw_any = true;
                continue;
            }
            // Thousand/million/billion need a whole part in front of them;
            // only a *leading* scale (nothing before it) may be bare
            // ("thousand" → 1000). A scale after another scale ("hundred
            // thousand") is not a plain number phrase.
            if current == 0 {
                if total != 0 {
                    return None;
                }
                current = 1;
            }
            let factor = current;
            // The part multiplied must be strictly below the scale ("two
            // thousand" → 2 < 1000; "two million thousand" → 1_000_000 is
            // not < 1000). A bare hundred ("hundred") may not be the part a
            // larger scale multiplies ("hundred thousand" is not a phrase,
            // "two hundred thousand" is).
            if factor >= v || bare_hundred {
                return None;
            }
            // Scale steps must stay in order: "million hundred" is not a
            // phrase, and a scale may only attach to the part of the rank
            // just below it ("two thousand", not "two hundred thousand").
            if let Some(s) = last_scale {
                if s >= rank {
                    return None;
                }
                let step_ok = match (s, rank) {
                    (0, 1) => factor.is_multiple_of(100),
                    (1, 2) => factor.is_multiple_of(1_000),
                    (2, 3) => factor.is_multiple_of(1_000_000),
                    _ => true,
                };
                if !step_ok {
                    return None;
                }
            }
            if total > u64::MAX / (factor * v) {
                return None;
            }
            total += factor * v;
            current = 0;
            last_scale = Some(rank);
            saw_any = true;
            continue;
        }
        if let Some(v) = TENS.iter().find(|(n, _)| *n == *w).map(|(_, v)| *v) {
            if current >= 20 || current + v >= 100 {
                return None; // "twenty thirty" is not a number phrase
            }
            current += v;
            saw_any = true;
            continue;
        }
        if let Some(v) = unit(w) {
            // Units attach to the current part; they never bridge a scale,
            // except the "and" bridge of "two hundred and ten" (210).
            if current >= 100 && !current.is_multiple_of(100) {
                return None;
            }
            // A bare ten word ("twenty", "ninety") is in TENS, not the unit
            // table, so a value >= 10 here means the "ten"/"eleven"... form.
            // After a bare ten (current 20..=99) a small unit compounds the
            // tens ("twenty-three" → 23), but a ten-unit ("twenty ten") and
            // an over-99 roll ("ninety ten" → 100) are not phrases. After a
            // hundred (current >= 100) only the small "and" unit may follow
            // ("two hundred and ten" → 210); a ten-unit ("two hundred
            // twenty" → 220) is not read as plain digits.
            let is_tens_word = TENS.iter().any(|(n, _)| *n == *w);
            if ((20..100).contains(&current) && (is_tens_word || current + v >= 100))
                || (current >= 100 && is_tens_word)
            {
                return None;
            }
            current += v;
            saw_any = true;
            continue;
        }
        // Any other word: the span is not a pure number phrase.
        return None;
    }
    if !saw_any {
        return None;
    }
    // A pending `hundred` group ("two hundred") is still in `current`.
    Some(total + current)
}

/// True when `from` is an English number-word phrase and `to` is exactly the
/// digits it reads as ("two" → "2", "fourteen" → "14", "two hundred" → "200").
/// Such a span is number formatting, not a mishearing: the engine should not
/// be taught to write digits where the user wrote a word.
pub fn is_number_word_formatting(from: &str, to: &str) -> bool {
    if !to
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ','))
        || !to.chars().any(|c| c.is_ascii_digit())
    {
        return false;
    }
    let digits: String = to.chars().filter(|c| c.is_ascii_digit()).collect();
    let Ok(parsed) = digits.parse::<u64>() else {
        return false;
    };
    number_word_reading(from) == Some(parsed)
}

fn number_or_date_like(s: &str) -> bool {
    // List markers ("1.", "2)", "10.") are not numbers — exempt them.
    if is_list_marker(s) {
        return false;
    }
    let trimmed = s.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-');
    if trimmed.is_empty() {
        return false;
    }
    if !trimmed.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    // A pure digit run is always a number.
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let digits = trimmed.chars().filter(|c| c.is_ascii_digit()).count();
    // Mostly digits (ids, dates, times, prices) — allow $, ., :, -, noise.
    // Two digits minimum: a lone digit in a short mixed token ("ce0",
    // "ap1") is a typo'd acronym or word, not a number.
    digits >= 2 && (digits >= trimmed.chars().count() / 2 || trimmed.chars().any(|c| c == ':'))
}

fn markdown_or_emoji(s: &str) -> bool {
    if s.starts_with('#') || s.starts_with("----") {
        return true;
    }
    if s.starts_with("- ") || s.starts_with("* ") || s.starts_with("+ ") {
        return true;
    }
    s.chars().any(|c| {
        let cp = c as u32;
        matches!(
            cp,
            0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF
        )
    })
}

// ── Style detection ────────────────────────────────────────────────────────

const CONTRACTIONS: &[&str] = &[
    "can't",
    "cannot",
    "won't",
    "will not",
    "don't",
    "do not",
    "didn't",
    "did not",
    "isn't",
    "is not",
    "aren't",
    "are not",
    "wasn't",
    "was not",
    "weren't",
    "were not",
    "haven't",
    "have not",
    "hasn't",
    "has not",
    "hadn't",
    "had not",
    "wouldn't",
    "would not",
    "shouldn't",
    "should not",
    "couldn't",
    "could not",
    "i'm",
    "i am",
    "i've",
    "i have",
    "i'll",
    "i will",
    "you're",
    "you are",
    "you've",
    "you have",
    "it's",
    "it is",
    "that's",
    "that is",
    "there's",
    "there is",
    "here's",
    "here is",
    "let's",
    "let us",
    "what's",
    "what is",
    "who's",
    "who is",
    "why's",
    "why is",
    "how's",
    "how is",
    "we're",
    "we are",
    "we've",
    "we have",
    "we'll",
    "we will",
    "they're",
    "they are",
    "they've",
    "they have",
    "they'll",
    "they will",
];

fn strip_punct(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

fn is_salutation_word(w: &str) -> bool {
    const SALUTATIONS: &[&str] = &["hi", "hello", "hey", "dear", "good", "greetings"];
    SALUTATIONS.contains(&w.to_lowercase().as_str())
}

fn is_closing_span(words: &[String]) -> bool {
    const CLOSINGS: &[&str] = &[
        "best regards",
        "regards",
        "best",
        "thanks",
        "thank you",
        "sincerely",
        "cheers",
        "all the best",
        "kind regards",
        "warm regards",
        "yours",
    ];
    let joined = words
        .iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = joined.trim_end_matches(['.', ',']);
    CLOSINGS.contains(&trimmed)
}

fn is_list_marker(w: &str) -> bool {
    // List markers: "1.", "2)", "10.", etc. — digits followed by . or )
    let t = w.trim_end_matches(['.', ')']);
    !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit())
        && t.len() <= 3
        && (w.ends_with('.') || w.ends_with(')'))
}

/// Style classification for a span, or `None` if not a style edit.
fn style_kind(from: &[String], to: &[String]) -> Option<StyleKind> {
    // Greeting / sign-off (word count may differ: "Best regards" → "Cheers").
    {
        let f_body: Vec<String> = from.iter().map(|w| strip_punct(w)).collect();
        let t_body: Vec<String> = to.iter().map(|w| strip_punct(w)).collect();
        let f_greet = f_body.first().is_some_and(|w| is_salutation_word(w));
        let t_greet = t_body.first().is_some_and(|w| is_salutation_word(w));
        let f_close = is_closing_span(&f_body);
        let t_close = is_closing_span(&t_body);
        if (f_greet && t_greet) || (f_close && t_close) {
            return Some(if f_greet {
                StyleKind::Greeting
            } else {
                StyleKind::Signoff
            });
        }
    }
    // List formatting: list markers added or removed (word count may differ).
    {
        let f_body: Vec<String> = from
            .iter()
            .filter(|w| !is_list_marker(w))
            .map(|w| strip_punct(w.as_str()))
            .collect();
        let t_body: Vec<String> = to
            .iter()
            .filter(|w| !is_list_marker(w))
            .map(|w| strip_punct(w.as_str()))
            .collect();
        let f_any = from.iter().any(|w| is_list_marker(w));
        let t_any = to.iter().any(|w| is_list_marker(w));
        if ((f_any && !t_any) || (!f_any && t_any)) && f_body == t_body {
            return Some(StyleKind::ListFormat);
        }
    }
    // Hyphenation: "e mail" → "e-mail" (N words → 1 hyphenated word, or vice versa).
    if from.len() != to.len() {
        let f_join: String = from
            .iter()
            .map(|w| w.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let t_join: String = to
            .iter()
            .map(|w| w.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        // Try joining with hyphen: "e mail" → "e-mail"
        let f_hyphen = f_join.replace(' ', "-");
        let t_hyphen = t_join.replace(' ', "-");
        if f_hyphen == t_hyphen || f_join.replace('-', " ") == t_join.replace('-', " ") {
            return Some(StyleKind::Punctuation);
        }
    }
    // Same word count, 1:1 comparison.
    if from.len() == to.len() {
        let mut casing = 0;
        let mut punct = 0;
        let mut other = 0;
        for (f, t) in from.iter().zip(to.iter()) {
            let (fs, ts) = (strip_punct(f), strip_punct(t));
            if f == t {
                continue;
            }
            if fs == ts {
                if f.chars().any(|c| c.is_uppercase()) != t.chars().any(|c| c.is_uppercase()) {
                    casing += 1;
                } else {
                    punct += 1;
                }
            } else if contraction_pair(&fs, &ts) {
                punct += 1; // contraction handled below via any
            } else {
                other += 1;
            }
        }
        if other == 0 && casing + punct > 0 {
            // A pure case flip on an otherwise-identical span where `to` is
            // proper-noun-like is a mishearing correction of a name
            // ("siobhan" → "Siobhan"), not a style preference — but only
            // when the target is not itself an everyday word, first name,
            // or acronym ("apple" → "Apple" stays Style{Casing}: there is
            // no term to learn). If punctuation also changed
            // ("apple" → "Apple."), it stays Style{Punctuation}.
            if casing > 0
                && punct == 0
                && to.iter().any(|w| proper_noun_like(w))
                && to.iter().all(|w| word_list_skip(w).is_none())
            {
                return None;
            }
            return Some(if casing >= punct {
                StyleKind::Casing
            } else {
                StyleKind::Punctuation
            });
        }
    }
    // Contraction ↔ two-word form.
    let f_join: Vec<String> = from.iter().map(|w| strip_punct(w)).collect();
    let t_join: Vec<String> = to.iter().map(|w| strip_punct(w)).collect();
    if f_join.len() == 1
        && t_join.len() == 2
        && contraction_pair(&f_join[0], &format!("{} {}", t_join[0], t_join[1]))
    {
        return Some(StyleKind::Contraction);
    }
    if t_join.len() == 1
        && f_join.len() == 2
        && contraction_pair(&t_join[0], &format!("{} {}", f_join[0], f_join[1]))
    {
        return Some(StyleKind::Contraction);
    }
    None
}

fn contraction_pair(a: &str, b: &str) -> bool {
    let a = a.replace('’', "'");
    let b = b.replace('’', "'");
    CONTRACTIONS
        .chunks_exact(2)
        .any(|pair| pair[0] == a && pair[1] == b || pair[1] == a && pair[0] == b)
}

// ── Proper-noun heuristics ─────────────────────────────────────────────────

/// `to` looks like a proper noun: CamelCase, contains a digit, or is
/// capitalized. (Mid-phrase capitalization needs the full sentence, which
/// the span-level judge doesn't see; capitalization of the token itself
/// and CamelCase cover the common cases.)
fn proper_noun_like(w: &str) -> bool {
    if w.is_empty() {
        return false;
    }
    if w.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if w.chars().next().is_some_and(|c| c.is_uppercase()) && w.len() > 1 {
        return true;
    }
    // CamelCase: an interior uppercase
    w[1..].chars().any(|c| c.is_uppercase())
}

/// Join a span back into a phrase (spaces only).
fn join(words: &[String]) -> String {
    words.join(" ")
}

/// Classify one replacement span.
///
/// Order: structural/word-list Skip → Style (casing, punctuation,
/// contraction, greeting, sign-off, list format) → phonetic similarity
/// bands (Rewrite / Mishearing / Ambiguous). Style edits are detected
/// before the word lists: a casing or punctuation change on an everyday
/// word ("apple" → "Apple") is a style preference, not a skip.
pub fn judge_replace(from: &str, to: &str, ctx: &JudgeContext) -> Judgement {
    let from_words: Vec<String> = from.split_whitespace().map(String::from).collect();
    let to_words: Vec<String> = to.split_whitespace().map(String::from).collect();

    // Number formatting first: "two" → "2" is a style preference, never a
    // mishearing to learn (port of `numberWordReadings`, commit faad17d1).
    // Checked before the structural skips so the digits target isn't claimed
    // by `NumberOrDate`; it only fires on single-word spans whose `from` is
    // an English number phrase and `to` is exactly those digits.
    if from_words.len() == 1
        && to_words.len() == 1
        && is_number_word_formatting(from, to)
    {
        return Judgement::Skip {
            reason: SkipReason::NumberWordReading,
        };
    }

    // Structural skips always win (paths, URLs, code, numbers, …).
    // Check the whole span AND each individual word.
    for w in [to, from] {
        if let Some(reason) = structural_skip(w) {
            return Judgement::Skip { reason };
        }
        for word in w.split_whitespace() {
            if let Some(reason) = structural_skip(word) {
                return Judgement::Skip { reason };
            }
        }
    }

    // Style edits (casing/punctuation/contraction/greeting/signoff/list)
    // take priority over word-list skips: the word sequence is otherwise
    // equal, so there is no term to learn.
    if let Some(kind) = style_kind(&from_words, &to_words) {
        return Judgement::Style { kind };
    }

    // Word-list skips (everyday words, first names, acronyms) only apply
    // to single-word spans: a multi-word span is a rewrite or ambiguous,
    // not a skip based on individual words being everyday.
    if from_words.len() == 1 && to_words.len() == 1 {
        if let Some(reason) = skip_reason(from, to) {
            return Judgement::Skip { reason };
        }
    }

    let sim = phon_sim(from, to);
    if from_words.len() > 4 || to_words.len() > 4 || sim < 0.3 {
        return Judgement::Rewrite;
    }

    let proper = proper_noun_like(to);
    let low_conf = ctx.min_conf.is_some_and(|c| c < 0.9);
    // Restrict lowercase-term learning to spans of 1–2 words on the
    // correction side so a lowercase rewrite cannot become a term.
    let lowercase_ok = ctx.allow_lowercase_terms && to_words.len() <= 2;
    if sim >= 0.6 && (low_conf || proper || lowercase_ok) && word_list_skip(to).is_none() {
        return Judgement::Mishearing {
            term: join(&to_words),
            alias: join(&from_words),
        };
    }
    Judgement::Ambiguous
}

/// Run the full pipeline: diff, candidate pairs, per-pair judgement.
/// Insert/delete-only edits classify as `Rewrite` (nothing to learn).
pub fn classify(pasted: &str, observed: &str, ctx: &JudgeContext) -> Vec<Judgement> {
    let ops = diff_words(pasted, observed);
    let pairs = changed_pairs(&ops);
    if pairs.is_empty() {
        if ops
            .iter()
            .any(|op| !matches!(op, super::diff::EditOp::Equal(_)))
        {
            return vec![Judgement::Rewrite];
        }
        return Vec::new();
    }
    pairs
        .iter()
        .map(|(f, t)| judge_replace(&join(f), &join(t), ctx))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> JudgeContext {
        JudgeContext::default()
    }

    #[test]
    fn number_word_reading_table() {
        let table: &[(&str, Option<u64>)] = &[
            ("two", Some(2)),
            ("fourteen", Some(14)),
            ("twenty", Some(20)),
            ("twenty-three", Some(23)),
            ("thirty five", Some(35)),
            ("hundred", Some(100)),
            ("a hundred", Some(100)),
            ("two hundred", Some(200)),
            ("two hundred and ten", Some(210)),
            ("one thousand", Some(1000)),
            ("thousand", Some(1000)),
            ("one thousand two hundred", Some(1200)),
            ("million", Some(1_000_000)),
            ("zero", Some(0)),
            ("PyTorch", None),
            ("twenty thirty", None),
            ("hundred thousand", None),
            ("two hundred thousand", Some(200_000)),
            ("one million", Some(1_000_000)),
            ("two million", Some(2_000_000)),
            ("and", None),
            ("", None),
            ("a b c", None),
            ("two 14", None),
        ];
        for (s, expected) in table {
            assert_eq!(number_word_reading(s), *expected, "number_word_reading({s:?})");
        }
    }

    #[test]
    fn number_word_formatting_pair_table() {
        let yes: &[(&str, &str)] = &[
            ("two", "2"),
            ("fourteen", "14"),
            ("twenty-three", "23"),
            ("thirty five", "35"),
            ("two hundred", "200"),
            ("one thousand", "1000"),
        ];
        let no: &[(&str, &str)] = &[
            ("two", "3"),
            ("two", "to"),
            ("kafka", "2"),
            ("fourteen", "15"),
            ("two hundred", "2"),
            ("twenty", "twenty"),
        ];
        for (f, t) in yes {
            assert!(is_number_word_formatting(f, t), "{f} → {t} should be number formatting");
        }
        for (f, t) in no {
            assert!(!is_number_word_formatting(f, t), "{f} → {t} should not be number formatting");
        }
    }

    #[test]
    fn judge_number_word_to_digits_is_skip() {
        let j = judge_replace("two", "2", &ctx());
        assert_eq!(
            j,
            Judgement::Skip {
                reason: SkipReason::NumberWordReading
            }
        );
        let j = judge_replace("fourteen", "14", &ctx());
        assert_eq!(
            j,
            Judgement::Skip {
                reason: SkipReason::NumberWordReading
            }
        );
    }

    #[test]
    fn judge_word_to_different_digits_not_number_skip() {
        // Different value is not number formatting; may be a normal mishearing
        // or ambiguous, but never the NumberWordReading skip.
        let j = judge_replace("two", "3", &ctx());
        assert_ne!(
            j,
            Judgement::Skip {
                reason: SkipReason::NumberWordReading
            }
        );
    }

    #[test]
    fn phon_sim_table() {
        // (a, b, lo, hi) — bounds chosen from the documented formula.
        let table: &[(&str, &str, f32, f32)] = &[
            ("siobhan", "shivon", 0.45, 1.0),
            ("pytorch", "pie torch", 0.40, 1.0),
            ("kate", "katie", 0.60, 1.0),
            ("xavier", "havier", 0.55, 1.0),
            ("jupyter", "ju pyter", 0.40, 1.0),
            ("sarah", "sara h", 0.40, 1.0),
            ("anthropic", "antropic", 0.70, 1.0),
            ("github", "git hub", 0.35, 1.0),
            ("docker", "dockr", 0.55, 1.0),
            ("kubernetes", "kube netes", 0.40, 1.0),
            ("tensorflow", "ten sor flow", 0.40, 1.0),
            ("postgres", "post gress", 0.40, 1.0),
            ("mongodb", "mongo db", 0.35, 1.0),
            ("elasticsearch", "elastic search", 0.35, 1.0),
            ("redis", "red is", 0.40, 1.0),
            ("webpack", "web pack", 0.40, 1.0),
            ("typescript", "type script", 0.40, 1.0),
            ("kafka", "cafka", 0.60, 1.0),
            ("rabbitmq", "rabbit em queue", 0.35, 1.0),
            ("grpc", "jrpc", 0.45, 1.0),
            ("oauth", "oh auth", 0.40, 1.0),
            ("graphql", "g r a p h q l", 0.30, 1.0),
            ("hello", "hello", 0.99, 1.0),
            ("hello", "world", 0.0, 0.60),
            ("abc", "abcd", 0.55, 1.0),
        ];
        assert!(table.len() >= 20);
        for (a, b, lo, hi) in table {
            let s = phon_sim(a, b);
            assert!(
                (*lo..=*hi).contains(&s),
                "phon_sim({a:?}, {b:?}) = {s} not in {lo}..={hi}"
            );
        }
    }

    #[test]
    fn phon_sim_bounds() {
        assert!((0.0..=1.0).contains(&phon_sim("xyz", "zzz")));
        assert!((0.0..=1.0).contains(&phon_sim("", "a")));
    }

    #[test]
    fn mishearing_rare_term() {
        let j = judge_replace("pie torch", "PyTorch", &ctx());
        assert_eq!(
            j,
            Judgement::Mishearing {
                term: "PyTorch".into(),
                alias: "pie torch".into()
            }
        );
    }

    #[test]
    fn mishearing_real_asr_name_pair() {
        // Taken from the names.wav fixture: the engine heard "Shavano". The
        // user's correction is the phonetic respelling "Shivon"; the Irish
        // "Siobhan" itself scores below the 0.6 phonetic band.
        let j = judge_replace("Shavano", "Shivon", &ctx());
        assert!(
            matches!(j, Judgement::Mishearing { .. }),
            "expected a mishearing, got {j:?}"
        );
    }

    #[test]
    fn mishearing_needs_low_conf_or_proper() {
        // No ctx, "kafka"->"cafka": both lowercase, sim high, but `to` not
        // proper-noun-like and no min_conf — ambiguous band per the rules.
        let j = judge_replace("cafka", "kafka", &ctx());
        assert_eq!(j, Judgement::Ambiguous);
        let low = JudgeContext {
            min_conf: Some(0.5),
            ..Default::default()
        };
        assert!(matches!(
            judge_replace("cafka", "kafka", &low),
            Judgement::Mishearing { .. }
        ));
    }

    #[test]
    fn style_casing() {
        let j = judge_replace("apple", "Apple", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Casing
            }
        );
    }

    #[test]
    fn style_punctuation() {
        let j = judge_replace("john", "john.", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Punctuation
            }
        );
        let j = judge_replace("e mail", "e-mail", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Punctuation
            }
        );
    }

    #[test]
    fn style_contraction() {
        let j = judge_replace("do not", "don't", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Contraction
            }
        );
        let j = judge_replace("won't", "will not", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Contraction
            }
        );
    }

    #[test]
    fn style_greeting_and_signoff() {
        let j = judge_replace("Hi", "Hello", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Greeting
            }
        );
        let j = judge_replace("Best regards", "Cheers", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::Signoff
            }
        );
    }

    #[test]
    fn style_list_format() {
        let j = judge_replace("one two three", "1. one 2. two 3. three", &ctx());
        // 3 vs 6 words: marker filtering keeps bodies equal via the
        // 1:1 path only when counts match; here counts differ, so this
        // falls through to phon_sim. Assert it is NOT a mishearing.
        assert!(!matches!(j, Judgement::Mishearing { .. }));
        // Equal-count case:
        let j = judge_replace("one", "1. one", &ctx());
        assert_eq!(
            j,
            Judgement::Style {
                kind: StyleKind::ListFormat
            }
        );
    }

    #[test]
    fn rewrite_far_apart() {
        // > 4 words per side → Rewrite.
        let j = judge_replace("a b c d e f", "x y z", &ctx());
        assert_eq!(j, Judgement::Rewrite);
        // Very different words, sim < 0.3 → Rewrite.
        let j = judge_replace("xylophone", "zzzqqq", &ctx());
        assert_eq!(j, Judgement::Rewrite);
    }

    #[test]
    fn ambiguous_mid_band() {
        // "night" vs "knight": both everyday words → Skip.
        // Use non-everyday words for a true ambiguous band test:
        // "pytorch" vs "pie torch" — high sim, lowercase `to`, no conf →
        // but `to` is not proper-noun-like → Ambiguous per the rules.
        let j = judge_replace("night", "knight", &ctx());
        assert_eq!(
            j,
            Judgement::Skip {
                reason: SkipReason::EverydayWord
            }
        );
        let j = judge_replace("pytorch", "pie torch", &ctx());
        assert_eq!(j, Judgement::Ambiguous);
    }

    #[test]
    fn skip_urls_paths_code() {
        assert_eq!(
            judge_replace("github.com", "git hub.com", &ctx()),
            Judgement::Skip {
                reason: SkipReason::UrlLike
            }
        );
        assert_eq!(
            judge_replace("src/main.rs", "src/main2.rs", &ctx()),
            Judgement::Skip {
                reason: SkipReason::FilePathLike
            }
        );
        assert_eq!(
            judge_replace("foo_bar_baz", "foo_bar_bat", &ctx()),
            Judgement::Skip {
                reason: SkipReason::CodeSymbol
            }
        );
        assert_eq!(
            judge_replace("abc123def456ghi", "abc123def456ghj", &ctx()),
            Judgement::Skip {
                reason: SkipReason::KeyOrHashLike
            }
        );
        assert_eq!(
            judge_replace("12:30", "12:45", &ctx()),
            Judgement::Skip {
                reason: SkipReason::NumberOrDate
            }
        );
        assert_eq!(
            judge_replace("price $50", "price $60", &ctx()),
            Judgement::Skip {
                reason: SkipReason::NumberOrDate
            }
        );
        assert_eq!(
            judge_replace("hello@x.com", "hi@x.com", &ctx()),
            Judgement::Skip {
                reason: SkipReason::EmailPartial
            }
        );
    }

    #[test]
    fn skip_everyday_names_acronyms() {
        assert_eq!(
            judge_replace("james", "jason", &ctx()),
            Judgement::Skip {
                reason: SkipReason::CommonFirstName
            }
        );
        assert_eq!(
            judge_replace("the", "a", &ctx()),
            Judgement::Skip {
                reason: SkipReason::EverydayWord
            }
        );
        assert_eq!(
            judge_replace("PDF", "PFD", &ctx()),
            Judgement::Skip {
                reason: SkipReason::EverydayAcronym
            }
        );
    }

    #[test]
    fn classify_end_to_end() {
        let pasted = "call pie torch today";
        let observed = "call PyTorch today";
        let js = classify(pasted, observed, &ctx());
        assert_eq!(
            js,
            vec![Judgement::Mishearing {
                term: "PyTorch".into(),
                alias: "pie torch".into()
            }]
        );
    }

    #[test]
    fn classify_insert_only_is_rewrite() {
        assert_eq!(
            classify("hi there", "hi there mate", &ctx()),
            vec![Judgement::Rewrite]
        );
    }

    #[test]
    fn classify_identical_is_empty() {
        assert!(classify("same", "same", &ctx()).is_empty());
    }

    #[test]
    fn never_panics_on_weird_input() {
        let ctx = ctx();
        let cases: Vec<(String, String)> = vec![
            ("".to_string(), "".to_string()),
            ("".to_string(), "x".to_string()),
            ("😀".to_string(), "😀😀".to_string()),
            ("a".repeat(64), "b".repeat(64)),
            (":::".to_string(), "{}(){}(){}(){}".to_string()),
        ];
        for (a, b) in &cases {
            let _ = judge_replace(a, b, &ctx);
            let _ = classify(a, b, &ctx);
        }
    }

    #[test]
    fn skip_list_positive_negative() {
        // Structural skips: any side structural → whole span skipped.
        let pos_structural: &[&str] = &[
            "https://example.com/x",
            "www.example.com",
            "example.com",
            "C:\\Users\\me",
            "src/lib.rs",
            "foo::bar",
            "foo->bar",
            "foo_bar_baz_qux",
            "abcdef0123456789",
            "0123456789ab",
            "2026-07-08",
            "14:30",
            "$1,200.00",
            "42",
            "a@b.co",
            "# heading",
            "- item",
            "* item",
        ];
        for w in pos_structural {
            assert!(
                structural_skip(w).is_some(),
                "expected structural skip for {w:?}"
            );
        }
        // Word-list skips (no structural interference).
        let pos_words: &[&str] = &[
            "PDF", "CEO", "ASAP", "FAQ", "URL", "API", "james", "mary", "john", "sarah", "the",
            "and", "but", "with",
        ];
        for w in pos_words {
            assert!(
                word_list_skip(w).is_some(),
                "expected word-list skip for {w:?}"
            );
        }
        // Full skip_reason: structural wins over word lists.
        assert_eq!(
            skip_reason("src/main.rs", "the"),
            Some(SkipReason::FilePathLike)
        );
        // Word-list skip when no structural side.
        assert_eq!(
            skip_reason("james", "jason"),
            Some(SkipReason::CommonFirstName)
        );
        let neg: &[&str] = &["PyTorch", "Siobhan", "Kubernetes", "Anthropic", "Jupyter"];
        for w in neg {
            assert!(skip_reason(w, w).is_none(), "unexpected skip for {w:?}");
            assert!(
                word_list_skip(w).is_none(),
                "unexpected word-list skip for {w:?}"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn prop_never_mishears_skipped_to(a in proptest::string::string_regex("[a-zA-Z0-9@./:_-]{1,20}").unwrap(),
                                          b in proptest::string::string_regex("[a-zA-Z0-9@./:_-]{1,20}").unwrap()) {
            let ctx = JudgeContext::default();
            let j = judge_replace(&a, &b, &ctx);
            if let Judgement::Mishearing { ref term, .. } = j {
                assert!(
                    skip_reason(&a, term).is_none(),
                    "Mishearing returned for skip-listed span {a:?} -> {b:?}"
                );
            }
        }
    }
}
