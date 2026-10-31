//! Usage insights: impact metrics, speaking speed, personal records,
//! milestones, and a contribution heatmap — all computed locally from the
//! dictation history.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::history::DictationHistory;
use crate::learning::diff::{self, EditOp};
use crate::learning::judge::{self, JudgeContext};
use crate::learning::store::{DictationRow, LexiconRow, PersonalAccuracyRow};

/// Baseline typing speed for the "time saved" comparison, in words per minute.
///
/// 40 wpm, the widely-cited average typing speed and the baseline the user
/// asked to compare against. (The CHI '18 mean of 168,000 volunteers is 52
/// wpm; 40 is the rounder, more familiar figure and is what the UI states.)
pub const TYPING_WPM: u32 = 40;

/// A take shorter than this is not used for a speaking-rate figure. Opening
/// and closing the mic dominates a very short take, so the rate computed from
/// it is noise: a two-word entry in 300 ms reads as 400 wpm, which is
/// recitation territory (the Guinness spontaneous record is far lower).
pub const MIN_DURATION_FOR_RATE_MS: u64 = 1_200;

/// A measured rate above this is not believable for spontaneous speech, so it
/// is clamped rather than displayed. The fastest documented recitation is
/// ~655 wpm; nothing measured in ordinary conversation approaches 300.
pub const MAX_PLAUSIBLE_WPM: u32 = 300;

/// A ranked item (e.g. a phrase or app name).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedItem {
    pub label: String,
    pub count: u32,
    /// Relative frequency 0..=1 vs the top item (for bar rendering).
    pub share: f32,
}

/// Habit-words summary: total count, per-1k rate, ranked items, and trend vs
/// the prior window of equal length.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HabitWords {
    /// Total habit words in the current window (across both tiers).
    pub total: u32,
    /// Habit words per 1,000 words in the window.
    pub per_1k_words: f32,
    /// Ranked habit words (label = the word, count = occurrences).
    pub items: Vec<RankedItem>,
    /// Trend: -1 = decreasing, 0 = stable, 1 = increasing (vs. prior equal window).
    pub trend: i8,
}

/// The impact of dictating instead of typing.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    /// Measured words per minute while dictating: total words over total
    /// measured microphone time. `None` when no take has a usable duration
    /// (a fresh install, or history written before durations were recorded).
    ///
    /// This is a measurement. It used to be `total_words / (total_words / 150)`,
    /// which is a restatement of a constant: it printed "290 wpm" for a user
    /// who had dictated 290 words, and the headline figure moved with the word
    /// count rather than with how fast the user actually spoke.
    pub words_per_minute: Option<u32>,
    /// Measured speaking rate against the [`TYPING_WPM`] baseline.
    /// `None` for the same reason as `words_per_minute`.
    pub times_faster: Option<f32>,
    /// Minutes saved against typing the same words at [`TYPING_WPM`].
    ///
    /// This one needs no measured duration, because both sides are derived
    /// from the same word count at a stated rate. It is an estimate and is
    /// labelled as one: it assumes dictation is hands-free and that the words
    /// would have been typed rather than skipped.
    pub time_saved_minutes: u32,
    /// Human-readable time saved, e.g. "2 hr 15 min" or "45 min".
    pub time_saved_label: String,
    /// Minutes spent actually speaking, from measured durations.
    /// `None` when nothing has a usable duration.
    pub minutes_spoken: Option<u32>,
    /// Minutes it would have taken to type the same words at [`TYPING_WPM`].
    pub minutes_typed: u32,
    /// "You've written N college essays!" style flourish.
    pub essays: u32,
    /// Takes that contributed a measured duration, and those skipped as too
    /// short to rate. Shown so the rate is never presented as covering more
    /// than it does.
    pub rated_takes: u32,
    pub skipped_takes: u32,
}

/// A single personal record.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub label: String,
    pub value: String,
}

/// One milestone tier, e.g. "1K" words.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub label: String,
    pub threshold: u32,
    pub reached: bool,
}

/// A milestone row (Words / Transcriptions / Streak) with its tiers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneRow {
    pub category: String,
    /// Current value for this category.
    pub current: u32,
    pub items: Vec<Milestone>,
}

/// One day in the contribution heatmap.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    /// UTC day start in ms.
    pub date: u64,
    pub words: u32,
    /// 0..=4 intensity bucket for coloring.
    pub level: u8,
}

/// One day in the 14-day activity series (oldest first).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    /// Short display label, e.g. "Mon 16".
    pub label: String,
    /// UTC day start in ms.
    pub date: u64,
    pub words: u32,
    pub dictations: u32,
}

/// The insights payload for the Insights screen.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    /// The most repeated 2- and 3-word phrases across all dictations.
    pub top_phrases: Vec<RankedItem>,
    /// Where the user dictates most, by app name.
    pub top_apps: Vec<RankedItem>,
    /// Dictations per app category (Email, Chat, Social, Code, Docs, Web, Terminal, Other).
    pub app_categories: Vec<RankedItem>,
    /// Days with ≥1 dictation in the last 7 days (0..=7).
    pub active_days_last_7: u32,
    /// Words in the equal-length window immediately before the selected one. `None` for lifetime.
    pub prior_window_words: Option<u32>,
    /// Percent change vs `prior_window_words`, 1 decimal. `None` when lifetime or prior had 0 words.
    pub words_change_pct: Option<f32>,
    /// AutoText trigger/snippet → use count, all time (patched by the desktop layer).
    #[serde(default)]
    pub top_autotext: Vec<RankedItem>,
    /// Filler word → times removed, all time (patched by the desktop layer).
    #[serde(default)]
    pub top_fillers: Vec<RankedItem>,
    /// Average words per dictation.
    pub avg_words_per_dictation: u32,
    /// The hour of day (UTC, 0-23) with the most dictations.
    pub busiest_hour: Option<u32>,
    /// Top weekdays by word count in the window (0=Mon..6=Sun), with count and
    /// share normalized to the busiest day. Zero-count days are omitted.
    pub top_days: Vec<RankedItem>,
    /// The single best weekday (highest count), or None if window has no data.
    pub best_day: Option<String>,
    /// Words per hour-band (0=Early morning … 4=Evening).
    pub hour_bands: Vec<RankedItem>,
    /// Most frequent non-stopword tokens in the window.
    pub top_words: Vec<RankedItem>,
    /// Habit words: counts of hard/softer habit words in the window.
    #[serde(default)]
    pub habit_words: HabitWords,
    /// Total distinct words used (vocabulary size).
    pub vocabulary_size: u32,
    /// Words in the personal dictionary (taught + auto-learned). Shown in
    /// Insights so the self-learning loop is visible: the number climbs as
    /// the app learns from the user's edits.
    pub dictionary_words: u32,
    /// How many of those were learned automatically from the user's edits
    /// (the rest were taught by hand or seeded as built-ins).
    pub learned_words: u32,
    /// Total words dictated within the selected range (all time for
    /// Lifetime).
    pub total_words: u32,
    /// Total dictations within the selected range (all time for Lifetime).
    pub total_dictations: u32,
    /// Current streak (days).
    pub streak_days: u32,
    /// Longest streak (days).
    pub longest_streak_days: u32,
    /// Impact of dictating vs typing.
    pub impact: Impact,
    /// Personal records.
    pub records: Vec<Record>,
    /// Milestone rows.
    pub milestones: Vec<MilestoneRow>,
    /// Contribution heatmap, oldest first (26 weeks, 182 days).
    pub heatmap: Vec<HeatCell>,
    /// Daily activity for the last 14 days (oldest first).
    pub daily: Vec<DayStat>,
    /// Words dictated today.
    pub words_today: u32,
    /// Words dictated in the last 7 days.
    pub words_last_7_days: u32,
    /// Average words per active day over the last 7 days.
    pub avg_words_per_day: u32,
    /// Earliest recorded activity (epoch ms): the oldest dictation, learned
    /// dictionary word, or learned preference. `None` when the user has no
    /// data at all. The UI uses this to clamp custom date ranges so they
    /// cannot start before the user began using the app.
    pub first_activity_at: Option<u64>,
    /// Filler words removed, all time: the persisted all-time counters
    /// (patched by the desktop layer; the core leaves them at 0). The
    /// persisted counters carry no timestamps, so they cannot be sliced to a
    /// range.
    pub filler_removed: u32,
    /// AutoText expansions (typed triggers + spoken snippets), all time:
    /// the persisted all-time counters, patched by the desktop layer.
    pub autotext_expansions: u32,
    /// Personalization corrections observed within the requested range
    /// (learned, non-explicit preferences created in the range).
    pub personalization_corrections: u32,
    /// Dictionary words learned (automatically from edits) in the range.
    pub dictionary_learned: u32,
    /// Status message for the polish/transform state, shown in the Insights
    /// screen. `None` when a model is loaded and transforms are working.
    pub polish_status: Option<String>,
    /// The self-learning loop: edit rate, terms learned, and top fixes.
    /// Computed from the learning store by the desktop layer after
    /// `compute` (same patch pattern as `filler_removed`/`dictionary_words`);
    /// the default (all empty) is what a UI sees when the store is absent.
    #[serde(default)]
    pub learning: LearningSummary,
    /// The strong-language coach: tiered counts of mild/strong/slur words
    /// in what the user said, split by work vs personal context, with a
    /// clean-days streak. Computed from the learning store's raw ASR by the
    /// desktop layer (same patch pattern as `learning`); the default (all
    /// zero) is what a UI sees when there is no data at all.
    #[serde(default)]
    pub language: crate::language_habits::LanguageSummary,
}

/// Build a ranked list from a count map: drop zeros, sort count desc then
/// label asc, truncate to [`MAX_ITEMS`], share = count / top_count.
pub fn ranked_from_counts(counts: &std::collections::BTreeMap<String, u32>) -> Vec<RankedItem> {
    let mut items: Vec<RankedItem> = counts
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(label, &count)| RankedItem {
            label: label.clone(),
            count,
            share: 0.0,
        })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    items.truncate(MAX_ITEMS);
    if let Some(top) = items.first().map(|i| i.count) {
        for item in &mut items {
            item.share = item.count as f32 / top as f32;
        }
    }
    items
}

const NGRAM_SIZES: [usize; 2] = [2, 3];
const MAX_ITEMS: usize = 8;
const HEAT_WEEKS: u64 = 26;
const DAY_MS: u64 = 86_400_000;

