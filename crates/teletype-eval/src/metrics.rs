//! Plan §P0.4 accuracy metrics.
//!
//! All scoring code for the "beat Wispr Flow" eval lives here:
//!
//! - [`normalize_verbatim`]: a clean-room Rust re-implementation of the
//!   Open ASR Leaderboard Whisper English normalizer rules (that project is
//!   Apache-2.0; no code was copied — the *rules* were ported: lowercasing,
//!   bracket removal, filler removal, contraction expansion, symbol/diacritic
//!   stripping, spelled-out-number conversion, spelling/acronym
//!   normalization). Two deliberate deviations are documented on
//!   [`normalize_verbatim`].
//! - [`wer_verbatim`]: WER of raw ASR against the verbatim reference.
//! - [`wer_clean`]: WER of the pasted text against the clean reference.
//!   Fillers removed by polish do NOT count as errors here (the clean
//!   reference has no fillers, so removal is free; leftover fillers still
//!   count as insertions).
//! - [`vocab_score`]: vocabulary-term recall and false-alarm rate.
//! - [`entity_accuracy`]: exact match of numbers/dates/money after ITN.
//! - [`judge_semantic`]: local-judge semantic check (report-only, never a
//!   gate).
//!
//! Everything here is pure except [`judge_semantic`], which POSTs to a local
//! OpenAI-compatible endpoint configured by environment variables and
//! returns `Ok(None)` ("not run") when no endpoint is configured.

use crate::wer::{self, Wer};
use std::collections::{HashMap, HashSet};

// ── Verbatim normalizer ────────────────────────────────────────────────────

/// Normalize English ASR text the way the Open ASR Leaderboard scores it.
///
/// Rules ported (re-implemented, not copied) from the leaderboard's
/// `EnglishTextNormalizer`:
/// 1. lowercase;
/// 2. drop `[...]`/`<...>` bracketed and `(...)` parenthesized asides;
/// 3. drop filler/hesitation tokens (`uh`, `um`, `er`, `hmm`, elongations…);
/// 4. expand contractions (`can't` → `can not`, `gonna` → `going to`, titles
///    like `mr` → `mister`);
/// 5. drop commas between digits, drop periods not followed by a digit;
/// 6. replace symbols/punctuation with spaces (keeping `.%$¢€£` for the
///    number pass) and fold Latin diacritics to ASCII;
/// 7. convert spelled-out numbers to arabic (`one oh one` → `101`,
///    `eighty million dollars` → `$80000000`);
/// 8. fold a curated subset of British→American spellings;
/// 9. collapse spaced acronyms (`b b c` → `bbc`, `5 g` → `5g`);
/// 10. drop currency/percent symbols not adjacent to a digit; collapse
///     whitespace.
///
/// Deliberate deviations from the upstream normalizer:
/// - The upstream British→American spelling table (~1900 entries) and the
///   name-alias table are external data files; only a curated high-frequency
///   spelling subset is folded here (both sides go through the same mapping,
///   so an unmapped spelling still matches itself). See
///   [`AMERICAN_SPELLINGS`].
/// - The upstream multi-word compound table (`wi fi` → `wifi`, …) is
///   external data and is NOT applied: an un-joined compound counts as an
///   error rather than being forgiven. Conservative by design.
pub fn normalize_verbatim(text: &str) -> String {
    normalize_impl(text, true)
}

/// Like [`normalize_verbatim`] but keeps filler tokens.
///
/// Used by [`wer_clean`]: the clean reference contains no fillers, so a
/// filler the polish stage *removed* is free, while a filler it *left in*
/// still counts as an insertion error. All other rules (contractions,
/// numbers, spellings, acronyms) apply to both sides identically.
pub fn normalize_keep_fillers(text: &str) -> String {
    normalize_impl(text, false)
}

fn normalize_impl(text: &str, remove_fillers: bool) -> String {
    let mut s = text.to_lowercase();
    s = remove_bracketed(&s);
    if remove_fillers {
        s = remove_fillers_text(&s);
    }
    // Standardize whitespace before an apostrophe ("don 't" → "don't").
    static APOS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\s+'").expect("static regex"));
    s = APOS.replace_all(&s, "'").into_owned();

    for (pattern, replacement) in contraction_replacers() {
        s = pattern.replace_all(&s, *replacement).into_owned();
    }

    // Remove commas between digits ("1,000" → "1000").
    static COMMA: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(\d),(\d)").expect("static regex"));
    s = COMMA.replace_all(&s, "$1$2").into_owned();
    // Remove periods not followed by a number ("end. next" → "end  next").
    static PERIOD: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\.([^0-9]|$)").expect("static regex"));
    s = PERIOD.replace_all(&s, " $1").into_owned();

    s = strip_symbols(&s);

    s = standardize_numbers(&s);
    s = apply_spellings(&s);
    s = collapse_acronyms(&s);

    // Drop currency/percent symbols not adjacent to a number.
    static PRE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[.$¢€£]([^0-9])").expect("static regex"));
    s = PRE.replace_all(&s, " $1").into_owned();
    static POST: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"([^0-9])%").expect("static regex"));
    s = POST.replace_all(&s, "$1 ").into_owned();

    collapse_spaces(&s)
}

/// Remove `[...]`/`<...>` and `(...)` spans (non-greedy).
fn remove_bracketed(s: &str) -> String {
    static ANGLES: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[<\[][^>\]]*[>\]]").expect("static regex"));
    static PARENS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\(([^)]+?)\)").expect("static regex"));
    let s = ANGLES.replace_all(s, "");
    PARENS.replace_all(&s, "").into_owned()
}

/// Filler/hesitation patterns, ported from the upstream `filler_words` list:
/// arbitrary elongation is covered by the `+`/`*` quantifiers (`uh`, `uhh`,
/// `uuuh`, …). `ah-ha` stays first: its second half (`ha`) is not a filler
/// on its own, so the pair must win over the `a+h+m*` prefix.
fn filler_regex() -> regex::Regex {
    static CELL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\b(ah-ha|a+h+m*|a+h+a+|e+h+m*|e+m+|e+r+m*|h+a+h+|h+e+h+|h+m+|h+u+h+|m{2,}|m+h+m*|t+s+k+|u+g+h+|u+h+m*|u+h+u+[hm]*|u+m+h*|ahem|eheh|ehehe|ehr|hmmph|hum|hunh|mhum|mmkay)\b")
            .expect("static regex")
    });
    CELL.clone()
}

fn remove_fillers_text(s: &str) -> String {
    filler_regex().replace_all(s, "").into_owned()
}

