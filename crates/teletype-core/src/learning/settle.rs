//! Settle-by-survival learning (U3, ported from Vocs session/learning.rs).
//!
//! A correction counts as accepted only if the corrected form is still
//! present in the observed text after the user has had time to notice and
//! undo it. Judging by survival, not by a click.
//!
//! The sweep finds dictation rows past their settle deadline (30 min),
//! judges each recorded correction against the stored observed text, and
//! counts kept/undone on the corresponding `learn_candidate` row.
//!
//! Promotion rule: 3 accepted and 0 rejected → auto-apply. A `manual` flag
//! (set by the user) outranks the tallies permanently. One rejection retires
//! a candidate from auto-apply.

use serde::{Deserialize, Serialize};

use super::store::{LearningStore, StoreResult};

/// A proposed correction carried on a dictation row, judged later by
/// survival. `pattern` is the original text (lowercased); `replacement` is
/// the corrected form. `auto` is true when the correction was applied from
/// the learned list rather than proposed by the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettleEdit {
    pub pattern: String,
    pub replacement: String,
    #[serde(default)]
    pub auto: bool,
}

/// Kept this many times, never undone: the point a fix applies itself.
pub const AUTO_APPLY_AFTER: i64 = 3;

/// Long enough for the user to have read the result and undone it if it was
/// wrong; short enough that a day of dictation settles the same day.
/// This is "long enough that the user has finished and reviewed the message,"
/// not a magic number.
pub const SETTLE_AFTER_MS: i64 = 30 * 60 * 1000;

/// Settle status tri-state on the dictation row.
///
/// 0 = unsettled (corrections recorded, not yet judged)
/// 1 = settled (judged: corrections counted as kept or undone)
/// 2 = reverted (user explicitly chose "use original" before settle)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum SettleStatus {
    Unsettled = 0,
    Settled = 1,
    Reverted = 2,
}

/// The verdict for one correction, judged from the observed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Kept,
    Undone,
    /// The observed span is empty or unlocatable — neither kept nor undone.
    /// Must not silently count as acceptance.
    Unlocatable,
}

/// Judge whether the user kept or undid a correction, from the observed
/// text at the paste anchor.
///
/// `observed` is the text captured by the observer at the paste span, or
/// `None` when the observer could not locate the span (field unreadable,
/// span drifted beyond the distance budget).
///
/// `reverted` is true when the user explicitly chose "use original" before
/// the settle deadline.
pub fn judge(edit: &SettleEdit, observed: Option<&str>, reverted: bool) -> Verdict {
    if reverted {
        return Verdict::Undone;
    }
    match observed {
        Some(text) if !text.is_empty() => {
            if contains_phrase(text, &edit.replacement) {
                Verdict::Kept
            } else {
                Verdict::Undone
            }
        }
        _ => Verdict::Unlocatable,
    }
}

