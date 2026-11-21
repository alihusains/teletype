//! T5: local, conservative profile-choice bandit.
//!
//! This is the *only* thing the Polish learning loop is allowed to change:
//! which existing profile runs for an app. It never edits text — the guard and
//! verifier still decide whether a polished take is inserted.
//!
//! Signals come from two local sources: the T1 polish-feedback store (thumbs)
//! and the settle sweep's kept/undone verdicts. Both are folded into a small
//! per-`(app, profile)` tally. A switch is offered only when:
//!
//! 1. the recommended and the challenger each have at least
//!    [`MIN_SETTLED_TAKES`] settled takes,
//! 2. the challenger is **adjacent** (Clean↔Natural or Natural↔Structured),
//! 3. the challenger has at most [`MAX_CHALLENGER_UNDONES`] undones, and wins
//!    the kept-rate by at least [`CLEAR_WIN_MARGIN`],
//! 4. no automatic change for the app happened in the last
//!    [`CHANGE_CAP_MS`] (one change per app per week).
//!
//! Transform and Off are never candidates. Everything is local: the store is
//! a JSON file beside the feedback store, with no telemetry.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::profile::PolishProfile;

/// Minimum settled takes on **both** sides before a switch is considered.
pub const MIN_SETTLED_TAKES: u64 = 12;
/// The challenger's kept-rate must beat the incumbent's by this much.
/// A conservative margin, not a per-take flip.
pub const CLEAR_WIN_MARGIN: f64 = 0.15;
/// A challenger with more than this many undones is not a "clear" win.
pub const MAX_CHALLENGER_UNDONES: u64 = 1;
/// At most one automatic profile change per app per week.
pub const CHANGE_CAP_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// A learnable signal. `None` where a signal is used means "ambiguous — learn
/// nothing" (focus moved, watch expired, take edited for unrelated reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Kept,
    Undone,
    ThumbUp,
    ThumbDown,
}

/// Only these three profiles can be chosen by the loop. Transform is an
/// explicit user rewrite and Off disables polish; neither is ever automatic.
pub fn is_learnable(profile: PolishProfile) -> bool {
    matches!(
        profile,
        PolishProfile::Clean | PolishProfile::Natural | PolishProfile::Structured
    )
}

/// True for a single step along Clean ↔ Natural ↔ Structured. Symmetric, and
/// deliberately excludes equal pairs, Transform and Off.
pub fn adjacent(a: PolishProfile, b: PolishProfile) -> bool {
    use PolishProfile::*;
    matches!(
        (a, b),
        (Clean, Natural) | (Natural, Clean) | (Natural, Structured) | (Structured, Natural)
    )
}

/// Stable key for one `(app, profile)` tally.
fn tally_key(app_key: &str, profile: PolishProfile) -> String {
    format!(
        "{}|{}",
        crate::profile::normalize_key(app_key),
        profile.key()
    )
}

/// The local tally for one app/profile pair. Thumbs are kept separate from
/// settle verdicts so a single take that produced both cannot double-count;
/// [`ProfileTally::score`] folds them into one conservative kept-rate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTally {
    /// Takes that reached a verdict (kept or undone). Thumbs do not count.
    #[serde(default)]
    pub settled: u64,
    #[serde(default)]
    pub kept: u64,
    #[serde(default)]
    pub undone: u64,
    #[serde(default)]
    pub thumb_up: u64,
    #[serde(default)]
    pub thumb_down: u64,
}

impl ProfileTally {
    /// Kept-rate in 0.0..=1.0, folding thumbs into the settle verdicts.
    /// Returns 0.0 with no evidence — absence of data is never a win.
    pub fn score(&self) -> f64 {
        let kept = self.kept + self.thumb_up;
        let undone = self.undone + self.thumb_down;
        let total = kept + undone;
        if total == 0 {
            0.0
        } else {
            kept as f64 / total as f64
        }
    }

    /// Negative votes against this profile, used as the "clear win" guard.
    fn negatives(&self) -> u64 {
        self.undone + self.thumb_down
    }
}