/// Ordered contraction/title/general-affix expansions, ported from the
/// upstream `replacers` table. Order matters (specific before general:
/// `won't` before `n't`, perfect-tense forms before bare `'d`/`'s`).
fn contraction_replacers() -> &'static [(regex::Regex, &'static str)] {
    static TABLE: &[(&str, &str)] = &[
        (r"\bwon't\b", "will not"),
        (r"\bcan't\b", "can not"),
        (r"\blet's\b", "let us"),
        (r"\bain't\b", "aint"),
        (r"\by'all\b", "you all"),
        (r"\bwanna\b", "want to"),
        (r"\bgotta\b", "got to"),
        (r"\bgonna\b", "going to"),
        (r"\bi'ma\b", "i am going to"),
        (r"\bimma\b", "i am going to"),
        (r"\bwoulda\b", "would have"),
        (r"\bcoulda\b", "could have"),
        (r"\bshoulda\b", "should have"),
        (r"\bma'am\b", "madam"),
        (r"\bmr\b", "mister "),
        (r"\bmrs\b", "missus "),
        (r"\bst\b", "saint "),
        (r"\bdr\b", "doctor "),
        (r"\bprof\b", "professor "),
        (r"\bcapt\b", "captain "),
        (r"\bgov\b", "governor "),
        (r"\bald\b", "alderman "),
        (r"\bgen\b", "general "),
        (r"\bsen\b", "senator "),
        (r"\brep\b", "representative "),
        (r"\bpres\b", "president "),
        (r"\brev\b", "reverend "),
        (r"\bhon\b", "honorable "),
        (r"\basst\b", "assistant "),
        (r"\bassoc\b", "associate "),
        (r"\blt\b", "lieutenant "),
        (r"\bcol\b", "colonel "),
        (r"\bjr\b", "junior "),
        (r"\bsr\b", "senior "),
        (r"\besq\b", "esquire "),
        (r"'d been\b", " had been"),
        (r"'s been\b", " has been"),
        (r"'d gone\b", " had gone"),
        (r"'s gone\b", " has gone"),
        (r"'d done\b", " had done"),
        (r"'s got\b", " has got"),
        (r"n't\b", " not"),
        (r"'re\b", " are"),
        (
            r"\b(it|he|she|what|that|who|here|there|how|when|where|why|this)'s\b",
            "$1 is",
        ),
        (r"'d\b", " would"),
        (r"'ll\b", " will"),
        (r"'t\b", " not"),
        (r"'ve\b", " have"),
        (r"'m\b", " am"),
    ];
    static COMPILED: std::sync::LazyLock<Vec<(regex::Regex, &'static str)>> =
        std::sync::LazyLock::new(|| {
            TABLE
                .iter()
                .map(|(p, r)| (regex::Regex::new(p).expect("static regex"), *r))
                .collect()
        });
    &COMPILED
}

/// Replace symbols/punctuation with spaces (keeping `.%$¢€£` for the number
/// pass) and fold Latin diacritics to ASCII. Non-Latin scripts pass through
/// unchanged, matching the upstream NFKD+Mn-strip outcome for those scripts.
fn strip_symbols(s: &str) -> String {
    const KEEP: &[char] = &['.', '%', '$', '¢', '€', '£'];
    s.chars()
        .filter_map(|c| {
            if KEEP.contains(&c) {
                return Some(c);
            }
            if let Some(mapped) = fold_latin_diacritic(c) {
                return Some(mapped);
            }
            if c.is_alphanumeric() {
                return Some(c);
            }
            // Combining marks with no precomposed mapping are dropped;
            // everything else (punctuation, symbols, control) becomes a space.
            if ('\u{0300}'..='\u{036f}').contains(&c) {
                return None;
            }
            Some(' ')
        })
        .collect()
}

/// Fold precomposed Latin diacritics to their ASCII base letter.
///
/// Single-character folds only (`æ`→`a`, `ß`→`s` rather than upstream's
/// `ae`/`ss`): both sides pass through the same fold, so token identity —
/// all WER needs — is preserved.
fn fold_latin_diacritic(c: char) -> Option<char> {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'æ' | 'ǣ' => Some('a'),
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => Some('A'),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => Some('c'),
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => Some('C'),
        'ð' => Some('d'),
        'Ð' => Some('D'),
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Some('e'),
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => Some('E'),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => Some('i'),
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' => Some('I'),
        'ñ' | 'ń' | 'ņ' | 'ň' => Some('n'),
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => Some('N'),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'œ' => Some('o'),
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' => Some('O'),
        'ß' => Some('s'),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => Some('u'),
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => Some('U'),
        'ý' | 'ÿ' => Some('y'),
        'Ý' => Some('Y'),
        'ł' => Some('l'),
        'Ł' => Some('L'),
        'þ' => Some('t'),
        'Þ' => Some('T'),
        'š' | 'ś' | 'ŝ' | 'ş' => Some('s'),
        'Š' | 'Ś' | 'Ŝ' | 'Ş' => Some('S'),
        'ž' | 'ź' | 'ż' => Some('z'),
        'Ž' | 'Ź' | 'Ż' => Some('Z'),
        _ => None,
    }
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Spelled-out-number conversion ─────────────────────────────────────────
///
/// Rule port of the upstream `EnglishNumberNormalizer`: a single left-to-right
/// pass over word tokens with a pending `(prefix, value)` accumulator.
/// Integers are exact (`i128`); decimals ride as text and multiply exactly
/// through a numerator/denominator pair, so `one point five million` is
/// `7500000`, never `7499999.99…`.
/// Pending number value: an exact integer, or decimal/serial text that is
/// still accumulating (`"1."`, `"20"`, `"203"`).
#[derive(Clone, Debug)]
enum NumVal {
    Int(i128),
    Text(String),
}

impl NumVal {
    fn as_text(&self) -> String {
        match self {
            NumVal::Int(i) => i.to_string(),
            NumVal::Text(t) => t.clone(),
        }
    }
}

fn gcd(mut a: i128, mut d: i128) -> i128 {
    a = a.abs();
    d = d.abs();
    while d != 0 {
        let t = d;
        d = a % d;
        a = t;
    }
    a.max(1)
}

/// Parse a decimal literal into an exact `(numerator, denominator)` pair.
fn parse_decimal(token: &str) -> Option<(i128, i128)> {
    let (head, tail) = token.split_once('.')?;
    let scale = 10i128.checked_pow(tail.len() as u32)?;
    let head_n: i128 = if head.is_empty() || head == "-" || head == "+" {
        0
    } else {
        head.parse().ok()?
    };
    let tail_n: i128 = if tail.is_empty() {
        0
    } else {
        tail.parse().ok()?
    };
    let neg = head.starts_with('-');
    let n = head_n.abs() * scale + tail_n;
    let g = gcd(n, scale);
    Some((if neg { -n / g } else { n / g }, scale / g))
}

struct NumberTables {
    zeros: HashSet<&'static str>,
    ones: HashMap<&'static str, i128>,
    ones_suffixed: HashMap<&'static str, (i128, &'static str)>,
    tens: HashMap<&'static str, i128>,
    tens_suffixed: HashMap<&'static str, (i128, &'static str)>,
    multipliers: HashMap<&'static str, i128>,
    multipliers_suffixed: HashMap<&'static str, (i128, &'static str)>,
    preceding_prefixers: HashMap<&'static str, &'static str>,
    following_prefixers: HashMap<&'static str, &'static str>,
    suffixers: HashSet<&'static str>,
    specials: HashSet<&'static str>,
    decimals: HashSet<&'static str>,
    words: HashSet<&'static str>,
}

