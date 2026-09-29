# BUG-013 — RunEvent::Exit shutdown is fire-and-forget; child can outlive the app (G008)

**Severity:** P2 (resource leak)
**Area:** desktop / lib.rs + inference / server.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `lib.rs:579-599` and `server.rs:128-152`

---

## Description

The `RunEvent::Exit` hook (`lib.rs:579-599`) takes the provider out of the
state and then does:

```rust
drop(tauri::async_runtime::spawn_blocking(move || provider.shutdown()));
```

The `JoinHandle` is intentionally dropped, so **nothing joins the task**.
Tauri tears down the tokio runtime immediately after the run callback
returns. A `kill()` + `wait()` on a 2.7 GB Metal-backed `llama-server`
process is not guaranteed to finish before teardown. If the blocking task is
dropped mid-`wait()`, the `SIGKILL` may never be delivered and the pid file
is never removed.

Symptom: exactly the G008 symptom — an orphaned `llama-server` after Quit.
The existing mitigation (pid file + `kill_stale_servers` on next
`warm_up`) only reaps the **same model's** next warm-up; if the user
switches models or quits twice, the orphan survives.

### Second related window (F6)

`kill_stale_servers` (`server.rs:128-152`) only reaps the pid recorded in
`pid_file`. If the previous run was killed between `spawn()` and
`fs::write(pid_file)` at `:152`, no pid is recorded — so the next launch
can't find the orphan and spawns a **second** server for the same model.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/lib.rs` | 579-599 | fire-and-forget shutdown on Exit |
| `crates/teletype-inference/src/server.rs` | 128-152 | `kill_stale_servers` (pid-file only) |
| `crates/teletype-inference/src/server.rs` | 152 | `fs::write(pid_file)` — the window after `spawn()` |

## Reproduction

1. Load a local model.
2. Cmd-Q the app **during** the 120 s warm-up (between `spawn()` and the
   pid file write).
3. `pgrep -fl llama-server` — an orphan exists.
4. Relaunch, select a **different** model — the orphan for the first model
   is never reaped.

## Unit test cases (must pass after fix)

```rust
// 1. The Exit hook must confirm the child is dead before returning.
//    (Either join the shutdown task or block until the child pid is gone.)
#[test]
fn exit_hook_does_not_return_before_child_dead() {
    // Spawn a real (or mocked) child, invoke the exit-shutdown path,
    // then assert the pid no longer exists before the function returns.
    let child = start_test_server();
    exit_shutdown(&child);
    assert!(!pid_alive(child.pid()),
        "exit hook returned while the llama-server child was still alive");
}

// 2. A crash between spawn() and pid-file write must still be recoverable:
//    kill_stale_servers must find the orphan even when no pid file exists.
#[test]
fn stale_kill_recovers_orphan_without_pid_file() {
    let orphan = spawn_server_without_pid_file("model-a.gguf");
    std::fs::remove_file(pid_file_path("model-a")).ok();
    kill_stale_servers("model-a");
    assert!(!pid_alive(orphan.pid()),
        "orphan with no pid file was not reaped");
}
```

## Acceptance criteria

- [ ] No orphan `llama-server` after a clean Quit.
- [ ] No orphan after a kill during warm-up.
- [ ] `kill_stale_servers` finds an orphan even when the pid file was never
      written.

## How to test (manual / smoke)

1. **Smoke (1 min):** Quit the app normally, then
   `pgrep -fl llama-server` shows zero Teletype-owned servers.
   - **Pass:** no Teletype `llama-server` process remains.
   - **Fail:** an orphaned `llama-server` is still running.

2. **Regression (2 min):** kill the app mid-warm-up (Cmd-Q during the 120 s
   warm-up window), relaunch, select the **same** model.
   - **Pass:** only one `llama-server` process exists afterwards.
   - **Fail:** two servers for the same model, or the old one survives.

## Fix direction

In the Exit hook, **join** the shutdown task (block on the `JoinHandle`)
with a bounded timeout, and as a last resort kill the child by pid
directly before returning:

```rust
let handle = tauri::async_runtime::spawn_blocking(move || provider.shutdown());
// block up to N seconds for the child to die, then SIGKILL by pid
```

For the pid-write window, write the pid file **before** (or
atomically-with) `spawn()`, and have `kill_stale_servers` also scan for
`llama-server` processes whose command line references the known model
paths — not just the pid file.

## Related

- G008 (orphaned `llama-server`, cause previously unconfirmed — this is the
  likely cause)
- BUG-007 (`select_model` ordering)
