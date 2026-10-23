# L-2026-10-11-001: Mutex self-deadlock in polish_candidate_accept test

**Date:** 2026-10-11
**Agent:** Codex (inline)
**Area:** crates/teletype-desktop/src/commands.rs (tests)
**Status:** verified

## Problem

`polish_candidate_accept_writes_provenance_and_counts_once` appeared to "hang"
indefinitely. No timeout, no output, the test binary sat at 100% CPU-free in
`__psynch_mutexwait`.

## Symptoms

- `cargo test -p teletype-desktop --lib polish_candidate_accept` never returned.
- `sample` showed only 2 threads: main (waiting in `run_tests_console`) and the
  test thread (blocked on `Mutex::lock` at `commands.rs:5142`).
- No other process or thread held the lock.

## Initial hypotheses

1. Stale orphaned `cargo test` processes contending on `target/` (from prior sessions).
2. A background thread spawned by `AppState::for_tests()` holding the lock.
3. A poisoned mutex from a prior test in the same binary.

## Investigation

- Killed all stale processes; the hang persisted on a fresh `cargo clean -p teletype-desktop` rebuild.
- `sample` confirmed only 2 threads existed — no hidden background thread.
- A minimal probe test (`drop(state.polish_candidates.lock()...)` x3) passed instantly.
- Re-read the failing test source line-by-line.

## Root cause

The test acquired `state.polish_candidates.lock()` into a `let store = ...`
binding for an assertion, then **in the same scope** tried to acquire the same
mutex again with `let mut store = state.polish_candidates.lock()`. The first
`MutexGuard` was still alive (not dropped), so the second `.lock()` blocked
forever — a classic single-thread mutex self-deadlock.

The earlier "stale process" diagnosis was a red herring: the processes were
indeed stale, but even after killing them the test still hung because the
binary contained the buggy test code.

## Fix

Wrapped each lock acquisition in its own `{ }` block so the guard is dropped
before the next acquisition:

```rust
{
    let store = state.polish_candidates.lock()...;
    assert_eq!(store.outcomes.accepted, 1);
}
{
    let mut store = state.polish_candidates.lock()...;
    let mut dict = state.dictionary.lock()...;
    // second accept attempt
}
```

## Verification

- `cargo test -p teletype-desktop --lib polish_candidate_accept_writes_provenance` passes in 0.00s.
- Full suite: 106 passed, 0 failed.

## Reusable lesson

When a Rust test "hangs" with no output and `sample` shows a single test
thread blocked on `__psynch_mutexwait` with no other threads, suspect a
**self-deadlock**: the same thread holds a `MutexGuard` and tries to re-lock
the same mutex in the same scope. Always scope guards in `{ }` blocks when
the same mutex is acquired multiple times in one test.
