//! FFI bindings for the Cactus Compute Needle 3 C API.
//!
//! The Needle 3 engine is a process-global, non-thread-safe C ABI. One text
//! model and one speech model can be loaded per process. All functions are
//! `unsafe` — the caller must ensure the model is loaded before calling
//! `init`/`complete`/`embed`, and must not call these from multiple threads
//! concurrently.
//!
//! # Safety model
//!
//! - `needle_load` must be called exactly once per model kind (text/speech)
//!   before any other function.
//! - `needle_init` must be called after `needle_load` and before
//!   `needle_complete`.
//! - `needle_complete` and `needle_embed` are not thread-safe; serialize
//!   access with a mutex in the calling crate.
//! - Output buffers are caller-allocated; the engine writes NUL-terminated
//!   JSON into them.
//!
//! # Telemetry
//!
//! The engine sends telemetry by default. Set `NEEDLE_TELEMETRY=0` and
//! `DO_NOT_TRACK=1` in the environment before calling `needle_load` to
//! disable it. The `teletype-inference` crate sets these at startup.
//!
//! # Windows
//!
//! Upstream ships a MinGW/libc++ archive for Windows, which cannot be linked
//! into the MSVC Tauri binary. This crate compiles a stub on Windows: every
//! load/init/complete call reports "unavailable" so callers fall back.

use std::ffi::{c_char, c_float, c_int, CStr, CString};
use std::os::raw::c_ulonglong;

// ── C API declarations ──────────────────────────────────────────────────────