fn number_tables() -> &'static NumberTables {
    static CELL: std::sync::LazyLock<NumberTables> = std::sync::LazyLock::new(|| {
        let ones_list = [
            "one",
            "two",
            "three",
            "four",
            "five",
            "six",
            "seven",
            "eight",
            "nine",
            "ten",
            "eleven",
            "twelve",
            "thirteen",
            "fourteen",
            "fifteen",
            "sixteen",
            "seventeen",
            "eighteen",
            "nineteen",
        ];
        let ones: HashMap<&'static str, i128> = ones_list
            .iter()
            .enumerate()
            .map(|(i, w)| (*w, i as i128 + 1))
            .collect();
        let mut ones_suffixed: HashMap<&'static str, (i128, &'static str)> = HashMap::new();
        for (w, v) in &ones {
            let plural: &'static str = if *w == "six" {
                "sixes"
            } else {
                Box::leak(format!("{w}s").into_boxed_str())
            };
            ones_suffixed.insert(plural, (*v, "s"));
        }
        ones_suffixed.insert("zeroth", (0, "th"));
        ones_suffixed.insert("first", (1, "st"));
        ones_suffixed.insert("second", (2, "nd"));
        ones_suffixed.insert("third", (3, "rd"));
        ones_suffixed.insert("fifth", (5, "th"));
        ones_suffixed.insert("twelfth", (12, "th"));
        for (w, v) in &ones {
            if *v > 3 && *v != 5 && *v != 12 {
                let suffixed: &'static str = if w.ends_with('t') {
                    Box::leak(format!("{w}h").into_boxed_str())
                } else {
                    Box::leak(format!("{w}th").into_boxed_str())
                };
                ones_suffixed.insert(suffixed, (*v, "th"));
            }
        }
        let tens_list = [
            ("twenty", 20i128),
            ("thirty", 30),
            ("forty", 40),
            ("fifty", 50),
            ("sixty", 60),
            ("seventy", 70),
            ("eighty", 80),
            ("ninety", 90),
        ];
        let tens: HashMap<&'static str, i128> = tens_list.iter().map(|(w, v)| (*w, *v)).collect();
        let mut tens_suffixed = HashMap::new();
        for (w, v) in tens_list {
            tens_suffixed.insert(
                Box::leak(w.replace('y', "ies").into_boxed_str()) as &'static str,
                (v, "s"),
            );
            tens_suffixed.insert(
                Box::leak(w.replace('y', "ieth").into_boxed_str()) as &'static str,
                (v, "th"),
            );
        }
        let multipliers_list = [
            ("hundred", 100i128),
            ("thousand", 1_000),
            ("million", 1_000_000),
            ("billion", 1_000_000_000),
            ("trillion", 1_000_000_000_000),
            ("quadrillion", 1_000_000_000_000_000),
        ];
        let multipliers: HashMap<&'static str, i128> =
            multipliers_list.iter().map(|(w, v)| (*w, *v)).collect();
        let mut multipliers_suffixed = HashMap::new();
        for (w, v) in multipliers_list {
            multipliers_suffixed.insert(
                Box::leak(format!("{w}s").into_boxed_str()) as &'static str,
                (v, "s"),
            );
            multipliers_suffixed.insert(
                Box::leak(format!("{w}th").into_boxed_str()) as &'static str,
                (v, "th"),
            );
        }
        let zeros: HashSet<&'static str> = ["o", "oh", "zero"].into_iter().collect();
        let preceding_prefixers: HashMap<&'static str, &'static str> = [
            ("minus", "-"),
            ("negative", "-"),
            ("plus", "+"),
            ("positive", "+"),
        ]
        .into_iter()
        .collect();
        let following_prefixers: HashMap<&'static str, &'static str> = [
            ("pound", "£"),
            ("pounds", "£"),
            ("euro", "€"),
            ("euros", "€"),
            ("dollar", "$"),
            ("dollars", "$"),
            ("cent", "¢"),
            ("cents", "¢"),
        ]
        .into_iter()
        .collect();
        let suffixers: HashSet<&'static str> = ["per", "percent"].into_iter().collect();
        let specials: HashSet<&'static str> =
            ["and", "double", "triple", "point"].into_iter().collect();
        let mut decimals: HashSet<&'static str> =
            ones.keys().copied().chain(tens.keys().copied()).collect();
        decimals.extend(zeros.iter().copied());
        let mut words: HashSet<&'static str> = HashSet::new();
        words.extend(zeros.iter().copied());
        words.extend(ones.keys().copied());
        words.extend(ones_suffixed.keys().copied());
        words.extend(tens.keys().copied());
        words.extend(tens_suffixed.keys().copied());
        words.extend(multipliers.keys().copied());
        words.extend(multipliers_suffixed.keys().copied());
        words.extend(preceding_prefixers.keys().copied());
        words.extend(following_prefixers.keys().copied());
        words.extend(suffixers.iter().copied());
        words.extend(specials.iter().copied());
        NumberTables {
            zeros,
            ones,
            ones_suffixed,
            tens,
            tens_suffixed,
            multipliers,
            multipliers_suffixed,
            preceding_prefixers,
            following_prefixers,
            suffixers,
            specials,
            decimals,
            words,
        }
    });
    &CELL
}

fn is_arabic(token: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^\d+(\.\d+)?$").expect("static regex"));
    RE.is_match(token)
}

fn is_digit_token(t: &NumberTables, token: Option<&str>) -> bool {
    match token {
        None => false,
        Some(tok) => {
            is_arabic(tok)
                || t.zeros.contains(tok)
                || t.ones.contains_key(tok)
                || t.tens.contains_key(tok)
        }
    }
}

/// Preprocess pass, mirroring upstream: `<number> and a half` →
/// `<number> point five` (only after a numeric word), letter/digit boundary
/// spacing, ordinal-suffix joining (`32 nd` → `32nd`).
fn preprocess_numbers(s: &str) -> String {
    static HALF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\band\s+a\s+half\b").expect("static regex")
    });
    let t = number_tables();
    let segments: Vec<&str> = HALF.split(s).collect();
    let mut out: Vec<String> = Vec::new();
    for (i, segment) in segments.iter().copied().enumerate() {
        if segment.trim().is_empty() {
            continue;
        }
        out.push(segment.to_string());
        if i < segments.len() - 1 {
            let last_word = segment.split_whitespace().next_back().unwrap_or("");
            if t.decimals.contains(last_word) || t.multipliers.contains_key(last_word) {
                out.push("point five".to_string());
            } else {
                out.push("and a half".to_string());
            }
        }
    }
    let mut s = out.join(" ");
    static L2D: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"([a-z])([0-9])").expect("static regex"));
    static D2L: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"([0-9])([a-z])").expect("static regex"));
    static SUF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"([0-9])\s+(st|nd|rd|th|s)\b").expect("static regex")
    });
    s = L2D.replace_all(&s, "$1 $2").into_owned();
    s = D2L.replace_all(&s, "$1 $2").into_owned();
    s = SUF.replace_all(&s, "$1$2").into_owned();
    s
}

