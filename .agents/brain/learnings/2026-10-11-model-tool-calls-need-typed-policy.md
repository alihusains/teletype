# Learning: A model tool call that parses is not proof the action is safe

- **ID:** L023
- **Date:** 2026-10-11
- **Agent:** teletype (DSH / Qwen)
- **Project area:** Command Mode safety (v2 roadmap Phase 3)
- **Status:** verified
- **Evidence:** `crates/teletype-core/src/tools/` (registry, policy, validate,
  schema, audit), `crates/teletype-desktop/src/command_mode.rs`
  (`validate_tool_call`), `evals/command-report.json` (56/56 baseline).

## Problem

Command Mode lets a local model (Needle/Whistle) propose a tool call
(`open_app`, `open_path`, `draft_text`, `run_transform`). The pre-existing
defenses were a hardcoded dangerous-verb list in the parser and per-argument
shell-metacharacter checks. That is a defense, not a system: there was no
per-tool risk level, no confirmation policy, no platform capability, no audit
trail, and no single executor that only accepts validated + authorized calls.
A model could, in principle, rename an argument, fabricate a confidence score,
or name a tool the policy had not thought about.

## Initial hypotheses

1. "Adding more dangerous verbs to the refusal list is enough." — Rejected:
   does not scale to the Phase 7/8 tool set (file ops, clipboard, key input)
   and has no confirmation/undo/audit semantics.
2. "Adopt an external policy engine." — Rejected: the tool set is small and
   local; a typed Rust registry is the right weight and keeps everything in the
   deterministic core (D003 local-first, no new dependency).
3. **Chosen:** a typed `ToolRegistry` + fail-closed `PermissionPolicy` in
   `teletype-core`, with the model-facing JSON schema **generated from the
   registry** so the model can only ever be advertised tools the policy knows.

## Investigation / design

- The model is **untrusted input**. It produces a *proposal*; the Rust policy
  decides. The policy takes no confidence score, so a fabricated score cannot
  grant authority (roadmap §13 Phase 3).
- The schema is generated from the same typed definitions the validator uses,
  so schema and validator cannot drift, and only *enabled* tools are
  advertised (disabled Phase B/C tools are invisible to the model).
- Validation is strict and fail-closed: unknown tool, disabled tool,
  unsupported platform, missing/extra argument, wrong type, shell
  metacharacter, path/URL/scheme escape, non-allowlist transform id → all
  refuse. `http`/`https` are the only URL schemes; `javascript:`, `file://`,
  `chrome-extension:` are rejected.
- Phase C destructive/privileged tools (`delete_file`, `run_shell`) are
  **defined but disabled**, so the registry documents the surface while the
  policy refuses them by default.

## Fix

- `teletype-core::tools` — `ToolDefinition`, `ToolRegistry`, `RiskLevel`,
  `PermissionPolicy` (`Decision`: Allow / Confirm{summary} / Deny{reason}),
  `validate_args` (typed `ValidatedArgs`), `schema::render_needle_tools`,
  `audit::AuditEvent` (redacted: tool id + decision + reason, never
  text/paths).
- `command_mode::needle_tools_json()` now returns
  `render_needle_tools(&default_registry())`; `validate_tool_call` resolves the
  model name → canonical id → typed validation → policy.
- The dictation controller records redacted audit events in a bounded
  in-memory ring (`AppState.command_audit`, capped 512).

## Verification

- `cargo test -p teletype-core --lib tools::` → 34 passing (policy fail-closed
  branches, validator rejections, schema only-ads-enabled, audit redaction,
  confirmation contract).
- `cargo test -p teletype-desktop --lib command_mode` → 24 passing (the
  existing security tests now run through the registry + policy).
- `command-eval` deterministic baseline → 56/56 (100%), refusals 10/10.
- `cargo test --workspace --lib` → 0 failures; clippy clean on the new
  modules; `tsc --noEmit` + `vite build` clean.

## Reusable lesson

When a model (or any external source) can propose an action, treat the
proposal as data, not a command. Put one typed registry as the single source
of truth, generate the model-facing schema from it, and make the policy
fail-closed at every branch. A self-reported confidence score, a renamed
argument, or an unknown tool name must never be able to bypass policy. Keep
destructive/privileged tools defined but disabled until per-tool policy,
confirmation, audit, and recovery are all proven — that is what keeps the
default release conservative while the registry documents the full surface.