/// Weekday label in the order `weekday_index` returns (Mon=0..Sun=6).
const WEEKDAY_LABELS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Hour-of-day bands for the "Your rhythm" card.
const HOUR_BAND_LABELS: [&str; 5] = ["Early morning", "Morning", "Midday", "Afternoon", "Evening"];

/// Map an hour (0-23) to a band index (0-4).
fn hour_band(hour: u32) -> usize {
    match hour {
        0..=5 => 0,   // Early morning
        6..=11 => 1,  // Morning
        12..=13 => 2, // Midday
        14..=17 => 3, // Afternoon
        _ => 4,       // Evening (18-23)
    }
}

/// English stopwords for the top-words ranking.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
    "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does",
    "did", "will", "would", "could", "should", "may", "might", "can", "shall", "i", "you", "he",
    "she", "it", "we", "they", "this", "that", "these", "those", "not", "so", "if", "as", "up",
    "out", "about", "into", "over", "after", "again", "then", "once", "here", "there", "when",
    "where", "why", "how", "all", "any", "both", "each", "few", "more", "most", "other", "some",
    "such", "no", "nor", "own", "same", "than", "too", "very",
];

fn is_stopword(w: &str) -> bool {
    STOPWORDS.contains(&w)
}

/// Hard habit words: strong profanity that always counts.
const HARD_HABIT_WORDS: &[&str] = &[
    "shit", "fuck", "fucking", "bitch", "bastard", "bollocks", "wanker",
];

/// Softer tier: frustration words that count toward the habit total.
const SOFT_HABIT_WORDS: &[&str] = &[
    "hate",
    "annoying",
    "stupid",
    "ridiculous",
    "absurd",
    "obnoxious",
];

fn is_habit_word(w: &str) -> bool {
    HARD_HABIT_WORDS.contains(&w) || SOFT_HABIT_WORDS.contains(&w)
}

// ── Learning (P7.8) ──────────────────────────────────────────────────────────

/// One day of edit activity in the Learning summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningDay {
    /// UTC day start in ms (same keying as the rest of Insights).
    pub day: u64,
    /// Words in the dictated (pasted) text that day.
    pub words: i64,
    /// Words the user replaced/inserted that day (see `compute_learning`).
    pub edited_words: i64,
}

/// A term the app learned from the user's edits, for the "learned this week"
/// list. `aliases` are the misheard forms it corrects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearnedTermInfo {
    pub term: String,
    pub aliases: Vec<String>,
    /// Epoch ms the term was learned (lexicon `created`).
    pub learned_at: u64,
}

/// A frequently corrected mishearing: `from` (as dictated) → `to` (as the
/// user corrected it), with the count of times it was fixed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFix {
    pub from: String,
    pub to: String,
    pub count: i64,
}

/// One replay run's personal-accuracy snapshot (error rate before/after
/// the personalization boost).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccuracyPoint {
    pub ts: u64,
    pub wer_before: f64,
    pub wer_after: f64,
}

/// The "Learning" section of the Insights screen: edit rate per 100 words,
/// daily edit activity, terms learned this week, the most frequent
/// mishearings fixed, and the personal-accuracy trend.
///
/// v1 computes everything from the stored learning rows at read time —
/// nothing writes `metric_daily` yet, so there is no new writer or schema
/// change here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningSummary {
    /// Edited words per 100 dictated words over the last 7 days.
    /// `None` when no words were dictated in the window.
    pub edit_rate_7d: Option<f64>,
    /// Daily edit activity, last 30 days, oldest first.
    pub daily: Vec<LearningDay>,
    /// Terms learned in the last 7 days (non-tombstoned), newest first.
    pub terms_learned_week: Vec<LearnedTermInfo>,
    /// The most frequent mishearings fixed, top 5 by count (desc).
    pub top_fixes: Vec<TrackedFix>,
    /// Personal-accuracy snapshots, oldest first, at most 10 points.
    pub accuracy: Vec<AccuracyPoint>,
    /// Corrections the pipeline made that the user **kept**, counted by the
    /// settle sweep after each row's 30-minute window (U3). A correction
    /// counts as kept only when the corrected form is still in the field at
    /// the anchored span: survival, not a click.
    pub kept_corrections: i64,
    /// The same corrections the user undid or wrote over.
    pub undone_corrections: i64,
    /// Pairs at three keeps and zero undoes: the point the fix would apply
    /// itself. Nothing rewrites text from this list yet; it is the evidence
    /// the auto-apply decision needs before such a stage exists.
    pub auto_apply_fixes: Vec<TrackedFix>,
}

/// Words the user changed in a pasted→observed edit: the **to-side** word
/// count of every changed span (the `to` side of every Insert and Replace
/// op from `diff_words`). Deletions count as zero — a removed word
/// contributed nothing the user wanted to keep — and Equal spans are
/// skipped. This is the numerator of the edit rate; it is deliberately
/// simpler than the judge's 1–4-word candidate filter, which exists to pick
/// out individual mishearings, not to measure edit volume.
fn changed_word_count(pasted: &str, observed: &str) -> i64 {
    diff::diff_words(pasted, observed)
        .iter()
        .map(|op| match op {
            EditOp::Insert(words) => words.len() as i64,
            EditOp::Replace { to, .. } => to.len() as i64,
            EditOp::Equal(_) | EditOp::Delete(_) => 0,
        })
        .sum()
}

/// Computes the Learning summary from stored learning rows.
///
/// `dictations` may be in any order (newest first, as the store returns
/// them); `accuracy` may be in any order (the store returns newest first).
pub fn compute_learning(
    dictations: &[DictationRow],
    accuracy: &[PersonalAccuracyRow],
    lexicon: &[LexiconRow],
    candidates: &[crate::learning::store::CandidateRow],
    now_ms: u64,
) -> LearningSummary {
    let today_start = crate::stats::day_start_ms(now_ms);

    // last 30 days of daily activity, oldest first.
    let daily_start = today_start.saturating_sub(29 * DAY_MS);
    let mut daily: HashMap<u64, (i64, i64)> = HashMap::new();
    // 7-day edit-rate numerator/denominator.
    let week_start = today_start.saturating_sub(6 * DAY_MS);
    let mut words_7d: i64 = 0;
    let mut edited_7d: i64 = 0;

    for row in dictations {
        let day = crate::stats::day_start_ms(row.ts);
        if day < daily_start {
            continue;
        }
        let words = row.pasted.split_whitespace().count() as i64;
        let edited = row
            .observed
            .as_deref()
            .map(|observed| changed_word_count(&row.pasted, observed))
            .unwrap_or(0);
        let entry = daily.entry(day).or_insert((0, 0));
        entry.0 += words;
        entry.1 += edited;
        if day >= week_start {
            words_7d += words;
            edited_7d += edited;
        }
    }
    let daily: Vec<LearningDay> = {
        let mut daily: Vec<LearningDay> = daily
            .into_iter()
            .map(|(day, (words, edited_words))| LearningDay {
                day,
                words,
                edited_words,
            })
            .collect();
        daily.sort_by_key(|d| d.day);
        daily
    };

    let edit_rate_7d = (words_7d != 0).then(|| 100.0 * edited_7d as f64 / words_7d as f64);

    // Top fixes: classify the most recent ≤ 500 observed edits and count the
    // mishearings by (alias, term). The cap bounds the per-call cost, since
    // `judge::classify` runs a phonetic similarity per changed span.
    const CLASSIFY_CAP: usize = 500;
    let mut fix_counts: HashMap<(String, String), i64> = HashMap::new();
    for row in dictations.iter().take(CLASSIFY_CAP) {
        let Some(observed) = row.observed.as_deref() else {
            continue;
        };
        for j in judge::classify(&row.pasted, observed, &JudgeContext::default()) {
            if let judge::Judgement::Mishearing { term, alias } = j {
                *fix_counts.entry((alias, term)).or_insert(0) += 1;
            }
        }
    }
    let mut top_fixes: Vec<TrackedFix> = fix_counts
        .into_iter()
        .map(|((from, to), count)| TrackedFix { from, to, count })
        .collect();
    top_fixes.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    top_fixes.truncate(5);

    // Terms learned this week: learned-source, non-tombstoned, created in
    // the last 7 days, newest first.
    let week_ago = now_ms.saturating_sub(7 * DAY_MS);
    let mut terms_learned_week: Vec<LearnedTermInfo> = lexicon
        .iter()
        .filter(|l| l.source == "learned" && !l.tombstoned && l.created >= week_ago)
        .map(|l| LearnedTermInfo {
            term: l.term.clone(),
            aliases: l.aliases.clone(),
            learned_at: l.created,
        })
        .collect();
    terms_learned_week.sort_by(|a, b| b.learned_at.cmp(&a.learned_at).then(a.term.cmp(&b.term)));

    // Accuracy: last ≤ 10 snapshots, oldest first.
    let mut accuracy: Vec<AccuracyPoint> = accuracy
        .iter()
        .map(|r| AccuracyPoint {
            ts: r.ts,
            wer_before: r.wer_before,
            wer_after: r.wer_after,
        })
        .collect();
    accuracy.sort_by_key(|p| p.ts);
    accuracy.truncate(10);

    // Survival evidence (U3): the settle sweep tallies kept/undone on the
    // candidate row for each correction pair. Only the mishearing kind is
    // judged by survival; style rows carry other evidence.
    let mut kept_corrections = 0i64;
    let mut undone_corrections = 0i64;
    let mut auto_apply_fixes: Vec<TrackedFix> = Vec::new();
    for c in candidates {
        if c.kind != crate::learning::settle::CANDIDATE_KIND {
            continue;
        }
        kept_corrections += c.accepted.max(0);
        undone_corrections += c.rejected.max(0);
        if c.auto_apply {
            auto_apply_fixes.push(TrackedFix {
                from: c.a.clone(),
                to: c.b.clone(),
                count: c.accepted,
            });
        }
    }
    auto_apply_fixes.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    auto_apply_fixes.truncate(5);

    LearningSummary {
        edit_rate_7d,
        daily,
        terms_learned_week,
        top_fixes,
        accuracy,
        kept_corrections,
        undone_corrections,
        auto_apply_fixes,
    }
}

fn format_minutes(total: u32) -> String {
    if total < 60 {
        format!("{total} min")
    } else {
        format!("{} hr {} min", total / 60, total % 60)
    }
}

