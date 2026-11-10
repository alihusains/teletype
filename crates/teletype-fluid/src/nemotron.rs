//! The real provider for Nemotron 3.5 ASR Streaming via FluidAudio Core ML
//! (Neural Engine). A second, independent session of the `TeletypeFluid`
//! Swift bridge (the `ttf_nemotron_*` entry points in
//! `swift/Sources/TeletypeFluid/Bridge.swift`), on top of FluidAudio's
//! `StreamingNemotronMultilingualAsrManager`.
//!
//! Contract mirrored from the Swift side:
//! - `ttf_nemotron_init` downloads (when absent) and loads the multilingual
//!   `chunk_ms` variant under `models_dir`; returns 0 on success, -1 on
//!   failure and writes a NUL-terminated message into `err`.
//! - `ttf_nemotron_transcribe` returns 0 on success, a positive byte count
//!   when `out` is too small (retry with a buffer of exactly that size), -1
//!   on failure; failure detail is available via `ttf_nemotron_last_error`.
//! - `ttf_nemotron_is_ready` reports 1 while the model is loaded;
//!   `ttf_nemotron_unload` releases it (the process-wide session, one owner
//!   expected).
//! - Every call blocks its thread; the first `load` may download the model
//!   and takes as long as the network allows.

use std::ffi::{c_char, CString};
use std::path::Path;

use teletype_speech::{ModelCapabilities, SpeechError, SpeechProvider, Transcript};

use crate::format::{decode_buffer, initial_out_capacity, parse_transcript_json, MAX_OUT_CAPACITY};

/// The chunk tier (milliseconds) the bridge downloads and loads: the
/// multilingual variant's lowest-latency tier.
const NEMOTRON_CHUNK_MS: isize = 560;

// Raw entry points exported by the Swift static library (`@_cdecl`).
//
// Contract mirrored from Bridge.swift (see the module docs). Every pointer
// parameter must be non-null and valid for the call; buffer capacities are
// passed in bytes (including the trailing NUL).
extern "C" {
    fn ttf_nemotron_init(
        models_dir: *const c_char,
        chunk_ms: isize,
        err: *mut c_char,
        err_cap: usize,
    ) -> i32;
    fn ttf_nemotron_is_ready() -> i32;
    fn ttf_nemotron_transcribe(
        pcm: *const f32,
        sample_count: usize,
        language: *const c_char,
        out: *mut c_char,
        out_cap: usize,
    ) -> i32;
    fn ttf_nemotron_unload();
    fn ttf_nemotron_last_error(out: *mut c_char, out_cap: usize) -> i32;
}

/// Nemotron 3.5 ASR Streaming Core ML provider (Neural Engine).
///
/// The bridge owns one process-wide session (the loaded model), so use one
/// live instance at a time. Creating, `load`ing, and transcribing all block;
/// call them from a worker thread. Dropping the instance unloads the session.
#[derive(Debug)]
pub struct FluidNemotron {
    loaded: bool,
    last_language: Option<String>,
}

impl FluidNemotron {
    /// Create an unloaded provider; nothing touches the disk or network until
    /// [`SpeechProvider::load`].
    pub fn new() -> Self {
        Self {
            loaded: false,
            last_language: None,
        }
    }

    /// Whether the Swift bridge currently holds a loaded Nemotron model.
    pub fn is_ready(&self) -> bool {
        // SAFETY: no arguments; the bridge only reads its own session state.
        self.loaded && unsafe { ttf_nemotron_is_ready() } == 1
    }
}

impl Default for FluidNemotron {
    fn default() -> Self {
        Self::new()
    }
}

/// Last Nemotron bridge error message (empty when the bridge has none, or if
/// it cannot produce one within the growth budget).
fn last_error() -> String {
    let mut capacity = 1024usize;
    loop {
        let mut buffer = vec![0u8; capacity];
        // SAFETY: `buffer` is a valid writable region and its length is passed
        // as the capacity, which is the callee's stated contract.
        let code =
            unsafe { ttf_nemotron_last_error(buffer.as_mut_ptr().cast::<c_char>(), buffer.len()) };
        match code {
            0 => return String::new(),
            code if code > 0 && (code as usize) <= buffer.len() => {
                return decode_buffer(&buffer);
            }
            code if code > 0 => {
                capacity = code as usize;
                if capacity > 1 << 20 {
                    return String::new();
                }
            }
            _ => return String::new(),
        }
    }
}

