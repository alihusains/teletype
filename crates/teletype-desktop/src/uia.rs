//! P8.5: Windows accessibility readers (UIA).
//!
//! The macOS equivalents live in `ax_text.rs` (AX reads of the focused
//! field) and `text_selection.rs` (selection read/replace). Windows has no
//! AX tree; the UI Automation (UIA) COM API is the equivalent. This module
//! is the thin, best-effort layer over it:
//!
//! - [`focused_control_text`] — read the focused control's text/value.
//! - [`text_before_caret`] — the text before the caret, for smart spacing.
//! - [`focused_selection`] — the currently selected text.
//! - [`replace_selection`] — write text via the ValuePattern.
//! - [`focused_surroundings`] — the text before/after the caret, for smart
//!   insertion (the Windows counterpart of `ax_text::focused_surroundings`).
//!
//! All functions return `Result<_, String>` (or `Option`) and fail soft: a
//! control with no TextPattern/ValuePattern (a button, a non-text app, a
//! secure field) yields `Err`/`None`, never a panic. The callers treat that
//! as "no context to read", exactly like the macOS path does for AX
//! failures.
//!
//! windows-rs 0.61.3 notes (verified against the crate source, 2026-07-17):
//! - The UIA COM API moved to `Win32::UI::Accessibility` (feature
//!   `Win32_UI_Accessibility`).
//! - `GetCurrentPattern` returns a bare `IUnknown` in 0.61; the typed
//!   `GetCurrentPatternAs::<T>()` helper is used instead.
//! - `GetCaretRange` moved from `IUIAutomationTextPattern` to
//!   `IUIAutomationTextPattern2` and now takes an out `isactive` param.
//! - `TextRange::MoveEndpointRange` was renamed `MoveEndpointByRange`.
//! - The UIA client has **no** text-replace method on `TextRange`; the only
//!   client-side write is `IUIAutomationValuePattern::SetValue`. (The old
//!   `ReplaceSelection`/`TextRange::SetText` client calls and the MSAA
//!   `put_accValue` write are not part of the 0.61 client surface.)
//! - 0.61 does not mark COM interface types `Send`/`Sync`, so the
//!   `CUIAutomation` client is created per call rather than in a `static`.
//!   `CoCreateInstance` for this CLSID is a cheap in-process singleton
//!   lookup — the per-call cost is negligible compared to the UIA queries
//!   it enables.

use windows::{
    core::{IUnknown, BOOL, BSTR},
    Win32::{
        System::{
            Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER},
            Variant::{VariantToBoolean, VariantToInt32, VariantToStringAlloc, VARIANT},
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationTextPattern2, IUIAutomationValuePattern, TextPatternRangeEndpoint_End,
                TextPatternRangeEndpoint_Start, UIA_ControlTypePropertyId,
                UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_IsPasswordPropertyId,
                UIA_NamePropertyId, UIA_TextControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
            },
            WindowsAndMessaging::GetForegroundWindow,
        },
    },
};

/// The focused control of the foreground window, if it exposes a usable
/// element. `None` when there is no foreground window or its element cannot
/// be fetched.
fn focused_element() -> Option<IUIAutomationElement> {
    let automation = automation().ok()?;
    // SAFETY: no parameters; returns the foreground HWND (NULL when the
    // calling thread has no foreground window).
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    // SAFETY: hwnd is the live foreground window handle; standard UIA
    // client query on a local COM object.
    Some(unsafe { automation.GetFocusedElement() }.ok()?)
}

fn automation() -> Result<IUIAutomation, String> {
    // SAFETY: CoCreateInstance with the well-known CUIAutomation class id
    // and a null outer unknown; standard COM creation call. The COM
    // apartment is initialized on the app's main thread.
    unsafe { CoCreateInstance(&CUIAutomation, None::<&IUnknown>, CLSCTX_INPROC_SERVER) }
        .map_err(|e| format!("CUIAutomation CoCreateInstance: {e}"))
}

/// True when the element is a password/secure field. Such fields must
/// never be read (plan §7.2: "never read AXSecureTextField / UIA
/// IsPassword").
fn is_password(element: &IUIAutomationElement) -> bool {
    // SAFETY: read-only UIA property query on a live client object.
    let variant = match unsafe { element.GetCurrentPropertyValue(UIA_IsPasswordPropertyId) } {
        Ok(v) => v,
        Err(_) => return false,
    };
    // SAFETY: the variant came from the query above; read-only extraction.
    unsafe { VariantToBoolean(&variant) }
        .map(|b| b.as_bool())
        .unwrap_or_default()
}

