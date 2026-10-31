//! The floating recorder pill: a small transparent window that never takes focus.
//!
//! All geometry here is in logical points, never pixels. Tauri reports monitors
//! in physical pixels scaled by that monitor's own factor, while the window
//! layer (tao/AppKit) works in points. Mixing the two corrupts the position as
//! soon as screens have different scales, which is the multi-monitor bug this
//! module avoids: convert every monitor rect to points once, then keep it there.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Monitor, WebviewWindow};

use teletype_core::state::{PillPosition, PillState};

pub const PILL_LABEL: &str = "pill";

/// Gap between the pill window and the screen edge, in logical points.
const INSET: f64 = 8.0;

/// A rectangle in logical points, top-left origin with y growing downwards.
type Rect = (f64, f64, f64, f64);

/// Work area (points) of the screen the pill was last placed on. A resize must
/// stay on that screen even when the mouse has moved elsewhere, and reading
/// the window back cannot be trusted: `set_position` reaches AppKit
/// asynchronously, so a resize that fires right after a placement would still
/// see the old screen. Recording the target at placement time removes that
/// race. The pill is not draggable, so the last target is where it is.
static LAST_SCREEN: Mutex<Option<Rect>> = Mutex::new(None);

/// Set when a fresh take is placed but not yet shown. The webview publishes the
/// new content size right after it renders, and showing before that report
/// paints one frame at the previous (often smaller) size, clipped. `resize_to`
/// shows the pill as soon as the report lands. The webview could in principle
/// never report (a hung reload), so `SHOW_FALLBACK` shows it anyway: a
/// misplaced frame is better than no pill at all.
static PENDING_SHOW: Mutex<bool> = Mutex::new(false);

/// How long a fresh take waits for its size report before showing regardless.
const SHOW_FALLBACK: Duration = Duration::from_millis(150);

/// The polish result card on screen, if any. The card arrives after the take
/// has gone idle, which hid the window (always_show off), so `show_result`
/// brings it back and `dismiss_result` hides it again. Every state push
/// (`update`) ends the card, which is how a late dismiss from an old card is
/// told apart from the take that replaced it.
static RESULT_CARD: Mutex<ResultCard> = Mutex::new(ResultCard::new());

/// Generation bookkeeping for the result card. Each card gets a fresh id; a
/// dismiss only counts for the id that is still on screen.
#[derive(Debug, PartialEq, Eq)]
struct ResultCard {
    last: u64,
    shown: Option<u64>,
}

impl ResultCard {
    const fn new() -> Self {
        Self {
            last: 0,
            shown: None,
        }
    }

    fn show(&mut self) -> u64 {
        self.last += 1;
        self.shown = Some(self.last);
        self.last
    }

    fn clear(&mut self) {
        self.shown = None;
    }

    /// True when `card` is still on screen; it is gone afterwards.
    fn dismiss(&mut self, card: u64) -> bool {
        if self.shown == Some(card) {
            self.shown = None;
            true
        } else {
            false
        }
    }
}

/// Shows the pill in `state`, or hides it when idle unless `always_show` is on.
pub fn update(app: &AppHandle, state: PillState, position: PillPosition, always_show: bool) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        tracing::warn!("pill window missing; not showing {state:?}");
        return;
    };
    let vis_before = window.is_visible().unwrap_or(false);
    tracing::info!(?state, always_show, vis_before, "pill update");
    // Any new pill state replaces a result card still on screen.
    if let Ok(mut card) = RESULT_CARD.lock() {
        card.clear();
    }
    // Emit before any show/hide so the webview always has the latest state
    // when it becomes visible.
    let _ = app.emit_to(PILL_LABEL, "pill-state", &state);
    if matches!(state, PillState::Idle) && !always_show {
        cancel_pending_show();
        let _ = window.hide();
        return;
    }
    // A new take follows the mouse even when the pill is already visible (it
    // can linger in idle with always_show on). Mid-take state changes keep the
    // pill where it is, so it never jumps between screens while recording.
    let fresh = !vis_before || matches!(state, PillState::Recording { .. });
    if fresh {
        place(app, position);
    }
    if !vis_before {
        request_show(app);
    }
}

