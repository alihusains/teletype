//! whisper.cpp backend via whisper-rs 0.16.
//!
//! On macOS uses Metal + CoreML when available; elsewhere CPU.
//!
//! `whisper-rs` (and its `whisper-rs-sys` bindgen/C build) is only wired up
//! for the macOS target in this workspace. On other targets this module
//! provides a stub [`WhisperProvider`] that compiles and reports a clear
//! "not supported" error if actually used. The native Windows port (W2/W3)
//! re-enables the real backend and drops the stub.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod stub;

#[cfg(target_os = "macos")]
pub use macos::WhisperProvider;
#[cfg(not(target_os = "macos"))]
pub use stub::WhisperProvider;
