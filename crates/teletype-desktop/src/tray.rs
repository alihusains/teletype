//! Menu bar (macOS) / system tray (Windows) icon and menu.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

use teletype_core::state::UiState;

use crate::AppState;

pub const TRAY_ID: &str = "teletype-tray";

/// Simple 16×16 template icon (a dot). In production this would be a proper
/// template PNG; the inline bytes keep the build self-contained.
const ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");

/// The static menu plus, when present, the undo offer on top.
///
/// Rebuilt (not mutated) on every offer and expiry: tauri menu handles are
/// cheap, and a single builder keeps the two states from drifting apart.
fn build_menu(app: &AppHandle, undo_label: Option<&str>) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open Teletype", true, None::<&str>)?;
    let dictate = MenuItem::with_id(app, "dictate", "Start Dictation", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Teletype", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    if let Some(label) = undo_label {
        let undo = MenuItem::with_id(app, "undo_learn", label, true, None::<&str>)?;
        let separator2 = PredefinedMenuItem::separator(app)?;
        Menu::with_items(
            app,
            &[
                &undo,
                &separator2,
                &dictate,
                &separator,
                &open,
                &settings,
                &separator,
                &quit,
            ],
        )
    } else {
        Menu::with_items(
            app,
            &[&dictate, &separator, &open, &settings, &separator, &quit],
        )
    }
}

/// Shows the auto-learn undo offer in the tray menu for 30 s.
///
/// Called when personalization learns from an edit. A newer learn replaces
/// the offer (and its timer); undoing, expiry, or a newer learn clears it.
/// Menu lifetime, not pill lifetime: the pill is click-through outside
/// recordings, while the tray menu is always one click away.
pub fn offer_undo(app: &AppHandle, message: &str) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    // Keep the label short: the menu is narrow and the full description is
    // one click away in Personalization settings.
    let short: String = message.chars().take(48).collect();
    let Ok(menu) = build_menu(app, Some(&format!("Undo learned: {short}"))) else {
        return;
    };
    if tray.set_menu(Some(menu)).is_err() {
        return;
    }
    let app_revert = app.clone();
    std::thread::Builder::new()
        .name("teletype-undo-expiry".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            clear_undo_offer(&app_revert);
        })
        .ok();
}

/// Removes the undo offer, if present. Idempotent: expiry, undo, and newer
/// offers all funnel through here.
pub fn clear_undo_offer(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, None) {
        let _ = tray.set_menu(Some(menu));
    }
    let state = app.state::<AppState>();
    *state
        .last_learned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Undoes the currently offered learn, if it is still the live one.
/// Returns how many preferences were removed.
pub fn undo_offered(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let ids: Vec<String> = state
        .last_learned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_default();
    if ids.is_empty() {
        return 0;
    }
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut removed = 0;
    for id in &ids {
        if profile.remove(id) {
            removed += 1;
        }
    }
    if removed > 0 {
        let _ = state.profile_store.save(&*profile);
    }
    drop(profile);
    clear_undo_offer(app);
    removed
}

pub fn build(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let menu = build_menu(app, None)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Teletype")
        .icon(tauri::image::Image::from_bytes(ICON_IDLE)?)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "settings" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.emit("navigate", "settings");
                }
            }
            "dictate" => {
                app.state::<AppState>()
                    .controller
                    .send(crate::dictation::Event::Toggle);
            }
            "undo_learn" => {
                let removed = undo_offered(app);
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("undo: removed {removed} just-learned preference(s)"),
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|_tray, _event| {
            // Left-click: show main window.
        })
        .build(app)?;
    apply_visibility(app, visible)?;
    Ok(())
}

/// Shows or hides the already-built tray item at runtime (BUG-009).
///
/// `build` always creates the tray item (the menu and "Quit" entry are the
/// only app-exit path, so the item must exist) and starts it in the
/// `visible` state. Toggling `show_tray_icon` later only flips visibility,
/// so the tray is never destroyed and rebuilt.
pub fn apply_visibility(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    tray.set_visible(visible)
}

/// Mirrors a transform skip/fallback message in the tray tooltip so the
/// status is visible without the pill. Called at dictation time (P1-16 T7b).
pub fn show_skip(app: &AppHandle, message: &str) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = format!("Teletype — {message}");
    let _ = tray.set_tooltip(Some(&tooltip));
}

/// Updates the tray tooltip to reflect dictation state.
pub fn show_state(app: &AppHandle, state: &UiState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = match state {
        UiState::Idle => "Teletype".to_string(),
        UiState::Listening { .. } => "Teletype — Listening".to_string(),
        UiState::Transcribing => "Teletype — Transcribing…".to_string(),
        UiState::Transforming => "Teletype — Transforming…".to_string(),
        UiState::Inserting => "Teletype — Inserting…".to_string(),
        UiState::Message { text } => format!("Teletype — {text}"),
    };
    let _ = tray.set_tooltip(Some(&tooltip));
}
