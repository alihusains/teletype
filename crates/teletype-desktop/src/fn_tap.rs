//! Bare-Fn key detection via a CGEventTap.
//!
//! Carbon `RegisterEventHotKey` never fires for Fn-only presses, so the Fn
//! push-to-talk trigger is watched with a passive event tap instead (the same
//! approach Wispr Flow uses). Requires Accessibility permission, which the
//! app already needs for typing.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use crate::dictation::{Controller, Event};

/// Durable sender used by the C callback (the tap outlives any command).
static TX: OnceLock<Sender<Event>> = OnceLock::new();
static RUNNING: Mutex<bool> = Mutex::new(false);

/// Starts the Fn event tap (no-op if already running).
///
/// Must be called from the main thread — the tap is added to the main run
/// loop, and Tauri commands run on the main thread.
pub fn start(controller: Controller) -> Result<(), String> {
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    TX.set(controller.tx()).ok(); // keep the first sender; controller is stable
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
    let was = *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !was {
        return;
    }
    extern "C" {
        fn teletype_fn_tap_stop();
    }
    // SAFETY: removes the tap installed by teletype_fn_tap_start.
    unsafe { teletype_fn_tap_stop() }
    *RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
}

/// C callback: 1 = Fn pressed, 0 = Fn released.
extern "C" fn on_fn_state(down: u8) {
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
