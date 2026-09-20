//! Usage insights: top phrases, most-used apps, and writing habits,
//! all computed locally from the dictation history.

use std::collections::HashMap;

use crate::history::DictationHistory;

/// A ranked item (e.g. a phrase or app name).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedItem {
    pub label: String,
    pub count: u32,
    /// Relative frequency 0..=1 vs the top item (for bar rendering).
    pub share: f32,
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
}

const NGRAM_SIZES: [usize; 2] = [2, 3];
const MAX_ITEMS: usize = 8;

/// Computes insights over the full history.
pub fn compute(history: &DictationHistory) -> Insights {
    let mut ngram_counts: HashMap<(usize, String), u32> = HashMap::new();
    let mut app_counts: HashMap<String, u32> = HashMap::new();
    let mut hour_counts: [u32; 24] = [0; 24];
    let mut total_words: u32 = 0;
    let mut dictations: u32 = 0;
    let mut vocab: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entry in &history.entries {
        dictations += 1;
        let words: Vec<&str> = entry.text.split_whitespace().collect();
        total_words += words.len() as u32;
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
                    // Skip n-grams that are mostly punctuation.
                    if phrase.chars().filter(|c| c.is_alphanumeric()).count() < n {
                        continue;
                    }
                    *ngram_counts.entry((n, phrase)).or_insert(0) += 1;
                }
            }
        }
        if let Some(ctx) = &entry.context {
            if ctx.is_known() {
                *app_counts.entry(ctx.application_name.clone()).or_insert(0) += 1;
            }
        }
        let secs = entry.created_at / 1000;
        hour_counts[(secs % 86_400) as usize / 3600] += 1;
    }

    // Top phrases: a 2-gram is dropped when the same words appear inside a
    // 3-gram ("please send" inside "please send the report"), then rank the
    // survivors by count.
    // A 2-gram is subsumed when its words appear inside a 3-gram with the
    // same count (e.g. "please send" inside "please send the report").
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

    Insights {
        top_phrases: phrases,
        top_apps: apps,
        avg_words_per_dictation: total_words / dictations.max(1),
        busiest_hour,
        vocabulary_size: vocab.len() as u32,
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

    #[test]
    fn repeated_phrases_are_ranked() {
        let mut h = DictationHistory::default();
        h.push(entry(1, "please send the report today"));
        h.push(entry(2, "hey, please send the report when ready"));
        h.push(entry(3, "can you please send the report now"));
        let i = compute(&h);
        // The recurring 3-grams rank at the top.
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
        let i = compute(&h);
        assert!(i.top_phrases.is_empty());
    }

    #[test]
    fn apps_and_hours_are_tracked() {
        let mut h = DictationHistory::default();
        let mut e = entry(3_600_000 * 9 + 1000, "hello world");
        e.context = Some(crate::context::normalize("com.google.gmail", "Gmail"));
        h.push(e);
        let i = compute(&h);
        assert_eq!(i.top_apps.len(), 1);
        assert_eq!(i.top_apps[0].label, "Gmail");
        assert_eq!(i.busiest_hour, Some(9));
        assert_eq!(i.vocabulary_size, 2);
    }

    #[test]
    fn empty_history_is_empty() {
        let i = compute(&DictationHistory::default());
        assert!(i.top_phrases.is_empty());
        assert!(i.top_apps.is_empty());
        assert_eq!(i.vocabulary_size, 0);
    }
}
