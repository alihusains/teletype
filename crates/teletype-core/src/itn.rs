//! Deterministic inverse text normalization (ITN): spoken-form to written-form.
//!
//! "twenty twenty six" -> "2026", "eighty million dollars" -> "$80 million",
//! "two zero three nine five four oh six" -> "203-954-0006".
//!
//! Pure Rust port of a reference `InverseTextNormalizer` engine. Phase 1
//! implements the high-value passes: cardinals, years, dates, times, phone
//! digit-runs, decimals, money/percent, keep-magnitude, ordinals, numeric
//! ranges, emails, and URLs. The pass order mirrors the reference engine.
//!
//! Design contract:
//! - Pure value transform, no state, no I/O, no model.
//! - English-only: the lexicon is English; the pipeline gates on `should_run`.
//! - Ambiguous minimal pairs are left spelled for the AI-polish layer.
//!
//! Regex note: the `regex` crate has no lookbehind. The two lookbehind
//! patterns in the reference are rewritten as leading-capture
//! `(?P<lead>^|[^\d.])` with the captured lead re-emitted.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

// ── Lexicon ─────────────────────────────────────────────────────────────────

fn units() -> &'static HashMap<&'static str, u64> {
    static UNITS: LazyLock<HashMap<&'static str, u64>> = LazyLock::new(|| {
        [
            ("zero", 0),
            ("oh", 0),
            ("o", 0),
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
        ]
        .into_iter()
        .collect()
    });
    &UNITS
}

fn tens() -> &'static HashMap<&'static str, u64> {
    static TENS: LazyLock<HashMap<&'static str, u64>> = LazyLock::new(|| {
        [
            ("twenty", 20),
            ("thirty", 30),
            ("forty", 40),
            ("fifty", 50),
            ("sixty", 60),
            ("seventy", 70),
            ("eighty", 80),
            ("ninety", 90),
        ]
        .into_iter()
        .collect()
    });
    &TENS
}

fn scales() -> &'static HashMap<&'static str, u64> {
    static SCALES: LazyLock<HashMap<&'static str, u64>> = LazyLock::new(|| {
        [
            ("hundred", 100),
            ("thousand", 1_000),
            ("million", 1_000_000),
            ("billion", 1_000_000_000),
        ]
        .into_iter()
        .collect()
    });
    &SCALES
}

fn ordinal_word() -> &'static HashMap<&'static str, u64> {
    static ORD: LazyLock<HashMap<&'static str, u64>> = LazyLock::new(|| {
        [
            ("first", 1),
            ("second", 2),
            ("third", 3),
            ("fourth", 4),
            ("fifth", 5),
            ("sixth", 6),
            ("seventh", 7),
            ("eighth", 8),
            ("ninth", 9),
            ("tenth", 10),
            ("eleventh", 11),
            ("twelfth", 12),
            ("thirteenth", 13),
            ("fourteenth", 14),
            ("fifteenth", 15),
            ("sixteenth", 16),
            ("seventeenth", 17),
            ("eighteenth", 18),
            ("nineteenth", 19),
            ("twentieth", 20),
            ("thirtieth", 30),
            ("twenty first", 21),
            ("twenty second", 22),
            ("twenty third", 23),
            ("twenty fourth", 24),
            ("twenty fifth", 25),
            ("twenty sixth", 26),
            ("twenty seventh", 27),
            ("twenty eighth", 28),
            ("twenty ninth", 29),
        ]
        .into_iter()
        .collect()
    });
    &ORD
}

fn months() -> &'static HashMap<&'static str, u64> {
    static MONTHS: LazyLock<HashMap<&'static str, u64>> = LazyLock::new(|| {
        [
            ("january", 1),
            ("february", 2),
            ("march", 3),
            ("april", 4),
            ("may", 5),
            ("june", 6),
            ("july", 7),
            ("august", 8),
            ("september", 9),
            ("october", 10),
            ("november", 11),
            ("december", 12),
        ]
        .into_iter()
        .collect()
    });
    &MONTHS
}

const MONTH_NAMES: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const AP_THRESHOLD: u64 = 10;

fn unit_nouns() -> &'static HashMap<&'static str, ()> {
    static UNIT_NOUNS: LazyLock<HashMap<&'static str, ()>> = LazyLock::new(|| {
        [
            "miles",
            "mile",
            "feet",
            "foot",
            "inch",
            "inches",
            "yard",
            "yards",
            "pound",
            "pounds",
            "lb",
            "lbs",
            "ounce",
            "ounces",
            "oz",
            "kg",
            "kilogram",
            "kilograms",
            "gram",
            "grams",
            "km",
            "kilometer",
            "kilometers",
            "meter",
            "meters",
            "metre",
            "metres",
            "cm",
            "centimeter",
            "centimeters",
            "liter",
            "liters",
            "litre",
            "litres",
            "gallon",
            "gallons",
            "cup",
            "cups",
            "tablespoon",
            "tablespoons",
            "tbsp",
            "teaspoon",
            "teaspoons",
            "tsp",
            "degree",
            "degrees",
            "mph",
            "percent",
            "milligram",
            "milligrams",
            "mg",
            "milliliter",
            "milliliters",
            "ml",
            "millimeter",
            "millimeters",
            "mm",
        ]
        .into_iter()
        .map(|w| (w, ()))
        .collect()
    });
    &UNIT_NOUNS
}

