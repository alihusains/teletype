# AI Polish controls are model-scoped, and the UI was presenting dead controls

- ID: L092
- Date: 2026-11-09
- Agent: pi (lead)
- Project area: AI Polish / Transforms screen (`ui/src/screens/TransformsScreen.tsx`, `crates/teletype-core/src/transforms/prompt.rs`)
- Status: verified

## Problem

The AI Polish page showed a wall of customization controls ("What Polish fixes"
rules, the "Polish prompt" override, "Custom instructions") that looked active
but were silently ignored by the loaded model. The user's default model
(SpeakoFlow Mini) honored none of them, so the whole right-hand side of the
page was decorative.

## Root cause

`model_style()` in `crates/teletype-core/src/style.rs` maps each model id to a
prompt path, and only some paths read the user controls:

| Model | Path | Honours |
|---|---|---|
| s1-mini | `run_s1` (fixed card + control line) | nothing (rules / override / instructions all ignored) |
| speakoflow-mini | `run_speakoflow` (fixed card + bare transcript) | nothing |
| qwen3.5-2b | `run_qwen_polish` (fixed editor + `ctx.user_override`) | **override only** |
| anything else (LFM3, remote/ollama) | generic `build_prompt` | rules + override + instructions |

The "What Polish fixes" rules and "Custom instructions" are composed into the
transform's instruction, which only the generic path sends. The prompt override
(`settings.polish_prompt_override` → `ctx.user_override`) is read by the Qwen
and generic paths only.

## Inconsistency found and fixed

The engine honors the Qwen override (`run_qwen_polish` uses
`qwen_polish_system_prompt_with_override(ctx.user_override)`), but
`polish_prompt_info` marked Qwen `editable: false`, so the UI hid the override
editor for a model that actually uses it. Fixed by making `polish_prompt_info`
return `editable: true` for Qwen (with a note that the override is an addendum
and the stock rules still win). Updated the test
`prompt_info_marks_fixed_contract_and_generic_paths` accordingly.

## Fix (UI scoping)

Added `polishCustomizationFor(modelId)` → `"none" | "override" | "full"` in
`TransformsScreen.tsx` and used it to:
- dim + disable the "What Polish fixes" rules and "Custom instructions" when
  the model doesn't honor them, with a one-line reason;
- append a model-capability note to the Band 0 status line;
- disambiguate the two preview panes (Band 2 runs the *selected* preset;
  Band 2.5 always runs the *Polish* prompt);
- move the change ceiling (`polishMaxChangePercent`) into the AI Polish
  section, next to the rules it caps.

## Verification

- `cargo test -p teletype-core`: 1120 passed, 0 failed.
- `npx tsc --noEmit`: clean.
- `npx vitest run`: 545/545 passed.
- App restarted, HTTP 200, speakoflow-mini loaded.

## Reusable lesson

The AI Polish page is organized around *presets*, but the **model** decides
which prompt path runs and therefore which controls actually apply. Any new
polish control must be scoped to the model's capability
(`none` / `override` / `full`) or it will look active but do nothing for the
default card models. Keep `polish_prompt_info`, the engine dispatch, and the
UI scoping in agreement about which models honour which controls.