#[cfg(not(needle_stub))]
extern "C" {
    /// Loads a `.cact` model file into the process-global engine.
    ///
    /// `cact` points to the entire file contents; `n` is the byte length.
    /// Returns `>= 0` on success, `< 0` on failure (call `needle_last_error`).
    /// The engine sniffs the file and loads whichever kind (text/speech) it
    /// holds, keeping the other slot free.
    pub fn needle_load(cact: *const u8, n: c_ulonglong) -> c_int;

    /// Bitmask of loaded model kinds: `NEEDLE_TEXT` (1), `NEEDLE_SPEECH` (2).
    pub fn needle_models() -> c_int;

    /// Last process-global error string. Owned by the runtime, valid until
    /// the next API call.
    pub fn needle_last_error() -> *const c_char;

    /// Initializes the engine with a system prompt and tool schemas.
    ///
    /// `system_prompt`: UTF-8 facts (date, locale, device, …) or null.
    /// `tools_json`: minified JSON array of tool schemas, or null.
    /// `tool_index_path`: path to persist tool embeddings, or null.
    ///
    /// Returns the tokenized static-prefix length on success; negative on
    /// failure (system prompt + tools exceed the context window).
    pub fn needle_init(
        system_prompt: *const c_char,
        tools_json: *const c_char,
        tool_index_path: *const c_char,
    ) -> c_int;

    /// Runs one completion (text or speech).
    ///
    /// Exactly one of `input` (UTF-8 text) and `pcm` (16 kHz mono float32)
    /// must be non-null. `max_new_tokens` caps the generation. `out` is a
    /// caller-allocated buffer of `out_capacity` bytes; on success it holds
    /// a NUL-terminated JSON object.
    ///
    /// Returns `>= 0` on success, `< 0` on failure (error message in `out`).
    pub fn needle_complete(
        input: *const c_char,
        pcm: *const c_float,
        samples: c_int,
        max_new_tokens: c_int,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int;

    /// Configures the speech transcription for `needle_complete` with PCM.
    ///
    /// `language`: "en", "de", "fr", "es", "it", "nl", "pl", or null (detect).
    /// `keywords`: newline-separated biasing words, or null.
    /// `word_timestamps`: non-zero to include per-word timestamps.
    pub fn needle_set_audio(
        language: *const c_char,
        keywords: *const c_char,
        word_timestamps: c_int,
    );

    /// Resets the conversation (keeps loaded tools and model).
    pub fn needle_reset();

    /// Transcribes 16 kHz mono float32 PCM in [-1, 1].
    ///
    /// Returns the number of tokens in the transcript on success. `out`
    /// receives JSON: `{"text":"...","language":"en","ttft_ms":0.0,...}`.
    pub fn needle_transcribe(
        pcm: *const c_float,
        samples: c_int,
        language: *const c_char,
        keywords: *const c_char,
        word_timestamps: c_int,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int;

    /// Live transcription: appends ~1 s of PCM, returns committed words.
    pub fn needle_stream_transcribe_process(
        pcm: *const c_float,
        samples: c_int,
        language: *const c_char,
        keywords: *const c_char,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int;

    /// Ends the live transcription stream.
    pub fn needle_stream_transcribe_stop(out: *mut c_char, out_capacity: c_int) -> c_int;

    /// Embeds text or speech.
    ///
    /// Exactly one of `input` and `pcm` must be non-null. Text returns the
    /// model's embedding dimension; speech returns one row per 80 ms frame.
    /// A null `out` returns the float count without computing.
    pub fn needle_embed(
        input: *const c_char,
        pcm: *const c_float,
        samples: c_int,
        out: *mut c_float,
        out_capacity: c_int,
    ) -> c_int;
}

// ── Windows stub ────────────────────────────────────────────────────────────

/// Pure-Rust stand-in for the native engine on Windows.
///
/// The upstream `windows-x86_64/libneedle.a` is built with LLVM-MinGW and
/// libc++ (Itanium ABI). It cannot be linked into the MSVC Tauri binary:
/// `link.exe` rejects its COMDATs with `LNK1143`, and `lld-link` still needs
/// the MinGW libc++/libc++abi runtime. Keeping the same raw signatures lets
/// the rest of the workspace compile unchanged; every load/init/complete
/// call reports "unavailable" so callers fall back.
#[cfg(needle_stub)]
mod stub {
    #![allow(clippy::missing_safety_doc)]

    use super::*;

    /// NUL-terminated error returned by the stub.
    const UNAVAILABLE: &[u8] =
        b"Needle is unavailable in this Windows build (the upstream engine is a MinGW/libc++ archive)\0";

    /// Writes the unavailable message into a caller buffer.
    ///
    /// # Safety
    /// `out` must be null or point to at least `out_capacity` writable bytes.
    unsafe fn write_error(out: *mut c_char, out_capacity: c_int) {
        if out.is_null() || out_capacity <= 0 {
            return;
        }
        let cap = out_capacity as usize;
        let n = UNAVAILABLE.len().min(cap);
        // SAFETY: the caller guarantees `out` is writable for `cap` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(UNAVAILABLE.as_ptr(), out as *mut u8, n);
            let end = if n < cap { n } else { cap - 1 };
            *out.add(end) = 0;
        }
    }

    /// Stub: loading is unsupported.
    pub unsafe fn needle_load(_cact: *const u8, _n: c_ulonglong) -> c_int {
        -1
    }

    /// Stub: no models are loaded.
    pub unsafe fn needle_models() -> c_int {
        0
    }

    /// Stub: returns the unavailable message.
    pub unsafe fn needle_last_error() -> *const c_char {
        UNAVAILABLE.as_ptr() as *const c_char
    }

    /// Stub: initialization is unsupported.
    pub unsafe fn needle_init(
        _system_prompt: *const c_char,
        _tools_json: *const c_char,
        _tool_index_path: *const c_char,
    ) -> c_int {
        -1
    }

    /// Stub: completion is unsupported.
    pub unsafe fn needle_complete(
        _input: *const c_char,
        _pcm: *const c_float,
        _samples: c_int,
        _max_new_tokens: c_int,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int {
        // SAFETY: the caller passes a writable output buffer (C API contract).
        unsafe { write_error(out, out_capacity) };
        -1
    }

    /// Stub: audio configuration is a no-op.
    pub unsafe fn needle_set_audio(
        _language: *const c_char,
        _keywords: *const c_char,
        _word_timestamps: c_int,
    ) {
    }

    /// Stub: reset is a no-op.
    pub unsafe fn needle_reset() {}

    /// Stub: transcription is unsupported.
    pub unsafe fn needle_transcribe(
        _pcm: *const c_float,
        _samples: c_int,
        _language: *const c_char,
        _keywords: *const c_char,
        _word_timestamps: c_int,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int {
        // SAFETY: the caller passes a writable output buffer (C API contract).
        unsafe { write_error(out, out_capacity) };
        -1
    }

    /// Stub: streaming transcription is unsupported.
    pub unsafe fn needle_stream_transcribe_process(
        _pcm: *const c_float,
        _samples: c_int,
        _language: *const c_char,
        _keywords: *const c_char,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int {
        // SAFETY: the caller passes a writable output buffer (C API contract).
        unsafe { write_error(out, out_capacity) };
        -1
    }

    /// Stub: stream stop is unsupported.
    pub unsafe fn needle_stream_transcribe_stop(out: *mut c_char, out_capacity: c_int) -> c_int {
        // SAFETY: the caller passes a writable output buffer (C API contract).
        unsafe { write_error(out, out_capacity) };
        -1
    }

    /// Stub: embeddings are unsupported.
    pub unsafe fn needle_embed(
        _input: *const c_char,
        _pcm: *const c_float,
        _samples: c_int,
        _out: *mut c_float,
        _out_capacity: c_int,
    ) -> c_int {
        -1
    }
}

#[cfg(needle_stub)]
pub use stub::*;

// ── Constants ───────────────────────────────────────────────────────────────

/// Bitmask: a text model is loaded.
pub const NEEDLE_TEXT: c_int = 1;
/// Bitmask: a speech model is loaded.
pub const NEEDLE_SPEECH: c_int = 2;

/// Default output buffer size (64 KB), matching the Python reference.
pub const DEFAULT_OUT_CAPACITY: usize = 65_536;

// ── Safe wrappers ───────────────────────────────────────────────────────────

/// Error returned by the Needle 3 engine.
#[derive(Debug, thiserror::Error)]
pub enum NeedleError {
    /// The engine returned a negative code. `message` is from
    /// `needle_last_error()` or the output buffer.
    #[error("needle engine error: {0}")]
    Engine(String),

    /// A string conversion failed (e.g. input contained a NUL byte).
    #[error("string conversion failed: {0}")]
    StringConv(String),

    /// The output buffer was too small.
    #[error("output buffer too small")]
    BufferTooSmall,
}

/// Reads the process-global last error string.
pub fn last_error() -> String {
    // SAFETY: `needle_last_error` returns a pointer to the engine's
    // process-global error string, valid while non-null.
    unsafe {
        let ptr = needle_last_error();
        if ptr.is_null() {
            return "unknown error".to_string();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// Loads a `.cact` model file from disk.
///
/// # Safety
/// Must be called at most once per model kind. The caller must ensure no
/// other thread is calling Needle 3 functions concurrently.
#[cfg(not(needle_stub))]
pub fn load_model(path: &std::path::Path) -> Result<(), NeedleError> {
    let data = std::fs::read(path)
        .map_err(|e| NeedleError::Engine(format!("cannot read {path:?}: {e}")))?;
    // SAFETY: data is a valid byte buffer; the documented load-once,
    // single-threaded precondition holds.
    let rc = unsafe { needle_load(data.as_ptr(), data.len() as c_ulonglong) };
    if rc < 0 {
        return Err(NeedleError::Engine(last_error()));
    }
    Ok(())
}

/// Loads a `.cact` model file from disk.
///
/// On Windows the engine is stubbed, so this reports [`NeedleError::Engine`]
/// without touching the filesystem.
#[cfg(needle_stub)]
pub fn load_model(_path: &std::path::Path) -> Result<(), NeedleError> {
    Err(NeedleError::Engine(last_error()))
}

/// Returns the bitmask of loaded model kinds.
pub fn loaded_models() -> c_int {
    // SAFETY: reads the engine's global model mask; no caller-managed pointers.
    unsafe { needle_models() }
}

/// Initializes the engine with a system prompt and tool schemas.
///
/// # Safety
/// The model must be loaded. Not thread-safe.
pub fn init(system_prompt: Option<&str>, tools_json: Option<&str>) -> Result<usize, NeedleError> {
    let sp = system_prompt
        .map(CString::new)
        .transpose()
        .map_err(|e| NeedleError::StringConv(e.to_string()))?;
    let tj = tools_json
        .map(CString::new)
        .transpose()
        .map_err(|e| NeedleError::StringConv(e.to_string()))?;

    let sp_ptr = sp.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null());
    let tj_ptr = tj.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null());

    // SAFETY: sp/tj are valid NUL-terminated strings or null; documented
    // loaded and single-threaded.
    let rc = unsafe { needle_init(sp_ptr, tj_ptr, std::ptr::null()) };
    if rc < 0 {
        return Err(NeedleError::Engine(last_error()));
    }
    Ok(rc as usize)
}

/// Runs one text completion.
///
/// `input` is the user text. `max_new_tokens` caps the generation.
/// Returns the JSON response string on success.
///
/// # Safety
/// The model must be loaded and initialized. Not thread-safe.
pub fn complete_text(input: &str, max_new_tokens: u32) -> Result<String, NeedleError> {
    let c_input = CString::new(input).map_err(|e| NeedleError::StringConv(e.to_string()))?;
    let mut buf = vec![0u8; DEFAULT_OUT_CAPACITY];

    // SAFETY: c_input is a valid NUL-terminated string, buf is a writable
    // output buffer; documented initialized and single-threaded.
    let rc = unsafe {
        needle_complete(
            c_input.as_ptr(),
            std::ptr::null(),
            0,
            max_new_tokens as c_int,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as c_int,
        )
    };
    if rc < 0 {
        // On failure the buffer holds an error message.
        let msg = c_str_from_buf(&buf);
        return Err(NeedleError::Engine(if msg.is_empty() {
            last_error()
        } else {
            msg
        }));
    }
    Ok(c_str_from_buf(&buf))
}

/// Runs one speech completion (16 kHz mono float32 PCM in [-1, 1]).
///
/// # Safety
/// The model must be loaded and initialized. Not thread-safe.
pub fn complete_speech(pcm: &[f32], max_new_tokens: u32) -> Result<String, NeedleError> {
    let mut buf = vec![0u8; DEFAULT_OUT_CAPACITY];

    // SAFETY: pcm is a valid f32 slice, buf is a writable output buffer;
    // documented initialized and single-threaded.
    let rc = unsafe {
        needle_complete(
            std::ptr::null(),
            pcm.as_ptr(),
            pcm.len() as c_int,
            max_new_tokens as c_int,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as c_int,
        )
    };
    if rc < 0 {
        let msg = c_str_from_buf(&buf);
        return Err(NeedleError::Engine(if msg.is_empty() {
            last_error()
        } else {
            msg
        }));
    }
    Ok(c_str_from_buf(&buf))
}

/// Embeds text, returning the embedding vector.
///
/// # Safety
/// The model must be loaded. Not thread-safe.
pub fn embed_text(input: &str) -> Result<Vec<f32>, NeedleError> {
    let c_input = CString::new(input).map_err(|e| NeedleError::StringConv(e.to_string()))?;

    // First call: query the dimension (out = null).
    // SAFETY: c_input is a valid NUL-terminated string; null out / 0
    // count is the documented dimension-query form; documented loaded.
    let dim = unsafe {
        needle_embed(
            c_input.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    if dim < 0 {
        return Err(NeedleError::Engine(last_error()));
    }
    let dim = dim as usize;

    // Second call: compute into a buffer.
    let mut vec = vec![0.0f32; dim];
    // SAFETY: c_input is valid; vec is writable for dim elements (dim
    // from the query call); documented loaded and single-threaded.
    let rc = unsafe {
        needle_embed(
            c_input.as_ptr(),
            std::ptr::null(),
            0,
            vec.as_mut_ptr(),
            dim as c_int,
        )
    };
    if rc < 0 {
        return Err(NeedleError::Engine(last_error()));
    }
    Ok(vec)
}

/// Resets the conversation (keeps loaded model and tools).
pub fn reset() {
    // SAFETY: needle_reset mutates only the engine's global state; no pointer
    // arguments.
    unsafe {
        needle_reset();
    }
}

/// Configures the speech transcription parameters.
pub fn set_audio(language: Option<&str>, keywords: Option<&str>, word_timestamps: bool) {
    let lang = language.and_then(|s| CString::new(s).ok());
    let kw = keywords.and_then(|s| CString::new(s).ok());

    // SAFETY: lang/kw are valid NUL-terminated strings or null; the FFI call
    // copies them before the CStrings drop.
    unsafe {
        needle_set_audio(
            lang.as_ref()
                .map(|s| s.as_ptr())
                .unwrap_or(std::ptr::null()),
            kw.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
            word_timestamps as c_int,
        );
    }
    // CStrings are dropped here, after the FFI call has copied the strings.
}

/// Extracts a NUL-terminated C string from a byte buffer.
fn c_str_from_buf(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}
