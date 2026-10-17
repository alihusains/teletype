# P7.2 Observation Wiring — Evidence Report

**Task:** `tasks/pi-tasks/bw-c19-p72-observation-wiring.md`
**Branch:** `feat/beat-wispr`
**Date:** 2026-07-10
**Status:** Complete — all verification commands pass.

---

## Files changed (2, as scoped)

| File | Change |
|------|--------|
| `crates/teletype-desktop/src/edit_watch.rs` | Rewritten: added `PasteAnchor`/`ObserverState` glue, `record_watch`, `build_anchor`, `observe_once`, `poll_observer`, `LearningSink` trait, `StoreSink`, `encode_edit_ops`, 23 inline tests. |
| `crates/teletype-desktop/src/dictation.rs` | Wired: P6.4 `capture_screen_context` at hotkey-down; `record_dictation_row` at paste; `observe_learning_store` on next-dictation trigger; `poll_learning_observer` on 2 s prewarm tick; `cancel_learning_watch` on `go_idle`; `read_focused_field` helper. |

`crates/teletype-desktop/src/lib.rs` has 10 extra lines from parallel workers (c20/c23/self_test module declarations) — not this task's change.

---

## Part 0 — P6.4 screen-context capture

- `capture_screen_context()` called once in `start()` immediately after `record_target` is set (same block, same moment).
- Calls `read_focused_context(400, 100)`, classifies with `app_category(bundle_id, window_title)`.
- Secure-field text is dropped (only app info kept), per `FocusedContext::secure` contract.
- `None` on any failure — the take proceeds with empty context, never fails or delays.
- Stored on `Session.screen_context`; cleared in `go_idle`.

## Part 1 — DictationRow recording

