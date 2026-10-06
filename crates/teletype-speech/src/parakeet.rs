//! NVIDIA Parakeet TDT backend via whisper.cpp's `parakeet` engine (C API).
//!
//! Parakeet TDT 0.6B v3 is the state-of-the-art streaming dictation model:
//! 25× faster than real time with punctuation, and more accurate than
//! Whisper for English dictation. The engine is compiled from whisper.cpp
//! v1.9.4 by the `parakeet-sys` crate.
//!
//! The native engine is a **macOS-only** dependency: `parakeet-sys` is wired
//! into the workspace under `target.'cfg(target_os = "macos")'`, and the
//! `teletype-speech` build script only emits its link flags on macOS. On
//! every other target (notably the native Windows release) there is no engine
//! to link, so this module provides a stub [`ParakeetProvider`] that
//! compiles and reports a clear "not supported" error if actually used. The
//! native Windows ASR port (W2/W3) re-enables the real backend and drops the
//! stub.

#[cfg(target_os = "macos")]
mod ffi {
    // ---- FFI (subset of whisper.cpp include/parakeet.h, v1.9.4) ----

    #[allow(non_camel_case_types)]
    #[repr(C)]
    pub struct parakeet_context_params {
        pub use_gpu: bool,
        pub gpu_device: i32,
    }

    #[repr(C)]
    pub struct parakeet_full_params {
        pub strategy: i32,
        pub n_threads: i32,
        pub offset_ms: i32,
        pub duration_ms: i32,
        pub no_context: bool,
        pub audio_ctx: i32,
        pub new_segment_callback: *mut std::ffi::c_void,
        pub new_segment_callback_user_data: *mut std::ffi::c_void,
        pub new_token_callback: *mut std::ffi::c_void,
        pub new_token_callback_user_data: *mut std::ffi::c_void,
        pub progress_callback: *mut std::ffi::c_void,
        pub progress_callback_user_data: *mut std::ffi::c_void,
        pub encoder_begin_callback: *mut std::ffi::c_void,
        pub encoder_begin_callback_user_data: *mut std::ffi::c_void,
        pub abort_callback: *mut std::ffi::c_void,
        pub abort_callback_user_data: *mut std::ffi::c_void,
    }

    pub const PARAKEET_SAMPLING_GREEDY: i32 = 0;

    extern "C" {
        pub fn parakeet_init_from_file_with_params(
            path_model: *const std::ffi::c_char,
            params: parakeet_context_params,
        ) -> *mut std::ffi::c_void;
        pub fn parakeet_free(ctx: *mut std::ffi::c_void);
        pub fn parakeet_full_default_params(strategy: i32) -> parakeet_full_params;
        pub fn parakeet_full(
            ctx: *mut std::ffi::c_void,
            params: parakeet_full_params,
            samples: *const f32,
            n_samples: i32,
        ) -> i32;
        pub fn parakeet_full_n_segments(ctx: *mut std::ffi::c_void) -> i32;
        pub fn parakeet_full_get_segment_text(
            ctx: *mut std::ffi::c_void,
            i_segment: i32,
        ) -> *const std::ffi::c_char;
        #[allow(dead_code)]
        pub fn parakeet_free_params(params: *mut parakeet_full_params);
    }
}

