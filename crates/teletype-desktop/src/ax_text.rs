//! Direct Accessibility text insertion: the fast first injection tier.
//!
//! # Why this exists
//!
//! The clipboard-paste path costs about 600 ms of fixed sleeps (250 ms before
//! the paste, 350 ms before restoring the clipboard) on *every* dictation, and
//! it fails outright in apps that virtualise the pasteboard: several Electron
//! apps, RDP sessions, and clipboard managers. Writing through the
//! Accessibility API inserts text at the caret with no clipboard round trip and
//! no sleeps.
//!
//! # Why verification is the whole point
//!
//! `AXUIElementSetAttributeValue` reports success in several toolkits that then
//! silently discard the write. An unverified success is worse than a failure:
//! the caller would skip the clipboard fallback and the user's words would
//! vanish. So every write is read back and checked before it is reported as
//! successful, which is what makes a `false` return safe to fall back from.
//!
//! # Why this is Rust and not Objective-C
//!
//! The repo's other platform bridges (`fn_tap.m`, `typing_tap.m`,
//! `mic_permission.m`) are Objective-C because they need run loops and ObjC
//! blocks that are genuinely awkward from Rust. AX calls are not: they are
//! plain C functions over CF types, and `objc2-application-services` ships
//! pre-generated bindings for them. That matters concretely, because the AX
//! headers ship only in the full Xcode SDK, while these bindings compile
//! against the Command Line Tools SDK alone, which is what CI uses.
//!
//! The attribute and role constants are the one thing the bindings do not
//! pre-generate, because in the C headers they are `CFSTR` macros rather than
//! symbols. Their string values are part of the stable, documented AX
//! contract (`AXValue`, `AXFocusedUIElement`, `AXTextField`, and so on), so they
//! are constructed here by name.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFRange, CFRetained, CFString, CFType, CGPoint, CGSize};
use objc2_foundation::NSString;
use teletype_core::platform::{InjectionOutcome, InjectionRoute};

/// An AX attribute or role name, built once and reused.
///
/// A plain `&'static str` rather than a cached `CFString`: the names are
/// interned by the CF layer on every construction, and building four short
/// strings per dictation is far cheaper than the alternative of leaking a
/// process-lifetime allocation. This also sidesteps `CFString` not being
/// `Sync`, which would make a `static OnceLock<CFString>` illegal.
type Name = &'static str;

const ATTR_VALUE: Name = "AXValue";
const ATTR_SELECTED_TEXT: Name = "AXSelectedText";
const ATTR_FOCUSED: Name = "AXFocusedUIElement";
const ATTR_ROLE: Name = "AXRole";
const ATTR_SELECTED_RANGE: Name = "AXSelectedTextRange";

const ROLE_TEXT_FIELD: Name = "AXTextField";
const ROLE_TEXT_AREA: Name = "AXTextArea";
const ROLE_COMBO_BOX: Name = "AXComboBox";
/// Used only by the tests that pin the writable-role allowlist.
#[cfg(test)]
const ROLE_SEARCH_FIELD: Name = "AXSearchField";
/// Used only by the tests that pin the writable-role allowlist.
#[cfg(test)]
const ROLE_TEXT_URL_FIELD: Name = "AXURLTextField";
/// Used only by the tests that pin the writable-role allowlist.
#[cfg(test)]
const ROLE_BUTTON: Name = "AXButton";
/// Used only by the tests that pin the writable-role allowlist.
#[cfg(test)]
const ROLE_STATIC_TEXT: Name = "AXStaticText";

/// Roles we are willing to write into.
///
/// `AXButton` and `AXStaticText` are deliberately excluded: writing into a
/// read-only element would either fail confusingly or, worse, overwrite UI the
/// user can see but never asked to change.
/// Roles we are willing to write into.
///
/// `AXButton` and `AXStaticText` are deliberately excluded: writing into a
/// read-only element would either fail confusingly or, worse, overwrite UI the
/// user can see but never asked to change.
///
/// `AXSearchField` and `AXURLTextField` are likewise excluded (BUG-006):
/// browsers expose their URL bars and search boxes with those roles, so
/// allowing them would let dictation land in a URL bar and navigate to the
/// dictated text.
fn is_writable_role(role: &str) -> bool {
    role == ROLE_TEXT_FIELD || role == ROLE_TEXT_AREA || role == ROLE_COMBO_BOX
}

