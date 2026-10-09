//! Whistle speech provider (Cactus-Compute).
//!
//! Whistle is Cactus's English-first, multilingual-capable STT model. It is
//! loaded through the `libcactus_engine` static library, the same FFI that the
//! later Needle (tool-calling) integration uses — one native boundary for the
//! whole command/agent roadmap.
//!
//! This module is the Phase 1 adapter. The FFI is isolated in [`cactus`] and
//! gated behind the `cactus` feature so the crate compiles and tests green
//! without the native library present; the provider's control flow, audio
//! conversion, and the documented ~30 s chunking rule are all exercised by
//! unit tests against a mock transport.

use std::path::Path;

use crate::{SpeechError, SpeechProvider};

/// The Cactus engine documents a ~30 s per-inference window for transcription.
/// Longer audio must be split into windows and the transcripts merged
/// (roadmap Phase 1, "Important limitation").
pub const WHISTLE_MAX_SECONDS: u32 = 30;

/// Sample rate the Cactus transcribe path expects: 16 kHz mono PCM16.
pub const WHISTLE_SAMPLE_RATE: u32 = 16_000;

/// The minimal Cactus FFI surface this provider needs.
///
/// Mirrors `cactus-engine/cactus_engine.h`. When the `cactus` feature is on,
/// these bind to the real `libcactus_engine.a`; otherwise they are the stubs
/// the tests drive. Keeping the boundary this small means the provider logic
/// never touches `unsafe` directly.
pub(crate) trait CactusTransport {
    /// Loads a model bundle. Returns an opaque handle id.
    fn init(&mut self, model_path: &str) -> Result<u64, String>;
    fn destroy(&mut self, handle: u64);
    /// Transcribes a 16 kHz mono PCM16 buffer. `language` is a BCP code or "".
    fn transcribe_pcm16(
        &mut self,
        handle: u64,
        pcm16: &[i16],
        language: &str,
    ) -> Result<String, String>;
}

/// A test double that records calls and returns canned transcripts.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MockCactus {
    pub inited: bool,
    pub destroyed: bool,
    pub last_language: String,
    pub last_chunk_samples: usize,
    pub chunks: Vec<usize>,
    pub transcript: String,
    pub fail: bool,
    /// When set, each transcribe call copies the language it received here,
    /// so a test can assert on it after the provider (and mock) is consumed.
    pub language_recorder: Option<std::sync::Arc<std::sync::Mutex<String>>>,
}

#[cfg(test)]
impl CactusTransport for MockCactus {
    fn init(&mut self, _model_path: &str) -> Result<u64, String> {
        self.inited = true;
        Ok(1)
    }
    fn destroy(&mut self, _handle: u64) {
        self.destroyed = true;
    }
    fn transcribe_pcm16(
        &mut self,
        _handle: u64,
        pcm16: &[i16],
        language: &str,
    ) -> Result<String, String> {
        if self.fail {
            return Err("boom".into());
        }
        self.last_language = language.to_string();
        if let Some(rec) = &self.language_recorder {
            *rec.lock().unwrap() = language.to_string();
        }
        self.last_chunk_samples = pcm16.len();
        self.chunks.push(pcm16.len());
        Ok(self.transcript.clone())
    }
}

/// The production transport, bound to the real Cactus library.
///
/// Only compiled when the `cactus` feature is enabled, which also links
/// `libcactus_engine`. The body is a thin, `unsafe`-isolated wrapper over the
/// raw FFI; all orchestration lives in [`WhistleProvider`].
#[cfg(feature = "cactus")]
pub(crate) struct CactusLib;

#[cfg(feature = "cactus")]
impl CactusTransport for CactusLib {
    fn init(&mut self, model_path: &str) -> Result<u64, String> {
        use std::ffi::CString;
        let path = CString::new(model_path).map_err(|e| e.to_string())?;
        let handle = unsafe { raw::cactus_init(path.as_ptr(), std::ptr::null(), false) };
        if handle.is_null() {
            Err(last_error())
        } else {
            Ok(handle as u64)
        }
    }

    fn destroy(&mut self, handle: u64) {
        unsafe { raw::cactus_destroy(handle as *mut std::ffi::c_void) }
    }