- `record_dictation_row()` called from `pipeline_ready` at the paste site only (refused/cancelled paths never reach it).
- `app_id`/`app_category`: from `screen_context` if available, else from `payload.context`.
- `engine`: `settings.selected_speech_model`.
- `lang`: `effective_language(settings.language, false)`.
- `raw_asr`: post-pipeline text (pre-pipeline text is not carried on `PipelinePayload`; documented in code comment as v1 compromise per task note).
- `words_json`: `"[]"` (word timings are consumed by the confidence rescorer on the worker and not carried to the controller; documented in code comment).
- `polish_model`: `settings.selected_llm_model` when non-empty, else `None`.
- `polish_skipped`: `false` (controller does not know whether polish ran; the trace's `polish_skipped_reason` carries the truth; documented in code comment).
- Store opened lazily via `learning_store()`; failure logs once and disables learning for the session. Never blocks or fails a paste.

## Part 2 — Anchor at paste

- `focused_surroundings()` read **before** `platform.insert_text()` in `pipeline_ready`.
- `build_anchor(before, after, pasted)` clips to 40 chars per side via `clip_anchor`.
- `None` when surroundings are unreadable → observation skipped, never guessed.
- `record_watch(inserted, anchor, dictation_id, now_ms)` arms the watch + `ObserverState::Paste`.
- `set_target(target)` fills the app name after `record_watch`.

## Part 3 — Observe on triggers

Triggers wired (of the five in `ObserverEvent`):

| Trigger | Source | Status |
|---------|--------|--------|
| (d) Next dictation starts | `observe_pending_edit()` → `observe_learning_store()` | **Wired** |
| (c) 20 s idle | `poll_learning_observer()` via `PrewarmTick` (2 s) | **Wired** |
| (e) 3 min cap | `poll_learning_observer()` via `PrewarmTick` (2 s) | **Wired** |
| (a) Focus lost | No AX focus-change event source exists in the codebase | **Not wired** (documented below) |
| (b) Enter pressed | `fn_tap.rs` only reports Fn state; no Enter keycode surfaced | **Not wired** (documented below) |

**Periodic mechanism used:** the existing 2 s `PrewarmTick` thread (spawned in `Controller::spawn` when prewarm mode is not Off). `poll_learning_observer()` is called in the `Event::PrewarmTick` handler alongside `prewarm_tick()`. No second timer was added.

**Observation flow (`observe_once` / `poll_observer`):**
1. `ObserverState` receives the event/poll → `ObserverDecision`.
2. `Wait` → `None` (still observing).
3. `Drop` → `ObserveOutcome::Skipped`.
4. `Observe` → read field via `read_focused_field()` → `FieldRead::Readable/Unreadable`.
   - Unreadable → `FieldUnreadable` (dropped silently).
   - `locate_observed(field, anchor, max_dist=3)` → `None` → `NotLocated` (dropped, logged, never written).
   - `Some(located)` → `diff_words(inserted, located.observed)` → `encode_edit_ops` → `sink.set_observed(id, observed, ops_json)`.

**Cancel/error:** `cancel_learning_watch()` in `go_idle` feeds `ObserverEvent::Canceled` and clears the watch. No store row written.

**Replacement:** `record_watch` on a new paste replaces the previous watch (the existing `.take()` flow in `observe_pending_edit` handles the old one first).

---

## Tests

### edit_watch.rs (23 tests, all pass)

| Test | Covers |
|------|--------|
| `record_watch_stores_anchor_and_id` | Anchor + dictation_id stored in watch |
| `record_watch_without_anchor_keeps_hint_layer` | Watch works without anchor (hint only) |
| `an_empty_insertion_is_never_watched` | Empty insert → no watch |
| `an_expired_watch_is_dropped_rather_than_reported` | Expiry unchanged |
| `set_target_fills_the_app_name` | `set_target` fills the app name |
| `build_anchor_clips_to_40_chars_per_side` | Anchor clipping |
| `build_anchor_is_none_when_surroundings_unreadable` | No anchor when read fails |
| `observe_glue_records_when_located` | Decision→observe→write with fake field + sink |
| `observe_glue_drops_when_not_located` | `None` locate → nothing written |
| `observe_glue_unreadable_field_is_dropped_silently` | Unreadable field → dropped |
| `observe_glue_skips_without_anchor_or_id` | No anchor/id → skipped |
| `observe_glue_waits_until_a_trigger` | Keystroke → `Wait` |
| `poll_fires_idle_and_cap` | 20 s idle via poll; no double-observe |
| `cancel_drops_the_observation` | `Canceled` → `Drop` |
| 9 legacy diff tests | `diff`/`EditDiff` heuristics unchanged |

### dictation.rs (51 tests, all pass)

All existing dictation tests remain green. The headless-test pattern (`Controller::for_tests`) does not drive the real paste path (it uses `headless_flash: true` which skips platform services), so no new dictation-level tests were added for the paste/observe path — the unit tests in `edit_watch.rs` cover the glue with fake field readers and sinks.

---

## Verification (raw output)

```
$ cargo test -p teletype-desktop --lib edit_watch
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 152 filtered out; finished in 0.00s
EXIT: 0

$ cargo test -p teletype-desktop --lib dictation
test result: ok. 51 passed; 0 failed; 0 ignored; 0 measured; 124 filtered out; finished in 0.34s
EXIT: 0

$ cargo check -p teletype-desktop -p teletype-core
warning: `teletype-desktop` (lib) generated 5 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.90s
EXIT: 0

$ rustfmt --check --edition 2021 crates/teletype-desktop/src/edit_watch.rs crates/teletype-desktop/src/dictation.rs
EXIT: 0
```

The 5 warnings in `cargo check` are pre-existing (unrelated to this task).

---

## Notes / compromises (per task file instructions)

1. **`raw_asr`**: post-pipeline text used as v1 stand-in. The pre-pipeline ASR text is consumed by the confidence rescorer on the worker thread and is not carried on `PipelinePayload` to the controller. Documented in code comment.
2. **`words_json`**: `"[]"` — word timings are not carried to the controller. Documented in code comment.
3. **`polish_skipped`**: set to `false` (polish attempted) as the v1 default. The controller does not know whether polish actually ran; the trace's `polish_skipped_reason` carries the truth. Documented in code comment.
4. **Focus-lost trigger (a)**: no AX focus-change event source exists in the codebase. The five triggers in `ObserverEvent` include `FocusLost`, but there is no existing tap or notification that surfaces focus changes. Not wired; documented here per task instruction.
5. **Enter trigger (b)**: `fn_tap.rs` only reports Fn key state (down/up). No Enter keycode is surfaced by any existing tap. Not wired per task instruction ("Enter only if an existing tap surfaces it").
6. **`encode_edit_ops`**: `EditOp` in `teletype-core/src/learning/diff.rs` does not derive `Serialize`. The encoding lives in `edit_watch.rs::encode_edit_ops` — the only site that must serialise it — as a JSON array of `{op, words/from/to}` objects.
