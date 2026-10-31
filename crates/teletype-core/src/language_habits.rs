//! Strong-language coach: a private, non-judgmental read on when the user's
//! speech slips into mild, strong, or slur language — computed locally from
//! the learning store's raw ASR (what the user SAID, not what the pipeline
//! delivered) and patched into `Insights` by the desktop layer.
//!
//! The lexicon is curated, not filtered: a speech-to-text engine never
//! produces disguised spellings, so a short tiered word list with
//! word-boundary matching beats an obfuscation crate (evaluated and
//! rejected: no per-word spans). Slurs are counted but their text is never
//! returned — not even masked.

use serde::Serialize;

use crate::history::DictationEntry;
use crate::learning::store::DictationRow;

/// The day boundary used for `clean_days_streak` (same UTC basis as
/// `stats::day_start_ms`).
const DAY_MS: u64 = 86_400_000;

/// Mild tier: frustration words that land as an exclamation rather than an
/// attack. "hell" is listed separately from "hell" the noun — the matcher is
/// word-boundary and case-insensitive, and both readings are the same word.
pub const MILD_WORDS: &[&str] = &["damn", "hell", "crap", "bloody", "bugger", "drat"];

/// Strong tier: the f-word and s-word families and the direct personal
/// insults that ship in the flat list the old habit card counted.
pub const STRONG_WORDS: &[&str] = &[
    "fuck", "fucking", "fucks", "shit", "shits", "shitty", "bitch", "bastard", "wanker", "bollocks",
];

/// Slur tier: counted only. The words are `&'static str` in a private
/// constant and the only code path that touches them is the count in
/// `scan_text` — no API in this module can return one, masked or not.
const SLUR_WORDS: &[&str] = &["nigger", "nigga", "fag", "faggot", "kike", "spic"];

/// App categories that count as "work" for the context split and the
/// clean-days streak. Everything else is `personal` (Chat, Social) or
/// `other` (Web, Terminal, unknown).
const WORK_CATEGORIES: &[&str] = &["email", "docs", "code"];

/// -1 = decreasing, 0 = stable, 1 = increasing (vs. the prior equal window).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Trend {
    Down,
    #[default]
    Flat,
    Up,
}

/// One context bucket: how the window's language split across where it was
/// said. `work` is Email/Docs/Code, `personal` is Chat/Social.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCounts {
    pub mild: u32,
    pub strong: u32,
    /// Slur tier: count only, never text.
    pub slur: u32,
}

/// The Language card's payload. Every word the UI can display is already
/// masked here; the raw lexicon never leaves this module.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageSummary {
    /// `"spoken"` when counted from the learning store's raw ASR,
    /// `"final"` when the store was absent and history text was used.
    pub source: String,
    pub mild: u32,
    pub strong: u32,
    /// Count only: the words themselves are never returned.
    pub slur: u32,
    pub work: ContextCounts,
    pub personal: ContextCounts,
    pub other: ContextCounts,
    /// Top words of the mild/strong tiers, masked (first letter + asterisks).
    pub top_words: Vec<MaskedWord>,
    /// Trend of the strong tier vs the prior equal-length window.
    pub trend: Trend,
    /// Consecutive days (ending today or yesterday) with zero strong words
    /// in work apps.
    pub clean_days_streak: u32,
}

/// A ranked word with its display form already masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskedWord {
    /// e.g. "f***"; the raw word is never included.
    pub label: String,
    pub count: u32,
}

/// One take's input to [`compute_language_summary`]. The desktop layer
/// builds these from learning-store rows (`raw_asr`) or, when the store is
/// absent, from history entries (final text).
pub struct LanguageTake<'a> {
    pub text: &'a str,
    pub app_category: &'a str,
    pub ts: u64,
}

impl<'a> LanguageTake<'a> {
    /// The context bucket this take lands in: `"work"`, `"personal"`, or
    /// `"other"`.
    fn bucket(&self) -> &'static str {
        bucket_for(self.app_category)
    }
}

impl<'a> From<&'a DictationRow> for LanguageTake<'a> {
    fn from(row: &'a DictationRow) -> Self {
        Self {
            text: &row.raw_asr,
            app_category: &row.app_category,
            ts: row.ts,
        }
    }
}

