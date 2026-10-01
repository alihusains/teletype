//! The floating recorder pill: a small transparent window that never takes focus.

use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};

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

/// Resizes the pill window to the frontend's measured natural size, keeping the
/// pill anchored at its `position` slot so it does not jump. Each pill style has
/// a different natural size (the "well" is 120px tall, the capsules are 44px),
/// so the fixed 520x84 window in tauri.conf.json would clip or letterbox them;
/// this matches the reference implementation, which sizes the OS window to the
/// pill content. `width`/`height` are in logical pixels.
pub fn resize_to(app: &AppHandle, width: u32, height: u32, position: PillPosition) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        return;
    };
    if width == 0 || height == 0 {
        return;
    }
    // Remember the anchor point (the pill's position within its old rect) so we
    // can re-derive the top-left after the resize.
    let old = window
        .outer_size()
        .unwrap_or_else(|_| tauri::PhysicalSize::new(0, 0));
    let old_pos = window
        .outer_position()
        .unwrap_or_else(|_| tauri::PhysicalPosition::new(0, 0));
    let (ax, ay) = anchor_offset(position, old.width as i32, old.height as i32);
    let anchor_x = old_pos.x + ax;
    let anchor_y = old_pos.y + ay;

    let _ = window.set_size(LogicalSize::new(width as f64, height as f64));
    // Re-resolve the monitor (the size change does not move the cursor) and
    // clamp the new top-left to the work area so a tall "well" never pokes
    // off-screen.
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        let w = (width as f64 * scale).round() as i32;
        let h = (height as f64 * scale).round() as i32;
        let (nax, nay) = anchor_offset(position, w, h);
        let new_left = anchor_x - nax;
        let new_top = anchor_y - nay;
        let (nx, ny) = clamp_to_area(
            (new_left, new_top),
            (w, h),
            (
                area.position.x,
                area.position.y,
                area.size.width as i32,
                area.size.height as i32,
            ),
        );
        let _ = window.set_position(PhysicalPosition::new(nx, ny));
    }
}

/// Physical-pixel offset from the window's top-left to its anchor point, for a
/// given `PillPosition`. Mirrors `origin`'s nine grid slots: left/center/right
/// horizontally, top/middle/bottom vertically.
fn anchor_offset(position: PillPosition, w: i32, h: i32) -> (i32, i32) {
    use PillPosition::*;
    let x = match position {
        TopLeft | CenterLeft | BottomLeft => 0,
        TopCenter | Center | BottomCenter => w / 2,
        TopRight | CenterRight | BottomRight => w,
    };
    let y = match position {
        TopLeft | TopCenter | TopRight => 0,
        CenterLeft | Center | CenterRight => h / 2,
        BottomLeft | BottomCenter | BottomRight => h,
    };
    (x, y)
}

/// Clamp a top-left so the (w, h) window stays inside the work area.
/// If the window is larger than the area, pins it to the top-left of the area
/// rather than panicking (the pill content will be clipped by the OS).
fn clamp_to_area(
    (x, y): (i32, i32),
    (w, h): (i32, i32),
    (area_x, area_y, area_w, area_h): (i32, i32, i32, i32),
) -> (i32, i32) {
    let max_x = area_x + area_w - w;
    let max_y = area_y + area_h - h;
    // Guard against min > max (window larger than the work area).
    let cx = if max_x >= area_x {
        x.clamp(area_x, max_x)
    } else {
        area_x
    };
    let cy = if max_y >= area_y {
        y.clamp(area_y, max_y)
    } else {
        area_y
    };
    (cx, cy)
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
