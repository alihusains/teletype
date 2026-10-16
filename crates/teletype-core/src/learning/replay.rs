//! Replay verification (Phase 7.7).
//!
//! When the machine is idle, `run_replay` re-transcribes the most recent
//! stored clips with the *current* and the *previous* boost lists, measures
//! word error rate (WER) against what the user actually kept (`observed`),
//! and feeds each boosted term's hit / false-alarm counters. A term that
//! keeps causing mistakes is demoted (weight halved) and, once its weight
//! drops to or below 0.5, tombstoned with a user-facing notice
//! ("Stopped boosting 'Kate' because it caused mistakes").
//!
//! This module does the pure work only. Idle / AC-power scheduling and real
//! engine dispatch are later wiring units; the engine is injected through
//! [`ReplayEngine`] so tests can use mocks.
//!
//! Note: `BoostTerm` is a re-export of `crate::rescoring::RescoreTerm` (the
//! crate's single boost-term shape). The engine-facing wiring bridges to
//! `teletype_speech::BoostTerm` by shape.

use std::path::Path;
use std::time::{Duration, Instant};

use super::store::{LearningStore, StoreResult};

/// One custom-vocabulary term to boost during a take. This is
/// [`crate::rescoring::RescoreTerm`] — the crate's single boost-term shape —
/// so the rescoring pass and replay consume the same lists.
pub use crate::rescoring::RescoreTerm as BoostTerm;

/// A transcription engine that can apply a boost list. Real engines plug in
/// later; tests use mocks.
pub trait ReplayEngine {
    fn transcribe(&self, audio: &Path, boost: &[BoostTerm]) -> Result<String, String>;
}

/// Tunables for one replay run.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Max clips to replay (newest first).
    pub max_clips: usize,
    /// Wall-clock budget; the run stops early once exceeded.
    pub budget: Duration,
    /// Cumulative false-alarm rate above which a term is demoted.
    pub demote_fa_rate: f64,
    /// Minimum cumulative false alarms required before demotion.
    pub demote_min_fa: i64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            max_clips: 200,
            budget: Duration::from_secs(60),
            demote_fa_rate: 0.3,
            demote_min_fa: 3,
        }
    }
}

/// A display-ready sentence for the user (e.g. "Stopped boosting 'Kate'
/// because it caused mistakes").
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayNotice {
    pub term: String,
    pub reason: String,
}

/// The outcome of one replay run.
#[derive(Debug, Clone, Default)]
pub struct ReplayReport {
    pub clips_processed: usize,
    pub clips_skipped: usize,
    pub wer_before: f64,
    pub wer_after: f64,
    pub hits_bumped: usize,
    pub false_alarms_bumped: usize,
    pub halved: Vec<String>,
    pub tombstoned: Vec<String>,
    pub notices: Vec<ReplayNotice>,
    pub cancelled: bool,
    pub elapsed_ms: u64,
}

// ── WER (verbatim normalization) ────────────────────────────────────────────

/// Verbatim normalization for WER: lowercase, replace every non-alphanumeric
/// with a space, collapse runs of spaces, trim. Empty input yields `""`.
///
/// The result is a space-separated token stream of alphanumeric "words".
/// Punctuation, case, and repeated separators are all stripped so WER is
/// insensitive to them.
pub fn normalize_verbatim(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true; // leading space suppressed
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    // Trim trailing space (prev_space==true means last char was a separator).
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Word-level Levenshtein distance between two whitespace-separated token
/// strings. `max(1, observed_words)` is used as the WER denominator
/// (callers' job), so this function returns the raw edit distance.
pub fn word_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<&str> = a.split_whitespace().collect();
    let b: Vec<&str> = b.split_whitespace().collect();
    lev(&a, &b)
}