/// Postprocess pass: `$2 and ¢7` → `$2.07`, lone `$0.07` → `¢7`,
/// and `1`/`1s` → `one`/`ones` (readability, as upstream).
fn postprocess_numbers(s: &str) -> String {
    static CENTS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"([€£$])([0-9]+) (?:and )?¢([0-9]{1,2})\b").expect("static regex")
    });
    let s = CENTS
        .replace_all(s, |caps: &regex::Captures| {
            let (cur, int, cents) = (&caps[1], &caps[2], &caps[3]);
            match cents.parse::<u32>() {
                Ok(c) => format!("{cur}{int}.{c:02}"),
                Err(_) => caps[0].to_string(),
            }
        })
        .into_owned();
    static ZERO_CENTS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"[€£$]0\.([0-9]{1,2})\b").expect("static regex")
    });
    let s = ZERO_CENTS
        .replace_all(&s, |caps: &regex::Captures| match caps[1].parse::<u32>() {
            Ok(c) => format!("¢{c}"),
            Err(_) => caps[0].to_string(),
        })
        .into_owned();
    static ONES: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\b1(s?)\b").expect("static regex"));
    ONES.replace_all(&s, "one$1").into_owned()
}

fn standardize_numbers(s: &str) -> String {
    postprocess_numbers(&standardize_numbers_direct(
        &preprocess_numbers(s),
        number_tables(),
    ))
}

/// Flush pending text `s` with the current prefix (helper for the multiplier
/// arms, where the value was already taken into a local).
fn flush_text_with_prefix(s: String, prefix: &mut Option<String>, out: &mut Vec<String>) {
    let mut text = s;
    if let Some(p) = prefix.take() {
        text = format!("{p}{text}");
    }
    out.push(text);
}

/// Flush an already-taken value `v` with the current prefix.
fn flush_value_with_prefix(v: &NumVal, prefix: &mut Option<String>, out: &mut Vec<String>) {
    flush_text_with_prefix(v.as_text(), prefix, out);
}