/// An owned +1 CF reference, released on drop.
///
/// The AX calls hand back raw `+1` references. Wrapping them keeps every early
/// return in this module leak-free without threading manual releases through
/// each branch.
struct Owned(*mut CFType);

impl Owned {
    fn as_ptr(&self) -> *mut CFType {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the pointer is a live +1 reference we own, released exactly
        // once here. `CFRelease` is the CF destructor for any CFType.
        unsafe { CFRelease(self.0.cast()) };
    }
}

extern "C" {
    /// `CFRelease`, the CoreFoundation release entry point. The objc2
    /// bindings expose it only through `CFRetained`, and this module holds raw
    /// pointers returned by the C AX functions.
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// Copy an attribute off `element`, returning the retained value on success.
fn copy_attribute(element: &AXUIElement, attribute: &CFString) -> Option<Owned> {
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: `element` and `attribute` are live CF objects and `value` is a
    // valid out-parameter. On success the returned reference is +1.
    let out = NonNull::new(&mut value as *mut *const CFType)?;
    // SAFETY: `out` is a valid out-parameter (a non-null pointer to a
    // `*const CFType` local) and the call is on live CF objects. On success
    // the caller's reference is +1 and released by `Owned`.
    let err = unsafe { element.copy_attribute_value(attribute, out) };
    if err != AXError::Success {
        return None;
    }
    // SAFETY: on success `value` is a non-null +1 reference, which `Owned` takes
    // ownership of and releases on drop.
    Some(Owned(value as *mut CFType))
}

/// Read a string attribute into an owned Rust `String`.
fn copy_string_attribute(element: &AXUIElement, attribute: &CFString) -> Option<String> {
    let value = copy_attribute(element, attribute)?;
    if value.as_ptr().is_null() {
        return None;
    }
    // SAFETY: the value of a text attribute is an NSString. `to_string` borrows
    // and does not consume the reference, and `value` outlives the read.
    Some(unsafe { (*(value.as_ptr() as *const NSString)).to_string() })
}

/// The focused element, when it is a text field we can write into.
///
/// The returned pointer is a +1 reference the caller owns and must release with
/// [`release_element`].
fn focused_text_field() -> Option<NonNull<AXUIElement>> {
    // SAFETY: `new_system_wide` takes no arguments and cannot fail.
    let system_wide = unsafe { AXUIElement::new_system_wide() };
    let focus_name = CFString::from_str(ATTR_FOCUSED);
    let focus = copy_attribute(&system_wide, &focus_name)?;
    if focus.as_ptr().is_null() {
        tracing::warn!("ax_insert: no focused UI element (AXFocusedUIElement is null)");
        return None;
    }
    // SAFETY: kAXFocusedUIElement always yields an AXUIElement, so retyping the
    // +1 reference is well defined. We keep the ownership `focus` holds and
    // forget the wrapper, so the reference is not released twice.
    let element = NonNull::new(focus.as_ptr() as *mut AXUIElement)?;
    std::mem::forget(focus);
    let role_name = CFString::from_str(ATTR_ROLE);
    // SAFETY: `element` is a live +1 reference for the read.
    let element_ref = unsafe { element.as_ref() };
    let role = match copy_string_attribute(element_ref, &role_name) {
        Some(r) => r,
        None => {
            tracing::warn!("ax_insert: could not read AXRole of focused element");
            // SAFETY: `element` is a live +1 reference held by this function;
            // releasing it here balances the `CFRetain` from `as_ref()` on
            // line 176. The pointer is not used after this release.
            unsafe { CFRelease(element.as_ptr().cast()) };
            return None;
        }
    };
    if is_writable_role(&role) {
        Some(element)
    } else {
        tracing::warn!(
            role,
            "ax_insert: focused element role is not writable \
             (only AXTextField, AXTextArea, AXComboBox are)"
        );
        // SAFETY: releasing the +1 reference this function owns.
        unsafe { CFRelease(element.as_ptr().cast()) };
        None
    }
}

/// Releases a focused-element reference from [`focused_text_field`].
fn release_element(element: NonNull<AXUIElement>) {
    // SAFETY: the pointer came from `focused_text_field`, which returned a +1
    // reference, and it is released exactly once here.
    unsafe { CFRelease(element.as_ptr().cast()) };
}

/// Caret offset and selection length, in UTF-16 units (the unit AX uses).
fn selected_range(element: &AXUIElement, text_len: usize) -> (usize, usize) {
    let range_name = CFString::from_str(ATTR_SELECTED_RANGE);
    let Some(value) = copy_attribute(element, &range_name) else {
        return (text_len, 0);
    };
    // The fallback when the attribute is missing or is not a range.
    let no_range = CFRange {
        location: 0,
        length: 0,
    };
    // SAFETY: the selected-range attribute is an AXValue, and its type is
    // checked above before the payload is read, so a value of another type is
    // never reinterpreted as a CFRange. `value` is the +1 `CFType` the copy
    // returned, live for the duration of this scope.
    let range = unsafe {
        let ax = &*value.as_ptr().cast::<AXValue>();
        if ax.r#type() != AXValueType::CFRange {
            no_range
        } else {
            let mut out = no_range;
            if ax.value(AXValueType::CFRange, NonNull::from(&mut out).cast()) {
                out
            } else {
                no_range
            }
        }
    };
    if range.length < 0 || range.location < 0 {
        // No usable selection: append at the end.
        return (text_len, 0);
    }
    let start = (range.location as usize).min(text_len);
    let end = start.saturating_add(range.length as usize).min(text_len);
    (start, end - start)
}