/// True when the element looks like a text entry/box (Edit, Document, or
/// Text control type). The check keeps the readers from returning the
/// *window's* name when the real caret is in an embedded control that
/// reports a generic type.
fn is_text_control(element: &IUIAutomationElement) -> bool {
    // SAFETY: read-only UIA property query.
    let control_type = match unsafe { element.GetCurrentPropertyValue(UIA_ControlTypePropertyId) } {
        Ok(v) => v,
        Err(_) => return false,
    };
    // SAFETY: read-only variant extraction of the control type id.
    let control_type = match unsafe { VariantToInt32(&control_type) } {
        Ok(id) => id,
        Err(_) => return false,
    };
    control_type == UIA_EditControlTypeId.0
        || control_type == UIA_DocumentControlTypeId.0
        || control_type == UIA_TextControlTypeId.0
}

/// The element's Name property (its label), for diagnostics.
fn element_name(element: &IUIAutomationElement) -> String {
    // SAFETY: read-only UIA property query.
    match unsafe { element.GetCurrentPropertyValue(UIA_NamePropertyId) } {
        Ok(v) => variant_to_string(&v),
        Err(_) => String::new(),
    }
}

/// A `VARIANT` string (VT_BSTR) as an owned `String`.
/// `VariantToStringAlloc` allocates its result with `CoTaskMemAlloc` per
/// the propsys contract, so it is freed with `CoTaskMemFree`. Returns an
/// empty string on any failure (including a non-string variant).
fn variant_to_string(variant: &VARIANT) -> String {
    // SAFETY: the variant came from a UIA property query; the allocator
    // matches the documented free function.
    unsafe {
        let p = match VariantToStringAlloc(variant) {
            Ok(p) if !p.0.is_null() => p.0,
            _ => return String::new(),
        };
        let mut len = 0usize;
        while len < (1 << 20) && *p.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(core::slice::from_raw_parts(p, len));
        CoTaskMemFree(Some(p as *const _));
        text
    }
}

/// The owned wide string out of a `BSTR` (copies, dropping the BSTR once).
fn bstr_to_string(bstr: &BSTR) -> String {
    // The BSTR is UTF-16 code units exactly the length it reports, so a
    // direct lossless UTF-16 -> UTF-8 conversion is safe.
    String::from_utf16_lossy(bstr)
}

/// The focused control's TextPattern (with its TextPattern2 pair, since the
/// caret range lives on TextPattern2 in the UIA spec). `Err` when there is
/// no focused control, it is a password field, or it has no TextPattern.
fn focused_text_patterns() -> Result<(IUIAutomationTextPattern, IUIAutomationTextPattern2), String>
{
    let element = focused_element().ok_or("no focused control")?;
    if is_password(&element) {
        return Err("focused control is a password field and is never read".into());
    }
    // SAFETY: read-only pattern queries on a live client object.
    let text =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
            .map_err(|e| format!("focused control has no TextPattern: {e}"))?;
    let text2 =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPatternId) }
            .map_err(|e| format!("focused control has no TextPattern2: {e}"))?;
    Ok((text, text2))
}

/// (i) Read the currently focused control's text/value.
///
/// Tries the TextPattern's full document text first (rich text fields),
/// then the ValuePattern's value (plain edit controls), then falls back to
/// the element's Name. Returns `Err` when the control is a password field
/// (never read) or exposes none of the three.
pub fn focused_control_text() -> Result<String, String> {
    let element = focused_element().ok_or("no focused control")?;
    if is_password(&element) {
        return Err("focused control is a password field and is never read".into());
    }
    if !is_text_control(&element) {
        return Err(format!(
            "focused control is not a text control (name {})",
            element_name(&element)
        ));
    }
    // TextPattern: full document text (rich edit, code editors).
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
    {
        // SAFETY: live UIA client object, read-only range queries.
        if let Ok(range) = unsafe { pattern.DocumentRange() } {
            if let Ok(text) = unsafe { range.GetText(i32::MAX) } {
                let text = bstr_to_string(&text);
                if !text.is_empty() {
                    return Ok(text);
                }
            }
        }
    }
    // ValuePattern: the plain value of an edit control.
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        // SAFETY: live UIA client object, read-only value query.
        if let Ok(value) = unsafe { pattern.CurrentValue() } {
            let value = bstr_to_string(&value);
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    // Name fallback: some controls still carry their content in the Name
    // property.
    let name = element_name(&element);
    if name.is_empty() {
        Err("focused control exposes no readable text".into())
    } else {
        Ok(name)
    }
}

/// (iii) The text immediately before the caret, for smart spacing.
///
/// `None` when the focused control has no TextPattern (the caret position
/// is only observable through the TextPattern's caret range), is a password
/// field, or the caret range cannot be read. An empty `String` is a valid
/// answer: the caret is at the start of the field.
pub fn text_before_caret(max_chars: usize) -> Option<String> {
    let (pattern, pattern2) = focused_text_patterns().ok()?;
    let mut is_active = BOOL(0);
    // SAFETY: live UIA client objects, read-only caret/range queries.
    let range = unsafe { pattern2.GetCaretRange(&mut is_active) }.ok()?;
    let start = unsafe { pattern.DocumentRange() }.ok()?;
    // Grow the range to the document start, then keep only the last
    // `max_chars` characters: smart spacing needs the caret's *immediate*
    // left context, not the whole field.
    let _ = unsafe {
        range.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &start,
            TextPatternRangeEndpoint_Start,
        )
    };
    let full = unsafe { range.GetText(i32::MAX) }.ok()?.to_string();
    let chars: Vec<char> = full.chars().collect();
    let from = chars.len().saturating_sub(max_chars);
    Some(chars[from..].iter().collect())
}

