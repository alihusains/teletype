---
id: L099
title: Keep polish formatting deterministic and limit grammar repairs to unambiguous agreement families
status: candidate
type: architecture
area: "teletype-core/transforms, UI/settings"
agent: OpenCode
tool: Hy4 flash Free
date: 2026-11-24
---

# Keep polish formatting deterministic and limit grammar repairs to unambiguous agreement families

## Problem

Users reported that Polish neither capitalized sentences properly nor fixed verb tense/grammar, even though both prompts already said "fix grammar, punctuation and capitalization". S1-mini and SpeakoFlow Mini use card-locked system prompts, and small instruct models (Qwen3.5-0.8B/2B) routinely ignore generic formatting advice.

## Evidence

- Wispr Flow documents Smart Formatting as a separate on-device layer for casing, spacing and punctuation that matches surrounding text; cleanup is a separate LLM stage. Public complaints about Flow are mostly over-editing, not missed capitalization (docs.wisprflow.ai, wisprflow.ai/whats-new, r/WisprFlow).
- EnviousWispr puts every binding on one Keybinds page inside its unified Settings window, grouped into Recording and Shortcuts; Wispr Flow puts Shortcuts under Settings > General.
- L059 already proved the 2B will not capitalize a lowercase "hey" under any prompt phrasing; the code-level capitalize_leads fix was the result.
- New unit tests (capitalization/agreement modules) and the integration test polish_capitalization_and_agreement.rs pass on the real transform chain. No live model A/B was run, so the model-side prompt changes remain unverified in production.

## Decision / fix

- transforms/capitalization.rs capitalizes each sentence start after period, question mark, exclamation, ellipsis, newline and open quotes, protecting abbreviations, initials, decimals and domains. transforms/agreement.rs repairs only closed-set subject-verb errors (she don't -> she doesn't, I has -> I have, we was -> we were); semantic rewrites are left to the model.
- Both run at the engine exit for accepted Polish results only; fallback stays byte-for-byte. The 2B/0.8B prompts additionally name tense and agreement with one-line examples; card-locked prompts were left untouched.
- All keyboard bindings (record, cancel, command, paste last, quick add, transform shortcuts) moved into one Shortcuts section inside Settings, with read-only links from Dictionary, Command Mode and AI Polish.

## Reusable lesson

A repeated formatting promise must not live only in a prompt: keep it in deterministic code. Conversely, grammar fixes that require a semantic choice must stay bounded to a closed family so the app never changes what the speaker meant.
