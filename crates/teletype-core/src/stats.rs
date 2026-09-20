//! Usage statistics derived from the dictation history.
//!
//! Pure functions over [`crate::history::DictationHistory`] so the Dashboard
//! and Insights screens are deterministic and testable.

use crate::history::DictationHistory;

/// One day in the activity chart. `label` is a short display name (e.g.
/// "Mon 16"); `date` is the UTC day start in ms.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub label: String,
    pub date: u64,
    pub words: u32,
    pub dictations: u32,
}

/// The dashboard summary.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub total_words: u32,
    pub total_dictations: u32,
    /// Current streak of consecutive days (ending today or yesterday) with
    /// at least one dictation.
    pub streak_days: u32,
    /// Average words per day over the last 7 days that have data.
    pub avg_words_per_day: u32,
    /// Words dictated today.
    pub words_today: u32,
    /// Words dictated in the last 7 days.
    pub words_last_7_days: u32,
    /// Daily activity for the last 14 days (oldest first).
    pub daily: Vec<DayStat>,
    /// Average words per minute across all entries (duration is unknown;
    /// approximated from word count and a conservative speaking rate).
    pub est_minutes_spoken: u32,
}

const WORDS_PER_MINUTE: u32 = 150;

/// Counts words the way users think of them: whitespace-separated tokens.
pub fn word_count(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

/// UTC day start (ms) for a timestamp.
pub fn day_start_ms(ts: u64) -> u64 {
    let secs = ts / 1000;
    (secs - secs % 86_400) * 1000
}

fn day_label(day_ms: u64) -> String {
    let days = day_ms / 86_400_000;
    // 1970-01-01 was a Thursday.
    let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][(4 + days as usize) % 7];
    let month_day = month_day_label(day_ms);
    format!("{weekday} {month_day}")
}

/// "16 Sep" style label from a UTC day-start timestamp.
fn month_day_label(day_ms: u64) -> String {
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (day_ms / 86_400_000) as i64;
    // Days since epoch → civil date (Howard Hinnant's algorithm).
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

/// Builds the dashboard summary for the last 14 days.
pub fn dashboard(history: &DictationHistory, now_ms: u64) -> DashboardStats {
    let today_start = day_start_ms(now_ms);
    let week_start = today_start.saturating_sub(6 * 86_400_000);

    let mut total_words: u32 = 0;
    let mut total_dictations: u32 = 0;
    let mut words_today: u32 = 0;
    let mut words_last_7: u32 = 0;

    // Bucket the last 14 days.
    let mut daily: Vec<DayStat> = (0..14)
        .map(|i| today_start - (13 - i) * 86_400_000)
        .map(|d| DayStat {
            label: day_label(d),
            date: d,
            words: 0,
            dictations: 0,
        })
        .collect();

    for entry in &history.entries {
        let words = word_count(&entry.text);
        total_words += words;
        total_dictations += 1;
        let day = day_start_ms(entry.created_at);
        if day == today_start {
            words_today += words;
        }
        if day >= week_start {
            words_last_7 += words;
        }
        if let Some(slot) = daily.iter_mut().find(|s| s.date == day) {
            slot.words += words;
            slot.dictations += 1;
        }
    }

    // Streak: count backwards from today; a today-less streak starts
    // yesterday.
    let mut streak: u32 = 0;
    let mut cursor = today_start;
    // Today counts only if it already has dictations.
    if daily.last().is_some_and(|d| d.dictations > 0) {
        streak += 1;
        cursor -= 86_400_000;
    }
    while let Some(day) = daily.iter().find(|s| s.date == cursor) {
        if day.dictations > 0 {
            streak += 1;
            cursor -= 86_400_000;
        } else {
            break;
        }
    }

    // Average over the last 7 days that have data.
    let last7: &[DayStat] = &daily[daily.len().saturating_sub(7)..];
    let active_days: u32 = last7.iter().filter(|d| d.dictations > 0).count() as u32;
    let avg = words_last_7 / active_days.max(1);

    DashboardStats {
        total_words,
        total_dictations,
        streak_days: streak,
        avg_words_per_day: avg,
        words_today,
        words_last_7_days: words_last_7,
        daily,
        est_minutes_spoken: total_words / WORDS_PER_MINUTE,
    }
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

    const DAY: u64 = 86_400_000;
    const NOW: u64 = 20_000 * DAY; // some day on the UTC grid

    #[test]
    fn word_count_counts_tokens() {
        assert_eq!(word_count("hello world"), 2);
        assert_eq!(word_count("  spaced   out "), 2);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn day_start_floors_to_utc_day() {
        assert_eq!(day_start_ms(NOW), NOW);
        assert_eq!(day_start_ms(NOW + 123_456), NOW);
        assert_eq!(day_start_ms(NOW + DAY - 1), NOW + DAY - DAY);
    }

    #[test]
    fn dashboard_aggregates_days_and_streak() {
        let mut h = DictationHistory::default();
        // Today: 2 dictations, 10 words.
        h.push(entry(NOW + 1000, "one two three four five"));
        h.push(entry(NOW + 2000, "six seven eight nine ten"));
        // Yesterday: 5 words.
        h.push(entry(NOW - DAY, "a b c d e"));
        // Two days ago: 4 words.
        h.push(entry(NOW - 2 * DAY, "w x y z"));
        // Gap, then four days ago (outside the 7-day window): streak must stop.
        h.push(entry(NOW - 8 * DAY, "old words here"));

        let stats = dashboard(&h, NOW);
        assert_eq!(stats.total_words, 22);
        assert_eq!(stats.total_dictations, 5);
        assert_eq!(stats.words_today, 10);
        assert_eq!(stats.words_last_7_days, 19);
        assert_eq!(stats.streak_days, 3);
        assert_eq!(stats.daily.len(), 14);
        let today = stats.daily.last().unwrap();
        assert_eq!(today.words, 10);
        assert_eq!(today.dictations, 2);
        assert_eq!(stats.est_minutes_spoken, 0); // 20 words < 150/min
    }

    #[test]
    fn empty_history_is_zero() {
        let stats = dashboard(&DictationHistory::default(), NOW);
        assert_eq!(stats.total_words, 0);
        assert_eq!(stats.streak_days, 0);
        assert_eq!(stats.daily.len(), 14);
    }

    #[test]
    fn day_labels_are_readable() {
        // 2026-09-21 is a Tuesday.
        let d = day_start_ms(1_790_000_000_000);
        assert_eq!(day_label(d), "Tue 21 Sep");
    }
}