/// Whole-word, case-insensitive substring match: does `haystack` contain
/// `needle` as a sequence of whole words, allowing punctuation between
/// words? Ported from Vocs `pipeline/grammar/learned.rs::contains_phrase`.
///
/// "he goes" matches "Then he goes home." but not "he goes." inside
/// "he goeson." — word boundaries must be non-alphanumeric.
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    let haystack_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    // Compare chars, not bytes: `str::len()` counts bytes and these are
    // indices into the char vector, so a non-ASCII replacement ("żółć")
    // would mis-slice at best and never match at worst.
    let needle_words: Vec<Vec<char>> = needle_lower
        .split_whitespace()
        .map(|w| w.chars().collect())
        .collect();
    if needle_words.is_empty() {
        return false;
    }

    // Walk the haystack char by char; at each position try to match the
    // first needle word as a whole word (bounded by non-alphanumeric on
    // both sides), then skip inter-word punctuation and whitespace, and
    // match the next needle word, etc.
    let h_chars: Vec<char> = haystack_lower.chars().collect();
    let h_len = h_chars.len();

    let is_word_boundary = |i: usize| -> bool {
        // True if position i is a word boundary: start of string, or the
        // char at i-1 is not alphanumeric.
        i == 0 || !h_chars[i - 1].is_alphanumeric()
    };

    'outer: for start in 0..h_len {
        if !is_word_boundary(start) {
            continue;
        }
        let mut pos = start;
        for (wi, word) in needle_words.iter().enumerate() {
            // Try to match `word` starting at `pos`.
            if pos + word.len() > h_len {
                continue 'outer;
            }
            let matched = h_chars[pos..pos + word.len()]
                .iter()
                .zip(word.iter())
                .all(|(a, b)| a == b);
            if !matched {
                continue 'outer;
            }
            // Check the trailing word boundary.
            let end = pos + word.len();
            if end < h_len && h_chars[end].is_alphanumeric() {
                continue 'outer;
            }
            pos = end;
            // Between words: skip whitespace and punctuation.
            if wi + 1 < needle_words.len() {
                while pos < h_len && (!h_chars[pos].is_alphanumeric()) {
                    pos += 1;
                }
                if pos >= h_len {
                    continue 'outer;
                }
            }
        }
        // All words matched.
        return true;
    }
    false
}

/// Kept enough, never undone: the point a fix applies itself.
pub fn should_auto_apply(accepted: i64, rejected: i64, manual: bool, manual_auto: bool) -> bool {
    if manual {
        return manual_auto;
    }
    accepted >= AUTO_APPLY_AFTER && rejected == 0
}

/// The result of settling one dictation row's corrections.
#[derive(Debug, Clone, Default)]
pub struct SettleResult {
    pub settled: usize,
    pub unlocatable: usize,
}

/// Rows judged per sweep. The sweep runs on the observation flush, which is
/// already a background moment; a large backlog (a machine left on over the
/// weekend) settles over several flushes rather than stalling one.
pub const SWEEP_BATCH: i64 = 200;

/// The `learn_candidate` kind a surviving correction tallies against.
/// Mishearing pairs share the `(kind, a, b, category)` key with the promote
/// side, so survival evidence lands on the row the learner already uses.
pub const CANDIDATE_KIND: &str = "mishearing";

/// Encodes a settle list as the JSON carried on `dictation.settle_edits`.
pub fn encode_edits(edits: &[SettleEdit]) -> String {
    serde_json::to_string(edits).unwrap_or_else(|_| "[]".to_string())
}

/// Decodes `dictation.settle_edits`. Malformed JSON decodes to an empty
/// list: the row is closed without touching any tally, so a corrupt payload
/// can neither inflate acceptance nor wedge the sweep on every pass.
pub fn decode_edits(json: &str) -> Vec<SettleEdit> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Settles every dictation row that is past its deadline.
///
/// For each row: judge every recorded correction against the stored observed
/// span, add the verdict to that pair's `learn_candidate` tallies, recompute
/// `auto_apply`, and close the row so it is never judged twice.
///
/// `Verdict::Unlocatable` counts as neither accepted nor rejected; it is
/// reported in [`SettleResult::unlocatable`] and leaves the tallies alone.
///
/// `reverted` (status 2, "the user chose the original before the deadline")
/// has no producer in the app yet, and the row query only returns status 0,
/// so every edit here is judged from the observed span alone. The undo signal
/// today is the span no longer containing the corrected form.
pub fn sweep(store: &LearningStore, now_ms: u64) -> StoreResult<SettleResult> {
    let cutoff = now_ms.saturating_sub(SETTLE_AFTER_MS as u64) as i64;
    let rows = store.unsettled_settled_edits(cutoff, SWEEP_BATCH)?;
    let mut out = SettleResult::default();
    for row in rows {
        // A row that reaches its deadline is closed even when its payload is
        // empty or unreadable: it is judged at most once.
        let mut tallies: Vec<super::store::CandidateTally> = Vec::new();
        for edit in decode_edits(&row.settle_edits) {
            let verdict = judge(&edit, row.observed.as_deref(), false);
            let (accepted_delta, rejected_delta) = match verdict {
                Verdict::Kept => (1, 0),
                Verdict::Undone => (0, 1),
                Verdict::Unlocatable => {
                    out.unlocatable += 1;
                    continue;
                }
            };
            tallies.push(super::store::CandidateTally {
                kind: CANDIDATE_KIND.to_string(),
                a: edit.pattern,
                b: edit.replacement,
                category: String::new(),
                accepted_delta,
                rejected_delta,
            });
        }
        // One transaction: the tallies and the "settled" mark land together,
        // or not at all. Splitting them let a crash count the same survival
        // twice on the next sweep.
        store.settle_dictation_row(row.id, &tallies, now_ms, should_auto_apply)?;
        out.settled += 1;
    }
    Ok(out)
}

