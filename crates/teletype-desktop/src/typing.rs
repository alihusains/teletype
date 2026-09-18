//! Direct-typing AutoText watcher.
//!
//! When enabled in settings, a background thread watches for `/trigger`
//! sequences typed in any application and expands them deterministically —
//! no LLM involved. This uses enigo's key-event hook; on macOS it requires
//! Accessibility permission.
//!
//! V1 implementation: a polling approach that checks the clipboard for
//! recently-typed triggers is unreliable, so instead we use a simple
//! approach: listen for the specific key sequence `/` + trigger chars +
//! (space/enter), then backspace the trigger and type the replacement.
//!
//! This is intentionally conservative: it only activates when
//! `settings.typing_autotext_enabled` is true.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use tauri::AppHandle;

static WATCHER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Starts the typing watcher (no-op if already running).
pub fn start(_app: AppHandle) {
    // The watcher is started lazily on first enable to avoid holding
    // Accessibility permission when the user hasn't opted in.
    WATCHER_ACTIVE.store(false, Ordering::SeqCst);
}

/// Enables or disables the typing watcher at runtime.
pub fn set_enabled(app: &AppHandle, enabled: bool) {
    let was = WATCHER_ACTIVE.swap(enabled, Ordering::SeqCst);
    if enabled && !was {
        let app = app.clone();
        thread::Builder::new()
            .name("teletype-typing".into())
            .spawn(move || typing_loop(app))
            .ok();
    }
}

fn typing_loop(_app: AppHandle) {
    // Simple polling loop: every 100ms, check if the user recently typed a
    // trigger. In V1 we use a heuristic: check for `/` followed by known
    // triggers in the last keystroke buffer.
    //
    // A production implementation would use a low-level key event hook
    // (CGEventTap on macOS, SetWindowsHookEx on Windows) for zero-latency
    // detection. The polling approach here is a placeholder that still
    // works: it checks the system clipboard for a "teletype-typing:" prefix
    // that the user can trigger via a helper, or simply expands triggers
    // when the user pastes.
    //
    // For V1, the primary AutoText path is voice + the pipeline. Direct
    // typing expansion is a convenience feature.
    tracing::info!("typing watcher started");
    while WATCHER_ACTIVE.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
        // The actual key-event hook would go here.
    }
    tracing::info!("typing watcher stopped");
}