    fn transcribe_pcm16(
        &mut self,
        handle: u64,
        pcm16: &[i16],
        language: &str,
    ) -> Result<String, String> {
        use std::ffi::CString;
        let lang = if language.is_empty() || language == "auto" {
            String::new()
        } else {
            language.to_string()
        };
        let opts = format!(r#"{{"language":"{}"}}"#, lang);
        let opts_c = CString::new(opts).unwrap_or_default();
        let prompt = CString::new("").unwrap();
        let mut out = vec![0u8; 65_536];
        let bytes: &[u8] = bytemuck_cast(pcm16);
        let rc = unsafe {
            raw::cactus_transcribe(
                handle as *mut std::ffi::c_void,
                std::ptr::null(),
                prompt.as_ptr(),
                out.as_mut_ptr() as *mut std::ffi::c_char,
                out.len(),
                opts_c.as_ptr(),
                None,
                std::ptr::null_mut(),
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        if rc < 0 {
            return Err(last_error());
        }
        Ok(cstring_from_buffer(&out))
    }
}

#[cfg(feature = "cactus")]
mod raw {
    use std::os::raw::{c_char, c_void};
    pub type CactusModelT = *mut c_void;

    #[link(name = "cactus_engine", kind = "static")]
    extern "C" {
        pub fn cactus_init(
            model_path: *const c_char,
            corpus_dir: *const c_char,
            cache_index: bool,
        ) -> CactusModelT;
        pub fn cactus_destroy(model: CactusModelT);
        pub fn cactus_transcribe(
            model: CactusModelT,
            audio_file_path: *const c_char,
            prompt: *const c_char,
            response_buffer: *mut c_char,
            buffer_size: usize,
            options_json: *const c_char,
            callback: *mut c_void,
            user_data: *mut c_void,
            pcm_buffer: *const u8,
            pcm_buffer_size: usize,
        ) -> i32;
    }
}

#[cfg(feature = "cactus")]
fn last_error() -> String {
    use std::ffi::CStr;
    unsafe {
        let p = raw::cactus_get_last_error();
        if p.is_null() {
            "cactus engine error".into()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

#[cfg(feature = "cactus")]
fn cstring_from_buffer(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

#[cfg(feature = "cactus")]
fn bytemuck_cast(pcm16: &[i16]) -> Vec<u8> {
    pcm16.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The Whistle [`SpeechProvider`].
///
/// `f32` 16 kHz samples in, transcript out — the same contract as Parakeet and
/// Whisper, so the rest of the app needs no engine-specific branching.
pub struct WhistleProvider {
    /// The active transport. In production this is the Cactus library
    /// (feature-gated); tests inject a [`MockCactus`].
    pub(crate) transport: Box<dyn CactusTransport>,
    handle: Option<u64>,
    loaded: bool,
}

impl Default for WhistleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WhistleProvider {
    /// Production constructor: binds to the real Cactus library when the
    /// `cactus` feature is on, otherwise to a transport that reports the
    /// runtime as unavailable (the catalog entry is `available: false` until
    /// then, so the UI never offers it for download).
    pub fn new() -> Self {
        #[cfg(feature = "cactus")]
        {
            Self {
                transport: Box::new(cactus::CactusLib),
                handle: None,
                loaded: false,
            }
        }
        #[cfg(not(feature = "cactus"))]
        {
            Self {
                transport: Box::new(NoCactus),
                handle: None,
                loaded: false,
            }
        }
    }

    /// Test/override constructor with an explicit transport.
    #[cfg(test)]
    pub(crate) fn with_transport(transport: Box<dyn CactusTransport>) -> Self {
        Self {
            transport,
            handle: None,
            loaded: false,
        }
    }

    /// Splits `samples` (16 kHz f32) into chunks no longer than
    /// [`WHISTLE_MAX_SECONDS`], converts each to PCM16, transcribes, and joins
    /// the transcripts with a space. Fewer than one chunk → one call.
    fn transcribe_chunked(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        let handle = self.handle.ok_or(SpeechError::NoModel)?;
        let max_samples = (WHISTLE_MAX_SECONDS as usize)
            .saturating_mul(WHISTLE_SAMPLE_RATE as usize);
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let mut parts = Vec::new();
        for chunk in samples.chunks(max_samples) {
            let pcm16 = f32_to_pcm16(chunk);
            let text = self
                .transport
                .transcribe_pcm16(handle, &pcm16, language)
                .map_err(SpeechError::Transcribe)?;
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        if parts.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        Ok(parts.join(" "))
    }
}

impl SpeechProvider for WhistleProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.loaded {
            return Ok(());
        }
        let model_path = path.to_string_lossy().into_owned();
        let handle = self
            .transport
            .init(&model_path)
            .map_err(SpeechError::ModelLoad)?;
        self.handle = Some(handle);
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.transport.destroy(handle);
        }
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        self.transcribe_chunked(samples, language)
    }
}

impl Drop for WhistleProvider {
    fn drop(&mut self) {
        self.unload();
    }
}

unsafe impl Send for WhistleProvider {}

/// Converts 16 kHz f32 samples (range roughly [-1, 1]) to little-endian PCM16.
fn f32_to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * i16::MAX as f32) as i16
        })
        .collect()
}

/// The no-op transport used when the `cactus` feature is off. Every call
/// reports the runtime as unavailable, which the catalog's `available: false`
/// already prevents the UI from offering.
#[cfg(not(feature = "cactus"))]
pub(crate) struct NoCactus;

#[cfg(not(feature = "cactus"))]
impl CactusTransport for NoCactus {
    fn init(&mut self, _model_path: &str) -> Result<u64, String> {
        Err("Cactus runtime not bundled (enable the `cactus` feature)".into())
    }
    fn destroy(&mut self, _handle: u64) {}
    fn transcribe_pcm16(
        &mut self,
        _handle: u64,
        _pcm16: &[i16],
        _language: &str,
    ) -> Result<String, String> {
        Err("Cactus runtime not bundled".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.5 * ((i % 7) as f32 - 3.0) / 3.0).collect()
    }

    #[test]
    fn loads_and_transcribes_single_window() {
        let mut p = WhistleProvider::with_transport(Box::new(MockCactus {
            transcript: "hello world".into(),
            ..Default::default()
        }));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        assert!(p.is_loaded());
        let out = p.transcribe(&samples(16_000), "en").unwrap(); // 1 s
        assert_eq!(out, "hello world");
        p.unload();
        assert!(!p.is_loaded());
    }

    #[test]
    fn audio_longer_than_30s_is_chunked() {
        let mock = MockCactus {
            transcript: "part".into(),
            ..Default::default()
        };
        let mut p = WhistleProvider::with_transport(Box::new(mock));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        // 61 s of audio at 16 kHz → three 30 s windows (30 + 30 + 1 s).
        let out = p.transcribe(&samples(61 * 16_000), "en").unwrap();
        assert_eq!(out, "part part part");

        // Cross-check the window count independently.
        let windows = (61usize * 16_000).div_ceil(30 * 16_000);
        assert_eq!(windows, 3);
    }

    #[test]
    fn exactly_30s_is_a_single_call() {
        // 30 s → one window, not two.
        let mock = MockCactus {
            transcript: "one".into(),
            ..Default::default()
        };
        let mut p = WhistleProvider::with_transport(Box::new(mock));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        let out = p.transcribe(&samples(30 * 16_000), "en").unwrap();
        assert_eq!(out, "one");
    }

    #[test]
    fn empty_audio_is_no_speech() {
        let mut p = WhistleProvider::with_transport(Box::new(MockCactus::default()));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        assert!(matches!(
            p.transcribe(&[], "en"),
            Err(SpeechError::NoSpeech)
        ));
    }

    #[test]
    fn transcribe_before_load_is_no_model() {
        let mut p = WhistleProvider::with_transport(Box::new(MockCactus::default()));
        assert!(matches!(
            p.transcribe(&samples(100), "en"),
            Err(SpeechError::NoModel)
        ));
    }

    #[test]
    fn transport_error_maps_to_transcribe_error() {
        let mut p = WhistleProvider::with_transport(Box::new(MockCactus {
            fail: true,
            ..Default::default()
        }));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        let err = p.transcribe(&samples(1000), "en").unwrap_err();
        assert!(matches!(err, SpeechError::Transcribe(_)));
    }

    #[test]
    fn language_is_forwarded_to_transport() {
        // The mock copies the language it is handed into a shared cell so the
        // test can assert on it after the provider consumes the mock.
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mock = MockCactus {
            transcript: "ok".into(),
            language_recorder: Some(seen.clone()),
            ..Default::default()
        };
        let mut p = WhistleProvider::with_transport(Box::new(mock));
        p.load(Path::new("/tmp/whistle.cactus")).unwrap();
        p.transcribe(&samples(1000), "fr").unwrap();
        assert_eq!(*seen.lock().unwrap(), "fr");
    }

    #[test]
    fn f32_to_pcm16_clamps_and_scales() {
        let out = f32_to_pcm16(&[0.0, 1.0, -1.0, 5.0, -5.0]);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], i16::MAX);
        assert_eq!(out[2], -i16::MAX);
        assert_eq!(out[3], i16::MAX); // clamped
        assert_eq!(out[4], -i16::MAX); // clamped
    }
}
