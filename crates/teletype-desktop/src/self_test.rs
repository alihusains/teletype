//! §9.2 installer smoke — `teletype-desktop --self-test`.
//!
//! Lead pre-scaffold (bw-c24): `main.rs` calls [`run`] when `--self-test`
//! is passed and exits with the returned code. The worker replaces the
//! stub body with the real smoke test:
//!
//! - discover the models dir exactly the way the app does;
//! - missing model -> `{"ok":false,"stage":"models","error":"..."}`, exit 2;
//! - otherwise load the speech model, run `hello.wav` through the real
//!   manager + text pipeline headlessly, print `{"ok":true,...}`, exit 0;
//! - any failure -> `{"ok":false,"stage":"...",...}`, exit 1;
//! - the whole run is bounded by a timeout so a hang fails loudly.
//!
//! Kept compiling on its own so the shared tree stays green while parallel
//! workers hold other files.

/// Runs the installer smoke test and returns the process exit code.
///
/// Stub until bw-c24 lands: always reports `stage: "stub"`, exit 1.
pub fn run() -> i32 {
    println!(r#"{{"ok":false,"stage":"stub","error":"self-test not implemented yet"}}"#);
    1
}