// ── Regex cache ─────────────────────────────────────────────────────────────

fn re(pattern: &str) -> Regex {
    // Compile each call; the regex crate caches internally and our patterns
    // are short. This avoids the 'static lifetime issue with a Mutex cache.
    Regex::new(pattern).expect("ITN regex failed to compile")
}

// ── Entry point ─────────────────────────────────────────────────────────────

/// Gate: run ITN only for English (en / en-* / en_*) or empty/legacy language.
pub fn should_run(language: &str) -> bool {
    let lower = language.to_lowercase();
    lower.is_empty() || lower == "en" || lower.starts_with("en-") || lower.starts_with("en_")
}

/// Deterministic ITN: spoken-form to written-form. Pure, no state.
pub fn normalize(text: &str) -> String {
    let mut t = format!(" {} ", text.trim());
    let mut protected: Vec<String> = Vec::new();

    t = protect_idioms(&t, &mut protected);
    t = join_hyphenated_numbers(&t);
    t = emails(&t);
    t = urls(&t);
    t = decimals(&t);
    t = money_pct(&t);
    t = times(&t);
    t = dates(&t);
    t = ordinals(&t);
    t = years(&t);
    t = money_pct(&t);
    t = phone_digit_runs(&t);
    t = mixed_state(&t);
    t = cardinals(&t);
    t = numeric_ranges(&t);
    t = keep_magnitude(&t);

    t = restore_protected(&t, &protected);
    t = tighten(&t);
    t
}

// ── Support helpers ─────────────────────────────────────────────────────────

/// The captured `(?P<lead>^|\s)` prefix of a pattern, so a replacement can
/// put back the boundary character it matched on.
///
/// Every substitution in this module that consumed a leading `\s` without
/// re-emitting it welded the substituted text onto the previous word, so
/// "fifty dollars" became "cost$50". Patterns that start with a boundary
/// group use this; see `numeric_ranges` for the original instance.
fn lead_of<'t>(m: &regex::Captures<'t>) -> &'t str {
    m.name("lead").map(|l| l.as_str()).unwrap_or("")
}

fn re_sub(
    t: &str,
    pattern: &str,
    mut repl: impl FnMut(&regex::Captures) -> Option<String>,
) -> String {
    let rx = re(pattern);
    let mut out = String::with_capacity(t.len());
    let mut last = 0;
    for caps in rx.captures_iter(t) {
        let m = caps.get(0).unwrap();
        out.push_str(&t[last..m.start()]);
        match repl(&caps) {
            Some(r) => out.push_str(&r),
            None => out.push_str(m.as_str()),
        }
        last = m.end();
    }
    out.push_str(&t[last..]);
    out
}

fn split_words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

fn comma(n: u64) -> String {
    let digits = n.to_string();
    let chars: Vec<char> = digits.chars().rev().collect();
    let mut grouped = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i != 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    grouped.chars().rev().collect()
}

fn pad2(n: u64) -> String {
    format!("{:02}", n)
}

fn ord_suffix(n: u64) -> &'static str {
    let mod100 = n % 100;
    if (11..=13).contains(&mod100) {
        return "th";
    }
    match n % 10 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

fn fmt_phone(d: &str) -> String {
    let c: Vec<char> = d.chars().collect();
    if c.len() == 10 {
        format!("{}-{}-{}", &d[0..3], &d[3..6], &d[6..10])
    } else if c.len() == 7 {
        format!("{}-{}", &d[0..3], &d[3..7])
    } else {
        d.to_string()
    }
}

fn token_core(tok: &str) -> String {
    let trimmed = tok.trim_matches(|c: char| !c.is_ascii_alphabetic());
    if trimmed.chars().all(|c| c.is_ascii_alphabetic()) && !trimmed.is_empty() {
        trimmed.to_lowercase()
    } else {
        String::new()
    }
}

// ── Strict cardinal parser ──────────────────────────────────────────────────