/// Brings the pill back for the polish result card and returns the card's id.
/// The card is emitted after `go_idle`, which already hid the window and
/// cancelled any pending show, so without this it would lay out in a hidden
/// window. The pill keeps its current position; the card's size report
/// (`resize_to`) re-anchors it and shows it, like a fresh take.
pub fn show_result(app: &AppHandle) -> u64 {
    let card = RESULT_CARD.lock().map(|mut c| c.show()).unwrap_or(0);
    if let Some(window) = app.get_webview_window(PILL_LABEL) {
        if !window.is_visible().unwrap_or(false) {
            request_show(app);
        }
    }
    set_interactive(app, true);
    card
}

/// The webview dismissed result card `card`. Hides the pill only when that
/// card is still the one on screen (a newer take may own the pill by now)
/// and `always_show` is off.
pub fn dismiss_result(app: &AppHandle, card: u64, always_show: bool) {
    let current = RESULT_CARD
        .lock()
        .map(|mut c| c.dismiss(card))
        .unwrap_or(false);
    if !current || always_show {
        return;
    }
    cancel_pending_show();
    if let Some(window) = app.get_webview_window(PILL_LABEL) {
        let _ = window.hide();
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
    let Some(monitor) = pointer_monitor(app) else {
        return;
    };
    let Some(size) = window_size_points(&window) else {
        return;
    };
    let area = work_area_points(&monitor);
    if let Ok(mut slot) = LAST_SCREEN.lock() {
        *slot = Some(area);
    }
    let (x, y) = origin(position, area, size, INSET);
    let (x, y) = clamp_to_area((x, y), size, area);
    crate::log_entry(
        crate::LogLevel::Info,
        format!("pill place: area={area:?} size={size:?} at=({x},{y})"),
    );
    let _ = window.set_position(LogicalPosition::new(x, y));
}

/// Resizes the pill window to the frontend's measured natural size, keeping the
/// pill anchored at its `position` slot so it does not jump. Each pill style has
/// a different natural size (the "well" is 120px tall, the capsules are 52px),
/// so the fixed 520x84 window in tauri.conf.json would clip or letterbox them;
/// this matches the reference implementation, which sizes the OS window to the
/// pill content. `width`/`height` are logical pixels.
///
/// The pill stays on the screen it is already on, never the mouse's: the mouse
/// can be anywhere while a take runs, and a resize must not teleport the pill.
/// Unlike the position, the new point size is known exactly (`width`/`height`
/// are already logical), so no outer-size conversion is involved.
///
/// Size and position land as one frame change (see `apply_frame`): applied
/// as two calls, a Bottom slot grew downwards first and then jumped back up,
/// which reads as a jitter under the cursor every time the well grows a line.
pub fn resize_to(app: &AppHandle, width: u32, height: u32, position: PillPosition) {
    let Some(window) = app.get_webview_window(PILL_LABEL) else {
        return;
    };
    if width == 0 || height == 0 {
        return;
    }
    let size = (width as f64, height as f64);
    let stored = last_screen(app);
    let Some(area) =
        stored.or_else(|| window_monitor(app, &window).map(|monitor| work_area_points(&monitor)))
    else {
        let _ = window.set_size(LogicalSize::new(size.0, size.1));
        show_if_pending(&window);
        return;
    };
    let (x, y) = origin(position, area, size, INSET);
    let (x, y) = clamp_to_area((x, y), size, area);
    crate::log_entry(
        crate::LogLevel::Info,
        format!(
            "pill resize: stored_screen={} area={area:?} size={size:?} at=({x},{y})",
            stored.is_some()
        ),
    );
    apply_frame(app, &window, (x, y), size, position);
}

/// Applies a new top-left and size to the pill, then shows a pending take.
///
/// macOS: one `setFrame:display:` on the main thread, so AppKit never draws
/// the new size at the old origin. The show runs after it in the same main
/// thread task, so a fresh take is never shown at its previous frame.
#[cfg(target_os = "macos")]
fn apply_frame(
    app: &AppHandle,
    window: &WebviewWindow,
    origin: (f64, f64),
    size: (f64, f64),
    _position: PillPosition,
) {
    let target = window.clone();
    let queued = app.run_on_main_thread(move || {
        if !set_frame_native(&target, origin, size) {
            set_frame_sequential(&target, origin, size, true);
        }
        show_if_pending(&target);
    });
    if queued.is_err() {
        set_frame_sequential(window, origin, size, true);
        show_if_pending(window);
    }
}

/// Elsewhere size and position are two calls. A Bottom slot moves first,
/// so its bottom edge never drops below the slot while the size catches up.
#[cfg(not(target_os = "macos"))]
fn apply_frame(
    _app: &AppHandle,
    window: &WebviewWindow,
    origin: (f64, f64),
    size: (f64, f64),
    position: PillPosition,
) {
    set_frame_sequential(window, origin, size, !anchored_at_bottom(position));
    show_if_pending(window);
}

/// The two-call fallback: size then position, or position then size.
fn set_frame_sequential(
    window: &WebviewWindow,
    (x, y): (f64, f64),
    (w, h): (f64, f64),
    size_first: bool,
) {
    if size_first {
        let _ = window.set_size(LogicalSize::new(w, h));
        let _ = window.set_position(LogicalPosition::new(x, y));
    } else {
        let _ = window.set_position(LogicalPosition::new(x, y));
        let _ = window.set_size(LogicalSize::new(w, h));
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn anchored_at_bottom(position: PillPosition) -> bool {
    matches!(
        position,
        PillPosition::BottomLeft | PillPosition::BottomCenter | PillPosition::BottomRight
    )
}

/// Sets the NSWindow frame in one call. Must run on the main thread. Returns
/// false when the native window or the primary screen is unavailable, so the
/// caller can fall back to the two-call path.
#[cfg(target_os = "macos")]
fn set_frame_native(window: &WebviewWindow, origin: (f64, f64), size: (f64, f64)) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    let Ok(ns_window) = window.ns_window() else {
        return false;
    };
    let ns_window = ns_window.cast::<AnyObject>();
    if ns_window.is_null() {
        return false;
    }
    // SAFETY: `NSScreen.screens` and `firstObject` are documented AppKit
    // selectors returning an autoreleased NSArray and its first NSScreen (or
    // nil, checked); `frame` returns a CGRect by value. This runs on the main
    // thread (`run_on_main_thread`), where AppKit objects may be used.
    let primary_height = unsafe {
        let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
        if screens.is_null() {
            return false;
        }
        let primary: *mut AnyObject = msg_send![screens, firstObject];
        if primary.is_null() {
            return false;
        }
        let frame: CGRect = msg_send![primary, frame];
        frame.size.height
    };
    let (x, y, w, h) = appkit_frame(origin, size, primary_height);
    let rect = CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
    // SAFETY: `ns_window` is the live NSWindow tao owns for this webview
    // window (non-null, checked above), and `setFrame:display:` takes an
    // NSRect (CGRect) and a BOOL. Main thread, as above.
    unsafe {
        let _: () = msg_send![ns_window, setFrame: rect, display: true];
    }
    true
}

/// Converts a top-left origin in global points (y down from the primary
/// screen's top, what `LogicalPosition` uses) to an AppKit frame (origin at
/// the bottom-left, y up from the primary screen's bottom).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn appkit_frame(
    (x, y): (f64, f64),
    (w, h): (f64, f64),
    primary_height: f64,
) -> (f64, f64, f64, f64) {
    (x, primary_height - (y + h), w, h)
}

/// Places a fresh take on hold, then shows it once the webview reports the
/// content size (see `PENDING_SHOW`). The fallback thread guarantees the pill
/// cannot stay invisible if that report never arrives.
fn request_show(app: &AppHandle) {
    if let Ok(mut slot) = PENDING_SHOW.lock() {
        *slot = true;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_FALLBACK);
        let due = take_pending_show();
        if due {
            if let Some(window) = app.get_webview_window(PILL_LABEL) {
                show_if_pending(&window);
            }
        }
    });
}