/// One recorded automatic change, kept for the weekly cap and for undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanditChange {
    #[serde(default)]
    pub app_key: String,
    #[serde(default)]
    pub from: PolishProfile,
    #[serde(default)]
    pub to: PolishProfile,
    #[serde(default)]
    pub at_ms: u64,
    /// Set by [`BanditState::undo_change`]; an undone change still consumes
    /// the week's cap so the loop cannot immediately flip the app back.
    #[serde(default)]
    pub undone: bool,
}

/// The whole local bandit state. Pure data: no I/O, no clocks, no app dirs —
/// all time comes from the caller so tests are deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanditState {
    #[serde(default)]
    pub tallies: BTreeMap<String, ProfileTally>,
    #[serde(default)]
    pub changes: Vec<BanditChange>,
}

impl BanditState {
    pub fn tally(&self, app_key: &str, profile: PolishProfile) -> &ProfileTally {
        // A &self accessor cannot insert into the map; empty is a shared value.
        self.tallies
            .get(&tally_key(app_key, profile))
            .unwrap_or(&EMPTY_TALLY)
    }

    /// Applies one signal. `None` (ambiguous) is a deliberate no-op: the row
    /// is not settled, or the edit reason is unknown, so nothing is learned.
    pub fn record_outcome(
        &mut self,
        app_key: &str,
        profile: PolishProfile,
        outcome: Option<Outcome>,
    ) {
        let Some(outcome) = outcome else {
            return;
        };
        if !is_learnable(profile) {
            return;
        }
        let tally = self.tallies.entry(tally_key(app_key, profile)).or_default();
        match outcome {
            Outcome::Kept => {
                tally.settled += 1;
                tally.kept += 1;
            }
            Outcome::Undone => {
                tally.settled += 1;
                tally.undone += 1;
            }
            Outcome::ThumbUp => tally.thumb_up += 1,
            Outcome::ThumbDown => tally.thumb_down += 1,
        }
    }

    /// Any automatic change for this app in the last [`CHANGE_CAP_MS`] consumes
    /// the cap, including one the user later undid (no instant flip-flop).
    pub fn changed_within_cap(&self, app_key: &str, now_ms: u64) -> bool {
        let key = crate::profile::normalize_key(app_key);
        self.changes.iter().any(|change| {
            change.app_key == key && now_ms.saturating_sub(change.at_ms) < CHANGE_CAP_MS
        })
    }

    /// The active automatic change for this app, if any.
    pub fn active_change(&self, app_key: &str) -> Option<&BanditChange> {
        let key = crate::profile::normalize_key(app_key);
        self.changes
            .iter()
            .rev()
            .find(|change| change.app_key == key && !change.undone)
    }

    /// The exact promotion rule. Returns a challenger only when every guard
    /// passes; `None` means "keep the recommended default".
    pub fn decide(
        &self,
        app_key: &str,
        recommended: PolishProfile,
        now_ms: u64,
    ) -> Option<PolishProfile> {
        if !is_learnable(recommended) {
            return None;
        }
        if self.changed_within_cap(app_key, now_ms) {
            return None;
        }
        let incumbent = self.tally(app_key, recommended);
        if incumbent.settled < MIN_SETTLED_TAKES {
            return None;
        }
        let target = incumbent.score() + CLEAR_WIN_MARGIN;
        for challenger in [PolishProfile::Clean, PolishProfile::Structured] {
            if !adjacent(recommended, challenger) || !is_learnable(challenger) {
                continue;
            }
            let evidence = self.tally(app_key, challenger);
            if evidence.settled < MIN_SETTLED_TAKES {
                continue;
            }
            if evidence.negatives() > MAX_CHALLENGER_UNDONES {
                continue;
            }
            if evidence.score() >= target {
                return Some(challenger);
            }
        }
        None
    }

