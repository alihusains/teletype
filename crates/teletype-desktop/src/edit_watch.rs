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

/// The most recent insertion, if it is still worth checking.
#[derive(Debug, Clone, Default)]
pub struct EditWatch {
    pending: Option<PendingEdit>,
    /// Trigger state machine for the learning-store observation. Armed on
    /// the paste that set `pending`; final after Observe/Drop.
    observer: Option<ObserverState>,
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
        let mut observer = ObserverState::new(now_ms);
        observer.on_event(ObserverEvent::Paste, now_ms);
        self.observer = Some(observer);
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
    }

    /// Takes the pending insertion, if it is still fresh.
    ///
    /// Returning it means the caller is responsible for calling
    /// [`Self::finish`] with the outcome, so a failed read does not silently
    /// discard the observation.
    pub fn take(&mut self) -> Option<PendingEdit> {
        match self.status() {
            EditWatchStatus::Watching => self.pending.take(),
            _ => {
                self.pending = None;
                None
            }
        }
    }

    /// Clears the watch without recording anything.
    pub fn finish(&mut self) {
        self.pending = None;
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
) -> Option<ObserveOutcome> {
    let decision = observer.on_event(event, now_ms);
    observe_decision(decision, now_ms, pending, read_field, sink)
}

/// The same, for the periodic poll (20 s idle / 3 min cap).
pub fn poll_observer(
    observer: &mut ObserverState,
    now_ms: i64,
    pending: &PendingEdit,
    read_field: &dyn Fn() -> FieldRead,
    sink: Option<&dyn LearningSink>,
) -> Option<ObserveOutcome> {
    let decision = observer.poll(now_ms);
    observe_decision(decision, now_ms, pending, read_field, sink)
}

/// Maps an [`ObserverDecision`] to an action. `Wait` yields `None` (still
/// observing); `Drop` yields [`ObserveOutcome::Skipped`]; `Observe` runs
/// the read→locate→write sequence.
fn observe_decision(
    decision: ObserverDecision,
    _now_ms: i64,
    pending: &PendingEdit,
    read_field: &dyn Fn() -> FieldRead,
    sink: Option<&dyn LearningSink>,
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
            let Some(located) = locate_observed(&field, anchor, LOCATE_MAX_DIST) else {
                // Never learn from a guess: the span is not confidently
                // ours, so the observation is dropped.
                return Some(ObserveOutcome::NotLocated);
            };
            let ops = diff_words(&pending.inserted, &located.observed);
            let ops_json = Some(encode_edit_ops(&ops));
            let Some(sink) = sink else {
                // No sink (learning disabled for the session): the
                // observation ran but had nowhere to go.
                return Some(ObserveOutcome::Recorded);
            };
            let written = sink.set_observed(id, &located.observed, ops_json.as_deref());
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
    // matches and word counts are close → in-place edit. `replacement`
    // carries the full current value; the caller uses it directly.
    if in_place_edit(value, inserted) {
        return EditDiff::Edited {
            at: 0,
            replacement: value.to_string(),
        };
    }
    EditDiff::Unrecognisable
}

/// True when `value` looks like an in-place edit of `inserted`: the first
/// word matches (case-insensitive) and the word counts differ by ≤ 3.
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
    if !first_ins.eq_ignore_ascii_case(first_val) {
        return false;
    }
    let ins_count = inserted.split_whitespace().count();
    let val_count = value.split_whitespace().count();
    ins_count.abs_diff(val_count) <= 3
}

impl EditDiff {
    /// Whether this diff represents a user edit (as opposed to unchanged or
    /// unrecognisable).
    pub fn was_edited(&self) -> bool {
        matches!(self, EditDiff::Edited { .. })
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
        );
        assert_eq!(outcome, None);
        // 20 s idle: observe fires.
        let outcome = poll_observer(
            &mut observer,
            20_000,
            &pending,
            &|| FieldRead::Readable("hello".to_string()),
            Some(&sink),
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
        );
        assert_eq!(outcome, None);
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
            EditDiff::Edited {
                at: 0,
                replacement: "The deploy went out. Cheers".to_string(),
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
        // counts are close, so this must be `Edited`, not `Unrecognisable`.
        let inserted = "We value our clients.";
        let current = "We value our customers.";
        let d = diff(current, inserted);
        assert!(
            matches!(d, EditDiff::Edited { .. }),
            "in-place word swap inside the insert must be Edited, not Unrecognisable; got {d:?}"
        );
    }
}
