//! Settings commands: get/save/patch, observers, icon/theme/motion/pill-size, hotkey capture, devices and permissions.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use tauri::{AppHandle, Emitter, Manager, State};
use teletype_core::platform::{Permission, PermissionKind};
use crate::AppState;
use super::*;

// ---- Native Hotkey Capture (macOS) ----

/// Opens the native macOS hotkey capture panel. The user presses a key
/// combination, Enter confirms (writes the result to a temp file), Esc cancels.
/// Returns immediately; the UI should call `get_captured_hotkey` after a delay.
#[tauri::command]
pub fn start_hotkey_capture() -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_start_hotkey_capture();
        }
        // SAFETY: the C function creates an NSPanel on the main thread.
        // Tauri commands run on the main thread by default.
        std::fs::remove_file("/tmp/teletype_hotkey_result.txt").ok();
        // Park the unified input engine while capturing: without this the
        // shortcut tap eats the second key of the combo (it matches a
        // registered shortcut) and the panel never sees it.
        crate::input_engine::set_capture_parked(true);
        // SAFETY: C function only creates an NSPanel on the main thread,
        // where Tauri commands run.
        unsafe { teletype_start_hotkey_capture() };
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(CommandError::user(
            "Hotkey capture is only available on macOS",
        ))
    }
}

/// Closes the native hotkey capture panel.
#[tauri::command]
pub fn stop_hotkey_capture() -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_stop_hotkey_capture();
        }
        // SAFETY: C function only touches the NSPanel created above.
        unsafe { teletype_stop_hotkey_capture() };
        // Resume the input engine. Every capture exit funnels through here
        // (confirm, cancel, timeout, unmount), so re-enabling here cannot leak
        // a parked engine.
        crate::input_engine::set_capture_parked(false);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Returns the hotkey captured by the native panel, if the user confirmed.
/// Returns `Ok(None)` if the user cancelled or hasn't confirmed yet.
#[tauri::command]
pub fn get_captured_hotkey() -> CommandResult<Option<String>> {
    let path = std::path::Path::new("/tmp/teletype_hotkey_result.txt");
    match std::fs::read_to_string(path) {
        Ok(h) if !h.trim().is_empty() => {
            let hotkey = h.trim().to_string();
            std::fs::remove_file(path).ok();
            Ok(Some(hotkey))
        }
        _ => Ok(None),
    }
}

/// The current hotkey registration conflict, if the saved binding is the one
/// that failed. A registration failure used to surface only as a save-time
/// error or a log line at startup, so a shortcut taken by something else left
/// dictation silently dead with the change looking saved. `None` when the
/// binding works or when the stored failure belongs to an older binding.
#[tauri::command]
pub async fn hotkey_conflict(
    state: State<'_, AppState>,
) -> CommandResult<Option<crate::dictation::HotkeyConflict>> {
    let stored = state.controller.conflict_snapshot();
    Ok(crate::dictation::visible_conflict(
        &stored,
        &state.settings().hotkey,
    ))
}

