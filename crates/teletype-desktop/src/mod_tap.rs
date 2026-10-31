//! Bare-modifier key detection (Ctrl, Cmd, Alt, Shift) via a CGEventTap.
//!
//! Carbon `RegisterEventHotKey` never fires for a lone modifier press (no
//! other key), so a single-modifier push-to-talk trigger is watched with a
//! passive event tap, the same approach as the Fn tap. The tap only reports
//! an edge while the target modifier is the sole modifier held, so typing
//! with that modifier (e.g. Ctrl+C) does not trigger dictation.
//!
//! Requires Accessibility permission, which the app already needs for typing.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use crate::dictation::{Controller, Event};

/// CGEventFlags bits, mirroring the constants in `mod_tap.m`. These are the
/// generic CoreGraphics masks: each covers both sides of its key (left and
/// right), which is what a bare-modifier hotkey must match. (Fn is handled
/// by the dedicated `fn_tap`, so it has no entry here.)
pub const MOD_CTRL: u32 = 0x0004_0000; // kCGEventFlagMaskControl
pub const MOD_ALT: u32 = 0x0008_0000; // kCGEventFlagMaskAlternate
pub const MOD_SHIFT: u32 = 0x0002_0000; // kCGEventFlagMaskShift
pub const MOD_CMD: u32 = 0x0010_0000; // kCGEventFlagMaskCommand (NX_COMMANDMASK)

/// Fn flag bit (NX_SECONDARYFNMASK), used by the sole-modifier decision.
#[cfg(test)]
pub const MOD_FN: u32 = 0x0080_0000;

/// Core of the bare-modifier tap decision, mirroring
/// `teletype_mod_tap_sole_down` in `mod_tap.m`. Returns `true` while `target`
/// (one of the `MOD_*` bits) is held and no other modifier, including Fn,
/// is. `flags` is the raw `flagsChanged` value.
#[cfg(test)]
fn mod_tap_sole_down(target: u32, flags: u32) -> bool {
    let all_other = MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_CMD;
    let target_down = flags & target != 0;
    let other_held = if target == MOD_FN {
        flags & all_other
    } else {
        (flags & all_other & !target) | (flags & MOD_FN)
    };
    target_down && other_held == 0
}

/// Maps a captured hotkey string to a bare-modifier bit, if it is one.
/// Returns `None` for combos (e.g. "Cmd+Space") and for "Fn", which the
/// dedicated Fn tap handles.
pub fn bare_modifier_mask(hotkey: &str) -> Option<u32> {
    match hotkey {
        "Ctrl" => Some(MOD_CTRL),
        "Alt" => Some(MOD_ALT),
        "Shift" => Some(MOD_SHIFT),
        "Cmd" => Some(MOD_CMD),
        _ => None,
    }
}

/// Durable sender used by the C callback (the tap outlives any command).
static TX: OnceLock<Sender<Event>> = OnceLock::new();
static RUNNING: Mutex<bool> = Mutex::new(false);

/// Starts the bare-modifier tap for `mask` (one of the MOD_* bits). No-op if
/// already running. Must be called from the main thread.
pub fn start(controller: Controller, mask: u32) -> Result<(), String> {
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    TX.set(controller.tx()).ok();
    extern "C" {
        fn teletype_mod_tap_start(cb: extern "C" fn(u8), target: u32) -> i32;
    }
    // SAFETY: installs a CGEventTap on the main run loop; the callback is a
    // plain C function pointer with no Rust state.
    let ok = unsafe { teletype_mod_tap_start(on_mod_state, mask) } != 0;
    if ok {
        *RUNNING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }
    if ok {
        Ok(())
    } else {
        Err("event tap could not be created".into())
    }
}

/// Stops the bare-modifier tap (no-op if not running).
pub fn stop() {
    let was = *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !was {
        return;
    }
    extern "C" {
        fn teletype_mod_tap_stop();
    }
    // SAFETY: removes the tap installed by teletype_mod_tap_start.
    unsafe { teletype_mod_tap_stop() }
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
}

