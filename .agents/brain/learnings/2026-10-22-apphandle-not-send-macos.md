# L-039 — AppHandle is not Send on macOS; a SIGTERM handler needs an AtomicPtr

- **ID**: L-039
- **Date**: 2026-10-22
- **Agent**: pi (t7-sigterm)
- **Area**: `teletype-desktop` (macOS, tauri 2.12.0 / tao 0.37.1)
- **Status**: verified
- **Evidence**: `crates/teletype-desktop/src/sigterm.rs`; `cargo build -p teletype-desktop --lib` E0277 at `lib.rs:878`

## Problem

t7 needed a SIGTERM handler that calls `AppHandle::exit(0)`. The obvious designs both fail to compile on macOS:

1. `static HANDLER: Mutex<Option<Box<dyn Fn() + Send + Sync>>>` — `AppHandle` is **not `Send`**, so the closure cannot be stored in a `Send + Sync` box.
2. `std::thread::Builder::new().spawn(move || { app.handle().clone() … })` — same reason; you cannot move an `AppHandle` into a spawned thread.

## Root cause

On macOS, `tauri::AppHandle` wraps `tao::EventLoopWindowTarget`, which is an `Rc<…>` (not `Arc`). `Rc` is not `Send`. So `AppHandle` is not `Send` on macOS, even though Tauri documents it as "safe to call from any thread" (that refers to the *methods*, which are internally synchronized — not to moving the handle across threads).

## Fix

Store the handle as a raw pointer in an `AtomicPtr<AppHandle>` (which **is** `Send + Sync` because raw pointers are untyped). The signal handler dereferences it on the **main thread** (where `sigaction` dispatches on macOS), so the reference is valid for the process lifetime. This is the one legitimate `unsafe` in the desktop crate outside the build script.

```rust
static APP: AtomicPtr<AppHandle> = AtomicPtr::new(ptr::null_mut());

fn signal_handler(_: i32) {
    let p = APP.load(Ordering::SeqCst);
    if !p.is_null() {
        unsafe { (*p).exit(0) }; // main thread, valid for process lifetime
    } else {
        process::exit(1);        // pre-setup fallback
    }
}
```

## Reusable lesson

- **Do not** try to move a `tauri::AppHandle` into a `std::thread` on macOS — it will not compile (`E0277`).
- **Do not** store it in a `static Mutex<Option<AppHandle>>` for a signal handler — same reason.
- Use an `AtomicPtr` + a documented main-thread `unsafe` deref, and only call the **channel-based** methods (`exit`, `emit`) from the handler — they are async-signal-safe enough in practice.
- **`kill -0` is not a liveness probe for a child you're about to reap** — it returns success for zombies. Use `ps -p <pid> -o state=` and treat `Z`/empty as gone. This bit me in `bounded_kill` (first version waited the full 5 s and reported the killed process as still alive).

## Related

- D009 (no C++ static teardown at exit) — the teardown order `mark_teardown` must be last is unchanged; this learning is about *reaching* that teardown from a signal, not about the order.