/// Plays one of the [`RECORDING_SOUND_NAMES`] cues so the user can hear it
/// before choosing.
///
/// Deliberately bypasses the master toggle: previewing a sound you have
/// switched off is exactly when a user most needs to hear it.
#[tauri::command]
pub fn preview_recording_sound(name: String) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        if !RECORDING_SOUND_NAMES.contains(&name.as_str()) {
            return Err(CommandError::user(format!(
                "Unknown recording sound: {name}"
            )));
        }
        extern "C" {
            fn teletype_play_system_sound(name: *const std::os::raw::c_char) -> i32;
        }
        let c = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: `c` outlives the call and NUL-terminated; the callee copies
        // what it needs to build an NSSound.
        let rc = unsafe { teletype_play_system_sound(c.as_ptr()) };
        if rc != 0 {
            return Err(CommandError::user(
                "That sound is not available on this Mac",
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        Err(CommandError::user(
            "Recording sounds are only available on macOS",
        ))
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state.settings())
}

/// Settings observers: one per subsystem, each watching exactly the fields
/// it owns. Each is a no-op when its watched field is unchanged, replacing
/// the hand-maintained `if new.X != old.X { ... }` diffing that used to live
/// inline in `save_settings` (B8).
pub mod settings_observers {
    use super::*;

    /// Re-registers the global dictation hotkey when it changed.
    pub fn hotkey(app: &AppHandle, state: &AppState, old: &Settings, new: &Settings) {
        if new.hotkey == old.hotkey {
            return;
        }
        if !new.hotkey.is_empty() {
            // Unregister the old hotkey first so the new one can take its
            // place.
            if !old.hotkey.is_empty() {
                state.controller.unregister_hotkey(app, &old.hotkey);
            }
            if let Err(e) = state.controller.register_hotkey(app, &new.hotkey) {
                // Revert.
                if !old.hotkey.is_empty() {
                    if let Err(revert_e) = state.controller.register_hotkey(app, &old.hotkey) {
                        tracing::error!(
                            "Hotkey registration failed ({e}) AND revert to previous hotkey also failed: {revert_e}"
                        );
                    }
                }
                crate::log_entry(crate::LogLevel::Error, format!("hotkey change failed: {e}"));
            }
        } else if !old.hotkey.is_empty() {
            state.controller.unregister_hotkey(app, &old.hotkey);
        }
    }

    /// Re-registers the Quick Add global shortcut when it changed (or was
    /// cleared).
    pub fn quick_add_hotkey(app: &AppHandle, state: &AppState, old: &Settings, new: &Settings) {
        if new.quick_add_hotkey == old.quick_add_hotkey {
            return;
        }
        sync_quick_add_shortcut(app, state);
    }

    /// Moves the floating pill immediately when its position changed (it may
    /// be visible mid-dictation or via alwaysShowPill).
    pub fn pill_position(app: &AppHandle, _state: &AppState, _old: &Settings, new: &Settings) {
        crate::overlay::place(app, new.pill_position.resolve());
    }

    /// Applies the selected app icon to the window and tray when it changed.
    pub fn app_icon(app: &AppHandle, _state: &AppState, old: &Settings, new: &Settings) {
        if new.app_icon == old.app_icon {
            return;
        }
        apply_app_icon(app, new.app_icon.as_id());
    }

    /// Applies tray visibility at runtime when the toggle changed (BUG-009).
    pub fn tray(app: &AppHandle, _state: &AppState, old: &Settings, new: &Settings) {
        if new.show_tray_icon == old.show_tray_icon {
            return;
        }
        if let Err(e) = crate::tray::apply_visibility(app, new.show_tray_icon) {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("tray visibility change failed: {e}"),
            );
        }
    }

    /// Applies the typing AutoText toggle at runtime (BUG-010).
    pub fn typing(app: &AppHandle, _state: &AppState, old: &Settings, new: &Settings) {
        if new.typing_autotext_enabled == old.typing_autotext_enabled {
            return;
        }
        crate::typing::set_enabled(app, new.typing_autotext_enabled);
    }

    /// Runs every observer whose watched field changed between `old` and
    /// `new`. Order matches the old hand-diffed sequence, so side-effect
    /// behaviour is unchanged.
    pub fn run_all(app: &AppHandle, state: &AppState, old: &Settings, new: &Settings) {
        if new.hotkey != old.hotkey {
            hotkey(app, state, old, new);
        }
        if new.quick_add_hotkey != old.quick_add_hotkey {
            quick_add_hotkey(app, state, old, new);
        }
        if new.pill_position != old.pill_position {
            pill_position(app, state, old, new);
        }
        if new.app_icon != old.app_icon {
            app_icon(app, state, old, new);
        }
        if new.show_tray_icon != old.show_tray_icon {
            tray(app, state, old, new);
        }
        if new.typing_autotext_enabled != old.typing_autotext_enabled {
            typing(app, state, old, new);
        }
    }
}

