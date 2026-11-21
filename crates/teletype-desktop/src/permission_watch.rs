//! Live permission and hotkey-liveness watcher.
//!
//! The hotkey event taps (`fn_tap`, `mod_tap`) need Accessibility. When the
//! grant arrives after launch nothing used to re-register them, so a restart
//! was the only fix. This thread samples the platform permissions, tells the
//! UI when they change (`permissions-changed`), and re-registers the hotkey on
//! the transition to granted.
//!
//! It also samples tap-level hotkey liveness (`hotkey_liveness::hotkey_live`).
//! A binding that goes live -> dead is recovered by re-registering the hotkey,
//! but only after two consecutive dead samples, with Accessibility granted and
//! the key not physically held. Baseline/liveness changes are published as the
//! `hotkey-status` event so the UI can show "active", "inactive" or
//! "not checkable" (string combos have no tap-level liveness signal).
//!
//! The decision is a pure function ([`decide`], [`decide_liveness`]) so it is
//! unit-tested without TCC. Polling is 1 s while a permission is missing and a
//! relaxed 5 s once all are granted, which still notices a later revocation.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use teletype_core::platform::Permission;

use crate::commands::HotkeyStatus;
use crate::AppState;

/// Poll period while at least one permission is missing.
pub const POLL_MISSING: Duration = Duration::from_secs(1);
/// Poll period once everything is granted (only watching for revocation).
pub const POLL_GRANTED: Duration = Duration::from_secs(5);

/// Which permissions are satisfied, plus the sampled tap-level liveness.
/// `Unsupported` counts as satisfied: there is nothing for the user to grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub microphone: bool,
    pub accessibility: bool,
    /// `Some(true)` live, `Some(false)` dead, `None` for bindings with no
    /// tap-level observer (string combos).
    pub hotkey_live: Option<bool>,
}

impl Snapshot {
    pub fn from_permissions(perms: &[Permission]) -> Self {
        use teletype_core::platform::{PermissionKind, PermissionState};
        let ok = |kind: PermissionKind| {
            perms.iter().filter(|p| p.kind == kind).all(|p| {
                matches!(
                    p.state,
                    PermissionState::Granted | PermissionState::Unsupported
                )
            })
        };
        Self {
            microphone: ok(PermissionKind::Microphone),
            accessibility: ok(PermissionKind::Accessibility),
            hotkey_live: None,
        }
    }

    pub fn all_granted(self) -> bool {
        self.microphone && self.accessibility
    }
}

/// Cross-tick state for liveness hysteresis and one-recovery-per-episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LivenessState {
    /// Last committed live/dead value. Stays `Some(true)` through a dead
    /// streak so the recovery condition can compare against "was live".
    pub last_live: Option<bool>,
    /// Consecutive `Some(false)` samples since the last live sample.
    pub dead_streak: u32,
}

/// Folds one liveness sample into the hysteresis state. A live sample resets
/// the streak and commits `last_live = Some(true)`; a dead sample only extends
/// the streak; an unobservable binding (`None`) leaves the state untouched.
pub fn advance_liveness(state: &mut LivenessState, now_live: Option<bool>) {
    match now_live {
        Some(true) => {
            state.last_live = Some(true);
            state.dead_streak = 0;
        }
        Some(false) => state.dead_streak = state.dead_streak.saturating_add(1),
        None => {}
    }
}

/// What the watcher should do after a new sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Tell the UI permissions changed (any change, including a grant being
    /// revoked). Unchanged from the permission-only watcher.
    pub notify: bool,
    /// Re-run hotkey registration (permission grant OR liveness recovery).
    pub reregister: bool,
    /// Publish the `hotkey-status` event (baseline or liveness change).
    pub hotkey_status_changed: bool,
}

/// Compares the previous sample with the new one. The first sample (`None`)
/// only establishes a permission baseline: startup already registered the
/// hotkey, but liveness status is still published so the UI learns the initial
/// state. This is the permission-only base; [`decide_liveness`] adds hotkey
/// recovery on top of it.
pub fn decide(prev: Option<Snapshot>, now: Snapshot) -> Decision {
    let baseline = prev.is_none();
    Decision {
        notify: prev.is_some_and(|p| {
            p.microphone != now.microphone || p.accessibility != now.accessibility
        }),
        reregister: prev.is_some_and(|p| {
            (!p.microphone && now.microphone) || (!p.accessibility && now.accessibility)
        }),
        hotkey_status_changed: baseline || prev.is_some_and(|p| p.hotkey_live != now.hotkey_live),
    }
}

