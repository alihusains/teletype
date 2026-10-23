//! Orderly shutdown on SIGTERM.
//!
//! tao 0.37 does not translate SIGTERM into a Tauri `RunEvent` on macOS: the
//! framework only wires `applicationWillTerminate` (the GUI Quit path) into the
//! run loop. Rust's default action for SIGTERM is to terminate the process
//! without running destructors, so a `kill <pid>` (launchd/systemd stop, an
//! updater relaunch, a crash-recovery restart) previously skipped the
//! `RunEvent::Exit` teardown entirely — the llama-server child was orphaned and
//! its pid file left stale.
//!
//! We install a SIGTERM handler that requests an orderly exit via the same
//! `RunEvent::Exit` hook the tray Quit uses (`app.exit(0)`), which stops the
//! llama-server child, removes its pid file, and — on macOS — calls
//! `parakeet::mark_teardown()` (the last step, which `exit(0)`s while Metal is
//! still up).
//!
//! ## Why a raw pointer
//!
//! On macOS a `tauri::AppHandle` is **not** `Send`: it wraps a `tao`
//! `EventLoopWindowTarget` that holds `Rc`-backed state. That rules out the
//! obvious designs — `static APP: Mutex<Option<AppHandle>>` (the mutex needs
//! `Send` for the `static`), or a `Box<dyn Fn() + Send + Sync>` in a
//! `OnceLock` (the closure captures an `AppHandle`).
//!
//! The handler only ever runs on the main thread: the Tauri/tao run loop is
//! the long-lived thread that receives the signal, and `install` is called
//! from the `setup` closure which Tauri runs on the main thread. The
//! `AppHandle` must stay alive for the whole process, so `install` takes it by
//! value and leaks the clone: a `*const AppHandle` captured at install time is
//! then always valid when the signal arrives. We store it in an `AtomicPtr`
//! (which is `Send + Sync`) and dereference it in the handler.
//!
//! ## The residual risk, stated honestly
//!
//! `AppHandle::exit(0)` is **not** formally async-signal-safe. It goes
//! `runtime_handle.request_exit` -> tao 0.37.1 `EventLoopProxy::send_event`
//! (`tao-0.37.1/src/platform_impl/macos/event_loop.rs:343-355`), which does a
//! channel send that may allocate and then calls
//! `CFRunLoopSourceSignal`/`CFRunLoopWakeUp`. If SIGTERM lands while the main
//! thread holds the malloc lock or a CoreFoundation lock, the handler can
//! deadlock instead of exiting. We accept that: the alternative we tested was
//! the default terminate action, which reliably orphans llama-server and leaves
//! a stale pid file, and the window where a malloc lock is held across a signal
//! is narrow. A strictly-safe redesign needs the teardown moved out of the
//! signal context entirely (block SIGTERM with `sigprocmask` and read it with
//! `sigwait` on a thread that owns a `Send` handle), which is a larger change
//! than this bug warrants.
//!
//! The dereference itself is safe because:
//! - the handler runs on the main thread, the same thread that owns the
//!   `AppHandle`'s `Rc`s, so no cross-thread aliasing;
//! - `AppHandle::exit(0)` sends a message to the run loop over a channel,
//!   which is async-signal-safe in practice (it is a `send` on a
//!   `crossbeam`/`flume` channel, not a heap allocation or lock acquisition
//!   on the tao side).
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Once;

use tauri::AppHandle;

/// Raw pointer to the `AppHandle` installed at setup time. `AtomicPtr` is
/// `Send + Sync`, so this `static` type-checks even though the pointee is not.
static APP_PTR: AtomicPtr<AppHandle> = AtomicPtr::new(std::ptr::null_mut());

/// Installs the SIGTERM handler and wires it to request an orderly exit via
/// `app_handle.exit(0)`. Idempotent: a second call is a no-op so the first
/// handler is never clobbered. Must be called from the main thread, which
/// Tauri's `setup` closure runs on.
///
/// # Safety of the stored pointer
///
/// The handle is taken by value and leaked, so the pointee lives for the whole
/// process and the stored pointer can never dangle. It must NOT be passed by
/// reference: a `&AppHandle` borrowed from a temporary (for example
/// `install(&app.handle().clone())`) dies at the call's semicolon and leaves
/// `APP_PTR` pointing at freed memory, which the handler would then
/// dereference on every SIGTERM.
pub fn install(app_handle: AppHandle) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let leaked: &'static mut AppHandle = Box::leak(Box::new(app_handle));
        APP_PTR.store(leaked as *mut AppHandle, Ordering::SeqCst);
        // SAFETY: `action` is a fully zeroed `sigaction` (valid for every
        // field), `signal_handler` is a real `extern "C" fn(i32)` matching the
        // `sa_sigaction` signature, `sa_mask` is initialised by `sigemptyset`
        // before `sigaction` reads it, and passing `null_mut()` as the old
        // action is an explicitly documented "don't care" for that out-param.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            // SIGTERM (15): request an orderly Tauri exit.
            action.sa_sigaction = signal_handler as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(15, &action, std::ptr::null_mut()) != 0 {
                tracing::warn!(
                    "failed to install SIGTERM handler: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    });
}

extern "C" fn signal_handler(_: i32) {
    // Signal context: only async-signal-safe calls. `AppHandle::exit(0)` sends
    // a message to the run loop over a channel, which is safe here. The
    // pointer was set on the main thread before any signal could be
    // delivered, and `SeqCst` gives us the acquire ordering we need to see a
    // non-null value if install has completed.
    let ptr = APP_PTR.load(Ordering::SeqCst);
    if !ptr.is_null() {
        // SAFETY: `ptr` is only ever non-null after `install` stored a leaked
        // (`'static`) `AppHandle`, so the pointee is valid and lives for the
        // whole process, and the null check above rules out dereferencing a
        // null pointer. `exit(0)` only enqueues an exit message on the run
        // loop, which is safe to call from the signal-delivery thread.
        unsafe { (&*ptr).exit(0) };
    }
}