/// Whether a direct insert is possible right now, and where the caret is.
pub fn can_insert() -> Option<usize> {
    let element = focused_text_field()?;
    // A field we cannot read cannot be verified after writing, so a readable
    // value is required rather than optional.
    let value_name = CFString::from_str(ATTR_VALUE);
    // SAFETY: `element` is a live +1 AXUIElement reference for the whole body.
    let element_ref = unsafe { element.as_ref() };
    let caret = copy_string_attribute(element_ref, &value_name).map(|current| {
        let utf16 = current.encode_utf16().count();
        selected_range(element_ref, utf16).0
    });
    release_element(element);
    caret
}

/// What happened when we tried to write into the focused field.
///
/// This used to be a `bool`, and the two failure modes it conflated are why
/// dictations were typed twice: `insert_into` returned `false` both when
/// nothing was written *and* when the write landed but the receiving app
/// normalised it. The caller cannot tell those apart, so it fell back to the
/// clipboard and pasted the same text a second time.
///
/// Concretely: dictate "don't stop" into a field with smart quotes. We write
/// `don't`, the app rewrites the apostrophe to `don’t`, the read-back does not
/// match, we reported failure, and the clipboard route pasted it again. The
/// field ended up reading `don't stop don't stop`.
///
/// A second, worse variant: Terminal.app treats a whole-value set as *type
/// this* rather than *replace with this*. It appends the string to the display
/// while reporting the set string back as its value, so a read-modify-write of
/// the whole value re-sends every previous take on every take, and the
/// read-back matches every time. Take 2 inserted take1+take2, take 3 inserted
/// take1+take2+take3, all logged "direct write ok". The fix is to never send
/// old content in the first place (see [`insert`]), so there is nothing for an
/// appending app to duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// Written, and read back exactly as we sent it.
    Exact,
    /// Written, and the app changed it on the way in (smart quotes, autocorrect,
    /// a trailing space), or the app reports oddly while still landing the
    /// text (a terminal emulator that appends the set string to the display
    /// while parroting it back as its value). Still success: falling back here
    /// is what caused the double paste, and for an appending app it would
    /// re-send old takes a second time.
    Normalized,
    /// Nothing was written. Only this case may fall back to the clipboard.
    Refused,
}

impl Insert {
    /// Did the text land in the field?
    pub fn landed(self) -> bool {
        !matches!(self, Insert::Refused)
    }
}

/// Inserts `text` at the caret of the focused field, replacing any selection.
///
/// Sends **only the new text**, never the field's existing content. The
/// previous implementation read the whole value, spliced, and wrote the whole
/// string back, which assumes a value-set means *replace*. Terminal.app treats
/// it as *type this*: the string is appended to the display while the reported
/// value parrots what was set, so every take re-sent all previous takes and
/// the read-back matched every time. Selection replacement is a true insert
/// primitive in every toolkit (AppKit, Chromium, Electron), so old content can
/// never be re-sent and an appending app has nothing to duplicate. The worst
/// case is now one copy or zero copies, never a growing concatenation.
pub fn insert(text: &str) -> Insert {
    insert_detailed(text).0
}