/// "16 Sep" style month-day from a UTC day-start timestamp.
fn month_day_label(day_ms: u64) -> String {
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (day_ms / DAY_MS) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let _year = if m <= 2 { y + 1 } else { y };
    format!("{d} {}", MONTHS[(m - 1) as usize])
}

/// Weekday index (Monday = 0) for a count of days since the Unix epoch.
///
/// Lives here and is used by both `insights::day_label` and `stats::day_label`
/// because it was duplicated and the two copies disagreed: `stats.rs` had
/// `(4 + days) % 7` and this one had `(3 + days) % 7`, so every stats bar was
/// labelled a day late. The duplicated constant was the bug, so the constant is
/// now shared.
///
/// 1970-01-01 (day 0) was a Thursday, which is index 3 Monday-first.
pub(crate) fn weekday_index(days_since_epoch: u64) -> usize {
    (3 + days_since_epoch as usize) % 7
}

/// The three-letter weekday name for a count of days since the Unix epoch.
pub(crate) fn weekday_name(days_since_epoch: u64) -> &'static str {
    static WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    WEEKDAYS[weekday_index(days_since_epoch)]
}

/// "Mon 16" style label from a UTC day-start timestamp.
fn day_label(day_ms: u64) -> String {
    let days = day_ms / DAY_MS;
    format!("{} {}", weekday_name(days), month_day_label(day_ms))
}

/// Computes the impact of dictating vs typing.
///
/// Two different kinds of number, deliberately kept apart:
///
/// * **Time saved** needs only the word count, because both sides of the
///   comparison are derived from it: `words / TYPING_WPM` against
///   `words / measured_wpm`. It is an estimate and is labelled as one, but it
///   is monotone in the word count by construction, which is what the old
///   version was not.
/// * **Speaking rate** needs a measured duration. Entries without one are
///   skipped rather than assumed, and when nothing is left to measure the
///   figure is `None` instead of a number derived from the word count.
///
/// The old version computed `wpm = total_words / (total_words / 150)`. Because
/// both divisions were integer, the result was a function of the word count
/// alone: 290 words printed "290 wpm" and a "7x faster" badge, and dictating
/// one more word could make the reported time saved go *down*.
fn compute_impact(total_words: u32, rated: RateSample) -> Impact {
    // wpm is words *per minute*, so minutes = words / wpm. No 60 here.
    let minutes_typed = (total_words as f64 / TYPING_WPM as f64).round() as u32;

    // Time saved is computed in seconds and floored at zero, which makes it
    // monotone: more words can never reduce the saving.
    let minutes_spoken = rated.minutes_spoken();
    let time_saved = match minutes_spoken {
        Some(spoken) => minutes_typed.saturating_sub(spoken),
        // No measured rate, so fall back to comparing against a documented
        // central speaking estimate (~150 wpm) rather than pretending to know.
        None => {
            const CENTRAL_SPEECH_WPM: f64 = 150.0;
            let spoken = (total_words as f64 / CENTRAL_SPEECH_WPM).round() as u32;
            minutes_typed.saturating_sub(spoken)
        }
    };

    let words_per_minute = rated.words_per_minute();
    let times_faster = words_per_minute
        .map(|wpm| (((wpm as f64 / TYPING_WPM as f64) * 10.0).round() / 10.0) as f32);

    Impact {
        words_per_minute,
        times_faster,
        time_saved_minutes: time_saved,
        time_saved_label: format_minutes(time_saved),
        minutes_spoken,
        minutes_typed,
        essays: total_words / 500,
        rated_takes: rated.takes,
        skipped_takes: rated.skipped,
    }
}

/// The measured part of the impact calculation: which takes had a usable
/// duration, and what they add up to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RateSample {
    words: u64,
    millis: u64,
    takes: u32,
    skipped: u32,
    /// The single fastest take (words/minute), a personal record. `None`
    /// until a take long enough to time has been seen.
    peak_wpm: Option<u32>,
}

impl RateSample {
    /// Add one take. A missing duration, or one too short to time reliably, is
    /// counted as skipped and contributes nothing to the rate.
    fn add(&mut self, words: u32, duration_ms: Option<u64>) {
        match duration_ms {
            Some(ms) if ms >= MIN_DURATION_FOR_RATE_MS => {
                self.words += words as u64;
                self.millis += ms;
                self.takes += 1;
                // Per-take rate, clamped like the aggregate so a fluke burst
                // (a pasted phrase read back fast) can't set an impossible
                // "record".
                let wpm = ((words as f64) * 60_000.0 / ms as f64).round() as u32;
                let wpm = wpm.clamp(1, MAX_PLAUSIBLE_WPM);
                self.peak_wpm = Some(self.peak_wpm.unwrap_or(0).max(wpm).max(1));
            }
            _ => self.skipped += 1,
        }
    }

    /// The fastest single take, or `None` when no take was long enough to time.
    fn peak(&self) -> Option<u32> {
        self.peak_wpm.filter(|w| *w > 0)
    }

    /// Measured speaking time in minutes, rounded to one decimal and truncated
    /// to whole minutes for display.
    fn minutes_spoken(&self) -> Option<u32> {
        if self.millis == 0 {
            return None;
        }
        Some(((self.millis as f64 / 60_000.0).round()) as u32)
    }

    /// Measured words per minute, clamped to a physically plausible range.
    fn words_per_minute(&self) -> Option<u32> {
        if self.millis == 0 {
            return None;
        }
        let wpm = ((self.words as f64) * 60_000.0 / self.millis as f64).round() as u32;
        Some(wpm.clamp(1, MAX_PLAUSIBLE_WPM))
    }
}

/// Computes insights over the full history. `now_ms` anchors streaks and the
/// heatmap to "today". Equivalent to `compute_for_window(history, None, now_ms)`.
pub fn compute(history: &DictationHistory, now_ms: u64) -> Insights {
    compute_for_window(history, None, now_ms)
}