    /// Records a decided switch. Enforces the same guards as [`Self::decide`]
    /// a second time so a caller cannot bypass the cap or jump to a
    /// non-adjacent profile. Returns true when the change was recorded.
    pub fn apply_change(
        &mut self,
        app_key: &str,
        from: PolishProfile,
        to: PolishProfile,
        now_ms: u64,
    ) -> bool {
        if !is_learnable(from)
            || !is_learnable(to)
            || !adjacent(from, to)
            || self.changed_within_cap(app_key, now_ms)
        {
            return false;
        }
        self.changes.push(BanditChange {
            app_key: crate::profile::normalize_key(app_key),
            from,
            to,
            at_ms: now_ms,
            undone: false,
        });
        true
    }

    /// Undoes the most recent active change for `app_key` and returns the
    /// profile to restore (the change's `from`). The change stays in history
    /// so it still consumes the week's cap.
    pub fn undo_change(&mut self, app_key: &str) -> Option<PolishProfile> {
        let key = crate::profile::normalize_key(app_key);
        let index = self
            .changes
            .iter()
            .rposition(|change| change.app_key == key && !change.undone)?;
        let change = &mut self.changes[index];
        change.undone = true;
        Some(change.from)
    }

    /// The profile that should run this take: an already-applied learned change
    /// first, then a newly-promoted challenger, otherwise exactly the
    /// recommended default.
    ///
    /// A recorded change is authoritative even though `decide` deliberately
    /// returns `None` once `changed_within_cap` is true — otherwise a learned
    /// profile would be recorded but never actually run.
    pub fn choose(&self, app_key: &str, recommended: PolishProfile, now_ms: u64) -> PolishProfile {
        self.active_change(app_key)
            .map(|c| c.to)
            .or_else(|| self.decide(app_key, recommended, now_ms))
            .unwrap_or(recommended)
    }
}

static EMPTY_TALLY: ProfileTally = ProfileTally {
    settled: 0,
    kept: 0,
    undone: 0,
    thumb_up: 0,
    thumb_down: 0,
};

/// A friendly label for the Insights row ("Gmail", not a bundle id). Known
/// bundle ids map to product names; anything else uses the last dot segment,
/// title-cased. This is display-only and never feeds a decision.
pub fn app_label(app_key: &str) -> String {
    let key = crate::profile::normalize_key(app_key);
    let known = match key.as_str() {
        "com.google.gmail" | "gmail" => "Gmail",
        "com.apple.mail" | "mail" => "Mail",
        "com.tinyspeck.slackmacgap" | "slack" => "Slack",
        "com.microsoft.teams" | "teams" => "Teams",
        "com.discordapp.discord" | "discord" => "Discord",
        "com.openai.chatgpt" | "chatgpt" => "ChatGPT",
        "com.microsoft.VSCode" | "vscode" => "VS Code",
        "com.microsoft.word" | "word" => "Word",
        "com.apple.notes" | "notes" => "Notes",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let last = key.rsplit('.').next().unwrap_or(&key);
    let mut chars = last.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => key,
    }
}

/// The per-app settings key an undo writes to restore the previous profile.
/// Known apps map to their category key (the same keys the per-app profile
/// table uses); anything else uses its normalized bundle id, which
/// `resolve_profile` also accepts as an override key.
pub fn scope_key_for_app(app_key: &str) -> String {
    let key = crate::profile::normalize_key(app_key);
    if key.contains("gmail") || key.contains("mail") {
        return "email".to_string();
    }
    if key.contains("slack") || key.contains("teams") || key.contains("discord") {
        return "chat".to_string();
    }
    if key.contains("chatgpt") || key.contains("claude") || key.contains("cursor") {
        return "aichat".to_string();
    }
    if key.contains("code") || key.contains("terminal") {
        return "ide".to_string();
    }
    if key.contains("word") || key.contains("notes") || key.contains("document") {
        return "document".to_string();
    }
    key
}

/// JSON store for the bandit state. Local only; mirrors
/// [`crate::learning::feedback::FeedbackStore`] (missing file is empty, a
/// malformed file fails closed to the default rather than bricking the UI).
pub struct BanditStore {
    path: PathBuf,
}