/// [`insert`], plus whether the focused field could be read.
///
/// The readability bit drives clipboard retention: when the target app is not
/// exposing its accessibility tree (a sleeping Chromium host, no focused
/// element at all), a clipboard paste still lands via keystrokes but is
/// unverifiable, so the dictation must not be retained on the clipboard
/// afterwards. `true` unless the element was missing or its value unreadable.
pub fn insert_detailed(text: &str) -> (Insert, bool) {
    if text.is_empty() {
        return (Insert::Exact, true);
    }
    let Some(element) = focused_text_field() else {
        return (Insert::Refused, false);
    };
    // SAFETY: `element` is a live +1 AXUIElement reference for the call.
    let out = insert_via_selection_detailed(unsafe { element.as_ref() }, text);
    release_element(element);
    out
}

/// The insert itself, against an already-resolved focused element.
///
/// Replaces the current selection (a collapsed selection is a pure insert)
/// with exactly `text`. The old value is read only as the baseline for
/// verification, never as material for the write.
///
/// Returns the outcome plus readability: `false` only when the value could
/// not be read at all, which is the sleeping-host case retention must hear
/// about.
fn insert_via_selection_detailed(element: &AXUIElement, text: &str) -> (Insert, bool) {
    let value_name = CFString::from_str(ATTR_VALUE);
    let Some(current) = copy_string_attribute(element, &value_name) else {
        tracing::warn!("ax_insert: refused — could not read AXValue (sleeping host or no value)");
        return (Insert::Refused, false);
    };

    // AX reports offsets in UTF-16 units, so splice on that boundary to avoid
    // splitting a composed character or an emoji surrogate pair. This is the
    // *expectation* for verification, not the write: the write below sends
    // only `text`.
    let units: Vec<u16> = current.encode_utf16().collect();
    let (caret, sel_len) = selected_range(element, units.len());
    let expected = splice_utf16(&units, caret, sel_len, text);

    // SAFETY: `element` is live and `ns` is an autoreleased NSString created on
    // this thread, so both outlive the call.
    let wrote = unsafe {
        let ns = NSString::from_str(text);
        let cf: &CFType = &*(Retained::as_ptr(&ns).cast::<CFType>());
        let sel_name = CFString::from_str(ATTR_SELECTED_TEXT);
        element.set_attribute_value(&sel_name, cf) == AXError::Success
    };
    if !wrote {
        tracing::warn!(
            text_len = text.len(),
            "ax_insert: refused — AXSelectedText set returned non-Success"
        );
        return (Insert::Refused, true);
    }

    // Move the caret past the inserted text so continued typing lands in the
    // right place. Best effort: a field that refuses this still has the text,
    // and the verification below is what decides success.
    move_caret(element, caret + text.encode_utf16().count());

    // Read back, to tell an exact write from one the app adjusted. Past this
    // point the text is already in the field, so nothing here may report a
    // failure the caller would answer by pasting a second copy.
    let Some(back) = copy_string_attribute(element, &value_name) else {
        tracing::info!("ax_insert: normalized — write succeeded but read-back unavailable");
        return (Insert::Normalized, true);
    };
    let outcome = classify_write(&current, caret, text, &expected, &back);
    if outcome == Insert::Refused {
        tracing::warn!(
            text_len = text.len(),
            old_len = current.len(),
            back_len = back.len(),
            caret,
            "ax_insert: refused — read-back identical to pre-write value \
             (write silently discarded by the app)"
        );
    }
    (outcome, true)
}

