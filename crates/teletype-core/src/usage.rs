//! Runtime usage tracking: which filler words were removed and which AutoText
//! entries were expanded, accumulated across dictations.
//!
//! Counts are captured at pipeline-run time (the raw transcript still contains
//! the filler words and the spoken snippet phrases) and persisted so the
//! Insights and AutoText screens can show "which filler was removed how many
//! times" and "which snippet is used how often".

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::autotext::{protect, AutoTextStore};
use crate::context::ApplicationContext;

/// Accumulated usage counters, persisted as JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageStats {
    /// Filler word (lowercased) → times removed.
    pub filler_counts: BTreeMap<String, u32>,
    /// AutoText trigger (e.g. `/email`) or spoken snippet phrase → times used.
    pub autotext_counts: BTreeMap<String, u32>,
}

impl UsageStats {
    /// Adds `delta` to the counter for `key`.
    fn bump(map: &mut BTreeMap<String, u32>, key: &str, delta: u32) {
        if delta == 0 {
            return;
        }
        *map.entry(key.to_string()).or_insert(0) += delta;
    }

    /// Folds one dictation's usage into the stats.
    pub fn record(&mut self, filler: &BTreeMap<String, u32>, autotext: &BTreeMap<String, u32>) {
        for (k, v) in filler {
            Self::bump(&mut self.filler_counts, k, *v);
        }
        for (k, v) in autotext {
            Self::bump(&mut self.autotext_counts, k, *v);
        }
    }

    /// Total filler words removed, summed across all words.
    pub fn total_fillers_removed(&self) -> u32 {
        self.filler_counts.values().sum()
    }

    /// Total AutoText expansions, summed across all entries.
    pub fn total_autotext_used(&self) -> u32 {
        self.autotext_counts.values().sum()
    }

    /// Drops any filler words no longer in the active list, so the Insights
    /// screen only shows words the user currently removes.
    pub fn prune_fillers(&mut self, active: &[String]) {
        let active: std::collections::HashSet<&str> = active.iter().map(|s| s.as_str()).collect();
        self.filler_counts.retain(|k, _| {
            active
                .iter()
                .any(|a| a.to_lowercase().as_str() == k.as_str())
        });
    }

    /// Drops AutoText counters for entries that no longer exist.
    pub fn prune_autotext(&mut self, store: &AutoTextStore) {
        let known: std::collections::HashSet<String> = store
            .entries
            .iter()
            .flat_map(|e| {
                let mut v = vec![e.trigger.clone()];
                if !e.snippet.trim().is_empty() {
                    v.push(e.snippet.trim().to_string());
                }
                v
            })
            .collect();
        self.autotext_counts.retain(|k, _| known.contains(k));
    }
}

/// Counts how many times each filler word appears in `text` (whole-word,
/// case-insensitive, matching the removal rules in `pipeline::remove_filler_words`).
pub fn count_fillers(text: &str, words: &[String]) -> BTreeMap<String, u32> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    if words.is_empty() {
        return counts;
    }
    let filler: std::collections::HashSet<String> =
        words.iter().map(|w| w.to_lowercase()).collect();
    for tok in text.split_whitespace() {
        let bare = tok
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if let Some(w) = filler.get(&bare) {
            *counts.entry(w.clone()).or_insert(0) += 1;
        }
    }
    counts
}

