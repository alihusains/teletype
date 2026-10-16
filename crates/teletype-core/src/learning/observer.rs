//! Paste-anchor observation (Phase 7.2).
//!
//! At paste time we snapshot short anchors around the insertion point.
//! Later we re-read the field and re-locate the pasted span by fuzzy
//! matching the anchors (banded Levenshtein, distance <= `max_dist`).
//! Pure logic only: no platform readers, no store access.

/// Snapshot of where dictated text was pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteAnchor {
    /// Up to 40 chars immediately before the insertion point
    /// (empty at field start).
    pub before: String,
    /// Up to 40 chars immediately after the pasted text
    /// (empty at field end).
    pub after: String,
    /// Exactly the text that was inserted.
    pub pasted: String,
}

/// The last `max_chars` characters of `text` (unicode-safe).
pub fn clip_anchor(text_before_cursor: &str, max_chars: usize) -> String {
    let mut v: Vec<char> = text_before_cursor.chars().rev().take(max_chars).collect();
    v.reverse();
    v.into_iter().collect()
}

/// A located paste span: byte indices into `field_text` plus the observed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub start: usize,
    pub end: usize,
    pub observed: String,
}

// TODO(7.2): locate_observed + Levenshtein, ObserverState, tests.

/// Banded Levenshtein distance with early exit: returns `None` as soon as
/// the distance provably exceeds `max_dist`.
fn bounded_levenshtein(a: &str, b: &str, max_dist: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if (a.len() as isize - b.len() as isize).unsigned_abs() > max_dist {
        return None;
    }
    let m = a.len();
    let n = b.len();
    if m == 0 {
        return Some(if n <= max_dist { n } else { n + 1 });
    }
    if n == 0 {
        return Some(if m <= max_dist { m } else { m + 1 });
    }
    // Standard two-row DP, banded: only compute column j when
    // |i - j| <= max_dist (outside the band the distance is > max_dist).
    let inf = max_dist + 1;
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![inf; n + 1];
    for i in 1..=m {
        cur.fill(inf);
        let lo = i.saturating_sub(max_dist);
        let hi = (i + max_dist).min(n);
        for j in lo..=hi {
            if j == 0 {
                cur[0] = i; // insertion-only column: exactly i
                continue;
            }
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[n];
    if d > max_dist {
        None
    } else {
        Some(d)
    }
}

/// Edit distance over the FULL strings, used only to rank candidates
/// that already matched within `max_dist` (strings are short anchors).
fn full_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = b.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0; n + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

/// Normalized edit distance in [0.0, 1.0]; 0.0 = identical.
fn normalized_distance(a: &str, b: &str) -> f64 {
    let d = full_levenshtein(a, b) as f64;
    let max_len = a.chars().count().max(b.chars().count()) as f64;
    if max_len == 0.0 {
        0.0
    } else {
        d / max_len
    }
}

/// A matching span (start/end are CHAR indices into `field_text`) and its
/// scores, compared by [`better`].
struct Candidate {
    start: usize,
    end: usize,
    before_dist: usize,
    after_dist: usize,
    pasted_dist: f64,
}

/// Re-locate the pasted span inside a later read of the field.
///
/// A candidate span `[start, end)` (char indices internally) matches when:
/// - the `anchor.before` chars immediately before `start` are within
///   `max_dist` edits of `anchor.before` (empty `before` pins `start` to the
///   field start), and
/// - the `anchor.after` chars immediately after `end` are within `max_dist`
///   edits of `anchor.after` (empty `after` pins `end` to the field end), and
/// - the span itself is within `max_dist` edits of `pasted` — the user may
///   have edited the text, but a span that drifted further is not a
///   confident match, so the observation is dropped.
///
/// Candidate ends are only considered within `max_dist` chars of `pasted`'s
/// length, a safe pre-filter (edit distance cannot be smaller than the
/// length difference) that keeps the scan linear on large fields.
///
/// Deterministic tie-break among matching candidates:
/// 1. sum of the two anchor edit distances (lower wins),
/// 2. normalized edit distance of the span to `pasted` (lower wins),
/// 3. earliest `start`.
///
/// Returns `None` when no candidate matches, and when both anchors are
/// empty and the field is empty (nothing to observe).
pub fn locate_observed(field_text: &str, anchor: &PasteAnchor, max_dist: usize) -> Option<Located> {
    if anchor.before.is_empty() && anchor.after.is_empty() && field_text.is_empty() {
        return None;
    }
    // `bounds[k]` = byte offset of char index `k`; the last entry is the end
    // offset, so the span `[start, end)` in char indices maps to
    // `field_text[bounds[start]..bounds[end]]`.
    let mut bounds: Vec<usize> = field_text.char_indices().map(|(i, _)| i).collect();
    bounds.push(field_text.len());
    let char_count = bounds.len() - 1;

    let before_len = anchor.before.chars().count();
    let after_len = anchor.after.chars().count();
    let pasted_len = anchor.pasted.chars().count();

    let hits = |window: &str, needle: &str| bounded_levenshtein(window, needle, max_dist);

    // Candidate start positions: every char index whose `before` window is
    // within `max_dist`, or exactly 0 when `before` is empty.
    let start_range: Vec<usize> = if anchor.before.is_empty() {
        vec![0]
    } else {
        (before_len..=char_count).collect()
    };

    let mut best: Option<Candidate> = None;
    for start in start_range {
        let before_dist = if anchor.before.is_empty() {
            0
        } else {
            match hits(
                &field_text[bounds[start - before_len]..bounds[start]],
                &anchor.before,
            ) {
                Some(d) => d,
                None => continue,
            }
        };
        let lo = start + pasted_len.saturating_sub(max_dist);
        let hi = (start + pasted_len + max_dist).min(char_count);
        for end in lo..=hi {
            if end == start && !anchor.pasted.is_empty() {
                continue;
            }
            let after_dist = if anchor.after.is_empty() {
                if end != char_count {
                    continue;
                }
                0
            } else {
                if end + after_len > char_count {
                    continue;
                }
                match hits(
                    &field_text[bounds[end]..bounds[end + after_len]],
                    &anchor.after,
                ) {
                    Some(d) => d,
                    None => continue,
                }
            };
            let span = &field_text[bounds[start]..bounds[end]];
            if full_levenshtein(span, &anchor.pasted) > max_dist {
                continue;
            }
            let cand = Candidate {
                start,
                end,
                before_dist,
                after_dist,
                pasted_dist: normalized_distance(span, &anchor.pasted),
            };
            if better(&cand, best.as_ref()) {
                best = Some(cand);
            }
        }
    }
    let best = best?;
    Some(Located {
        start: bounds[best.start],
        end: bounds[best.end],
        observed: field_text[bounds[best.start]..bounds[best.end]].to_string(),
    })
}

/// Deterministic ordering: lower (before+after distance), then lower
/// pasted similarity distance, then earlier start.
fn better(cand: &Candidate, cur: Option<&Candidate>) -> bool {
    match cur {
        None => true,
        Some(c) => {
            let cs = (
                cand.before_dist + cand.after_dist,
                cand.pasted_dist,
                cand.start,
            );
            let cc = (c.before_dist + c.after_dist, c.pasted_dist, c.start);
            cs < cc
        }
    }
}

const IDLE_MS: i64 = 20_000;
const CAP_MS: i64 = 180_000;

/// Events that drive the trigger state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverEvent {
    Paste,
    FocusLost,
    EnterPressed,
    Keystroke,
    NextDictationStarted,
    Canceled,
}

/// What the observer should do after an event or poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverDecision {
    /// Keep waiting for a trigger.
    Wait,
    /// Trigger fired: read the field now.
    Observe,
    /// Observation dropped (take aborted before delivery).
    Drop,
}

