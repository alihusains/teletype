# Plan 010: Fail loudly when the DFN3 stream cannot be rebuilt

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-core/src/audio/mod.rs
> Note: plan 004 changes three logging lines in this file. If those changes are already present, that is expected drift — verify only the DFN3 rebuild excerpt below still matches. On any other divergence, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plan 004 (audio logging) is independent but touches the same file; merge 004 first, or apply this plan on top of it.
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

With noise reduction enabled, audio capture builds a first stream with no frame hook (the hook is meant to live in the rebuilt DFN3 stream). If the detection worker fails to spawn or the rebuilt stream cannot be created, the code only logs a warning and keeps the hook-less first stream. The take then captures audio but never feeds on_frame, so VAD auto-stop, silent-tail cutting, mid-take commits and the recovery spool are all silently dead while the UI shows a normal recording. Converting this to an explicit capture failure uses the existing mic-failure pill state and turns a silent broken take into a visible, recoverable one.

## Current state

- crates/teletype-core/src/audio/mod.rs:570-577 — the first build intentionally receives no hook when dfn3 is present:

      let first_build_hook = if dfn3.is_some() {
          None
      } else {
          on_frame.take()
      };

- :667-684 — the DFN3 branch takes on_frame into the detection worker and installs a channel-send callback in the rebuilt stream:

      let w_on_frame = on_frame.take();
      let worker = thread::Builder::new().name("teletype-dfn3-detect".into())
          .spawn(move || run_detection_worker(slot, frame_rx, w_level, w_on_frame)).ok();
      if worker.is_none() {
          tracing::warn!("dfn3 detection worker spawn failed");
      }
      let w_cb: FrameCb = Some(Box::new(move |mono: &[f32]| { let _ = frame_tx.send(mono.to_vec()); }));

- :766-774 — the silent-failure branch:

      match rebuilt {
          Some(s) => stream = s,
          None => {
              tracing::warn!("dfn3: could not rebuild the input stream");
          }
      }

- :777 — the normal error path already exists: if let Err(e) = stream.play() { let _ = ready_tx.send(Err(format!("Couldn't start microphone: {e}"))); return; } and :780 sends Ok(sample_rate) on success. ready_tx is in scope in this function (see :519).
- FrameCb = Option<Box<dyn Fn(&[f32]) + Send + Sync + 'static>> (:835).
- Repo convention: device-start failures go through ready_tx.send(Err(String)) and the pill shows a mic-failure state; keep messages user-facing and plain.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Core tests | cargo test -p teletype-core | exit 0 |
| Full suite | cargo test --workspace | exit 0 (run sequentially; timing tests are load-sensitive) |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope:
- crates/teletype-core/src/audio/mod.rs — the worker-spawn failure branch (:672-674), the rebuilt-stream failure branch (:766-774), and one small pure helper near them.

Out of scope (do NOT touch):
- The Off-mode (no DFN3) path, the first build hook selection at :570-577, run_detection_worker, resampling, and any other audio module.
- Plan 004's three logging lines — do not revert or re-edit them.
- Any mic-failure UI code in ui/.

## Git workflow

- Branch: advisor/010-dfn3-rebuild-failure
- Commit message style: fix(audio): report DFN3 stream setup failures instead of running hookless
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add a small message helper

Near the DFN3 code, add:

    fn dfn3_start_error(reason: &str) -> String {
        format!("Couldn't start the microphone with noise reduction ({reason}); try turning noise reduction off")
    }

Keep the message plain and user-facing. This is the only new function.

Verify: cargo check -p teletype-core -> exit 0.

### Step 2: Turn both silent failures into ready_tx errors

In the worker-spawn branch at :672-674, replace the warn-and-continue with:

    if worker.is_none() {
        let _ = ready_tx.send(Err(dfn3_start_error("detection worker did not start")));
        return;
    }

In the rebuilt-stream match at :766-774, replace the warn-and-continue with:

    match rebuilt {
        Some(s) => stream = s,
        None => {
            let _ = ready_tx.send(Err(dfn3_start_error("the audio stream could not be rebuilt")));
            return;
        }
    }

Returning before stream.play() means no hook-less stream is ever started and no orphan worker is left receiving frames (dropping w_cb closes the channel).

Verify: grep -n 'dfn3: could not rebuild|worker spawn failed' crates/teletype-core/src/audio/mod.rs -> no matches; grep -n 'dfn3_start_error' -> three occurrences (definition + two calls). cargo check -p teletype-core -> exit 0.

### Step 3: Unit-test the helper and re-run audio tests

In the existing tests module of audio/mod.rs, add test dfn3_start_error_names_the_setting: assert the returned string contains "noise reduction" and the supplied reason. Model it on the other small pure-function tests in that module.

Verify: cargo test -p teletype-core dfn3_start_error_names_the_setting -> 1 passed; cargo test -p teletype-core -> all pass.

## Test plan

- New test: dfn3_start_error_names_the_setting (pure string helper).
- Existing audio tests must remain green; the cpal stream paths require hardware and are not unit-testable, so the branch change is verified by grep plus the existing suite.
- Verification: cargo test -p teletype-core and cargo test --workspace -> all pass.

## Done criteria

- [ ] Both former warn-and-continue branches now send an Err through ready_tx and return.
- [ ] grep -n 'dfn3_start_error' shows definition plus two call sites; no old warning strings remain.
- [ ] cargo test -p teletype-core -> all pass including the new test by name.
- [ ] cargo test --workspace -> exit 0.
- [ ] Only audio/mod.rs and plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live excerpts at :570-577, :667-684 or :766-774 do not match.
- ready_tx is not in scope at either failure branch (report the compiler error; do not restructure the capture function).
- Plan 004's changes are absent AND you cannot apply this plan without reverting them.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- This deliberately trades a rare failed capture for a silent broken take. If users report that DFN3 rebuild failures are common on a device, the right follow-up is to make the rebuild reliable, not to reinstate the hook-less stream.
- If a future DFN3 change keeps the first stream alive after a successful rebuild, the error path must also stop that stream — today it has not been played, so returning is sufficient.
- A reviewer should confirm the two returns happen before stream.play(), so no half-configured stream is ever started.
