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
        Input::Cancel => match phase {
            Phase::Listening => Action::Stop { cancelled: true },
            Phase::Stopping | Phase::Transcribing | Phase::Transforming | Phase::Inserting => {
                Action::CancelPipeline
            }
            _ => Action::Nothing,
        },
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
    #[default]
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// The visual state of the floating pill window.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PillState {
    Idle,
    Recording { started_at_ms: u64 },
    Warming,
    Processing { message: String },
}

/// The user-facing message for a transform skip/fallback, shown at
/// dictation time (pill, tray, toast). `None` when nothing needs telling
/// the user (clean pass or successful transform).
///
/// The specific [`crate::transforms::engine::SkipReason`] carries the
/// detail; this is the short, display-ready form.
pub fn skip_message(reason: &crate::transforms::engine::SkipReason) -> String {
    match reason {
        crate::transforms::engine::SkipReason::NoModelLoaded => {
            "Polish skipped: no model loaded. Using raw text".to_string()
        }
        crate::transforms::engine::SkipReason::ModelLoadFailed => {
            "Polish failed: model load failed. Using raw text".to_string()
        }
        crate::transforms::engine::SkipReason::InferenceError { detail } => {
            format!("Polish failed ({detail}). Using raw text")
        }
        crate::transforms::engine::SkipReason::ValidationRejected { kind } => {
            format!("Polish rejected ({kind}). Using raw text")
        }
        crate::transforms::engine::SkipReason::TooShort => {
            "Too short to polish. Using raw text".to_string()
        }
        crate::transforms::engine::SkipReason::ContextOverflow => {
            "Input too large for the model. Using raw text".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transforms::engine::SkipReason;

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
            skip_message(&SkipReason::NoModelLoaded),
            "Polish skipped: no model loaded. Using raw text"
        );
        assert_eq!(
            skip_message(&SkipReason::ModelLoadFailed),
            "Polish failed: model load failed. Using raw text"
        );
        assert_eq!(
            skip_message(&SkipReason::InferenceError {
                detail: "timeout".into()
            }),
            "Polish failed (timeout). Using raw text"
        );
        assert_eq!(
            skip_message(&SkipReason::ValidationRejected {
                kind: "empty".into()
            }),
            "Polish rejected (empty). Using raw text"
        );
        assert_eq!(
            skip_message(&SkipReason::TooShort),
            "Too short to polish. Using raw text"
        );
        assert_eq!(
            skip_message(&SkipReason::ContextOverflow),
            "Input too large for the model. Using raw text"
        );
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
