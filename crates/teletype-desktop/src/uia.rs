//! P8.5: Windows accessibility readers (UIA).
//!
//! The macOS equivalents live in `ax_text.rs` (AX reads of the focused
//! field) and `text_selection.rs` (selection read/replace). Windows has no
//! AX tree; the UI Automation (UIA) COM API is the equivalent. This module
//! is the thin, best-effort layer over it:
//!
//! - [`focused_control_text`] — read the focused control's text/value.
//! - [`focused_surroundings`] — the text before/after the caret, for smart
//!   insertion (the Windows counterpart of `ax_text::focused_surroundings`).
//! - [`focused_selection`] — the currently selected text.
//! - [`replace_selection`] — replace the selection (or, when nothing is
//!   selected, insert at the caret).
//!
//! All functions return `Result<_, String>` (or `Option`) and fail soft: a
//! control with no TextPattern/ValuePattern (a button, a non-text app, a
//! secure field) yields `Err`/`None`, never a panic. The callers treat that
//! as "no context to read", exactly like the macOS path does for AX
//! failures.
//!
//! The UIA client object is created once per process: creating
//! `CUIAutomation` on every call is the classic UIA performance trap.

use std::sync::LazyLock;

use windows::{
    core::{Interface, PCWSTR},
    Win32::{
        System::Com::CoCreateInstance,
        UI::{
            Automation::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationValuePattern, TextPatternRangeEndpoint,
                UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
                UIA_IsPasswordPropertyId, UIA_NamePropertyId, UIA_TextControlTypeId,
            },
            WindowsAndMessaging::GetForegroundWindow,
        },
    },
};

/// The UIA client, created once. A COM failure degrades to "no text
/// context" (the readers return `None`/`Err`), never an app failure.
static AUTOMATION: LazyLock<Result<IUIAutomation, windows::core::Error>> = LazyLock::new(|| {
    // SAFETY: CoCreateInstance with the well-known CUIAutomation class id
    // and interface pointer; the standard COM creation call.
    unsafe { CoCreateInstance(&CUIAutomation, None, windows::core::COM_INTERFACE_BASE) }
});

/// The focused control of the foreground window, if it exposes a usable
/// element. `None` when there is no foreground window or its element
/// cannot be fetched.
fn focused_element() -> Option<IUIAutomationElement> {
    let automation = AUTOMATION.as_ref().ok()?;
    // SAFETY: no parameters; returns the foreground HWND (NULL when the
    // calling thread has no foreground window).
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let element = automation
        .ElementFromHandle(hwnd)
        .ok()
        .and_then(|root| root.GetFocusedElement().ok())?;
    Some(element)
}

/// True when the element is a password/secure field. Such fields must
/// never be read (plan §7.2: "never read AXSecureTextField / UIA
/// IsPassword").
fn is_password(element: &IUIAutomationElement) -> bool {
    element
        .GetCurrentPropertyValue(UIA_IsPasswordPropertyId)
        .map(|v| v.as_bool().unwrap_or_default())
        .unwrap_or_default()
}

/// True when the element looks like a text entry/box (Edit, Document, or
/// Text control type). The check keeps the readers from returning the
/// *window's* name when the real caret is in an embedded control that
/// reports a generic type.
fn is_text_control(element: &IUIAutomationElement) -> bool {
    let control_type = element
        .GetCurrentPropertyValue(UIA_ControlTypePropertyId)
        .map(|v| v.as_i32().unwrap_or_default())
        .unwrap_or_default();
    matches!(
        control_type,
        UIA_EditControlTypeId | UIA_DocumentControlTypeId | UIA_TextControlTypeId
    )
}

