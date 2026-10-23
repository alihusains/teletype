# L051 — A test that shells out to deliver a signal silently fails on a loaded runner

- **ID:** L051
- **Date:** 2026-10-23
- **Agent:** Qoder (CI triage)
- **Area:** `crates/teletype-inference/src/server.rs` (`bounded_kill`, `reap_orphan`), startup reaper
- **Status:** verified (local 15x stress passed; CI failure timing matched the budget exactly; fix awaits a CI run)

## Problem

Two tests failed on `release/v0.2.7` CI and passed locally, repeatedly:

- `server::tests::bounded_kill_terminates_a_live_process`
- `server::tests::reap_orphan_kills_a_matching_orphan_and_removes_pid_file`

Same failure in two jobs: `Tests (inference)` and `Coverage gate (per-crate
floors)`. The Coverage job is especially misleading: its own floors passed, and
it went red only because it re-runs the inference suite under `cargo llvm-cov`.
A "coverage" red there is a test flake, not a coverage regression.

## Diagnosis that worked

The elapsed time, not the assertion message. `bounded_kill` has a fixed kill
budget; the CI log ran 88 passed in 7.83s, and the two failures appeared ~7 s
after the previous line (03:14:05 to 03:14:12). That is the whole budget burned
without the target ever dying. So the signal was never delivered, rather than
delivered and ignored.

## Root cause

`bounded_kill` sent SIGTERM and SIGKILL by spawning `/bin/kill`. Every signal
delivery therefore needed a successful `fork()` from the test process. On a
loaded GitHub runner, where the test binary is one of many processes under
process-table pressure, a fork can fail, and the polling loop ignored the
closure's `bool` return. The result: no signal, no error, budget exhausted, test
red. Locally the fork always succeeded, which is why it never reproduced.

This is a production bug, not a test bug: the startup reaper that cleans up an
orphaned `llama-server` runs exactly when the machine is loaded (right after a
crash). A reaper whose only failure mode is "silently does nothing" leaves the
orphan holding the port.

## Fix

Deliver the signal in-process (`308cdeb`):

```rust
// SAFETY: `kill(2)` only reads two integer arguments; an invalid or
// already-exited pid is a no-op returning -1 with ESRCH.
unsafe { libc::kill(pid as libc::pid_t, signum) == 0 }
```

with `libc` added under `[target.'cfg(unix)'.dependencies]` (no new external
crate; 0.2.189 was already resolved). The module gate stays `#[cfg(unix)]`, not
`#[cfg(target_os = "macos")]`, because `bounded_kill` is unix-wide.

## Hypotheses ruled out first

Both were worth testing because the surrounding code looks suspicious:

1. Zombie state detection. `running()` polls `ps -p <pid> -o state=`. A reaped
   child does report `Z`, so the check was correct as written.
2. `/bin/sh` exec-ing away the argv markers that `reap_orphan` matches.
   Empirically the `ps -o command=` markers stayed stable, so the match logic was
   not the cause.

Both were left unchanged. Note the direction the two failures would have
produced: `ps` spawn failure errs toward "not running" and "do not reap", which
makes them safe under the same fork pressure. Only the kill path was load-sensitive.

## Rule

- Anything that must send a signal should use `libc::kill`, not
  `std::process::Command::new("/bin/kill")`. A signal is not an operation you
  are allowed to make contingent on being able to spawn a helper.
- Never ignore a `bool` return from a best-effort kernel call in a poll loop; it
  is the only place a fork/exec failure can become visible.
- When a CI-only test flake has a suspicious elapsed time, compare it to a known
  timeout budget before reading assertions. Matching the budget means the wait
  expired; that is a different fix class than a wrong assertion.
- Two jobs running the same suite both went red, so a per-job "flaky test" label
  was never on the table. If only one job had failed, the elapsed-time check
  would still have been the discriminator.

## Evidence

`gh run view 39893783804 --log-failed`: both names FAILED in `Tests
(inference)` and `Coverage gate (per-crate floors)`, `88 passed; 2 failed`,
`finished in 7.83s`. Local stress with `--test-threads=2`, 15 repetitions: all
green before and after the change.