/// (ii) The selected text, or `Ok(None)` when the focused control has no
/// selection. `Err` when a read fails mid-way. The selection is the first
/// of the TextPattern's selection ranges (a control can have several).
pub fn focused_selection() -> Result<Option<String>, String> {
    let (pattern, _) = focused_text_patterns()?;
    // SAFETY: live UIA client object, read-only selection queries.
    let ranges = unsafe { pattern.GetSelection() }.map_err(|e| format!("GetSelection: {e}"))?;
    let Some(first) = unsafe { ranges.GetElement(0) }.ok() else {
        return Ok(None);
    };
    let text = unsafe { first.GetText(i32::MAX) }
        .map_err(|e| format!("selection GetText: {e}"))?
        .to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// (ii) Write `replacement` into the focused control via the UIA
/// ValuePattern, and report how many characters it replaced.
///
/// The 0.61 UIA *client* surface has no `TextRange` text-replacement
/// method (`TextPattern::ReplaceSelection` and `TextRange::SetText` are
/// provider-side only, and MSAA `put_accValue` is not in the bindings), so
/// the write goes out as `IUIAutomationValuePattern::SetValue` — the same
/// primitive UIA clients (screen readers, automation tools) use to type
/// into a control. For a standard edit control this replaces the current
/// selection (or the whole value when nothing is selected); the returned
/// count is the length of the selection that was present before the write
/// (0 = plain insertion). `Err` when the focused control has no
/// ValuePattern or the provider refuses the write (e.g. read-only).
pub fn replace_selection(replacement: &str) -> Result<usize, String> {
    let element = focused_element().ok_or("no focused control")?;
    if is_password(&element) {
        return Err("focused control is a password field and is never written".into());
    }
    // The selected text about to be replaced (if any): its length is what
    // the caller reports as "replaced count" (0 = insertion at caret).
    let (pattern, _) = focused_text_patterns()?;
    // SAFETY: live UIA client object, read-only selection query.
    let replaced_len = unsafe { pattern.GetSelection() }
        .ok()
        .and_then(|ranges| unsafe { ranges.GetElement(0) }.ok())
        .and_then(|range| unsafe { range.GetText(i32::MAX) }.ok())
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
        .map_or(0, |t| t.chars().count());

    // SAFETY: read-only pattern query.
    let value_pattern: IUIAutomationValuePattern =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
            .map_err(|e| format!("focused control has no ValuePattern to write to: {e}"))?;
    // The UIA value write takes an owned BSTR, composed from the owned
    // replacement string, released by its Drop.
    let value = BSTR::from(replacement);
    // SAFETY: live UIA client object; the BSTR is valid for the call.
    unsafe { value_pattern.SetValue(&value) }.map_err(|e| format!("ValuePattern SetValue: {e}"))?;
    Ok(replaced_len)
}

/// The text before/after the caret of the focused control, for smart
/// insertion. The Windows counterpart of `ax_text::focused_surroundings`.
///
/// `None` when the focused control has no TextPattern, is a password field,
/// or the caret range cannot be read. The caller (the dictation
/// controller's `focused_surroundings`) treats `None` as "insert the
/// dictated text unchanged", the same fallback the macOS path uses when the
/// AX read fails.
pub fn focused_surroundings() -> Option<(String, String)> {
    let (pattern, pattern2) = focused_text_patterns().ok()?;
    let mut is_active = BOOL(0);
    // SAFETY: live UIA client objects, read-only caret/document queries.
    let before_range = unsafe { pattern2.GetCaretRange(&mut is_active) }.ok()?;
    let document = unsafe { pattern.DocumentRange() }.ok()?;

    // before = document start .. caret start.
    let _ = unsafe {
        before_range.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            &document,
            TextPatternRangeEndpoint_Start,
        )
    };
    let before = unsafe { before_range.GetText(i32::MAX) }.ok()?.to_string();

    // after = caret end .. document end. (The caret range is zero-length,
    // so start and end are the same point.)
    let after_range = unsafe { pattern2.GetCaretRange(&mut is_active) }.ok()?;
    let _ = unsafe {
        after_range.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            &document,
            TextPatternRangeEndpoint_End,
        )
    };
    let after = unsafe { after_range.GetText(i32::MAX) }.ok()?.to_string();

    Some((before, after))
}
