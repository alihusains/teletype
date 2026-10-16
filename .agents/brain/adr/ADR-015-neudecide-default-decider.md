# ADR-015 — NeuDecide becomes the default Command Mode decider

**Date:** 2026-10-16
**Status:** accepted — crate + config committed (`crates/teletype-decider`);
desktop wiring lands in the A.3.3 unit; Gate A (eval results + model
re-host) pending.

## Decision

Command Mode decisions go through a **hybrid decider**: the NeuDecide ONNX
model (~43 MB, Apache-2.0) takes the take audio and returns the tool plus
enum arguments (~46 ms); the transcript fills the free-form arguments
(query, recipient, body) via deterministic rules first, LLM JSON-schema
fallback second. Selection order at runtime:

1. **NeuDecide** when the model files are present and the language is
   English (or `auto`) — the default.
2. **Needle** (existing `cactus` path) when selected/unavailable above.
3. **LLM JSON fallback** (`teletype-inference/src/command_llm.rs`) — always
   available.

NeuDecide runs **only on command takes** (never as a dictation classifier)
and is **skipped for non-English**. Model output is parsed with the
byte-compatible Python `json.dumps` tool serializer and the no-panic
`parse_tool_calls` parser (fuzz target `fuzz_decoder_output`).

## Context

The 2026-10-10 research verdict was "do not integrate now": the model was
Python-only and alpha, and the Python package could not be downloaded to
verify assumptions. The plan addendum (A.1) reversed the requirement and
specified the Rust crate, hybrid arg-filling and wiring. Since then:

- `crates/teletype-decider` reproduces the model's expected inputs
  byte-for-byte (20-fixture `json.dumps` compat test) and the decode loop is
  unit-tested with a fake runner; the ONNX runtime is behind the default-off
  `onnx` feature.
- The model is Apache-2.0, self-hostable, and small enough to ship.
- The extractor it replaces (Needle) does not cover free-form args anyway, so
  the "hybrid" split is required either way.

## Alternatives considered

- **Stay on Whistle → Needle** — rejected: the addendum makes NeuDecide the
  default; the crate removes the integration cost that motivated the old
  verdict.
- **Keep research-only** — rejected for the same reason.
- **Use NeuDecide on the dictation path too** — rejected: it is a command
  decider; dictation keeps the existing classifier, and non-English is
  explicitly skipped to avoid accuracy claims the model cannot support.
- **Remote/hosted inference** — rejected: violates D003 (no network
  dependency in the hot path) and the offline stance.

## Consequences

- Command takes get a faster, tool-native decider; free-form args stay
  deterministic-first.
- Until the model is re-hosted (gated HF source → `teletype-models` release
  `neudecide-v1-q4` with recorded SHA-256), availability is `false` and every
  command take falls through to Needle/LLM — **no user-visible regression**.
- Four new trace fields (`decider`, `decider_ms`, `tool`, `args_source`,
  serde-defaulted) make the chosen path auditable in every dictation trace.
- `// UNVERIFIED` assumptions (ONNX tensor names/shapes, config keys) must be
  reconciled against the real package when the re-host lands.

## Evidence / source

Plan addendum `Teletype Agent Plan  NeuDecide Addendum.md` (A.1–A.5);
`docs/research/neudecide.md`; `crates/teletype-decider` (committed with the
C4 unit); fuzz target `fuzz_decoder_output`; `PROJECT_BRAIN.md` D015.