impl BanditStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> io::Result<BanditState> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(BanditState::default()),
            Err(err) => Err(err),
        }
    }

    /// Serializes first and propagates any encode error, so a failed
    /// serialization can never truncate the store to an empty body. The bytes
    /// are written to a sibling temp file and renamed into place, so a crash
    /// mid-write cannot leave a half-written JSON file behind.
    pub fn save(&self, state: &BanditState) -> io::Result<()> {
        let body = serde_json::to_string_pretty(state)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let tmp = self.path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> io::Result<()> {
            std::fs::write(&tmp, body)?;
            std::fs::rename(&tmp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

/// Maps a T1 feedback kind to a bandit signal. `NeverChange` and `CopyRaw`
/// are not kept/undone votes — learning from them would be learning from an
/// ambiguous signal, so they map to `None`.
pub fn feedback_outcome(kind: super::feedback::FeedbackKind) -> Option<Outcome> {
    use super::feedback::FeedbackKind::*;
    match kind {
        ThumbUp => Some(Outcome::ThumbUp),
        ThumbDown => Some(Outcome::ThumbDown),
        Undo => Some(Outcome::Undone),
        NeverChange | CopyRaw => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PolishProfile::*;

    fn seed(state: &mut BanditState, app: &str, profile: PolishProfile, kept: u64, undone: u64) {
        for _ in 0..kept {
            state.record_outcome(app, profile, Some(Outcome::Kept));
        }
        for _ in 0..undone {
            state.record_outcome(app, profile, Some(Outcome::Undone));
        }
    }

    const NOW: u64 = 1_700_000_000_000;

    #[test]
    fn adjacency_is_the_clean_natural_structured_chain_only() {
        assert!(adjacent(Clean, Natural));
        assert!(adjacent(Natural, Clean));
        assert!(adjacent(Natural, Structured));
        assert!(adjacent(Structured, Natural));
        assert!(!adjacent(Clean, Structured));
        assert!(!adjacent(Structured, Clean));
        assert!(!adjacent(Clean, Clean));
        assert!(!adjacent(Natural, Transform));
        assert!(!adjacent(Natural, Off));
        assert!(!adjacent(Transform, Off));
    }

    #[test]
    fn learnable_profiles_exclude_transform_and_off() {
        assert!(is_learnable(Clean));
        assert!(is_learnable(Natural));
        assert!(is_learnable(Structured));
        assert!(!is_learnable(Transform));
        assert!(!is_learnable(Off));
    }

    #[test]
    fn ambiguous_and_settled_none_signals_move_nothing() {
        let mut state = BanditState::default();
        state.record_outcome("gmail", Natural, None);
        state.record_outcome("gmail", Natural, None);
        state.record_outcome("gmail", Transform, Some(Outcome::Kept));
        assert!(
            state.tallies.is_empty(),
            "an ambiguous signal created a tally"
        );
    }

    #[test]
    fn thumbs_fold_into_score_but_not_settled_count() {
        let mut state = BanditState::default();
        state.record_outcome("gmail", Natural, Some(Outcome::ThumbUp));
        state.record_outcome("gmail", Natural, Some(Outcome::ThumbDown));
        let tally = state.tally("gmail", Natural);
        assert_eq!(tally.settled, 0);
        assert_eq!(tally.score(), 0.5);
    }

    #[test]
    fn minimum_sample_is_twelve_settled_takes_on_both_sides() {
        let mut state = BanditState::default();
        // Incumbent has a weak kept-rate; challenger is strong but one take
        // short of the 12-take minimum.
        seed(&mut state, "gmail", Natural, 7, 5);
        seed(&mut state, "gmail", Structured, 11, 0);
        assert_eq!(state.decide("gmail", Natural, NOW), None);

        // The 12th settled take on the challenger opens the decision.
        seed(&mut state, "gmail", Structured, 1, 0);
        assert_eq!(state.decide("gmail", Natural, NOW), Some(Structured));
    }

    #[test]
    fn clear_win_margin_and_undone_guard() {
        // Incumbent: 12/20 kept = 0.60.
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Natural, 12, 8);
        // Challenger 13/20 = 0.65: only 0.05 better, not a clear win.
        seed(&mut state, "gmail", Structured, 13, 7);
        assert_eq!(state.decide("gmail", Natural, NOW), None);

        // A perfect but small challenger (12/12 = 1.00) clears 0.15.
        let mut state2 = BanditState::default();
        seed(&mut state2, "gmail", Natural, 12, 8);
        seed(&mut state2, "gmail", Structured, 12, 0);
        assert_eq!(state2.decide("gmail", Natural, NOW), Some(Structured));

        // Two undones break the clear win even at a higher kept-rate.
        let mut state3 = BanditState::default();
        seed(&mut state3, "gmail", Natural, 12, 8);
        seed(&mut state3, "gmail", Structured, 18, 2);
        assert_eq!(state3.decide("gmail", Natural, NOW), None);

        // A single undone is allowed; 18/19 = 0.947 still clears 0.15.
        let mut state4 = BanditState::default();
        seed(&mut state4, "gmail", Natural, 12, 8);
        seed(&mut state4, "gmail", Structured, 18, 1);
        assert_eq!(state4.decide("gmail", Natural, NOW), Some(Structured));

        // A perfect incumbent is never replaced: no challenger can beat 1.0
        // by the margin, so the loop stays put rather than churning.
        let mut state5 = BanditState::default();
        seed(&mut state5, "gmail", Natural, 20, 0);
        seed(&mut state5, "gmail", Structured, 40, 0);
        assert_eq!(state5.decide("gmail", Natural, NOW), None);
    }

    #[test]
    fn never_switches_to_transform_or_off() {
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Transform, 30, 0);
        seed(&mut state, "gmail", Off, 30, 0);
        seed(&mut state, "gmail", Natural, 12, 0);
        assert_eq!(state.decide("gmail", Natural, NOW), None);
        assert_eq!(state.decide("gmail", Transform, NOW), None);
        assert_eq!(state.decide("gmail", Off, NOW), None);
    }

    #[test]
    fn cannot_jump_more_than_one_step() {
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Clean, 20, 0);
        seed(&mut state, "gmail", Structured, 20, 0);
        // Structured is two steps from Clean: not adjacent.
        assert_eq!(state.decide("gmail", Clean, NOW), None);
    }

    #[test]
    fn one_change_per_app_per_week_cap() {
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Natural, 12, 8);
        seed(&mut state, "gmail", Structured, 18, 0);
        assert!(state.apply_change("gmail", Natural, Structured, NOW));
        // Still a clear win a day later, but the cap blocks a second change.
        assert_eq!(state.decide("gmail", Natural, NOW + 86_400_000), None);
        // A different app is unaffected.
        seed(&mut state, "slack", Natural, 12, 8);
        seed(&mut state, "slack", Structured, 18, 0);
        assert_eq!(state.decide("slack", Natural, NOW), Some(Structured));
        // After the week the cap expires.
        assert_eq!(
            state.decide("gmail", Natural, NOW + CHANGE_CAP_MS),
            Some(Structured)
        );
    }

    #[test]
    fn apply_change_enforces_adjacency_and_learnability() {
        let mut state = BanditState::default();
        assert!(!state.apply_change("gmail", Natural, Transform, NOW));
        assert!(!state.apply_change("gmail", Natural, Off, NOW));
        assert!(!state.apply_change("gmail", Clean, Structured, NOW));
        assert!(state.apply_change("gmail", Natural, Structured, NOW));
    }

    #[test]
    fn undo_restores_the_previous_profile() {
        let mut state = BanditState::default();
        assert!(state.apply_change("gmail", Natural, Structured, NOW));
        assert_eq!(state.undo_change("gmail"), Some(Natural));
        assert!(state.active_change("gmail").is_none());
        // A second undo is a no-op; there is no active change to restore.
        assert_eq!(state.undo_change("gmail"), None);
        // The undone change still consumes the week's cap.
        assert!(state.changed_within_cap("gmail", NOW + 1000));
    }

    #[test]
    fn choose_defaults_to_recommended_when_the_rule_does_not_fire() {
        let state = BanditState::default();
        assert_eq!(state.choose("gmail", Structured, NOW), Structured);
        assert_eq!(state.choose("gmail", Transform, NOW), Transform);
    }

    #[test]
    fn choose_honors_active_change_after_apply_change() {
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Natural, 12, 8);
        seed(&mut state, "gmail", Structured, 18, 0);
        assert_eq!(state.choose("gmail", Natural, NOW), Structured);

        assert!(state.apply_change("gmail", Natural, Structured, NOW));
        // The weekly cap now suppresses `decide`, but the active change is the
        // profile that should actually run on the next take.
        assert_eq!(state.decide("gmail", Natural, NOW + 1000), None);
        assert_eq!(state.choose("gmail", Natural, NOW + 1000), Structured);

        // Undoing the change restores the recommendation rather than replaying
        // the stale learned profile.
        assert_eq!(state.undo_change("gmail"), Some(Natural));
        assert_eq!(state.choose("gmail", Natural, NOW + 2000), Natural);
    }

    #[test]
    fn store_round_trip_and_missing_or_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = BanditStore::new(dir.path().join("bandit.json"));
        assert_eq!(store.load().unwrap(), BanditState::default());

        let mut state = BanditState::default();
        seed(&mut state, "gmail", Natural, 12, 0);
        assert!(state.apply_change("gmail", Natural, Structured, NOW));
        store.save(&state).unwrap();
        assert_eq!(store.load().unwrap(), state);

        std::fs::write(store.path(), "not json").unwrap();
        assert_eq!(store.load().unwrap(), BanditState::default());
    }

    #[test]
    fn save_round_trips_and_never_writes_an_empty_body() {
        let dir = tempfile::tempdir().unwrap();
        let store = BanditStore::new(dir.path().join("bandit.json"));
        let mut state = BanditState::default();
        seed(&mut state, "gmail", Natural, 12, 0);
        assert!(state.apply_change("gmail", Natural, Structured, NOW));

        store.save(&state).unwrap();

        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(
            !text.trim().is_empty(),
            "save fell back to an empty body instead of serializing state"
        );
        assert_eq!(store.load().unwrap(), state);

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains("tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "save left a temp file behind");
    }

    #[test]
    fn feedback_kinds_map_to_signals() {
        use super::super::feedback::FeedbackKind;
        assert_eq!(
            feedback_outcome(FeedbackKind::ThumbUp),
            Some(Outcome::ThumbUp)
        );
        assert_eq!(
            feedback_outcome(FeedbackKind::ThumbDown),
            Some(Outcome::ThumbDown)
        );
        assert_eq!(feedback_outcome(FeedbackKind::Undo), Some(Outcome::Undone));
        assert_eq!(feedback_outcome(FeedbackKind::NeverChange), None);
        assert_eq!(feedback_outcome(FeedbackKind::CopyRaw), None);
    }

    #[test]
    fn app_label_is_display_only() {
        assert_eq!(app_label("com.google.gmail"), "Gmail");
        assert_eq!(app_label("Slack"), "Slack");
        assert_eq!(app_label("com.example.form"), "Form");
        assert_eq!(app_label(""), "");
    }

    #[test]
    fn scope_key_maps_known_apps_to_their_category_override() {
        assert_eq!(scope_key_for_app("com.google.gmail"), "email");
        assert_eq!(scope_key_for_app("com.tinyspeck.slackmacgap"), "chat");
        assert_eq!(scope_key_for_app("com.openai.chatgpt"), "aichat");
        assert_eq!(scope_key_for_app("com.example.form"), "com.example.form");
    }
}
