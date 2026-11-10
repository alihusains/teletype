# T6: Which LLM is used with Qwen3 ASR in Command Mode?

**Task:** t7-batch / t6-qwen (investigation only, no code changes)
**Worktree:** `/Users/a.sorathiya/Documents/Ali/teletype-wt/t6-qwen`, branch `t7/t6-qwen`
**Date:** 2026-11-08

## 1. The two roles, clearly separated

### Role A: ASR (speech -> text)

Speech is transcribed by a **speech model** chosen in the Models screen
(`list_speech_models` / `select_speech_model`), catalog in
`crates/teletype-speech/src/catalog.rs`. Engines include Whisper rows, Apple
on-device speech, and **Qwen3-ASR**:

- `Qwen3-ASR 1.7B` is catalog row `id: "qwen3-asr"`,
  `Engine::Qwen3Asr` (`crates/teletype-speech/src/catalog.rs:928-931`).
  It runs as a child `llama-server` process (OpenAI-compatible), macOS only,
  30 languages, artifacts `QWEN3_ASR_ARTIFACTS` = main GGUF
  `Qwen3-ASR-1.7B-Q8_0.gguf` + `mmproj-Qwen3-ASR-1.7B-Q8_0.gguf`
  (`catalog.rs:175-233`, `catalog.rs:215-224`).
