# ADR-012: Canonical branch is `whistle-integration`; Command Mode safety goes through a typed ToolRegistry + permission policy

**Status:** accepted
**Date:** 2026-10-11
**Deciders:** Ali (user), teletype-architect
**Supersedes:** ADR-011 (Command Mode on InferenceProvider) — see D014.

## Decision

1. **Canonical integration branch is `whistle-integration`.** It is the only
   branch with unique feature commits (engine foundation, Whistle provider,
   Needle FFI, Command Mode hardening, Quick Add, in-app updates, CGEventTap
   reliability fix). The one `main`-only commit (`916f666`, D014 brain note)
   is merged into it. The three stale branches
   (`integration/merge-all`, `fix/speaktype-adopt`, `release/icon-fix`) are
   historical ancestors and are not merged wholesale. Full evidence:
   `docs/architecture/branch-audit.md`.

2. **Command Mode side effects are authorized by a typed `ToolRegistry` +
   permission policy, not by hardcoded verb refusals.** The deterministic
   parser remains the fast path. A model's tool call is a *proposal*; the Rust
   policy decides whether the action is allowed. This is the v2 roadmap's
   Phase 3 "Command safety substrate" and the largest architectural gap
   affecting user safety.

3. **The `cactus` feature stays off by default** in release until the
   Cactus/Needle runtime provenance, exported-symbol, ABI, and clean-build
   gates pass (roadmap §12 / Phase 6). The tracked
   `third_party/needle/*` artifacts are development assets, not a release
   guarantee.

## Context

- `main` is a stable dictation product; `whistle-integration` carries the
  Command Mode / Whistle / Needle direction. `git rev-list --left-right
  --count main...whistle-integration` → `1 7`. Merge-base `2532515`.
- The current `command_mode.rs` enforces safety by a hardcoded dangerous-verb
  list (`send`, `delete`, …) and per-argument shell-metacharacter checks.
  That is a defense, not a system: there is no per-tool risk level, no
  confirmation policy, no platform capability metadata, no audit trail, and
  no single executor that only accepts validated+authorized invocations.
- The v2 roadmap §6/§18.4 names this the largest architectural gap and
  requires it before adding tools, MCP, or Agent Mode.

## Alternatives considered

- **Merge all branches wholesale.** Rejected: the stale branches are
  historical ancestors with no unique feature work; merging them would drag in
  duplicated pipelines and generated output.
- **Keep the hardcoded-refusal model and just add more verbs.** Rejected:
  does not scale to the Phase 8 tool set (file ops, clipboard, key input),
  has no confirmation/undo/audit semantics, and cannot express per-platform
  capability.
- **Adopt an external policy engine.** Rejected: the tool set is small and
  local; a typed Rust registry is the right weight and keeps everything in the
  deterministic core (D003 local-first, no new dependency).

## Consequences

- Positive: every command execution goes through one policy/enforcement path;
  unknown tool / invalid schema / path escape / unsupported platform / stale
  target / cancellation / prompt injection all fail closed; a model cannot
  bypass policy by renaming arguments or fabricating a confidence score.
- Positive: unblocks Phase 8 (add tools one at a time), the confirmation UX,
  and later MCP/Agent Mode with a stable permission contract.
- Negative: the Needle-backed path and the deterministic path must both be
  re-routed through the registry; this is a refactor of `command_mode.rs`.
- The `cactus`-gated model path remains unavailable in the default release
  until Phase 6 gates pass; the deterministic path is unaffected.

## Evidence / source

- `docs/architecture/branch-audit.md` (this audit).
- `crates/teletype-desktop/src/command_mode.rs` (current parser + validator).
- `PROGRESS.md` Phase 10/12/19 (tool registry / permissions / security
  architecture: PARTIAL / NOT STARTED).
- v2 roadmap `TELETYPE_ARCHITECTURE_AND_EXECUTION_ROADMAP_v2.md` §0, §6, §13
  (Phase 3), §18.
- `PROJECT_BRAIN.md` D014 (validated Needle tool calling), D016 (NeuDecide
  KILL).