/// Decide what a write accomplished from the value before, the caret, the text
/// sent, the value a true insert would produce, and the value read back.
///
/// Pure, so the terminal-emulator case is pinnable without AX: an app that
/// appends the set string to the display while parroting it back as its value
/// defeats any read-back that only compares against the sent string, and the
/// only defence is never sending old content (which [`insert`] guarantees).
fn classify_write(old: &str, caret: usize, text: &str, expected: &str, back: &str) -> Insert {
    if back == expected {
        return Insert::Exact;
    }
    // Some apps normalise whitespace or substitute characters on write, so the
    // whole value differs while our text still landed exactly where targeted.
    // That is the app doing its job, and it still counts as success.
    let back_units: Vec<u16> = back.encode_utf16().collect();
    let inserted: Vec<u16> = text.encode_utf16().collect();
    if caret + inserted.len() <= back_units.len()
        && back_units[caret..caret + inserted.len()] == inserted[..]
    {
        return Insert::Exact;
    }
    if back == old {
        // Byte-identical to before the write: the set was silently discarded
        // and nothing landed, so the clipboard fallback cannot double anything.
        // This is the only case that may fall back.
        return Insert::Refused;
    }
    // The value changed but is not what a true insert would produce. Either
    // the app rewrote what we sent (smart quotes, autocorrect), or it reports
    // oddly while still landing the text (a terminal emulator parroting the
    // set string while appending it to the display). The set call succeeded
    // and something landed, so falling back would paste a second copy.
    Insert::Normalized
}

/// Replace `units[caret..caret+sel_len]` with `text`, returning the new string.
///
/// The units are decoded with [`String::from_utf16_lossy`] rather than per-unit
/// `char::from_u32`, because a surrogate *pair* is two `u16` values that only
/// combine into one character. Decoding unit by unit turns every emoji into two
/// replacement characters, which would silently corrupt any dictation
/// containing one.
fn splice_utf16(units: &[u16], caret: usize, sel_len: usize, text: &str) -> String {
    let start = caret.min(units.len());
    let end = (start + sel_len).min(units.len());
    let mut out = String::with_capacity(units.len() + text.len());
    out.push_str(&String::from_utf16_lossy(&units[..start]));
    out.push_str(text);
    out.push_str(&String::from_utf16_lossy(&units[end..]));
    out
}

/// Best-effort caret placement after an insert.
fn move_caret(element: &AXUIElement, offset: usize) {
    let mut range = CFRange {
        location: offset as isize,
        length: 0,
    };
    // SAFETY: `range` is a live local, borrowed only for this call, and the
    // `AXValue` it creates is +1. AX does not take ownership of an attribute
    // value, so the `CFRetained` releases it when this function returns.
    //
    // Bound to a `let` rather than inlined into `if let` so the SAFETY comment
    // is the line immediately above the `unsafe` block, which is where clippy's
    // `undocumented_unsafe_blocks` looks for it.
    let new_value = unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&mut range).cast()) };
    if let Some(value) = new_value {
        let name = CFString::from_str(ATTR_SELECTED_RANGE);
        // SAFETY: `value` is a live +1 `AXValue` and `name` a live `CFString`;
        // both are borrowed only for the duration of the call, which copies
        // what it needs.
        let cf: &CFType = unsafe { &*CFRetained::as_ptr(&value).as_ptr().cast::<CFType>() };
        // SAFETY: as above.
        let _ = unsafe { element.set_attribute_value(&name, cf) };
    }
}

/// The focused field's current value, for the personalization diff.
pub fn focused_value() -> Option<String> {
    let element = focused_text_field()?;
    let value_name = CFString::from_str(ATTR_VALUE);
    // SAFETY: `element` is a live +1 AXUIElement reference for the read.
    let value = copy_string_attribute(unsafe { element.as_ref() }, &value_name);
    release_element(element);
    value
}

/// Frame (x, y, w, h) of the frontmost app's focused window, when the
/// accessibility tree exposes it.
///
/// Walks system-wide -> focused application -> focused window, then reads
/// `AXPosition` + `AXSize`. `None` for a sleeping host, a windowless app, or
/// any unreadable step: the delivery gate treats that as no evidence, never
/// as a mismatch. Coordinates are truncated, not rounded; a 1 px difference
/// still counts as a different window, which is the safe direction (an abort
/// strands recoverable text, a wrong paste does not un-send).
pub fn focused_window_frame() -> Option<[i64; 4]> {
    // SAFETY: `new_system_wide` takes no arguments and cannot fail.
    let system_wide = unsafe { AXUIElement::new_system_wide() };
    let app_name = CFString::from_str("AXFocusedApplication");
    let app = copy_attribute(&system_wide, &app_name)?;
    if app.as_ptr().is_null() {
        return None;
    }
    // SAFETY: kAXFocusedApplication always yields an AXUIElement, and `app`
    // outlives the borrow.
    let app_ref = unsafe { &*(app.as_ptr() as *const AXUIElement) };
    let win_name = CFString::from_str("AXFocusedWindow");
    let win = copy_attribute(app_ref, &win_name)?;
    if win.as_ptr().is_null() {
        return None;
    }
    // SAFETY: kAXFocusedWindow always yields an AXUIElement, and `win`
    // outlives the borrow.
    let win_ref = unsafe { &*(win.as_ptr() as *const AXUIElement) };
    let (x, y) = read_point(win_ref, "AXPosition")?;
    let (w, h) = read_size(win_ref, "AXSize")?;
    Some([x as i64, y as i64, w as i64, h as i64])
}