- **Qwen3-ASR is a transcription engine only. It cannot emit tool calls.**
  This is stated explicitly in `crates/teletype-desktop/src/command_mode.rs:521-524`
  ("Qwen3-ASR transcribes but cannot emit tool calls, so this decider splits
  the work").

### Role B: Decider (text/audio -> command)

Command Mode classifies a take into a tool call. The decider is picked per
take by `select_decider` (`crates/teletype-desktop/src/dictation.rs:6294`,
impl `crates/teletype-desktop/src/command_mode.rs:769`) from a `DeciderSlot`
populated by `rebuild_command_decider`
(`crates/teletype-desktop/src/lib.rs:497-548`). The four decider kinds
(`command_mode.rs:30-50`):

| Kind | What runs |
|---|---|
| `NeuDecide` (default) | 55.5M-param ONNX command-decision model (`neudecide-q4`), speech -> tool call directly |
| `Needle` | TinyDecide + Needle 3 combined provider (fallback) |
| `LlmFallback` | `LlmCommandExtractor` over the local polish LLM |
| `QwenAsrLlm` (id `qwen3-asr+llm`) | **The hybrid:** Qwen3-ASR transcribes; the **local polish LLM** extracts the command |

## 2. The direct answer

**In Command Mode, when the `qwen3-asr+llm` decider is selected, the LLM that
does the command classification/extraction is the user's local polish LLM**
(the currently loaded `InferenceProvider`, i.e. whichever `polish_capable`
catalog model is installed/selected: S1-mini, Qwen3.5 2B, SpeakOfLow Mini,
Qwen2.5 3B, etc. from `crates/teletype-inference/src/catalog.rs`). It runs via
`LlmCommandExtractor` (grammar-constrained JSON tool-call decoding,
`crates/teletype-inference/src/command_llm.rs:47-60`) with the same
`COMMAND_TOOLS_JSON` as the Needle path.

**Qwen3-ASR is only the speech-to-text engine in that hybrid and is NOT the
command LLM.** It transcribes the take (only when the pipeline carries no
transcript already), after a 300 ms VAD gate; then the polish LLM classifies
the text (`command_mode.rs:540-612`, `QwenASRLlmDecider::decide`).

Note the LLM is whatever is *currently loaded* as the polish model: the
hybrid snapshots the inference provider Arc at rebuild time and re-reads it
through a closure at extraction time (`lib.rs:505-518`).

## 3. The selection mechanism

- **Setting key:** `command_decider_model` (Rust) / `commandDeciderModel`
  (camelCase IPC) on the settings struct,
  `crates/teletype-desktop/src/commands.rs:630-635`; patchable via
  `SettingsPatch` (`commands.rs:882`, applied at `commands.rs:956`).
- **Default:** `"neudecide-q4"`
  (`default_command_decider_model`, `commands.rs:1047-1049`).
- **Values:** `"neudecide-q4"` (default), `"qwen3-asr+llm"` (hybrid), and the
  picker also lists `"lfm3-omni"` (always unavailable: needs llama-server
  b11960+, bundled b11488) (`command_decider_rows`, `commands.rs:1082-1125`).
- **Where the decider is chosen:** `rebuild_command_decider`
  (`crates/teletype-desktop/src/lib.rs:497-548`):
  - `"qwen3-asr+llm"` builds the `QwenASRLlmDecider` only when both halves are
    installed (`qwen_asr_installed` = both Qwen3-ASR GGUFs on disk,
    `commands.rs:1133-1153`; `local_llm_installed` = any `polish_capable`
    catalog model downloaded, `commands.rs:1159-1164`);
  - any other id / unavailable hybrid falls back to NeuDecide if its model is
    on disk, else `None` (take falls through to plain Needle/dictation).
- **Slot rebuild triggers:** startup warm-up thread (`lib.rs:1248-1259`) and
  after a settings change touching `command_decider_model` (`lib.rs:491-495`
  doc).
- **Per-take use:** `dictation.rs:6880` checks `settings.command_mode_enabled`,
  then `dictation.rs:6294` calls `select_decider(&state.command_decider,
  language)`; NeuDecide is additionally gated to English/auto
  (`command_mode.rs:769-777`).

## 4. Confusion in the UI (recommendation only)

1. **The decider picker labels the hybrid "Qwen ASR + local LLM"**
   (`commands.rs:1104-1105`) but the Command Mode screen just shows a "Decider"
   dropdown of these rows (`ui/src/screens/CommandModeScreen.tsx:425-455`)
   with no explanation that "local LLM" = the user's *Polish* model, which the
   user picks separately on the Models screen. A user selecting this row
   cannot tell which LLM will actually classify commands (it follows the
   currently loaded polish model, and can change with a model swap).
2. **Models screen vs Command Mode screen split:** the Models screen shows
   speech models (incl. Qwen3-ASR) and LLM/Polish models as separate lists
   (`ui/src/screens/ModelsScreen.tsx:383-391`, speech rows at 800-827), while
   the Command Mode screen's row "Qwen ASR + local LLM" silently depends on
   *both* lists. Nothing on the Command Mode screen links the hybrid row to
   the two concrete models behind it.
3. The row's `installed` flag only reflects the Qwen3-ASR half
   (`commands.rs:1106`), while `available` requires both halves
   (`commands.rs:1109-1112`) - the UI only surfaces `available`/`reason`, so
   this is not user-visible, but the field semantics are easy to misread in
   code.

Recommended (not done, out of scope): on the Command Mode screen, when the
`qwen3-asr+llm` row is selected, show the names of the two concrete models
behind it (speech: Qwen3-ASR 1.7B; LLM: current polish model), or a one-line
tooltip: "Qwen3-ASR transcribes; your selected Polish LLM decides the command."

## 5. Verification

Raw output (command, tail, exit code):

```
$ git -C /Users/a.sorathiya/Documents/Ali/teletype-wt/t6-qwen status --short
?? ui/node_modules
(exit 0; only pre-existing untracked node_modules from an environment build -
no tracked file touched; `git diff --stat` empty, exit 0)

$ ls -la /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t6-qwen.md
-rw-r--r--  1 a.sorathiya  ICM\Domain Users  7185 Nov  8 22:36 /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t6-qwen.md
(exit 0)
```

## 6. Files needed but not owned

None. Investigation only; all cited files were read, none edited.

## 7. Decisions / notes for the lead

- The phrase "qwen3 asr + llm" in the UI is a *decider id*, not a model name;
  the "llm" half is deliberately the already-resident polish LLM (P2.7 design,
  `command_llm.rs` header), not a dedicated command model.
- The default decider in Command Mode (when `qwen3-asr+llm` is not selected)
  is NeuDecide (`neudecide-q4`), matching brain D018
  (`.agents/brain/PROJECT_BRAIN.md`, ADR-015).
