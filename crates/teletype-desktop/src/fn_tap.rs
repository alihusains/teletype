//! Bare-Fn key detection via a CGEventTap.
//!
//! Carbon `RegisterEventHotKey` never fires for Fn-only presses, so the Fn
//! push-to-talk trigger is watched with a passive event tap instead (the same
//! approach Wispr Flow uses). Requires Accessibility permission, which the
//! app already needs for typing.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use crate::dictation::{Controller, Event};
use crate::hotkey_latch::{now_ms, EdgeLatch};

/// Durable sender used by the C callback (the tap outlives any command).
static TX: OnceLock<Sender<Event>> = OnceLock::new();
static RUNNING: Mutex<bool> = Mutex::new(false);
/// The native latch is reset on restart; the Rust latch must be reset too.
static LAST_DOWN: EdgeLatch = EdgeLatch::new();
/// Serializes native stop/start so two callers cannot double-join the tap.
static TAP_LIFECYCLE: Mutex<()> = Mutex::new(());

/// Starts the Fn event tap (no-op if already running).
///
/// Must be called from the main thread — the tap is added to the main run
/// loop, and Tauri commands run on the main thread.
pub fn start(controller: Controller) -> Result<(), String> {
    let _lifecycle = TAP_LIFECYCLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    LAST_DOWN.reset();
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    if let Some(tx) = controller.tx() {
        TX.set(tx).ok(); // keep the first sender; controller is stable
    }
    extern "C" {
        fn teletype_fn_tap_start(cb: extern "C" fn(u8)) -> i32;
    }
    // SAFETY: installs a CGEventTap on the main run loop; the callback is a
    // plain C function pointer with no Rust state.
    let ok = unsafe { teletype_fn_tap_start(on_fn_state) } != 0;
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

/// Stops the Fn event tap (no-op if not running).
pub fn stop() {
    let _lifecycle = TAP_LIFECYCLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let was = *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !was {
        return;
    }
    LAST_DOWN.reset();
    extern "C" {
        fn teletype_fn_tap_stop();
    }
    // SAFETY: removes the tap installed by teletype_fn_tap_start.
    unsafe { teletype_fn_tap_stop() }
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
}

/// Whether the Fn tap is currently installed and enabled.
pub fn is_live() -> bool {
    extern "C" {
        fn teletype_fn_tap_is_live() -> i32;
    }
    // SAFETY: reads native tap state only; takes no arguments and touches no
    // Rust state.
    unsafe { teletype_fn_tap_is_live() != 0 }
}

/// C callback: 1 = Fn pressed, 0 = Fn released.
extern "C" fn on_fn_state(down: u8) {
    let now_down = down != 0;
    // flagsChanged repeats while held — only react to edges. A stale latch
    // from a tap that lost the release edge is recovered after 30 s.
    if !LAST_DOWN.accept(now_down, now_ms()) {
        return;
    }
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