/// Read an `AXValue`-typed point attribute (e.g. `AXPosition`).
fn read_point(element: &AXUIElement, attribute: &str) -> Option<(f64, f64)> {
    let name = CFString::from_str(attribute);
    let value = copy_attribute(element, &name)?;
    if value.as_ptr().is_null() {
        return None;
    }
    // SAFETY: position attributes are AXValues; the type check below rejects
    // anything else before the payload is read. The borrow and the type
    // query are valid on the live `value` this function owns.
    let is_point = unsafe {
        let ax = &*(value.as_ptr() as *const AXValue);
        ax.r#type() == AXValueType::CGPoint
    };
    if !is_point {
        return None;
    }
    // SAFETY: `value` holds a CGPoint (just checked).
    let ax = unsafe { &*(value.as_ptr() as *const AXValue) };
    let mut out = CGPoint { x: 0.0, y: 0.0 };
    // SAFETY: `out` is a live local of the exact type the AXValue holds (just
    // checked), borrowed only for the call.
    if unsafe { ax.value(AXValueType::CGPoint, NonNull::from(&mut out).cast()) } {
        Some((out.x, out.y))
    } else {
        None
    }
}

/// Read an `AXValue`-typed size attribute (e.g. `AXSize`).
fn read_size(element: &AXUIElement, attribute: &str) -> Option<(f64, f64)> {
    let name = CFString::from_str(attribute);
    let value = copy_attribute(element, &name)?;
    if value.as_ptr().is_null() {
        return None;
    }
    // SAFETY: as in `read_point`: the type query runs on the live owned
    // value, and the payload read below only runs after the check passed.
    let is_size = unsafe {
        let ax = &*(value.as_ptr() as *const AXValue);
        ax.r#type() == AXValueType::CGSize
    };
    if !is_size {
        return None;
    }
    // SAFETY: `value` holds a CGSize (just checked).
    let ax = unsafe { &*(value.as_ptr() as *const AXValue) };
    let mut out = CGSize {
        width: 0.0,
        height: 0.0,
    };
    // SAFETY: as in `read_point`.
    if unsafe { ax.value(AXValueType::CGSize, NonNull::from(&mut out).cast()) } {
        Some((out.width, out.height))
    } else {
        None
    }
}

