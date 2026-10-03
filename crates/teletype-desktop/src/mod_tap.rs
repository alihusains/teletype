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

/// CGEventFlags bits, mirroring the constants in `mod_tap.m`. (Fn is handled
/// by the dedicated `fn_tap`, so it has no entry here.)
pub const MOD_CTRL: u32 = 0x00000001;
pub const MOD_ALT: u32 = 0x00000002;
pub const MOD_SHIFT: u32 = 0x00000004;
pub const MOD_CMD: u32 = 0x00000008;

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
    if let Some(tx) = TX.get() {
        let _ = tx.send(if now_down {
            Event::HotkeyDown
        } else {
            Event::HotkeyUp
        });
    }
}
