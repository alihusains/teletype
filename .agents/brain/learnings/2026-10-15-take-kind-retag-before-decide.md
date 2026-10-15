---
id: 2026-10-15-take-kind-retag-before-decide
title: Setting session state before the state machine decides re-tags in-flight takes (command takes silently became dictation)
status: verified
type: debugging
area: teletype-desktop / dictation controller / command mode
agent: opencode
tool: opencode (lead session, inline implementation)
date: 2026-10-15
---

# Setting session state before the state machine decides re-tags in-flight takes

## Problem

P2-A introduced a per-session `TakeKind` (`Dictation` / `Command` / `Transform`)
so a command take is forced through command extraction. P2-B then wired the
first real producer of `TakeKind::Command` (the dedicated Command hotkey).
While reviewing the P2-B path before running it, the release path turned out
to silently destroy the kind:

    fn input_with_kind(&mut self, input: Input, kind: TakeKind) {
        self.take_kind = kind;            // <- happens on EVERY input
        let action = state::decide(...);
        match action { ... }
    }

`hotkey_up_event()` calls `self.input(Input::HotkeyUp)`, which calls
`input_with_kind(Input::HotkeyUp, TakeKind::Dictation)`. In Hold mode a
release decides `Stop` — but the kind had already been overwritten to
`Dictation` *before* the decision. `transcribed()` later reads
`self.take_kind` and would route every Hold-mode command take as dictation.

The same shape bug hits swallowed presses: a cross-kind press while the
hotkey latch is already down decides `Nothing`, yet still overwrote the kind.

## Symptoms (what to look for)

- A feature whose tests pass in isolation but silently degrades in the real
  flow: the command hotkey "works" (take starts, pill shows) but the spoken
  text lands as dictation text instead of running a command.
- No error, no log: the decision that mattered (`Stop`) was correct; only the
  session field read later held the wrong value.
- The P2-A tests did not catch it because they set the kind *after* the stop
  path (`headless_release(TakeKind::Command)`), mirroring the intended
  behavior rather than the real call chain (`hotkey_up_event`).

## Root cause

Ordering: state mutation for the *next* take was made unconditionally, before
the state machine decided whether the input even starts a take. Only
`Action::Start` should tag a take; `Stop`/`Nothing`/`Discard` must leave the
in-flight take's state alone. The pattern to distrust: "set the request field,
then ask the state machine what to do".

## Fix

- `input_with_kind` no longer tags; the tag happens in `start()` (already did)
  — i.e. only when `state::decide` returns `Action::Start`.
- New test `swallowed_press_does_not_retag_take_kind` pins that a press
  swallowed by the hotkey latch (`Action::Nothing`) leaves the kind alone.
- The full release path cannot be unit-tested headlessly (real `stop()` spawns
  the transcribe worker), so the invariant is structural: `take_kind` is
  assigned in exactly two places — `start()` (production) and the test-only
  `headless_release()`.

## Verification

- `cargo test -p teletype-desktop --lib` — 114/114, including the new test and
  all P2-A kind tests.
- Commit `c601f65` (P2.4–P2.8) contains fix + tests; PROGRESS.md records the
  bug explicitly.

## Reusable lesson

When a session/entity carries a mode or kind that later stages read:
1. Tag it at the single point where the state machine decides a transition
   INTO the active state (here: `Action::Start`), never on the way in to the
   decision function.
2. Design the code so the "stop" side reads the stored kind instead of
   receiving a fresh one — if a caller must pass a kind to a release event,
   pass the stored value (`self.take_kind`), not a default.
3. When reviewing state machines, grep for assignments that happen before
   `decide`/`match` — they are the classic silent-corruption spot.