/// Parses a numeric phrase that may be words ("ninety nine"), plain digits
/// ("99"), or a decimal another pass already wrote ("99.9").
///
/// The passes run in sequence and hand each other their output, so a pass
/// that only understood spoken words would silently skip anything an earlier
/// pass had already converted. That is why "ninety nine point nine percent"
/// reached `money_pct` as "99.9 percent" and was left alone: `words_to_int`
/// rejects a token containing a decimal point.
fn parse_numeric(raw: &str) -> Option<String> {
    let words: Vec<&str> = split_words(raw);
    if words.is_empty() {
        return None;
    }
    // Every token is numeric: pass it through, keeping commas and any decimal.
    if words.iter().all(|w| {
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
    }) {
        let joined = words.concat();
        // Guard against something that is only separators.
        if joined.chars().any(|c| c.is_ascii_digit()) {
            return Some(joined);
        }
        return None;
    }
    // "zero" is deliberately not a cardinal (see `words_to_int`), but it is a
    // valid whole-number part of a decimal: "zero point five" is 0.5.
    if words.len() == 1 && words[0].eq_ignore_ascii_case("zero") {
        return Some("0".into());
    }
    words_to_int(&words).map(|n| n.to_string())
}

fn words_to_int(words: &[&str]) -> Option<u64> {
    let units = units();
    let tens = tens();
    let scales = scales();

    let mut total: u64 = 0;
    let mut current: u64 = 0;
    let mut saw_word = false;

    for &w in words {
        let wl = w.to_lowercase();
        if wl == "and" {
            continue;
        }
        if let Some(&v) = units.get(wl.as_str()) {
            // 'o'/'oh'/'zero' are not cardinal digits.
            if v == 0 {
                return None;
            }
            // A unit (1-19) must be the first number word in its group.
            // "twenty five" is fine (tens then unit); "five twenty" is not.
            // "one hundred five" is fine (unit, scale, unit).
            if current >= 20 {
                // current is 20-99 (tens already seen): unit must be < 20
                // and current must be a tens value (20,30,...90).
                if !current.is_multiple_of(10) {
                    return None; // e.g. "twenty five three"
                }
                if v >= 20 {
                    return None;
                }
                current += v;
            } else if current > 0 && current < 20 {
                // Two units in a row: "one two" -> not a cardinal.
                return None;
            } else {
                current += v;
            }
            saw_word = true;
        } else if let Some(&v) = tens.get(wl.as_str()) {
            // A tens word: current must be 0 (new group) or >= 100
            // (after "hundred", e.g. "three hundred twenty").
            // "five twenty" (unit before tens) and "twenty thirty"
            // (two tens) are both invalid.
            if current > 0 && current < 100 {
                return None;
            }
            current += v;
            saw_word = true;
        } else if wl == "hundred" {
            // "hundred" multiplies current by 100 (or 100 if current==0).
            // Does NOT flush to total - this lets "six hundred ninety six
            // thousand" be parsed as 696 * 1000, not 600 + 96 * 1000.
            let base = if current == 0 { 1 } else { current };
            current = base.checked_mul(100)?;
            saw_word = true;
        } else if let Some(&v) = scales.get(wl.as_str()) {
            if current == 0 {
                current = 1; // "hundred" alone = 100
            }
            current = current.checked_mul(v)?;
            total = total.checked_add(current)?;
            current = 0;
            saw_word = true;
        } else {
            return None;
        }
    }

    if !saw_word {
        return None;
    }
    Some(total + current)
}

fn parse_year(words: &[&str]) -> Option<u32> {
    if words.len() >= 2 {
        let first = words[0].to_lowercase();
        let century_map: &[(&str, u32)] = &[
            ("fifteen", 15),
            ("sixteen", 16),
            ("seventeen", 17),
            ("eighteen", 18),
            ("nineteen", 19),
            ("twenty", 20),
        ];
        if let Some(&(_w, century_val)) = century_map.iter().find(|(w, _)| **w == first) {
            let rest: Vec<&str> = words[1..].to_vec();
            if let Some(y) = words_to_int(&rest) {
                if y < 100 {
                    return Some(century_val * 100 + y as u32);
                }
            }
        }
    }
    if words.len() >= 2 {
        if let Some(n) = words_to_int(words) {
            if (1000..=9999).contains(&n) {
                return Some(n as u32);
            }
        }
    }
    None
}

// ── Passes ──────────────────────────────────────────────────────────────────

fn protect_idioms(t: &str, protected: &mut Vec<String>) -> String {
    let t = re_sub(t, r"\b(?:a|an)\s+(hundred\b)", |m| {
        // Reject "a hundred-year" (hyphenated compound keeps its article).
        let end = m.get(0).unwrap().end();
        if t.as_bytes().get(end) == Some(&b'-') {
            return None;
        }
        Some(format!(" {}", m.get(1).map(|m| m.as_str()).unwrap_or("")))
    });
    let idiom_pat = r"\bthe whole nine yards\b|\bthe eleventh hour\b|\bthe seventh heaven\b|\bthe fourth wall\b|\bthe fifth wheel\b|\bthe fifth column\b|\bthe fourth estate\b|\bfirst among equals\b";
    re_sub(&t, idiom_pat, |m| {
        let whole = m.get(0).unwrap().as_str().to_string();
        protected.push(whole.clone());
        Some(format!(" \u{0}{}\u{0} ", protected.len() - 1))
    })
}