/// Trigger state machine: arms on `Paste`, observes at the first of
/// focus loss, Enter, 20 s idle after the last keystroke, next
/// dictation start, or the 3 min hard cap. Final after `Observe`/`Drop`.
#[derive(Debug, Clone)]
pub struct ObserverState {
    armed: bool,
    final_: bool,
    last_keystroke_ms: Option<i64>,
    paste_ms: i64,
}

impl ObserverState {
    pub fn new(now_ms: i64) -> Self {
        Self {
            armed: false,
            final_: false,
            last_keystroke_ms: None,
            paste_ms: now_ms,
        }
    }

    fn idle_expired(&self, now_ms: i64) -> bool {
        match self.last_keystroke_ms {
            Some(t) => now_ms - t >= IDLE_MS,
            None => self.armed && now_ms - self.paste_ms >= IDLE_MS,
        }
    }

    pub fn on_event(&mut self, ev: ObserverEvent, now_ms: i64) -> ObserverDecision {
        if self.final_ {
            return ObserverDecision::Wait;
        }
        match ev {
            ObserverEvent::Paste => {
                self.armed = true;
                self.paste_ms = now_ms;
                ObserverDecision::Wait
            }
            ObserverEvent::Canceled if self.armed => {
                self.final_ = true;
                ObserverDecision::Drop
            }
            ObserverEvent::FocusLost
            | ObserverEvent::EnterPressed
            | ObserverEvent::NextDictationStarted
                if self.armed =>
            {
                self.final_ = true;
                ObserverDecision::Observe
            }
            ObserverEvent::Keystroke if self.armed => {
                self.last_keystroke_ms = Some(now_ms);
                ObserverDecision::Wait
            }
            _ => ObserverDecision::Wait,
        }
    }

