# Plan 004: Remove raw stderr writes from audio stream error callbacks

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-core/src/audio/mod.rs
> If that file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-24
- **Revision 1 (2026-11-24)**: scope expanded from one callback to all three raw-stderr audio callbacks after executor review found two more of the same class.

## Why this matters

The audio capture path has three cpal stream error callbacks that write to raw stderr with eprintln!. On a relaunched app whose inherited stderr is closed (Gotcha G002 in PROJECT_BRAIN.md), writing from an audio thread can abort the process exactly when the microphone device fails — turning a recoverable device error into a crash during dictation. The first stream callback already uses tracing::warn!; the three raw writers are the rebuilt DFN3 stream (:690), the first prewarm stream (:1268), and the rebuilt prewarm stream (:1373). All three are the same defect class in the same file.

## Current state

- crates/teletype-core/src/audio/mod.rs:563-566 — first stream callback, the exemplar to match:

      let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> = Box::new(move |err| {
          tracing::warn!("[audio] stream error: {err}");
          err_flag.store(true, Ordering::SeqCst);
      });

- :688-692 — rebuilt DFN3 stream callback (defect 1):

      let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> = Box::new(move |err| {
          eprintln!("[audio] stream error: {err}");
          err_flag.store(true, Ordering::SeqCst);
      });

- :1266-1269 — first prewarm stream callback (defect 2):

      let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> =
          Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"));

- :1371-1374 — rebuilt prewarm stream callback (defect 3), same shape and message as :1268.
- Repo convention: PROJECT_BRAIN.md Gotcha G002 — in dictation paths use log_line()/tracing, never raw eprintln!, because closed stderr on relaunch can abort the app. :564 is the in-file exemplar.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Core tests | cargo test -p teletype-core | exit 0 |
| Full suite | cargo test --workspace | exit 0 |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope:
- crates/teletype-core/src/audio/mod.rs — exactly three eprintln! calls at the current lines 690, 1268 and 1373.

Out of scope:
- The first stream callback (already correct), the error-flag logic, the frame-hook / DFN3 split, and any other file.
- No change to error messages, field names, or public API.

## Git workflow

- Branch: advisor/004-audio-stream-logging
- Commit message style: fix(audio): use tracing for all stream error callbacks
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Replace all three raw prints

At :690, change eprintln!("[audio] stream error: {err}"); to tracing::warn!("[audio] stream error: {err}"); so it is identical to :564. Keep the err_flag.store(true, Ordering::SeqCst); line and the bug comment unchanged.

At :1268 and :1373, change Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}")) to Box::new(move |err| tracing::warn!("[audio] prewarm stream error: {err}")). Keep the messages and surrounding structure exactly as they are.

Verify: grep -rn 'eprintln!' crates/teletype-core/src/audio/ -> no matches. Then cargo check -p teletype-core -> exit 0.

### Step 2: Verify the invariant repo-wide

Confirm there is no other raw stderr write in the capture path:

    grep -rn 'eprintln!' crates/teletype-core/src/audio/ crates/teletype-desktop/src/dictation.rs

Expected: no matches in audio/; any hits outside this file must be reported, not edited.

Verify: cargo test -p teletype-core -> exit 0.

## Test plan

- No new unit test: logging swaps have no behavior branch to exercise. The machine-checkable guard is the grep in step 1.
- Existing audio tests (cargo test -p teletype-core) must remain green.
- Verification: grep-based done criterion plus full core tests.

## Done criteria

- [ ] grep -rn 'eprintln!' crates/teletype-core/src/audio/ returns nothing.
- [ ] The DFN3 callback at :690 uses the identical tracing::warn! line as :564; both prewarm callbacks use tracing::warn! with their existing message.
- [ ] cargo test -p teletype-core and cargo test --workspace exit 0.
- [ ] Only audio/mod.rs and plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live code at :563-566, :688-692, :1266-1269 or :1371-1374 does not match these excerpts.
- A fourth eprintln! appears in crates/teletype-core/src/audio/ — report its location instead of editing it.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- tracing::warn! on an audio callback must stay cheap: it allocates a formatted string, acceptable because these callbacks fire only on device errors, not per frame. Do not add logging to the per-frame path.
- If a future audio stream is added, its error callback must follow the :564 exemplar — this plan fixes the three known divergences, not every future one.