fn cancel_pending_show() {
    take_pending_show();
}

/// Shows the pill only if it is still hidden. Re-checks visibility at the
/// moment of the show: a fallback thread can lag an Idle hide by a beat, and
/// an unconditional `show()` would resurrect a pill the user has already
/// closed.
fn show_if_pending(window: &WebviewWindow) {
    if take_pending_show() && !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
}

fn take_pending_show() -> bool {
    PENDING_SHOW
        .lock()
        .map(|mut slot| std::mem::take(&mut *slot))
        .unwrap_or(false)
}

/// The screen the pill was last placed on, if it still exists. A screen that
/// was disconnected must not keep pulling the pill off-screen, so the stored
/// rect is matched against the current monitor list on every use.
fn last_screen(app: &AppHandle) -> Option<Rect> {
    let stored = (*LAST_SCREEN.lock().ok()?)?;
    let monitors = app.available_monitors().ok()?;
    monitors
        .iter()
        .any(|monitor| work_area_points(monitor) == stored)
        .then_some(stored)
}

/// The monitor under the mouse cursor, falling back to the primary one.
///
/// tao reports the cursor in the primary monitor's physical space: global
/// points multiplied by the primary's scale factor. `monitor_from_point`
/// expects unscaled global points (it compares against `CGDisplayBounds`), so
/// the raw value has to be divided back first. Without that, every off-screen
/// value misses and the pill silently stays on the primary display.
fn pointer_monitor(app: &AppHandle) -> Option<Monitor> {
    let primary = app.primary_monitor().ok().flatten();
    let scale = primary
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let hit = app.cursor_position().ok().and_then(|p| {
        let (x, y) = cursor_points((p.x, p.y), scale);
        let monitor = app.monitor_from_point(x, y).ok().flatten();
        crate::log_entry(
            crate::LogLevel::Info,
            format!(
                "pill pointer: raw=({},{}) primary_scale={scale} points=({x},{y}) hit={:?}",
                p.x,
                p.y,
                monitor.as_ref().map(|m| (m.position(), m.scale_factor()))
            ),
        );
        monitor
    });
    if hit.is_none() {
        crate::log_entry(
            crate::LogLevel::Info,
            "pill pointer: miss, falling back to the primary monitor".to_string(),
        );
    }
    hit.or(primary)
}

