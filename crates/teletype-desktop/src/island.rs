//! The meeting island: a small always-on-top overlay that shows the live
//! state of a meeting (recording / finalizing / report ready) so the user
//! can check or stop a meeting from anywhere — no need to navigate to the
//! Meeting screen.
//!
//! The window is declared in `tauri.conf.json` (label [`ISLAND_LABEL`]),
//! starts hidden, and is shown by [`show`] when a meeting starts. The
//! island's own webview polls `meeting_status` for its content and hides
//! itself again after the "report ready" moment, so this module only owns
//! show/hide and placement — the same division of labor as the dictation
//! pill in [`crate::overlay`].
//!
//! All geometry is in logical points, never pixels (see `overlay.rs` for
//! why mixing the two corrupts positions on mixed-scale monitors).
//!
//! The island is user-movable: the webview drags it with
//! `window.startDragging()` and persists the drop position
//! ([`save_position`]); [`show`] restores the last position when it is
//! still on-screen, and falls back to top-center otherwise.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, LogicalPosition};

pub const ISLAND_LABEL: &str = "island";

/// Logical size — must match the window declaration in tauri.conf.json.
const ISLAND_W: f64 = 440.0;
const ISLAND_H: f64 = 92.0;

/// Top inset from the work-area edge, in logical points.
const TOP_INSET: f64 = 14.0;

#[derive(Serialize, Deserialize)]
struct SavedPosition {
    x: f64,
    y: f64,
}

fn position_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("island-position.json"))
}

/// A previously dragged position is usable when it still fits the
/// primary monitor's work area (the island may be dragged off-screen
/// between sessions, e.g. after a display change).
fn restored_position(app: &AppHandle) -> Option<LogicalPosition<f64>> {
    let path = position_file(app)?;
    let raw = fs::read_to_string(path).ok()?;
    let saved: SavedPosition = serde_json::from_str(&raw).ok()?;
    let monitor = app.primary_monitor().ok()??;
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let wx = work.position.x as f64 / scale;
    let wy = work.position.y as f64 / scale;
    let ww = work.size.width as f64 / scale;
    let wh = work.size.height as f64 / scale;
    let ok = saved.x >= wx && saved.y >= wy
        && saved.x + ISLAND_W <= wx + ww
        && saved.y + ISLAND_H <= wy + wh;
    if ok {
        Some(LogicalPosition::new(saved.x, saved.y))
    } else {
        None
    }
}

/// Top-center of the primary monitor's work area (logical points) — the
/// default when no saved position exists.
fn default_position(app: &AppHandle) -> Option<LogicalPosition<f64>> {
    let monitor = app.primary_monitor().ok()??;
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let x = work.position.x as f64 / scale + (work.size.width as f64 / scale - ISLAND_W) / 2.0;
    let y = work.position.y as f64 / scale + TOP_INSET;
    Some(LogicalPosition::new(x, y))
}

/// Shows the island (idempotent). Called when a meeting starts. The window
/// is `focusable: false`, so showing it never steals keyboard focus.
pub fn show(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(ISLAND_LABEL) {
        if let Some(pos) = restored_position(app).or_else(|| default_position(app)) {
            let _ = win.set_position(pos);
        }
        let _ = win.show();
    }
}

/// Hides the island. Called on cancel and on finalization failure. The
/// successful-done case is handled by the island's own webview, which
/// shows a short "report ready" moment and then hides itself.
pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(ISLAND_LABEL) {
        let _ = win.hide();
    }
}

/// Persists the island's drop position (logical points) after a drag.
/// Invalid values are ignored — the next show falls back to top-center.
#[tauri::command]
pub fn island_save_position(app: AppHandle, x: f64, y: f64) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    let Some(path) = position_file(&app) else {
        return;
    };
    let saved = SavedPosition { x, y };
    if let Ok(json) = serde_json::to_string_pretty(&saved) {
        let _ = fs::write(path, json);
    }
}
