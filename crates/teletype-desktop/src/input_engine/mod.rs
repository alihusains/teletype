//! Unified InputEngine (macOS).
//!
//! One CGEventTap feeds both engines:
//!
//! ```text
//! CGEventTap -> InputNormalizer -> ShortcutManager OR AutoTextManager
//!                                -> TextContext -> Text replacement/action.
//! ```
//!
//! * `ShortcutManager`: configurable transform/Quick-Add shortcuts. Matching
//!   KeyDown/KeyUp events are consumed; the app never sees them.
//! * `AutoTextManager`: typed-trigger expansion. Ordinary input is observed
//!   only, never consumed; triggers are replaced afterwards.
//!
//! This is the ONLY native event listener. The legacy `transform_tap` is
//! retired; the legacy `typing` watcher routes its enable flag here and never
//! starts its own tap on macOS.

pub mod autotext_manager;
pub mod normalizer;
pub mod shortcut_manager;
pub mod text_context;

use tauri::AppHandle;

use normalizer::ObservedKind;

/// Ensure the unified tap is running with `action` + `observe` callbacks.
/// Idempotent: safe to call on every rebuild. Returns true when live.
fn ensure_tap() -> bool {
    extern "C" {
        fn teletype_input_engine_start(
            action_cb: extern "C" fn(u32),
            observe_cb: extern "C" fn(u32, i64, u64, *const u16, u32),
        ) -> i32;
    }
    // SAFETY: plain C function pointers into the callbacks below; state lives
    // in statics, and the tap is added to the main run loop.
    (unsafe { teletype_input_engine_start(on_action_slot, on_observed_event) }) != 0
}

/// Rebuild shortcut bindings from the current stores and push the mirror to
/// the native tap. Returns true when the tap owns the bindings.
pub fn rebuild(app: &AppHandle, state: &crate::AppState) -> bool {
    note_app(app);
    let bindings = shortcut_manager::plan(state);
    shortcut_manager::remember(app, bindings.clone());

    if !ensure_tap() {
        crate::log_entry(
            crate::LogLevel::Warn,
            "input engine tap could not be created (grant Teletype Accessibility access)"
                .to_string(),
        );
        return false;
    }

    extern "C" {
        fn teletype_input_engine_set_bindings(flags: *const u64, keycodes: *const i64, count: u32);
    }
    let flags: Vec<u64> = bindings.iter().map(|b| b.flags).collect();
    let codes: Vec<i64> = bindings.iter().map(|b| b.keycode).collect();
    // SAFETY: borrowed for the duration of the call, which copies them.
    unsafe {
        teletype_input_engine_set_bindings(flags.as_ptr(), codes.as_ptr(), flags.len() as u32)
    };
    for b in &bindings {
        crate::log_entry(
            crate::LogLevel::Info,
            format!("input engine shortcut registered: {}", b.label),
        );
    }
    true
}

/// Park (`true`) or resume (`false`) the whole engine while the native
/// hotkey-capture panel is open, so the panel sees raw keys.
pub fn set_capture_parked(parked: bool) {
    extern "C" {
        fn teletype_input_engine_set_parked(parked: i32);
    }
    // SAFETY: plain integer flag.
    unsafe { teletype_input_engine_set_parked(i32::from(parked)) };
}

/// Enable/disable shortcut firing (keeps observing).
pub fn set_shortcuts_on(on: bool) {
    extern "C" {
        fn teletype_input_engine_set_shortcuts_on(on: i32);
    }
    // SAFETY: plain integer flag.
    unsafe { teletype_input_engine_set_shortcuts_on(i32::from(on)) };
}

/// Enable/disable AutoText observation (keeps shortcuts).
pub fn set_observe_on(on: bool) {
    extern "C" {
        fn teletype_input_engine_set_observe_on(on: i32);
    }
    // SAFETY: plain integer flag.
    unsafe { teletype_input_engine_set_observe_on(i32::from(on)) };
}

static ENGINE_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// Remember the app handle for observe-path state access. Called on rebuild.
pub fn note_app(app: &AppHandle) {
    let _ = ENGINE_APP.set(app.clone());
}

/// Native shortcut callback (main runloop, KeyDown only): dispatch async.
extern "C" fn on_action_slot(slot: u32) {
    shortcut_manager::dispatch(slot);
}

/// Native observe callback (main runloop, non-shortcut KeyDown only).
/// Forwards resulting text to the AutoText buffer; never blocks the tap.
extern "C" fn on_observed_event(
    kind_raw: u32,
    _keycode: i64,
    _flags: u64,
    chars_ptr: *const u16,
    char_len: u32,
) {
    let Some(app) = ENGINE_APP.get().cloned() else {
        return;
    };
    let kind = ObservedKind::from_raw(kind_raw);
    let text = if matches!(kind, ObservedKind::Text | ObservedKind::Delimiter)
        && !chars_ptr.is_null()
        && char_len > 0
    {
        // SAFETY: borrowed for the call duration; the tap owns the buffer.
        let units = unsafe { std::slice::from_raw_parts(chars_ptr, char_len as usize) };
        normalizer::decode_utf16(units)
    } else {
        String::new()
    };
    autotext_manager::observe(&app, kind, &text);
}
