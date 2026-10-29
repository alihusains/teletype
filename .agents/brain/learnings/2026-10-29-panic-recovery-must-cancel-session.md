---
id: L085
title: A panic-recovered take must be tagged cancelled or its result injects late
status: candidate
type: debugging
area: teletype-desktop / dictation controller
agent: opencode
tool: opencode
date: 2026-10-29
---

# A panic-recovered take must be tagged cancelled or its result injects late

## Problem

`recover_after_panic` (dictation.rs) is the catch-all the controller uses after
any `handle(event)` panics: it returns the phase to Idle so a stuck pill and
swallowed hotkeys are cleared. But it only called `go_idle()`. That left the
abandoned take's session id *untagged*. The transcribe/pipeline worker that was
mid-flight keeps running on its own thread and will still emit a
`PipelineReady` for that session id.

## Symptoms

Silent data corruption: after a panic mid-take, the user's dead take's polished
text can be pasted into whatever app they are editing seconds later, with no
error. Rare (needs a panic mid-pipeline) but high blast radius.

## Root cause

`pipeline_ready` discards a result only when the session is in
`cancelled_sessions` or `session != next_id`. After `recover_after_panic` the
phase is Idle but `next_id` still equals the abandoned session (a new take has
not started, so `next_id` was not advanced), so the stale
`PipelineReady` passes the staleness check and injects.

## Fix

`recover_after_panic` now calls `self.cancel_session(self.next_id)` before
`go_idle()`, so the orphaned result is discarded on arrival, exactly like a
user cancel.

## Verification

`recover_after_panic_returns_a_busy_session_to_idle` now asserts the session is
in `cancelled_sessions` after recovery. Full workspace green.

## Reusable lesson

Any path that abandons an in-flight take without running its normal terminal
(stop/cancel) must tag the session cancelled, or the worker's late
`PipelineReady` will inject. "Reset the phase to Idle" is not the same as
"end the take".