/// Computes insights over the full history, scoping the aggregate metrics to
/// `window` while keeping the habit views (today, last 7 days, streaks,
/// heatmap, daily series), personal records, milestones and `first_activity_at`
/// anchored to all time.
///
/// `window` is `None` for lifetime. Passing a window must not change any of
/// the all-time fields: they describe "now" and the whole history, so they
/// cannot move when the user switches filters.
pub fn compute_for_window(
    all: &DictationHistory,
    window: Option<(u64, u64)>,
    now_ms: u64,
) -> Insights {
    let today_start = crate::stats::day_start_ms(now_ms);

    // ── All-time pass: habit views, records, milestones, streaks. ─────────
    let mut day_words: HashMap<u64, u32> = HashMap::new();
    let mut day_dictations: HashMap<u64, u32> = HashMap::new();
    let mut rate = RateSample::default();
    let mut all_time_words: u32 = 0;
    let mut all_time_dictations: u32 = 0;
    let mut longest_words: u32 = 0;
    let mut first_activity_at: Option<u64> = None;

    for entry in &all.entries {
        all_time_dictations += 1;
        let wc = crate::stats::word_count(&entry.text);
        all_time_words += wc;
        longest_words = longest_words.max(wc);
        rate.add(wc, entry.duration_ms);
        first_activity_at =
            Some(first_activity_at.map_or(entry.created_at, |f| f.min(entry.created_at)));

        let day = crate::stats::day_start_ms(entry.created_at);
        *day_words.entry(day).or_insert(0) += wc;
        *day_dictations.entry(day).or_insert(0) += 1;
    }

    // Personal records (all-time).
    let most_words_day = day_words.values().copied().max().unwrap_or(0);
    let most_dictations_day = day_dictations.values().copied().max().unwrap_or(0);
    let mut records = vec![
        Record {
            label: "Longest dictation".into(),
            value: format!("{} words", longest_words),
        },
        Record {
            label: "Most words in a day".into(),
            value: format!("{} words", most_words_day),
        },
        Record {
            label: "Most in a day".into(),
            value: format!("{} dictations", most_dictations_day),
        },
    ];
    if let Some(peak) = rate.peak() {
        records.push(Record {
            label: "Fastest take".into(),
            value: format!("{} wpm", peak),
        });
    }

    // Streaks: current (ending today or yesterday) and longest.
    let (streak_days, longest_streak_days) = compute_streaks(&day_dictations, today_start);

    // Milestones (all-time).
    let milestones = vec![
        milestone_row(
            "Words",
            all_time_words,
            &[1_000, 10_000, 50_000, 100_000, 500_000, 1_000_000],
            &["1K", "10K", "50K", "100K", "500K", "1M"],
        ),
        milestone_row(
            "Transcriptions",
            all_time_dictations,
            &[50, 100, 500, 1_000, 5_000, 10_000],
            &["50", "100", "500", "1K", "5K", "10K"],
        ),
        milestone_row(
            "Streak",
            longest_streak_days,
            &[7, 14, 30, 60, 100, 365],
            &[
                "7 days", "14 days", "30 days", "60 days", "100 days", "1 year",
            ],
        ),
    ];

    // Heatmap: last 26 weeks ending this week, oldest first.
    let heatmap = build_heatmap(&day_words, today_start);

    // 14-day activity series (oldest first) + today / last-7-day totals.
    let week_start = today_start.saturating_sub(6 * DAY_MS);
    let mut daily: Vec<DayStat> = (0..14)
        .map(|i| today_start - (13 - i) * DAY_MS)
        .map(|d| DayStat {
            label: day_label(d),
            date: d,
            words: 0,
            dictations: 0,
        })
        .collect();
    let mut words_today: u32 = 0;
    let mut words_last_7: u32 = 0;
    for entry in &all.entries {
        let wc = crate::stats::word_count(&entry.text);
        let day = crate::stats::day_start_ms(entry.created_at);
        if day == today_start {
            words_today += wc;
        }
        if day >= week_start {
            words_last_7 += wc;
        }
        if let Some(slot) = daily.iter_mut().find(|s| s.date == day) {
            slot.words += wc;
            slot.dictations += 1;
        }
    }
    let last7: &[DayStat] = &daily[daily.len().saturating_sub(7)..];
    let active_days: u32 = last7.iter().filter(|d| d.dictations > 0).count() as u32;
    let active_days_last_7 = active_days;
    let avg_words_per_day = words_last_7 / active_days.max(1);

    // ── Window-scoped pass: totals, phrases, apps, hours, vocabulary, impact ─
    let in_window = |at: u64| window.is_none_or(|(from, to)| at >= from && at <= to);

    let mut ngram_counts: HashMap<(usize, String), u32> = HashMap::new();
    let mut app_counts: HashMap<String, u32> = HashMap::new();
    let mut app_category_counts: HashMap<String, u32> = HashMap::new();
    let mut hour_counts: [u32; 24] = [0; 24];
    let mut band_counts: [u32; 5] = [0; 5];
    let mut weekday_counts: [u32; 7] = [0; 7];
    let mut word_counts: HashMap<String, u32> = HashMap::new();
    let mut habit_counts: HashMap<String, u32> = HashMap::new();
    let mut total_words: u32 = 0;
    let mut dictations: u32 = 0;
    let mut vocab: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut window_rate = RateSample::default();

    for entry in &all.entries {
        if !in_window(entry.created_at) {
            continue;
        }
        dictations += 1;
        let words: Vec<&str> = entry.text.split_whitespace().collect();
        let wc = words.len() as u32;
        total_words += wc;
        window_rate.add(wc, entry.duration_ms);

        for w in &words {
            let bare = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if !bare.is_empty() {
                vocab.insert(bare);
            }
        }
        for &n in &NGRAM_SIZES {
            if words.len() >= n {
                for window in words.windows(n) {
                    let phrase = window
                        .iter()
                        .map(|w| {
                            w.trim_matches(|c: char| !c.is_alphanumeric())
                                .to_lowercase()
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    if phrase.is_empty() {
                        continue;
                    }
                    if phrase.chars().filter(|c| c.is_alphanumeric()).count() < n {
                        continue;
                    }
                    *ngram_counts.entry((n, phrase)).or_insert(0) += 1;
                }
            }
        }
        if let Some(ctx) = &entry.context {
            // Count any dictation whose app name was captured, even when the
            // app's category is Unknown — the user still dictated into it.
            let name = ctx.application_name.trim();
            if !name.is_empty() {
                *app_counts.entry(name.to_string()).or_insert(0) += 1;
            }
            // App category for the work-mix view.
            let cat = match ctx.application_type {
                crate::context::AppType::Email => "Email",
                crate::context::AppType::Chat => "Chat",
                crate::context::AppType::Social => "Social",
                crate::context::AppType::Coding => "Code",
                crate::context::AppType::Document => "Docs",
                crate::context::AppType::Browser => "Web",
                crate::context::AppType::Terminal => "Terminal",
                crate::context::AppType::Unknown => "Other",
            };
            *app_category_counts.entry(cat.to_string()).or_insert(0) += 1;
        }
        let secs = entry.created_at / 1000;
        let hour = (secs % 86_400) as usize / 3600;
        hour_counts[hour] += 1;
        band_counts[hour_band(hour as u32)] += wc;

        // Weekday: same UTC basis as day_start_ms / day_labels.
        let days_since_epoch = entry.created_at / DAY_MS;
        let weekday = weekday_index(days_since_epoch);
        weekday_counts[weekday] += wc;

        // Tokenize for top-words and habit-words (reuse the same lowercase
        // alnum normalization as the n-gram pass).
        for w in &words {
            let bare = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if bare.len() < 3 {
                continue;
            }
            if is_habit_word(&bare) {
                *habit_counts.entry(bare.clone()).or_insert(0) += 1;
            }
            if is_stopword(&bare) {
                continue;
            }
            *word_counts.entry(bare).or_insert(0) += 1;
        }
    }

    // Top phrases (drop 2-grams subsumed by an equal-count 3-gram, checked
    // at word boundaries: a 3-gram "a b c" subsumes "a b" and "b c" only
    // when both 2-grams appear with the same count as the 3-gram itself).
    let mut subsumed: std::collections::HashSet<(usize, String)> = std::collections::HashSet::new();
    for ((n, phrase), count) in ngram_counts.iter() {
        if *n != 3 {
            continue;
        }
        let words: Vec<&str> = phrase.split(' ').collect();
        if words.len() != 3 {
            continue;
        }
        for i in 0..2 {
            let two = format!("{} {}", words[i], words[i + 1]);
            let key = (2, two);
            if ngram_counts.get(&key) == Some(count) {
                subsumed.insert(key);
            }
        }
    }
    let mut phrases: Vec<RankedItem> = ngram_counts
        .into_iter()
        .map(|((n, phrase), count)| (n, phrase, count))
        .filter(|(_, phrase, count)| *count >= 2 && phrase.chars().count() <= 48)
        .filter(|(n, phrase, _)| !subsumed.contains(&(*n, phrase.clone())))
        .map(|(_, phrase, count)| RankedItem {
            label: phrase,
            count,
            share: 0.0,
        })
        .collect();
    phrases.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    phrases.truncate(MAX_ITEMS);
    if let Some(top) = phrases.first().map(|p| p.count) {
        for p in &mut phrases {
            p.share = p.count as f32 / top as f32;
        }
    }

    let mut apps: Vec<RankedItem> = app_counts
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            share: 0.0,
        })
        .collect();
    apps.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    apps.truncate(MAX_ITEMS);
    if let Some(top) = apps.first().map(|p| p.count) {
        for p in &mut apps {
            p.share = p.count as f32 / top as f32;
        }
    }

    // App categories: sort desc by count then label asc, share = count / top.
    let mut app_categories: Vec<RankedItem> = app_category_counts
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            share: 0.0,
        })
        .collect();
    app_categories.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    if let Some(top) = app_categories.first().map(|p| p.count) {
        for p in &mut app_categories {
            p.share = p.count as f32 / top as f32;
        }
    }

    let busiest_hour = hour_counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .max_by_key(|(_, c)| **c)
        .map(|(h, _)| h as u32);

    // Top days: weekdays with non-zero counts, share normalized to busiest day.
    let max_day_count = *weekday_counts.iter().max().unwrap_or(&0);
    let top_days: Vec<RankedItem> = weekday_counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(i, &c)| RankedItem {
            label: WEEKDAY_LABELS[i].to_string(),
            count: c,
            share: if max_day_count > 0 {
                c as f32 / max_day_count as f32
            } else {
                0.0
            },
        })
        .collect();
    let best_day = weekday_counts
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .and_then(|(i, &c)| {
            if c > 0 {
                Some(WEEKDAY_LABELS[i].to_string())
            } else {
                None
            }
        });

    // Hour bands: all 5 bands, share normalized to the busiest band.
    let max_band_count = *band_counts.iter().max().unwrap_or(&0);
    let hour_bands: Vec<RankedItem> = band_counts
        .iter()
        .enumerate()
        .map(|(i, &c)| RankedItem {
            label: HOUR_BAND_LABELS[i].to_string(),
            count: c,
            share: if max_band_count > 0 {
                c as f32 / max_band_count as f32
            } else {
                0.0
            },
        })
        .collect();

    // Top words: most frequent non-stopword tokens, capped at MAX_ITEMS.
    let mut top_words: Vec<RankedItem> = word_counts
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            share: 0.0,
        })
        .collect();
    top_words.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    top_words.truncate(MAX_ITEMS);
    if let Some(top) = top_words.first().map(|p| p.count) {
        for p in &mut top_words {
            p.share = p.count as f32 / top as f32;
        }
    }

    // Habit words: total, per-1k rate, ranked items, trend vs prior window.
    let habit_total: u32 = habit_counts.values().sum();
    let per_1k = if total_words > 0 {
        (habit_total as f32 / total_words as f32) * 1000.0
    } else {
        0.0
    };
    let mut habit_items: Vec<RankedItem> = habit_counts
        .into_iter()
        .map(|(label, count)| RankedItem {
            label,
            count,
            share: 0.0,
        })
        .collect();
    habit_items.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    habit_items.truncate(MAX_ITEMS);
    if let Some(top) = habit_items.first().map(|p| p.count) {
        for p in &mut habit_items {
            p.share = p.count as f32 / top as f32;
        }
    }

    // Prior window words and percent change.
    let (prior_window_words, words_change_pct) = if let Some((from, to)) = window {
        let span = to.saturating_sub(from);
        let prior_from = from.saturating_sub(span);
        let mut prior_sum: u32 = 0;
        for entry in &all.entries {
            if entry.created_at >= prior_from && entry.created_at < from {
                prior_sum += crate::stats::word_count(&entry.text);
            }
        }
        let pct = if prior_sum > 0 {
            let raw = (total_words as f32 - prior_sum as f32) / prior_sum as f32 * 100.0;
            Some((raw * 10.0).round() / 10.0)
        } else {
            None
        };
        (Some(prior_sum), pct)
    } else {
        (None, None)
    };

    // Trend: compare current window's per-1k rate to the prior equal-length window.
    let trend: i8 = if let Some((from, to)) = window {
        let window_ms = to.saturating_sub(from);
        if window_ms == 0 {
            0
        } else {
            let prior_from = from.saturating_sub(window_ms);
            // Count habit words in the prior window.
            let prior_in_window = |at: u64| at >= prior_from && at < from;
            let mut prior_words: u32 = 0;
            let mut prior_habit: u32 = 0;
            for entry in &all.entries {
                if !prior_in_window(entry.created_at) {
                    continue;
                }
                let words: Vec<&str> = entry.text.split_whitespace().collect();
                prior_words += words.len() as u32;
                for w in &words {
                    let bare = w
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase();
                    if bare.len() >= 3 && is_habit_word(&bare) {
                        prior_habit += 1;
                    }
                }
            }
            let prior_per_1k = if prior_words > 0 {
                (prior_habit as f32 / prior_words as f32) * 1000.0
            } else {
                0.0
            };
            if prior_per_1k == 0.0 && per_1k == 0.0 {
                0
            } else if per_1k > prior_per_1k + 0.5 {
                1
            } else if per_1k < prior_per_1k - 0.5 {
                -1
            } else {
                0
            }
        }
    } else {
        0 // Lifetime: no prior window to compare against.
    };

    let habit_words = HabitWords {
        total: habit_total,
        per_1k_words: per_1k,
        items: habit_items,
        trend,
    };

    Insights {
        top_phrases: phrases,
        top_apps: apps,
        app_categories,
        active_days_last_7,
        prior_window_words,
        words_change_pct,
        top_autotext: Vec::new(),
        top_fillers: Vec::new(),
        avg_words_per_dictation: total_words / dictations.max(1),
        busiest_hour,
        top_days,
        best_day,
        hour_bands,
        top_words,
        habit_words,
        vocabulary_size: vocab.len() as u32,
        dictionary_words: 0,
        learned_words: 0,
        total_words,
        total_dictations: dictations,
        streak_days,
        longest_streak_days,
        impact: compute_impact(total_words, window_rate),
        records,
        milestones,
        heatmap,
        daily,
        words_today,
        words_last_7_days: words_last_7,
        avg_words_per_day,
        first_activity_at,
        filler_removed: 0,
        autotext_expansions: 0,
        personalization_corrections: 0,
        dictionary_learned: 0,
        polish_status: None,
        learning: LearningSummary::default(),
        language: crate::language_habits::LanguageSummary::default(),
    }
}

