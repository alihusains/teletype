//! Usage insights: impact metrics, speaking speed, personal records,
//! milestones, and a contribution heatmap — all computed locally from the
//! dictation history.

use std::collections::HashMap;

use crate::history::DictationHistory;

/// Baseline typing speed used to compute "time saved" (words per minute).
/// A common average touch-typing rate.
pub const TYPING_WPM: u32 = 40;
/// Conservative speaking rate used to estimate how long the speech took.
const SPEAKING_WPM: u32 = 150;

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
    /// Estimated words per minute while speaking (word count / speaking time).
    pub words_per_minute: u32,
    /// How many times faster speaking is than typing at [`TYPING_WPM`].
    pub times_faster: f32,
    /// Total time (minutes) saved vs typing the same words at [`TYPING_WPM`].
    pub time_saved_minutes: u32,
    /// Human-readable time saved, e.g. "2 hr 15 min" or "45 min".
    pub time_saved_label: String,
    /// Minutes spent speaking (estimated from word count).
    pub minutes_spoken: u32,
    /// Minutes it would have taken to type the same words.
    pub minutes_typed: u32,
    /// "You've written N college essays!" style flourish.
    pub essays: u32,
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
    /// Contribution heatmap, oldest first (12 weeks, 84 days).
    pub heatmap: Vec<HeatCell>,
    /// Daily activity for the last 14 days (oldest first).
    pub daily: Vec<DayStat>,
    /// Words dictated today.
    pub words_today: u32,
    /// Words dictated in the last 7 days.
    pub words_last_7_days: u32,
    /// Average words per active day over the last 7 days.
    pub avg_words_per_day: u32,
    /// Status message for the polish/transform state, shown in the Insights
    /// screen. `None` when a model is loaded and transforms are working.
    pub polish_status: Option<String>,
}

const NGRAM_SIZES: [usize; 2] = [2, 3];
const MAX_ITEMS: usize = 8;
const HEAT_WEEKS: u64 = 12;
const DAY_MS: u64 = 86_400_000;

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

/// "Mon 16" style label from a UTC day-start timestamp.
fn day_label(day_ms: u64) -> String {
    let days = day_ms / DAY_MS;
    // 1970-01-01 was a Thursday (index 3 in the Monday-first array).
    let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(3 + days as usize) % 7];
    format!("{weekday} {}", month_day_label(day_ms))
}

/// Computes the impact of dictating vs typing.
fn compute_impact(total_words: u32) -> Impact {
    let minutes_spoken = total_words / SPEAKING_WPM;
    let minutes_typed = total_words / TYPING_WPM;
    let time_saved = minutes_typed.saturating_sub(minutes_spoken);
    let wpm = if minutes_spoken > 0 {
        total_words / minutes_spoken
    } else if total_words > 0 {
        SPEAKING_WPM
    } else {
        0
    };
    let times_faster = if wpm > 0 {
        (wpm as f32 / TYPING_WPM as f32).round() * 10.0 / 10.0
    } else {
        0.0
    };
    Impact {
        words_per_minute: wpm,
        times_faster,
        time_saved_minutes: time_saved,
        time_saved_label: format_minutes(time_saved),
        minutes_spoken,
        minutes_typed,
        essays: total_words / 500,
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

    for entry in &history.entries {
        dictations += 1;
        let words: Vec<&str> = entry.text.split_whitespace().collect();
        let wc = words.len() as u32;
        total_words += wc;
        longest_words = longest_words.max(wc);

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
    let records = vec![
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
        total_words,
        total_dictations: dictations,
        streak_days,
        longest_streak_days,
        impact: compute_impact(total_words),
        records,
        milestones,
        heatmap,
        daily,
        words_today,
        words_last_7_days: words_last_7,
        avg_words_per_day,
        polish_status: None,
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

    #[test]
    fn impact_scales_with_words() {
        let mut h = DictationHistory::default();
        // 1500 words = 10 min spoken, 37 min typed at 40wpm -> 27 min saved.
        let text = "word ".repeat(1500);
        h.push(entry(NOW, &text));
        let i = compute(&h, NOW);
        assert_eq!(i.total_words, 1500);
        assert_eq!(i.impact.words_per_minute, 150);
        assert_eq!(i.impact.time_saved_minutes, 27);
        assert!(i.impact.times_faster >= 3.0);
        assert_eq!(i.impact.essays, 3);
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
    fn heatmap_spans_84_days_with_levels() {
        let mut h = DictationHistory::default();
        let text = "word ".repeat(500); // 500 words today -> level 4
        h.push(entry(NOW, &text));
        let i = compute(&h, NOW);
        assert_eq!(i.heatmap.len(), 84);
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
}
