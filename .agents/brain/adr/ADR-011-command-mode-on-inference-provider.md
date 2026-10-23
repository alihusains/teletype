# ADR-011: Command Mode production backend and entry point

**Status:** rejected as a production architecture (2026-10-10)

## Context

The initial Command Mode implementation in `command_mode.rs` used Teletype's
generic `InferenceProvider` and exposed a typed Settings panel. It supports a
small `OpenApp` allowlist and guarded draft/transform plumbing, but it does not
provide the user's required spoken activation and does not use Cactus Needle
tool calling.

The reason recorded in the earlier version of this ADR was factually wrong:
the local `third_party/needle/needle.h` declares `needle_init` with
`tools_json` and `tool_index_path`, and `needle_complete` for structured JSON
completion. The Rust wrapper in `crates/teletype-speech/src/whistle.rs` has not
bound those functions. The local header, static archive, and model file are
ignored by Git, so their presence on one developer machine does not establish
a reproducible release build or redistribution rights.

## Decision

The InferenceProvider-only implementation is a prototype and must not be
marked production-complete. Production Command Mode must use validated
Needle-backed tool calls when the dependency and licensing permit, and must
integrate with the existing dictation controller for spoken activation. If
Needle assets cannot be obtained through a reproducible, licensed build path,
keep Command Mode disabled or clearly unavailable in release builds; do not
silently fall back to a model that cannot execute the requested tools.

All model-returned tool names and arguments are untrusted. Rust handlers must
apply strict schema validation and an explicit capability allowlist. The
initial local action set may include allowlisted app launch and draft insertion
into the captured focused target. Drafting must never send email or messages.
Command Mode must preserve the dictation controller's cancellation,
operation-id and stale-target protections.

## Evidence

- `third_party/needle/needle.h`: declarations for `needle_init` and
  `needle_complete`.
- `crates/teletype-speech/src/whistle.rs`: current FFI binds load/transcription,
  not tool completion.
- `crates/teletype-speech/build.rs`: static linking is conditional on a local
  archive.
- `git check-ignore -v third_party/needle/needle.h
  third_party/needle/macos-arm64/libneedle.a third_party/needle/whistle.cact`:
  all three local assets are ignored.
- `crates/teletype-desktop/src/command_mode.rs` and
  `ui/src/settings/CommandModeSection.tsx`: current generic provider and typed
  panel implementation.
- User requirement in this task: spoken natural-language commands, Cactus
  Needle tool calling, safe drafting and local app actions.

## Consequences

- The existing implementation is partial, not production-ready.
- FFI correctness, model/archive provenance, license, clean checkout behavior,
  spoken controller integration, side-effect validation and user feedback are
  release gates.
- Signing/updater decisions are independent of this ADR.
