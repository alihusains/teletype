//! Pure gate: is Command Mode available for the selected LLM + ASR pair?

use crate::schema::{Capability, LlmEntry, SpeechEntry};

/// Why Command Mode is (or isn't) available for a given selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandModeStatus {
    Available,
    MissingToolCallingLlm,
    MissingUsableAsr,
    MissingBoth,
}

/// Available iff the selected LLM has `tool_calling` AND the selected ASR is
/// usable on this host. `asr_usable` is passed in (caller resolves from
/// `speech_offered_on` + installed state) so the gate stays pure and host-free.
pub fn command_mode_available(
    selected_llm: &LlmEntry,
    _selected_asr: &SpeechEntry,
    asr_usable: bool,
) -> CommandModeStatus {
    let has_tool_calling = selected_llm.capabilities.contains(&Capability::ToolCalling);
    match (has_tool_calling, asr_usable) {
        (true, true) => CommandModeStatus::Available,
        (false, true) => CommandModeStatus::MissingToolCallingLlm,
        (true, false) => CommandModeStatus::MissingUsableAsr,
        (false, false) => CommandModeStatus::MissingBoth,
    }
}
