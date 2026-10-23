# L051 — `ps -o state=` prints `Z+` under a pty, so compare the state letter

- **ID:** L051
- **Date:** 2026-10-23
- **Agent:** Qoder (CI triage); diagnosis corrected by OpenCode after a pty reproduction
- **Area:** `crates/teletype-inference/src/server.rs` (`is_live_state`, `bounded_kill`, `reap_orphan`)
- **Status:** verified locally (pty reproduction and pty test run); CI run pending
- **Supersedes:** the first version of this learning, which blamed fork pressure on a loaded runner. That diagnosis was wrong: the signal was delivered, but the poll loop never accepted that the target had died.

## Problem

Two tests failed on `release/v0.2.7` CI and passed locally, repeatedly:

- `server::tests::bounded_kill_terminates_a_live_process`
- `server::tests::reap_orphan_kills_a_matching_orphan_and_removes_pid_file`

Same failure in two jobs: `Tests (inference)` and `Coverage gate (per-crate
floors)`. The Coverage job is especially misleading: its own floors passed, and
it went red only because it re-runs the inference suite under `cargo llvm-cov`.
A "coverage" red there is a test flake, not a coverage regression.

## Diagnosis that worked

1. **Elapsed time matched the kill budget.** `bounded_kill` has a fixed budget
   (5 s SIGTERM + 2 s SIGKILL). The CI log ran `88 passed; 2 failed; finished in
   7.98s`, and the two failures appeared ~7 s after the previous line. The wait
   expired; the target was never observed dying.
2. **The remaining difference was the terminal.** CI runs the step under a pty;
   a local `cargo test` does not. Reproduce with:

   ```bash
   script -q /dev/null cargo test -p teletype-inference
   ```

   Both tests fail there, with the same ~8 s elapsed time.
3. **`ps` output differs.** Inside a pty, `ps -p <pid> -o state=` prints `Z+`
   for a foreground zombie. Without a tty it prints `Z`.

## Root cause

`running()` compared the whole trimmed string:

```rust
!state.is_empty() && state != "Z" && state != "z"
```

`Z+` is not `Z`, so a zombie counted as alive. The sequence in the test:

1. `bounded_kill` sends SIGTERM; `sleep` dies but the test has not `wait()`ed it,
   so it is a zombie.
2. `ps` reports `Z+` under the pty; the old check says "still running".
3. The 5 s SIGTERM poll expires, SIGKILL is sent to a zombie (harmless).
4. The 2 s SIGKILL poll expires, `bounded_kill` returns `false`, the assertion
   fails — 7 s after the test started.

Locally the same zombie reports `Z`, the old check passed, and nothing
reproduced. That is the whole CI-only difference.

The earlier in-process `libc::kill` change (`308cdeb`) was already in the failing
run (`a4a29ec`, run `39904457565`) and did not fix these tests. Confirm that with
`git merge-base --is-ancestor 308cdeb a4a29ec`.

## Fix

Parse the leading state letter only, in a pure helper so the parsing is testable
without a pty:

```rust
fn is_live_state(ps_state: &str) -> bool {
    match ps_state.trim().chars().next() {
        None => false,
        Some(c) => !matches!(c, 'Z' | 'z' | 'X' | 'x'),
    }
}
```

`X` is BSD `ps`'s "dead" state and is treated like a zombie. The regression test
`server::tests::ps_state_parsing_counts_zombies_and_dead_as_gone` covers `""`,
`"Z"`, `"Z+"`, `"z"`, `"X"`, `"X+"` and the live forms `"R"`, `"R+"`, `"S"`,
`"S+"`, `"Ss"`, `"D"`, `"I"`.

`bounded_kill` keeps in-process signal delivery (no `/bin/kill` fork), plus a
`pid_t::try_from` guard: a pid above `i32::MAX` would wrap negative, and
`kill(-n, ...)` signals a whole process group. That guard is hardening, not the
CI fix.

## Verification

- Before: `script -q /dev/null cargo test -p teletype-inference` → the two tests
  fail; `88 passed; 2 failed; finished in 8.10s`.
- After: the same command → `91 passed; 0 failed; 1 ignored; finished in 5.81s`
  (91 = the original 90 plus the new parser test), with both t7 tests passing.
- `cargo clippy -p teletype-inference -p teletype-speech --all-targets -- -D
  warnings` exit 0; `cargo fmt --all -- --check` exit 0.
- CI confirmation is still pending at the time of writing.

## Hypotheses ruled out

1. **Fork pressure on a loaded runner.** Plausible, and it motivated the
   in-process `libc::kill` change, but that change was present in the run that
   still failed. The elapsed-time evidence only says "the loop expired"; it
   cannot distinguish "no signal was sent" from "the signal was sent and the
   death was not recognized". Only reproducing the pty distinguished them.
2. **`/bin/sh` exec-ing away the argv markers that `reap_orphan` matches.** The
   `ps -o command=` markers stayed stable, so the match logic was not the cause.

## Rule

- Parse `ps` state fields by leading letter, never by whole-string equality.
  A pty appends flags (`+`, `s`, ...) that a terminal-less local run omits.
- When a test is CI-only, reproduce the *execution environment* (pty, env vars,
  cwd, stdin) before theorizing about load. `script -q /dev/null <cmd>` is the
  cheapest pty simulation on macOS and Linux.
- Put environment-dependent parsing behind a pure function and unit-test the
  raw strings (`Z+`), so the regression is caught without the environment that
  exposed it.
- Before accepting a fix for a CI-only failure, check whether the failing run
  actually contained it (`git merge-base --is-ancestor <fix> <failed-head>`).
  Otherwise you are guessing about which change moved the needle.