/// Direct rule port of the upstream `process_words` state machine.
///
/// The branch order mirrors the upstream `if/elif` chain one-to-one. Emits
/// with a pending currency/sign `prefix` folded in, then clears the state.
#[allow(clippy::too_many_lines)]
fn standardize_numbers_direct(s: &str, t: &NumberTables) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut prefix: Option<String> = None;
    let mut value: Option<NumVal> = None;
    let mut skip = false;

    /// Flush one pending value with its prefix.
    macro_rules! emit {
        ($v:expr) => {{
            let mut text: String = $v;
            if let Some(p) = prefix.take() {
                text = format!("{p}{text}");
            }
            value = None;
            out.push(text);
        }};
    }
    macro_rules! flush {
        () => {{
            if let Some(v) = value.take() {
                flush_value_with_prefix(&v, &mut prefix, &mut out);
            }
        }};
    }

    for (i, current) in words.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        let prev = if i > 0 { Some(words[i - 1]) } else { None };
        let next = words.get(i + 1).copied();
        let next_is_numeric = next.is_some_and(is_arabic);

        // Currency/sign-prefixed arabic numbers ("$20", "-3.5").
        let (has_prefix, bare) = match current.chars().next() {
            Some(c) if matches!(c, '-' | '+' | '£' | '€' | '$' | '¢') && current.len() > 1 => {
                (true, &current[c.len_utf8()..])
            }
            _ => (false, *current),
        };
        if is_arabic(bare) {
            if let Some(v) = value.take() {
                match &v {
                    NumVal::Text(text) if text.ends_with('.') => {
                        // Decimal-point continuation ("1." + "5"); the
                        // pending prefix is untouched (as upstream).
                        value = Some(NumVal::Text(format!("{text}{current}")));
                        continue;
                    }
                    _ => {
                        let mut s = v.as_text();
                        if let Some(p) = prefix.take() {
                            s = format!("{p}{s}");
                        }
                        out.push(s);
                    }
                }
            }
            if has_prefix {
                prefix = Some(current.chars().next().unwrap().to_string());
            }
            if bare.contains('.') {
                // Non-integers ride as text (as upstream); the multiplier
                // arms parse them exactly when needed.
                value = Some(NumVal::Text(bare.to_string()));
            } else {
                match bare.parse::<i128>() {
                    Ok(n) => value = Some(NumVal::Int(n)),
                    Err(_) => value = Some(NumVal::Text(bare.to_string())),
                }
            }
            continue;
        }

        if !t.words.contains(*current) {
            flush!();
            out.push(current.to_string());
            continue;
        }

        if t.zeros.contains(*current) {
            if *current == "oh" {
                // "oh" is usually the interjection, not a zero: only a digit
                // inside a digit sequence ("four oh one", serial readings).
                let next2 = words.get(i + 2).copied();
                let in_number =
                    is_digit_token(t, next) && (value.is_some() || is_digit_token(t, next2));
                if !in_number {
                    flush!();
                    out.push("oh".to_string());
                    continue;
                }
            }
            let base = value.take().map(|v| v.as_text()).unwrap_or_default();
            value = Some(NumVal::Text(format!("{base}0")));
            continue;
        }

        if let Some(ones) = t.ones.get(*current).copied() {
            match value.take() {
                None => value = Some(NumVal::Int(ones)),
                Some(NumVal::Text(s)) => {
                    if prev.is_some_and(|p| t.tens.contains_key(p)) && ones < 10 {
                        let mut base = s;
                        base.pop();
                        value = Some(NumVal::Text(format!("{base}{ones}")));
                    } else {
                        value = Some(NumVal::Text(format!("{s}{ones}")));
                    }
                }
                Some(NumVal::Int(v)) => {
                    if prev.is_some_and(|p| t.ones.contains_key(p)) {
                        let s = v.to_string();
                        if prev.is_some_and(|p| t.tens.contains_key(p)) && ones < 10 {
                            let mut base = s;
                            base.pop();
                            value = Some(NumVal::Text(format!("{base}{ones}")));
                        } else {
                            value = Some(NumVal::Text(format!("{s}{ones}")));
                        }
                    } else if ones < 10 {
                        if v % 10 == 0 {
                            value = Some(NumVal::Int(v + ones));
                        } else {
                            value = Some(NumVal::Text(format!("{v}{ones}")));
                        }
                    } else if v % 100 == 0 {
                        value = Some(NumVal::Int(v + ones));
                    } else {
                        value = Some(NumVal::Text(format!("{v}{ones}")));
                    }
                }
            }
            continue;
        }

        if let Some((num, suffix)) = t.ones_suffixed.get(*current).copied() {
            // Ordinals/cardinals with suffixes yield immediately.
            match value.take() {
                None => emit!(format!("{num}{suffix}")),
                Some(NumVal::Text(s)) => {
                    if prev.is_some_and(|p| t.tens.contains_key(p)) && num < 10 {
                        let mut base = s;
                        base.pop();
                        emit!(format!("{base}{num}{suffix}"));
                    } else {
                        emit!(format!("{s}{num}{suffix}"));
                    }
                }
                Some(NumVal::Int(v)) => {
                    if prev.is_some_and(|p| t.ones.contains_key(p)) {
                        let s = v.to_string();
                        if prev.is_some_and(|p| t.tens.contains_key(p)) && num < 10 {
                            let mut base = s;
                            base.pop();
                            emit!(format!("{base}{num}{suffix}"));
                        } else {
                            emit!(format!("{s}{num}{suffix}"));
                        }
                    } else if num < 10 {
                        if v % 10 == 0 {
                            emit!(format!("{}{suffix}", v + num));
                        } else {
                            emit!(format!("{v}{num}{suffix}"));
                        }
                    } else if v % 100 == 0 {
                        emit!(format!("{}{suffix}", v + num));
                    } else {
                        emit!(format!("{v}{num}{suffix}"));
                    }
                }
            }
            continue;
        }

        if let Some(tens) = t.tens.get(*current).copied() {
            match value.take() {
                None => value = Some(NumVal::Int(tens)),
                Some(NumVal::Text(s)) => value = Some(NumVal::Text(format!("{s}{tens}"))),
                Some(NumVal::Int(v)) => {
                    if v % 100 == 0 {
                        value = Some(NumVal::Int(v + tens));
                    } else {
                        value = Some(NumVal::Text(format!("{v}{tens}")));
                    }
                }
            }
            continue;
        }

        if let Some((num, suffix)) = t.tens_suffixed.get(*current).copied() {
            match value.take() {
                None => emit!(format!("{num}{suffix}")),
                Some(NumVal::Text(s)) => emit!(format!("{s}{num}{suffix}")),
                Some(NumVal::Int(v)) => {
                    if v % 100 == 0 {
                        emit!(format!("{}{suffix}", v + num));
                    } else {
                        emit!(format!("{v}{num}{suffix}"));
                    }
                }
            }
            continue;
        }

        if let Some(mult) = t.multipliers.get(*current).copied() {
            match value.take() {
                None => value = Some(NumVal::Int(mult)),
                Some(NumVal::Text(s)) => {
                    // Decimal text (or a zero) times the multiplier, exactly.
                    let frac = s
                        .parse::<i128>()
                        .ok()
                        .map(|n| (n, 1))
                        .or_else(|| parse_decimal(&s));
                    match frac {
                        Some((n, d)) => {
                            let (pn, pd) = (n * mult, d);
                            let g = gcd(pn, pd);
                            if pd / g == 1 {
                                value = Some(NumVal::Int(pn / g));
                            } else {
                                flush_text_with_prefix(s, &mut prefix, &mut out);
                                value = Some(NumVal::Int(mult));
                            }
                        }
                        None => {
                            flush_text_with_prefix(s, &mut prefix, &mut out);
                            value = Some(NumVal::Int(mult));
                        }
                    }
                }
                Some(NumVal::Int(v)) => {
                    if v == 0 {
                        // "0 thousand" stays 0 (as upstream's fraction path).
                        value = Some(NumVal::Int(0));
                    } else {
                        let before = v / 1000 * 1000;
                        let residual = v % 1000;
                        value = Some(NumVal::Int(before + residual * mult));
                    }
                }
            }
            continue;
        }

        if let Some((mult, suffix)) = t.multipliers_suffixed.get(*current).copied() {
            match value.take() {
                None => emit!(format!("{mult}{suffix}")),
                Some(NumVal::Text(s)) => {
                    let frac = s
                        .parse::<i128>()
                        .ok()
                        .map(|n| (n, 1))
                        .or_else(|| parse_decimal(&s));
                    match frac {
                        Some((n, d)) => {
                            let (pn, pd) = (n * mult, d);
                            let g = gcd(pn, pd);
                            if pd / g == 1 {
                                emit!(format!("{}{suffix}", pn / g));
                            } else {
                                flush_text_with_prefix(s, &mut prefix, &mut out);
                                emit!(format!("{mult}{suffix}"));
                            }
                        }
                        None => {
                            flush_text_with_prefix(s, &mut prefix, &mut out);
                            emit!(format!("{mult}{suffix}"));
                        }
                    }
                }
                Some(NumVal::Int(v)) => {
                    let before = v / 1000 * 1000;
                    let residual = v % 1000;
                    emit!(format!("{}{suffix}", before + residual * mult));
                }
            }
            continue;
        }

        if let Some(sign) = t.preceding_prefixers.get(*current).copied() {
            // Sign words apply only directly before a number.
            flush!();
            if next.is_some_and(|n| t.words.contains(n)) || next_is_numeric {
                prefix = Some(sign.to_string());
            } else {
                out.push(current.to_string());
            }
            continue;
        }

        if let Some(sym) = t.following_prefixers.get(*current).copied() {
            // Currency words apply only directly after a number.
            if value.is_some() {
                prefix = Some(sym.to_string());
                flush!();
            } else {
                out.push(current.to_string());
            }
            continue;
        }

        if t.suffixers.contains(*current) {
            if let Some(v) = value.take() {
                if *current == "percent" {
                    let mut s = v.as_text();
                    if let Some(p) = prefix.take() {
                        s = format!("{p}{s}");
                    }
                    out.push(format!("{s}%"));
                } else if next == Some("cent") {
                    // "per cent" → "%".
                    let mut s = v.as_text();
                    if let Some(p) = prefix.take() {
                        s = format!("{p}{s}");
                    }
                    out.push(format!("{s}%"));
                    skip = true;
                } else {
                    flush_value_with_prefix(&v, &mut prefix, &mut out);
                    out.push(current.to_string());
                }
            } else {
                out.push(current.to_string());
            }
            continue;
        }

        if t.specials.contains(*current) {
            if !(next.is_some_and(|n| t.words.contains(n)) || next_is_numeric) {
                flush!();
                out.push(current.to_string());
            } else if *current == "and" {
                // "and" after hundreds/thousands is silent ("one hundred and
                // five" still accumulates); elsewhere it is a real word.
                if !prev.is_some_and(|p| t.multipliers.contains_key(p)) {
                    flush!();
                    out.push("and".to_string());
                }
            } else if *current == "double" || *current == "triple" {
                let digit = next.and_then(|n| {
                    if t.ones.contains_key(n) {
                        t.ones.get(n).copied()
                    } else if t.zeros.contains(n) {
                        Some(0)
                    } else {
                        None
                    }
                });
                match digit {
                    Some(d) => {
                        let repeats = if *current == "double" { 2 } else { 3 };
                        let base = value.take().map(|v| v.as_text()).unwrap_or_default();
                        value = Some(NumVal::Text(format!(
                            "{base}{}",
                            d.to_string().repeat(repeats)
                        )));
                        skip = true;
                    }
                    None => {
                        flush!();
                        out.push(current.to_string());
                    }
                }
            } else if *current == "point" {
                if next.is_some_and(|n| t.decimals.contains(n)) || next_is_numeric {
                    let base = value.take().map(|v| v.as_text()).unwrap_or_default();
                    value = Some(NumVal::Text(format!("{base}.")));
                } else {
                    flush!();
                    out.push("point".to_string());
                }
            }
            continue;
        }

        // All word-classes are covered above; reaching here means the tables
        // and the chain disagree — emit the token rather than dropping it.
        flush!();
        out.push(current.to_string());
    }

    flush!();
    postprocess_numbers(&out.join(" "))
}

// ── Spelling + acronym passes ─────────────────────────────────────────────

