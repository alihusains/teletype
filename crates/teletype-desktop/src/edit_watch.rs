//! Watching what the user does to a dictation after it lands.
//!
//! # Why this exists
//!
//! The personalization loop can only learn from a signal if someone observes
//! the user's edits. There is no edit event from a text field: fields do not
//! notify, and key events are global, not per-field. So the only way to see
//! an edit is to read the field back and compare it against what we wrote.
//!
//! # Two layers
//!
//! 1. **UI hint layer (old, unchanged semantics):** the word-diff heuristics
//!    below (`diff`, `EditDiff`, `was_edited`) drive the "edit watch" hint and
//!    the legacy preference-learning read-back. They stay conservative: a
//!    wrong signal teaches the model a preference the user never expressed.
//!
//! 2. **Learning-store layer (Phase 7.2):** at paste we snapshot a
//!    [`PasteAnchor`] (≤ 40 chars before/after the insertion point) and the
//!    store row id. `ObserverState` (frozen core API) decides when to read
//!    the field back; `locate_observed` re-finds the pasted span by fuzzy
//!    anchors; the located text plus a `diff_words` op list is written to the
//!    learning store via [`LearningSink`]. Never learn from a guess: if the
//!    span cannot be located within the distance budget, the observation is
//!    dropped and counted, never written.

use std::time::{Duration, Instant};

use teletype_core::learning::diff::diff_words;
use teletype_core::learning::observer::{
    clip_anchor, locate_observed, ObserverDecision, ObserverEvent, ObserverState, PasteAnchor,
};

/// How long after an insertion we are still willing to consider the field
/// edited. Long enough for a human to fix a sign-off, short enough that a
/// field left open overnight is not treated as an edit made in context.
const WATCH_WINDOW: Duration = Duration::from_secs(15 * 60);

/// How many edits an anchor (or the pasted span itself) may drift by and
/// still count as the same span. The user may have fixed a misheard word;
/// anything further is a different piece of text, not an edit of ours.
pub const LOCATE_MAX_DIST: usize = 3;

/// Minimum wall-clock age (ms) a cached [`LastGoodRead`] must have before
/// the observe path may fall back on it. A read taken in the same tick as a
/// keystroke can catch half a word; one at least this old has had time to
/// settle.
pub const FLUSH_MIN_QUIET_MS: i64 = 1_000;

/// Anchor clip width in characters, per the core API contract.
pub const ANCHOR_CHARS: usize = 40;

/// An insertion we are watching for changes.
#[derive(Debug, Clone)]
pub struct PendingEdit {
    /// The exact text we inserted.
    pub inserted: String,
    /// The app we inserted into, as a normalised application name.
    pub target: String,
    /// When the insertion happened.
    pub at: Instant,
    /// The paste anchor for the learning-store observation, when it could
    /// be captured. `None` means the observation is skipped entirely.
    pub anchor: Option<PasteAnchor>,
    /// The row id in the learning store, when the take was recorded.
    pub dictation_id: Option<i64>,
    /// Wall-clock ms of the insertion (for the observer clock; `at` is
    /// monotonic and cannot be polled against wall time).
    pub recorded_at_ms: i64,
}

/// The most recent field read that still located our paste and differed
/// from what we inserted. Memory only, never persisted: it exists so a
/// field that empties after the user's fix (a sent message) can still be
/// learned from. Dropped with the pending edit.
#[derive(Debug, Clone, PartialEq)]
pub struct LastGoodRead {
    pub observed: String,
    /// Wall-clock ms when the read was taken (`edit_watch::now_ms`).
    pub at_ms: i64,
}

/// The most recent insertion, if it is still worth checking.
#[derive(Debug, Clone, Default)]
pub struct EditWatch {
    pending: Option<PendingEdit>,
    /// Trigger state machine for the learning-store observation. Armed on
    /// the paste that set `pending`; final after Observe/Drop.
    observer: Option<ObserverState>,
    /// Flush fallback for the observe read, when the field no longer
    /// resembles the paste (sent away). Cleared wherever `pending` is
    /// cleared or replaced.
    last_good: Option<LastGoodRead>,
}

impl EditWatch {
    /// Records an insertion to watch (UI hint layer only; no anchor, no
    /// store id). Kept for the scratchpad path and existing tests.
    pub fn record(&mut self, inserted: String, target: String) {
        if inserted.trim().is_empty() {
            self.pending = None;
            return;
        }
        self.pending = Some(PendingEdit {
            inserted,
            target,
            at: Instant::now(),
            anchor: None,
            dictation_id: None,
            recorded_at_ms: 0,
        });
        self.observer = None;
        self.last_good = None;
    }

