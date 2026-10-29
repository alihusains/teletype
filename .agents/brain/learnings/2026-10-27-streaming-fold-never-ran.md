---
id: L070
title: Streaming commit never folded in production - four independent defects between the pill test and release
status: verified
type: debugging
area: teletype-desktop/dictation + committer
agent: main session (Qoder)
tool: cargo test, e2e socket harness, app log
date: 2026-10-27
---

# Streaming commit never folded in production: four independent defects

## Problem

The P4 streaming commit pipeline ("transcribe segments while the user is still
talking; on release only the tail remains") had passing unit tests but never
actually ran in the app. Every real take decoded the whole recording after
release. Found while verifying the stuck-pill fix on a live e2e take.

## Symptoms

- Every take logged `transcribing audio: <whole take> samples` at release, even
  when `streaming commit: ... ms segment` had logged mid-take.
- The default engine (Fluid) logged `whisper_model_load: invalid model data
  (bad magic)` on every take (separate defect, fixed first).
- The fix appeared to work (2 commits mid-take) but the returned text stopped
  after the first segment: the long-01 corpus take returned 210 chars of a
  653-char reference.

## Root causes (four, all able to block the fold on their own)

1. **Provider misbinding.** `streaming_for_take` built a `WhisperProvider` for
   Fluid/Sherpa/Whistle and loaded the Fluid model directory into it, so the
   committer never had a working engine for the default model. Fix:
   `streaming_provider_for(kind, model_id)` returns the same provider the main
   manager uses (`neural_provider_for` for Fluid/Sherpa) and `None` for
   Whistle (no catalog entry, no streaming provider).
2. **Double take of the streaming slot.** `stop()` moved the slot into a local
   (`let streaming = self.streaming.take()`) to measure trace marks, then the
   transcribe worker re-took `self.streaming.take()` (now `None`). The worker
   ALWAYS ran the whole-take fallback. Fix: one take, moved into the worker.
3. **Circular release gate.** The worker decided streaming vs whole-take with
   `committer.committed_count() > 0`, but that counter only grows inside
   `poll_completed()`, whose only production caller was inside the branch the
   gate guarded. Unreachable by construction. Fix: gate on
   `Committer::submitted_count()` (the Segmenter's cut decision, incremented in
   `submit_segment`) through one shared helper
   `StreamingTake::streaming_marks()` used by both release paths.
4. **Segment-relative word timings fed into an absolute-timeline merge.** Each
   ASR call returns word times relative to its own audio, but
   `merge_transcripts` de-duplicates by comparing `next` words against `prev`'s
   last word end. Segment 2's words all looked older than segment 1's last word
   and were dropped as overlap, keeping only segment 1's text. Fix: every
   committer job carries its take offset (`start_ms`), and the worker shifts
   the returned words into take-absolute time before sending them.

Two more defects surfaced while fixing the above:

5. **Tail queue-jump could lose text.** `finish()` sent the tail job at the
   queue front and then only non-blocking-polled the remaining segments, so
   segments still queued behind the tail could be dropped (and arrived out of
   order when they were not). Fix: `wait_pending()` blocks on every submitted
   segment after the tail, and `committed` is kept as `(id, transcript)` sorted
   by id.
6. **Sample-buffer lag in `feed_frame`.** `buf16` was extended AFTER the chunk
   loop, so a segment ending inside the current frame was sliced against the
   previous frame's length (`end = seg.end_sample.min(buf16.len())`). With
   real 16 ms frames this loses 16 ms per segment; with the 10 s frames the
   tests feed it truncates test segments badly. Fix: append the frame to
   `buf16` before feeding its chunks.

## Why the tests missed all of it

- The P4 tests drive `StreamingTake` directly; nothing exercised `stop()`'s
  handoff to the worker, which is where defect 2 lives.
- The gate tests modeled the production gate with the same wrong expression
  (`committed_count() > 0`) and called `poll_completed()` manually, a call no
  production code makes ("the same call the pill loop makes" was false).
- Mock providers return one constant string with no words, so segment slicing
  and timestamp shifts were invisible. The parity tests built words with
  take-absolute timings, hiding defect 4.

## Fix and verification

Regression tests added: `streaming_fold_gate_opens_without_any_poll`,
`fold_keeps_every_segment_when_word_timings_are_segment_relative`,
`finish_never_drops_segments_queued_behind_the_tail`,
`submitted_count_is_visible_without_any_poll`,
`streaming_committer_uses_the_selected_neural_engine_not_whisper`.

Live e2e (`--e2e` + `TELETYPE_FORCE_DELIVERY=hold`, long-01.wav, 33.6 s):

- before: whole-take decode `537913 samples` (3641 ms), no fold;
- after: `streaming transcription complete: 2 segments committed mid-take, tail
  11059 ms - 96 ms`, output 648 chars matching the 653-char corpus reference
  (differences: "14th" ITN and a trailing period).

Short take (plain-01, 2.6 s, no mid-take cut) still takes the whole-take
fallback (`transcribing audio: 41246 samples`, 303 ms) with the correct text.

## Reusable lesson

A gate that reads a counter only an inner call can advance is a proof of
unreachability, not a heuristic. When a feature has tests but "never runs",
compare the production handoff path (who takes/owns the state) against the test
harness, not just the branch logic. Mock audio must also mimic frame sizes and
timestamp scopes of real providers, or the tests exercise a different geometry
than production.

## How to read `committed_segments` in the trace store (added 2026-10-27, after a false alarm)

On the evening after the fix, the main session read the store, saw
`committed_segments = 0` on 200 of 203 takes, and reported that streaming "has
never run on this Mac at all". That was wrong twice over, and the worker it
contradicted was right:

1. **A zero is the correct value for any take that never reached a cut point.**
   `StreamingTake::streaming_marks` returns `None` when `submitted_count() == 0`,
   so both `committed_segments` and `tail_audio_ms` stay at their serde defaults.
   A take under `max_segment_secs` (12 s) with no long-enough pause commits
   nothing, exactly like an unarmed take. The column is only informative on
   takes long enough to force a cut.
2. **The store has no `date` field.** Sorting rows by `t.get("date","")` is a
   silent no-op, so a "newest 200" window over an 8-day store interleaves files
   by session counter and can drop the positive rows entirely (all three here sit
   at `session = 1`, which sorts last). Group by filename instead.

The three non-zero rows in the whole store (33.69/33.68/33.68 s,
`tail_audio_ms = 11059`, `parakeet-ultra-coreml`) are the `--e2e` long-01.wav
verification runs recorded in the section above, not user dictations. So the
honest summary is: the mechanism is proven working under a 33 s forced-cut take
and proven byte-identical to whole-utterance decode, and it has never engaged on
a real take because real takes on this machine are 0.3 to 9.3 s. Streaming is
therefore not the lever for short-take latency; see
`2026-10-27-release-to-paste-budget-conditions.md` for where the gap actually sits.
