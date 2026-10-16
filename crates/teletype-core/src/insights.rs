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
    /// Average words per dictation.
    pub avg_words_per_dictation: u32,
    /// The hour of day (UTC, 0-23) with the most dictations.
    pub busiest_hour: Option<u32>,
    /// Total distinct words used (vocabulary size).
    pub vocabulary_size: u32,
    /// Words in the personal dictionary (taught + auto-learned). Shown in
    /// Insights so the self-learning loop is visible: the number climbs as
    /// the app learns from the user's edits.
    pub dictionary_words: u32,
    /// How many of those were learned automatically from the user's edits
    /// (the rest were taught by hand or seeded as built-ins).
    pub learned_words: u32,
    /// Total words dictated (all time).
    pub total_words: u32,
    /// Total dictations (all time).
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
    /// Filler words removed within the requested range, recomputed from the
    /// range-filtered history (the persisted all-time counters carry no
    /// timestamps and cannot be sliced).
    pub filler_removed: u32,
    /// AutoText expansions (typed triggers + spoken snippets) within the
    /// requested range, recomputed the same way.
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
}

const NGRAM_SIZES: [usize; 2] = [2, 3];
const MAX_ITEMS: usize = 8;
const HEAT_WEEKS: u64 = 26;
const DAY_MS: u64 = 86_400_000;

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

    LearningSummary {
        edit_rate_7d,
        daily,
        terms_learned_week,
        top_fixes,
        accuracy,
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
/// heatmap to "today".
pub fn compute(history: &DictationHistory, now_ms: u64) -> Insights {
    let today_start = crate::stats::day_start_ms(now_ms);

    let mut ngram_counts: HashMap<(usize, String), u32> = HashMap::new();
    let mut app_counts: HashMap<String, u32> = HashMap::new();
    let mut hour_counts: [u32; 24] = [0; 24];
    let mut total_words: u32 = 0;
    let mut dictations: u32 = 0;
    let mut vocab: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut longest_words: u32 = 0;

    // Per-day aggregates for records, streak, and heatmap.
    let mut day_words: HashMap<u64, u32> = HashMap::new();
    let mut day_dictations: HashMap<u64, u32> = HashMap::new();
    // Measured speaking time, accumulated per take. Word count is taken from
    // the *final* text, so a polished dictation is counted at its delivered
    // length.
    let mut rate = RateSample::default();

    for entry in &history.entries {
        dictations += 1;
        let words: Vec<&str> = entry.text.split_whitespace().collect();
        let wc = words.len() as u32;
        total_words += wc;
        longest_words = longest_words.max(wc);
        rate.add(wc, entry.duration_ms);

        let day = crate::stats::day_start_ms(entry.created_at);
        *day_words.entry(day).or_insert(0) += wc;
        *day_dictations.entry(day).or_insert(0) += 1;

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
        }
        let secs = entry.created_at / 1000;
        hour_counts[(secs % 86_400) as usize / 3600] += 1;
    }

    // Top phrases (drop 2-grams subsumed by an equal-count 3-gram).
    let subsumed: Vec<(usize, String)> = ngram_counts
        .iter()
        .filter(|((n, phrase), count)| {
            *n == 2
                && ngram_counts.iter().any(|((m, p), c)| {
                    *m == 3 && p.len() >= 5 && p.contains(phrase.as_str()) && *c == **count
                })
        })
        .map(|((n, phrase), _)| (*n, phrase.clone()))
        .collect();
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

    let busiest_hour = hour_counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .max_by_key(|(_, c)| **c)
        .map(|(h, _)| h as u32);

    // Personal records.
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

    // Milestones.
    let milestones = vec![
        milestone_row(
            "Words",
            total_words,
            &[1_000, 10_000, 50_000, 100_000, 500_000, 1_000_000],
            &["1K", "10K", "50K", "100K", "500K", "1M"],
        ),
        milestone_row(
            "Transcriptions",
            dictations,
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

    // Heatmap: last 12 weeks ending this week, oldest first.
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
    for entry in &history.entries {
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
    let avg_words_per_day = words_last_7 / active_days.max(1);

    Insights {
        top_phrases: phrases,
        top_apps: apps,
        avg_words_per_dictation: total_words / dictations.max(1),
        busiest_hour,
        vocabulary_size: vocab.len() as u32,
        dictionary_words: 0,
        learned_words: 0,
        total_words,
        total_dictations: dictations,
        streak_days,
        longest_streak_days,
        impact: compute_impact(total_words, rate),
        records,
        milestones,
        heatmap,
        daily,
        words_today,
        words_last_7_days: words_last_7,
        avg_words_per_day,
        first_activity_at: None,
        filler_removed: 0,
        autotext_expansions: 0,
        personalization_corrections: 0,
        dictionary_learned: 0,
        polish_status: None,
        learning: LearningSummary::default(),
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

/// Builds the last 12 weeks (84 days) of heatmap cells, oldest first.
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
        let s = compute_learning(&[], &[], &[], NOW);
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
        let s = compute_learning(&rows, &[], &[], NOW);
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
        let s2 = compute_learning(&[ldictation(old, "x y", Some("z w"))], &[], &[], NOW);
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
        let s = compute_learning(&rows, &[], &[], NOW);
        // 2 to-side words over 8 dictated (pasted-side) words: 3 + 2 + 3.
        // (The rate divides by the words the user dictated, i.e. the pasted
        // side; the test author originally miscounted the denominator as 9.)
        assert_eq!(s.edit_rate_7d, Some(100.0 * 2.0 / 8.0));
        // Rows without an observed text are not edit-relevant.
        let s2 = compute_learning(&[ldictation(NOW, "a b", None)], &[], &[], NOW);
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
        let s = compute_learning(&rows, &[], &[], NOW);
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
        let s2 = compute_learning(&capped, &[], &[], NOW);
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
        let s3 = compute_learning(&inside, &[], &[], NOW);
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
        let s = compute_learning(&[], &[], &lexicon, NOW);
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
        let s = compute_learning(&[], &accuracy, &[], NOW);
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
        let a = compute_learning(&rows, &accuracy, &lexicon, NOW);
        let b = compute_learning(&rows, &accuracy, &lexicon, NOW);
        assert_eq!(a, b);
    }
}