    /// Records a paste for the learning-store observation (Phase 7.2).
    ///
    /// `anchor` is the paste-time snapshot (≤ 40 chars each side); when it
    /// is `None` the watch still drives the UI hint but the observation is
    /// dropped — we never guess where the pasted span is.
    /// `dictation_id` is the store row for this take, when the recording
    /// succeeded; `None` disables the store write but keeps the hint.
    pub fn record_watch(
        &mut self,
        inserted: String,
        anchor: Option<PasteAnchor>,
        dictation_id: Option<i64>,
        now_ms: i64,
    ) {
        if inserted.trim().is_empty() {
            self.pending = None;
            return;
        }
        self.pending = Some(PendingEdit {
            target: String::new(),
            inserted,
            anchor,
            dictation_id,
            at: Instant::now(),
            recorded_at_ms: now_ms,
        });
        self.last_good = None;
        let mut observer = ObserverState::new(now_ms);
        observer.on_event(ObserverEvent::Paste, now_ms);
        self.observer = Some(observer);
    }

    /// Stores the read as the flush fallback when it locates our paste and
    /// the observed text differs from what we inserted.
    pub fn note_field_read(&mut self, field: &str, now_ms: i64) {
        let Some(pending) = self.pending.as_ref() else {
            return;
        };
        let Some(anchor) = pending.anchor.as_ref() else {
            return;
        };
        let Some(located) = locate_observed(field, anchor, LOCATE_MAX_DIST) else {
            return;
        };
        // An unchanged field is not an edit; caching it would only flush
        // our own text back into the store.
        if located.observed.trim() == pending.inserted.trim() {
            return;
        }
        self.last_good = Some(LastGoodRead {
            observed: located.observed,
            at_ms: now_ms,
        });
    }

    /// The cached fallback read, if one exists.
    pub fn last_good(&self) -> Option<&LastGoodRead> {
        self.last_good.as_ref()
    }

    /// Fills in the target app name once it is known (the paste site knows
    /// it; the record site may not).
    pub fn set_target(&mut self, target: String) {
        if let Some(p) = &mut self.pending {
            p.target = target;
        }
    }

    pub fn pending(&self) -> Option<&PendingEdit> {
        self.pending.as_ref()
    }

    /// Mutable access to the observer (for the trigger-state machine).
    pub fn observer_mut(&mut self) -> Option<&mut ObserverState> {
        self.observer.as_mut()
    }

    /// Whether there is something to check, and whether the watch has expired.
    pub fn status(&self) -> EditWatchStatus {
        match &self.pending {
            None => EditWatchStatus::Idle,
            Some(p) if p.at.elapsed() > WATCH_WINDOW => EditWatchStatus::Expired,
            Some(_) => EditWatchStatus::Watching,
        }
    }

    /// Clears the pending watch and observer without recording anything.
    pub fn clear(&mut self) {
        self.pending = None;
        self.observer = None;
        self.last_good = None;
    }

    /// Takes the pending insertion, if it is still fresh.
    ///
    /// Returning it means the caller is responsible for calling
    /// [`Self::finish`] with the outcome, so a failed read does not silently
    /// discard the observation.
    pub fn take(&mut self) -> Option<PendingEdit> {
        let taken = match self.status() {
            EditWatchStatus::Watching => self.pending.take(),
            _ => {
                self.pending = None;
                None
            }
        };
        self.last_good = None;
        taken
    }

    /// Clears the watch without recording anything.
    pub fn finish(&mut self) {
        self.pending = None;
        self.last_good = None;
    }

    /// Replaces the observer state (used by the poll path to write back the
    /// updated state machine after each tick).
    pub fn set_observer(&mut self, observer: Option<ObserverState>) {
        self.observer = observer;
    }
}
/// What the watch is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditWatchStatus {
    /// Nothing pending.
    Idle,
    /// An insertion is being watched.
    Watching,
    /// The insertion is too old to attribute an edit to.
    Expired,
}

/// The outcome of one observation attempt (learning-store layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveOutcome {
    /// The span was located and the store was updated (or would have been,
    /// with no sink).
    Recorded,
    /// The field was not readable (no AX access, secure field, ...).
    FieldUnreadable,
    /// The field was readable but the pasted span could not be located
    /// within the distance budget: dropped, counted, never written.
    NotLocated,
    /// Nothing to observe (no anchor and no store id on the watch).
    Skipped,
}

/// Where an observation writes its result. A thin seam so the glue below
/// is testable with a fake sink and usable with the real `LearningStore`.
pub trait LearningSink {
    fn set_observed(&self, id: i64, observed: &str, edit_ops_json: Option<&str>) -> bool;
}

/// The real sink: the learning store. The session re-opens the store
/// lazily on each observe (a cheap `open()` on an existing file), so the
/// sink owns its own handle for the duration of the call. Failures log
/// and return false; they never propagate into the paste path.
pub struct StoreSink {
    store: teletype_core::learning::store::LearningStore,
}

impl StoreSink {
    /// Wraps an already-open store handle.
    pub fn new(store: teletype_core::learning::store::LearningStore) -> Self {
        Self { store }
    }

