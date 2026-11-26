# Plan 009: Close the mid-take idle-unload race

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-inference/src/manager.rs
> If that file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-25

## Why this matters

BUG-003's acceptance criterion is "never unload the speech model mid-dictation". The 5-second tick thread and the hotkey path still race: tick() checks active under one mutex, reads armed_until under a different mutex, then calls unload() with no lock held. A dictation can begin between the check and the unload, so the model is freed while the take is in flight. The fix is to hold the active guard across the decision and the unload, so begin_dictation() cannot win that window.

## Current state

- crates/teletype-inference/src/manager.rs:113-120 — SpeechModelManager fields: provider: Mutex<P>, model_path: Mutex<PathBuf>, active: Mutex<bool>, armed_until: Mutex<Option<Instant>>.
- :183 — pub fn lock<'a, T>(&self, m: &'a Mutex<T>) -> MutexGuard<'a, T> (poison-tolerant). All fields use this helper.
- :225-228 — pub fn unload(&self) { self.lock::<P>(&self.provider).unload(); *self.lock::<Option<Instant>>(&self.armed_until) = None; }
- :234-237 — begin_dictation(): sets active = true, then clears armed_until in a separate lock acquisition.
- :243-245 — end_dictation(): sets active = false, then arms armed_until.
- :252-262 — the defect:

      pub fn tick(&self) {
          if *self.lock::<bool>(&self.active) {
              return; // never unload mid-dictation
          }
          let due = self
              .lock::<Option<std::time::Instant>>(&self.armed_until)
              .map(|t| t <= Instant::now());
          if due == Some(true) {
              self.unload();
          }
      }

- Lock order today is active -> armed_until in begin/end/tick, and provider -> armed_until in unload; no path takes them in the opposite order.
- Tests: mod unload_timer_tests starts near :280; existing tests include unload_does_not_fire_during_active_dictation (:385), unload_fires_after_policy_duration (:398), never_policy_never_unloads (:411), next_dictation_after_unload_reloads (:424), end_dictation_arms_unload_that_tick_fires (:445). MockProvider / TaggedProvider doubles are already defined there.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Inference crate tests | cargo test -p teletype-inference | exit 0 |
| Full suite | cargo test --workspace | exit 0 (run sequentially, not alongside other test jobs — timing tests are load-sensitive) |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope:
- crates/teletype-inference/src/manager.rs (tick() plus a new test in unload_timer_tests)

Out of scope (do NOT touch):
- begin_dictation / end_dictation / unload bodies — the fix is in tick()'s lock lifetime.
- The field layout of SpeechModelManager (do not merge the mutexes; that is a larger refactor).
- crates/teletype-desktop/src/dictation.rs (the hotkey call site is correct).
- Any timing constant, tick interval, or unload policy.

## Git workflow

- Branch: advisor/009-idle-unload-race
- Commit message style: fix(inference): hold the active guard across idle unload
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Hold the active guard across the unload decision

Rewrite tick() at :252-262 so the guard returned by lock::<bool>(&self.active) is bound to a local and lives until after unload():

    pub fn tick(&self) {
        let active = self.lock::<bool>(&self.active);
        if *active {
            return; // never unload mid-dictation
        }
        let due = self
            .lock::<Option<std::time::Instant>>(&self.armed_until)
            .map(|t| t <= Instant::now());
        if due == Some(true) {
            self.unload();
        }
        drop(active);
    }

Why this is safe: begin_dictation() must acquire the same active mutex before it can set the flag, so it cannot run while tick() is unloading. Lock order remains active -> armed_until -> provider, matching the existing paths; no path takes them in reverse order.

Verify: cargo check -p teletype-inference -> exit 0. Run all existing unload tests: cargo test -p teletype-inference unload_timer_tests -> all pass.

### Step 2: Add a deterministic race test

In unload_timer_tests, add a provider double BlockingProvider with fields started: Sender<()> and release: Receiver<()>, implementing UnloadableProvider. Its unload() sends () on started, blocks on release.recv(), then marks itself unloaded.

Add test begin_dictation_cannot_overlap_unload:
1. Build a manager around BlockingProvider, load it, arm an immediate deadline with end_dictation(Some(Duration::ZERO)).
2. Spawn thread A calling manager.tick().
3. Receive the started signal (recv with a generous 5 s timeout — this is a sequencing wait, not a perf assertion).
4. Spawn thread B that signals "begin called" then calls manager.begin_dictation(), then signals "begin done".
5. Assert begin_done does NOT arrive within 300 ms (recv_timeout) — with the fix, B is blocked on the active guard held by tick().
6. Release thread A's unload, join both threads.
7. Assert the model is unloaded (unload completed before the dictation began), and that a subsequent tick() with active set cannot unload again.

Use std::sync::mpsc channels and scoped threads in the style of the existing tests. If step 5 proves flaky under load, increase the timeout rather than weakening the assertion — a false PASS is preferable to a false FAIL here; the underlying lock semantics are what the test checks.

Verify: cargo test -p teletype-inference begin_dictation_cannot_overlap_unload -> 1 passed. Run it five times (for i in 1 2 3 4 5; do ...; done) -> all pass.

## Test plan

- New test: begin_dictation_cannot_overlap_unload (with BlockingProvider double).
- Existing tests to re-run: all of unload_timer_tests, especially unload_does_not_fire_during_active_dictation and end_dictation_arms_unload_that_tick_fires.
- Verification: cargo test -p teletype-inference -> all pass; cargo test --workspace -> all pass when run without competing test jobs.

## Done criteria

- [ ] grep -n 'let active = self.lock::<bool>(&self.active);' crates/teletype-inference/src/manager.rs shows the guard is bound and dropped after unload().
- [ ] cargo test -p teletype-inference -> all pass including the new test by name.
- [ ] cargo test --workspace -> exit 0 with zero failures.
- [ ] cargo fmt --all -- --check -> exit 0.
- [ ] Only manager.rs and plans/README.md are modified.

## STOP conditions

Stop and report if:
- The live tick()/field excerpts do not match.
- Another path locks provider -> active (reverse order) — that would make the new lock lifetime deadlock-prone.
- The new test deadlocks on unload release (report the thread state; do not remove the assertion).
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- Holding the active guard across unload means begin_dictation() can block for the duration of provider.unload() (milliseconds for these providers). That is intentional: the alternative is unloading a live take.
- If the fields are ever merged into one state mutex, this plan's guard becomes redundant — revisit it in the same change.
- Reviewers should scrutinize the new test's two-channel sequencing: a non-blocking provider would make it pass on the old code too.
