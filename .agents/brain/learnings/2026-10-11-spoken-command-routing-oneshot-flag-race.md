---
id: L037
title: Spoken-command routing keyed off a one-shot flag is a race that always loses
status: candidate
type: debugging
area: teletype-desktop / dictation controller
agent: opencode
tool: opencode
date: 2026-10-11
---

# Spoken-command routing keyed off a one-shot flag is a race that always loses

## Problem

Spoken Command Mode must route a take's *transcript* to the command pipeline
(`command_transcribed`) instead of the dictation pipeline. The controller had
been routing on the one-shot flag `command_mode_active`. That flag is set when
Command Mode is armed (`Event::CommandStart`) and cleared the instant an armed
take begins (hotkey down, in `hotkey_down_event`) — but the transcript only
arrives seconds later. So by the time `transcribed()` ran, the flag was already
false and every spoken command silently fell through into the dictation
pipeline. Command Mode's spoken path never actually reached the command
pipeline.

## Symptoms

Spoken commands behaved like dictation: they were cleaned/polished and inserted
as text rather than routed to `run_command_core`. No error, no crash — just the
wrong path.

## Initial hypotheses

- The parser or Needle path was refusing the utterance.
- The spoken→command handoff was never wired.

## Investigation

1. Traced the `command_mode_active` lifecycle: set at `CommandStart`
  (dictation.rs:876), cleared in `hotkey_down_event` (dictation.rs:960) *before*
  the `start()` call it guards, i.e. before any recording happens.
2. Confirmed an uncommitted working-tree change had tried to fix this by adding
  `command_session: Option<u64>` and routing on `command_session == Some(session)`.
3. Checked the two facts that make that replacement non-functional:
   - `start()` set the id guarded on `if self.command_mode_active` — already
     cleared one line earlier, so the field is never assigned.
   - Even when assigned it captured the pre-stop `next_id`, while `stop()`
     increments `next_id` and dispatches the transcript with `next_id + 1`
     (dictation.rs:1436-1437, 1552). The values never match.
4. Confirmed no test drove the spoken Command take path (nothing references
   `CommandStart` → hotkey → `start` → `stop` → `transcribed`).

## Evidence

`git log`/diff on `feature/command-safety-substrate` (commit `be42b87` "typed
ToolRegistry + permission policy + golden command eval") plus the uncommitted
`dictation.rs` `command_session` change. Runtime confirmation: the routing
decision is a couple of `if`s in `transcribed()` whose input the flag can never
satisfy at transcript time.

## Root cause

Routing a post-recording outcome by a value that is intentionally spent *before*
the outcome is produced. `command_mode_active` is a correct "arm the next take"
signal but a wrong "this transcript is a command" signal, because it does not
survive from take-start to transcript-arrival.

## Fix

Replaced the broken `command_session` approach with a take-scoped boolean
`command_take: bool`:

- set in `hotkey_down_event` at the one point that knows the new take is a
  command take (before the one-shot flag is spent);
- consumed and cleared in `transcribed()` right when it routes to
  `command_transcribed`;
- also cleared in the cancel branch of `stop()` and in `go_idle()`.

Session-identity safety is provided by the existing `next_id != session`
staleness guard at the top of `transcribed()`, so no fragile `next_id + 1`
arithmetic is needed. Added regression test
`dictation::tests::spoken_command_transcript_routes_by_persisted_marker`
(drives `Event::Transcribed` with `command_take=true`, asserts the marker is
consumed and nothing is injected via the dictation path).

## Verification

`cargo test -p teletype-desktop --lib` → 122 passed (was 121 + the new test);
`-p teletype-core --lib tools` → 34 passed; `command_parser` → 7 passed;
`command-eval --cases evals/command-cases.jsonl` → 56/56 deterministic,
refusals 10/10, gate passed (exit 0). `rustfmt --check` clean on the edited file.

## Reusable lesson

When a decision must survive from "take starts" to "take result arrives,"
persist a take-scoped marker (or the resolved session id) at the *decision
point*, and consume it where the result is handled. Never read it back through a
one-shot flag that legitimate control flow already cleared, and never predict a
future session id by adding a constant to the current counter.

## Evidence / sources

- Commit: `be42b87` (branch `feature/command-safety-substrate`)
- Files: `crates/teletype-desktop/src/dictation.rs`
  (`command_mode_active`, `command_take`, `hotkey_down_event`, `start`, `stop`,
  `transcribed`, `command_transcribed`, `go_idle`)
- Brain note + follow-up: `PROJECT_BRAIN.md` D015 claims "Clippy clean on the
  new modules" for this feature. That was inaccurate for the desktop wiring:
  commit `be42b87` carried cosmetic clippy warnings in `command_mode.rs`
  (dead `Intent::OpenApp`/`OpenPath` variants that are never constructed, plus
  `needless_borrow`, `needless_return`, `unnecessary_sort_by`,
  `explicit_auto_deref`) and `dictation.rs` (a dead second `Err(e)` mic arm
  after the reachable one, and an `&*state` deref). Core `teletype-core::tools`
  was clean. On 2026-10-11 these were cleaned: the two dead variants and their
  match arms removed, `sort_by`→`sort_by_key`, `needless_borrow`/`needless_return`
  fixed, `&*x`→`&x` at the deref sites, and the unreachable `Err` arm deleted.
  `cargo clippy -p teletype-desktop --lib` is now clean on both files
  (122 tests still pass; remaining warnings are in unrelated modules:
  `typing.rs`, `commands.rs`, `autotext_manager.rs`).