    /// Opens a fresh handle on the same file. `open` on an existing file is
    /// cheap (WAL already created at first open).
    pub fn open(path: &std::path::Path) -> Option<Self> {
        teletype_core::learning::store::LearningStore::open(path)
            .ok()
            .map(|store| Self { store })
    }

    /// Reclaims the store from the sink after the observation call.
    pub fn into_store(self) -> teletype_core::learning::store::LearningStore {
        self.store
    }
}

impl LearningSink for StoreSink {
    fn set_observed(&self, id: i64, observed: &str, edit_ops_json: Option<&str>) -> bool {
        match self.store.set_observed(id, observed, edit_ops_json) {
            Ok(()) => true,
            Err(e) => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("learning store: set_observed failed: {e}"),
                );
                false
            }
        }
    }
}

impl StoreSink {
    /// U3: settle every dictation row that is past its 30-minute window.
    /// Called from the observation flush — the moment the session already
    /// talks to the learning store — so the sweep needs no timer of its own.
    /// Failures log; they never propagate into the paste path.
    pub fn settle_sweep(&self, now_ms: u64) {
        match teletype_core::learning::settle::sweep(&self.store, now_ms) {
            Ok(r) if r.settled > 0 => crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "learning settle: {} rows judged, {} spans unlocatable",
                    r.settled, r.unlocatable
                ),
            ),
            Ok(_) => {}
            Err(e) => crate::log_entry(
                crate::LogLevel::Warn,
                format!("learning settle: sweep failed: {e}"),
            ),
        }
    }
}

/// The result of reading the focused field at observe time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldRead {
    /// The field is readable and holds `text`.
    Readable(String),
    /// The field cannot be read (no permission, no field, secure field).
    Unreadable,
}