/// Curated high-frequency subset of the upstream British→American spelling
/// table (plus its `ok`→`okay`, `mhm`/`mm`/`mmm`→`hmm` rows, harmless here
/// because those tokens are already removed as fillers).
///
/// This is intentionally a subset: the full ~1900-entry table is external
/// data. Both sides pass through the same mapping, so an unmapped spelling
/// still matches itself on both sides.
pub const AMERICAN_SPELLINGS: &[(&str, &str)] = &[
    ("ok", "okay"),
    ("kay", "okay"),
    ("etcetera", "etc"),
    ("mhm", "hmm"),
    ("mm", "hmm"),
    ("mmm", "hmm"),
    ("colour", "color"),
    ("colours", "colors"),
    ("coloured", "colored"),
    ("colourful", "colorful"),
    ("favour", "favor"),
    ("favours", "favors"),
    ("favourite", "favorite"),
    ("favourites", "favorites"),
    ("behaviour", "behavior"),
    ("behaviours", "behaviors"),
    ("honour", "honor"),
    ("labour", "labor"),
    ("neighbour", "neighbor"),
    ("neighbours", "neighbors"),
    ("organise", "organize"),
    ("organised", "organized"),
    ("realise", "realize"),
    ("realised", "realized"),
    ("recognise", "recognize"),
    ("recognised", "recognized"),
    ("theatre", "theater"),
    ("centre", "center"),
    ("metre", "meter"),
    ("litre", "liter"),
    ("kilometre", "kilometer"),
    ("cheque", "check"),
    ("grey", "gray"),
    ("flavour", "flavor"),
    ("harbour", "harbor"),
    ("rumour", "rumor"),
    ("humour", "humor"),
    ("armour", "armor"),
    ("odour", "odor"),
    ("travelled", "traveled"),
    ("travelling", "traveling"),
    ("cancelled", "canceled"),
    ("cancelling", "canceling"),
    ("labelled", "labeled"),
    ("modelled", "modeled"),
    ("signalled", "signaled"),
    ("programme", "program"),
    ("judgement", "judgment"),
    ("aluminium", "aluminum"),
    ("aeroplane", "airplane"),
    ("aeroplanes", "airplanes"),
    ("kerb", "curb"),
    ("pyjamas", "pajamas"),
    ("optimise", "optimize"),
    ("minimise", "minimize"),
    ("maximise", "maximize"),
    ("normalise", "normalize"),
    ("summarise", "summarize"),
    ("utilise", "utilize"),
];

fn apply_spellings(s: &str) -> String {
    static MAP: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
        std::sync::LazyLock::new(|| AMERICAN_SPELLINGS.iter().copied().collect());
    s.split_whitespace()
        .map(|w| *MAP.get(w).unwrap_or(&w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collapse runs of single-character tokens into acronyms (`b b c` → `bbc`,
/// `5 g` → `5g`). A run containing the common words `a`/`i` needs 3+ tokens
/// (`a big cat` is untouched).
fn collapse_acronyms(s: &str) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut result: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i].len() == 1 && words[i].chars().all(|c| c.is_alphanumeric()) {
            let mut run = vec![words[i]];
            let mut j = i + 1;
            while j < words.len()
                && words[j].len() == 1
                && words[j].chars().all(|c| c.is_alphanumeric())
            {
                run.push(words[j]);
                j += 1;
            }
            let has_common = run.iter().any(|c| *c == "a" || *c == "i");
            let min_run = if has_common { 3 } else { 2 };
            if run.len() >= min_run {
                result.push(run.join(""));
            } else {
                result.extend(run.iter().map(|w| w.to_string()));
            }
            i = j;
        } else {
            result.push(words[i].to_string());
            i += 1;
        }
    }
    result.join(" ")
}

// ── WER entry points ───────────────────────────────────────────────────────

/// Verbatim WER: raw ASR against the verbatim (fillers-included) reference,
/// both through [`normalize_verbatim`].
pub fn wer_verbatim(raw_asr: &str, verbatim: &str) -> Wer {
    wer::word_error_rate(&normalize_verbatim(verbatim), &normalize_verbatim(raw_asr))
}

/// Clean WER: final pasted text against the clean (what-the-user-wants)
/// reference. Fillers removed by polish do not count as errors — the clean
/// reference has no fillers — while fillers left in still count as
/// insertions, because filler stripping is disabled on both sides here.
pub fn wer_clean(final_text: &str, clean: &str) -> Wer {
    wer::word_error_rate(
        &normalize_keep_fillers(clean),
        &normalize_keep_fillers(final_text),
    )
}

// ── Vocabulary recall ──────────────────────────────────────────────────────

/// Vocabulary-term score for one case.
///
/// Terms are matched case-insensitively as token subsequences after
/// [`normalize_verbatim`] (so `us-east-1` and `us east 1` are the same term).
/// Set semantics: a term counts once however often it occurs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VocabScore {
    /// Vocabulary terms present in the reference.
    pub terms_in_ref: usize,
    /// Of those, how many the hypothesis also produced.
    pub hits: usize,
    /// Vocabulary terms the hypothesis produced (in-ref or not).
    pub produced: usize,
    /// Produced terms absent from the reference.
    pub false_alarms: usize,
    /// `hits / terms_in_ref` (1.0 when the reference has no vocab terms).
    pub recall: f64,
    /// `false_alarms / produced` (0.0 when nothing was produced).
    pub false_alarm_rate: f64,
    pub missed: Vec<String>,
    pub false_alarm_terms: Vec<String>,
}

/// Score one reference/hypothesis pair against a vocabulary list.
pub fn vocab_score(reference: &str, hypothesis: &str, vocab: &[String]) -> VocabScore {
    let ref_tokens = tokenize_normalized(&normalize_verbatim(reference));
    let hyp_tokens = tokenize_normalized(&normalize_verbatim(hypothesis));
    let mut terms_in_ref = 0usize;
    let mut hits = 0usize;
    let mut produced = 0usize;
    let mut false_alarms = 0usize;
    let mut missed = Vec::new();
    let mut false_alarm_terms = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for term in vocab {
        let norm = normalize_verbatim(term);
        if norm.is_empty() || !seen.insert(norm.clone()) {
            continue;
        }
        let term_tokens = tokenize_normalized(&norm);
        if term_tokens.is_empty() {
            continue;
        }
        let in_ref = contains_subsequence(&ref_tokens, &term_tokens);
        let in_hyp = contains_subsequence(&hyp_tokens, &term_tokens);
        if in_ref {
            terms_in_ref += 1;
        }
        if in_hyp {
            produced += 1;
        }
        if in_ref && in_hyp {
            hits += 1;
        } else if in_ref {
            missed.push(term.clone());
        } else if in_hyp {
            false_alarms += 1;
            false_alarm_terms.push(term.clone());
        }
    }

    VocabScore {
        terms_in_ref,
        hits,
        produced,
        false_alarms,
        recall: if terms_in_ref == 0 {
            1.0
        } else {
            hits as f64 / terms_in_ref as f64
        },
        false_alarm_rate: if produced == 0 {
            0.0
        } else {
            false_alarms as f64 / produced as f64
        },
        missed,
        false_alarm_terms,
    }
}

fn tokenize_normalized(normalized: &str) -> Vec<String> {
    normalized.split_whitespace().map(str::to_string).collect()
}

fn contains_subsequence(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ── Entity accuracy ────────────────────────────────────────────────────────

/// Entity-accuracy score for one case: numbers, dates and money must match
/// exactly after ITN normalization on both sides.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityScore {
    pub ref_entities: usize,
    pub matched: usize,
    /// `matched / ref_entities` (1.0 when the reference has no entities).
    pub accuracy: f64,
    pub ref_list: Vec<String>,
    pub hyp_list: Vec<String>,
}