fn milestone_row(
    category: &str,
    current: u32,
    thresholds: &[u32],
    labels: &[&str],
) -> MilestoneRow {
    let items = thresholds
        .iter()
        .zip(labels.iter())
        .map(|(&t, &l)| Milestone {
            label: l.to_string(),
            threshold: t,
            reached: current >= t,
        })
        .collect();
    MilestoneRow {
        category: category.to_string(),
        current,
        items,
    }
}

/// Returns (current_streak, longest_streak) in days, from per-day dictation
/// counts. Current streak ends today (if active) or yesterday.
fn compute_streaks(day_dictations: &HashMap<u64, u32>, today_start: u64) -> (u32, u32) {
    // Longest run over all active days.
    let mut longest: u32 = 0;
    if !day_dictations.is_empty() {
        let mut days: Vec<u64> = day_dictations.keys().copied().collect();
        days.sort();
        let mut run: u32 = 1;
        for i in 1..days.len() {
            if days[i] - days[i - 1] == DAY_MS {
                run += 1;
            } else {
                run = 1;
            }
            longest = longest.max(run);
        }
    }

    // Current streak.
    let mut current: u32 = 0;
    let mut cursor = today_start;
    if day_dictations.get(&cursor).copied().unwrap_or(0) > 0 {
        current += 1;
    }
    while day_dictations.get(&(cursor - DAY_MS)).copied().unwrap_or(0) > 0 {
        current += 1;
        cursor -= DAY_MS;
    }

    (current, longest)
}

