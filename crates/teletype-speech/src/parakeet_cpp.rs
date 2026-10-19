//! Parakeet backend via mudler/parakeet.cpp (C++/ggml runtime, C API).
//!
//! parakeet.cpp's GGUF loader keeps per-tensor quant types, so mixed-quant
//! GGUFs (e.g. Fluid Speech Mini's Q2_K/Q4_K encoder) load natively —
//! unlike whisper.cpp's parakeet engine, which only reads the legacy ggml v3
//! binary format. The engine is compiled from a pinned parakeet.cpp commit by
//! the `parakeet-cpp-sys` crate.
//!
//! The native engine is a **macOS-only** dependency: `parakeet-cpp-sys` is
//! wired into the workspace under `target.'cfg(target_os = "macos")'`, and the
//! `teletype-speech` build script only emits its link flags on macOS. On
//! every other target there is no engine to link, so this module provides a
//! stub [`ParakeetCppProvider`] that compiles and reports a clear "not
//! supported" error if actually used.

#[cfg(target_os = "macos")]
mod ffi {
    // ---- FFI (subset of parakeet.cpp include/parakeet_capi.h, ABI v10) ----
    //
    // The final binary links BOTH parakeet engines, which ship two different
    // ggml versions under identical archive names (libparakeet.a, libggml*.a).
    // The whisper.cpp archives are emitted first on the search path, so the
    // linker binds every ggml symbol from the whisper.cpp copy; parakeet.cpp's
    // object files (compiled against ggml 0.13) would then run on a ggml 0.9
    // runtime and crash. To keep the two runtimes disjoint we dlopen the
    // parakeet.cpp archive at runtime and resolve its C API through a
    // function-pointer table. The archive path is passed from
    // teletype-speech/build.rs as DEP_TELETYPE_SPEECH_PARAKEET_CPP_LIB.

    pub type ParakeetCtx = std::ffi::c_void;

    #[allow(non_camel_case_types)]
    pub type FnLoad = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut ParakeetCtx;
    #[allow(non_camel_case_types)]
    pub type FnFree = unsafe extern "C" fn(*mut ParakeetCtx);
    #[allow(non_camel_case_types)]
    pub type FnTranscribePcm =
        unsafe extern "C" fn(*mut ParakeetCtx, *const f32, i32, i32, i32) -> *mut std::ffi::c_char;
    #[allow(non_camel_case_types)]
    pub type FnFreeString = unsafe extern "C" fn(*mut std::ffi::c_char);
    #[allow(non_camel_case_types)]
    pub type FnLastError = unsafe extern "C" fn(*mut ParakeetCtx) -> *const std::ffi::c_char;
    #[allow(non_camel_case_types)]
    pub type FnSetConcurrency = unsafe extern "C" fn(*mut ParakeetCtx, i32, i32) -> i32;

    /// The resolved parakeet.cpp C API, loaded once from the dlopen'd archive.
    pub struct CApi {
        pub load: FnLoad,
        pub free: FnFree,
        pub transcribe_pcm: FnTranscribePcm,
        pub free_string: FnFreeString,
        pub last_error: FnLastError,
        pub set_concurrency: FnSetConcurrency,
    }