/// Mask a word for display: first letter, then one asterisk per remaining
/// letter ("fuck" → "f***", "shit" → "s***"). The raw form is never part of
/// the return value.
pub fn mask_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => {
            let count = chars.count();
            let mut out = String::with_capacity(count + 1);
            out.push(first);
            for _ in 0..count {
                out.push('*');
            }
            out
        }
        None => String::new(),
    }
}

/// The tier a bare lowercase word belongs to, if any: direct lexicon match
/// first, then plural/-ing inflection of a lexicon stem (the stem's tier).
pub fn tier_of(word: &str) -> Option<&'static str> {
    if MILD_WORDS.contains(&word) {
        Some("mild")
    } else if STRONG_WORDS.contains(&word) {
        Some("strong")
    } else if SLUR_WORDS.contains(&word) {
        Some("slur")
    } else {
        inflection_of(word).map(|stem| {
            if MILD_WORDS.contains(&stem) {
                "mild"
            } else {
                "strong"
            }
        })
    }
}

/// True when `word` (bare, lowercase) is a plural or -ing inflection of a
/// lexicon stem: "fucks" → "fuck", "shits" → "shit", "damns" → "damn".
/// The stem is always a full lexicon entry, so the result is a `&'static
/// str` straight from the table.
fn inflection_of(word: &str) -> Option<&'static str> {
    for suffix in ["s", "ing"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            if stem.len() >= 3 {
                if let Some(entry) = STRONG_WORDS
                    .iter()
                    .chain(MILD_WORDS.iter())
                    .find(|e| **e == stem)
                {
                    return Some(entry);
                }
            }
        }
    }
    None
}

fn bucket_for(app_category: &str) -> &'static str {
    let cat = app_category.trim().to_ascii_lowercase();
    if WORK_CATEGORIES.contains(&cat.as_str()) {
        "work"
    } else if matches!(cat.as_str(), "chat" | "social") {
        "personal"
    } else {
        "other"
    }
}

/// Count the lexicon hits in one text, splitting them by context bucket.
/// Word-boundary matching: a token is a run of letters/digits, compared
/// case-insensitively after trimming punctuation; plurals and -ing forms
/// resolve to their stem.
fn scan_text(
    text: &str,
    bucket: &mut ContextCounts,
    per_word: &mut std::collections::HashMap<String, u32>,
) {
    for token in text.split_whitespace() {
        let bare: String = token
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if bare.is_empty() {
            continue;
        }
        let tier = tier_of(&bare).or_else(|| inflection_of(&bare));
        if let Some(tier) = tier {
            match tier {
                "mild" => bucket.mild += 1,
                "strong" => bucket.strong += 1,
                _ => bucket.slur += 1,
            }
            // Slurs are counted but never ranked or displayed.
            if tier != "slur" {
                *per_word.entry(bare).or_insert(0) += 1;
            }
        }
    }
}

/// Per-window accumulator used by the summary and the prior-window trend
/// pass.
#[derive(Default)]
struct WindowCounts {
    mild: u32,
    strong: u32,
    slur: u32,
    work: ContextCounts,
    personal: ContextCounts,
    other: ContextCounts,
    per_word: std::collections::HashMap<String, u32>,
}

impl WindowCounts {
    fn add_take(&mut self, take: &LanguageTake<'_>) {
        let bucket = match take.bucket() {
            "work" => &mut self.work,
            "personal" => &mut self.personal,
            _ => &mut self.other,
        };
        scan_text(take.text, bucket, &mut self.per_word);
        self.mild = self.work.mild + self.personal.mild + self.other.mild;
        self.strong = self.work.strong + self.personal.strong + self.other.strong;
        self.slur = self.work.slur + self.personal.slur + self.other.slur;
    }

    fn total(&self) -> u32 {
        self.mild + self.strong + self.slur
    }
}

fn day_start(ts: u64) -> u64 {
    let secs = ts / 1000;
    (secs - secs % 86_400) * 1000
}

