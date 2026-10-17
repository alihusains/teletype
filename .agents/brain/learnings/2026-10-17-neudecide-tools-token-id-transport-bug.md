# L034 — "Token soup" from model audio was a transport bug: missing tools_token_id prefix

- **ID:** L034
- **Title:** NeuDecide emitted token soup on noisy audio because the Rust runtime omitted the `tools_token_id` prefix the Python encoder adds
- **Date:** 2026-10-17
- **Agent/tool:** OpenCode lead session (worktree `ahsorathiya7-fuzzy-happiness`)
- **Project area:** `crates/teletype-decider` (plan addendum A.3.2 runtime)
- **Status:** verified
- **Evidence:** shell/TTS-noise input produced garbage tokens ("corrupted concurrency + fresh inputs" was the initial suspect); cross-parity diff via `scripts/gen_neudecide_parity.py` showed Python `encode_tools` prepends `tokenizer.encode("<|tools|>")` (`tools_token_id`) before the serialized tool list; the Rust port sent the tool list bare. Prepending the token made `tests/a4_no_mic.rs` (20-fixture json_dumps parity, noise-beats-true on `evals/commands/tts` wavs, same-process regression) pass; frozen `tests/fixtures/parity/expected.json`.

## Problem

Noisy-shell audio fed to the UART decider decoded into token soup, which looked
like concurrent model corruption or bad input.

## Root cause

The ported decode transport was not byte-identical to the reference: the
Python encoder prepends the special `tools_token_id` before the tool-list
string; the Rust runtime sent the tool list without it. The model then treated
the tool JSON as ordinary prompt text, and on low-signal audio it collapsed
into raw-token output.

## Rule

When porting an inference transport, diff **token-level** inputs (emitted
token ids, including special tokens) against the reference, not just the
surrounding strings. Keep an independent no-mic closed-loop gate: fixture
parity + a known-bad noisy input that must score better than the raw
baseline + a same-process regression test. That catches transport drift
without a live mic. "The model corrupted concurrency + fresh inputs" is
usually a transport bug, not the model.

## Verification

`ORT_DYLIB_PATH=... cargo test -p teletype-decider --features onnx` green
(23 lib + 3 a4 including the noise gate + 6 json_dumps + 3 fuzz); full
workspace gate green (2026-10-17).