    /// dlopen the parakeet.cpp static archive and resolve the C API symbols.
    /// Returns an error string on failure.
    pub unsafe fn load_capi(lib_path: &std::ffi::CStr) -> Result<CApi, String> {
        use std::ffi::{CStr, CString};
        extern "C" {
            fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut std::ffi::c_void;
            fn dlsym(
                handle: *mut std::ffi::c_void,
                symbol: *const std::ffi::c_char,
            ) -> *mut std::ffi::c_void;
            fn dlerror() -> *const std::ffi::c_char;
        }
        const RTLD_NOW: i32 = 2;
        let handle = unsafe { dlopen(lib_path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            let err = unsafe { dlerror() };
            let msg = if err.is_null() {
                "unknown dlopen error".to_string()
            } else {
                unsafe { CStr::from_ptr(err) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(format!("dlopen failed: {msg}"));
        }
        let sym = |name: &str| -> Result<*mut std::ffi::c_void, String> {
            let c = CString::new(name).map_err(|e| e.to_string())?;
            let p = unsafe { dlsym(handle, c.as_ptr()) };
            if p.is_null() {
                Err(format!("missing symbol {name}"))
            } else {
                Ok(p)
            }
        };
        let load = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnLoad>(
                sym("parakeet_capi_load").map_err(|e| format!("{e} (handle {handle:p})"))?,
            )
        };
        let free = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnFree>(sym("parakeet_capi_free")?)
        };
        let transcribe_pcm = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnTranscribePcm>(sym(
                "parakeet_capi_transcribe_pcm",
            )?)
        };
        let free_string = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnFreeString>(sym(
                "parakeet_capi_free_string",
            )?)
        };
        let last_error = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnLastError>(sym(
                "parakeet_capi_last_error",
            )?)
        };
        let set_concurrency = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnSetConcurrency>(sym(
                "parakeet_capi_set_concurrency",
            )?)
        };
        Ok(CApi {
            load,
            free,
            transcribe_pcm,
            free_string,
            last_error,
            set_concurrency,
        })
    }

    /// `decoder` value: 0 = default head for the model's architecture.
    pub const DECODER_DEFAULT: i32 = 0;
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
/// parakeet.cpp's ggml Metal backend parks the GPU device in a function-local
/// static, the same pattern as whisper.cpp's. If that destructor runs during
/// the final `exit()` — after AppKit's `terminate:` has already torn the Metal
/// environment down — the Metal cleanup asserts and aborts (SIGABRT on Quit).
/// Calling `exit(0)` from `RunEvent::Exit` runs the same destructors at a
/// point where every Metal object we own has already been freed in-session,
/// so the assert holds.
///
/// This MUST be the last statement in the hook: the process never returns.
#[cfg(target_os = "macos")]
pub fn mark_teardown() {
    static TEARDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    TEARDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    // The process is exiting; no Rust code runs after this.
    std::process::exit(0);
}

/// No-op outside macOS: the stub provider holds no C context, so there is
/// nothing to protect against the C++ teardown order.
#[cfg(not(target_os = "macos"))]
pub fn mark_teardown() {}

#[cfg(target_os = "macos")]
mod native {
    use super::ffi::{CApi, DECODER_DEFAULT};
    use super::{in_teardown, ParakeetCppProvider};
    use crate::{SpeechError, SpeechProvider};
    use std::path::Path;

    /// The dlopen'd parakeet.cpp C API, resolved once per process.
    static CAPI: std::sync::OnceLock<CApi> = std::sync::OnceLock::new();

