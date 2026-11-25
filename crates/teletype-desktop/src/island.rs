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

/// A previously dragged position is usable when it still fits on one of
/// the attached monitors' work areas (the island may be dragged off-screen
/// between sessions, e.g. after a display change).
fn restored_position(app: &AppHandle) -> Option<LogicalPosition<f64>> {
    let path = position_file(app)?;
    let raw = fs::read_to_string(path).ok()?;
    let saved: SavedPosition = serde_json::from_str(&raw).ok()?;
    let monitors = app.available_monitors().ok()?;
    let fits = monitors.iter().any(|m| {
        let work = m.work_area();
        let scale = m.scale_factor();
        let wx = work.position.x as f64 / scale;
        let wy = work.position.y as f64 / scale;
        let ww = work.size.width as f64 / scale;
        let wh = work.size.height as f64 / scale;
        saved.x >= wx && saved.y >= wy
            && saved.x + ISLAND_W <= wx + ww
            && saved.y + ISLAND_H <= wy + wh
    });
    if fits {
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
        normalize_size(&win);
        if let Some(pos) = restored_position(app).or_else(|| default_position(app)) {
            let _ = win.set_position(pos);
        }
        let _ = win.show();
    }
}

/// The full pill's logical size (matches the UI's `FULL_W`/`FULL_H`).
const FULL_SIZE: (f64, f64) = (440.0, 92.0);

/// Forces the island window to its intended size at show time.
///
/// Windows quirk: a hidden, frameless, transparent window can be created
/// with a native frame (the client area shrinks to ~426x55 at 125% DPI),
/// and `set_size` to the *configured* size is a no-op — the first real
/// resize is what drops the frame. The UI requests 440x92 on mount (the
/// configured size is deliberately 441x93 so that request is always a
/// real resize); this is the backstop for when that request never landed.
fn normalize_size(win: &tauri::WebviewWindow) {
    let (w, h) = FULL_SIZE;
    let Ok(scale) = win.scale_factor() else {
        return;
    };
    let want = tauri::PhysicalSize::new((w * scale).round() as u32, (h * scale).round() as u32);
    let Ok(actual) = win.inner_size() else {
        return;
    };
    if actual == want {
        return;
    }
    // Nudge with a size one logical point off, then restore: a request
    // equal to the last-set (configured) size is skipped, so the nudge
    // guarantees the frame is dropped and the true size applied.
    let _ = win.set_size(tauri::LogicalSize::new(w - 1.0, h));
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
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

/// Resizes the island window to its current content (logical points).
/// The window starts at the full pill size; when the island minimizes to
/// the compact pill, the webview reports the smaller size so the
/// transparent (click-blocking) area shrinks with the visible pill.
#[tauri::command]
pub fn island_set_size(app: AppHandle, width: f64, height: f64) {
    if !width.is_finite() || !height.is_finite() {
        return;
    }
    let w = width.clamp(120.0, 520.0);
    let h = height.clamp(56.0, 120.0);
    if let Some(win) = app.get_webview_window(ISLAND_LABEL) {
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
    }
}