/// Builds the last 26 weeks (182 days) of heatmap cells, oldest first.
fn build_heatmap(day_words: &HashMap<u64, u32>, today_start: u64) -> Vec<HeatCell> {
    let days = HEAT_WEEKS * 7;
    let start = today_start.saturating_sub((days - 1) * DAY_MS);
    // Intensity buckets by words: 0, 1-49, 50-149, 150-399, 400+.
    let level = |w: u32| -> u8 {
        if w == 0 {
            0
        } else if w < 50 {
            1
        } else if w < 150 {
            2
        } else if w < 400 {
            3
        } else {
            4
        }
    };
    (0..days)
        .map(|i| {
            let date = start + i * DAY_MS;
            let words = day_words.get(&date).copied().unwrap_or(0);
            HeatCell {
                date,
                words,
                level: level(words),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::DictationEntry;

    fn entry(at: u64, text: &str) -> DictationEntry {
        DictationEntry {
            id: at.to_string(),
            created_at: at,
            text: text.into(),
            context: None,
            duration_ms: None,
        }
    }

    /// A take with a real measured duration, for the speaking-rate figures.
    fn timed(at: u64, text: &str, duration_ms: u64) -> DictationEntry {
        DictationEntry {
            duration_ms: Some(duration_ms),
            ..entry(at, text)
        }
    }

    const NOW: u64 = 20_000 * DAY_MS;

    #[test]
    fn repeated_phrases_are_ranked() {
        let mut h = DictationHistory::default();
        h.push(entry(1, "please send the report today"));
        h.push(entry(2, "hey, please send the report when ready"));
        h.push(entry(3, "can you please send the report now"));
        let i = compute(&h, NOW);
        assert!(
            i.top_phrases
                .iter()
                .any(|p| p.label == "send the report" && p.count == 3),
            "{:?}",
            i.top_phrases
        );
        assert!(i.top_phrases[0].share <= 1.0);
    }

    #[test]
    fn single_occurrences_are_excluded() {
        let mut h = DictationHistory::default();
        h.push(entry(1, "one two three"));
        h.push(entry(2, "four five six"));
        let i = compute(&h, NOW);
        assert!(i.top_phrases.is_empty());
    }

    #[test]
    fn apps_and_hours_are_tracked() {
        let mut h = DictationHistory::default();
        let mut e = entry(3_600_000 * 9 + 1000, "hello world");
        e.context = Some(crate::context::normalize("com.google.gmail", "Gmail"));
        h.push(e);
        let i = compute(&h, NOW);
        assert_eq!(i.top_apps.len(), 1);
        assert_eq!(i.top_apps[0].label, "Gmail");
        assert_eq!(i.busiest_hour, Some(9));
        assert_eq!(i.vocabulary_size, 2);
    }

    #[test]
    fn apps_with_unknown_category_are_still_counted() {
        let mut h = DictationHistory::default();
        // An app not in the classification table gets AppType::Unknown, but it
        // should still appear in "where you dictate".
        for _ in 0..3 {
            let mut e = entry(1, "some words here");
            e.context = Some(crate::context::normalize("com.example.terax", "Terax"));
            h.push(e);
        }
        // A known app too, to confirm both are ranked together.
        let mut e = entry(2, "more words");
        e.context = Some(crate::context::normalize("com.google.gmail", "Gmail"));
        h.push(e);
        let i = compute(&h, NOW);
        assert_eq!(i.total_dictations, 4);
        let by_label: std::collections::HashMap<&str, u32> = i
            .top_apps
            .iter()
            .map(|a| (a.label.as_str(), a.count))
            .collect();
        assert_eq!(by_label.get("Terax"), Some(&3));
        assert_eq!(by_label.get("Gmail"), Some(&1));
        // Terax (3) ranks above Gmail (1).
        assert_eq!(i.top_apps[0].label, "Terax");
    }

    #[test]
    fn empty_history_is_empty() {
        let i = compute(&DictationHistory::default(), NOW);
        assert!(i.top_phrases.is_empty());
        assert!(i.top_apps.is_empty());
        assert_eq!(i.vocabulary_size, 0);
        assert_eq!(i.total_words, 0);
        assert!(i.impact.time_saved_minutes == 0);
        assert_eq!(i.heatmap.len(), (HEAT_WEEKS * 7) as usize);
        assert_eq!(i.daily.len(), 14);
        assert_eq!(i.words_today, 0);
        assert_eq!(i.words_last_7_days, 0);
    }

    #[test]
    fn daily_series_and_period_totals() {
        let mut h = DictationHistory::default();
        // Today: 10 words across 2 dictations.
        h.push(entry(NOW + 1000, "one two three four five"));
        h.push(entry(NOW + 2000, "six seven eight nine ten"));
        // Yesterday: 5 words.
        h.push(entry(NOW - DAY_MS, "a b c d e"));
        let i = compute(&h, NOW);
        assert_eq!(i.words_today, 10);
        assert_eq!(i.words_last_7_days, 15);
        assert_eq!(i.daily.len(), 14);
        let today = i.daily.last().unwrap();
        assert_eq!(today.words, 10);
        assert_eq!(today.dictations, 2);
        assert_eq!(i.avg_words_per_day, 15 / 2); // 15 words over 2 active days
    }

    /// Time saved, with no measured duration available: 1500 words is 38 min
    /// typed at the 40 wpm baseline and 10 min spoken at a documented 150 wpm
    /// central estimate, so about 28 min saved. The speaking figure is
    /// explicitly an estimate, which is why the rate itself reports `None`.
    #[test]
    fn time_saved_falls_back_to_a_documented_central_estimate() {
        let mut h = DictationHistory::default();
        h.push(entry(NOW, &"word ".repeat(1500)));
        let i = compute(&h, NOW);
        assert_eq!(i.total_words, 1500);
        assert_eq!(i.impact.words_per_minute, None, "no duration, no claim");
        assert_eq!(i.impact.times_faster, None, "no duration, no claim");
        assert_eq!(i.impact.minutes_typed, 38, "1500 words at 40 wpm");
        assert_eq!(i.impact.time_saved_minutes, 28);
        assert_eq!(i.impact.essays, 3);
    }

    /// The rate is measured, so it must track the duration, not the word count.
    #[test]
    fn the_speaking_rate_is_measured_not_derived() {
        // 300 words in 60 s is 300 wpm of speech...
        let mut slow = DictationHistory::default();
        slow.push(timed(NOW, &"word ".repeat(300), 120_000));
        assert_eq!(compute(&slow, NOW).impact.words_per_minute, Some(150));

        // ...and the same 300 words over 40 s is 450, which is recitation, so
        // it is clamped to a plausible ceiling rather than shown.
        let mut fast = DictationHistory::default();
        fast.push(timed(NOW, &"word ".repeat(300), 40_000));
        let i = compute(&fast, NOW);
        assert_eq!(i.impact.words_per_minute, Some(MAX_PLAUSIBLE_WPM));
    }

    /// The bug this replaced: dictating more must never reduce the saving.
    #[test]
    fn time_saved_is_monotonic_in_the_word_count() {
        let mut prev = 0u32;
        for words in [39u32, 100, 149, 150, 200, 290, 449, 1000, 5000] {
            let mut h = DictationHistory::default();
            h.push(timed(NOW, &"word ".repeat(words as usize), 60_000));
            let saved = compute(&h, NOW).impact.time_saved_minutes;
            assert!(
                saved >= prev,
                "time saved went DOWN from {prev} to {saved} at {words} words"
            );
            prev = saved;
        }
    }

    #[test]
    fn a_take_too_short_to_time_is_skipped_not_assumed() {
        // A two-word entry typed in 300 ms reads as 400 wpm, which is not a
        // thing a person does. It must be excluded, and the count reported.
        let mut h = DictationHistory::default();
        h.push(timed(NOW, "hello there", 300));
        let i = compute(&h, NOW);
        assert_eq!(i.impact.words_per_minute, None);
        assert_eq!(i.impact.rated_takes, 0);
        assert_eq!(i.impact.skipped_takes, 1);
    }

    #[test]
    fn entries_without_a_duration_are_counted_but_not_rated() {
        // History written before durations were recorded keeps contributing
        // words, and is reported as skipped so the rate is never presented as
        // covering more than it does.
        let mut h = DictationHistory::default();
        h.push(entry(NOW, "a b c d e f g"));
        h.push(entry(NOW - DAY_MS, "a b c d e f g"));
        h.push(timed(NOW - 2 * DAY_MS, "a b c d e f g", 10_000));
        let i = compute(&h, NOW);
        assert_eq!(i.total_words, 21);
        assert_eq!(i.impact.rated_takes, 1);
        assert_eq!(i.impact.skipped_takes, 2);
        assert!(i.impact.words_per_minute.is_some());
    }

    #[test]
    fn the_typing_baseline_is_40_wpm() {
        // 40 wpm is the widely-cited average the UI states and compares against.
        assert_eq!(TYPING_WPM, 40);
        // 1000 words at 40 wpm is 25 minutes.
        let mut h = DictationHistory::default();
        h.push(entry(NOW, &"word ".repeat(1000)));
        assert_eq!(compute(&h, NOW).impact.minutes_typed, 25);
    }

    #[test]
    fn records_capture_longest_and_daily_max() {
        let mut h = DictationHistory::default();
        h.push(entry(NOW, "a b c d e f g")); // 7 words today
        h.push(entry(NOW - DAY_MS, "one two three")); // 3 words yesterday
        h.push(entry(NOW - DAY_MS, "four five")); // 2 words, 2 dictations yesterday
        let i = compute(&h, NOW);
        assert_eq!(i.records[0].value, "7 words");
        assert_eq!(i.records[1].value, "7 words");
        assert_eq!(i.records[2].value, "2 dictations");
    }

    #[test]
    fn fastest_take_record_is_the_single_best_rate() {
        let mut h = DictationHistory::default();
        // 120 words in 60 s = 120 wpm; 60 words in 60 s = 60 wpm.
        h.push(timed(NOW, &"w ".repeat(120), 60_000));
        h.push(timed(NOW, &"w ".repeat(60), 60_000));
        let i = compute(&h, NOW);
        let rec = i
            .records
            .iter()
            .find(|r| r.label == "Fastest take")
            .expect("Fastest take record present");
        assert_eq!(rec.value, "120 wpm");
    }

    #[test]
    fn no_fastest_take_without_a_timed_entry() {
        let mut h = DictationHistory::default();
        h.push(entry(NOW, "a b c d e f"));
        let i = compute(&h, NOW);
        assert!(i.records.iter().all(|r| r.label != "Fastest take"));
    }

    #[test]
    fn streaks_current_and_longest() {
        let mut h = DictationHistory::default();
        // Today + yesterday + 2 days ago = 3-day current streak.
        h.push(entry(NOW, "x"));
        h.push(entry(NOW - DAY_MS, "x"));
        h.push(entry(NOW - 2 * DAY_MS, "x"));
        // An older 4-day run (longest).
        for d in 10..14 {
            h.push(entry(NOW - d * DAY_MS, "x"));
        }
        let i = compute(&h, NOW);
        assert_eq!(i.streak_days, 3);
        assert_eq!(i.longest_streak_days, 4);
    }

    #[test]
    fn milestones_track_progress() {
        let mut h = DictationHistory::default();
        let text = "word ".repeat(1200); // 1200 words
        h.push(entry(NOW, &text));
        h.push(entry(NOW, "a b"));
        let i = compute(&h, NOW);
        let words_row = i.milestones.iter().find(|r| r.category == "Words").unwrap();
        assert!(words_row.items[0].reached); // 1K
        assert!(!words_row.items[1].reached); // 10K
        let t_row = i
            .milestones
            .iter()
            .find(|r| r.category == "Transcriptions")
            .unwrap();
        assert!(!t_row.items[0].reached); // 50
    }

    #[test]
    fn heatmap_spans_182_days_with_levels() {
        let mut h = DictationHistory::default();
        let text = "word ".repeat(500); // 500 words today -> level 4
        h.push(entry(NOW, &text));
        let i = compute(&h, NOW);
        assert_eq!(i.heatmap.len(), 182);
        let today = i.heatmap.last().unwrap();
        assert_eq!(today.date, crate::stats::day_start_ms(NOW));
        assert_eq!(today.level, 4);
        assert_eq!(i.heatmap.first().unwrap().level, 0);
    }

    #[test]
    fn day_labels_use_correct_weekday() {
        // 2026-09-20 is a Sunday (20716 days since epoch).
        let sunday = 20_716 * DAY_MS;
        assert_eq!(day_label(sunday), "Sun 20 Sep");
        // 2026-09-21 is a Monday.
        assert_eq!(day_label(sunday + DAY_MS), "Mon 21 Sep");
        // 1970-01-01 was a Thursday.
        assert_eq!(day_label(0), "Thu 1 Jan");
    }

    #[test]
    fn a_window_scopes_totals_but_not_the_habit_views() {
        let mut h = DictationHistory::default();
        // Today: 10 words across 2 dictations.
        h.push(entry(NOW + 1000, "one two three four five"));
        h.push(entry(NOW + 2000, "six seven eight nine ten"));
        // Yesterday: 5 words.
        h.push(entry(NOW - DAY_MS, "a b c d e"));
        // 100 days ago: 4 words, inside the 26-week heatmap.
        h.push(entry(NOW - 100 * DAY_MS, "old words are here"));

        let today_start = crate::stats::day_start_ms(NOW);
        let windowed = compute_for_window(&h, Some((today_start, NOW + 3000)), NOW);
        let lifetime = compute_for_window(&h, None, NOW);

        // Range-scoped aggregates only see the window.
        assert_eq!(windowed.total_words, 10);
        assert_eq!(windowed.total_dictations, 2);
        assert_eq!(lifetime.total_words, 19);
        assert_eq!(lifetime.total_dictations, 4);

        // Habit views are anchored to now over the full history, so they
        // must not move when the selected window changes.
        assert_eq!(windowed.words_today, 10);
        assert_eq!(windowed.words_today, lifetime.words_today);
        assert_eq!(windowed.words_last_7_days, 15);
        assert_eq!(windowed.words_last_7_days, lifetime.words_last_7_days);
        assert_eq!(windowed.daily, lifetime.daily);
        assert_eq!(windowed.heatmap, lifetime.heatmap);
        assert_eq!(windowed.streak_days, 2);
        assert_eq!(windowed.streak_days, lifetime.streak_days);
        assert_eq!(windowed.longest_streak_days, lifetime.longest_streak_days);
        assert_eq!(windowed.avg_words_per_day, lifetime.avg_words_per_day);
        assert_eq!(windowed.first_activity_at, Some(NOW - 100 * DAY_MS));
        assert_eq!(windowed.first_activity_at, lifetime.first_activity_at);
    }

    #[test]
    fn records_and_milestones_are_all_time_even_inside_a_window() {
        let mut h = DictationHistory::default();
        h.push(entry(NOW - 100 * DAY_MS, &"word ".repeat(10_000)));
        h.push(entry(NOW, "short take"));

        let today_start = crate::stats::day_start_ms(NOW);
        let i = compute_for_window(&h, Some((today_start, NOW + 1000)), NOW);
        assert_eq!(i.total_words, 2, "the windowed total only sees today");

        let words = i.milestones.iter().find(|r| r.category == "Words").unwrap();
        assert_eq!(words.current, 10_002, "milestones count the whole history");
        assert!(words.items[1].reached, "10K is a lifetime milestone");
        let transcriptions = i
            .milestones
            .iter()
            .find(|r| r.category == "Transcriptions")
            .unwrap();
        assert_eq!(transcriptions.current, 2);
        assert_eq!(
            i.records[0].value, "10000 words",
            "longest dictation is a lifetime record"
        );
        assert_eq!(
            i.records[1].value, "10000 words",
            "most words in a day is a lifetime record"
        );
        assert_eq!(i.records[2].value, "1 dictations");
    }

    /// Lifetime now computes over every stored dictation instead of the
    /// 1000-entry snapshot cap. The old subsumption sweep was quadratic
    /// (measured at 64 s for 1000 entries), which made that impossible; this
    /// guards the linear bound with a generous ceiling.
    #[test]
    fn a_thousand_entries_compute_without_a_quadratic_sweep() {
        let mut h = DictationHistory::default();
        for i in 0..1000u64 {
            let text = format!(
                "alpha{i} beta{i} gamma{i} delta{i} epsilon{i} zeta{i} eta{i} \
                 theta{i} iota{i} kappa{i} lambda{i} mu{i} nu{i} xi{i} omicron{i} \
                 pi{i} rho{i} sigma{i} tau{i} upsilon{i} phi{i} chi{i} psi{i} \
                 omega{i} word{i}a word{i}b word{i}c word{i}d word{i}e word{i}f \
                 word{i}g word{i}h"
            );
            h.push(entry(NOW - i * 1000, &text));
        }
        let start = std::time::Instant::now();
        let i = compute(&h, NOW);
        let elapsed = start.elapsed();
        assert_eq!(i.total_dictations, 1000);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "compute over 1000 entries took {elapsed:?}; the phrase sweep is quadratic again"
        );
    }

    #[test]
    fn phrase_subsumption_respects_word_boundaries() {
        // "the report" is covered by "send the report" at the same count, so
        // the longer phrase wins.
        let mut covered = DictationHistory::default();
        for _ in 0..3 {
            covered.push(entry(1, "please send the report today"));
        }
        let i = compute(&covered, NOW);
        assert!(
            i.top_phrases
                .iter()
                .any(|p| p.label == "send the report" && p.count == 3),
            "{:?}",
            i.top_phrases
        );
        assert!(!i.top_phrases.iter().any(|p| p.label == "the report"));

        // A 2-gram that appears more often than any 3-gram containing it is
        // still a phrase in its own right.
        let mut independent = DictationHistory::default();
        independent.push(entry(1, "send the report now"));
        independent.push(entry(2, "send the report please"));
        independent.push(entry(3, "the report is late"));
        let i = compute(&independent, NOW);
        assert!(
            i.top_phrases
                .iter()
                .any(|p| p.label == "the report" && p.count == 3),
            "{:?}",
            i.top_phrases
        );
    }

    // ── Top days, hour bands, top words, habit words ────────────────────

    #[test]
    fn top_days_and_best_day_are_computed() {
        let mut h = DictationHistory::default();
        // NOW is 20_000 * DAY_MS. weekday_index(20_000) = (3 + 20_000) % 7 = 20_003 % 7 = 4 (Friday).
        // Day 19_999: (3 + 19_999) % 7 = 20_002 % 7 = 3 (Thursday).
        // 10 words today (Fri), 5 words yesterday (Thu).
        h.push(entry(NOW, "one two three four five"));
        h.push(entry(NOW, "six seven eight nine ten"));
        h.push(entry(NOW - DAY_MS, "a b c d e"));
        let i = compute(&h, NOW);
        // Should have 2 entries (Fri and Thu), no zero-count days.
        assert_eq!(i.top_days.len(), 2, "{:?}", i.top_days);
        // Friday (10 words) should be the best day.
        assert_eq!(i.best_day.as_deref(), Some("Friday"), "{:?}", i.top_days);
        // Share of the busiest day is 1.0.
        assert!(i
            .top_days
            .iter()
            .any(|d| d.label == "Friday" && d.share == 1.0));
    }

    #[test]
    fn top_days_empty_when_no_data() {
        let i = compute(&DictationHistory::default(), NOW);
        assert!(i.top_days.is_empty());
        assert!(i.best_day.is_none());
    }

    #[test]
    fn hour_bands_have_five_entries() {
        let mut h = DictationHistory::default();
        // Entry at hour 9 (UTC) -> Morning band.
        h.push(entry(9 * 3_600_000 + 1000, "hello world"));
        let i = compute(&h, NOW);
        assert_eq!(i.hour_bands.len(), 5, "always 5 bands");
        // Morning band should have the count.
        let morning = i.hour_bands.iter().find(|b| b.label == "Morning").unwrap();
        assert_eq!(morning.count, 2, "2 words in morning");
        // All other bands are zero.
        assert!(i.hour_bands.iter().filter(|b| b.count > 0).count() == 1);
    }

    #[test]
    fn top_words_excludes_stopwords_and_short_tokens() {
        let mut h = DictationHistory::default();
        // "the" and "a" are stopwords; "hi" is too short (< 3 chars).
        h.push(entry(1, "the quick brown fox jumps over the lazy dog"));
        h.push(entry(2, "the quick brown fox jumps again"));
        let i = compute(&h, NOW);
        // "the" must not appear.
        assert!(
            !i.top_words.iter().any(|w| w.label == "the"),
            "stopword leaked: {:?}",
            i.top_words
        );
        // "quick" and "brown" and "fox" should appear.
        assert!(i
            .top_words
            .iter()
            .any(|w| w.label == "quick" && w.count == 2));
        assert!(i.top_words.iter().any(|w| w.label == "fox" && w.count == 2));
    }

    #[test]
    fn habit_words_count_hard_and_soft_tiers() {
        let mut h = DictationHistory::default();
        h.push(entry(
            1,
            "this is so annoying and i hate this stupid meeting",
        ));
        h.push(entry(2, "what a shit situation really"));
        let i = compute(&h, NOW);
        // "annoying" (1) + "hate" (1) + "stupid" (1) + "shit" (1) = 4 total.
        assert_eq!(i.habit_words.total, 4, "{:?}", i.habit_words);
        assert!(i.habit_words.items.iter().any(|w| w.label == "annoying"));
        assert!(i.habit_words.items.iter().any(|w| w.label == "hate"));
        assert!(i.habit_words.items.iter().any(|w| w.label == "stupid"));
        assert!(i.habit_words.items.iter().any(|w| w.label == "shit"));
        // per_1k: 4 habit / 18 total words * 1000 ≈ 222.2
        assert!(
            i.habit_words.per_1k_words > 200.0,
            "got {}",
            i.habit_words.per_1k_words
        );
        // Lifetime: trend is 0 (no prior window).
        assert_eq!(i.habit_words.trend, 0);
    }

    #[test]
    fn habit_words_zero_when_no_habit_words() {
        let mut h = DictationHistory::default();
        h.push(entry(1, "the quick brown fox jumps over the lazy dog"));
        let i = compute(&h, NOW);
        assert_eq!(i.habit_words.total, 0);
        assert_eq!(i.habit_words.per_1k_words, 0.0);
        assert!(i.habit_words.items.is_empty());
    }

    #[test]
    fn habit_words_trend_increasing_when_current_higher() {
        // Window: last 2 days. Prior window: 2 days before that.
        let from = NOW - 2 * DAY_MS;
        let to = NOW;
        let mut h = DictationHistory::default();
        // Prior window (NOW-4*DAY to NOW-2*DAY, exclusive): 10 words, 1 habit word.
        h.push(entry(
            NOW - 4 * DAY_MS,
            "this is a very annoying meeting with many people here today",
        ));
        // Current window (NOW-2*DAY to NOW): 15 words, 8 habit words.
        h.push(entry(
            NOW - DAY_MS,
            "hate this stupid annoying ridiculous shit meeting",
        ));
        h.push(entry(NOW, "i hate this annoying stupid situation so much"));
        let i = compute_for_window(&h, Some((from, to)), NOW);
        // Current: 8 habit / 15 words * 1000 = 533.3 per 1k.
        // Prior: 1 habit / 10 words * 1000 = 100 per 1k.
        // 533 > 100 + 0.5 => trend = 1 (increasing).
        assert_eq!(i.habit_words.trend, 1, "{:?}", i.habit_words);
        assert_eq!(i.habit_words.total, 8);
    }

    #[test]
    fn habit_words_trend_decreasing_when_current_lower() {
        let from = NOW - 2 * DAY_MS;
        let to = NOW;
        let mut h = DictationHistory::default();
        // Prior window: 10 words, 5 habit words.
        h.push(entry(
            NOW - 4 * DAY_MS,
            "hate this stupid annoying ridiculous shit meeting",
        ));
        h.push(entry(
            NOW - 3 * DAY_MS,
            "i hate this annoying stupid situation so much",
        ));
        // Current window: 10 words, 1 habit word.
        h.push(entry(
            NOW - DAY_MS,
            "this is a very nice meeting with many people here today",
        ));
        let i = compute_for_window(&h, Some((from, to)), NOW);
        // Current: 0 habit / 9 words = 0 per 1k ("nice" is not a habit word).
        // Prior: 9 habit / 18 words * 1000 = 500 per 1k.
        // 0 < 500 - 0.5 => trend = -1 (decreasing).
        assert_eq!(i.habit_words.trend, -1, "{:?}", i.habit_words);
    }

    #[test]
    fn hour_band_boundaries() {
        // 0-5 -> Early morning (0)
        assert_eq!(hour_band(0), 0);
        assert_eq!(hour_band(5), 0);
        // 6-11 -> Morning (1)
        assert_eq!(hour_band(6), 1);
        assert_eq!(hour_band(11), 1);
        // 12-13 -> Midday (2)
        assert_eq!(hour_band(12), 2);
        assert_eq!(hour_band(13), 2);
        // 14-17 -> Afternoon (3)
        assert_eq!(hour_band(14), 3);
        assert_eq!(hour_band(17), 3);
        // 18-23 -> Evening (4)
        assert_eq!(hour_band(18), 4);
        assert_eq!(hour_band(23), 4);
    }

    // ── App categories, active days, prior window, ranked_from_counts ──────

    #[test]
    fn app_categories_counts_by_category_and_maps_unknown_to_other() {
        let mut h = DictationHistory::default();
        // Two Email, one Chat, one with no context (should not contribute),
        // one Unknown app (maps to "Other").
        let mut e = entry(1, "hello world");
        e.context = Some(crate::context::normalize("com.google.gmail", "Gmail"));
        h.push(e);
        let mut e = entry(2, "another email");
        e.context = Some(crate::context::normalize("outlook", "Outlook"));
        h.push(e);
        let mut e = entry(3, "a chat message");
        e.context = Some(crate::context::normalize("slack", "Slack"));
        h.push(e);
        // No context: does not contribute.
        h.push(entry(4, "no context here"));
        // Unknown app → "Other".
        let mut e = entry(5, "some words");
        e.context = Some(crate::context::normalize("com.example.terax", "Terax"));
        h.push(e);

        let i = compute(&h, NOW);
        let by_label: std::collections::HashMap<&str, u32> = i
            .app_categories
            .iter()
            .map(|a| (a.label.as_str(), a.count))
            .collect();
        assert_eq!(by_label.get("Email"), Some(&2));
        assert_eq!(by_label.get("Chat"), Some(&1));
        assert_eq!(by_label.get("Other"), Some(&1));
        // Share normalized to top (Email = 2):
        let email = i
            .app_categories
            .iter()
            .find(|a| a.label == "Email")
            .unwrap();
        assert_eq!(email.share, 1.0);
        let chat = i.app_categories.iter().find(|a| a.label == "Chat").unwrap();
        assert!((chat.share - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn active_days_last_7_counts_distinct_days_and_ignores_older() {
        let mut h = DictationHistory::default();
        // 3 distinct days within the last 7.
        h.push(entry(NOW, "x"));
        h.push(entry(NOW - 2 * DAY_MS, "x"));
        h.push(entry(NOW - 5 * DAY_MS, "x"));
        // An entry 10 days ago: outside the 7-day window.
        h.push(entry(NOW - 10 * DAY_MS, "x"));
        let i = compute(&h, NOW);
        assert_eq!(i.active_days_last_7, 3);
    }

    #[test]
    fn prior_window_words_and_change_pct_for_week_window() {
        let from = NOW - 7 * DAY_MS;
        let to = NOW;
        let mut h = DictationHistory::default();
        // Current window: 10 words.
        h.push(entry(NOW - DAY_MS, "one two three four five"));
        h.push(entry(NOW, "six seven eight nine ten"));
        // Prior window (NOW-14*DAY to NOW-7*DAY): 20 words.
        h.push(entry(NOW - 8 * DAY_MS, &"word ".repeat(10)));
        h.push(entry(NOW - 10 * DAY_MS, &"word ".repeat(10)));

        let i = compute_for_window(&h, Some((from, to)), NOW);
        assert_eq!(i.prior_window_words, Some(20));
        // (10 - 20) / 20 * 100 = -50.0
        assert_eq!(i.words_change_pct, Some(-50.0));

        // Lifetime: both None.
        let lifetime = compute(&h, NOW);
        assert_eq!(lifetime.prior_window_words, None);
        assert_eq!(lifetime.words_change_pct, None);
    }

    #[test]
    fn prior_window_words_none_when_prior_is_zero() {
        let from = NOW - 7 * DAY_MS;
        let to = NOW;
        let mut h = DictationHistory::default();
        // Current window has words, prior window is empty.
        h.push(entry(NOW, "hello world there"));
        let i = compute_for_window(&h, Some((from, to)), NOW);
        assert_eq!(i.prior_window_words, Some(0));
        assert_eq!(i.words_change_pct, None, "0 prior → None");
    }

    #[test]
    fn ranked_from_counts_orders_truncates_and_drops_zeros() {
        let mut counts = std::collections::BTreeMap::new();
        counts.insert("zero".into(), 0u32);
        counts.insert("alpha".into(), 5);
        counts.insert("beta".into(), 3);
        counts.insert("gamma".into(), 3);
        counts.insert("delta".into(), 1);
        // 6 more to exceed MAX_ITEMS (8).
        for (k, v) in [
            ("e1", 1),
            ("e2", 1),
            ("e3", 1),
            ("e4", 1),
            ("e5", 1),
            ("e6", 1),
        ] {
            counts.insert(k.into(), v);
        }
        let ranked = ranked_from_counts(&counts);
        // Zero dropped; truncated to 8.
        assert_eq!(ranked.len(), 8);
        assert!(!ranked.iter().any(|r| r.label == "zero"));
        // Top is alpha (5), share 1.0.
        assert_eq!(ranked[0].label, "alpha");
        assert_eq!(ranked[0].count, 5);
        assert_eq!(ranked[0].share, 1.0);
        // Tie at 3: beta before gamma (label asc).
        assert_eq!(ranked[1].label, "beta");
        assert_eq!(ranked[2].label, "gamma");
        // Share = count / top.
        assert!((ranked[1].share - 3.0 / 5.0).abs() < f32::EPSILON);
    }

    // ── Learning (P7.8) ────────────────────────────────────────────────────

    use crate::learning::store::{DictationRow, LexiconRow, PersonalAccuracyRow};

    fn ldictation(ts: u64, pasted: &str, observed: Option<&str>) -> DictationRow {
        DictationRow {
            id: ts as i64,
            ts,
            app_id: "slack".into(),
            app_category: "chat".into(),
            engine: "whisper".into(),
            lang: "en".into(),
            audio_path: None,
            raw_asr: pasted.into(),
            words_json: "[]".into(),
            pasted: pasted.into(),
            observed: observed.map(String::from),
            edit_ops_json: None,
            polish_model: None,
            polish_skipped: true,
        }
    }

    fn llexicon(term: &str, source: &str, created: u64, tombstoned: bool) -> LexiconRow {
        LexiconRow {
            term: term.into(),
            aliases: vec!["alias".into()],
            source: source.into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created,
            last_used: None,
            tombstoned,
            app_scope: None,
            learned_from: None,
        }
    }

    fn laccuracy(ts: u64, before: f64, after: f64) -> PersonalAccuracyRow {
        PersonalAccuracyRow {
            ts,
            wer_before: before,
            wer_after: after,
            clips: 1,
        }
    }

    #[test]
    fn compute_learning_empty_inputs_are_all_defaults() {
        let s = compute_learning(&[], &[], &[], &[], NOW);
        assert_eq!(s, LearningSummary::default());
        // `compute` leaves the learning section at its default too, so a UI
        // with no store still sees a valid (empty) payload.
        let i = compute(&DictationHistory::default(), NOW);
        assert_eq!(i.learning, LearningSummary::default());
    }

    #[test]
    fn compute_learning_rate_and_7d_boundary() {
        // A row 8 days old: inside the 30-day daily window, outside the
        // 7-day edit-rate window.
        let old = NOW - 8 * DAY_MS;
        // A row 6 days old: inside the 7-day window. 4 words, 1 changed
        // ("pie torch" → "PyTorch").
        let recent = NOW - 6 * DAY_MS;
        let rows = vec![
            ldictation(old, "a b c d", Some("e f g h")),
            ldictation(recent, "call pie torch today", Some("call PyTorch today")),
        ];
        let s = compute_learning(&rows, &[], &[], &[], NOW);
        // 1/4 words changed -> 25 per 100; the old row is excluded.
        assert_eq!(s.edit_rate_7d, Some(25.0));
        // Both days appear in the daily series, oldest first.
        assert_eq!(s.daily.len(), 2);
        assert_eq!(s.daily[0].day, crate::stats::day_start_ms(old));
        assert_eq!(s.daily[0].words, 4);
        assert_eq!(s.daily[0].edited_words, 4);
        assert_eq!(s.daily[1].day, crate::stats::day_start_ms(recent));
        assert_eq!(s.daily[1].words, 4);
        assert_eq!(s.daily[1].edited_words, 1);
        // No words at all in the 7-day window -> None, not 0.
        let s2 = compute_learning(&[ldictation(old, "x y", Some("z w"))], &[], &[], &[], NOW);
        assert_eq!(s2.edit_rate_7d, None);
    }

    #[test]
    fn compute_learning_edited_words_count_to_side_only() {
        // Replace: 1 to-side word. Insert: 1. Delete: 0 to-side words.
        let rows = vec![
            ldictation(NOW, "call pie torch", Some("call PyTorch")),
            ldictation(NOW + 1000, "hi there", Some("hi there mate")),
            ldictation(NOW + 2000, "hi there mate", Some("hi there")),
        ];
        let s = compute_learning(&rows, &[], &[], &[], NOW);
        // 2 to-side words over 8 dictated (pasted-side) words: 3 + 2 + 3.
        // (The rate divides by the words the user dictated, i.e. the pasted
        // side; the test author originally miscounted the denominator as 9.)
        assert_eq!(s.edit_rate_7d, Some(100.0 * 2.0 / 8.0));
        // Rows without an observed text are not edit-relevant.
        let s2 = compute_learning(&[ldictation(NOW, "a b", None)], &[], &[], &[], NOW);
        assert_eq!(s2.edit_rate_7d, Some(0.0));
    }

    #[test]
    fn compute_learning_top_fixes_order_tiebreak_and_cap() {
        let mut rows = Vec::new();
        // 3x "pie torch" -> "PyTorch" (newest), 3x "ju pyter" -> "Jupyter"
        // (tie, lex order puts Jupyter first), 2x "dockr" -> "Docker".
        for i in 0..2 {
            rows.push(ldictation(
                NOW - i * 1000,
                "call dockr up",
                Some("call Docker up"),
            ));
        }
        for i in 2..5 {
            rows.push(ldictation(
                NOW - i * 1000,
                "run ju pyter",
                Some("run Jupyter"),
            ));
        }
        for i in 5..8 {
            rows.push(ldictation(
                NOW - i * 1000,
                "call pie torch",
                Some("call PyTorch"),
            ));
        }
        // 600 more recent rows that are not mishearings: the 500-row cap
        // keeps the 8 signal rows (they are the newest 8 of the 608).
        for i in 0..600 {
            rows.push(ldictation(NOW - 9000 - i * 1000, "x y z", Some("w v u")));
        }
        let s = compute_learning(&rows, &[], &[], &[], NOW);
        assert_eq!(s.top_fixes.len(), 3);
        assert_eq!(s.top_fixes[0].to, "Jupyter");
        assert_eq!(s.top_fixes[0].from, "ju pyter");
        assert_eq!(s.top_fixes[0].count, 3);
        assert_eq!(s.top_fixes[1].to, "PyTorch");
        assert_eq!(s.top_fixes[1].count, 3);
        assert_eq!(s.top_fixes[2].to, "Docker");
        assert_eq!(s.top_fixes[2].count, 2);

        // The cap itself: only the 500 most recent rows are classified.
        // Row 501 (index 500) is a mishearing; it must not be counted.
        let mut capped = Vec::new();
        for i in 0..500 {
            capped.push(ldictation(NOW - i * 1000, "x y z", Some("w v u")));
        }
        capped.push(ldictation(
            NOW - 500_000,
            "call pie torch",
            Some("call PyTorch"),
        ));
        let s2 = compute_learning(&capped, &[], &[], &[], NOW);
        assert!(
            s2.top_fixes.is_empty(),
            "row beyond the 500 cap was classified"
        );
        // ...and the same row at index 499 (inside the cap) is counted.
        let mut inside = Vec::new();
        for i in 0..499 {
            inside.push(ldictation(NOW - i * 1000, "x y z", Some("w v u")));
        }
        inside.push(ldictation(
            NOW - 499_000,
            "call pie torch",
            Some("call PyTorch"),
        ));
        let s3 = compute_learning(&inside, &[], &[], &[], NOW);
        assert!(s3.top_fixes.iter().any(|f| f.to == "PyTorch"));
    }

    #[test]
    fn compute_learning_terms_week_filters_and_orders() {
        let lexicon = vec![
            llexicon("old-term", "learned", NOW - 8 * DAY_MS, false),
            llexicon("manual-term", "manual", NOW - DAY_MS, false),
            llexicon("undone-term", "learned", NOW - DAY_MS, true),
            llexicon("b-term", "learned", NOW - 2 * DAY_MS, false),
            llexicon("a-term", "learned", NOW - 2 * DAY_MS, false),
            llexicon("fresh-term", "learned", NOW - DAY_MS, false),
        ];
        let s = compute_learning(&[], &[], &lexicon, &[], NOW);
        // Newest first; same ts breaks by term; old/manual/tombstoned out.
        assert_eq!(
            s.terms_learned_week
                .iter()
                .map(|t| t.term.as_str())
                .collect::<Vec<_>>(),
            vec!["fresh-term", "a-term", "b-term"]
        );
        assert_eq!(s.terms_learned_week[0].learned_at, NOW - DAY_MS);
    }

    #[test]
    fn compute_learning_accuracy_oldest_first_capped_at_10() {
        // 12 points, newest first (as the store returns them).
        let accuracy: Vec<PersonalAccuracyRow> = (0..12)
            .map(|i| laccuracy(NOW - (11 - i as u64) * DAY_MS, 0.5, 0.4))
            .collect();
        let s = compute_learning(&[], &accuracy, &[], &[], NOW);
        assert_eq!(s.accuracy.len(), 10);
        assert_eq!(s.accuracy[0].ts, NOW - 11 * DAY_MS, "oldest first");
        assert_eq!(s.accuracy[9].ts, NOW - 2 * DAY_MS);
    }

    #[test]
    fn compute_learning_is_deterministic() {
        let rows = vec![
            ldictation(NOW, "call pie torch", Some("call PyTorch")),
            ldictation(NOW - DAY_MS, "run ju pyter", Some("run Jupyter")),
        ];
        let lexicon = vec![llexicon("a-term", "learned", NOW, false)];
        let accuracy = vec![laccuracy(NOW, 0.5, 0.4)];
        let a = compute_learning(&rows, &accuracy, &lexicon, &[], NOW);
        let b = compute_learning(&rows, &accuracy, &lexicon, &[], NOW);
        assert_eq!(a, b);
    }
}
