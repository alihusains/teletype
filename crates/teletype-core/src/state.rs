//! The dictation state machine.
//!
//! `decide` is a pure function of (input, phase, mode) so the whole voice
//! lifecycle is testable without a microphone. The desktop app drives it from
//! a single controller thread (see `teletype-desktop::dictation`).

use serde::Serialize;

/// How the user controls recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RecordingMode {
    /// Record while the hotkey is held; transcribe on release.
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
    /// Stop recording and throw the audio away.
    Discard,
    /// Let the running pipeline finish in the background without inserting.
    CancelPipeline,
}

/// Decides what `input` does in `phase`. `hotkey_down` tracks whether the
/// hotkey is currently held, so key repeat is ignored.
pub fn decide(input: Input, phase: Phase, mode: RecordingMode, hotkey_down: &mut bool) -> Action {
    match input {
        Input::HotkeyDown => {
            if mem_replace(hotkey_down, true) {
                return Action::Nothing;
            }
            match phase {
                Phase::Idle => Action::Start { by_hotkey: true },
                Phase::Listening if mode == RecordingMode::Toggle => {
                    Action::Stop { cancelled: false }
                }
                _ => Action::Nothing,
            }
        }
        Input::HotkeyUp => {
            let was_down = mem_replace(hotkey_down, false);
            if was_down && phase == Phase::Listening && mode == RecordingMode::Hold {
                Action::Stop { cancelled: false }
            } else {
                Action::Nothing
            }
        }
        Input::HotkeyInterrupted => {
            *hotkey_down = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run(phase: Phase, mode: RecordingMode, inputs: &[Input]) -> Vec<Action> {
        let mut down = false;
        inputs
            .iter()
            .map(|&i| decide(i, phase, mode, &mut down))
            .collect()
    }

    #[test]
    fn hold_mode_records_while_held() {
        let mode = RecordingMode::Hold;
        let mut down = false;
        assert_eq!(
            decide(Input::HotkeyDown, Phase::Idle, mode, &mut down),
            Action::Start { by_hotkey: true }
        );
        // Key repeat while held does nothing.
        assert_eq!(
            decide(Input::HotkeyDown, Phase::Listening, mode, &mut down),
            Action::Nothing
        );
        assert_eq!(
            decide(Input::HotkeyUp, Phase::Listening, mode, &mut down),
            Action::Stop { cancelled: false }
        );
        assert!(!down);
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
                &mut false
            ),
            Action::Stop { cancelled: true }
        );
        assert_eq!(
            decide(
                Input::Cancel,
                Phase::Transcribing,
                RecordingMode::Hold,
                &mut false
            ),
            Action::CancelPipeline
        );
        assert_eq!(
            decide(Input::Cancel, Phase::Idle, RecordingMode::Hold, &mut false),
            Action::Nothing
        );
    }

    #[test]
    fn toggle_start_stop_and_wait() {
        assert_eq!(
            decide(Input::Toggle, Phase::Idle, RecordingMode::Hold, &mut false),
            Action::Start { by_hotkey: false }
        );
        assert_eq!(
            decide(
                Input::Toggle,
                Phase::Listening,
                RecordingMode::Hold,
                &mut false
            ),
            Action::Stop { cancelled: false }
        );
        assert_eq!(
            decide(
                Input::Toggle,
                Phase::Transcribing,
                RecordingMode::Hold,
                &mut false
            ),
            Action::Nothing
        );
    }
}