/// Applies `(old, new)` to the live app: every subsystem observer fires only
/// when its watched field changed, then the webviews are told to refresh.
/// Runs under the caller's `save_lock` guard.
fn apply_settings_side_effects(app: &AppHandle, state: &AppState, old: &Settings, new: &Settings) {
    settings_observers::run_all(app, state, old, new);
    // Let all webviews (main window + pill) refresh from the new settings.
    // The main window uses this to show/hide the Developer tab and re-fetch
    // settings; the pill uses it to update its visual style.
    let _ = app.emit("settings-changed", &());
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<Settings> {
    let (new, old) = state.apply_settings(&settings)?;
    apply_settings_side_effects(&app, &state, &old, &new);
    Ok(new)
}

/// Saves a partial settings update: only the `Some` fields of `patch` are
/// applied, every other field keeps its current value, so a stale snapshot
/// can no longer clobber a newer field (B8). The recommended path for
/// single-field savers; `save_settings` (whole document) stays for
/// backward compatibility.
#[tauri::command]
pub async fn update_settings_patch(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> CommandResult<Settings> {
    let (new, old) = state.update_settings(&patch)?;
    apply_settings_side_effects(&app, &state, &old, &new);
    Ok(new)
}

/// Resolves a settings icon id to its bundled PNG bytes.
fn app_icon_bytes(id: &str) -> Option<&'static [u8]> {
    APP_ICONS
        .iter()
        .find(|(k, _, _)| *k == id)
        .map(|(_, _, b)| *b)
}

/// Sets the selected app icon, persists it, and applies it to the window
/// and tray immediately.
#[tauri::command]
pub async fn set_app_icon(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let mut settings = state.settings();
    let icon: AppIcon = id.parse()?;
    settings.app_icon = icon;
    state.replace_settings(settings)?;
    apply_app_icon(&app, icon.as_id());
    Ok(())
}

/// Sets the UI color theme ("system" | "light" | "dark") and persists it.
/// The UI applies it to `document.documentElement`; no Rust-side effect.
#[tauri::command]
pub async fn set_theme(state: State<'_, AppState>, theme: String) -> CommandResult<()> {
    let theme: UiTheme = theme.parse()?;
    let mut settings = state.settings();
    settings.theme = theme;
    state.replace_settings(settings)
}

/// Toggles Reduce Motion and persists it. The UI applies it to
/// `document.documentElement.dataset.motion`; no Rust-side effect.
#[tauri::command]
pub async fn set_reduce_motion(state: State<'_, AppState>, enabled: bool) -> CommandResult<()> {
    let mut settings = state.settings();
    settings.reduce_motion = enabled;
    state.replace_settings(settings)
}

/// The pill webview reports its measured natural size (logical px) so the OS
/// window can be resized to fit the active style. The "well" style is 120px
/// tall and the capsules are 44px, so a single fixed window would clip or
/// letterbox them; resizing per style matches the reference implementation.
#[tauri::command]
pub fn set_pill_size(
    app: AppHandle,
    state: State<'_, AppState>,
    width: u32,
    height: u32,
) -> CommandResult<()> {
    let settings = state.settings();
    let position = settings.pill_position.resolve();
    crate::overlay::resize_to(&app, width, height, position);
    Ok(())
}

/// Applies the selected icon to the main window and the tray.
///
/// Both `Window::set_icon` and `TrayIcon::set_icon` must run on the main
/// thread. Tauri's sync wrappers do that by blocking the *calling* thread on
/// `rx.recv()` until the main thread runs the task; called from an async
/// command's tokio worker that wedges the main-thread pipeline and freezes the
/// window. So we schedule fire-and-forget work on the main thread instead of
/// blocking on it. The window and tray handles are `Send + 'static`, so they
/// can be moved into the main-thread closure.
pub fn apply_app_icon(app: &AppHandle, id: &str) {
    let Some(bytes) = app_icon_bytes(id) else {
        return;
    };
    let Ok(icon) = tauri::image::Image::from_bytes(bytes) else {
        return;
    };
    let window = app.get_webview_window("main");
    let tray = app.tray_by_id(crate::tray::TRAY_ID);
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = window {
            let _ = win.set_icon(icon.clone());
        }
        if let Some(tray) = tray {
            let _ = tray.set_icon(Some(icon));
        }
    });
}

