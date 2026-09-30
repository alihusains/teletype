//! Paste-target delivery gate: paste back into the window the user dictated
//! into, not wherever focus happens to be when the (slow) pipeline finishes.
//!
//! A take starts with a hotkey and ends 50 ms to 20 s later, after ASR plus an
//! optional LLM polish. In that gap the user can switch apps or windows —
//! deliberately or by fumbling the hotkey — and an ungated inject then types
//! private dictation into the wrong place. The reference product fixed exactly
//! this for sleeping Chromium hosts: record the target at record start, raise
//! and verify it at delivery time, and refuse the paste on a proven mismatch.
//!
//! The rule is deliberately asymmetric, matching the reference:
//!
//! - **Proven switch, abort.** The recorded and current targets are both
//!   readable and differ. Pasting there would be wrong with certainty.
//! - **Anything unreadable, proceed.** An app whose accessibility sleeps
//!   (Chromium before any client wakes it) reports no focused element and no
//!   window frame. Refusing on no evidence would break every such app, so
//!   missing evidence is not a mismatch.
//!
//! Aborting never loses text: history and the transcript file are written
//! before injection runs, so the gate only decides *where* the text goes, and
//! "nowhere" leaves it recoverable.

/// The paste target as captured when the take started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordTarget {
    /// Bundle id (macOS) or process name (Windows). Empty when unknown.
    pub bundle_id: String,
    /// Human-readable name, for the abort notice only. Never matched on.
    pub app_name: String,
    /// OS process id, when the platform exposes it. Guards against pid reuse
    /// only in combination with `bundle_id`; never matched alone.
    pub pid: Option<u32>,
    /// Focused-window frame (x, y, w, h) when readable. Catches the
    /// same-app-different-window switch no app check can see.
    pub frame: Option<[i64; 4]>,
}

/// What the gate decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Paste. Either the target matches, or there is no evidence it changed.
    Proceed,
    /// Do not paste. The user is provably somewhere else now.
    AbortSwitch,
}

/// The pure decision. `current_bundle` / `current_pid` / `current_frame` are
/// read at delivery time; `None`/empty means "unreadable", which passes.
pub fn resolve_delivery(
    recorded: &RecordTarget,
    current_bundle: &str,
    current_pid: Option<u32>,
    current_frame: Option<[i64; 4]>,
) -> Delivery {
    // No recorded identity, or nothing readable now: no evidence of a switch.
    if recorded.bundle_id.is_empty() || current_bundle.is_empty() {
        return Delivery::Proceed;
    }
    if !bundles_match(&recorded.bundle_id, current_bundle) {
        return Delivery::AbortSwitch;
    }
    // Same app, but both frames readable and different: the user moved to
    // another window of the same app. We cannot reliably raise the old one
    // (no retained element), so refusing is the only safe answer.
    match (recorded.frame, current_frame) {
        (Some(a), Some(b)) if a != b => Delivery::AbortSwitch,
        _ => {
            // A pid change under a matching bundle means the app relaunched
            // mid-take. Its windows are gone, but so is any evidence about
            // where the user is; the frame check above already passed or was
            // unavailable, so proceed rather than strand the text.
            let _ = (recorded.pid, current_pid);
            Delivery::Proceed
        }
    }
}

/// Bundle-id comparison. Case-insensitive: the OS is not consistent about
/// `com.foo.Bar` vs `com.foo.bar` across the APIs we read.
fn bundles_match(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Whether a multi-line dictation into a terminal must be refused.
///
/// A newline in a terminal submits the line: pasting "deploy this\nrm -rf /"
/// runs two commands. The reference refuses terminal payloads containing a
/// newline for exactly this reason. Single-line text is unaffected, and
/// nothing outside a terminal is affected. Refused text stays in history (and
/// on the clipboard when retention applies), so nothing is lost; the user
/// pastes it deliberately if that is what they meant.
///
/// Pure, so the boundary is pinnable: any newline anywhere refuses, including
/// a trailing one (a trailing newline submits an empty command after the
/// text, which still submits).
pub fn refuse_newline_in_terminal(app_type: crate::context::AppType, text: &str) -> bool {
    matches!(app_type, crate::context::AppType::Terminal) && text.contains('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> RecordTarget {
        RecordTarget {
            bundle_id: "com.apple.TextEdit".into(),
            app_name: "TextEdit".into(),
            pid: Some(1234),
            frame: Some([0, 0, 800, 600]),
        }
    }

    #[test]
    fn same_app_same_window_proceeds() {
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(1234),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn different_app_aborts() {
        // The reference case: dictating into Chrome, pasting into Slack.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.slackmac.Slack",
                Some(5678),
                Some([0, 0, 800, 600])
            ),
            Delivery::AbortSwitch
        );
    }

    #[test]
    fn same_app_different_window_aborts() {
        // Same app, another window: the app check passes, the frame catches it.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(1234),
                Some([100, 100, 800, 600])
            ),
            Delivery::AbortSwitch
        );
    }

    #[test]
    fn unreadable_now_proceeds() {
        // A sleeping host reports nothing. That is no evidence of a switch.
        assert_eq!(
            resolve_delivery(&target(), "", None, None),
            Delivery::Proceed
        );
    }

    #[test]
    fn unrecorded_target_proceeds() {
        // Nothing captured at record start (permissions, race): no baseline,
        // so there is nothing to mismatch against.
        let mut t = target();
        t.bundle_id.clear();
        t.frame = None;
        assert_eq!(
            resolve_delivery(&t, "com.slackmac.Slack", Some(5678), Some([1, 2, 3, 4])),
            Delivery::Proceed
        );
    }

    #[test]
    fn bundle_match_ignores_case() {
        assert_eq!(
            resolve_delivery(
                &target(),
                "COM.APPLE.TEXTEDIT",
                Some(1234),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn relaunched_app_with_same_frame_proceeds() {
        // Same bundle and frame, new pid: the app relaunched into an
        // indistinguishable state. Stranding the text helps nobody.
        assert_eq!(
            resolve_delivery(
                &target(),
                "com.apple.TextEdit",
                Some(9999),
                Some([0, 0, 800, 600])
            ),
            Delivery::Proceed
        );
    }

    #[test]
    fn missing_frame_on_one_side_proceeds_on_app_match() {
        assert_eq!(
            resolve_delivery(&target(), "com.apple.TextEdit", Some(1234), None),
            Delivery::Proceed
        );
    }

    #[test]
    fn multiline_text_into_a_terminal_refuses() {
        use crate::context::AppType;
        assert!(refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this\nrm -rf /"
        ));
        // A trailing newline still submits (an empty command after the text).
        assert!(refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this\n"
        ));
        // Single-line terminal dictation is the normal case and proceeds.
        assert!(!refuse_newline_in_terminal(
            AppType::Terminal,
            "deploy this"
        ));
        // Multi-line anywhere else (an editor, a chat box) is fine.
        assert!(!refuse_newline_in_terminal(
            AppType::Document,
            "line one\nline two"
        ));
        assert!(!refuse_newline_in_terminal(
            AppType::Unknown,
            "line one\nline two"
        ));
        assert!(!refuse_newline_in_terminal(AppType::Terminal, ""));
    }
}