/// Undoes tao's primary-scale multiplication on the reported cursor position.
fn cursor_points((x, y): (f64, f64), primary_scale: f64) -> (f64, f64) {
    let scale = if primary_scale > 0.0 {
        primary_scale
    } else {
        1.0
    };
    (x / scale, y / scale)
}

/// The monitor the pill window currently sits on. `current_monitor` asks
/// AppKit for the window's screen, so it is authoritative; the frame centre is
/// a fallback for the brief moment before a queued move lands. The mouse is
/// never consulted here.
fn window_monitor(app: &AppHandle, window: &WebviewWindow) -> Option<Monitor> {
    if let Ok(Some(monitor)) = window.current_monitor() {
        return Some(monitor);
    }
    let scale = window
        .scale_factor()
        .ok()
        .filter(|s| *s > 0.0)
        .unwrap_or(1.0);
    let centre = window.outer_position().ok().map(|pos| {
        let size = window
            .outer_size()
            .unwrap_or_else(|_| tauri::PhysicalSize::new(0, 0));
        (
            pos.x as f64 / scale + size.width as f64 / scale / 2.0,
            pos.y as f64 / scale + size.height as f64 / scale / 2.0,
        )
    });
    if let Some((x, y)) = centre {
        if let Ok(Some(monitor)) = app.monitor_from_point(x, y) {
            return Some(monitor);
        }
    }
    pointer_monitor(app)
}

/// The monitor's work area (screen minus menu bar and dock) in logical points.
fn work_area_points(monitor: &Monitor) -> Rect {
    let area = monitor.work_area();
    // `work_area` is physical pixels scaled by this monitor's factor, so
    // dividing by that same factor lands in the NSScreen point space that
    // `LogicalPosition` lives in.
    to_points(
        (
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
        ),
        monitor.scale_factor(),
    )
}

/// Converts a physical rect to points. A non-positive scale (broken monitor)
/// falls back to 1 so the geometry stays finite.
fn to_points((x, y, w, h): (i32, i32, u32, u32), scale: f64) -> Rect {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    (
        x as f64 / scale,
        y as f64 / scale,
        w as f64 / scale,
        h as f64 / scale,
    )
}