fn join_hyphenated_numbers(t: &str) -> String {
    // "twenty-two" -> "twenty two". Match hyphen between two number words
    // and replace with a space. No lookahead: match the full pair.
    let nw = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion)";
    let pat = format!(r"\b({nw})-({nw})\b");
    let mut t = t.to_string();
    let mut prev = String::new();
    while prev != t {
        prev = t.clone();
        t = re_sub(&t, &pat, |m| {
            Some(format!("{} {}", m.get(1)?.as_str(), m.get(2)?.as_str()))
        });
    }
    t
}

fn emails(t: &str) -> String {
    let pat = r"\b([a-z0-9]+)\s+at\s+([a-z0-9]+(?:\s+dot\s+[a-z0-9]+)+)\b";
    re_sub(t, pat, |m| {
        let local = m.get(1)?.as_str();
        let domain_raw = m.get(2)?.as_str();
        let domain: String = domain_raw
            .split_whitespace()
            .filter(|w| *w != "dot")
            .collect::<Vec<_>>()
            .join(".");
        Some(format!("{local}@{domain}"))
    })
}

/// Words that are never the first label of a real host. Without this the
/// pass turned ordinary prose into domains: "i said dot com out loud" became
/// "i said.com out loud", because `said` looks exactly like a domain label.
const NOT_A_HOST_LABEL: &[&str] = &[
    "a", "an", "i", "the", "is", "it", "its", "that", "this", "these", "those", "and", "but", "or",
    "if", "so", "we", "you", "he", "she", "they", "them", "my", "your", "our", "his", "her", "was",
    "were", "are", "do", "does", "did", "have", "has", "had", "will", "would", "can", "could",
    "should", "may", "might", "must", "there", "here", "what", "when", "where", "which", "who",
    "whom", "how", "why", "not", "no", "yes", "ok", "okay", "just", "very", "really", "quite",
    "also", "even", "still", "yet", "now", "then", "than", "with", "without", "into", "onto",
    "from", "about", "over", "under", "again", "once", "twice", "said", "say", "says", "like",
    "get", "got", "make", "made", "take", "took", "use", "used", "go", "goes", "went", "come",
    "came", "see", "saw", "know", "knew", "think", "thought", "want", "need", "let", "put",
];

fn urls(t: &str) -> String {
    // Path segments are separated by an explicit "slash", so a multi-segment
    // path keeps its slashes instead of collapsing into one run of letters.
    let pat = r"\b([a-z0-9]+(?:\s+dot\s+[a-z0-9]+)+)(?:\s+slash\s+((?:[a-z0-9\-]+)(?:\s+slash\s+[a-z0-9\-]+)*))?\b";
    re_sub(t, pat, |m| {
        let host_raw = m.get(1)?.as_str();
        let labels: Vec<&str> = host_raw
            .split_whitespace()
            .filter(|w| *w != "dot")
            .collect();
        // A real host's first label is at least 2 characters and is not an
        // English word. "a dot com" and "said dot com" are prose, not hosts.
        let first = labels.first()?.to_lowercase();
        if first.len() < 2 || NOT_A_HOST_LABEL.contains(&first.as_str()) {
            return None;
        }
        // The last label is a TLD: 2-6 letters.
        let last = labels.last()?.to_lowercase();
        if !(2..=6).contains(&last.len()) || !last.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let host = labels.join(".");
        let path: Vec<String> = m
            .get(2)
            .map(|p| {
                p.as_str()
                    .split_whitespace()
                    .filter(|w| *w != "slash")
                    .map(|w| w.to_string())
                    .collect()
            })
            .unwrap_or_default();
        if path.is_empty() {
            Some(host)
        } else {
            Some(format!("{host}/{}", path.join("/")))
        }
    })
}

fn decimals(t: &str) -> String {
    // `\b` on every alternative, including the bare `o`, so the pass cannot
    // eat the start of the following word (`out` is not `o` + `ut`).
    let digit_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine)\b";
    let numtok = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion|\d[\d,]*)";
    // `lead` is captured and re-emitted so the space in front of the number
    // survives; every pass in this module that used to start with `\s`
    // silently welded the number onto the previous word.
    let pat = format!(
        r"(?P<lead>^|\s)(?<w>(?:{numtok})(?:\s+(?:{numtok}))*)\s+(?:point|dot)\s+(?<d>(?:{digit_alt})(?:\s+(?:{digit_alt}))*)"
    );
    re_sub(t, &pat, |m| {
        let whole_raw = m.name("w")?.as_str().to_lowercase();
        let whole = parse_numeric(&whole_raw)?;
        let digs: String = split_words(m.name("d")?.as_str())
            .iter()
            .filter_map(|w| units().get(*w).map(|v| v.to_string()))
            .collect();
        Some(format!("{}{whole}.{digs}", lead_of(m)))
    })
}

