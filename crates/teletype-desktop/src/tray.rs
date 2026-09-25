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

pub fn build(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Teletype", true, None::<&str>)?;
    let dictate = MenuItem::with_id(app, "dictate", "Start Dictation", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Teletype", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&dictate, &separator, &open, &settings, &separator, &quit],
    )?;

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
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|_tray, _event| {
            // Left-click: show main window.
        })
        .build(app)?
        .set_visible(visible)?;
    Ok(())
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
