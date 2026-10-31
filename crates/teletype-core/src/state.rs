//! The dictation state machine.
//!
//! `decide` is a pure function of (input, phase, mode) so the whole voice
//! lifecycle is testable without a microphone. The desktop app drives it from
//! a single controller thread (see `teletype-desktop::dictation`).

use serde::{Deserialize, Serialize};

/// How the user controls recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RecordingMode {
    /// Record while the hotkey is held; transcribe on release. A double-tap
    /// (release then press again within the window) switches to hands-free so
    /// you can keep talking; the next tap stops and transcribes.
    #[default]
    Hold,
    /// Press once to start, press again to stop.
    Toggle,
}

/// User input into the dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    HotkeyDown,
    HotkeyUp,
    /// Another key was pressed while a single-modifier hotkey was held.
    HotkeyInterrupted,
    /// Start/stop from the UI or tray, independent of the hotkey.
    Toggle,
    /// Cancel: stop and discard (while recording) or drop the result (while busy).
    Cancel,
}

/// Phases of a dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Phase {
    #[default]
    Idle,
    Listening,
    Stopping,
    Transcribing,
    Transforming,
    Inserting,
    Completed,
    Cancelled,
    Error,
}

impl Phase {
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Phase::Stopping | Phase::Transcribing | Phase::Transforming | Phase::Inserting
        )
    }
}

/// What the session should do in response to an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Start {
        by_hotkey: bool,
    },
    /// Stop recording and run the pipeline. `cancelled` keeps the result from
    /// being inserted.
    Stop {
        cancelled: bool,
    },
    /// A double-tap in Hold mode: keep recording after the key is released
    /// (hands-free). The next hotkey press stops and transcribes.
    GoHandsFree,
    /// Stop recording and throw the audio away.
    Discard,
    /// Let the running pipeline finish in the background without inserting.
    CancelPipeline,
}

/// Decides what `input` does in `phase`. `hotkey_down` tracks whether the
/// hotkey is currently held, so key repeat is ignored.
pub fn decide(
    input: Input,
    phase: Phase,
    mode: RecordingMode,
    hotkey_down: &mut bool,
    hands_free: &mut bool,
) -> Action {
    match input {
        Input::HotkeyDown => {
            if mem_replace(hotkey_down, true) {
                return Action::Nothing;
            }
            match phase {
                Phase::Idle => Action::Start { by_hotkey: true },
                // Hands-free (Hold) or Toggle: a press stops the recording.
                Phase::Listening if mode == RecordingMode::Toggle || *hands_free => {
                    Action::Stop { cancelled: false }
                }
                _ => Action::Nothing,
            }
        }
        Input::HotkeyUp => {
            let was_down = mem_replace(hotkey_down, false);
            if !was_down || phase != Phase::Listening || mode != RecordingMode::Hold {
                return Action::Nothing;
            }
            if *hands_free {
                // Already hands-free: release does nothing (keep recording).
                Action::Nothing
            } else {
                // Normal hold release: stop + transcribe.
                Action::Stop { cancelled: false }
            }
        }
        Input::HotkeyInterrupted => {
            *hotkey_down = false;
            *hands_free = false;
            // The hotkey was part of another shortcut (e.g. ⌘C): a recording it
            // started wasn't a dictation.
            if phase == Phase::Listening && mode == RecordingMode::Hold {
                Action::Discard
            } else {
                Action::Nothing
            }
        }
        Input::Toggle => match phase {
            Phase::Idle => Action::Start { by_hotkey: false },
            Phase::Listening => Action::Stop { cancelled: false },
            _ => Action::Nothing,
        },
        Input::Cancel => {
            // Cancelling is the other way a take ends, so it releases the
            // hotkey and hands-free latches exactly like
            // `HotkeyInterrupted` does. Without this, a user who holds the
            // hotkey, cancels from the pill, and keeps holding is left in Idle
            // with the latch set: nothing they say is recorded and the app
            // looks broken until they release and press again.
            *hotkey_down = false;
            *hands_free = false;
            match phase {
                Phase::Listening => Action::Stop { cancelled: true },
                Phase::Stopping | Phase::Transcribing | Phase::Transforming | Phase::Inserting => {
                    Action::CancelPipeline
                }
                _ => Action::Nothing,
            }
        }
    }
}

fn mem_replace(value: &mut bool, new: bool) -> bool {
    std::mem::replace(value, new)
}

/// The state broadcast to the UI overlay.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum UiState {
    #[default]
    Idle,
    Listening {
        started_at_ms: u64,
    },
    Transcribing,
    Transforming,
    Inserting,
    /// A short status message before returning to idle.
    Message {
        text: String,
    },
}

/// Where the floating pill sits on the screen, as a 3×3 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PillPosition {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