    /// Resolve the C API, dlopen'ing the parakeet.cpp archive on first use.
    /// `Err` when the engine was not built (load() then reports a clear
    /// "engine unavailable" error).
    fn capi() -> Result<&'static CApi, String> {
        if let Some(api) = CAPI.get() {
            return Ok(api);
        }
        let lib = std::env::var("TELETYPE_SPEECH_PARAKEET_CPP_LIB")
            .map_err(|_| "parakeet.cpp engine not built on this machine".to_string())?;
        let c_lib = std::ffi::CString::new(lib)
            .map_err(|_| "invalid parakeet.cpp library path".to_string())?;
        // SAFETY: dlopen/dlsym are called with a valid NUL-terminated path;
        // the archive is a self-contained static library built by
        // parakeet-cpp-sys.
        let api = unsafe { super::ffi::load_capi(&c_lib) }?;
        // A concurrent caller may have won the race; get_or_init keeps the
        // first one.
        Ok(CAPI.get_or_init(|| api))
    }

    impl ParakeetCppProvider {
        pub fn new() -> Self {
            Self {
                ctx: std::ptr::null_mut(),
            }
        }
    }

    impl Default for ParakeetCppProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for ParakeetCppProvider {
        fn drop(&mut self) {
            // Never free the context during process teardown: the process is
            // already exiting, and parakeet_capi_free() would race the C++
            // static destructors that `mark_teardown()` forces at exit.
            // Leaking the context at exit is safe — the OS reclaims it.
            // Unloads during a session go through `unload()` (called
            // explicitly from `RunEvent::Exit` before the teardown flag is
            // set).
            if !std::thread::panicking() && !in_teardown() && !self.ctx.is_null() {
                if let Ok(api) = capi() {
                    // SAFETY: ctx is a non-null, owned parakeet context.
                    unsafe {
                        (api.free)(self.ctx);
                    }
                    self.ctx = std::ptr::null_mut();
                }
            }
        }
    }

    impl SpeechProvider for ParakeetCppProvider {
        fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
            if self.is_loaded() {
                return Ok(());
            }
            let api = capi().map_err(|e| {
                SpeechError::ModelLoad(format!("parakeet.cpp engine unavailable: {e}"))
            })?;
            let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
                .map_err(|_| SpeechError::ModelLoad("invalid model path".into()))?;
            // SAFETY: c_path is a valid NUL-terminated C string. Returns null
            // on failure, which we check.
            let ctx = unsafe { (api.load)(c_path.as_ptr()) };
            if ctx.is_null() {
                return Err(SpeechError::ModelLoad(format!(
                    "failed to load Parakeet.cpp model at {}",
                    path.display()
                )));
            }
            // The C API exposes no per-call thread parameter; the compute
            // thread count is set once here. FluidVoice's README notes the
            // default thread count can be slow, so pin a reasonable budget
            // (same shape as the whisper.cpp parakeet path).
            // SAFETY: ctx is a valid, non-null context; no request is running.
            unsafe {
                (api.set_concurrency)(
                    ctx,
                    1,
                    std::cmp::max(
                        2,
                        std::thread::available_parallelism()
                            .map(|n| n.get())
                            .unwrap_or(4),
                    ) as i32,
                );
            }
            self.ctx = ctx;
            Ok(())
        }

        fn unload(&mut self) {
            if !self.ctx.is_null() {
                if let Ok(api) = capi() {
                    // SAFETY: ctx is a non-null, owned parakeet context.
                    unsafe {
                        (api.free)(self.ctx);
                    }
                    self.ctx = std::ptr::null_mut();
                }
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
            let api = capi().map_err(|e| {
                SpeechError::Transcribe(format!("parakeet.cpp engine unavailable: {e}"))
            })?;

            // SAFETY: ctx is non-null and valid; samples points to a valid f32
            // buffer of the given length; 16 kHz mono is the rate the dictation
            // path always feeds. Returns a malloc'd transcript to free, or
            // null on error.
            let c = unsafe {
                (api.transcribe_pcm)(
                    ctx,
                    samples.as_ptr(),
                    samples.len() as i32,
                    16000,
                    DECODER_DEFAULT,
                )
            };
            if c.is_null() {
                // SAFETY: ctx is valid; returns a borrowed, NUL-terminated
                // message ("" when none).
                let msg = unsafe {
                    let p = (api.last_error)(ctx);
                    if p.is_null() {
                        String::new()
                    } else {
                        // SAFETY: p is a valid NUL-terminated C string.
                        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                };
                return Err(SpeechError::Transcribe(if msg.is_empty() {
                    "parakeet_capi_transcribe_pcm failed".into()
                } else {
                    msg
                }));
            }

            // SAFETY: c is a non-null, malloc'd, NUL-terminated C string owned
            // by us; freed exactly once below.
            let text = unsafe {
                let s = std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned();
                (api.free_string)(c);
                s
            };

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
// No native parakeet.cpp engine is built on these targets. The stub keeps the
// type and its constructor available so the rest of the app compiles
// unchanged, and surfaces a clear error if dictation is actually attempted.

#[cfg(not(target_os = "macos"))]
mod stub {
    use super::ParakeetCppProvider;
    use crate::{SpeechError, SpeechProvider};
    use std::path::Path;

    impl ParakeetCppProvider {
        pub fn new() -> Self {
            Self { loaded: false }
        }
    }

    impl Default for ParakeetCppProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SpeechProvider for ParakeetCppProvider {
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
                "Parakeet.cpp speech recognition is not available on this platform yet".into(),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
pub struct ParakeetCppProvider {
    ctx: *mut std::ffi::c_void,
}

#[cfg(not(target_os = "macos"))]
pub struct ParakeetCppProvider {
    loaded: bool,
}

// SAFETY: On macOS the opaque C context is owned exclusively by this handle
// and only accessed through it; freed exactly once in Drop. The stub holds no
// shared state, so it is trivially Send.
unsafe impl Send for ParakeetCppProvider {}

#[cfg(test)]
mod tests {
    use super::ParakeetCppProvider;
    use crate::SpeechProvider;

    /// Compile gate: the provider constructs on every target (native macOS or
    /// stub) and reports unloaded until `load` is called.
    #[test]
    fn provider_constructs_unloaded() {
        let p = ParakeetCppProvider::new();
        assert!(!p.is_loaded());
    }

    /// Transcribing without a loaded model is a clean NoModel error, not a
    /// panic, on every target.
    #[test]
    fn transcribe_without_model_is_no_model() {
        let mut p = ParakeetCppProvider::new();
        let err = p.transcribe(&[0.0f32], "en").unwrap_err();
        assert!(matches!(err, crate::SpeechError::NoModel));
    }
}