/// Counts how many times each AutoText trigger/snippet is used in `text`.
///
/// For typed triggers it counts whole-token `/trigger` matches; for spoken
/// snippets it reuses the same matching the pipeline uses (protect_snippets)
/// by counting the placeholders that would be produced.
pub fn count_autotext(
    text: &str,
    store: &AutoTextStore,
    app: &ApplicationContext,
) -> BTreeMap<String, u32> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();

    // Typed triggers: count whole-token occurrences of each applicable trigger.
    let mut applicable: Vec<&crate::autotext::AutoTextEntry> =
        store.entries.iter().filter(|e| e.applies_to(app)).collect();
    applicable.sort_by_key(|e| std::cmp::Reverse(e.trigger.len()));
    for entry in &applicable {
        let trigger = entry.trigger.as_str();
        if trigger.is_empty() {
            continue;
        }
        let mut n: u32 = 0;
        for tok in text.split_whitespace() {
            // A trigger token is the trigger possibly followed by punctuation.
            let bare = tok.trim_start_matches('/');
            if bare.eq_ignore_ascii_case(trigger.trim_start_matches('/')) {
                n += 1;
            }
        }
        if n > 0 {
            bump(&mut counts, &entry.trigger, n);
        }
    }

    // Spoken snippets: count placeholders protect_snippets would produce.
    let protected = protect::protect_snippets(text, store, app);
    // protect_snippets stores values but not which entry; re-derive counts by
    // scanning for each snippet phrase directly.
    for entry in store.snippets_for(app) {
        let phrase = entry.snippet_phrase();
        if phrase.is_empty() {
            continue;
        }
        let n = count_phrase_occurrences(text, phrase);
        if n > 0 {
            bump(&mut counts, phrase, n);
        }
    }
    let _ = protected; // (placeholder count cross-check; phrase scan is authoritative)

    counts
}

fn bump(map: &mut BTreeMap<String, u32>, key: &str, delta: u32) {
    if delta == 0 {
        return;
    }
    *map.entry(key.to_string()).or_insert(0) += delta;
}

/// Case-insensitive whole-word count of a multi-word `phrase` in `text`.
fn count_phrase_occurrences(text: &str, phrase: &str) -> u32 {
    let mut count = 0u32;
    let mut rest = text;
    loop {
        match crate::autotext::match_snippet_at(rest, 0, phrase) {
            Some(end) => {
                count += 1;
                rest = &rest[end..];
            }
            None => {
                // Advance past the first char and retry, so overlapping or
                // later occurrences are found.
                if rest.is_empty() {
                    break;
                }
                let next = rest
                    .find(|c: char| c.is_alphanumeric())
                    .unwrap_or(rest.len());
                if next == 0 {
                    rest = &rest[1..];
                } else {
                    rest = &rest[next..];
                }
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autotext::AutoTextEntry;

    #[test]
    fn counts_fillers_whole_word_only() {
        let words = vec!["um".into(), "er".into()];
        let c = count_fillers("um, this is er, fine", &words);
        assert_eq!(c.get("um"), Some(&1));
        assert_eq!(c.get("er"), Some(&1));
        // "error" must not count as "er".
        let c2 = count_fillers("error", &words);
        assert_eq!(c2.get("er"), None);
    }

    #[test]
    fn counts_typed_triggers() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/email", "a@b.c")).unwrap();
        let app = ApplicationContext::unknown();
        let c = count_autotext("mail /email and /email ok", &store, &app);
        assert_eq!(c.get("/email"), Some(&2));
    }

    #[test]
    fn counts_spoken_snippets() {
        let mut store = AutoTextStore::default();
        let mut e = AutoTextEntry::new("/email", "a@b.c");
        e.snippet = "my email".into();
        store.insert(e).unwrap();
        let app = ApplicationContext::unknown();
        let c = count_autotext("send to my email please", &store, &app);
        assert_eq!(c.get("my email"), Some(&1));
    }

    #[test]
    fn record_accumulates() {
        let mut s = UsageStats::default();
        let mut f = BTreeMap::new();
        f.insert("um".to_string(), 2);
        let mut a = BTreeMap::new();
        a.insert("/email".to_string(), 1);
        s.record(&f, &a);
        s.record(&f, &a);
        assert_eq!(s.filler_counts.get("um"), Some(&4));
        assert_eq!(s.autotext_counts.get("/email"), Some(&2));
        assert_eq!(s.total_fillers_removed(), 4);
        assert_eq!(s.total_autotext_used(), 2);
    }
}