/// The element's Name property (its label), for diagnostics.
fn element_name(element: &IUIAutomationElement) -> String {
    element
        .GetCurrentPropertyValue(UIA_NamePropertyId)
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

/// Gets the focused control's TextPattern, if it has one.
fn focused_text_pattern() -> Result<IUIAutomationTextPattern, String> {
    let element = focused_element().ok_or("no focused control")?;
    if is_password(&element) {
        return Err("focused control is a password field and is never read".into());
    }
    let pattern = element
        .GetCurrentPattern(windows::Win32::UI::Automation::UIA_TextPatternId)
        .map_err(|e| format!("focused control has no TextPattern: {e}"))?;
    pattern
        .into_interface::<IUIAutomationTextPattern>()
        .map_err(|e| format!("TextPattern interface: {e}"))
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
            "focused control is not a text control (name {element_name:?})"
        ));
    }
    // TextPattern: full document text (rich edit, code editors).
    if let Ok(pattern) = element.GetCurrentPattern(
        windows::Win32::UI::Automation::UIA_TextPatternId,
    ) {
        if let Ok(text_pattern) = pattern.into_interface::<IUIAutomationTextPattern>() {
            if let Ok(range) = text_pattern.DocumentRange() {
                if let Ok(text) = range.GetText(i32::MAX) {
                    let text = text.to_string();
                    if !text.is_empty() {
                        return Ok(text);
                    }
                }
            }
        }
    }
    // ValuePattern: the plain value of an edit control.
    if let Ok(pattern) = element.GetCurrentPattern(
        windows::Win32::UI::Automation::UIA_ValuePatternId,
    ) {
        if let Ok(value_pattern) = pattern.into_interface::<IUIAutomationValuePattern>() {
            if let Ok(value) = value_pattern.GetCurrentValue() {
                let value = value.to_string();
                if !value.is_empty() {
                    return Ok(value);
                }
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
    let pattern = focused_text_pattern().ok()?;
    let mut range = pattern
        .GetCaretRange(0)
        .ok()
        .and_then(|r| r.ok())?;
    // Grow the range to the document start, then keep only the last
    // `max_chars` characters: smart spacing needs the caret's *immediate*
    // left context, not the whole field.
    let start = pattern.DocumentRange().ok()?;
    let _ = range.MoveEndpointRange(
        TextPatternRangeEndpoint_Start,
        &start,
        TextPatternRangeEndpoint_Start,
    );
    let full = range.GetText(i32::MAX).ok()?.to_string();
    let chars: Vec<char> = full.chars().collect();
    let from = chars.len().saturating_sub(max_chars);
    Some(chars[from..].iter().collect())
}

/// (ii) The selected text, or `Ok(None)` when the focused control has no
/// selection. `Err` when a read fails mid-way. The selection is the
/// first of the TextPattern's selection ranges (a control can have
/// several).
pub fn focused_selection() -> Result<Option<String>, String> {
    let pattern = focused_text_pattern()?;
    let ranges = pattern
        .GetSelection()
        .map_err(|e| format!("GetSelection: {e}"))?;
    let Some(first) = ranges.GetElement(0).ok().flatten() else {
        return Ok(None);
    };
    let text = first
        .GetText(i32::MAX)
        .map_err(|e| format!("selection GetText: {e}"))?
        .to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// (ii) Replace the focused control's selection (or insert at the caret
/// when nothing is selected) with `replacement`.
///
/// Uses the TextPattern's `ReplaceSelection` when the control supports it
/// (most edit controls do); falls back to `Range::SetText` on the
/// selection range. Returns the number of characters replaced, so a caller
/// can tell "inserted at caret" (0) from "replaced a selection" (n > 0).
pub fn replace_selection(replacement: &str) -> Result<usize, String> {
    let pattern = focused_text_pattern()?;

    // ReplaceSelection: the clean path. It replaces the selection (or
    // inserts at the caret when empty) and moves the caret after the new
    // text.
    let replacement_wide: Vec<u16> = replacement
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let replaced = pattern
        .ReplaceSelection(PCWSTR::from_raw(replacement_wide.as_ptr()))
        .map_err(|e| format!("ReplaceSelection: {e}"))?;
    if replaced {
        return Ok(0);
    }

    // Fallback: set the text on the selection range (or the caret range
    // when there is no selection).
    let range = pattern
        .GetSelection()
        .map_err(|e| format!("GetSelection: {e}"))?
        .GetElement(0)
        .ok()
        .flatten()
        .or_else(|| pattern.GetCaretRange(0).ok().flatten())
        .ok_or("no selection or caret range")?;
    let old = range
        .GetText(i32::MAX)
        .map_err(|e| format!("range GetText: {e}"))?
        .to_string();
    range
        .SetText(replacement)
        .map_err(|e| format!("range SetText: {e}"))?;
    Ok(old.chars().count())
}

/// The text before/after the caret of the focused control, for smart
/// insertion. The Windows counterpart of `ax_text::focused_surroundings`.
///
/// `None` when the focused control has no TextPattern, is a password field,
/// or the caret range cannot be read. The caller (the dictation
/// controller's `focused_surroundings`) treats `None` as "insert the
/// dictated text unchanged", the same fallback the macOS path uses when
/// the AX read fails.
pub fn focused_surroundings() -> Option<(String, String)> {
    let pattern = focused_text_pattern().ok()?;
    let mut before_range = pattern
        .GetCaretRange(0)
        .ok()
        .and_then(|r| r.ok())?;
    let document = pattern.DocumentRange().ok()?;

    // before = document start .. caret start.
    let _ = before_range.MoveEndpointRange(
        TextPatternRangeEndpoint_Start,
        &document,
        TextPatternRangeEndpoint_Start,
    );
    let before = before_range.GetText(i32::MAX).ok()?.to_string();

    // after = caret end .. document end. (The caret range is zero-length,
    // so start and end are the same point.)
    let mut after_range = pattern
        .GetCaretRange(0)
        .ok()
        .and_then(|r| r.ok())?;
    let _ = after_range.MoveEndpointRange(
        TextPatternRangeEndpoint_End,
        &document,
        TextPatternRangeEndpoint_End,
    );
    let after = after_range.GetText(i32::MAX).ok()?.to_string();

    Some((before, after))
}