impl SpeechProvider for FluidNemotron {
    /// Download (when absent) and load the multilingual Nemotron variant.
    /// `path` is the FluidAudio models base directory (the app's
    /// `models/fluid`); the checkpoint lands in
    /// `<path>/<repo folder>/<langDir>/<chunkMs>ms/`. Blocking and potentially
    /// slow on first run — worker thread only.
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        let directory = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
            SpeechError::ModelLoad(format!(
                "FluidAudio model directory contains a NUL byte: {}",
                path.display()
            ))
        })?;
        let mut err = vec![0u8; 512];
        // SAFETY: `directory` is a valid NUL-terminated C string that lives
        // for the call, and `err` is a valid writable region whose length is
        // passed as its capacity.
        let code = unsafe {
            ttf_nemotron_init(
                directory.as_ptr(),
                NEMOTRON_CHUNK_MS,
                err.as_mut_ptr().cast::<c_char>(),
                err.len(),
            )
        };
        if code == 0 {
            self.loaded = true;
            return Ok(());
        }
        self.loaded = false;
        let from_buffer = decode_buffer(&err);
        let message = if from_buffer.is_empty() {
            let from_log = last_error();
            if from_log.is_empty() {
                "Nemotron init failed".to_string()
            } else {
                from_log
            }
        } else {
            from_buffer
        };
        Err(SpeechError::ModelLoad(message))
    }

    fn unload(&mut self) {
        if self.loaded {
            // SAFETY: no arguments; the bridge only clears its own session.
            unsafe { ttf_nemotron_unload() };
        }
        self.loaded = false;
        self.last_language = None;
    }

    fn is_loaded(&self) -> bool {
        self.is_ready()
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        Ok(self.transcribe_detailed(samples, language)?.text)
    }

    /// The language the model reported on the last run, when auto-detect was
    /// active. `None` before the first take, when a language was locked, or
    /// when the model did not report a detection.
    fn detected_language(&self) -> Option<String> {
        self.last_language.clone()
    }

    /// Detailed transcription with per-word timings (Neural Engine path).
    ///
    /// `language` is a BCP-47 hint ("en-US", "auto", ...). The model auto-
    /// detects when the hint is empty or "auto" and reports the detection
    /// back (see [`SpeechProvider::detected_language`]).
    fn transcribe_detailed(
        &mut self,
        samples: &[f32],
        language: &str,
    ) -> Result<Transcript, SpeechError> {
        if !self.is_ready() {
            return Err(SpeechError::NoModel);
        }
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let language = CString::new(language)
            .or_else(|_| CString::new("auto"))
            .expect("static fallback language");

        let mut capacity = initial_out_capacity(samples.len());
        loop {
            let mut out = vec![0u8; capacity];
            // SAFETY: `samples` and `out` are valid regions whose lengths are
            // passed as their capacities, and the language pointer is a valid
            // NUL-terminated C string for the duration of the call.
            let code = unsafe {
                ttf_nemotron_transcribe(
                    samples.as_ptr(),
                    samples.len(),
                    language.as_ptr(),
                    out.as_mut_ptr().cast::<c_char>(),
                    out.len(),
                )
            };
            match code {
                0 => {
                    let parsed = parse_transcript_json(&decode_buffer(&out))?;
                    self.last_language = parsed.transcript.language.clone();
                    return Ok(parsed.transcript);
                }
                code if code > 0 => {
                    let needed = code as usize;
                    if needed > MAX_OUT_CAPACITY {
                        return Err(SpeechError::Transcribe(format!(
                            "Nemotron needs a {needed} byte result buffer (limit {MAX_OUT_CAPACITY})"
                        )));
                    }
                    capacity = needed;
                }
                _ => {
                    let message = last_error();
                    return Err(SpeechError::Transcribe(if message.is_empty() {
                        "Nemotron transcribe failed".to_string()
                    } else {
                        message
                    }));
                }
            }
        }
    }

    /// No native boost: Nemotron's language prompt is its own conditioning,
    /// so post-ASR rescoring covers this engine (the default no-op).
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_boost: false,
            word_timings: true,
        }
    }
}

impl Drop for FluidNemotron {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;
    use teletype_speech::SpeechProvider;

    /// The bridge session is process-wide, and cargo runs tests in parallel
    /// threads, so every test that touches it takes this lock.
    fn ffi_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A provider whose `load` was never called refuses to transcribe, and
    /// reports no detected language before any take.
    #[test]
    fn unloaded_provider_refuses_and_reports_no_language() {
        let _guard = ffi_lock();
        let mut provider = FluidNemotron::new();
        assert!(!provider.is_loaded());
        assert_eq!(provider.detected_language(), None);
        let error = provider
            .transcribe(&[0.0; 160], "en")
            .expect_err("unloaded provider must refuse");
        assert!(matches!(error, SpeechError::NoModel));
    }

    /// The FFI error path that is safe to exercise without any model weights:
    /// an init pointed at a directory that cannot hold the download still
    /// fails cleanly through the bridge rather than panicking.
    #[test]
    fn init_reports_a_clean_error_on_failure() {
        let _guard = ffi_lock();
        let mut provider = FluidNemotron::new();
        // A path under a file (not a directory) makes the download fail in
        // the bridge; the point is the clean Rust-side error, not the
        // specific OS message.
        let err = provider
            .load(Path::new("/nonexistent/teletype-fluid-nemotron-test/nope"))
            .err();
        // On a machine where that path happens to be creatable the load may
        // succeed (it would then download the model); either way the provider
        // must be in a consistent state, not a panic.
        if let Some(err) = err {
            assert!(matches!(err, SpeechError::ModelLoad(_)), "{err:?}");
        }
        assert_eq!(provider.is_loaded(), provider.is_ready());
    }

    /// End-to-end test against real weights. Disabled unless
    /// `TELETYPE_FLUID_NEMOTRON_MODELS` points at a models base directory (an
    /// empty directory downloads into it — ~660 MB). Never fails when the
    /// env var is unset.
    #[test]
    fn model_gated_end_to_end_transcription() {
        let Some(directory) = std::env::var_os("TELETYPE_FLUID_NEMOTRON_MODELS") else {
            return; // no weights requested on this machine
        };
        let _guard = ffi_lock();
        let mut provider = FluidNemotron::new();
        provider
            .load(Path::new(&directory))
            .expect("loading real Nemotron Core ML models must succeed");
        assert!(provider.is_loaded(), "provider reports loaded model");

        let detailed = provider
            .transcribe_detailed(&[0.0f32; 16_000], "en")
            .expect("a take must transcribe without error");
        println!(
            "nemotron e2e: text={:?} confidence={:.3} words={} language={:?}",
            detailed.text,
            detailed.confidence,
            detailed.words.len(),
            detailed.language
        );
        provider.unload();
        assert!(!provider.is_loaded());
    }
}
