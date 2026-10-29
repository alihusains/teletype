---
id: L087
title: Quit-time history loss and silent persist-actor death, fixed with a join barrier
status: candidate
type: debugging
area: teletype-desktop / persist actor + app teardown
agent: opencode
tool: opencode
date: 2026-10-29
---

# Quit-time history loss and silent persist-actor death, fixed with a join barrier

## Problem

The background persistence actor (`crates/teletype-desktop/src/persist.rs`)
coalesces writes on a 250 ms tick and batches up to 64 messages. Two behaviours
around it were wrong:

1. `PersistHandle::shutdown()` only sent `PersistMsg::Shutdown`. It did not wait
   for the writer thread to drain, so anything still queued at quit was lost.
2. `PersistActor::run()` tried `PersistStores::open` once; on failure it logged a
   warning and returned, killing the actor for the whole session.

## Symptoms

- The last one or two dictations of a session are missing from History and the
  Insights counts after a restart. Nothing is logged - the messages were simply
  never applied.
- `AppState::drop` slept 500 ms before returning, a blind guess at "how long a
  flush takes", which is both a stall on the main thread and still not a
  guarantee.
- Two unit tests (`coalescing_lands_all_messages`, `remove_and_reset_are_applied`)
  worked around the fire-and-forget shutdown by polling the database for up to
  10 seconds. Under CPU load, `coalescing_lands_all_messages` failed
  intermittently while the code was correct; the test was encoding the race.

## Root cause

There was no completion signal between the writer thread and the caller. The
actor had a `Receiver<()>` it could send on exit, but `PersistHandle` had no
field to hold it, so `shutdown()` had nothing to wait on and `Drop` had to
substitute a sleep.

The retry gap is separate: SQLite open can fail transiently at launch (a lock
held by the previous process still exiting, a WAL checkpoint in flight), and a
single attempt turned a recoverable 200 ms condition into a silent permanent
loss of history for the session.

## Fix

- `PersistHandle` gained `done: Arc<Mutex<Option<mpsc::Receiver<()>>>>`. `spawn`
  creates the channel, moves the sender into the thread (`run(rx, &dir); let _ =
  done_tx.send(());`) and stores the receiver in the handle.
- `shutdown()` sends `Shutdown`, then takes the receiver out of the slot and
  joins with `recv_timeout(SHUTDOWN_WAIT)` (2 s). `Disconnected` counts as
  success (the thread exited without signalling). A real timeout logs a warning
  naming the possible loss. The slot makes it idempotent: the quit path calls
  `shutdown` twice (the Exit menu hook, then `AppState::drop`), and the second
  call must return immediately rather than wait behind a barrier that has
  nothing left to wait for.
- `run()` retries the open `OPEN_ATTEMPTS` (3) times with `OPEN_RETRY_WAIT`
  (200 ms) between, warns per failed attempt, and on exhaustion logs
  `tracing::error!` that history will not persist this session.
- `AppState::drop` no longer sleeps; the join barrier replaced it.
- Both polling tests became direct assertions after `handle.shutdown()`, and
  `a_second_shutdown_does_not_wait_again` pins the idempotency (second call
  returns in under a second and history has still landed).

## Verification

- `cargo test -p teletype-desktop --lib persist`: 5 passed, including the new
  idempotency test.
- `cargo test -p teletype-desktop --lib`: 425 passed, 0 failed.
- `cargo clippy --workspace --all-targets`: exit 0.
- `cargo test --workspace`: exit 0, no failures, no load-dependent flake
  reproduction after the change.
- Not yet verified: the loss itself has no live repro. It needs a quit inside
  the coalescing window; the join barrier is the mechanism, demonstrated by the
  tests now asserting directly instead of polling.

## Reusable lesson

An async writer that owns the last piece of user data needs a join signal, not a
sleep in the teardown path. If a test polls wall-clock for the effect to appear,
the test is encoding a race and will fail under load long after the code is
correct; give the production path a real completion event and let the test assert
directly. And a one-shot open of an OS-backed store (SQLite, keychain, audio
device) must retry a few times: a transient lock at launch is routine, and
silently giving up turns it into a session-long data loss.