/// Survival evidence for a pair the learner is about to promote: how many
/// times the user kept the correction and how many times they undid it. Both
/// zero means no row carrying this pair has reached its deadline yet, which
/// is not evidence either way.
pub fn survival(
    store: &LearningStore,
    pattern: &str,
    replacement: &str,
) -> StoreResult<(i64, i64)> {
    Ok(store
        .candidate_get_full(CANDIDATE_KIND, pattern, replacement, "")?
        .map(|c| (c.accepted, c.rejected))
        .unwrap_or((0, 0)))
}

/// Pairs worth judging later, from the text ASR produced and the text that
/// was pasted. Word-level replacements of 1-4 words on each side only (the
/// same span budget the judge uses; a full-rewrite pair is not a correction
/// the user can be said to have kept or undid).
///
/// Caller gates this on polish having run: on such a take the pairs are every
/// change between ASR and paste, which includes filler removal and AutoText
/// expansion, not only the model's edits.
pub fn edits_from_change(raw_asr: &str, pasted: &str) -> Vec<SettleEdit> {
    super::diff::changed_pairs(&super::diff::diff_words(raw_asr, pasted))
        .into_iter()
        .take(MAX_EDITS_PER_ROW)
        .map(|(from, to)| SettleEdit {
            pattern: from.join(" "),
            replacement: to.join(" "),
            auto: false,
        })
        .collect()
}