/// Consecutive days with zero strong words in work apps, ending today (a
/// day with no work takes at all is clean by definition).
fn clean_days_streak(takes: &[LanguageTake<'_>], now_ms: u64) -> u32 {
    let today = day_start(now_ms);
    let mut days_with_work_strong: std::collections::HashSet<u64> =
        std::collections::HashSet::new();
    for take in takes {
        if bucket_for(take.app_category) != "work" {
            continue;
        }
        for token in take.text.split_whitespace() {
            let bare: String = token
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if bare.is_empty() {
                continue;
            }
            if matches!(
                tier_of(&bare).or_else(|| inflection_of(&bare)),
                Some("strong")
            ) {
                days_with_work_strong.insert(day_start(take.ts));
                break;
            }
        }
    }

    let mut streak: u32 = 0;
    let mut cursor = today;
    if !days_with_work_strong.contains(&cursor) {
        streak += 1;
    }
    while let Some(prev) = cursor.checked_sub(DAY_MS) {
        if days_with_work_strong.contains(&prev) {
            break;
        }
        streak += 1;
        cursor = prev;
        if streak > 3650 {
            // ~10 years of clean work days: stop walking. A real user has no
            // data older than the app's first activity, so this cap bounds
            // walk cost, not the reported value.
            break;
        }
    }
    if days_with_work_strong.contains(&today) {
        // A strong word in a work app today resets the streak.
        0
    } else {
        streak
    }
}

/// Computes the Language card summary over `takes` in the selected window.
/// `prior_takes` is the equal-length window immediately before it (for the
/// trend); pass an empty slice for lifetime. `now_ms` anchors the streak.
pub fn compute_language_summary(
    takes: &[LanguageTake<'_>],
    prior_takes: &[LanguageTake<'_>],
    now_ms: u64,
    source: &str,
) -> LanguageSummary {
    let mut cur = WindowCounts::default();
    for take in takes {
        cur.add_take(take);
    }
    let mut prior = WindowCounts::default();
    for take in prior_takes {
        prior.add_take(take);
    }

    // Trend: current window total vs the prior window's total.
    let trend = match (prior.total(), cur.total()) {
        (p, c) if c > p => Trend::Up,
        (p, c) if c < p => Trend::Down,
        _ => Trend::Flat,
    };

    let mut top: Vec<(String, u32)> = cur.per_word.iter().map(|(w, c)| (w.clone(), *c)).collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(5);
    let top_words: Vec<MaskedWord> = top
        .into_iter()
        .map(|(w, c)| MaskedWord {
            label: mask_word(&w),
            count: c,
        })
        .collect();

    LanguageSummary {
        source: source.to_string(),
        mild: cur.mild,
        strong: cur.strong,
        slur: cur.slur,
        work: cur.work,
        personal: cur.personal,
        other: cur.other,
        top_words,
        trend,
        clean_days_streak: clean_days_streak(takes, now_ms),
    }
}

/// History-based fallback when the learning store is absent: the final
/// (polished) text is all that exists, so the summary says `"final"`.
///
/// The `app_category` strings are leaked so the returned takes own their
/// strings; the slice is short (one per history entry in the window) and
/// lives for the duration of one `get_insights` call. The `AppType` names
/// are mapped onto the same lowercase category vocabulary the learning
/// store uses (`Coding` → `code`, `Document` → `docs`); a `None` context
/// or `Unknown` type lands in `other`.
pub fn history_takes<'a>(
    entries: impl IntoIterator<Item = &'a DictationEntry>,
) -> Vec<LanguageTake<'a>> {
    entries
        .into_iter()
        .map(|e| {
            let cat = match e.context.as_ref().map(|c| c.application_type) {
                Some(crate::context::AppType::Email) => "email",
                Some(crate::context::AppType::Chat) => "chat",
                Some(crate::context::AppType::Social) => "social",
                Some(crate::context::AppType::Coding) => "code",
                Some(crate::context::AppType::Document) => "docs",
                Some(crate::context::AppType::Browser) => "browser",
                Some(crate::context::AppType::Terminal) => "terminal",
                _ => "other",
            };
            LanguageTake {
                text: &e.text,
                app_category: cat,
                ts: e.created_at,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_767_225_600_000 + 30 * DAY_MS; // 2026-01-31 UTC

    fn take(text: &str, cat: &str, ts: u64) -> LanguageTake<'static> {
        LanguageTake {
            text: Box::leak(text.into()),
            app_category: Box::leak(cat.into()),
            ts,
        }
    }

    #[test]
    fn tiers_are_classified() {
        assert_eq!(tier_of("damn"), Some("mild"));
        assert_eq!(tier_of("fuck"), Some("strong"));
        assert_eq!(tier_of("nigga"), Some("slur"));
        assert_eq!(tier_of("hello"), None);
    }

    #[test]
    fn plurals_and_ing_forms_resolve_to_their_stem() {
        assert_eq!(
            tier_of("fucks").or_else(|| inflection_of("fucks")),
            Some("strong")
        );
        assert_eq!(tier_of("fucking"), Some("strong"));
        assert_eq!(tier_of("shitty"), Some("strong"));
        // A normal plural of a normal word is not a hit.
        assert_eq!(
            tier_of("shits").or_else(|| inflection_of("shits")),
            Some("strong")
        );
        assert_eq!(inflection_of("damns"), Some("damn"));
        assert_eq!(inflection_of("helloes"), None);
    }

    #[test]
    fn masking_keeps_first_letter_and_asterisks() {
        assert_eq!(mask_word("fuck"), "f***");
        assert_eq!(mask_word("shit"), "s***");
        assert_eq!(mask_word("fucking"), "f******");
        assert_eq!(mask_word("a"), "a");
        assert_eq!(mask_word(""), "");
    }

    #[test]
    fn slur_words_never_appear_in_output() {
        let takes = vec![take(
            "what the hell, that nigger comment",
            "chat",
            NOW - DAY_MS,
        )];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(s.slur, 1);
        let json = serde_json::to_string(&s).unwrap();
        for w in SLUR_WORDS {
            assert!(
                !json.to_lowercase().contains(w),
                "slur text leaked into the summary: {json}"
            );
        }
        // The masked top words must not reconstruct one either.
        for w in &s.top_words {
            for slur in SLUR_WORDS {
                assert!(
                    !slur.starts_with(&w.label.chars().next().unwrap().to_string())
                        || w.label.len() != slur.len(),
                    "masked label {w:?} matches a slur shape"
                );
            }
        }
    }

    #[test]
    fn word_boundaries_do_not_match_inside_words() {
        let takes = vec![take("the shipping dock and her shell", "email", NOW)];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(s.mild, 0);
        assert_eq!(s.strong, 0);
    }

    #[test]
    fn work_vs_personal_split_by_app_category() {
        let takes = vec![
            take("that's damn annoying", "email", NOW - 3 * DAY_MS),
            take("what the fuck", "chat", NOW - 2 * DAY_MS),
            take("bloody mess", "code", NOW - DAY_MS),
            take("crap", "browser", NOW),
        ];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(s.work.mild, 2); // damn (email) + bloody (code)
        assert_eq!(s.work.strong, 0);
        assert_eq!(s.personal.strong, 1); // fuck (chat)
        assert_eq!(s.other.mild, 1); // crap (browser)
        assert_eq!(s.mild, 3);
        assert_eq!(s.strong, 1);
    }

    #[test]
    fn trend_compared_to_prior_equal_window() {
        // Trend compares the current window's total against the prior
        // window's total (mild+strong+slur each).
        let prior = vec![take("damn it", "email", NOW - 10 * DAY_MS)];
        let cur = vec![take("what the fuck damn", "email", NOW - DAY_MS)];
        let s = compute_language_summary(&cur, &prior, NOW, "spoken");
        assert_eq!(s.trend, Trend::Up); // 2 total > 1 prior total

        let s2 = compute_language_summary(&prior, &cur, NOW, "spoken");
        assert_eq!(s2.trend, Trend::Down); // 1 total < 2 prior total

        // Equal windows: 2 total vs 2 prior total → Flat.
        let s3 = compute_language_summary(&cur, &cur, NOW, "spoken");
        assert_eq!(s3.trend, Trend::Flat);
    }

    #[test]
    fn clean_days_streak_counts_consecutive_clean_work_days() {
        // Strong word in a work app two days ago breaks the streak at 2
        // (today + yesterday).
        let takes = vec![
            take("that's fuck off", "email", NOW - 2 * DAY_MS),
            take("clean day", "email", NOW - DAY_MS),
            take("also clean", "chat", NOW),
        ];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(s.clean_days_streak, 2);

        // No work takes at all: today and yesterday are clean.
        let takes2 = vec![take("damn", "chat", NOW)];
        let s2 = compute_language_summary(&takes2, &[], NOW, "spoken");
        assert_eq!(s2.clean_days_streak, 3651); // capped walk

        // Strong word in a personal app does not break the work streak.
        let takes3 = vec![take("damn it", "chat", NOW - DAY_MS)];
        let s3 = compute_language_summary(&takes3, &[], NOW, "spoken");
        assert_eq!(s3.clean_days_streak, 3651);

        // A strong word in a work app today resets the streak to 0.
        let takes4 = vec![take("that's fuck off", "email", NOW)];
        let s4 = compute_language_summary(&takes4, &[], NOW, "spoken");
        assert_eq!(s4.clean_days_streak, 0);
    }

    #[test]
    fn top_words_are_masked_and_ranked() {
        let takes = vec![
            take("damn, damn, damn", "chat", NOW - DAY_MS),
            take("hell yeah and damn", "chat", NOW),
        ];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(
            s.top_words[0],
            MaskedWord {
                label: "d***".into(),
                count: 4
            }
        );
        assert_eq!(
            s.top_words[1],
            MaskedWord {
                label: "h***".into(),
                count: 1
            }
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("damn") && !json.contains("hell"));
    }

    #[test]
    fn source_is_reported_from_the_caller() {
        let s = compute_language_summary(&[], &[], NOW, "final");
        assert_eq!(s.source, "final");
        let s2 = compute_language_summary(&[], &[], NOW, "spoken");
        assert_eq!(s2.source, "spoken");
    }

    #[test]
    fn dictation_row_uses_raw_asr_not_pasted() {
        let row = DictationRow {
            id: 1,
            ts: NOW,
            app_id: "x".into(),
            app_category: "email".into(),
            engine: "e".into(),
            lang: "en".into(),
            audio_path: None,
            raw_asr: "that was fucking awful".into(),
            words_json: "[]".into(),
            pasted: "that was awful".into(),
            observed: None,
            edit_ops_json: None,
            polish_model: None,
            polish_skipped: false,
        };
        let takes: Vec<LanguageTake<'_>> = vec![(&row).into()];
        let s = compute_language_summary(&takes, &[], NOW, "spoken");
        assert_eq!(s.strong, 1);
        assert_eq!(s.work.strong, 1);
    }

    #[test]
    fn empty_input_is_an_empty_summary() {
        let s = compute_language_summary(&[], &[], NOW, "spoken");
        assert_eq!(s.mild, 0);
        assert_eq!(s.strong, 0);
        assert_eq!(s.slur, 0);
        assert!(s.top_words.is_empty());
        assert_eq!(s.trend, Trend::Flat);
    }

    #[test]
    fn history_takes_map_app_types_to_categories() {
        use crate::context::{AppType, ApplicationContext};
        fn entry(at: u64, text: &str, app_type: Option<AppType>) -> crate::history::DictationEntry {
            crate::history::DictationEntry {
                id: at.to_string(),
                created_at: at,
                text: text.into(),
                context: app_type.map(|t| ApplicationContext {
                    application_id: String::new(),
                    application_name: String::new(),
                    application_type: t,
                    window_title: None,
                    confidence: 1.0,
                }),
                duration_ms: None,
            }
        }
        let e1 = entry(NOW - DAY_MS, "damn", Some(AppType::Email));
        let e2 = entry(NOW, "what the hell", Some(AppType::Coding));
        let e3 = entry(NOW, "crap", None);
        let takes = history_takes([&e1, &e2, &e3]);
        let s = compute_language_summary(&takes, &[], NOW, "final");
        assert_eq!(s.source, "final");
        assert_eq!(s.work.mild, 2); // damn (Email) + hell (Coding→code)
        assert_eq!(s.other.mild, 1); // crap (no context)
    }
}
