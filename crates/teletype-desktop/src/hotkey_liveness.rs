//! Hotkey liveness: which bindings can be observed, and are they down?
//!
//! Bare Fn and bare modifiers are watched by CGEventTaps (`fn_tap`,
//! `mod_tap`), which expose a native liveness signal. Combo bindings (e.g.
//! "Cmd+Space") are registered with the global-shortcut plugin instead and
//! have no tap-level observer, so they are not liveness-observable here.

use crate::{fn_tap, mod_tap};

/// The native listener, if any, that can observe a binding's physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyTapKind {
    /// Bare Fn, watched by the dedicated Fn event tap.
    Fn,
    /// A single modifier (mask from `mod_tap::bare_modifier_mask`).
    BareModifier(u32),
    /// Combos and unknown bindings: no observer, not liveness-observable.
    NotObservable,
}

/// Maps a hotkey binding string to the native tap that can observe it.
pub(crate) fn tap_kind(binding: &str) -> HotkeyTapKind {
    match binding {
        "Fn" => HotkeyTapKind::Fn,
        _ => match mod_tap::bare_modifier_mask(binding) {
            Some(mask) => HotkeyTapKind::BareModifier(mask),
            None => HotkeyTapKind::NotObservable,
        },
    }
}

/// Whether the physical key for `binding` is currently held, when observable.
///
/// `None` for combos (e.g. "Cmd+Space") and unknown bindings: those have no
/// tap-level liveness signal.
pub(crate) fn hotkey_live(binding: &str) -> Option<bool> {
    match tap_kind(binding) {
        HotkeyTapKind::Fn => Some(fn_tap::is_live()),
        HotkeyTapKind::BareModifier(_) => Some(mod_tap::is_live()),
        HotkeyTapKind::NotObservable => None,
    }
}

/// Whether a hotkey is physically held, per the timestamp stamped by
/// `mark_hotkey_down`/`mark_hotkey_up`.
pub(crate) fn hotkey_physically_down() -> bool {
    crate::dictation::hotkey_physically_down()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_kind_maps_fn_and_bare_modifiers() {
        assert_eq!(tap_kind("Fn"), HotkeyTapKind::Fn);
        assert_eq!(
            tap_kind("Ctrl"),
            HotkeyTapKind::BareModifier(mod_tap::MOD_CTRL)
        );
        assert_eq!(
            tap_kind("Alt"),
            HotkeyTapKind::BareModifier(mod_tap::MOD_ALT)
        );
        assert_eq!(
            tap_kind("Shift"),
            HotkeyTapKind::BareModifier(mod_tap::MOD_SHIFT)
        );
        assert_eq!(
            tap_kind("Cmd"),
            HotkeyTapKind::BareModifier(mod_tap::MOD_CMD)
        );
    }

    #[test]
    fn tap_kind_combos_and_unknown_are_not_observable() {
        assert_eq!(tap_kind("Cmd+Space"), HotkeyTapKind::NotObservable);
        assert_eq!(tap_kind("Ctrl+Alt+Delete"), HotkeyTapKind::NotObservable);
        assert_eq!(tap_kind("F19"), HotkeyTapKind::NotObservable);
        assert_eq!(tap_kind(""), HotkeyTapKind::NotObservable);
    }

    #[test]
    fn hotkey_live_is_none_for_combos() {
        assert_eq!(hotkey_live("Cmd+Space"), None);
        assert_eq!(hotkey_live("Ctrl+Alt+Delete"), None);
        assert_eq!(hotkey_live("F19"), None);
    }

    #[test]
    fn hotkey_live_is_some_for_observable_bindings() {
        // No tap is started during unit tests, so native liveness is false;
        // the point is that these bindings are observable (Some), unlike combos.
        assert!(hotkey_live("Fn").is_some());
        assert!(hotkey_live("Ctrl").is_some());
        assert!(hotkey_live("Alt").is_some());
        assert!(hotkey_live("Shift").is_some());
        assert!(hotkey_live("Cmd").is_some());
    }

    #[test]
    fn physical_down_tracks_marks() {
        crate::dictation::mark_hotkey_up();
        assert!(!hotkey_physically_down());
        crate::dictation::mark_hotkey_down();
        assert!(hotkey_physically_down());
        crate::dictation::mark_hotkey_up();
        assert!(!hotkey_physically_down());
    }
}