/// Extract entity tokens: any token containing an ASCII digit or a
/// currency/percent sign. Dates like `march 14th` carry digits after ITN
/// (`teletype_core::itn` renders ordinals numerically); fully spelled-out
/// dates with no digits are covered by `wer_verbatim`'s number pass instead.
fn entity_tokens(itn_text: &str) -> Vec<String> {
    itn_text
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| ".,;:!?\"'()[]".contains(c)))
        .filter(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit() || "$€£¢%".contains(c)))
        .map(str::to_string)
        .collect()
}

/// Score entities: multiset intersection of hypothesis entities against
/// reference entities (each reference entity can be matched once).
pub fn entity_accuracy(reference: &str, hypothesis: &str) -> EntityScore {
    let ref_norm = teletype_core::itn::normalize(reference);
    let hyp_norm = teletype_core::itn::normalize(hypothesis);
    let ref_list = entity_tokens(&ref_norm);
    let hyp_list = entity_tokens(&hyp_norm);
    let mut remaining: HashMap<&str, usize> = HashMap::new();
    for e in &hyp_list {
        *remaining.entry(e.as_str()).or_insert(0) += 1;
    }
    let mut matched = 0usize;
    for e in &ref_list {
        if let Some(count) = remaining.get_mut(e.as_str()) {
            if *count > 0 {
                *count -= 1;
                matched += 1;
            }
        }
    }
    EntityScore {
        accuracy: if ref_list.is_empty() {
            1.0
        } else {
            matched as f64 / ref_list.len() as f64
        },
        ref_entities: ref_list.len(),
        matched,
        ref_list,
        hyp_list,
    }
}

// ── Semantic judge (report-only) ───────────────────────────────────────────

/// Fixed rubric prompt for the local semantic judge. The judge must answer
/// with exactly `{"ok": true}` or `{"ok": false}`: meaning preserved and
/// nothing invented.
pub const SEMANTIC_RUBRIC: &str = "You are a strict transcription judge. \
    Compare CANDIDATE against REFERENCE. Answer with exactly {\"ok\": true} \
    if the candidate preserves the reference meaning with nothing invented, \
    reworded content included; answer {\"ok\": false} if any fact, number, \
    name, or instruction is changed, dropped, or added. Punctuation, \
    capitalisation, and filler-word removal never matter. Reply with the JSON \
    object only, no other text.";

/// Environment knobs for the local judge (Qwen3-4B served from a local
/// OpenAI-compatible endpoint, e.g. llama-server):
/// - `TELETYPE_EVAL_JUDGE_URL` — full `/v1/chat/completions` URL. Unset →
///   the judge does not run (`Ok(None)`).
/// - `TELETYPE_EVAL_JUDGE_MODEL` — model id (default `qwen3-4b`).
/// - `TELETYPE_EVAL_JUDGE_API_KEY` — bearer token (default `no-key`, fine
///   for local servers).
pub const JUDGE_URL_ENV: &str = "TELETYPE_EVAL_JUDGE_URL";
pub const JUDGE_MODEL_ENV: &str = "TELETYPE_EVAL_JUDGE_MODEL";
pub const JUDGE_API_KEY_ENV: &str = "TELETYPE_EVAL_JUDGE_API_KEY";
pub const JUDGE_DEFAULT_MODEL: &str = "qwen3-4b";

/// Build the judge prompt for one pair.
pub fn semantic_prompt(reference: &str, candidate: &str) -> String {
    format!("REFERENCE: {reference}\nCANDIDATE: {candidate}\n")
}

/// One semantic verdict.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticResult {
    pub ok: bool,
    pub raw_response: String,
}

/// Run the local semantic judge. Returns `Ok(None)` when
/// `TELETYPE_EVAL_JUDGE_URL` is unset (judge not run — the normal CI case).
/// Report-only: callers must never gate on this.
pub fn judge_semantic(reference: &str, candidate: &str) -> Result<Option<SemanticResult>, String> {
    let url = match std::env::var(JUDGE_URL_ENV) {
        Ok(u) if !u.trim().is_empty() => u,
        _ => return Ok(None),
    };
    let model = std::env::var(JUDGE_MODEL_ENV).unwrap_or_else(|_| JUDGE_DEFAULT_MODEL.into());
    let api_key = std::env::var(JUDGE_API_KEY_ENV).unwrap_or_else(|_| "no-key".into());
    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 32,
        "messages": [
            {"role": "system", "content": SEMANTIC_RUBRIC},
            {"role": "user", "content": semantic_prompt(reference, candidate)},
        ],
    })
    .to_string();
    let raw = post_chat_http(&url, &api_key, &body)?;
    let verdict = parse_semantic_verdict(&raw)?;
    Ok(Some(SemanticResult {
        ok: verdict,
        raw_response: raw,
    }))
}

/// Parse `{"ok": true|false}` out of a chat-completion body, tolerating a
/// surrounding code fence and extra whitespace.
fn parse_semantic_verdict(body: &str) -> Result<bool, String> {
    // First unwrap the OpenAI envelope if present.
    let content: String = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or(body)
            .to_string(),
        Err(_) => body.to_string(),
    };
    let content = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("bad verdict JSON {content:?}: {e}"))?;
    v["ok"]
        .as_bool()
        .ok_or_else(|| format!("verdict has no boolean `ok` field: {content:?}"))
}

