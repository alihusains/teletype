//! The floating recorder pill: a small transparent window that never takes focus.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use teletype_core::state::{PillPosition, PillState};

pub const PILL_LABEL: &str = "pill";

/// Gap between the pill window and the screen edge, in logical pixels.
const INSET: f64 = 8.0;

/// Shows the pill in `state`, or hides it when idle unless `always_show` is on.
pub fn update(app: &AppHandle, state: PillState, position: PillPosition, always_show: bool) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        tracing::warn!("pill window missing; not showing {state:?}");
        return;
    };
    let vis_before = window.is_visible().unwrap_or(false);
    tracing::info!(?state, always_show, vis_before, "pill update");
    // Emit before any show/hide so the webview always has the latest state
    // when it becomes visible.
    let _ = app.emit_to(PILL_LABEL, "pill-state", &state);
    if matches!(state, PillState::Idle) && !always_show {
        let _ = window.hide();
        return;
    }
    // Only reposition when appearing, so the pill doesn't jump mid-dictation.
    if !vis_before {
        place(app, position);
        let _ = window.show();
    }
}

/// Lets the pill receive clicks and hover (while recording) or passes them
/// through to whatever is underneath (otherwise).
pub fn set_interactive(app: &AppHandle, interactive: bool) {
    if let Some(window) = app.get_webview_window(PILL_LABEL) {
        let _ = window.set_ignore_cursor_events(!interactive);
    }
}

/// Moves the pill to `position` on the monitor under the mouse cursor.
pub fn place(app: &AppHandle, position: PillPosition) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        return;
    };
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) else {
        return;
    };
    let area = monitor.work_area();
    let inset = (INSET * monitor.scale_factor()).round() as i32;
    let (x, y) = origin(
        position,
        (
            area.position.x,
            area.position.y,
            area.size.width as i32,
            area.size.height as i32,
        ),
        (size.width as i32, size.height as i32),
        inset,
    );
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Top-left corner of the pill window within a work area, all in physical pixels
/// with y growing downwards.
fn origin(
    position: PillPosition,
    (area_x, area_y, area_w, area_h): (i32, i32, i32, i32),
    (w, h): (i32, i32),
    inset: i32,
) -> (i32, i32) {
    use PillPosition::*;
    let left = area_x + inset;
    let center = area_x + (area_w - w) / 2;
    let right = area_x + area_w - w - inset;
    let top = area_y + inset;
    let middle = area_y + (area_h - h) / 2;
    let bottom = area_y + area_h - h - inset;
    match position {
        TopLeft => (left, top),
        TopCenter => (center, top),
        TopRight => (right, top),
        CenterLeft => (left, middle),
        Center => (center, middle),
        CenterRight => (right, middle),
        BottomLeft => (left, bottom),
        BottomCenter => (center, bottom),
        BottomRight => (right, bottom),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_cover_all_nine_positions() {
        let area = (0, 40, 1920, 1040);
        let size = (320, 56);
        let cases = [
            (PillPosition::TopLeft, (8, 48)),
            (PillPosition::TopCenter, (800, 48)),
            (PillPosition::TopRight, (1592, 48)),
            (PillPosition::CenterLeft, (8, 532)),
            (PillPosition::Center, (800, 532)),
            (PillPosition::CenterRight, (1592, 532)),
            (PillPosition::BottomLeft, (8, 1016)),
            (PillPosition::BottomCenter, (800, 1016)),
            (PillPosition::BottomRight, (1592, 1016)),
        ];
        for (position, expected) in cases {
            assert_eq!(origin(position, area, size, 8), expected, "{position:?}");
        }
    }

    #[test]
    fn origins_respect_a_secondary_monitor_offset() {
        let area = (-1280, 200, 1280, 720);
        assert_eq!(
            origin(PillPosition::TopLeft, area, (320, 56), 16),
            (-1264, 216)
        );
    }
}
