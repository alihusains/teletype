# Task: lifecycle hardening (BUG-013 + 014 + 015 + 016)

**Status:** deferred (track, don't block release)
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-013-orphan-llama-server-on-exit.md`, `bugs/BUG-014-chat-stream-partial-on-server-death.md`, `bugs/BUG-015-provider-race-ensure-vs-select.md`, `bugs/BUG-016-download-no-sha-corrupt-resume.md`

## Problem

Four related lifecycle/reliability gaps in the local llama-server path:

- **BUG-013:** `RunEvent::Exit` shutdown is fire-and-forget; the child
  `llama-server` can outlive the app (G008).
- **BUG-014:** `chat_stream` returns `Ok(partial)` if the server dies after
  the first token — silently truncated rewrite, no error.
- **BUG-015:** `ensure_local_provider` races `select_openai_provider` /
  `select_model`; a stale provider can overwrite the user's choice.
- **BUG-016:** an interrupted download of a no-SHA catalog entry can leave a
  corrupt model that passes the size check (fast/quality models).

## Read first

- All four tickets in `bugs/` (repro, expected/actual, acceptance criteria).
- `crates/teletype-desktop/src/lib.rs` (`RunEvent::Exit` handler).
- `crates/teletype-inference/src/server.rs` (`chat_stream`, `shutdown`,
  `stop_child`).
- `crates/teletype-desktop/src/commands.rs` (`ensure_local_provider`,
  `select_model` — note BUG-007's kill-before-spawn is already in place).
- `crates/teletype-inference/src/download.rs` + `catalog.rs` (no-SHA path).

## Acceptance criteria

Copied from the tickets — see each BUG file. Summary:
- Exit path waits for the child to actually exit (bounded wait, then kill).
- `chat_stream` returns `Err` when the stream ends before the model's
  stop condition (server death is not a clean finish).
- Provider selection is serialized (one mutex-protected transition;
  `ensure_*` never overwrites a user-selected provider).
- No-SHA downloads verify a minimum plausibility bound (or are refused)
  and an interrupted partial file is never treated as complete.

## Verification

- `cargo test -p teletype-inference`
- `cargo test -p teletype-desktop`
- `cargo clippy -p teletype-inference -p teletype-desktop`
- Manual: kill llama-server mid-stream in the running app; expect an error
  surfaced, not a silent partial.

## Out of scope

- BUG-007 (already fixed: kill-before-spawn in `select_model`).
- Any transform/pipeline change.
- Do not touch `personalization/` or `ui/`.
