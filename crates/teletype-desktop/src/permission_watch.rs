//! Live permission watcher.
//!
//! The hotkey event taps (`fn_tap`, `mod_tap`) need Accessibility. When the
//! grant arrives after launch nothing used to re-register them, so a restart
//! was the only fix. This thread samples the platform permissions, tells the
//! UI when they change (`permissions-changed`), and re-registers the hotkey on
//! the transition to granted.
//!
//! The decision is a pure function ([`decide`]) so it is unit-tested without
//! TCC. Polling is 1 s while a permission is missing and a relaxed 5 s once all
//! are granted, which still notices a later revocation.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use teletype_core::platform::Permission;

use crate::AppState;

/// Poll period while at least one permission is missing.
pub const POLL_MISSING: Duration = Duration::from_secs(1);
/// Poll period once everything is granted (only watching for revocation).
pub const POLL_GRANTED: Duration = Duration::from_secs(5);

/// Which permissions are satisfied. `Unsupported` counts as satisfied: there
/// is nothing for the user to grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub microphone: bool,
    pub accessibility: bool,
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
        }
    }

    pub fn all_granted(self) -> bool {
        self.microphone && self.accessibility
    }
}

/// What the watcher should do after a new sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Tell the UI (any change, including a grant being revoked).
    pub notify: bool,
    /// Re-run hotkey registration (a permission went missing -> granted).
    pub reregister: bool,
}

/// Compares the previous sample with the new one. The first sample (`None`)
/// only establishes a baseline: startup already registered the hotkey.
pub fn decide(prev: Option<Snapshot>, now: Snapshot) -> Decision {
    let Some(prev) = prev else {
        return Decision {
            notify: false,
            reregister: false,
        };
    };
    Decision {
        notify: prev != now,
        reregister: (!prev.microphone && now.microphone)
            || (!prev.accessibility && now.accessibility),
    }
}

/// How long to wait before the next sample.
pub fn poll_interval(now: Snapshot) -> Duration {
    if now.all_granted() {
        POLL_GRANTED
    } else {
        POLL_MISSING
    }
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
    let mut wait = POLL_MISSING;
    loop {
        std::thread::sleep(wait);
        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };
        let perms = state.platform.permissions();
        let now = Snapshot::from_permissions(&perms);
        let decision = decide(prev, now);
        prev = Some(now);
        wait = poll_interval(now);
        if decision.notify {
            let _ = app.emit("permissions-changed", &perms);
        }
        if decision.reregister {
            reregister_hotkey(&app, &state);
        }
    }
}

/// Re-registers the dictation hotkey on the main thread (the taps attach to the
/// main run loop).
fn reregister_hotkey(app: &AppHandle, state: &AppState) {
    let configured = state.settings.get().hotkey.clone();
    let hotkey = if configured.is_empty() {
        state.platform.default_hotkey()
    } else {
        configured
    };
    tracing::info!("permission granted while running: re-registering hotkey {hotkey}");
    let handle = app.clone();
    let queued = app.run_on_main_thread(move || {
        let Some(state) = handle.try_state::<AppState>() else {
            return;
        };
        match state.controller.register_hotkey(&handle, &hotkey) {
            Ok(()) => tracing::info!("hotkey {hotkey} registered after permission grant"),
            Err(e) => tracing::warn!("hotkey {hotkey} still not registered: {e}"),
        }
    });
    if let Err(e) = queued {
        tracing::warn!("could not queue hotkey re-registration: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::platform::{PermissionKind, PermissionState};

    const fn snap(microphone: bool, accessibility: bool) -> Snapshot {
        Snapshot {
            microphone,
            accessibility,
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
}