    pub fn poll(&mut self, now_ms: i64) -> ObserverDecision {
        if self.final_ || !self.armed {
            return ObserverDecision::Wait;
        }
        if now_ms - self.paste_ms >= CAP_MS || self.idle_expired(now_ms) {
            self.final_ = true;
            return ObserverDecision::Observe;
        }
        ObserverDecision::Wait
    }
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

    // ── clip_anchor ──────────────────────────────────────────────────────

    #[test]
    fn clip_anchor_takes_last_n_chars() {
        assert_eq!(clip_anchor("hello world", 5), "world");
        assert_eq!(clip_anchor("hi", 10), "hi");
        assert_eq!(clip_anchor("héllo wörld", 3), "rld");
        assert_eq!(clip_anchor("", 40), "");
        assert_eq!(clip_anchor("abc", 0), "");
    }

    #[test]
    fn clip_anchor_never_splits_char() {
        let s = "aé中x".repeat(5);
        let clipped = clip_anchor(&s, 7);
        // The last 7 chars of "aé中xaé中xaé中xaé中xaé中x" end with the final x.
        assert_eq!(clipped, "é中xaé中x");
        assert_eq!(clipped.chars().count(), 7);
    }

    // ── locate_observed ──────────────────────────────────────────────────

    #[test]
    fn locate_exact_paste() {
        let a = anchor("Hello ", "WORLD", ". Bye");
        let loc = locate_observed("Hello WORLD. Bye", &a, 3).unwrap();
        assert_eq!((loc.start, loc.end), ("Hello ".len(), "Hello WORLD".len()));
        assert_eq!(loc.observed, "WORLD");
    }

    #[test]
    fn locate_with_edits_before_and_after() {
        // User edited inside the before anchor ("Hellp ") and right after
        // the pasted span ("! Bye"); both are within max_dist, so the span
        // "WORLD" itself is still located exactly.
        let a = anchor("Hello ", "WORLD", ". Bye");
        let field = "Hellp WORLD! Bye, and thanks";
        let loc = locate_observed(field, &a, 3).unwrap();
        assert_eq!(loc.observed, "WORLD");
        assert_eq!(&field[loc.start..loc.end], "WORLD");
    }

    #[test]
    fn locate_edit_inside_pasted_text() {
        let a = anchor("start ", "fix the bug", " end");
        let field = "start fix the bugs end";
        let loc = locate_observed(field, &a, 3).unwrap();
        assert_eq!(loc.observed, "fix the bugs");
        assert_ne!(loc.observed, a.pasted);
    }

    #[test]
    fn locate_picks_best_candidate_on_repeated_before() {
        // "foo" appears twice; the second occurrence is the true paste site.
        let a = anchor("foo ", "BAR", " baz");
        let field = "foo X baz mid foo BAR baz";
        let loc = locate_observed(field, &a, 1).unwrap();
        assert_eq!(loc.observed, "BAR");
    }

    #[test]
    fn locate_none_when_insertion_beyond_max_dist() {
        let a = anchor("start ", "pasted", " end");
        let field = "start pasted a very long inserted chunk end";
        assert!(locate_observed(field, &a, 2).is_none());
    }

    #[test]
    fn locate_empty_before_at_field_start() {
        let a = anchor("", "hello", " world");
        let loc = locate_observed("hello world", &a, 3).unwrap();
        assert_eq!((loc.start, loc.end), (0, 5));
        assert_eq!(loc.observed, "hello");
    }

    #[test]
    fn locate_empty_after_at_field_end() {
        let a = anchor("say ", "bye", "");
        let loc = locate_observed("say bye", &a, 3).unwrap();
        assert_eq!((loc.start, loc.end), (4, 7));
        assert_eq!(loc.observed, "bye");
    }

    #[test]
    fn locate_none_when_both_anchors_empty_and_field_empty() {
        assert!(locate_observed("", &anchor("", "x", ""), 3).is_none());
    }

    #[test]
    fn locate_unicode_heavy() {
        let a = anchor("héllo wörld ", "日本語テキスト", " 👋 fin");
        let field = "héllo wörld 日本語テキストよ 👋 fin";
        let loc = locate_observed(field, &a, 3).unwrap();
        assert_eq!(loc.observed, "日本語テキストよ");
    }

    #[test]
    fn locate_fuzzy_before_within_max_dist() {
        // Anchor captured "Hello " but the field has "Hellp " (one edit);
        // the span is still located exactly.
        let a = anchor("Hello ", "WORLD", "");
        let field = "Hellp WORLD";
        let loc = locate_observed(field, &a, 1).unwrap();
        assert_eq!((loc.start, loc.end), (6, 11));
        assert_eq!(loc.observed, "WORLD");
    }