// ---- Devices & Permissions ----

#[tauri::command]
pub fn list_input_devices() -> CommandResult<Vec<teletype_core::audio::InputDevice>> {
    Ok(teletype_core::audio::list_input_devices())
}

#[tauri::command]
pub async fn get_permissions(state: State<'_, AppState>) -> CommandResult<Vec<Permission>> {
    Ok(state.platform.permissions())
}

#[tauri::command]
pub async fn request_permission(
    state: State<'_, AppState>,
    kind: PermissionKind,
) -> CommandResult<()> {
    state.platform.request_permission(kind);
    Ok(())
}

#[tauri::command]
pub async fn open_permission_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: PermissionKind,
) -> CommandResult<()> {
    if let Some(url) = state.platform.permission_settings_url(kind) {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_state_decides_whether_asking_again_can_work() {
        use teletype_core::platform::{Permission, PermissionKind, PermissionState};
        let p = |state| Permission {
            kind: PermissionKind::Microphone,
            state,
        };
        assert!(p(PermissionState::Granted).can_prompt());
        assert!(p(PermissionState::NotDetermined).can_prompt());
        // The point of the tri-state: a refused permission must not keep
        // offering "Grant Access", because macOS will never prompt again.
        assert!(!p(PermissionState::Denied).can_prompt());
        assert!(!p(PermissionState::Unsupported).can_prompt());
        assert!(p(PermissionState::Granted).granted());
        assert!(!p(PermissionState::Denied).granted());
    }


    #[test]
    fn patch_updates_only_named_fields() {
        let mut s = Settings::default();
        s.language = "fr".into();
        s.input_device = "mic-1".into();

        let mut t = s.clone();
        let patch = SettingsPatch {
            language: Some("de".into()),
            ..Default::default()
        };
        assert!(patch.apply_to(&mut t), "a changed field must count");
        assert_eq!(t.language, "de");
        // The untouched field keeps its value.
        assert_eq!(t.input_device, "mic-1");
        assert_eq!(t.hotkey, s.hotkey);
        assert_eq!(t.app_icon, s.app_icon);
    }


    #[test]
    fn patch_deserializes_partial_json_with_none_for_absent_fields() {
        let patch: SettingsPatch =
            serde_json::from_value(serde_json::json!({ "inputDevice": "mic-9" }))
                .expect("a partial body must parse");
        assert_eq!(patch.input_device.as_deref(), Some("mic-9"));
        assert!(patch.language.is_none());
        assert!(patch.app_icon.is_none());
        // An empty body is a valid no-op patch.
        let empty: SettingsPatch = serde_json::from_value(serde_json::json!({})).unwrap();
        let mut s = Settings::default();
        let before = s.clone();
        assert!(!empty.apply_to(&mut s));
        assert_eq!(s, before);
    }


    #[test]
    fn patch_does_not_clobber_concurrent_field_change() {
        // The lost-update regression: tab A reads a snapshot, tab B changes
        // `app_icon`, then tab A saves a patch that only touches
        // `inputDevice`. Tab A's stale snapshot must not revert tab B's icon.
        let state = crate::AppState::for_tests();
        let (a, _) = state
            .update_settings(&SettingsPatch {
                language: Some("fr".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(a.language, "fr");

        // Tab A reads a snapshot, then tab B changes app_icon.
        let snapshot_a = state.settings();
        let (b, _) = state
            .update_settings(&SettingsPatch {
                app_icon: Some(AppIcon::Blue),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(b.app_icon, AppIcon::Blue);

        // Tab A now applies its single-field patch (built from the stale
        // snapshot, whose app_icon is still "white").
        let (c, old_c) = state
            .update_settings(&SettingsPatch {
                input_device: Some("mic-2".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(c.input_device, "mic-2");
        assert_eq!(c.app_icon, AppIcon::Blue, "tab B's app_icon must survive");
        assert_eq!(c.language, "fr");
        assert_eq!(old_c.app_icon, AppIcon::Blue);
        assert_eq!(
            snapshot_a.app_icon,
            AppIcon::White,
            "sanity: A's snapshot was stale"
        );

        // And it persisted, not just in-memory.
        assert_eq!(state.settings().app_icon, AppIcon::Blue);
    }


    #[test]
    fn whole_document_save_does_not_clobber_concurrent_field_change() {
        // The legacy whole-document path goes through apply_settings, which
        // reads the CURRENT snapshot for `old` under the same lock, so its
        // observers see the real diff (the icon change is not lost either).
        let state = crate::AppState::for_tests();
        let (a, _) = state
            .apply_settings(&Settings {
                language: "fr".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(a.language, "fr");

        let snapshot_a = state.settings();
        let (b, _) = state
            .apply_settings(&Settings {
                app_icon: AppIcon::Blue,
                ..snapshot_a.clone()
            })
            .unwrap();
        assert_eq!(b.app_icon, AppIcon::Blue);

        // Tab A saves its stale snapshot with only inputDevice changed.
        let stale = snapshot_a.clone();
        let (c, old_c) = state
            .apply_settings(&Settings {
                input_device: "mic-2".into(),
                ..stale
            })
            .unwrap();
        assert_eq!(c.input_device, "mic-2");
        // Whole-document semantics: the document becomes exactly what tab A
        // sent, but `old_c` is the current snapshot, so a caller diffing
        // (old_c, c) applies the right side effects.
        assert_eq!(old_c.app_icon, AppIcon::Blue);
    }


    #[test]
    fn observers_only_fire_for_their_own_field() {
        // run_all's guards: each observer is skipped unless its watched
        // field changed, so a patch to one field cannot trigger another
        // subsystem's side effect.
        let base = Settings::default();
        let changed_icon = Settings {
            app_icon: AppIcon::Blue,
            ..base.clone()
        };
        assert!(changed_icon.app_icon != base.app_icon);
        assert!(changed_icon.hotkey == base.hotkey);
        assert!(changed_icon.pill_position == base.pill_position);
        assert!(changed_icon.show_tray_icon == base.show_tray_icon);
        assert!(changed_icon.typing_autotext_enabled == base.typing_autotext_enabled);
        assert!(changed_icon.quick_add_hotkey == base.quick_add_hotkey);

        // The guards run_all uses, verbatim:
        assert!(!(&changed_icon.hotkey != &base.hotkey));
        assert!(!(&changed_icon.quick_add_hotkey != &base.quick_add_hotkey));
        assert!(!(&changed_icon.pill_position != &base.pill_position));
        assert!(&changed_icon.app_icon != &base.app_icon);
        assert!(!(&changed_icon.show_tray_icon != &base.show_tray_icon));
        assert!(!(&changed_icon.typing_autotext_enabled != &base.typing_autotext_enabled));

        // And the individual observers are no-ops on equality:
        let changed_hotkey = Settings {
            hotkey: "Cmd+Shift+Z".into(),
            ..base.clone()
        };
        assert!(changed_hotkey.hotkey != base.hotkey);
        assert!(changed_hotkey.app_icon == base.app_icon);
        assert!(!(&changed_hotkey.app_icon != &base.app_icon));
        assert!(&changed_hotkey.hotkey != &base.hotkey);
    }
}