/// C callback: 1 = modifier pressed (as sole modifier), 0 = released.
extern "C" fn on_mod_state(down: u8) {
    static LAST_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let now_down = down != 0;
    // flagsChanged repeats while held — only react to edges.
    if now_down == LAST_DOWN.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    LAST_DOWN.store(now_down, std::sync::atomic::Ordering::SeqCst);
    if now_down {
        crate::dictation::mark_hotkey_down();
    } else {
        crate::dictation::mark_hotkey_up();
    }
    if let Some(tx) = TX.get() {
        let _ = tx.send(if now_down {
            Event::HotkeyDown
        } else {
            Event::HotkeyUp
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Raw flagsChanged values exactly as macOS reports them: the generic CG
    // mask for the modifier down, plus the IOKit device bit for the physical
    // key, plus the 0x100 non-coalesced bit. These literals are independent
    // of the MOD_* constants, so a wrong constant cannot pass its own test.
    //
    //   left Control  0x40101   right Control  0x42100
    //   left Shift    0x20102   right Shift    0x20104
    //   left Command  0x100108  right Command  0x100110
    //   left Option   0x80120   right Option   0x80140
    //   Fn            0x800100

    #[test]
    fn ctrl_fires_on_left_and_right_control() {
        assert!(mod_tap_sole_down(MOD_CTRL, 0x40101));
        assert!(mod_tap_sole_down(MOD_CTRL, 0x42100));
        assert!(mod_tap_sole_down(MOD_CTRL, 0x40000));
    }

    #[test]
    fn alt_fires_on_left_and_right_option() {
        assert!(mod_tap_sole_down(MOD_ALT, 0x80120));
        assert!(mod_tap_sole_down(MOD_ALT, 0x80140));
        assert!(mod_tap_sole_down(MOD_ALT, 0x80000));
    }

    #[test]
    fn shift_fires_on_left_and_right_shift() {
        assert!(mod_tap_sole_down(MOD_SHIFT, 0x20102));
        assert!(mod_tap_sole_down(MOD_SHIFT, 0x20104));
        assert!(mod_tap_sole_down(MOD_SHIFT, 0x20000));
    }

    #[test]
    fn cmd_fires_on_left_and_right_command() {
        assert!(mod_tap_sole_down(MOD_CMD, 0x100108));
        assert!(mod_tap_sole_down(MOD_CMD, 0x100110));
        assert!(mod_tap_sole_down(MOD_CMD, 0x100000));
    }

    #[test]
    fn fn_fires_on_bare_fn() {
        assert!(mod_tap_sole_down(MOD_FN, 0x800100));
        assert!(!mod_tap_sole_down(MOD_FN, 0));
    }

    #[test]
    fn left_shift_must_not_trigger_alt() {
        // The old masks made MOD_ALT == 0x2 (NX_DEVICELSHIFTKEYMASK), so a
        // left Shift press fired the Alt hotkey and Option never did.
        assert!(!mod_tap_sole_down(MOD_ALT, 0x20102));
        assert!(!mod_tap_sole_down(MOD_ALT, 0x20104));
        assert!(mod_tap_sole_down(MOD_ALT, 0x80120));
    }

    #[test]
    fn another_mod_held_suppresses_the_target() {
        // Ctrl plus Command held together is neither a bare Ctrl nor a
        // bare Cmd.
        assert!(!mod_tap_sole_down(MOD_CTRL, 0x140109));
        assert!(!mod_tap_sole_down(MOD_CMD, 0x140109));
        assert!(!mod_tap_sole_down(MOD_CTRL, 0x60102));
        assert!(!mod_tap_sole_down(MOD_ALT, 0x180108));
    }

    #[test]
    fn fn_held_suppresses_the_target() {
        assert!(!mod_tap_sole_down(MOD_CTRL, 0x40101 | 0x800000));
        assert!(!mod_tap_sole_down(MOD_FN, 0x800000 | 0x40101));
        assert!(mod_tap_sole_down(MOD_FN, 0x800100));
    }

    #[test]
    fn nothing_held_is_inactive() {
        for target in [MOD_CTRL, MOD_ALT, MOD_SHIFT, MOD_CMD, MOD_FN] {
            assert!(!mod_tap_sole_down(target, 0));
        }
    }

    #[test]
    fn masks_match_the_generic_cg_masks() {
        // Literal values, so a wrong constant fails loudly here.
        assert_eq!(MOD_CTRL, 0x0004_0000);
        assert_eq!(MOD_ALT, 0x0008_0000);
        assert_eq!(MOD_SHIFT, 0x0002_0000);
        assert_eq!(MOD_CMD, 0x0010_0000);
        assert_eq!(MOD_FN, 0x0080_0000);
    }

    #[test]
    fn hotkey_strings_map_to_generic_masks() {
        assert_eq!(bare_modifier_mask("Ctrl"), Some(MOD_CTRL));
        assert_eq!(bare_modifier_mask("Alt"), Some(MOD_ALT));
        assert_eq!(bare_modifier_mask("Shift"), Some(MOD_SHIFT));
        assert_eq!(bare_modifier_mask("Cmd"), Some(MOD_CMD));
        assert_eq!(bare_modifier_mask("Fn"), None);
        assert_eq!(bare_modifier_mask("Cmd+Space"), None);
    }
}