fn money_pct(t: &str) -> String {
    let numtok = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion|\d[\d,]*)";
    let cur_pat = format!(r"(?P<lead>^|\s)((?<d>(?:{numtok})(?:\s+(?:{numtok}))*)\s+dollars?)");
    let t = re_sub(t, &cur_pat, |m| {
        let d_raw = m.name("d")?.as_str().to_lowercase();
        let n = parse_numeric(&d_raw)?;
        // A decimal has no meaning for a price, so leave the input alone
        // rather than printing "$50.5" for "fifty point five dollars".
        if n.contains('.') {
            return None;
        }
        let n: u64 = n.replace(',', "").parse().ok()?;
        Some(format!("{}${}", lead_of(m), comma(n)))
    });
    // `decimals` runs before this pass and leaves a written-out decimal, so the
    // numeric token has to be able to span a decimal point. Without this,
    // "ninety nine point nine percent" became "99.9 percent".
    let dec_numtok = format!(r"(?:{numtok}(?:\.\d+)?)");
    let pct_pat = format!(
        r"(?P<lead>^|\s)((?:{dec_numtok})(?:\s+(?:{dec_numtok}))*)\s+(?:percent|per\s+cent)"
    );
    re_sub(&t, &pct_pat, |m| {
        let raw = m.get(2)?.as_str().to_lowercase();
        let n = parse_numeric(&raw)?;
        Some(format!("{}{}%", lead_of(m), n.trim_end_matches(".0")))
    })
}

fn times(t: &str) -> String {
    let units_tens_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety)";
    let numtok = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|\d{1,2})";
    let time_pat = format!(
        r"(?P<lead>^|\s)(?<h>{units_tens_alt}|\d{{1,2}})(?:\s+(?<m>(?:{numtok})(?:\s+(?:{numtok}))*))?\s+(?<ap>[ap]\s*m)\b"
    );
    let t = re_sub(t, &time_pat, |m| {
        let h_raw = m.name("h")?.as_str();
        let h = words_to_int(&split_words(h_raw))?;
        if h > 12 {
            return None;
        }
        let mins = if let Some(mraw) = m.name("m") {
            let mw: Vec<&str> = split_words(mraw.as_str());
            if let Some(parsed) = words_to_int(&mw) {
                if parsed >= 60 {
                    return None;
                }
                parsed
            } else if mw
                .iter()
                .all(|w| units().get(*w).map(|v| *v < 10).unwrap_or(false))
            {
                let digs: String = mw
                    .iter()
                    .filter_map(|w| units().get(*w).map(|v| v.to_string()))
                    .collect();
                digs.parse().unwrap_or(0)
            } else {
                return None;
            }
        } else {
            0
        };
        let ap = m.name("ap")?.as_str().replace(' ', "").to_uppercase();
        Some(format!("{}{h}:{} {}", lead_of(m), pad2(mins), ap))
    });
    let oclock_pat = format!(r"(?P<lead>^|\s)({units_tens_alt})\s+o'?clock\b");
    re_sub(&t, &oclock_pat, |m| {
        let n = words_to_int(&[m.get(2)?.as_str()])?;
        if n > 12 {
            return None;
        }
        Some(format!("{}{n}:00", lead_of(m)))
    })
}

fn dates(t: &str) -> String {
    let months_alt = r"(?:january|february|march|april|may|june|july|august|september|october|november|december)";
    let ord_alt = r"(?:first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|eleventh|twelfth|thirteenth|fourteenth|fifteenth|sixteenth|seventeenth|eighteenth|nineteenth|twentieth|thirtieth|twenty first|twenty second|twenty third|twenty fourth|twenty fifth|twenty sixth|twenty seventh|twenty eighth|twenty ninth)";
    let numword_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion)";
    let pat = format!(
        r#"(?P<lead>^|\s)(?<mon>{months_alt})\s+(?<day>{ord_alt}|\d{{1,2}}),?\s+(?<yr>(?:{numword_alt})(?:\s+(?:{numword_alt})){{1,3}})"#
    );
    re_sub(t, &pat, |m| {
        let mon_raw = m.name("mon")?.as_str().to_lowercase();
        let mon = months().get(mon_raw.as_str()).copied()?;
        let day_raw = m.name("day")?.as_str().trim();
        let day: u64 = if let Ok(d) = day_raw.parse::<u64>() {
            d
        } else {
            let dl = day_raw.to_lowercase();
            ordinal_word().get(dl.as_str()).copied()?
        };
        if !(1..=31).contains(&day) {
            return None;
        }
        let yr_words: Vec<&str> = split_words(m.name("yr")?.as_str());
        let yr = parse_year(&yr_words)?;
        Some(format!(
            "{}{} {}, {}",
            lead_of(m),
            MONTH_NAMES[mon as usize],
            day,
            yr
        ))
    })
}

