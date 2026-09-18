//! The floating dictation overlay (pill).
//!
//! A small, always-on-top, transparent window that shows the dictation state
//! without stealing focus. Positioned near the bottom-center of the screen.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

pub const PILL_LABEL: &str = "pill";

/// Shows or hides the pill window based on the dictation state.
#[allow(dead_code)]
pub fn update(app: &AppHandle, visible: bool, text: &str) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        return;
    };
    let _ = app.emit_to(
        PILL_LABEL,
        "pill-state",
        serde_json::json!({
            "visible": visible,
            "text": text,
        }),
    );
    if visible {
        if !window.is_visible().unwrap_or(false) {
            position(app, &window);
            let _ = window.show();
        }
    } else if window.is_visible().unwrap_or(true) {
        let _ = window.hide();
    }
}

#[allow(dead_code)]
fn position(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitor = app.primary_monitor().ok().flatten();
    let Some(monitor) = monitor else {
        return;
    };
    let size = window.outer_size().unwrap_or_default();
    let screen = monitor.size();
    let scale = monitor.scale_factor();
    let x = (screen.width as f64 / scale - size.width as f64 / scale) / 2.0;
    let y = screen.height as f64 / scale - size.height as f64 / scale - 80.0;
    let _ = window.set_position(PhysicalPosition::new(
        (x * scale) as i32,
        (y * scale) as i32,
    ));
}

/// Called at startup to pre-create the pill window (hidden).
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        return Ok(());
    };
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_decorations(false);
    let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.hide();
    Ok(())
}