/// The `Platform::insert_text` implementation, wired up in `macos_impl.rs`.
pub fn insert_text(text: &str) -> InjectionOutcome {
    let (result, field_readable) = insert_detailed(text);
    match result {
        Insert::Exact => InjectionOutcome {
            route: InjectionRoute::DirectWrite,
            fallback_reason: None,
            field_readable,
        },
        // The app adjusted our text (smart quotes, autocorrect). The words are
        // in the field, so falling back to the clipboard here is what used to
        // paste the dictation twice. Report the fast path and say why.
        Insert::Normalized => InjectionOutcome {
            route: InjectionRoute::DirectWrite,
            fallback_reason: Some(
                "direct write confirmed; the app adjusted the text on the way in".into(),
            ),
            field_readable,
        },
        Insert::Refused => InjectionOutcome {
            route: InjectionRoute::ClipboardPaste,
            fallback_reason: Some(
                "accessibility direct write unavailable or unverified; using the clipboard".into(),
            ),
            field_readable,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this enum exists for. `Insert::Normalized` must be treated as
    /// success, because the alternative is pasting the same dictation twice.
    #[test]
    fn a_normalised_write_is_success_not_a_reason_to_paste_again() {
        assert!(Insert::Normalized.landed());
        assert!(Insert::Exact.landed());
        assert!(!Insert::Refused.landed());
        // Only Refused may reach the clipboard.
        assert!(!Insert::Refused.landed());
    }

    #[test]
    fn only_refused_falls_back_to_the_clipboard() {
        for (out, route) in [
            (Insert::Exact, InjectionRoute::DirectWrite),
            (Insert::Normalized, InjectionRoute::DirectWrite),
            (Insert::Refused, InjectionRoute::ClipboardPaste),
        ] {
            assert_eq!(
                route_for(out),
                route,
                "{out:?} must not produce a duplicate paste"
            );
        }
    }

    /// Mirrors the match in `insert_text` without needing a focused field.
    fn route_for(i: Insert) -> InjectionRoute {
        match i {
            Insert::Exact | Insert::Normalized => InjectionRoute::DirectWrite,
            Insert::Refused => InjectionRoute::ClipboardPaste,
        }
    }

    #[test]
    fn read_only_roles_are_refused() {
        // Writing into a button or a label would overwrite UI the user never
        // asked to change, so those roles must not be treated as writable.
        for name in [ROLE_BUTTON, ROLE_STATIC_TEXT, "AXUnknownRole", ""] {
            assert!(!is_writable_role(name), "{name} must not be writable");
        }
    }

    #[test]
    fn text_field_roles_are_writable() {
        // The legitimate text-input roles must stay writable; search/URL fields
        // were removed from this list in BUG-006 and are pinned in
        // `search_and_url_fields_are_not_writable` below.
        for name in [ROLE_TEXT_FIELD, ROLE_TEXT_AREA, ROLE_COMBO_BOX] {
            assert!(is_writable_role(name), "{name} must be writable");
        }
    }

    #[test]
    fn search_and_url_fields_are_not_writable() {
        // BUG-006: browsers expose their URL bars and search boxes as
        // AXSearchField / AXURLTextField, so dictation must never be injected
        // into them — it would navigate to the dictated text or drop it in a
        // search box the user was not typing in.
        for name in [ROLE_SEARCH_FIELD, ROLE_TEXT_URL_FIELD] {
            assert!(!is_writable_role(name), "{name} must not be writable");
        }
    }

    #[test]
    fn attribute_constants_match_the_documented_ax_names() {
        // These are written out rather than imported from generated bindings,
        // so a typo would silently break every AX call. Pin the exact strings
        // against the documented AX contract.
        assert_eq!(ATTR_VALUE, "AXValue");
        assert_eq!(ATTR_FOCUSED, "AXFocusedUIElement");
        assert_eq!(ATTR_ROLE, "AXRole");
        assert_eq!(ATTR_SELECTED_RANGE, "AXSelectedTextRange");
        assert_eq!(ROLE_TEXT_URL_FIELD, "AXURLTextField");
    }

    #[test]
    fn splicing_replaces_the_selection() {
        let units: Vec<u16> = "hello world".encode_utf16().collect();
        assert_eq!(splice_utf16(&units, 6, 5, "there"), "hello there");
    }

    #[test]
    fn splicing_inserts_at_the_caret_with_no_selection() {
        let units: Vec<u16> = "ac".encode_utf16().collect();
        assert_eq!(splice_utf16(&units, 1, 0, "b"), "abc");
    }

    #[test]
    fn splicing_clamps_a_caret_past_the_end() {
        // Defensive: a stale range must not panic or lose the text.
        let units: Vec<u16> = "abc".encode_utf16().collect();
        assert_eq!(splice_utf16(&units, 99, 0, "!"), "abc!");
    }

    #[test]
    fn splicing_preserves_astral_characters() {
        // A thumbs-up emoji is a surrogate pair. Splitting it would corrupt the
        // text, which is why offsets are handled in UTF-16 units.
        let units: Vec<u16> = "a\u{1f44d}b".encode_utf16().collect();
        assert_eq!(splice_utf16(&units, 1, 0, "!"), "a!\u{1f44d}b");
    }

    #[test]
    fn empty_text_is_a_no_op_success() {
        // Nothing to insert is not a failure: the caller must not fall back to
        // the clipboard for an empty string.
        assert_eq!(insert(""), Insert::Exact);
        assert!(insert("").landed());
    }

    #[test]
    fn no_focused_field_falls_back_rather_than_claiming_success() {
        // A test process has no focused text field, so this must report the
        // fallback instead of a direct write.
        let outcome = insert_text("hello");
        assert_eq!(outcome.route, InjectionRoute::ClipboardPaste);
        assert!(outcome.fallback_reason.is_some());
    }

    #[test]
    fn an_exact_readback_is_exact() {
        assert_eq!(classify_write("hi", 2, "!", "hi!", "hi!"), Insert::Exact);
    }

    #[test]
    fn text_at_the_target_offset_is_exact_despite_normalisation() {
        // The app uppercased the existing content on write, but our text
        // landed where targeted. That is success, not a reason to paste again.
        assert_eq!(classify_write("hi", 2, "!", "hi!", "HI!"), Insert::Exact);
    }

    #[test]
    fn an_unchanged_value_means_nothing_landed_so_fallback_is_safe() {
        // Byte-identical: the set was silently discarded. Only this case may
        // reach the clipboard, because only here is there nothing to double.
        let out = classify_write("hi", 2, "!", "hi!", "hi");
        assert_eq!(out, Insert::Refused);
        assert!(!out.landed());
    }

    #[test]
    fn a_changed_but_unexpected_value_is_landed_not_a_reason_to_paste() {
        // Smart quotes: the app rewrote the apostrophe itself, so the slice
        // check misses but the value changed. The words are in the field, so
        // falling back would paste them a second time.
        let out = classify_write("say ", 4, "don't", "say don't", "say don’t!");
        assert_eq!(out, Insert::Normalized);
        assert!(out.landed());
    }

    /// A fake app with Terminal.app's semantics: setting the value *appends*
    /// the string to the display while the reported value parrots what was
    /// set. The read-back then matches whatever was sent, which is how the
    /// old whole-value strategy was verified as "ok" while duplicating.
    struct AppendParrotApp {
        display: String,
        value: String,
    }

    impl AppendParrotApp {
        fn set_value(&mut self, s: &str) {
            self.display.push_str(s);
            self.value = s.to_string();
        }
        fn set_selected_text(&mut self, s: &str) {
            // A true insert primitive: only the new text touches the display.
            self.display.push_str(s);
            self.value = s.to_string();
        }
    }

    /// The reported bug, reproduced without AX: three takes into a terminal,
    /// driven the old way (send whole value) and the new way (send only the
    /// new text). The old way must produce the user's pasted shape
    /// `t1 | t1t2 | t1t2t3`; the new way must produce `t1 | t2 | t3`.
    #[test]
    fn whole_value_resend_duplicates_in_a_terminal_and_selection_does_not() {
        let takes = [
            "Yeah, the chat is being repeated. See this bug right now.",
            "It starts with this one: one, two, three.",
            "See, now the chat started to repeat.",
        ];

        // Old strategy: read value, splice at end, write the whole string.
        let mut old_app = AppendParrotApp {
            display: String::new(),
            value: String::new(),
        };
        for take in takes {
            let units: Vec<u16> = old_app.value.encode_utf16().collect();
            let next = splice_utf16(&units, units.len(), 0, take);
            old_app.set_value(&next);
            // The read-back matched every time: this is why the duplication
            // was logged "direct write ok" instead of falling back.
            assert_eq!(old_app.value, next);
        }
        assert_eq!(
            old_app.display,
            format!("{0}{0}{1}{0}{1}{2}", takes[0], takes[1], takes[2]),
            "the old strategy re-sends every previous take on every take"
        );

        // New strategy: send only the new text per take.
        let mut new_app = AppendParrotApp {
            display: String::new(),
            value: String::new(),
        };
        let mut outcomes = Vec::new();
        for take in takes {
            let old = new_app.value.clone();
            let units: Vec<u16> = old.encode_utf16().collect();
            let expected = splice_utf16(&units, units.len(), 0, take);
            new_app.set_selected_text(take);
            let back = new_app.value.clone();
            let out = classify_write(&old, units.len(), take, &expected, &back);
            assert!(out.landed(), "every take must land without falling back");
            outcomes.push(out);
        }
        assert_eq!(new_app.display, takes.concat());
        // Take 1 reads back exact; takes 2+ report the parroted value, which
        // is landed-but-unexpected: success, never a second paste.
        assert_eq!(outcomes[0], Insert::Exact);
        assert_eq!(outcomes[1], Insert::Normalized);
        assert_eq!(outcomes[2], Insert::Normalized);
    }
}