/// The visual state of the floating pill window.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PillState {
    Idle,
    Recording {
        started_at_ms: u64,
        /// P2.8: true when the take in flight is a Command take. The pill
        /// renders a distinct badge so the user can tell "Transcribe my
        /// words" from "Run what I say" before speaking.
        command: bool,
    },
    Warming,
    Processing {
        message: String,
    },
}

/// The user-facing message for a transform skip/fallback, shown at
/// dictation time (pill, tray, toast).
///
/// `None` when nothing needs telling the user: a successful transform, or a
/// silent skip (too short, gate skip, intent skip, validator fallback, a
/// valid empty answer). Those still reach the trace and the log, so they
/// stay diagnosable (F-13).
///
/// `Some` is reserved for the actionable failures the user can do something
/// about: no model loaded, the model failed to load, and cloud inference
/// errors (auth, quota, network). The specific
/// [`crate::transforms::engine::SkipReason`] carries the detail; this is the
/// short, display-ready form.
pub fn skip_message(reason: &crate::transforms::engine::SkipReason) -> Option<String> {
    match reason {
        crate::transforms::engine::SkipReason::NoModelLoaded => {
            Some("Polish skipped: no model loaded. Using raw text".to_string())
        }
        crate::transforms::engine::SkipReason::ModelLoadFailed => {
            Some("Polish failed: model load failed. Using raw text".to_string())
        }
        crate::transforms::engine::SkipReason::InferenceError { detail } => {
            Some(format!("Polish failed ({detail}). Using raw text"))
        }
        // Silent: the take was clean as spoken, or the model's answer was
        // rejected in favour of the user's own words. Nothing to tell.
        crate::transforms::engine::SkipReason::ValidationRejected { .. } => None,
        crate::transforms::engine::SkipReason::TooShort => None,
        crate::transforms::engine::SkipReason::ContextOverflow => None,
        crate::transforms::engine::SkipReason::GateSkip { .. } => None,
        crate::transforms::engine::SkipReason::IntentSkip { .. } => None,
        crate::transforms::engine::SkipReason::ModelNotCapable { model, transform } => {
            Some(format!(
                "{model} can only clean transcripts; {transform} needs a general model. Using raw text"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transforms::engine::SkipReason;

    #[test]
    fn pill_position_defaults_to_bottom_center() {
        assert_eq!(PillPosition::default(), PillPosition::BottomCenter);
    }

    fn run(phase: Phase, mode: RecordingMode, inputs: &[Input]) -> Vec<Action> {
        let mut down = false;
        let mut hands_free = false;
        inputs
            .iter()
            .map(|&i| decide(i, phase, mode, &mut down, &mut hands_free))
            .collect()
    }

    #[test]
    fn hold_mode_records_while_held() {
        let mode = RecordingMode::Hold;
        let mut down = false;
        let mut hands_free = false;
        assert_eq!(
            decide(
                Input::HotkeyDown,
                Phase::Idle,
                mode,
                &mut down,
                &mut hands_free
            ),
            Action::Start { by_hotkey: true }
        );
        // Key repeat while held does nothing.
        assert_eq!(
            decide(
                Input::HotkeyDown,
                Phase::Listening,
                mode,
                &mut down,
                &mut hands_free
            ),
            Action::Nothing
        );
        assert_eq!(
            decide(
                Input::HotkeyUp,
                Phase::Listening,
                mode,
                &mut down,
                &mut hands_free
            ),
            Action::Stop { cancelled: false }
        );
        assert!(!down);
    }

    #[test]
    fn hold_mode_double_tap_goes_hands_free() {
        // Hold mode: a quick second press after release (within the window) is
        // handled by the controller, which sets hands_free before the next
        // HotkeyUp. Here we model the post-double-tap state: hands_free is set,
        // so a release does NOT stop, and the next press stops.
        let mode = RecordingMode::Hold;
        let mut down = false;
        let mut hands_free = true; // set by the controller on double-tap
                                   // Release while hands-free: keep recording.
        assert_eq!(
            decide(
                Input::HotkeyUp,
                Phase::Listening,
                mode,
                &mut down,
                &mut hands_free
            ),
            Action::Nothing
        );
        // Next press stops and transcribes.
        assert_eq!(
            decide(
                Input::HotkeyDown,
                Phase::Listening,
                mode,
                &mut down,
                &mut hands_free
            ),
            Action::Stop { cancelled: false }
        );
    }

    #[test]
    fn toggle_mode_stops_on_next_press() {
        let mode = RecordingMode::Toggle;
        assert_eq!(
            run(
                Phase::Listening,
                mode,
                &[Input::HotkeyDown, Input::HotkeyUp]
            ),
            [Action::Stop { cancelled: false }, Action::Nothing]
        );
    }

    #[test]
    fn release_without_press_is_ignored() {
        assert_eq!(
            run(Phase::Listening, RecordingMode::Hold, &[Input::HotkeyUp]),
            [Action::Nothing]
        );
    }

    #[test]
    fn interrupt_discards_hotkey_recording() {
        let inputs = [Input::HotkeyDown, Input::HotkeyInterrupted, Input::HotkeyUp];
        assert_eq!(
            run(Phase::Listening, RecordingMode::Hold, &inputs),
            [Action::Nothing, Action::Discard, Action::Nothing]
        );
        // In toggle mode the interrupt must not discard.
        assert_eq!(
            run(
                Phase::Listening,
                RecordingMode::Toggle,
                &[Input::HotkeyInterrupted]
            ),
            [Action::Nothing]
        );
    }

    #[test]
    fn hotkey_does_nothing_while_busy() {
        for phase in [
            Phase::Stopping,
            Phase::Transcribing,
            Phase::Transforming,
            Phase::Inserting,
        ] {
            assert_eq!(
                run(
                    phase,
                    RecordingMode::Hold,
                    &[Input::HotkeyDown, Input::HotkeyUp]
                ),
                [Action::Nothing, Action::Nothing]
            );
        }
    }

    #[test]
    fn cancel_behaviour_per_phase() {
        assert_eq!(
            decide(
                Input::Cancel,
                Phase::Listening,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::Stop { cancelled: true }
        );
        assert_eq!(
            decide(
                Input::Cancel,
                Phase::Transcribing,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::CancelPipeline
        );
        assert_eq!(
            decide(
                Input::Cancel,
                Phase::Idle,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::Nothing
        );
    }

    #[test]
    fn skip_message_covers_every_reason() {
        assert_eq!(
            skip_message(&SkipReason::NoModelLoaded).as_deref(),
            Some("Polish skipped: no model loaded. Using raw text")
        );
        assert_eq!(
            skip_message(&SkipReason::ModelLoadFailed).as_deref(),
            Some("Polish failed: model load failed. Using raw text")
        );
        assert_eq!(
            skip_message(&SkipReason::InferenceError {
                detail: "timeout".into()
            })
            .as_deref(),
            Some("Polish failed (timeout). Using raw text")
        );
        assert_eq!(
            skip_message(&SkipReason::ValidationRejected {
                kind: "empty".into()
            }),
            None
        );
        assert_eq!(skip_message(&SkipReason::TooShort), None);
        assert_eq!(skip_message(&SkipReason::ContextOverflow), None);
    }

    #[test]
    fn actionable_skip_reasons_still_show_a_message() {
        // F-13: these are the failures the user can act on (load a model,
        // fix auth or quota, check the network), so they keep their notice.
        for reason in [
            SkipReason::NoModelLoaded,
            SkipReason::ModelLoadFailed,
            SkipReason::InferenceError {
                detail: "cloud endpoint unreachable".into(),
            },
            SkipReason::InferenceError {
                detail: "401: invalid api key".into(),
            },
            SkipReason::InferenceError {
                detail: "402: quota exceeded".into(),
            },
        ] {
            assert!(
                skip_message(&reason).is_some(),
                "actionable reason must stay visible: {reason:?}"
            );
        }
    }

    #[test]
    fn silent_skip_reasons_show_no_message() {
        // F-13: a clean pass that was never worth polishing must not flash
        // "skipped / rejected / failed" on the pill and tray.
        for reason in [
            SkipReason::TooShort,
            SkipReason::ContextOverflow,
            SkipReason::GateSkip {
                detail: "short and clean".into(),
            },
            SkipReason::IntentSkip {
                intent: "command".into(),
            },
            SkipReason::ValidationRejected {
                kind: "empty".into(),
            },
            SkipReason::ValidationRejected {
                kind: "too much change".into(),
            },
        ] {
            assert!(
                skip_message(&reason).is_none(),
                "silent reason must not surface: {reason:?}"
            );
        }
    }

    #[test]
    fn cancelled_stop_is_marked() {
        // A cancelled stop must be distinguishable from a normal stop so the
        // controller can skip transcription, injection, and history (P0-1).
        let cancelled = decide(
            Input::Cancel,
            Phase::Listening,
            RecordingMode::Hold,
            &mut true,
            &mut false,
        );
        let normal = decide(
            Input::HotkeyUp,
            Phase::Listening,
            RecordingMode::Hold,
            &mut true,
            &mut false,
        );
        match cancelled {
            Action::Stop { cancelled } => assert!(cancelled),
            other => panic!("expected cancelled Stop, got {other:?}"),
        }
        match normal {
            Action::Stop { cancelled } => assert!(!cancelled),
            other => panic!("expected normal Stop, got {other:?}"),
        }
    }

    #[test]
    fn toggle_start_stop_and_wait() {
        assert_eq!(
            decide(
                Input::Toggle,
                Phase::Idle,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::Start { by_hotkey: false }
        );
        assert_eq!(
            decide(
                Input::Toggle,
                Phase::Listening,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::Stop { cancelled: false }
        );
        assert_eq!(
            decide(
                Input::Toggle,
                Phase::Transcribing,
                RecordingMode::Hold,
                &mut false,
                &mut false
            ),
            Action::Nothing
        );
    }
}
