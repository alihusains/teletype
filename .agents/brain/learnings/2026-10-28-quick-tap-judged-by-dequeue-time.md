---
id: L084
title: Quick-tap paste was misread as a long hold because the hold was measured when the controller dequeued the release
status: verified
type: debugging
area: teletype-desktop/dictation + mod_tap/fn_tap
agent: main session (Qoder)
tool: app log, live Notes/TextEdit taps, cargo test
date: 2026-10-28
---

# Quick tap judged by dequeue time, not physical hold time

## Problem

The quick-tap gesture (Hold mode, hotkey held under 1 s) must throw the take
away and paste the clipboard. It worked on a warm app, but after an idle unload
a 300 ms tap was handled as a normal dictation: the app transcribed ~100 ms of
silence (`transcribe failed: invalidAudioData`), showed `Transcribing...`, and
never pasted.

## Symptoms

- Text tap into TextEdit worked when the model was warm.
- Tap into Notes right after a model reload pasted nothing; the log showed
  `transcribing audio: 3200 samples` for a 300 ms press.
- One take left the pill in `Transcribing...` for over 6 s while the engine
  recompiled (Encoder 10.8 s), with no completion line.

## Initial hypotheses

1. The image on the clipboard could not be pasted by Cmd+V at all. Rejected: a
   manual Cmd+V of the same PNG pasted into the same empty Notes note.
2. Teletype's synthetic Cmd+V (Enigo) was not delivered. Rejected: the same
   tap path pasted plain text into TextEdit and, after the fix, into Notes.
3. The tap predicate failed because the controller was busy when the release
   arrived. Confirmed by the logs.

## Investigation

1. Reproduced text tap in TextEdit: worked. Reproduced image tap in Notes: the
   tap path was not taken (`transcribing audio` logged, no `quick tap` line).
2. Read `hotkey_up_event`/`tap_paste_due`: `held_ms` was
   `now_ms() - started_at_ms`, and `started_at_ms` is stamped when the
   controller thread *dequeues* the press. The release is dequeued by the same
   thread, after the take start has already done model loads and AX reads.
3. Logs lined up: take start at 20:05:42.7, Committer Encoder compile
   `10799.52 ms`, i.e. the controller was busy for 10.8 s while both edges sat
   in the queue. The physical 300 ms hold was measured as seconds.
4. After the fix, the same cold-model tap logs `quick tap: pasting the
   clipboard` and pastes into Notes.

## Root cause

`is_tap_paste` received a hold duration measured at dequeue time. Any slow
controller work between the physical press and the physical release (engine
load, AX capture, screen-context read, boost scoring) inflates the number past
`TAP_PASTE_MAX_HOLD` (1000 ms), so a real tap is classified as a dictation.

## Fix

Both physical edges are now stamped where they happen:

- `mod_tap.rs`, `fn_tap.rs` and the string-hotkey callback call
  `dictation::mark_hotkey_down()` / `mark_hotkey_up()` before sending the event.
  The hold (`now - down`) is computed in the up callback and stored in
  `HOTKEY_HOLD_MS`, consumed once by `tap_paste_due`.
- The old enqueue-time estimate remains only as a fallback for releases that do
  not come from an event tap (headless tests, e2e injection).
- `hotkey_up_event` logs `quick tap: pasting the clipboard` when the tap path
  fires, so the branch is visible in the field log.

## Verification

- Cold-model tap into Notes: `quick tap` logged, `TAP2-NOTES-OK` landed.
- PNG clipboard tap into Notes: image inserted.
- Final tap into a fresh TextEdit doc: `FINAL-TAP-OK` landed, `Pasted` toast.
- `cargo test -p teletype-desktop --lib dictation::` 97/97, `cargo check
  --all-targets` clean, app restarted with vite 200 and one instance.

## Reusable lesson

A timing decision about a physical input must be measured at the input edge,
never after the event has crossed a possibly-blocked worker queue. Stamping in
the event-tap callback (main run loop) is unaffected by controller work; one
atomic hold value consumed per release is enough. Watch for the same pattern in
any "short press vs long press" logic.

## Evidence / sources

- `crates/teletype-desktop/src/dictation.rs` (`mark_hotkey_down`,
  `mark_hotkey_up`, `take_hotkey_hold_ms`, `tap_paste_due`)
- `crates/teletype-desktop/src/mod_tap.rs`, `fn_tap.rs`
- Live log `/tmp/teletype-app.log` (Encoder 10799.52 ms between the edges)
- User report: quick tap must paste the clipboard, images included