fn ordinals(t: &str) -> String {
    let ord_alt = r"(?:first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|eleventh|twelfth|thirteenth|fourteenth|fifteenth|sixteenth|seventeenth|eighteenth|nineteenth|twentieth|thirtieth|fortieth|fiftieth|sixtieth|seventieth|eightieth|ninetieth|twenty first|twenty second|twenty third|twenty fourth|twenty fifth|twenty sixth|twenty seventh|twenty eighth|twenty ninth)";
    let pat = format!(r"\b({ord_alt})\b");
    re_sub(t, &pat, |m| {
        let w = m.get(1)?.as_str().to_lowercase();
        // Skip if preceded by a tens word (e.g. "seventy second" -> the
        // cardinals pass will handle "seventy second" as 72 and add the
        // ordinal suffix). Without this check, "second" gets converted to
        // "2nd" before cardinals can join the compound.
        let start = m.get(0).unwrap().start();
        let before = &t[..start];
        let before_words: Vec<&str> = before.split_whitespace().collect();
        if let Some(prev) = before_words.last() {
            let prev_l = prev.to_lowercase();
            if tens().contains_key(prev_l.as_str()) {
                return None;
            }
            // BUG-004: "second" is a count noun ("a second of silence",
            // "the second meeting", "my second child") far more often than
            // a positional ordinal. The only unambiguous positional readings
            // are "the second tuesday" (weekday after) and "the second of
            // july" (date day). Every other determiner-plus-"second" phrase
            // is a noun and is left alone.
            let end = m.get(0).unwrap().end();
            let next = split_words(&t[end..])
                .first()
                .map(|n| n.to_lowercase())
                .unwrap_or_default();
            let is_weekday = matches!(
                next.as_str(),
                "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
            );
            let is_date_day = next == "of";
            if w == "second" {
                // A determiner before "second" is a quantifier, not a
                // position marker: "a second", "another second", "the
                // second meeting" are all noun phrases.
                if matches!(
                    prev_l.as_str(),
                    "a" | "an"
                        | "the"
                        | "another"
                        | "my"
                        | "your"
                        | "his"
                        | "her"
                        | "its"
                        | "our"
                        | "their"
                ) && !is_weekday
                    && !(is_date_day && prev_l == "the")
                {
                    return None;
                }
            } else if matches!(prev_l.as_str(), "a" | "an" | "another")
                && (next.is_empty()
                    || matches!(
                        next.as_str(),
                        "later" | "ago" | "before" | "after" | "more" | "please" | "and"
                    ))
            {
                // "first" and other ordinals keep the original duration guard.
                return None;
            }
        }
        let n = ordinal_word().get(w.as_str()).copied()?;
        Some(format!("{}{}", n, ord_suffix(n)))
    })
}

fn years(t: &str) -> String {
    let numword_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion)";
    let pat = format!(r#"\b(?<yr>(?:{numword_alt})(?:\s+(?:{numword_alt})){{1,3}})\b"#);
    re_sub(t, &pat, |m| {
        let words: Vec<&str> = split_words(m.name("yr")?.as_str());
        // Year-shaped: at most 4 words.
        if words.len() > 4 {
            return None;
        }
        // Reject if the match starts with a scale word (thousand/million/
        // billion) - that's the middle of a cardinal, not a year.
        let first = words[0].to_lowercase();
        if scales().contains_key(first.as_str()) {
            return None;
        }
        // "twenty one" / "twenty three" / "twenty five" are cardinal counts,
        // not years: the century reading only applies to "twenty twenty six"
        // and "twenty twenty". Left alone, "she is twenty one" became
        // "she is 2001" and "twenty three people" became "2003 people".
        // The module contract (see the file header) is that an ambiguous
        // minimal pair stays spelled for the polish layer.
        if words.len() == 2
            && first == "twenty"
            && units().contains_key(words[1].to_lowercase().as_str())
        {
            return None;
        }
        // Reject if more number words follow (this is a longer cardinal).
        let end = m.get(0).unwrap().end();
        let after = &t[end..];
        let after_words: Vec<&str> = split_words(after);
        if let Some(nxt) = after_words.first() {
            let nl = nxt.to_lowercase();
            if units().contains_key(nl.as_str())
                || tens().contains_key(nl.as_str())
                || scales().contains_key(nl.as_str())
            {
                return None;
            }
        }
        let yr = parse_year(&words)?;
        Some(yr.to_string())
    })
}