/// Full decision used by the watcher: permission reregistration plus
/// liveness-triggered recovery.
///
/// The hotkey is re-registered only when the previously committed liveness was
/// live, the current sample is dead, two consecutive dead samples were seen,
/// Accessibility is granted, and the key is not physically held. Permission
/// grants keep their original, independent reregister condition.
pub fn decide_liveness(
    prev: Option<Snapshot>,
    now: Snapshot,
    liveness: &LivenessState,
    physically_down: bool,
) -> Decision {
    let mut decision = decide(prev, now);
    let previous_live = liveness
        .last_live
        .or_else(|| prev.and_then(|p| p.hotkey_live));
    let liveness_reregister = prev.is_some()
        && previous_live == Some(true)
        && now.hotkey_live == Some(false)
        && liveness.dead_streak >= 2
        && now.accessibility
        && !physically_down;
    decision.reregister |= liveness_reregister;
    decision
}

/// How long to wait before the next sample.
pub fn poll_interval(now: Snapshot) -> Duration {
    if now.all_granted() {
        POLL_GRANTED
    } else {
        POLL_MISSING
    }
}

/// Process-wide guard: only one re-registration may be in flight at a time.
static REREGISTER_LOCK: Mutex<()> = Mutex::new(());
/// Last re-registration failure, surfaced by the `hotkey_status` command.
static LAST_REREGISTER_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Records (or clears) the last re-registration error for the UI.
pub(crate) fn set_last_reregister_error(error: Option<String>) {
    if let Ok(mut slot) = LAST_REREGISTER_ERROR.lock() {
        *slot = error;
    }
}

/// The last re-registration error, if any.
pub(crate) fn last_reregister_error() -> Option<String> {
    LAST_REREGISTER_ERROR
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

/// Whether this process was launched in the e2e harness (no real hotkey).
fn e2e_mode() -> bool {
    std::env::args().any(|a| a == "--e2e")
}

/// Binding used for liveness sampling: the saved binding, or `"Fn"` as a
/// conservative default when unset/unavailable. String combos are passed
/// through and reported as `None` by `hotkey_live` ("not checkable").
pub(crate) fn liveness_binding(state: Option<&AppState>) -> String {
    state
        .map(|s| s.settings.get().hotkey.clone())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "Fn".into())
}