    #[test]
    fn locate_large_field_is_fast() {
        let filler = "x".repeat(50_000);
        let a = anchor("tail ", "PASTED", " head");
        let field = format!("{filler}tail PASTED head");
        let loc = locate_observed(&field, &a, 3).unwrap();
        assert_eq!(loc.observed, "PASTED");
        assert_eq!(loc.start, field.len() - "PASTED head".len());
    }

    // ── bounded Levenshtein ──────────────────────────────────────────────

    #[test]
    fn bounded_levenshtein_values() {
        assert_eq!(bounded_levenshtein("kitten", "sitting", 10), Some(3));
        assert_eq!(bounded_levenshtein("abc", "abc", 0), Some(0));
        assert_eq!(bounded_levenshtein("abc", "xyz", 2), None);
        assert_eq!(bounded_levenshtein("", "ab", 2), Some(2));
        assert_eq!(bounded_levenshtein("", "ab", 1), None);
    }

    // ── trigger state machine ────────────────────────────────────────────

    #[test]
    fn paste_arms() {
        let mut s = ObserverState::new(0);
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 100),
            ObserverDecision::Wait
        );
        assert_eq!(
            s.on_event(ObserverEvent::Paste, 100),
            ObserverDecision::Wait
        );
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 200),
            ObserverDecision::Observe
        );
    }

    #[test]
    fn focus_lost_fires_once() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 5_000),
            ObserverDecision::Observe
        );
        assert_eq!(
            s.on_event(ObserverEvent::EnterPressed, 10_000),
            ObserverDecision::Wait
        );
        assert_eq!(s.poll(30_000), ObserverDecision::Wait);
    }

    #[test]
    fn enter_fires() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(
            s.on_event(ObserverEvent::EnterPressed, 1_000),
            ObserverDecision::Observe
        );
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 2_000),
            ObserverDecision::Wait
        );
    }

    #[test]
    fn next_dictation_fires() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(
            s.on_event(ObserverEvent::NextDictationStarted, 1_000),
            ObserverDecision::Observe
        );
    }

    #[test]
    fn idle_20s_fires_via_poll() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(s.poll(19_999), ObserverDecision::Wait);
        assert_eq!(s.poll(20_000), ObserverDecision::Observe);
        assert_eq!(s.poll(40_000), ObserverDecision::Wait);
    }

    #[test]
    fn keystroke_resets_idle_clock() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        s.on_event(ObserverEvent::Keystroke, 15_000);
        assert_eq!(s.poll(19_999), ObserverDecision::Wait);
        assert_eq!(s.poll(34_999), ObserverDecision::Wait);
        assert_eq!(s.poll(35_000), ObserverDecision::Observe);
    }

    #[test]
    fn cap_180s_fires_via_poll() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        s.on_event(ObserverEvent::Keystroke, 170_000); // keeps idle clock under 20 s
        assert_eq!(s.poll(179_999), ObserverDecision::Wait);
        assert_eq!(s.poll(180_000), ObserverDecision::Observe);
    }

    #[test]
    fn cancel_drops() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(
            s.on_event(ObserverEvent::Canceled, 1_000),
            ObserverDecision::Drop
        );
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 2_000),
            ObserverDecision::Wait
        );
        assert_eq!(s.poll(100_000), ObserverDecision::Wait);
    }

    #[test]
    fn no_double_observe() {
        let mut s = ObserverState::new(0);
        s.on_event(ObserverEvent::Paste, 0);
        assert_eq!(
            s.on_event(ObserverEvent::FocusLost, 1_000),
            ObserverDecision::Observe
        );
        assert_eq!(
            s.on_event(ObserverEvent::EnterPressed, 2_000),
            ObserverDecision::Wait
        );
        assert_eq!(
            s.on_event(ObserverEvent::NextDictationStarted, 3_000),
            ObserverDecision::Wait
        );
        assert_eq!(s.poll(100_000), ObserverDecision::Wait);
    }

    proptest::proptest! {
        #[test]
        fn prop_locate_never_panics_and_bounds(
            field in proptest::string::string_regex("[a-zé中]{0,60}").unwrap(),
            before in proptest::string::string_regex("[a-zé中]{0,20}").unwrap(),
            after in proptest::string::string_regex("[a-zé中]{0,20}").unwrap(),
            pasted in proptest::string::string_regex("[a-zé中]{0,20}").unwrap(),
        ) {
            let a = PasteAnchor { before, after, pasted };
            if let Some(loc) = locate_observed(&field, &a, 3) {
                assert!(loc.start <= loc.end);
                assert!(loc.end <= field.len());
                assert!(field.is_char_boundary(loc.start));
                assert!(field.is_char_boundary(loc.end));
                assert_eq!(loc.observed, field[loc.start..loc.end]);
            }
        }
    }
}