/// Minimal blocking HTTP POST for `http://` endpoints only (local servers).
/// Mirrors the approach in `polish.rs`: no HTTP dependency in this crate.
fn post_chat_http(url: &str, api_key: &str, body: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("judge URL must be http:// (local server only)")?;
    let (host, path) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/".to_string()));
    let sock = std::net::TcpStream::connect(host).map_err(|e| format!("connect {host}: {e}"))?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {api_key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    use std::io::{Read, Write};
    let mut sock = sock;
    sock.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = Vec::new();
    sock.read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&raw).to_string();
    text.find("\r\n\r\n")
        .map(|i| text[i + 4..].to_string())
        .ok_or_else(|| "no HTTP response body".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalizer ──

    #[test]
    fn lowercase_and_punctuation() {
        assert_eq!(normalize_verbatim("Hello, World!"), "hello world");
    }

    #[test]
    fn brackets_removed() {
        assert_eq!(normalize_verbatim("hello [inaudible] world"), "hello world");
        assert_eq!(normalize_verbatim("hello (aside) world"), "hello world");
    }

    #[test]
    fn fillers_removed() {
        assert_eq!(
            normalize_verbatim("um the uh deploy er went out"),
            "the deploy went out"
        );
        assert_eq!(normalize_verbatim("uuuh hmmm well"), "well");
    }

    #[test]
    fn ah_ha_pair_removed() {
        // "ha" alone is not a filler; the pair must still vanish.
        assert_eq!(normalize_verbatim("ah-ha that is funny"), "that is funny");
    }

    #[test]
    fn contractions_expanded() {
        assert_eq!(normalize_verbatim("can't do it"), "can not do it");
        assert_eq!(normalize_verbatim("gonna ship it"), "going to ship it");
        assert_eq!(normalize_verbatim("he's here"), "he is here");
        assert_eq!(normalize_verbatim("i'm ready"), "i am ready");
    }

    #[test]
    fn titles_expanded() {
        assert_eq!(normalize_verbatim("mr smith"), "mister smith");
        assert_eq!(normalize_verbatim("dr jones"), "doctor jones");
    }

    #[test]
    fn digit_serials_and_years() {
        // Hand-traced through the state machine:
        // "one oh one": one→1, oh inside a digit sequence→"10",
        // one after text→"101".
        assert_eq!(normalize_verbatim("one oh one"), "101");
        // "twenty twenty six": tens(20), then tens again with 20 % 100 != 0
        // → text "2020"; six after text with prev in tens replaces the last
        // zero → "2026". Year readings come out right.
        assert_eq!(normalize_verbatim("twenty twenty six"), "2026");
    }

    #[test]
    fn hundreds_and_thousands() {
        assert_eq!(normalize_verbatim("one hundred and five"), "105");
        assert_eq!(
            normalize_verbatim("two thousand twenty four"),
            "2024",
            "thousand: before=0 residual=2 → 2000; twenty: 2000%100==0 → 2020; four: 2020%10==0 → 2024"
        );
    }

    #[test]
    fn digit_sequence_with_oh() {
        assert_eq!(normalize_verbatim("four oh one"), "401");
    }

    #[test]
    fn lone_oh_is_interjection() {
        assert_eq!(normalize_verbatim("oh wow"), "oh wow");
        assert_eq!(normalize_verbatim("oh oh"), "oh oh");
    }

    #[test]
    fn money_and_percent() {
        assert_eq!(normalize_verbatim("eighty million dollars"), "$80000000");
        assert_eq!(normalize_verbatim("forty two percent"), "42%");
        assert_eq!(
            normalize_verbatim("ninety nine point nine five percent"),
            "99.95%"
        );
    }

    #[test]
    fn ordinals() {
        assert_eq!(
            normalize_verbatim("the fourteenth of march"),
            "the 14th of march"
        );
    }

    #[test]
    fn acronyms_collapsed() {
        assert_eq!(normalize_verbatim("the b b c report"), "the bbc report");
        assert_eq!(normalize_verbatim("a big cat"), "a big cat");
    }

    #[test]
    fn spellings_folded() {
        assert_eq!(normalize_verbatim("favourite colour"), "favorite color");
    }

    #[test]
    fn one_readability_postprocess() {
        // A lone arabic 1 normalizes to "one" (as upstream).
        assert_eq!(normalize_verbatim("room 1"), "room one");
    }

    // ── wer_verbatim / wer_clean ──

    #[test]
    fn verbatim_wer_ignores_fillers_and_case() {
        // Hyp dropped the reference's fillers: the verbatim norm removes both.
        let w = wer_verbatim("Um ship it now uh", "ship it now");
        assert_eq!(w.wer, 0.0, "{w:?}");
    }

    #[test]
    fn verbatim_wer_counts_real_errors() {
        // "cat"→"dog": 1 substitution over 4 reference words.
        let w = wer_verbatim("the cat sat on", "the dog sat on");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (1, 0, 0));
        assert!((w.wer - 0.25).abs() < 1e-12);
    }

    #[test]
    fn verbatim_wer_number_equivalence() {
        // Spelled-out vs digits must not count after normalization.
        let w = wer_verbatim("we have three servers", "we have 3 servers");
        assert_eq!(w.wer, 0.0, "{w:?}");
    }

    #[test]
    fn clean_wer_forgives_removed_fillers() {
        // Polish removed the spoken "um"/"uh": the clean reference has no
        // fillers, so removal costs nothing.
        let w = wer_clean(
            "The deploy went out this morning.",
            "the deploy went out this morning",
        );
        assert_eq!(w.wer, 0.0, "{w:?}");
    }

    #[test]
    fn clean_wer_counts_leftover_fillers() {
        // Polish left "um" in: the clean reference has no "um", and filler
        // stripping is disabled for wer_clean, so it is an insertion.
        let w = wer_clean("Um ship it now.", "ship it now");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 1));
    }

    // ── vocab ──

    #[test]
    fn vocab_perfect_recall() {
        let s = vocab_score(
            "ask priya sharma about the kafka consumer",
            "ask priya sharma about the kafka consumer",
            &["Priya Sharma".into(), "Kafka".into(), "S3".into()],
        );
        assert_eq!((s.terms_in_ref, s.hits, s.false_alarms), (2, 2, 0));
        assert_eq!(s.recall, 1.0);
        assert_eq!(s.false_alarm_rate, 0.0);
    }

    #[test]
    fn vocab_miss_and_false_alarm() {
        let s = vocab_score(
            "the kafka consumer is down",
            "the s3 bucket is down",
            &["Kafka".into(), "S3".into()],
        );
        assert_eq!(s.terms_in_ref, 1);
        assert_eq!(s.hits, 0);
        assert_eq!(s.false_alarms, 1);
        assert_eq!(s.recall, 0.0);
        assert_eq!(s.false_alarm_rate, 1.0);
        assert_eq!(s.missed, vec!["Kafka".to_string()]);
        assert_eq!(s.false_alarm_terms, vec!["S3".to_string()]);
    }

    #[test]
    fn vocab_no_terms_in_ref_is_vacuous_recall() {
        let s = vocab_score("hello world", "hello world", &["Kafka".into()]);
        assert_eq!(s.recall, 1.0);
        assert_eq!(s.false_alarm_rate, 0.0);
    }

    // ── entities ──

    #[test]
    fn entity_exact_match() {
        let s = entity_accuracy("we have three servers", "we have 3 servers");
        assert_eq!(s.accuracy, 1.0, "{s:?}");
    }

    #[test]
    fn entity_mismatch_counts() {
        let s = entity_accuracy("pay eighty dollars", "pay eighteen dollars");
        assert!(s.ref_entities >= 1);
        assert!(s.accuracy < 1.0, "{s:?}");
    }

    #[test]
    fn entity_no_entities_is_vacuous() {
        let s = entity_accuracy("the cat sat", "the cat sat");
        assert_eq!(s.accuracy, 1.0);
    }

    // ── semantic ──

    #[test]
    fn semantic_prompt_shape() {
        let p = semantic_prompt("ship it", "Ship it.");
        assert!(p.contains("REFERENCE: ship it"));
        assert!(p.contains("CANDIDATE: Ship it."));
        assert!(SEMANTIC_RUBRIC.contains("{\"ok\": true}"));
    }

    #[test]
    fn semantic_not_run_without_endpoint() {
        // This test mutates process env; it is the only test touching this
        // variable, so no cross-test interference is possible.
        let saved = std::env::var(JUDGE_URL_ENV).ok();
        std::env::remove_var(JUDGE_URL_ENV);
        let r = judge_semantic("a", "b").unwrap();
        assert!(r.is_none());
        if let Some(v) = saved {
            std::env::set_var(JUDGE_URL_ENV, v);
        }
    }

    #[test]
    fn semantic_verdict_parsing() {
        let body = r#"{"id":"x","choices":[{"message":{"content":"{\"ok\": true}"}}]}"#;
        assert!(parse_semantic_verdict(body).unwrap());
        assert!(!parse_semantic_verdict("{\"ok\": false}").unwrap());
        assert!(parse_semantic_verdict("not json").is_err());
    }
}