/// A take that rewrote more than this many spans is a wholesale rewrite, not
/// a set of corrections; the row carries the first `MAX_EDITS_PER_ROW`.
const MAX_EDITS_PER_ROW: usize = 16;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::store::DictationRow;

    fn edit(pattern: &str, replacement: &str) -> SettleEdit {
        SettleEdit {
            pattern: pattern.into(),
            replacement: replacement.into(),
            auto: false,
        }
    }

    #[test]
    fn kept_when_replacement_is_in_observed_text() {
        let e = edit("he go", "he goes");
        assert_eq!(judge(&e, Some("Then he goes home."), false), Verdict::Kept);
    }

    #[test]
    fn undone_when_replacement_is_absent() {
        let e = edit("he go", "he goes");
        assert_eq!(judge(&e, Some("Then he go home."), false), Verdict::Undone);
    }

    #[test]
    fn unlocatable_when_observed_is_none() {
        let e = edit("he go", "he goes");
        assert_eq!(judge(&e, None, false), Verdict::Unlocatable);
    }

    #[test]
    fn unlocatable_when_observed_is_empty() {
        let e = edit("he go", "he goes");
        assert_eq!(judge(&e, Some(""), false), Verdict::Unlocatable);
    }

    #[test]
    fn reverted_always_undone() {
        let e = edit("he go", "he goes");
        assert_eq!(judge(&e, Some("Then he goes home."), true), Verdict::Undone);
        assert_eq!(judge(&e, None, true), Verdict::Undone);
    }

    #[test]
    fn case_insensitive_match() {
        let e = edit("he go", "He Goes");
        assert_eq!(judge(&e, Some("Then he goes home."), false), Verdict::Kept);
    }

    #[test]
    fn whole_word_only() {
        let e = edit("he go", "he goes");
        // "he goest" should NOT match "he goes"
        assert_eq!(
            judge(&e, Some("Then he goest home."), false),
            Verdict::Undone
        );
    }

    #[test]
    fn punctuation_between_words_ok() {
        let e = edit("he go", "he, goes");
        assert_eq!(judge(&e, Some("Then he, goes home."), false), Verdict::Kept);
    }

    #[test]
    fn multi_word_pattern() {
        let e = edit("the the", "the");
        assert_eq!(judge(&e, Some("the cat sat"), false), Verdict::Kept);
        assert_eq!(
            judge(&e, Some("the cat sat on the mat"), false),
            Verdict::Kept
        );
    }

    #[test]
    fn auto_apply_rule_three_keeps_no_undone() {
        assert!(!should_auto_apply(2, 0, false, false));
        assert!(should_auto_apply(3, 0, false, false));
        assert!(!should_auto_apply(3, 1, false, false));
        assert!(should_auto_apply(10, 0, false, false));
    }

    #[test]
    fn manual_flag_outranks_tallies() {
        // manual=true with manual_auto=false: user said no, arithmetic can't override
        assert!(!should_auto_apply(10, 0, true, false));
        // manual=true with manual_auto=true: user said yes
        assert!(should_auto_apply(0, 5, true, true));
    }

    #[test]
    fn contains_phrase_exact() {
        assert!(contains_phrase("he goes home", "he goes"));
        assert!(!contains_phrase("he go home", "he goes"));
    }

    #[test]
    fn contains_phrase_word_boundary() {
        // "goes" inside "goest" is not a whole-word match
        assert!(!contains_phrase("he goest", "he goes"));
        // But "goes" at the end is fine
        assert!(contains_phrase("he goes", "he goes"));
    }

    #[test]
    fn contains_phrase_punctuation() {
        assert!(contains_phrase("he, goes", "he goes"));
        assert!(contains_phrase("he goes.", "he goes"));
        assert!(!contains_phrase("hegoes", "he goes"));
    }

    #[test]
    fn contains_phrase_non_ascii_matches() {
        // `word.len()` is bytes and the haystack index is chars: with a
        // multibyte replacement the old comparison never matched, so every
        // non-ASCII correction was judged Undone and permanently banned from
        // auto-apply.
        assert!(contains_phrase("to żółć teraz", "to żółć"));
        assert!(contains_phrase("dziękuję bardzo", "dziękuję"));
        assert_eq!(
            judge(&edit("to zolc", "to żółć"), Some("to żółć teraz"), false),
            Verdict::Kept
        );
        // A byte-short needle must not match a longer non-ASCII word either.
        assert!(!contains_phrase("żółćab", "żółć"));
    }

    #[test]
    fn settle_status_roundtrip() {
        assert_eq!(SettleStatus::Unsettled as i64, 0);
        assert_eq!(SettleStatus::Settled as i64, 1);
        assert_eq!(SettleStatus::Reverted as i64, 2);
    }

    #[test]
    fn settle_after_ms_is_30_minutes() {
        assert_eq!(SETTLE_AFTER_MS, 30 * 60 * 1000);
    }

    fn tmp_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
        (dir, store)
    }

    fn row(ts: u64, raw: &str, pasted: &str) -> DictationRow {
        DictationRow {
            id: 0,
            ts,
            app_id: "slack".into(),
            app_category: "chat".into(),
            engine: "whisper".into(),
            lang: "en".into(),
            audio_path: None,
            raw_asr: raw.into(),
            words_json: "[]".into(),
            pasted: pasted.into(),
            observed: None,
            edit_ops_json: None,
            polish_model: None,
            polish_skipped: false,
        }
    }

    /// Records a dictation whose pasted text differs from ASR, tags it with
    /// the settle payload, and optionally writes the observed span.
    fn record(
        store: &LearningStore,
        ts: u64,
        raw: &str,
        pasted: &str,
        observed: Option<&str>,
    ) -> i64 {
        let mut r = row(ts, raw, pasted);
        r.observed = observed.map(str::to_string);
        let id = store.record_dictation(&r).unwrap();
        let edits = edits_from_change(raw, pasted);
        if !edits.is_empty() {
            store.set_settle_edits(id, &encode_edits(&edits)).unwrap();
        }
        if let Some(text) = observed {
            store.set_observed(id, text, None).unwrap();
        }
        id
    }

    const NOW: u64 = 1_000_000_000;

    /// Puts the pair in the store with the user's own auto/manual choice.
    /// `set_candidate_auto` only updates an existing row, so the row is
    /// created first with zero tallies.
    fn seed_manual(store: &LearningStore, auto: bool) {
        store
            .count_candidate(CANDIDATE_KIND, "he go", "He goes", "", 0, 0, NOW)
            .unwrap();
        store
            .set_candidate_auto(CANDIDATE_KIND, "he go", "He goes", "", auto, true)
            .unwrap();
    }

    #[test]
    fn sweep_counts_a_kept_correction_as_accepted() {
        let (_dir, store) = tmp_store();
        record(
            &store,
            NOW - 31 * 60 * 1000,
            "he go home",
            "He goes home",
            Some("He goes home now."),
        );
        let out = sweep(&store, NOW).unwrap();
        assert_eq!(out.settled, 1);
        assert_eq!(survival(&store, "he go", "He goes").unwrap(), (1, 0));
    }

    #[test]
    fn sweep_counts_an_undone_correction_as_rejected() {
        let (_dir, store) = tmp_store();
        record(
            &store,
            NOW - 31 * 60 * 1000,
            "he go home",
            "He goes home",
            Some("he go home"),
        );
        sweep(&store, NOW).unwrap();
        assert_eq!(survival(&store, "he go", "He goes").unwrap(), (0, 1));
    }

    #[test]
    fn unlocatable_span_does_not_increment_accepted() {
        let (_dir, store) = tmp_store();
        // Recorded but never observed: the span could not be located, so the
        // correction is neither kept nor undone.
        record(
            &store,
            NOW - 31 * 60 * 1000,
            "he go home",
            "He goes home",
            None,
        );
        let out = sweep(&store, NOW).unwrap();
        assert_eq!(out.unlocatable, 1);
        assert_eq!(out.settled, 1);
        assert_eq!(
            store
                .candidate_get_full(CANDIDATE_KIND, "he go", "He goes", "")
                .unwrap()
                .map(|c| (c.accepted, c.rejected)),
            None,
            "an unjudged span must not create a tally"
        );
    }

    #[test]
    fn a_settled_row_is_never_settled_twice() {
        let (_dir, store) = tmp_store();
        record(
            &store,
            NOW - 31 * 60 * 1000,
            "he go home",
            "He goes home",
            Some("He goes home"),
        );
        assert_eq!(sweep(&store, NOW).unwrap().settled, 1);
        let second = sweep(&store, NOW + 1).unwrap();
        assert_eq!(
            second.settled, 0,
            "a settled row came back for a second sweep"
        );
        assert_eq!(
            second.unlocatable, 0,
            "a settled row's edits were re-judged"
        );
        assert_eq!(survival(&store, "he go", "He goes").unwrap(), (1, 0));
    }

    #[test]
    fn rows_inside_the_window_are_not_judged_yet() {
        let (_dir, store) = tmp_store();
        record(
            &store,
            NOW - SETTLE_AFTER_MS as u64 + 1000,
            "he go home",
            "He goes home",
            Some("He goes home"),
        );
        assert_eq!(sweep(&store, NOW).unwrap().settled, 0);
    }

    #[test]
    fn three_keeps_auto_applies_and_a_rejection_afterwards_stops_it() {
        let (_dir, store) = tmp_store();
        for i in 0..3 {
            record(
                &store,
                NOW - (31 + i) * 60 * 1000,
                "he go home",
                "He goes home",
                Some("He goes home"),
            );
        }
        sweep(&store, NOW).unwrap();
        let c = store
            .candidate_get_full(CANDIDATE_KIND, "he go", "He goes", "")
            .unwrap()
            .unwrap();
        assert_eq!((c.accepted, c.rejected, c.auto_apply), (3, 0, true));

        // One explicit undo retires the candidate from auto-apply.
        record(
            &store,
            NOW - 40 * 60 * 1000,
            "he go home",
            "He goes home",
            Some("he go home"),
        );
        sweep(&store, NOW + 1).unwrap();
        let c = store
            .candidate_get_full(CANDIDATE_KIND, "he go", "He goes", "")
            .unwrap()
            .unwrap();
        assert_eq!((c.accepted, c.rejected, c.auto_apply), (3, 1, false));
    }

    #[test]
    fn manual_choice_is_not_flipped_by_arithmetic() {
        let (_dir, store) = tmp_store();
        seed_manual(&store, false);
        for i in 0..5 {
            record(
                &store,
                NOW - (31 + i) * 60 * 1000,
                "he go home",
                "He goes home",
                Some("He goes home"),
            );
        }
        sweep(&store, NOW).unwrap();
        let c = store
            .candidate_get_full(CANDIDATE_KIND, "he go", "He goes", "")
            .unwrap()
            .unwrap();
        assert_eq!(c.accepted, 5);
        assert!(!c.auto_apply, "arithmetic overrode a manual off switch");
    }

    #[test]
    fn manual_yes_survives_a_rejection() {
        let (_dir, store) = tmp_store();
        seed_manual(&store, true);
        record(
            &store,
            NOW - 31 * 60 * 1000,
            "he go home",
            "He goes home",
            Some("he go home"),
        );
        sweep(&store, NOW).unwrap();
        let c = store
            .candidate_get_full(CANDIDATE_KIND, "he go", "He goes", "")
            .unwrap()
            .unwrap();
        assert_eq!((c.accepted, c.rejected, c.auto_apply), (0, 1, true));
    }

    #[test]
    fn corrupt_payload_closes_the_row_without_touching_tallies() {
        let (_dir, store) = tmp_store();
        let id = store
            .record_dictation(&row(NOW - 31 * 60 * 1000, "a b", "a c"))
            .unwrap();
        store.set_settle_edits(id, "{not json").unwrap();
        assert_eq!(sweep(&store, NOW).unwrap().settled, 1);
        assert!(store.candidate_all().unwrap().is_empty());
        // And it is not re-read on the next pass.
        assert_eq!(sweep(&store, NOW + 1).unwrap().settled, 0);
    }

    #[test]
    fn edits_from_change_takes_short_replacements_only() {
        let edits = edits_from_change("i hav a idea", "I have an idea");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].pattern, "i hav a");
        assert_eq!(edits[0].replacement, "I have an");
        // A pure deletion is not a correction pair the user kept or undid.
        assert!(edits_from_change("the the cat sat", "the cat sat").is_empty());
        // A span over 4 words on either side is a rewrite, not a fix.
        assert!(
            edits_from_change("one two three four five six", "seven eight nine ten eleven")
                .is_empty()
        );
    }

    #[test]
    fn encode_decode_roundtrips() {
        let edits = vec![
            SettleEdit {
                pattern: "he go".into(),
                replacement: "He goes".into(),
                auto: true,
            },
            SettleEdit {
                pattern: "the the".into(),
                replacement: "the".into(),
                auto: false,
            },
        ];
        assert_eq!(decode_edits(&encode_edits(&edits)), edits);
        assert!(decode_edits("null").is_empty());
    }
}
