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

/// The parakeet.cpp shared library this build compiled, if any.
///
/// Build scripts pass the path with `cargo:rustc-env`, which becomes a
/// compile-time constant. It is not present in the process environment at
/// runtime, so reading it back needs `option_env!`; `std::env::var` alone
/// reported a built engine as missing.
pub fn compiled_library() -> Option<&'static str> {
    option_env!("TELETYPE_SPEECH_PARAKEET_CPP_LIB")
}

/// Whether this build can dlopen the parakeet.cpp engine right now.
///
/// A runtime override wins, then the compiled-in path. The file must still
/// exist: a stale path from a cleaned target directory counts as unavailable.
pub fn engine_available() -> bool {
    std::env::var_os("TELETYPE_SPEECH_PARAKEET_CPP_LIB")
        .or_else(|| compiled_library().map(std::ffi::OsString::from))
        .map(std::path::PathBuf::from)
        .is_some_and(|path| path.is_file())
}

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
    // teletype-speech/build.rs as the compile-time
    // TELETYPE_SPEECH_PARAKEET_CPP_LIB (DEP_PARAKEET_CPP_SYS_ROOT).

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
    /// `pk::shutdown_backend()` — the engine's own release hook for its
    /// process-global Metal backend. Itanium-mangled C++ symbol; see
    /// [`super::shutdown_backend`] for why we must call it.
    #[allow(non_camel_case_types)]
    pub type FnShutdownBackend = unsafe extern "C" fn();

    /// The resolved parakeet.cpp C API, loaded once from the dlopen'd archive.
    pub struct CApi {
        pub load: FnLoad,
        pub free: FnFree,
        pub transcribe_pcm: FnTranscribePcm,
        pub free_string: FnFreeString,
        pub last_error: FnLastError,
        pub set_concurrency: FnSetConcurrency,
        /// `None` when this dylib does not export the symbol. The engine still
        /// works; the process just cannot be exited cleanly (see
        /// [`super::shutdown_backend`]).
        pub shutdown_backend: Option<FnShutdownBackend>,
    }

    /// dlopen the parakeet.cpp static archive and resolve the C API symbols.
    /// Returns an error string on failure.
    ///
    /// # Safety
    /// `lib_path` must name a loadable archive that exposes the parakeet.cpp
    /// C API (ABI v10); every transmute'd function pointer below assumes
    /// exactly that ABI.
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
        // SAFETY: RTLD_NOW is a valid dlopen mode; the caller keeps the
        // returned handle alive for the whole lifetime of the CApi.
        let handle = unsafe { dlopen(lib_path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            // SAFETY: dlerror must be called immediately after a failed
            // dlopen; it returns the loader's own NUL-terminated message.
            let err = unsafe { dlerror() };
            let msg = if err.is_null() {
                "unknown dlopen error".to_string()
            } else {
                // SAFETY: a non-null dlerror result is a valid C string.
                unsafe { CStr::from_ptr(err) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(format!("dlopen failed: {msg}"));
        }
        let sym = |name: &str| -> Result<*mut std::ffi::c_void, String> {
            let c = CString::new(name).map_err(|e| e.to_string())?;
            // SAFETY: `handle` is non-null (checked by the caller) and `c`
            // is a valid NUL-terminated symbol name; dlsym takes no ownership.
            let p = unsafe { dlsym(handle, c.as_ptr()) };
            if p.is_null() {
                Err(format!("missing symbol {name}"))
            } else {
                Ok(p)
            }
        };
        // SAFETY: each block transmutes a non-null dlsym result into its
        // documented parakeet.cpp C ABI function type (ABI v10); the pointers
        // are only ever called through the CApi that returns them.
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
        // Not part of the C API header: the engine's C++ teardown hook. Note
        // the name as `dlsym` wants it, not as `nm` prints it — Mach-O adds one
        // `_` prefix, and the `_ZN...` underscore already belongs to the Itanium
        // mangling, so asking for the two-underscore table name resolves null.
        // A dylib built without the symbol must still load, so a miss is
        // tolerated rather than an error.
        let shutdown_backend = sym("_ZN2pk16shutdown_backendEv").ok().map(|p| unsafe {
            // SAFETY: `p` is a non-null dlsym result for the C++ member-less
            // free function `pk::shutdown_backend()`, which is `extern "C++"`
            // with no varargs and takes/returns nothing.
            std::mem::transmute::<*mut std::ffi::c_void, FnShutdownBackend>(p)
        });
        Ok(CApi {
            load,
            free,
            transcribe_pcm,
            free_string,
            last_error,
            set_concurrency,
            shutdown_backend,
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

/// Release parakeet.cpp's global GPU backend before the process exits.
///
/// Call this after every [`ParakeetCppProvider`] has been unloaded and before
/// [`mark_teardown`] (which never returns). Without it the app aborts with
/// `SIGABRT` on Quit whenever a parakeet.cpp model was used in the session;
/// see the native implementation for the exact destructor ordering.
#[cfg(target_os = "macos")]
pub fn shutdown_backend() {
    native::shutdown_backend()
}

/// No-op outside macOS: there is no engine to release.
#[cfg(not(target_os = "macos"))]
pub fn shutdown_backend() {}

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
        let lib = std::env::var_os("TELETYPE_SPEECH_PARAKEET_CPP_LIB")
            .map(|value| value.to_string_lossy().into_owned())
            .or_else(|| super::compiled_library().map(str::to_string))
            .ok_or_else(|| "parakeet.cpp engine not built on this machine".to_string())?;
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

    /// Free parakeet.cpp's process-global Metal backend (`pk::Backend`, which
    /// owns the GPU backend, its scheduler and graph allocator).
    ///
    /// The engine itself documents why this cannot be left to static
    /// destruction (`ggml_graph.cpp`): the backend and its graph allocator's
    /// device buffers must be released while the GPU driver is still alive.
    /// Otherwise they are still registered as Metal residency sets when ggml's
    /// device cache is destroyed during `exit()`, and
    /// `ggml_metal_rsets_free` aborts on `GGML_ASSERT([rsets->data count] == 0)`
    /// — the SIGABRT on Quit. Verified: unloading every provider without this
    /// call aborts at exit; with it the process exits 0.
    ///
    /// No-op if the engine was never loaded (no parakeet.cpp model used in this
    /// session) or the dylib does not export the symbol.
    pub fn shutdown_backend() {
        if let Some(api) = CAPI.get() {
            if let Some(shutdown) = api.shutdown_backend {
                // SAFETY: the engine guards this with its own recursive mutex
                // and re-creates the backend lazily on the next request, so
                // calling it between sessions is supported. We call it only
                // after every provider has unloaded.
                unsafe { shutdown() }
            }
        }
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