/// Encodes a word-level [`EditOp`] list as the JSON shape the learning
/// store's `edit_ops_json` column carries. The core `EditOp` type is frozen
/// without serde derives, so the on-disk encoding lives here, at the only
/// site that must serialise it.
pub fn encode_edit_ops(ops: &[teletype_core::learning::diff::EditOp]) -> String {
    let items: Vec<serde_json::Value> = ops
        .iter()
        .map(|op| match op {
            teletype_core::learning::diff::EditOp::Equal(words) => {
                serde_json::json!({ "op": "equal", "words": words })
            }
            teletype_core::learning::diff::EditOp::Replace { from, to } => {
                serde_json::json!({ "op": "replace", "from": from, "to": to })
            }
            teletype_core::learning::diff::EditOp::Insert(words) => {
                serde_json::json!({ "op": "insert", "words": words })
            }
            teletype_core::learning::diff::EditOp::Delete(words) => {
                serde_json::json!({ "op": "delete", "words": words })
            }
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
}

/// Builds the paste anchor from focused-field surroundings read at paste
/// time. `before`/`after` are the text around the caret; the pasted text is
/// spliced in between them, so `before` is everything before the insertion
/// point and `after` starts right after the pasted span.
///
/// Returns `None` when the surroundings are unknowable (the read failed),
/// in which case the caller must skip the observation rather than guess.
pub fn build_anchor(
    before: Option<&str>,
    after: Option<&str>,
    pasted: &str,
) -> Option<PasteAnchor> {
    let (before, after) = (before?, after?);
    Some(PasteAnchor {
        before: clip_anchor(before, ANCHOR_CHARS),
        after: clip_anchor(after, ANCHOR_CHARS),
        pasted: pasted.to_string(),
    })
}

/// Feeds one trigger event into the observer, then — if it says "observe
/// now" — reads the field, locates the pasted span, and writes the result
/// to the sink. This is the whole decision→observe glue; it is pure over
/// its arguments so tests drive it with a fake field reader and sink.
///
/// `pending` must be the watch that is armed (already taken out of the
/// [`EditWatch`] by the caller). Returns the outcome so the caller can log
/// a counter for dropped observations.
pub fn observe_once(
    observer: &mut ObserverState,
    event: ObserverEvent,
    now_ms: i64,
    pending: &PendingEdit,
    read_field: &dyn Fn() -> FieldRead,
    sink: Option<&dyn LearningSink>,
    last_good: Option<&LastGoodRead>,
) -> Option<ObserveOutcome> {
    let decision = observer.on_event(event, now_ms);
    observe_decision(decision, now_ms, pending, read_field, sink, last_good)
}

/// The same, for the periodic poll (20 s idle / 3 min cap).
pub fn poll_observer(
    observer: &mut ObserverState,
    now_ms: i64,
    pending: &PendingEdit,
    read_field: &dyn Fn() -> FieldRead,
    sink: Option<&dyn LearningSink>,
    last_good: Option<&LastGoodRead>,
) -> Option<ObserveOutcome> {
    let decision = observer.poll(now_ms);
    observe_decision(decision, now_ms, pending, read_field, sink, last_good)
}

/// Maps an [`ObserverDecision`] to an action. `Wait` yields `None` (still
/// observing); `Drop` yields [`ObserveOutcome::Skipped`]; `Observe` runs
/// the read→locate→write sequence. When the read cannot locate the paste
/// (the field emptied because the user sent it), `last_good` is the flush
/// fallback: the most recent read that still located our paste.
fn observe_decision(
    decision: ObserverDecision,
    now_ms: i64,
    pending: &PendingEdit,
    read_field: &dyn Fn() -> FieldRead,
    sink: Option<&dyn LearningSink>,
    last_good: Option<&LastGoodRead>,
) -> Option<ObserveOutcome> {
    match decision {
        ObserverDecision::Wait => None,
        ObserverDecision::Drop => Some(ObserveOutcome::Skipped),
        ObserverDecision::Observe => {
            // No anchor and no store id: there is nothing to locate and
            // nothing to write to.
            let (Some(anchor), Some(id)) = (&pending.anchor, pending.dictation_id) else {
                return Some(ObserveOutcome::Skipped);
            };
            let field = match read_field() {
                FieldRead::Readable(t) => t,
                FieldRead::Unreadable => return Some(ObserveOutcome::FieldUnreadable),
            };
            let observed = match locate_observed(&field, anchor, LOCATE_MAX_DIST) {
                Some(located) => located.observed,
                None => match last_good {
                    // Only flush a read that had time to settle: a read taken in the
                    // same tick as a keystroke can catch half a word.
                    Some(lg) if now_ms - lg.at_ms >= FLUSH_MIN_QUIET_MS => lg.observed.clone(),
                    _ => return Some(ObserveOutcome::NotLocated),
                },
            };
            let ops = diff_words(&pending.inserted, &observed);
            let ops_json = Some(encode_edit_ops(&ops));
            let Some(sink) = sink else {
                // No sink (learning disabled for the session): the
                // observation ran but had nowhere to go.
                return Some(ObserveOutcome::Recorded);
            };
            let written = sink.set_observed(id, &observed, ops_json.as_deref());
            if written {
                Some(ObserveOutcome::Recorded)
            } else {
                Some(ObserveOutcome::Skipped)
            }
        }
    }
}

/// What comparing the field against our insertion tells us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditDiff {
    /// The field still holds exactly what we inserted. Not an edit.
    Unchanged,
    /// The field starts with what we inserted and continues, so the user
    /// appended to it. This is the common case and carries no preference
    /// signal, because we cannot tell an addition from a rewrite.
    Appended,
    /// The field starts with what we inserted up to `at`, then differs. The
    /// text after `at` is the user's edit.
    Edited { at: usize, replacement: String },
    /// The exact inserted text is gone, but the field still looks like an
    /// in-place edit of our dictation. `current` is the whole current value,
    /// which is the user's version of the dictation.
    InPlace { current: String },
    /// The text we inserted is not in the field at all: the user replaced it,
    /// the field was cleared, or focus moved somewhere else. Not learnable.
    Unrecognisable,
}

/// Compares the current field value against what we inserted.
///
/// `value` is the whole field, which may contain text the user had before we
/// inserted. The comparison is anchored on our own text, so a field with prior
/// content still works.
pub fn diff(value: &str, inserted: &str) -> EditDiff {
    if inserted.is_empty() {
        return EditDiff::Unrecognisable;
    }
    if let Some(at) = value.find(inserted) {
        let after = &value[at + inserted.len()..];
        if after.is_empty() {
            return EditDiff::Unchanged;
        }
        // Whitespace-only growth is a trailing newline or a space, not an edit.
        if after.trim().is_empty() {
            return EditDiff::Appended;
        }
        return EditDiff::Edited {
            at,
            replacement: after.to_string(),
        };
    }
    // The exact inserted text is gone: the user edited it in place (e.g.
    // fixed a misheard word). Detect via word-level alignment: first word
    // matches and word counts are close → in-place edit. `InPlace` carries
    // the whole current value; `edited_text` hands it to the caller directly.
    if in_place_edit(value, inserted) {
        return EditDiff::InPlace {
            current: value.to_string(),
        };
    }
    EditDiff::Unrecognisable
}

/// True when `value` looks like an in-place edit of `inserted`: the word
/// counts differ by ≤ 3 and either the first word matches (case-insensitive)
/// or the two texts are phonetically close.
fn in_place_edit(value: &str, inserted: &str) -> bool {
    if value.trim().is_empty() {
        return false;
    }
    let (Some(first_ins), Some(first_val)) = (
        inserted.split_whitespace().next(),
        value.split_whitespace().next(),
    ) else {
        return false;
    };
    let ins_count = inserted.split_whitespace().count();
    let val_count = value.split_whitespace().count();
    if ins_count.abs_diff(val_count) > 3 {
        return false;
    }
    if first_ins.eq_ignore_ascii_case(first_val) {
        return true;
    }
    // A misheard name is often the first word of the take, so the most
    // common correction replaces the first word. Accept it when the two
    // texts are phonetically close (the same band the learning judge uses);
    // an unrelated replacement stays Unrecognisable.
    const MIN_PHONETIC: f32 = 0.6;
    teletype_core::learning::judge::phon_sim(inserted, value) >= MIN_PHONETIC
}

impl EditDiff {
    /// Whether this diff represents a user edit (as opposed to unchanged or
    /// unrecognisable).
    pub fn was_edited(&self) -> bool {
        matches!(self, EditDiff::Edited { .. } | EditDiff::InPlace { .. })
    }

    /// The user's version of the dictation, when this diff represents an
    /// edit. Append edits only carry the tail; in-place edits carry the whole
    /// field. Deciding this from lengths is wrong: a correction that shortens
    /// the text ("Shavano" → "Shivon") looks shorter than the insertion.
    pub fn edited_text(&self, inserted: &str) -> Option<String> {
        match self {
            EditDiff::Edited { replacement, .. } => Some(format!("{inserted}{replacement}")),
            EditDiff::InPlace { current } => Some(current.clone()),
            EditDiff::Unchanged | EditDiff::Appended | EditDiff::Unrecognisable => None,
        }
    }
}

/// The observer's clock: wall-clock ms, injectable so tests do not sleep.
pub fn now_ms() -> i64 {
    teletype_core::storage::now_ms() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(before: &str, pasted: &str, after: &str) -> PasteAnchor {
        PasteAnchor {
            before: before.to_string(),
            after: after.to_string(),
            pasted: pasted.to_string(),
        }
    }

    fn pending_with(anchor: Option<PasteAnchor>, id: Option<i64>, inserted: &str) -> PendingEdit {
        PendingEdit {
            inserted: inserted.to_string(),
            target: "com.example.app".into(),
            at: Instant::now(),
            anchor,
            dictation_id: id,
            recorded_at_ms: 0,
        }
    }

    /// A sink that records its calls in a Mutex so assertions can inspect
    /// exactly what the glue wrote.
    #[derive(Default)]
    struct FakeSink {
        calls: std::sync::Mutex<Vec<(i64, String, Option<String>)>>,
    }

    impl LearningSink for FakeSink {
        fn set_observed(&self, id: i64, observed: &str, edit_ops_json: Option<&str>) -> bool {
            self.calls.lock().unwrap().push((
                id,
                observed.to_string(),
                edit_ops_json.map(String::from),
            ));
            true
        }
    }

    // ── record_watch / status / expiry ───────────────────────────────────

    #[test]
    fn record_watch_stores_anchor_and_id() {
        let mut w = EditWatch::default();
        w.record_watch(
            "hello world".into(),
            Some(anchor("prefix ", "hello world", " suffix")),
            Some(42),
            1_000,
        );
        assert_eq!(w.status(), EditWatchStatus::Watching);
        let p = w.take().expect("fresh watch is takeable");
        assert_eq!(p.inserted, "hello world");
        assert_eq!(p.dictation_id, Some(42));
        assert!(p.anchor.is_some());
        assert_eq!(w.status(), EditWatchStatus::Idle);
    }

    #[test]
    fn record_watch_without_anchor_keeps_hint_layer() {
        let mut w = EditWatch::default();
        w.record_watch("hello".into(), None, None, 1_000);
        assert_eq!(w.status(), EditWatchStatus::Watching);
        let p = w.take().unwrap();
        assert!(p.anchor.is_none());
        assert_eq!(p.dictation_id, None);
    }

    #[test]
    fn an_empty_insertion_is_never_watched() {
        let mut w = EditWatch::default();
        w.record_watch("   ".into(), Some(anchor("", "   ", "")), Some(1), 0);
        assert_eq!(w.status(), EditWatchStatus::Idle);
        w.record("   ".into(), "app".into());
        assert_eq!(w.status(), EditWatchStatus::Idle);
    }

    #[test]
    fn an_expired_watch_is_dropped_rather_than_reported() {
        let mut w = EditWatch::default();
        w.record_watch(
            "hello".into(),
            Some(anchor("", "hello", "")),
            Some(1),
            1_000,
        );
        if let Some(p) = w.pending.as_mut() {
            p.at = Instant::now() - WATCH_WINDOW - Duration::from_secs(1);
        }
        assert_eq!(w.status(), EditWatchStatus::Expired);
        assert!(w.take().is_none());
    }

    #[test]
    fn set_target_fills_the_app_name() {
        let mut w = EditWatch::default();
        w.record_watch("hello".into(), None, None, 0);
        w.set_target("com.apple.Notes".into());
        assert_eq!(w.take().unwrap().target, "com.apple.Notes");
    }

    // ── build_anchor ─────────────────────────────────────────────────────

    #[test]
    fn build_anchor_clips_to_40_chars_per_side() {
        let before = "x".repeat(100);
        let after = "y".repeat(100);
        let a = build_anchor(Some(&before), Some(&after), "PASTED").unwrap();
        assert_eq!(a.before, "x".repeat(40));
        assert_eq!(a.after, "y".repeat(40));
        assert_eq!(a.pasted, "PASTED");
    }

    #[test]
    fn build_anchor_is_none_when_surroundings_unreadable() {
        assert!(build_anchor(None, Some("after"), "x").is_none());
        assert!(build_anchor(Some("before"), None, "x").is_none());
    }

    // ── decision→observe glue ────────────────────────────────────────────

    #[test]
    fn observe_glue_records_when_located() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            "fix the bug",
        );
        let sink = FakeSink::default();
        // The user fixed a word inside the paste: "fix the bugs".
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::FocusLost,
            5_000,
            &pending,
            &|| FieldRead::Readable("start fix the bugs end".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::Recorded));
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 7);
        assert_eq!(calls[0].1, "fix the bugs");
        // The ops json must carry the one word the user fixed.
        let json = calls[0].2.as_deref().unwrap();
        assert!(json.contains("replace"), "expected a replace op in {json}");
        assert!(
            json.contains("bug"),
            "the pasted word must appear in {json}"
        );
        assert!(
            json.contains("bugs"),
            "the observed word must appear in {json}"
        );
    }

    #[test]
    fn observe_glue_drops_when_not_located() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            "fix the bug",
        );
        let sink = FakeSink::default();
        // The field no longer resembles the paste at all: beyond max_dist.
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::EnterPressed,
            5_000,
            &pending,
            &|| FieldRead::Readable("completely different text that shares nothing".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::NotLocated));
        assert!(
            sink.calls.lock().unwrap().is_empty(),
            "a guess must never be written"
        );
    }

    #[test]
    fn observe_glue_unreadable_field_is_dropped_silently() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(Some(anchor("", "hello", "")), Some(7), "hello");
        let sink = FakeSink::default();
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::NextDictationStarted,
            5_000,
            &pending,
            &|| FieldRead::Unreadable,
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::FieldUnreadable));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn observe_glue_skips_without_anchor_or_id() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        // No anchor (and no id): nothing to locate or write.
        let pending = pending_with(None, None, "hello");
        let sink = FakeSink::default();
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::FocusLost,
            5_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::Skipped));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn observe_glue_waits_until_a_trigger() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(Some(anchor("", "hello", "")), Some(7), "hello");
        let sink = FakeSink::default();
        // A keystroke keeps the observer waiting.
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::Keystroke,
            1_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, None);
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn poll_fires_idle_and_cap() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(Some(anchor("", "hello", "")), Some(7), "hello");
        let sink = FakeSink::default();
        // 19.9 s: still waiting.
        let outcome = poll_observer(
            &mut observer,
            19_999,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, None);
        // 20 s idle: observe fires.
        let outcome = poll_observer(
            &mut observer,
            20_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::Recorded));
        assert_eq!(sink.calls.lock().unwrap().len(), 1);
        // A second poll after the decision is final: nothing.
        let outcome = poll_observer(
            &mut observer,
            30_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, None);
        assert_eq!(sink.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_consumed_store_observer_keeps_the_pending_for_the_hint_layer() {
        // Regression: the controller cleared the watch after the store
        // observation, so the preference/dictionary path never saw a real
        // paste. The store layer writes its final observer back and leaves
        // the pending edit in place; `take()` must still yield it once.
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("", "fix the bug", "")),
            Some(7),
            0,
        );
        let pending = w.pending().cloned().unwrap();
        let mut observer = w.observer_mut().cloned().unwrap();
        let sink = FakeSink::default();
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::NextDictationStarted,
            5_000,
            &pending,
            &|| FieldRead::Readable("fix the bugs".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::Recorded));
        w.set_observer(Some(observer));

        let taken = w.take().expect("the hint layer must still see the paste");
        assert_eq!(taken.inserted, "fix the bug");
        assert_eq!(taken.dictation_id, Some(7));

        // The observer was final, so no second observation can record.
        let mut observer = w.observer_mut().cloned().expect("observer survives");
        let second = observe_once(
            &mut observer,
            ObserverEvent::NextDictationStarted,
            6_000,
            &pending,
            &|| FieldRead::Readable("fix the bugs".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(second, None);
        assert_eq!(sink.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_drops_the_observation() {
        let mut observer = ObserverState::new(0);
        observer.on_event(ObserverEvent::Paste, 0);
        let pending = pending_with(Some(anchor("", "hello", "")), Some(7), "hello");
        let sink = FakeSink::default();
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::Canceled,
            1_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
            None,
        );
        assert_eq!(outcome, Some(ObserveOutcome::Skipped));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    // ── legacy diff heuristics (UI hint layer) ───────────────────────────

    #[test]
    fn an_untouched_field_is_unchanged() {
        assert_eq!(
            diff("The deploy went out.", "The deploy went out."),
            EditDiff::Unchanged
        );
    }

    #[test]
    fn prior_content_before_our_insert_is_ignored() {
        // The field already had text; we inserted at the caret after it.
        let value = "Existing note. The deploy went out.";
        assert_eq!(diff(value, "The deploy went out."), EditDiff::Unchanged);
    }

    #[test]
    fn a_trailing_space_is_not_an_edit() {
        // Every dictation ends with a space when the user keeps typing. Treating
        // that as an edit would manufacture a signal on almost every dictation.
        assert_eq!(
            diff("The deploy went out. ", "The deploy went out."),
            EditDiff::Appended
        );
        assert_eq!(
            diff("The deploy went out.\n", "The deploy went out."),
            EditDiff::Appended
        );
    }

    #[test]
    fn a_rewritten_tail_is_an_edit_carrying_the_replacement() {
        let value = "The deploy went out. Best regards";
        match diff(value, "The deploy went out.") {
            EditDiff::Edited { at, replacement } => {
                // The insertion starts at 0 here, and what the user added after
                // it is the replacement.
                assert_eq!(at, 0);
                assert_eq!(replacement, " Best regards");
            }
            other => panic!("expected Edited, got {other:?}"),
        }
    }

    #[test]
    fn an_edit_after_prior_content_reports_the_anchored_offset() {
        // The field already held text, so the insertion does not start at 0. The
        // offset has to be the real one or the caller rebuilds the wrong text.
        let value = "Existing note. The deploy went out. Best regards";
        match diff(value, "The deploy went out.") {
            EditDiff::Edited { at, replacement } => {
                assert_eq!(at, 15);
                assert_eq!(replacement, " Best regards");
            }
            other => panic!("expected Edited, got {other:?}"),
        }
    }

    #[test]
    fn a_rewritten_sign_off_inside_the_insert_is_found_by_prefix() {
        // The user edited the middle of what we wrote, so the exact inserted
        // string is gone. This is now recognised as an in-place edit.
        let value = "The deploy went out. Cheers";
        assert_eq!(
            diff(value, "The deploy went out. Best regards,"),
            EditDiff::InPlace {
                current: "The deploy went out. Cheers".to_string(),
            }
        );
    }

    #[test]
    fn an_emptied_field_is_unrecognisable() {
        assert_eq!(diff("", "hello"), EditDiff::Unrecognisable);
        assert_eq!(diff("hello", ""), EditDiff::Unrecognisable);
    }

    #[test]
    fn recording_then_checking_reports_watching() {
        let mut w = EditWatch::default();
        assert_eq!(w.status(), EditWatchStatus::Idle);
        w.record("hello".into(), "com.apple.Notes".into());
        assert_eq!(w.status(), EditWatchStatus::Watching);
        let taken = w.take().expect("a fresh watch is takeable");
        assert_eq!(taken.inserted, "hello");
        assert_eq!(taken.target, "com.apple.Notes");
        assert_eq!(w.status(), EditWatchStatus::Idle);
    }

    #[test]
    fn in_place_edit_inside_insert_is_recognised() {
        // The user fixed a misheard word inside the inserted text. The exact
        // inserted string is gone, but the first word matches and the word
        // counts are close, so this must be an edit, not `Unrecognisable`.
        let inserted = "We value our clients.";
        let current = "We value our customers.";
        let d = diff(current, inserted);
        assert!(
            matches!(d, EditDiff::InPlace { .. }),
            "in-place word swap inside the insert must be InPlace, not Unrecognisable; got {d:?}"
        );
    }

    #[test]
    fn a_corrected_first_word_is_recognised_as_an_edit() {
        // A misheard name is often the first word of the take. Replacing it
        // changes the first word, which the old heuristic read as somebody
        // else's text and skipped, so this whole class of corrections never
        // reached the dictionary.
        let inserted = "Shavano Rourke, Epha, Py Torch, Kubernetes.";
        let current = "Siobhan Rourke, Epha, Py Torch, Kubernetes.";
        let d = diff(current, inserted);
        assert!(
            matches!(d, EditDiff::InPlace { .. }),
            "a corrected first word must be an edit, not Unrecognisable; got {d:?}"
        );
    }

    #[test]
    fn a_shorter_correction_is_the_learnable_text_not_a_concatenation() {
        // "Shavano" (7) -> "Shivon" (6) shortens the text. A caller that
        // infers append-vs-in-place from `replacement.len()` concatenates the
        // old and new takes, and the extractor then sees no aligned swap.
        let inserted = "Shavano Rourke, Epha, Py Torch, Kubernetes.";
        let current = "Shivon Rourke, Epha, Py Torch, Kubernetes.";
        let user_text = diff(current, inserted)
            .edited_text(inserted)
            .expect("a corrected first word is an edit");
        assert_eq!(user_text, current);
    }

    #[test]
    fn an_append_edit_still_rebuilds_from_the_inserted_text() {
        let inserted = "hello world";
        let field = "hello world, and goodnight";
        let user_text = diff(field, inserted)
            .edited_text(inserted)
            .expect("an appended take is still an edit");
        assert_eq!(user_text, field);
    }

    #[test]
    fn an_unrelated_replacement_stays_unrecognisable() {
        // Clearing the field and writing something else is not an edit of
        // our take, and must never become a learned correction.
        assert_eq!(diff("Goodbye now", "Hello there"), EditDiff::Unrecognisable);
    }

    // ── last-good-read flush fallback ─────────────────────────────────

    #[test]
    fn a_good_read_is_flushed_when_the_field_empties() {
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            0,
        );
        // The user fixes the misheard word; a tick reads the field while
        // the text is still there. That read becomes the flush fallback.
        w.note_field_read("start fix the bugs end", 5_000);
        assert!(w.last_good().is_some());

        // The user pressed Enter and the composer emptied. The next poll
        // (20 s idle) cannot locate the paste in the empty field, so it
        // must fall back on the cached read.
        let pending = w.pending().cloned().unwrap();
        let mut observer = w.observer_mut().cloned().unwrap();
        let sink = FakeSink::default();
        let outcome = poll_observer(
            &mut observer,
            20_000,
            &pending,
            &|| FieldRead::Readable(String::new()),
            Some(&sink),
            w.last_good(),
        );
        assert_eq!(outcome, Some(ObserveOutcome::Recorded));
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 7);
        assert_eq!(calls[0].1, "fix the bugs");
        // The ops json must carry the one word the user fixed.
        let json = calls[0].2.as_deref().unwrap();
        assert!(json.contains("replace"), "expected a replace op in {json}");
        assert!(
            json.contains("bugs"),
            "the observed word must appear in {json}"
        );
    }

    #[test]
    fn a_good_read_is_flushed_when_the_next_dictation_starts() {
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            0,
        );
        w.note_field_read("start fix the bugs end", 5_000);

        // The user sent the message and started dictating again: the field is
        // already empty when the next-dictation event arrives. The flush must
        // happen here too, not only on the 20 s idle poll.
        let pending = w.pending().cloned().unwrap();
        let mut observer = w.observer_mut().cloned().unwrap();
        let sink = FakeSink::default();
        let outcome = observe_once(
            &mut observer,
            ObserverEvent::NextDictationStarted,
            10_000,
            &pending,
            &|| FieldRead::Readable(String::new()),
            Some(&sink),
            w.last_good(),
        );
        assert_eq!(outcome, Some(ObserveOutcome::Recorded));
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "fix the bugs");
    }

    #[test]
    fn an_unchanged_field_is_not_cached() {
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            0,
        );
        // A read identical to what we inserted is not an edit; nothing is
        // cached, so an emptied field still yields NotLocated.
        w.note_field_read("start fix the bug end", 5_000);
        assert!(w.last_good().is_none());

        let pending = w.pending().cloned().unwrap();
        let mut observer = w.observer_mut().cloned().unwrap();
        let sink = FakeSink::default();
        let outcome = poll_observer(
            &mut observer,
            20_000,
            &pending,
            &|| FieldRead::Readable(String::new()),
            Some(&sink),
            w.last_good(),
        );
        assert_eq!(outcome, Some(ObserveOutcome::NotLocated));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_read_from_the_same_tick_is_not_flushed() {
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            0,
        );
        // Cache a read taken 900 ms before the poll fires: too young to trust.
        w.note_field_read("start fix the bugs end", 19_100);
        let cached = w.last_good().cloned().unwrap();

        // The poll fires at 20_000 (20 s idle): the observer decides to
        // Observe, the empty field cannot locate the paste, and the cached
        // read is only 900 ms old (< FLUSH_MIN_QUIET_MS), so it is not
        // flushed.
        let pending = w.pending().cloned().unwrap();
        let mut observer = w.observer_mut().cloned().unwrap();
        let sink = FakeSink::default();
        let outcome = poll_observer(
            &mut observer,
            20_000,
            &pending,
            &|| FieldRead::Readable(String::new()),
            Some(&sink),
            Some(&cached),
        );
        assert_eq!(outcome, Some(ObserveOutcome::NotLocated));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn the_cache_is_cleared_with_the_pending_edit() {
        let mut w = EditWatch::default();
        w.record_watch(
            "fix the bug".into(),
            Some(anchor("start ", "fix the bug", " end")),
            Some(7),
            0,
        );
        w.note_field_read("start fix the bugs end", 5_000);
        assert!(w.last_good().is_some());
        assert!(w.take().is_some());
        assert!(
            w.last_good().is_none(),
            "the cache must die with the pending edit"
        );
    }
}