fn phone_digit_runs(t: &str) -> String {
    let digit_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine)";
    // The separator goes *between* tokens, never after the last one. With a
    // trailing `\s*` the match consumed the space that followed the number, so
    // "555 010 9999 at" came out as "555-010-9999at".
    let one = format!(r"(?:\b(?:{digit_alt})\b|\b\d{{1,4}}\b)");
    let pat = format!(r"(?P<lead>^|\s){one}(?:\s*{one})+");
    re_sub(t, &pat, |m| {
        let toks = split_words(m.get(0)?.as_str());
        let mut out: Vec<String> = Vec::new();
        for w in &toks {
            let wl = w.to_lowercase();
            if let Some(&v) = units().get(wl.as_str()) {
                if v < 10 {
                    out.push(v.to_string());
                } else {
                    return None;
                }
            } else if w.len() <= 4 && w.chars().all(|c| c.is_ascii_digit()) {
                out.push(w.to_string());
            } else {
                return None;
            }
        }
        let d: String = out.join("");
        if d.len() == 7 || d.len() == 10 {
            return Some(format!("{}{}", lead_of(m), fmt_phone(&d)));
        }
        let has_word = toks.iter().any(|w| {
            units()
                .get(w.to_lowercase().as_str())
                .map(|v| *v < 10)
                .unwrap_or(false)
        });
        let has_word_zero = toks
            .iter()
            .any(|w| ["zero", "oh", "o"].contains(&w.to_lowercase().as_str()));
        if has_word && has_word_zero && d.len() <= 6 {
            return Some(format!("{}{}", lead_of(m), d));
        }
        None
    })
}

fn mixed_state(t: &str) -> String {
    let numword_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion)";
    let pat = format!(r"\b(\d[\d,]*\s+(?:hundred|thousand)(?:\s+(?:{numword_alt}))*)\b");
    re_sub(t, &pat, |m| {
        let words: Vec<&str> = split_words(m.get(1)?.as_str());
        let n = words_to_int(&words)?;
        Some(comma(n))
    })
}

fn cardinals(t: &str) -> String {
    let numword_no_and_alt = r"(?:zero|oh|o|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million|billion)";
    let numword_alt = format!(r"{numword_no_and_alt}|and");
    let pat = format!(r"\b(?:{numword_no_and_alt})\b(?:\s+(?:{numword_alt})\b)*");
    re_sub(t, &pat, |m| {
        let raw = m.get(0)?.as_str();
        let mut words = split_words(raw).to_vec();
        if words.is_empty() {
            return None;
        }
        let mut trail_and = 0;
        while let Some(last) = words.last() {
            if *last == "and" {
                words.pop();
                trail_and += 1;
            } else {
                break;
            }
        }
        if words.is_empty() {
            return None;
        }
        let tail_and = " and".repeat(trail_and);
        let lower_words: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let lower_refs: Vec<&str> = lower_words.iter().map(|s| s.as_str()).collect();
        let n = words_to_int(&lower_refs)?;
        if n < AP_THRESHOLD {
            let end = m.get(0).unwrap().end();
            let after = &t[end..];
            let after_words: Vec<&str> = split_words(after);
            if let Some(nxt) = after_words.first() {
                let nxtw = token_core(nxt);
                if unit_nouns().contains_key(nxtw.as_str()) {
                    return Some(format!("{}{}", n, tail_and));
                }
                if [
                    "year", "years", "month", "months", "week", "weeks", "day", "days",
                ]
                .contains(&nxtw.as_str())
                    && after_words.get(1).map(|w| token_core(w)) == Some("old".into())
                {
                    return Some(format!("{}{}", n, tail_and));
                }
            }
            return None;
        }
        Some(format!("{}{}", comma(n), tail_and))
    })
}

fn numeric_ranges(t: &str) -> String {
    re_sub(t, r"(?P<lead>^|[^\d.-])\b(\d+)\s+to\s+(\d+)\b", |m| {
        let lead = m.name("lead").map(|l| l.as_str()).unwrap_or("");
        let a = m.get(2)?.as_str();
        let b = m.get(3)?.as_str();
        Some(format!("{lead}{a}-{b}"))
    })
}

fn keep_magnitude(t: &str) -> String {
    re_sub(t, r"\$(\d[\d,]*)", |m| {
        let num_str = m.get(1)?.as_str().replace(',', "");
        let n: u64 = num_str.parse().ok()?;
        if n >= 1_000_000_000 && n.is_multiple_of(1_000_000_000) {
            Some(format!("${} billion", n / 1_000_000_000))
        } else if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
            Some(format!("${} million", n / 1_000_000))
        } else {
            None
        }
    })
}

fn restore_protected(t: &str, protected: &[String]) -> String {
    let mut t = t.to_string();
    for (i, span) in protected.iter().enumerate() {
        let sentinel = format!("\u{0}{i}\u{0}");
        t = t.replacen(&sentinel, span, 1);
    }
    t
}