// True from the moment the main thread starts tearing down the process
// (exit() / app.run() returning) until the process dies. Only the native
// macOS provider needs it (to avoid freeing the Metal device at exit); the
// stub holds no C context.
#[cfg(target_os = "macos")]
fn in_teardown() -> bool {
    static TEARDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    TEARDOWN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Idempotently marks the process as tearing down and forces the OS to run
/// C++ static-object destructors NOW, on the main thread, while the Metal
/// runtime is still fully alive.
///
/// whisper.cpp's Metal backend parks the GPU device in a function-local
/// static (`ggml_metal_device_get`). If that destructor runs during the
/// final `exit()` — after AppKit's `terminate:` has already torn the Metal
/// environment down — `ggml_metal_rsets_free` asserts that no residency-set
/// buffers remain and aborts (SIGABRT on Quit). Calling `exit(0)` from
/// `RunEvent::Exit` runs the same destructors at a point where every Metal
/// object we own has already been freed in-session, so the assert holds.
///
/// This MUST be the last statement in the hook: the process never returns.
#[cfg(target_os = "macos")]
pub fn mark_teardown() {
    static TEARDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    TEARDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the process is exiting; no Rust code runs after this.
    unsafe { std::process::exit(0) };
}

/// No-op outside macOS: the stub provider holds no C context, so there is
/// nothing to protect against the C++ teardown order.
#[cfg(not(target_os = "macos"))]
pub fn mark_teardown() {}

#[cfg(target_os = "macos")]
mod native {
    use super::ffi::*;
    use super::{in_teardown, ParakeetProvider};
    use crate::{SpeechError, SpeechProvider};
    use std::path::Path;

    impl ParakeetProvider {
        pub fn new() -> Self {
            Self {
                ctx: std::ptr::null_mut(),
            }
        }
    }

    impl Default for ParakeetProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for ParakeetProvider {
        fn drop(&mut self) {
            // Never free the context during process teardown: the process is
            // already exiting, and parakeet_free() would race the C++ static
            // destructors that `mark_teardown()` forces at exit. Leaking the
            // context at exit is safe — the OS reclaims it. Unloads during a
            // session go through `unload()` (called explicitly from
            // `RunEvent::Exit` before the teardown flag is set).
            if !std::thread::panicking() && !in_teardown() && !self.ctx.is_null() {
                // SAFETY: ctx is a non-null, owned parakeet context.
                unsafe {
                    parakeet_free(self.ctx);
                }
                self.ctx = std::ptr::null_mut();
            }
        }
    }

    impl SpeechProvider for ParakeetProvider {
        fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
            if self.is_loaded() {
                return Ok(());
            }
            let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
                .map_err(|_| SpeechError::ModelLoad("invalid model path".into()))?;
            let params = parakeet_context_params {
                use_gpu: true,
                gpu_device: 0,
            };
            // SAFETY: c_path is a valid NUL-terminated C string; params is a
            // valid context-params struct. Returns null on failure, which we
            // check.
            let ctx = unsafe { parakeet_init_from_file_with_params(c_path.as_ptr(), params) };
            if ctx.is_null() {
                return Err(SpeechError::ModelLoad(format!(
                    "failed to load Parakeet model at {}",
                    path.display()
                )));
            }
            self.ctx = ctx;
            Ok(())
        }

        fn unload(&mut self) {
            if !self.ctx.is_null() {
                // SAFETY: ctx is a non-null, owned parakeet context.
                unsafe {
                    parakeet_free(self.ctx);
                }
                self.ctx = std::ptr::null_mut();
            }
        }

        fn is_loaded(&self) -> bool {
            !self.ctx.is_null()
        }

        fn transcribe(&mut self, samples: &[f32], _language: &str) -> Result<String, SpeechError> {
            let ctx = self.ctx;
            if ctx.is_null() {
                return Err(SpeechError::NoModel);
            }
            if samples.is_empty() {
                return Err(SpeechError::NoSpeech);
            }

            // parakeet_full_default_params returns a value-initialized struct
            // with null callbacks (nothing to free). parakeet_full takes it by
            // value, so there is nothing to free afterwards.
            // SAFETY: returns a valid default params struct for the given
            // strategy.
            let mut params = unsafe { parakeet_full_default_params(PARAKEET_SAMPLING_GREEDY) };
            params.n_threads = std::cmp::max(
                2,
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4),
            ) as i32;

            // SAFETY: ctx is non-null and valid; params is a valid, fully
            // initialized struct; samples points to a valid f32 buffer of the
            // given length.
            let ret = unsafe { parakeet_full(ctx, params, samples.as_ptr(), samples.len() as i32) };
            if ret != 0 {
                return Err(SpeechError::Transcribe(format!(
                    "parakeet_full failed ({ret})"
                )));
            }

            // SAFETY: ctx is a valid, non-null parakeet context.
            let n_segments = unsafe { parakeet_full_n_segments(ctx) };
            let mut text = String::new();
            for i in 0..n_segments {
                // SAFETY: ctx valid; i is within [0, n_segments).
                let c = unsafe { parakeet_full_get_segment_text(ctx, i) };
                if !c.is_null() {
                    // SAFETY: c is a non-null C string pointer returned by
                    // parakeet.
                    let s = unsafe { std::ffi::CStr::from_ptr(c) }
                        .to_string_lossy()
                        .into_owned();
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(s.trim());
                }
            }

            let text = text.trim().to_string();
            if text.is_empty() {
                Err(SpeechError::NoSpeech)
            } else {
                Ok(text)
            }
        }
    }
}

// ---- Non-macOS (Windows) stub ----
//
// No native Parakeet engine is built on these targets. The stub keeps the
// type and its constructor available so the rest of the app compiles
// unchanged, and surfaces a clear error if dictation is actually attempted.

#[cfg(not(target_os = "macos"))]
mod stub {
    use super::ParakeetProvider;
    use crate::{SpeechError, SpeechProvider};
    use std::path::Path;

    impl ParakeetProvider {
        pub fn new() -> Self {
            Self { loaded: false }
        }
    }

    impl Default for ParakeetProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SpeechProvider for ParakeetProvider {
        fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
            self.loaded = true;
            Ok(())
        }

        fn unload(&mut self) {
            self.loaded = false;
        }

        fn is_loaded(&self) -> bool {
            self.loaded
        }

        fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
            Err(SpeechError::Transcribe(
                "Parakeet speech recognition is not available on this platform yet".into(),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
pub struct ParakeetProvider {
    ctx: *mut std::ffi::c_void,
}

#[cfg(not(target_os = "macos"))]
pub struct ParakeetProvider {
    loaded: bool,
}

// SAFETY: On macOS the opaque C context is owned exclusively by this handle
// and only accessed through it; freed exactly once in Drop. The stub holds no
// shared state, so it is trivially Send.
unsafe impl Send for ParakeetProvider {}