/// The pill's current outer size in logical points.
fn window_size_points(window: &WebviewWindow) -> Option<(f64, f64)> {
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().ok()?;
    if scale <= 0.0 {
        return None;
    }
    Some((size.width as f64 / scale, size.height as f64 / scale))
}

/// Clamp a top-left so the (w, h) window stays inside the work area in points.
/// If the window is larger than the area, pins it to the top-left of the area
/// rather than panicking (the pill content will be clipped by the OS).
fn clamp_to_area((x, y): (f64, f64), (w, h): (f64, f64), area: Rect) -> (f64, f64) {
    let (ax, ay, aw, ah) = area;
    let max_x = ax + aw - w;
    let max_y = ay + ah - h;
    // Guard against min > max (window larger than the work area).
    let cx = if max_x >= ax { x.clamp(ax, max_x) } else { ax };
    let cy = if max_y >= ay { y.clamp(ay, max_y) } else { ay };
    (cx, cy)
}

/// Top-left corner of the pill window within a work area, all in logical points
/// with y growing downwards.
fn origin(position: PillPosition, area: Rect, (w, h): (f64, f64), inset: f64) -> (f64, f64) {
    use PillPosition::*;
    let (area_x, area_y, area_w, area_h) = area;
    let left = area_x + inset;
    let center = area_x + (area_w - w) / 2.0;
    let right = area_x + area_w - w - inset;
    let top = area_y + inset;
    let middle = area_y + (area_h - h) / 2.0;
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
    fn appkit_frame_flips_y_about_the_primary_screen() {
        // A 250x52 pill at the bottom of a 982pt-tall primary screen sits
        // 8pt above its bottom edge in AppKit's y-up space.
        assert_eq!(
            appkit_frame((631.0, 922.0), (250.0, 52.0), 982.0),
            (631.0, 8.0, 250.0, 52.0)
        );
        // A screen above the primary has negative top-left y and a frame
        // above the primary's height.
        assert_eq!(
            appkit_frame((0.0, -1080.0), (400.0, 120.0), 982.0),
            (0.0, 1942.0, 400.0, 120.0)
        );
    }

    #[test]
    fn only_bottom_slots_move_before_resizing() {
        assert!(anchored_at_bottom(PillPosition::BottomLeft));
        assert!(anchored_at_bottom(PillPosition::BottomCenter));
        assert!(anchored_at_bottom(PillPosition::BottomRight));
        assert!(!anchored_at_bottom(PillPosition::TopCenter));
        assert!(!anchored_at_bottom(PillPosition::Center));
    }

    #[test]
    fn result_card_dismiss_counts_only_for_the_card_on_screen() {
        let mut card = ResultCard::new();
        let first = card.show();
        assert!(card.dismiss(first));
        // A second dismiss of the same card (X click after the timeout) is a
        // no-op, so it can never hide whatever came next.
        assert!(!card.dismiss(first));
    }

    #[test]
    fn a_new_take_makes_a_late_dismiss_a_no_op() {
        let mut card = ResultCard::new();
        let first = card.show();
        // `update` ran for the next take's Recording state.
        card.clear();
        assert!(!card.dismiss(first));
    }

    #[test]
    fn a_newer_card_ignores_the_older_cards_dismiss() {
        let mut card = ResultCard::new();
        let old = card.show();
        let new = card.show();
        assert_ne!(old, new);
        assert!(!card.dismiss(old));
        assert!(card.dismiss(new));
    }

    #[test]
    fn origins_cover_all_nine_positions() {
        let area = (0.0, 40.0, 1920.0, 1040.0);
        let size = (320.0, 56.0);
        let cases = [
            (PillPosition::TopLeft, (8.0, 48.0)),
            (PillPosition::TopCenter, (800.0, 48.0)),
            (PillPosition::TopRight, (1592.0, 48.0)),
            (PillPosition::CenterLeft, (8.0, 532.0)),
            (PillPosition::Center, (800.0, 532.0)),
            (PillPosition::CenterRight, (1592.0, 532.0)),
            (PillPosition::BottomLeft, (8.0, 1016.0)),
            (PillPosition::BottomCenter, (800.0, 1016.0)),
            (PillPosition::BottomRight, (1592.0, 1016.0)),
        ];
        for (position, expected) in cases {
            assert_eq!(origin(position, area, size, 8.0), expected, "{position:?}");
        }
    }

    #[test]
    fn origins_respect_a_secondary_monitor_offset() {
        let area = (-1280.0, 200.0, 1280.0, 720.0);
        assert_eq!(
            origin(PillPosition::TopLeft, area, (320.0, 56.0), 16.0),
            (-1264.0, 216.0)
        );
    }

    #[test]
    fn origins_respect_a_screen_above_primary() {
        // A screen whose point origin has a negative y (placed above the
        // built-in display). Bottom-right must land in that screen's quadrant,
        // not in the primary's.
        let area = (-1512.0, -982.0, 1512.0, 982.0);
        assert_eq!(
            origin(PillPosition::BottomRight, area, (320.0, 56.0), 8.0),
            (-1512.0 + 1512.0 - 320.0 - 8.0, -982.0 + 982.0 - 56.0 - 8.0)
        );
    }

    #[test]
    fn physical_rects_convert_to_points_per_scale() {
        // Retina primary: 3024x1964 physical at scale 2 is 1512x982 points.
        assert_eq!(
            to_points((0, 0, 3024, 1964), 2.0),
            (0.0, 0.0, 1512.0, 982.0)
        );
        // Non-Retina DELL above and to the right: physical equals points.
        assert_eq!(
            to_points((801, -1080, 1920, 1080), 1.0),
            (801.0, -1080.0, 1920.0, 1080.0)
        );
        // A zero scale must not divide by zero.
        assert_eq!(to_points((0, 0, 100, 100), 0.0), (0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn cursor_points_undo_the_primary_scale() {
        // tao reports the mouse in global points multiplied by the primary
        // factor, so a pointer at (1500, -500) on a 1x screen under a 2x
        // primary arrives as (3000, -1000). This is the exact multi-monitor
        // defect: unscaled, that value contains no monitor and the pill falls
        // back to the primary display.
        assert_eq!(cursor_points((3000.0, -1000.0), 2.0), (1500.0, -500.0));
        // A 1x primary needs no correction.
        assert_eq!(cursor_points((1500.0, -500.0), 1.0), (1500.0, -500.0));
        // A broken scale must not divide by zero.
        assert_eq!(cursor_points((10.0, 20.0), 0.0), (10.0, 20.0));
    }

    #[test]
    fn mixed_scale_monitors_place_in_their_own_point_space() {
        let retina = to_points((0, 0, 3024, 1964), 2.0);
        let dell = to_points((801, -1080, 1920, 1080), 1.0);
        let size = (320.0, 56.0);
        // Same slot on each screen resolves inside that screen's own rect,
        // which is the property the old physical/scale mix broke.
        let a = clamp_to_area(
            origin(PillPosition::TopLeft, retina, size, 8.0),
            size,
            retina,
        );
        assert_eq!(a, (8.0, 8.0));
        let b = clamp_to_area(origin(PillPosition::TopLeft, dell, size, 8.0), size, dell);
        assert_eq!(b, (809.0, -1072.0));
    }

    #[test]
    fn tall_pill_is_clamped_inside_a_short_screen() {
        // The "well" is 120 points tall; a 100-point work area cannot fit it,
        // so its y pins to the area top instead of going negative. The x slot
        // still applies because the width does fit.
        let area = (0.0, 0.0, 800.0, 100.0);
        let size = (320.0, 120.0);
        assert_eq!(
            clamp_to_area(
                origin(PillPosition::BottomCenter, area, size, 8.0),
                size,
                area
            ),
            (240.0, 0.0)
        );
    }

    #[test]
    fn clamp_keeps_a_fitting_pill_fully_inside() {
        let area = (801.0, -1080.0, 1920.0, 1080.0);
        let size = (320.0, 56.0);
        // A top-left past the right and bottom edges snaps back inside.
        let (x, y) = clamp_to_area((2600.0, -40.0), size, area);
        assert_eq!((x, y), (801.0 + 1920.0 - 320.0, -1080.0 + 1080.0 - 56.0));
    }
}