fn tighten(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut prev_space = false;
    for c in t.chars() {
        if c == ' ' || c == '\t' {
            if !prev_space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_run_english() {
        assert!(should_run("en"));
        assert!(should_run("en-US"));
        assert!(should_run("en_US"));
        assert!(should_run(""));
        assert!(!should_run("de"));
        assert!(!should_run("fr"));
        assert!(!should_run("ja"));
    }

    #[test]
    fn cardinal_basic() {
        assert_eq!(
            normalize("seventy eight thousand five hundred and forty seven"),
            "78,547"
        );
        assert_eq!(normalize("ninety four"), "94");
        assert_eq!(
            normalize("six hundred ninety six thousand one hundred six"),
            "696,106"
        );
    }

    #[test]
    fn cardinal_idempotence() {
        assert_eq!(normalize("78,547"), "78,547");
        assert_eq!(normalize("94"), "94");
    }

    #[test]
    fn year_pair() {
        assert_eq!(normalize("twenty twenty six"), "2026");
        assert_eq!(normalize("twenty twenty five"), "2025");
    }

    #[test]
    fn date_basic() {
        assert_eq!(
            normalize("march twelfth two thousand seven"),
            "March 12, 2007"
        );
    }

    #[test]
    fn time_basic() {
        assert_eq!(normalize("six fifty p m"), "6:50 PM");
    }

    #[test]
    fn phone_basic() {
        assert_eq!(
            normalize("eight two four six one nine six one seven five"),
            "824-619-6175"
        );
    }

    #[test]
    fn money_basic() {
        assert_eq!(
            normalize("six thousand two hundred thirty nine dollars"),
            "$6,239"
        );
    }

    #[test]
    fn email_basic() {
        assert_eq!(normalize("casey at proton dot me"), "casey@proton.me");
    }

    #[test]
    fn url_basic() {
        assert_eq!(
            normalize("stackoverflow dot io slash blog"),
            "stackoverflow.io/blog"
        );
    }

    #[test]
    fn negative_give_me_five() {
        assert_eq!(normalize("give me five"), "give me five");
    }

    #[test]
    fn negative_cloud_nine() {
        assert_eq!(normalize("on cloud nine"), "on cloud nine");
    }

    #[test]
    fn words_to_int_strict() {
        assert_eq!(words_to_int(&["twenty", "five"]), Some(25));
        assert_eq!(words_to_int(&["one", "hundred", "and", "one"]), Some(101));
        assert_eq!(words_to_int(&["two", "zero", "three"]), None);
        assert_eq!(words_to_int(&["twenty", "twenty"]), None);
        assert_eq!(words_to_int(&["one", "twenty"]), None);
        assert_eq!(
            words_to_int(&[
                "seventy", "eight", "thousand", "five", "hundred", "and", "forty", "seven"
            ]),
            Some(78547)
        );
    }

    #[test]
    fn parse_year_century_pair() {
        assert_eq!(parse_year(&["twenty", "twenty", "six"]), Some(2026));
        assert_eq!(parse_year(&["two", "thousand", "nine"]), Some(2009));
    }

    #[test]
    fn comma_grouping() {
        assert_eq!(comma(78547), "78,547");
        assert_eq!(comma(1000000), "1,000,000");
        assert_eq!(comma(42), "42");
    }

    #[test]
    fn ord_suffix_basic() {
        assert_eq!(ord_suffix(1), "st");
        assert_eq!(ord_suffix(2), "nd");
        assert_eq!(ord_suffix(3), "rd");
        assert_eq!(ord_suffix(11), "th");
        assert_eq!(ord_suffix(21), "st");
        assert_eq!(ord_suffix(102), "nd");
    }

    #[test]
    fn bug_004_second_noun_context() {
        // BUG-004: "second" in a duration/noun sense must not convert.
        for (input, want) in [
            ("in a second i will be there", "in a second i will be there"),
            ("a second of silence", "a second of silence"),
            ("the second floor", "the second floor"),
            ("a second attempt", "a second attempt"),
            ("the second one was better", "the second one was better"),
            (
                "the second meeting is tomorrow",
                "the second meeting is tomorrow",
            ),
            ("my second child", "my second child"),
            ("the second time i tried", "the second time i tried"),
            ("wait a second", "wait a second"),
            ("give me another second", "give me another second"),
            ("i will be there in a second", "i will be there in a second"),
            // Genuine ordinals still convert.
            ("the second tuesday", "the 2nd tuesday"),
            ("twenty second", "22nd"),
            ("the second of july", "the 2nd of july"),
        ] {
            assert_eq!(normalize(input), want, "input: {input:?}");
        }
    }
}

#[cfg(test)]
mod debug_cardinal {
    use super::*;
    #[test]
    fn debug_trace() {
        let input = "six hundred ninety six thousand one hundred six";
        // Test words_to_int directly
        let words: Vec<&str> = input.split_whitespace().collect();
        let result = words_to_int(&words);
        eprintln!("words_to_int({:?}) = {:?}", words, result);

        // Test the cardinals pass directly
        let result2 = cardinals(input);
        eprintln!("cardinals({:?}) = {:?}", input, result2);

        // Test the full normalize
        let result3 = normalize(input);
        eprintln!("normalize({:?}) = {:?}", input, result3);

        // Test years pass
        let result4 = years(input);
        eprintln!("years({:?}) = {:?}", input, result4);
    }
}