/// Binding used for actual registration (preserves the old default-hotkey
/// fallback).
pub(crate) fn configured_hotkey(state: &AppState) -> String {
    let configured = state.settings.get().hotkey.clone();
    if configured.is_empty() {
        state.platform.default_hotkey()
    } else {
        configured
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn sample_hotkey_live(binding: &str) -> Option<bool> {
    crate::hotkey_liveness::hotkey_live(binding)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn sample_hotkey_live(_binding: &str) -> Option<bool> {
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn hotkey_is_physically_down() -> bool {
    crate::hotkey_liveness::hotkey_physically_down()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hotkey_is_physically_down() -> bool {
    false
}

/// Starts the watcher thread. Call once, after `AppState` is managed.
pub fn spawn(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("teletype-permission-watch".into())
        .spawn(move || run(app));
    if let Err(e) = spawned {
        tracing::warn!("permission watcher did not start: {e}");
    }
}

fn run(app: AppHandle) {
    let mut prev: Option<Snapshot> = None;
    let mut liveness = LivenessState::default();
    let mut wait = POLL_MISSING;
    loop {
        std::thread::sleep(wait);
        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };
        let platform = state.platform.as_ref();
        let binding = liveness_binding(Some(&state));
        let physically_down = hotkey_is_physically_down();
        let notify = |perms: &[Permission]| {
            let _ = app.emit("permissions-changed", perms);
        };
        let reregister = || {
            if let Err(e) = reregister_hotkey_now(&app) {
                set_last_reregister_error(Some(e));
            }
        };
        let status = || {
            let status = HotkeyStatus::current(&state);
            let _ = app.emit("hotkey-status", &status);
        };
        wait = tick(
            &mut prev,
            &mut liveness,
            platform,
            &binding,
            physically_down,
            &notify,
            &reregister,
            &status,
        );
    }
}

/// One watcher iteration: sample permissions and hotkey liveness, decide, fire
/// the side effects. Returns the interval to wait before the next sample. The
/// decision state is kept in `prev`/`liveness` so the caller can drive the loop
/// with an injectable platform and poll interval.
#[allow(clippy::too_many_arguments)]
fn tick<P, N, R, S>(
    prev: &mut Option<Snapshot>,
    liveness: &mut LivenessState,
    platform: &P,
    binding: &str,
    physically_down: bool,
    notify: &N,
    reregister: &R,
    status: &S,
) -> Duration
where
    P: teletype_core::platform::Platform + ?Sized,
    N: Fn(&[Permission]),
    R: Fn(),
    S: Fn(),
{
    let perms = platform.permissions();
    let mut now = Snapshot::from_permissions(&perms);
    now.hotkey_live = sample_hotkey_live(binding);
    advance_liveness(liveness, now.hotkey_live);
    let decision = decide_liveness(*prev, now, liveness, physically_down);
    *prev = Some(now);
    if decision.notify {
        notify(&perms);
    }
    if decision.hotkey_status_changed {
        status();
    }
    if decision.reregister {
        reregister();
        // One recovery per live -> dead episode: committing the dead value
        // stops a long dead streak from re-registering on every sample.
        liveness.last_live = Some(false);
    }
    poll_interval(now)
}

/// Re-registers the dictation hotkey on the main thread (the taps attach to
/// the main run loop). Shared by the permission/liveness watcher and the
/// `reregister_hotkey` command. The process-wide lock is held for the whole
/// helper so two callers cannot queue duplicate registrations.
pub(crate) fn reregister_hotkey_now(app: &AppHandle) -> Result<(), String> {
    let _guard = REREGISTER_LOCK
        .lock()
        .map_err(|e| format!("re-registration lock is unavailable: {e}"))?;

    // The e2e harness skips the watcher and has no real hotkey: stay a no-op.
    if e2e_mode() {
        return Ok(());
    }

    if hotkey_is_physically_down() {
        let err = "Hotkey is physically held; not re-registering it".to_string();
        set_last_reregister_error(Some(err.clone()));
        return Err(err);
    }

    let state = app.try_state::<AppState>().ok_or_else(|| {
        let err = "App state is unavailable; cannot re-register hotkey".to_string();
        set_last_reregister_error(Some(err.clone()));
        err
    })?;
    let hotkey = configured_hotkey(&state);
    tracing::info!("re-registering hotkey {hotkey}");
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        let Some(state) = handle.try_state::<AppState>() else {
            return;
        };
        match state.controller.register_hotkey(&handle, &hotkey) {
            Ok(()) => {
                tracing::info!("hotkey {hotkey} registered");
                set_last_reregister_error(None);
            }
            Err(e) => {
                tracing::warn!("hotkey {hotkey} still not registered: {e}");
                set_last_reregister_error(Some(e));
            }
        }
        let status = HotkeyStatus::current(&state);
        let _ = handle.emit("hotkey-status", &status);
    });
    if let Err(e) = queued {
        let err = format!("could not queue hotkey re-registration: {e}");
        set_last_reregister_error(Some(err.clone()));
        return Err(err);
    }
    set_last_reregister_error(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::platform::{PermissionKind, PermissionState};

    const fn snap(microphone: bool, accessibility: bool) -> Snapshot {
        Snapshot {
            microphone,
            accessibility,
            hotkey_live: None,
        }
    }

    #[test]
    fn first_sample_is_only_a_baseline() {
        let d = decide(None, snap(true, false));
        assert!(!d.notify && !d.reregister);
    }

    #[test]
    fn missing_to_granted_reregisters_exactly_once() {
        let missing = snap(true, false);
        let granted = snap(true, true);
        let d = decide(Some(missing), granted);
        assert!(d.notify && d.reregister);
        // The next sample is unchanged: quiet, no second registration.
        let d = decide(Some(granted), granted);
        assert!(!d.notify && !d.reregister);
    }

    #[test]
    fn microphone_grant_also_reregisters() {
        let d = decide(Some(snap(false, true)), snap(true, true));
        assert!(d.notify && d.reregister);
    }

    #[test]
    fn still_missing_stays_quiet() {
        let d = decide(Some(snap(true, false)), snap(true, false));
        assert!(!d.notify && !d.reregister);
    }

    #[test]
    fn granted_to_revoked_is_reflected_without_reregistering() {
        let d = decide(Some(snap(true, true)), snap(true, false));
        assert!(d.notify);
        assert!(!d.reregister);
        // And granting it back later registers again.
        assert!(decide(Some(snap(true, false)), snap(true, true)).reregister);
    }

    #[test]
    fn polls_fast_only_while_something_is_missing() {
        assert_eq!(poll_interval(snap(true, false)), POLL_MISSING);
        assert_eq!(poll_interval(snap(false, false)), POLL_MISSING);
        assert_eq!(poll_interval(snap(true, true)), POLL_GRANTED);
    }

    #[test]
    fn snapshot_treats_unsupported_as_satisfied_and_denied_as_missing() {
        let p = |kind, state| Permission { kind, state };
        let s = Snapshot::from_permissions(&[
            p(PermissionKind::Microphone, PermissionState::NotDetermined),
            p(PermissionKind::Accessibility, PermissionState::Unsupported),
        ]);
        assert_eq!(s, snap(false, true));
        let s = Snapshot::from_permissions(&[
            p(PermissionKind::Microphone, PermissionState::Granted),
            p(PermissionKind::Accessibility, PermissionState::Denied),
        ]);
        assert_eq!(s, snap(true, false));
    }

    /// A platform whose permission state can be flipped between samples.
    struct ScriptedPlatform {
        accessibility: PermissionState,
    }

    impl teletype_core::platform::Platform for ScriptedPlatform {
        fn active_application(&self) -> Option<teletype_core::context::ApplicationContext> {
            None
        }
        fn permissions(&self) -> Vec<Permission> {
            vec![
                Permission {
                    kind: PermissionKind::Microphone,
                    state: PermissionState::Granted,
                },
                Permission {
                    kind: PermissionKind::Accessibility,
                    state: self.accessibility,
                },
            ]
        }
        fn request_permission(&self, _kind: PermissionKind) {}
        fn permission_settings_url(&self, _kind: PermissionKind) -> Option<String> {
            None
        }
        fn default_hotkey(&self) -> String {
            "fn".into()
        }
        fn paste_shortcut(&self) -> Option<teletype_core::platform::PasteShortcut> {
            None
        }
        fn setup_notes(&self) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn loop_denied_then_granted_reregisters_exactly_once_and_notifies() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let reregister_count = Arc::new(AtomicUsize::new(0));
        let notify_count = Arc::new(AtomicUsize::new(0));
        let status_count = Arc::new(AtomicUsize::new(0));
        let platform = Arc::new(std::sync::Mutex::new(ScriptedPlatform {
            accessibility: PermissionState::Denied,
        }));

        let notify = |perms: &[Permission]| {
            let _ = perms;
            notify_count.fetch_add(1, Ordering::SeqCst);
        };
        let reregister = || {
            reregister_count.fetch_add(1, Ordering::SeqCst);
        };
        let status = || {
            status_count.fetch_add(1, Ordering::SeqCst);
        };

        let mut prev: Option<Snapshot> = None;
        let mut liveness = LivenessState::default();

        // Sample 1: denied. Baseline only, no side effects (status is emitted
        // once so the UI learns the initial liveness).
        let wait = {
            let p = platform.lock().unwrap();
            tick(
                &mut prev,
                &mut liveness,
                &*p,
                "Fn",
                false,
                &notify,
                &reregister,
                &status,
            )
        };
        assert_eq!(wait, POLL_MISSING);
        assert_eq!(reregister_count.load(Ordering::SeqCst), 0);
        assert_eq!(notify_count.load(Ordering::SeqCst), 0);
        assert_eq!(status_count.load(Ordering::SeqCst), 1);

        // Sample 2: still denied. No change, no side effects.
        let wait = {
            let p = platform.lock().unwrap();
            tick(
                &mut prev,
                &mut liveness,
                &*p,
                "Fn",
                false,
                &notify,
                &reregister,
                &status,
            )
        };
        assert_eq!(wait, POLL_MISSING);
        assert_eq!(reregister_count.load(Ordering::SeqCst), 0);
        assert_eq!(notify_count.load(Ordering::SeqCst), 0);
        assert_eq!(status_count.load(Ordering::SeqCst), 1);

        // Flip to granted.
        platform.lock().unwrap().accessibility = PermissionState::Granted;

        // Sample 3: granted. The transition fires notify + reregister once.
        let wait = {
            let p = platform.lock().unwrap();
            tick(
                &mut prev,
                &mut liveness,
                &*p,
                "Fn",
                false,
                &notify,
                &reregister,
                &status,
            )
        };
        assert_eq!(wait, POLL_GRANTED);
        assert_eq!(reregister_count.load(Ordering::SeqCst), 1);
        assert_eq!(notify_count.load(Ordering::SeqCst), 1);
        assert_eq!(status_count.load(Ordering::SeqCst), 1);

        // Sample 4: still granted. Quiet, no second registration.
        let wait = {
            let p = platform.lock().unwrap();
            tick(
                &mut prev,
                &mut liveness,
                &*p,
                "Fn",
                false,
                &notify,
                &reregister,
                &status,
            )
        };
        assert_eq!(wait, POLL_GRANTED);
        assert_eq!(reregister_count.load(Ordering::SeqCst), 1);
        assert_eq!(notify_count.load(Ordering::SeqCst), 1);
        assert_eq!(status_count.load(Ordering::SeqCst), 1);
    }

    // ---- Hotkey liveness recovery ----

    const fn live(accessibility: bool) -> Snapshot {
        Snapshot {
            microphone: true,
            accessibility,
            hotkey_live: Some(true),
        }
    }

    const fn dead(accessibility: bool) -> Snapshot {
        Snapshot {
            microphone: true,
            accessibility,
            hotkey_live: Some(false),
        }
    }

    /// State after a live sample followed by `dead_count` dead samples.
    fn streak(dead_count: u32) -> LivenessState {
        let mut state = LivenessState::default();
        advance_liveness(&mut state, Some(true));
        for _ in 0..dead_count {
            advance_liveness(&mut state, Some(false));
        }
        state
    }

    #[test]
    fn live_to_dead_with_one_sample_reports_status_but_does_not_reregister() {
        let d = decide_liveness(Some(live(true)), dead(true), &streak(1), false);
        assert!(d.hotkey_status_changed);
        assert!(!d.reregister, "hysteresis: first dead sample is not enough");
        assert!(!d.notify, "permissions did not change");
    }

    #[test]
    fn two_dead_samples_reregister_and_stop_after_one_recovery() {
        let d = decide_liveness(Some(live(true)), dead(true), &streak(2), false);
        assert!(d.hotkey_status_changed);
        assert!(d.reregister);
        assert!(!d.notify);

        // The watcher commits the dead value after recovering, so a still-dead
        // streak cannot re-register on every subsequent sample.
        let committed = LivenessState {
            last_live: Some(false),
            dead_streak: 3,
        };
        let d = decide_liveness(Some(dead(true)), dead(true), &committed, false);
        assert!(!d.hotkey_status_changed);
        assert!(!d.reregister);
    }

    #[test]
    fn live_to_dead_while_key_held_reports_status_but_does_not_reregister() {
        let d = decide_liveness(Some(live(true)), dead(true), &streak(2), true);
        assert!(d.hotkey_status_changed);
        assert!(!d.reregister, "do not touch a physically held hotkey");
    }

    #[test]
    fn live_to_dead_without_accessibility_never_reregisters() {
        let d = decide_liveness(Some(live(false)), dead(false), &streak(2), false);
        assert!(d.hotkey_status_changed);
        assert!(
            !d.reregister,
            "tap cannot be restored without Accessibility"
        );
    }

    #[test]
    fn baseline_dead_reports_status_but_never_reregisters() {
        let mut state = LivenessState::default();
        advance_liveness(&mut state, Some(false));
        let d = decide_liveness(None, dead(true), &state, false);
        assert!(
            d.hotkey_status_changed,
            "UI must learn the initial dead state"
        );
        assert!(!d.reregister, "startup already registered the hotkey");
    }

    #[test]
    fn dead_to_live_reports_status_and_does_not_reregister() {
        let d = decide_liveness(Some(dead(true)), live(true), &streak(2), false);
        assert!(d.hotkey_status_changed);
        assert!(!d.reregister);
    }

    #[test]
    fn unchanged_liveness_stays_quiet() {
        let committed = LivenessState {
            last_live: Some(false),
            dead_streak: 3,
        };
        let d = decide_liveness(Some(dead(true)), dead(true), &committed, false);
        assert!(!d.hotkey_status_changed);
        assert!(!d.reregister);
    }

    #[test]
    fn unobservable_binding_reports_no_liveness_change() {
        let d = decide_liveness(Some(snap(true, true)), snap(true, true), &streak(0), false);
        assert!(!d.hotkey_status_changed, "combo bindings are not checkable");
        assert!(!d.reregister);
    }

    #[test]
    fn advance_liveness_handles_unobservable_samples_without_losing_streak() {
        let mut state = LivenessState::default();
        advance_liveness(&mut state, Some(true));
        advance_liveness(&mut state, Some(false));
        advance_liveness(&mut state, None);
        advance_liveness(&mut state, Some(false));
        assert_eq!(state, streak(2));
    }
}