fn lev(a: &[&str], b: &[&str]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// WER = word_levenshtein(transcript, observed) / max(1, observed_words).
/// Both inputs are verbatim-normalized internally.
pub fn word_error_rate(transcript: &str, observed: &str) -> f64 {
    let t = normalize_verbatim(transcript);
    let o = normalize_verbatim(observed);
    let obs_words = o.split_whitespace().count();
    if obs_words == 0 {
        // No observed words: WER is 0.0 if the transcript is also empty
        // after normalization, otherwise 1.0 (all transcript words are
        // insertions against an empty reference).
        return if t.is_empty() { 0.0 } else { 1.0 };
    }
    word_levenshtein(&t, &o) as f64 / obs_words as f64
}

// ── Term occurrence (word-boundary, case-insensitive) ──────────────────────

/// Returns true if `term` appears in `text` as a whole word, case-insensitively.
///
/// A "word" here is a maximal alphanumeric run. So `kate` matches "kate",
/// "Kate!", "kate's" (the apostrophe is a non-alnum boundary), and
/// "hi kate", but not "katherine". Empty or whitespace-only terms never
/// match.
pub fn contains_word(text: &str, term: &str) -> bool {
    let term = term.trim().to_ascii_lowercase();
    if term.is_empty() {
        return false;
    }
    // Tokenize `text` into alphanumeric runs (lowercased) and compare.
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens.contains(&term)
}

// ── Replay run ──────────────────────────────────────────────────────────────

/// Per-term counters accumulated across the replay run.
#[derive(Default)]
struct TermTally {
    hits: i64,
    false_alarms: i64,
}

/// Runs one replay pass over the most recent clips.
///
/// See the module docs for the overall contract. The function is pure with
/// respect to wall clock except for the `budget` enforcement (which uses
/// `Instant::now`); `now_ms` is the caller-supplied timestamp for the
/// personal-accuracy row.
pub fn run_replay(
    store: &LearningStore,
    engine: &dyn ReplayEngine,
    current: &[BoostTerm],
    previous: &[BoostTerm],
    cfg: &ReplayConfig,
    now_ms: u64,
    cancel: &dyn Fn() -> bool,
) -> StoreResult<ReplayReport> {
    let started = Instant::now();
    let mut report = ReplayReport::default();

    let clips = store.recent_dictations(cfg.max_clips as i64)?;

    // Per-term tallies, indexed by term text (deduped, order preserved).
    let mut term_order: Vec<String> = Vec::new();
    let mut tally: std::collections::HashMap<String, TermTally> = std::collections::HashMap::new();
    for t in current {
        let key = t.text.clone();
        if let std::collections::hash_map::Entry::Vacant(slot) = tally.entry(key.clone()) {
            term_order.push(key);
            slot.insert(TermTally::default());
        }
    }

    let mut wer_before_sum: f64 = 0.0;
    let mut wer_after_sum: f64 = 0.0;
    let mut processed: usize = 0;

    for row in &clips {
        // Cancel check before every clip.
        if cancel() {
            report.cancelled = true;
            break;
        }
        // Budget check (wall clock).
        if started.elapsed() > cfg.budget {
            report.cancelled = true;
            break;
        }

        // Eligibility: observed Some, audio_path Some, file exists.
        let (Some(observed), Some(audio_path)) = (&row.observed, &row.audio_path) else {
            report.clips_skipped += 1;
            continue;
        };
        if !Path::new(audio_path).exists() {
            report.clips_skipped += 1;
            continue;
        }

        // Transcribe twice: current boost (after) and previous boost (before).
        let transcript_after = match engine.transcribe(Path::new(audio_path), current) {
            Ok(t) => t,
            Err(_) => {
                report.clips_skipped += 1;
                continue;
            }
        };
        let transcript_before = match engine.transcribe(Path::new(audio_path), previous) {
            Ok(t) => t,
            Err(_) => {
                // Current succeeded but previous failed: count as skipped so
                // we do not half-process the clip.
                report.clips_skipped += 1;
                continue;
            }
        };

        wer_before_sum += word_error_rate(&transcript_before, observed);
        wer_after_sum += word_error_rate(&transcript_after, observed);

        // Per-term hit / false-alarm on the CURRENT transcript only.
        let observed_norm = normalize_verbatim(observed);
        for term in &term_order {
            let in_transcript = contains_word(&transcript_after, term);
            if !in_transcript {
                continue;
            }
            let in_observed = contains_word(&observed_norm, term);
            let t = tally.get_mut(term).unwrap();
            if in_observed {
                t.hits += 1;
            } else {
                t.false_alarms += 1;
            }
        }

        processed += 1;
    }

    report.clips_processed = processed;

    // WER means over processed clips (0.0 if none).
    if processed > 0 {
        report.wer_before = wer_before_sum / processed as f64;
        report.wer_after = wer_after_sum / processed as f64;
    }

    // Bump counters and demote / tombstone.
    let mut demote_candidates: Vec<String> = Vec::new();
    for term in &term_order {
        let t = &tally[term];
        if t.hits + t.false_alarms == 0 {
            continue;
        }
        if t.hits > 0 {
            store.bump_hits(term, t.hits)?;
            report.hits_bumped += 1;
        }
        if t.false_alarms > 0 {
            store.bump_false_alarms(term, t.false_alarms)?;
            report.false_alarms_bumped += 1;
        }
        demote_candidates.push(term.clone());
    }

    for term in demote_candidates {
        let row = match store.lexicon_by_term(&term)? {
            Some(r) => r,
            None => continue,
        };
        let total = row.hits + row.false_alarms;
        if total == 0 {
            continue;
        }
        let fa_rate = row.false_alarms as f64 / total as f64;
        if fa_rate > cfg.demote_fa_rate && row.false_alarms >= cfg.demote_min_fa {
            if row.weight > 0.5 {
                store.set_weight(&term, row.weight / 2.0)?;
                report.halved.push(term);
            } else {
                store.tombstone(&term)?;
                report.tombstoned.push(term.clone());
                report.notices.push(ReplayNotice {
                    term: term.clone(),
                    reason: format!("Stopped boosting '{term}' because it caused mistakes"),
                });
            }
        }
    }

    // Write the personal-accuracy row only when at least one clip was
    // processed (no clips → no row).
    if processed > 0 {
        store.record_personal_accuracy(
            now_ms,
            report.wer_before,
            report.wer_after,
            processed as i64,
        )?;
    }

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::store::{DictationRow, LearningStore, LexiconRow};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Engine that "hears" a name correctly iff the clip's observed text
    /// contains that name AND the name is in the boost list it was given.
    /// For each clip it emits: base words ("hey team") + every boost term
    /// that (a) it knows and (b) appears in the clip's observed text.
    /// This makes WER(current) < WER(previous) when `current` boosts the
    /// name and `previous` does not.
    struct MockEngine {
        /// Names this engine can hear when boosted.
        knows: Vec<String>,
        /// Per-clip observed text, indexed by call count.
        observed: Vec<String>,
        calls: std::sync::Mutex<usize>,
    }

    impl MockEngine {
        fn new(knows: Vec<String>, observed: Vec<String>) -> Self {
            Self {
                knows,
                observed,
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    impl ReplayEngine for MockEngine {
        fn transcribe(&self, _audio: &Path, boost: &[BoostTerm]) -> Result<String, String> {
            let clip_idx = {
                let mut c = self.calls.lock().unwrap();
                let i = *c;
                *c += 1;
                i % self.observed.len()
            };
            let observed = &self.observed[clip_idx];
            let mut words: Vec<String> = vec!["hey".into(), "team".into()];
            for t in boost {
                let known = self.knows.iter().any(|k| k.eq_ignore_ascii_case(&t.text));
                let in_clip = contains_word(observed, &t.text);
                if known && in_clip {
                    words.push(t.text.clone());
                }
            }
            Ok(words.join(" "))
        }
    }

    struct FailingEngine;
    impl ReplayEngine for FailingEngine {
        fn transcribe(&self, _audio: &Path, _boost: &[BoostTerm]) -> Result<String, String> {
            Err("engine down".into())
        }
    }

    /// Engine that returns a fixed transcript regardless of boost.
    struct FixedEngine {
        transcript: String,
    }
    impl ReplayEngine for FixedEngine {
        fn transcribe(&self, _audio: &Path, _boost: &[BoostTerm]) -> Result<String, String> {
            Ok(self.transcript.clone())
        }
    }

    fn make_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("replay.sqlite")).unwrap();
        (dir, store)
    }

    /// Creates a real temp audio file and returns its path.
    fn make_audio(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, b"fake-audio").unwrap();
        p
    }

    fn dictation_row(
        id: i64,
        ts: u64,
        audio: Option<PathBuf>,
        observed: Option<&str>,
    ) -> DictationRow {
        DictationRow {
            id,
            ts,
            app_id: "slack".into(),
            app_category: "chat".into(),
            engine: "whisper".into(),
            lang: "en".into(),
            audio_path: audio.map(|p| p.to_string_lossy().into_owned()),
            raw_asr: "hey team".into(),
            words_json: r#"["hey","team"]"#.into(),
            pasted: "hey team".into(),
            observed: observed.map(|s| s.to_string()),
            edit_ops_json: None,
            polish_model: None,
            polish_skipped: true,
        }
    }

    fn lex_row(term: &str, weight: f64) -> LexiconRow {
        LexiconRow {
            term: term.into(),
            aliases: vec![],
            source: "learned".into(),
            weight,
            hits: 0,
            false_alarms: 0,
            created: 1_000,
            last_used: None,
            tombstoned: false,
            app_scope: None,
        }
    }

    fn boost(text: &str) -> BoostTerm {
        BoostTerm {
            text: text.into(),
            aliases: vec![],
            weight: 1.0,
        }
    }

    fn no_cancel() -> impl Fn() -> bool {
        || false
    }

    #[test]
    fn normalize_verbatim_strips_case_punct_and_collapses() {
        assert_eq!(normalize_verbatim("Hello, World!"), "hello world");
        assert_eq!(normalize_verbatim("  a   b  "), "a b");
        assert_eq!(normalize_verbatim("KATE's-Number"), "kate s number");
        assert_eq!(normalize_verbatim("!!!"), "");
        assert_eq!(normalize_verbatim(""), "");
        assert_eq!(normalize_verbatim("café"), "café");
    }

    #[test]
    fn wer_table() {
        // Empty observed: 0.0 if transcript also empty, else 1.0.
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("hey", ""), 1.0);
        // Exact match (case/punct-insensitive).
        assert_eq!(word_error_rate("Hello, world!", "hello world"), 0.0);
        // One substitution in 2 observed words → 0.5.
        assert!((word_error_rate("hey kate", "hey team") - 0.5).abs() < 1e-9);
        // Insertion: transcript has an extra word, observed 2 words → 0.5.
        assert!((word_error_rate("hey extra team", "hey team") - 0.5).abs() < 1e-9);
        // Deletion: transcript missing a word.
        assert!((word_error_rate("hey", "hey team") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn word_levenshtein_basic() {
        assert_eq!(word_levenshtein("a b", "a b"), 0);
        assert_eq!(word_levenshtein("a b", "a c"), 1);
        assert_eq!(word_levenshtein("", "a b"), 2);
        assert_eq!(word_levenshtein("a b c", ""), 3);
        assert_eq!(word_levenshtein("a b", "b a"), 2);
    }

    #[test]
    fn contains_word_word_boundary_case_insensitive() {
        assert!(contains_word("hi Kate!", "kate"));
        assert!(contains_word("kate's number", "kate"));
        assert!(contains_word("KATE", "kate"));
        assert!(!contains_word("katherine", "kate"));
        assert!(!contains_word("hey team", "kate"));
        assert!(!contains_word("anything", ""));
        assert!(!contains_word("anything", "   "));
    }

    #[test]
    fn engine_errors_count_as_skipped_not_fatal() {
        // A failing engine must not abort the run: the clip is skipped.
        let (dir, store) = make_store();
        let audio = make_audio(&dir, "a0.m4a");
        store
            .record_dictation(&dictation_row(0, 100, Some(audio), Some("hey team kate")))
            .unwrap();
        let report = run_replay(
            &store,
            &FailingEngine,
            &[boost("Kate")],
            &[],
            &ReplayConfig::default(),
            999,
            &no_cancel(),
        )
        .unwrap();
        assert_eq!(report.clips_processed, 0);
        assert_eq!(report.clips_skipped, 1);
        assert!(report.notices.is_empty());
    }

    #[test]
    fn wer_improves_when_current_boosts_the_name() {
        let (dir, store) = make_store();
        // 3 clips, each observed contains "Kate".
        for i in 0..3 {
            let audio = make_audio(&dir, &format!("a{i}.m4a"));
            store
                .record_dictation(&dictation_row(
                    0,
                    100 + i as u64,
                    Some(audio),
                    Some("hey team kate"),
                ))
                .unwrap();
        }
        let engine = MockEngine::new(vec!["Kate".into()], vec!["hey team kate".into(); 3]);
        let current = vec![boost("Kate")];
        let previous: Vec<BoostTerm> = vec![];
        let cfg = ReplayConfig::default();
        let report = run_replay(
            &store,
            &engine,
            &current,
            &previous,
            &cfg,
            999,
            &no_cancel(),
        )
        .unwrap();

        assert_eq!(report.clips_processed, 3);
        assert_eq!(report.clips_skipped, 0);
        // current boosts Kate → hears it → WER 0.0; previous doesn't →
        // transcript "hey team" vs observed "hey team kate" → WER 1/3.
        assert!(
            (report.wer_after - 0.0).abs() < 1e-9,
            "after={}",
            report.wer_after
        );
        assert!(
            (report.wer_before - 1.0 / 3.0).abs() < 1e-9,
            "before={}",
            report.wer_before
        );
        assert!(report.wer_after < report.wer_before);
        // Kate hit on all 3 clips.
        assert_eq!(report.hits_bumped, 1);
        // Kate was never in the lexicon in this test, so no row exists.
        assert!(store.lexicon_by_term("Kate").unwrap().is_none());
    }

    #[test]
    fn hits_and_false_alarms_bump_cumulatively_across_runs() {
        let (dir, store) = make_store();
        store.upsert_lexicon(&lex_row("Kate", 1.0)).unwrap();
        for i in 0..2 {
            let audio = make_audio(&dir, &format!("c{i}.m4a"));
            store
                .record_dictation(&dictation_row(
                    0,
                    100 + i as u64,
                    Some(audio),
                    Some("hey team kate"),
                ))
                .unwrap();
        }
        let engine = MockEngine::new(vec!["Kate".into()], vec!["hey team kate".into(); 2]);
        let current = vec![boost("Kate")];
        let cfg = ReplayConfig::default();

        // Run 1: 2 clips, Kate in observed → 2 hits.
        let r1 = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();
        assert_eq!(r1.hits_bumped, 1);
        assert_eq!(r1.false_alarms_bumped, 0);
        let row1 = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row1.hits, 2);
        assert_eq!(row1.false_alarms, 0);

        // Run 2: same clips again → 2 more hits (cumulative).
        let r2 = run_replay(&store, &engine, &current, &[], &cfg, 2000, &no_cancel()).unwrap();
        assert_eq!(r2.hits_bumped, 1);
        let row2 = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row2.hits, 4, "hits accumulate across runs");
        assert_eq!(row2.false_alarms, 0);
    }

    #[test]
    fn hallucinated_boosted_term_counts_as_false_alarm() {
        let (dir, store) = make_store();
        store.upsert_lexicon(&lex_row("Kate", 1.0)).unwrap();
        let audio = make_audio(&dir, "h.m4a");
        // observed does NOT contain Kate.
        store
            .record_dictation(&dictation_row(0, 100, Some(audio), Some("hey team")))
            .unwrap();
        // Engine "hears" Kate even though the user didn't say it (hallucination).
        let engine = FixedEngine {
            transcript: "hey team kate".into(),
        };
        let current = vec![boost("Kate")];
        let cfg = ReplayConfig {
            demote_fa_rate: 0.3,
            demote_min_fa: 3,
            ..Default::default()
        };
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();
        assert_eq!(report.clips_processed, 1);
        assert_eq!(report.false_alarms_bumped, 1);
        assert_eq!(report.hits_bumped, 0);
        let row = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row.false_alarms, 1);
        assert_eq!(row.hits, 0);
    }

    #[test]
    fn term_correct_in_both_lists_only_hits() {
        let (dir, store) = make_store();
        store.upsert_lexicon(&lex_row("Kate", 1.0)).unwrap();
        let audio = make_audio(&dir, "ok.m4a");
        store
            .record_dictation(&dictation_row(0, 100, Some(audio), Some("hey team kate")))
            .unwrap();
        let engine = FixedEngine {
            transcript: "hey team kate".into(),
        };
        let current = vec![boost("Kate")];
        let cfg = ReplayConfig::default();
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();
        assert_eq!(report.hits_bumped, 1);
        assert_eq!(report.false_alarms_bumped, 0);
        let row = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row.hits, 1);
        assert_eq!(row.false_alarms, 0);
        // Clean term: not halved, not tombstoned.
        assert!(report.halved.is_empty());
        assert!(report.tombstoned.is_empty());
    }

    /// A clip whose observed text has no boosted term, so the current
    /// transcript (which does contain the term) counts as a false alarm.
    fn fa_clip(dir: &tempfile::TempDir, store: &LearningStore, _id: i64, ts: u64, name: &str) {
        let audio = make_audio(dir, name);
        store
            .record_dictation(&dictation_row(0, ts, Some(audio), Some("hey team")))
            .unwrap();
    }

    #[test]
    fn bad_term_halved_then_tombstoned_with_notice() {
        let (dir, store) = make_store();
        // Pre-seed weight 1.0.
        store.upsert_lexicon(&lex_row("Kate", 1.0)).unwrap();
        // 3 clips, observed has no Kate; engine always emits Kate.
        for i in 0..3 {
            fa_clip(&dir, &store, 0, 100 + i as u64, &format!("f{i}.m4a"));
        }
        let engine = FixedEngine {
            transcript: "hey team kate".into(),
        };
        let current = vec![boost("Kate")];
        let cfg = ReplayConfig {
            demote_fa_rate: 0.3,
            demote_min_fa: 3,
            ..Default::default()
        };

        // Run 1: 3 false alarms, fa_rate = 3/3 = 1.0 > 0.3, fa >= 3 → halved.
        let r1 = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();
        assert_eq!(r1.false_alarms_bumped, 1);
        assert_eq!(r1.halved, vec!["Kate".to_string()]);
        assert!(r1.tombstoned.is_empty());
        let row1 = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row1.false_alarms, 3);
        assert!(
            (row1.weight - 0.5).abs() < 1e-9,
            "weight halved: {}",
            row1.weight
        );

        // Run 2: 3 more false alarms (cumulative 6), weight 0.5 (not > 0.5)
        // → tombstoned + notice.
        let r2 = run_replay(&store, &engine, &current, &[], &cfg, 2000, &no_cancel()).unwrap();
        assert!(r2.halved.is_empty());
        assert_eq!(r2.tombstoned, vec!["Kate".to_string()]);
        assert_eq!(r2.notices.len(), 1);
        assert_eq!(r2.notices[0].term, "Kate");
        assert!(r2.notices[0].reason.contains("Kate"));
        let row2 = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert!(row2.tombstoned);
        assert_eq!(row2.false_alarms, 6);
    }

    #[test]
    fn clean_term_untouched_and_tombstoned_not_rebumped() {
        let (dir, store) = make_store();
        store.upsert_lexicon(&lex_row("Clean", 1.0)).unwrap();
        // Pre-seed a tombstoned term that is NOT in `current`.
        let mut t = lex_row("Old", 1.0);
        t.tombstoned = true;
        store.upsert_lexicon(&t).unwrap();

        let audio = make_audio(&dir, "clean.m4a");
        store
            .record_dictation(&dictation_row(0, 100, Some(audio), Some("hey team clean")))
            .unwrap();
        let engine = FixedEngine {
            transcript: "hey team clean".into(),
        };
        let current = vec![boost("Clean")]; // Old not in current.
        let cfg = ReplayConfig::default();
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();

        // Clean: hit, untouched weight, not tombstoned.
        let clean = store.lexicon_by_term("Clean").unwrap().unwrap();
        assert_eq!(clean.hits, 1);
        assert!((clean.weight - 1.0).abs() < 1e-9);
        assert!(!clean.tombstoned);
        // Old: tombstoned and not in current → not re-bumped.
        let old = store.lexicon_by_term("Old").unwrap().unwrap();
        assert!(old.tombstoned);
        assert_eq!(old.hits, 0);
        assert_eq!(old.false_alarms, 0);
        assert!(report.halved.is_empty());
        assert!(report.tombstoned.is_empty());
    }

    #[test]
    fn missing_audio_and_engine_error_are_skipped_but_run_continues() {
        let (dir, store) = make_store();
        store.upsert_lexicon(&lex_row("Kate", 1.0)).unwrap();
        // Clip 0: audio path does not exist.
        store
            .record_dictation(&dictation_row(
                0,
                100,
                Some(dir.path().join("nope.m4a")),
                Some("hey team kate"),
            ))
            .unwrap();
        // Clip 1: valid audio, engine will fail on it.
        let audio = make_audio(&dir, "ok.m4a");
        store
            .record_dictation(&dictation_row(0, 200, Some(audio), Some("hey team kate")))
            .unwrap();
        // Clip 2: no observed.
        store
            .record_dictation(&dictation_row(
                0,
                300,
                Some(make_audio(&dir, "c2.m4a")),
                None,
            ))
            .unwrap();
        // Clip 3: valid, engine ok.
        let audio3 = make_audio(&dir, "ok3.m4a");
        store
            .record_dictation(&dictation_row(0, 400, Some(audio3), Some("hey team kate")))
            .unwrap();

        // Engine fails on the 2nd call (clip 1's current transcribe) only —
        // simplest: fail all, then clip 0 skipped (missing) and clip 3 also
        // fails. Use a counting engine to fail exactly clip 1.
        struct CountFail {
            fail_clip: usize,
            calls: std::sync::Mutex<usize>,
        }
        impl ReplayEngine for CountFail {
            fn transcribe(&self, _a: &Path, _b: &[BoostTerm]) -> Result<String, String> {
                let mut c = self.calls.lock().unwrap();
                let i = *c;
                *c += 1;
                // Each clip makes 2 calls (current, previous). Clip 1 is the
                // 2nd clip → calls 2 (current) / 3 (previous).
                if i / 2 == 1 {
                    return Err("boom".into());
                }
                Ok("hey team kate".into())
            }
        }
        let engine = CountFail {
            fail_clip: 1,
            calls: std::sync::Mutex::new(0),
        };
        let current = vec![boost("Kate")];
        let cfg = ReplayConfig::default();
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();

        // clip0 missing → skipped; clip1 engine err → skipped; clip2 no
        // observed → skipped; clip3 ok → processed.
        assert_eq!(report.clips_processed, 1);
        assert_eq!(report.clips_skipped, 3);
        assert!(!report.cancelled);
        // Kate hit on clip3.
        let row = store.lexicon_by_term("Kate").unwrap().unwrap();
        assert_eq!(row.hits, 1);
    }

    #[test]
    fn cancel_stops_early_and_reports_cancelled() {
        let (dir, store) = make_store();
        for i in 0..5 {
            let audio = make_audio(&dir, &format!("x{i}.m4a"));
            store
                .record_dictation(&dictation_row(
                    0,
                    100 + i as u64,
                    Some(audio),
                    Some("hey team"),
                ))
                .unwrap();
        }
        let engine = FixedEngine {
            transcript: "hey team".into(),
        };
        let current: Vec<BoostTerm> = vec![];
        let cfg = ReplayConfig::default();
        let processed = Arc::new(AtomicUsize::new(0));
        let p = processed.clone();
        let cancel = move || {
            p.fetch_add(1, Ordering::SeqCst) >= 2 // stop after 2 clips
        };
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &cancel).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.clips_processed, 2, "stopped early");
        assert!(report.clips_processed < 5);
    }

    #[test]
    fn budget_enforced_stops_early() {
        let (dir, store) = make_store();
        for i in 0..5 {
            let audio = make_audio(&dir, &format!("b{i}.m4a"));
            store
                .record_dictation(&dictation_row(
                    0,
                    100 + i as u64,
                    Some(audio),
                    Some("hey team"),
                ))
                .unwrap();
        }
        let engine = FixedEngine {
            transcript: "hey team".into(),
        };
        let current: Vec<BoostTerm> = vec![];
        let cfg = ReplayConfig {
            budget: Duration::from_nanos(1),
            ..Default::default()
        };
        let report = run_replay(&store, &engine, &current, &[], &cfg, 1000, &no_cancel()).unwrap();
        // Budget of 1ns is exceeded almost immediately → cancelled, few/zero
        // clips processed.
        assert!(report.cancelled);
        assert!(report.clips_processed < 5);
    }

    #[test]
    fn personal_accuracy_row_written_with_means_and_no_row_when_no_clips() {
        let (dir, store) = make_store();
        // No clips at all → no row.
        let engine = FixedEngine {
            transcript: "hey".into(),
        };
        let cfg = ReplayConfig::default();
        let r = run_replay(&store, &engine, &[], &[], &cfg, 1234, &no_cancel()).unwrap();
        assert_eq!(r.clips_processed, 0);
        assert!(store.personal_accuracy_history(10).unwrap().is_empty());

        // One clip → row with the means.
        let audio = make_audio(&dir, "pa.m4a");
        store
            .record_dictation(&dictation_row(0, 100, Some(audio), Some("hey team kate")))
            .unwrap();
        // current hears kate (WER 0), previous doesn't (WER 0.5).
        let engine2 = MockEngine::new(vec!["Kate".into()], vec!["hey team kate".into()]);
        let current = vec![boost("Kate")];
        let r2 = run_replay(&store, &engine2, &current, &[], &cfg, 1234, &no_cancel()).unwrap();
        assert_eq!(r2.clips_processed, 1);
        let hist = store.personal_accuracy_history(10).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].ts, 1234);
        assert_eq!(hist[0].clips, 1);
        assert!((hist[0].wer_after - 0.0).abs() < 1e-9);
        assert!((hist[0].wer_before - 1.0 / 3.0).abs() < 1e-9);
    }
}
