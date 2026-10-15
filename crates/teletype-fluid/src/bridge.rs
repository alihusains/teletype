//! The real provider: a safe Rust wrapper over the `TeletypeFluid` Swift C
//! ABI (see `swift/Sources/TeletypeFluid/Bridge.swift`).
//!
//! Contract mirrored from the Swift side:
//! - `ttf_init` loads (downloading when needed) the Parakeet models for
//!   `version` (`ultra|v3|v2`) under `models_dir`; returns 0 on success, -1
//!   on failure and writes a NUL-terminated message into `err`.
//! - `ttf_transcribe` returns 0 on success, a positive byte count when `out`
//!   is too small (retry with a buffer of exactly that size), -1 on failure;
//!   failure detail is available via `ttf_last_error`.
//! - `ttf_set_vocab` returns 0/-1; detail via `ttf_last_error`.
//! - `ttf_is_ready` reports 1 while models are loaded; `ttf_unload` releases
//!   them (the process-wide session, one owner expected).
//! - Every call blocks its thread; the first `load` may download models and
//!   takes as long as the network allows.

use std::ffi::{c_char, CString};
use std::path::Path;

use teletype_speech::{BoostTerm, ModelCapabilities, SpeechError, SpeechProvider, Transcript};

use crate::format::{
    boost_terms_json, decode_buffer, initial_out_capacity, parse_transcript_json, MAX_OUT_CAPACITY,
};
use crate::FluidVersion;

// Raw entry points exported by the Swift static library (`@_cdecl`).
//
// Contract mirrored from Bridge.swift:
// - `ttf_init` loads (downloading when needed) the Parakeet models for
//   `version` (`ultra|v3|v2`) under `models_dir`; returns 0 on success, -1
//   on failure and writes a NUL-terminated message into `err`.
// - `ttf_transcribe` returns 0 on success, a positive byte count when `out`
//   is too small (retry with a buffer of exactly that size), -1 on failure;
//   failure detail is available via `ttf_last_error`.
// - `ttf_set_vocab` returns 0/-1; detail via `ttf_last_error`.
// - `ttf_is_ready` reports 1 while models are loaded; `ttf_unload` releases
//   them (the process-wide session, one owner expected).
// - Every call blocks its thread; the first `load` may download models and
//   takes as long as the network allows.
//
// Every pointer parameter must be non-null and valid for the call; buffer
// capacities are passed in bytes (including the trailing NUL).
extern "C" {
    fn ttf_init(
        version: *const c_char,
        models_dir: *const c_char,
        err: *mut c_char,
        err_cap: usize,
    ) -> i32;
    fn ttf_is_ready() -> i32;
    fn ttf_set_vocab(json: *const c_char) -> i32;
    fn ttf_transcribe(
        pcm: *const f32,
        sample_count: usize,
        language: *const c_char,
        out: *mut c_char,
        out_cap: usize,
    ) -> i32;
    fn ttf_unload();
    fn ttf_last_error(out: *mut c_char, out_cap: usize) -> i32;
}

/// FluidAudio's Parakeet Core ML provider.
///
/// The bridge owns one process-wide session (models + optional boost
/// vocabulary), so use one live instance at a time. Creating, `load`ing, and
/// transcribing all block; call them from a worker thread. Dropping the
/// instance unloads the session.
#[derive(Debug)]
pub struct FluidParakeet {
    version: FluidVersion,
    loaded: bool,
    last_detected: Vec<String>,
    last_applied: Vec<String>,
}

impl FluidParakeet {
    /// Create an unloaded provider for `version`; nothing touches the disk or
    /// network until [`SpeechProvider::load`].
    pub fn new(version: FluidVersion) -> Self {
        Self {
            version,
            loaded: false,
            last_detected: Vec::new(),
            last_applied: Vec::new(),
        }
    }

    /// The requested checkpoint family.
    pub fn version(&self) -> FluidVersion {
        self.version
    }

    /// Whether the Swift bridge currently holds loaded models.
    pub fn is_ready(&self) -> bool {
        // SAFETY: no arguments; the bridge only reads its own session state.
        self.loaded && unsafe { ttf_is_ready() } == 1
    }

    /// Vocabulary terms the CTC spotter detected on the last detailed
    /// transcription (canonical text, time order). Empty when boosting is off
    /// or nothing matched.
    pub fn last_detected_terms(&self) -> &[String] {
        &self.last_detected
    }

    /// Vocabulary terms actually applied as corrections on the last detailed
    /// transcription.
    pub fn last_applied_terms(&self) -> &[String] {
        &self.last_applied
    }
}

/// Last bridge error message (empty when the bridge has none, or if it cannot
/// produce one within the growth budget).
fn last_error() -> String {
    let mut capacity = 1024usize;
    loop {
        let mut buffer = vec![0u8; capacity];
        // SAFETY: `buffer` is a valid writable region and its length is passed
        // as the capacity, which is the callee's stated contract.
        let code = unsafe { ttf_last_error(buffer.as_mut_ptr().cast::<c_char>(), buffer.len()) };
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

impl SpeechProvider for FluidParakeet {
    /// Load the models for the configured version. `path` is the FluidAudio
    /// models base directory (the app's `models/fluid`); the checkpoint lands
    /// in `<path>/<repo folder>/` and is downloaded when absent. Blocking and
    /// potentially slow on first run — worker thread only.
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        let directory = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
            SpeechError::ModelLoad(format!(
                "FluidAudio model directory contains a NUL byte: {}",
                path.display()
            ))
        })?;
        let version = CString::new(self.version.as_str()).expect("static version string");
        let mut err = vec![0u8; 512];
        // SAFETY: both string pointers are valid NUL-terminated C strings that
        // live for the call, and `err` is a valid writable region whose length
        // is passed as its capacity.
        let code = unsafe {
            ttf_init(
                version.as_ptr(),
                directory.as_ptr(),
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
                "FluidAudio init failed".to_string()
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
            unsafe { ttf_unload() };
        }
        self.loaded = false;
        self.last_detected.clear();
        self.last_applied.clear();
    }

    fn is_loaded(&self) -> bool {
        self.is_ready()
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        Ok(self.transcribe_detailed(samples, language)?.text)
    }

    /// Detailed transcription with per-word timings (Neural Engine path).
    ///
    /// `language` is a BCP-47 hint ("en-US", "auto", ...). FluidAudio uses it
    /// for script-aware token filtering on the v3 family and ignores it
    /// otherwise; it is never reported back as a detected language.
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
                ttf_transcribe(
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
                    self.last_detected = parsed.detected_terms;
                    self.last_applied = parsed.applied_terms;
                    return Ok(parsed.transcript);
                }
                code if code > 0 => {
                    let needed = code as usize;
                    if needed > MAX_OUT_CAPACITY {
                        return Err(SpeechError::Transcribe(format!(
                            "FluidAudio needs a {needed} byte result buffer (limit {MAX_OUT_CAPACITY})"
                        )));
                    }
                    capacity = needed;
                }
                _ => {
                    let message = last_error();
                    return Err(SpeechError::Transcribe(if message.is_empty() {
                        "FluidAudio transcribe failed".to_string()
                    } else {
                        message
                    }));
                }
            }
        }
    }

    /// Replace the CTC vocabulary-boosting terms. Non-empty lists load the
    /// auxiliary CTC models on first use (blocking, from FluidAudio's own
    /// cache); an empty list clears boosting without any model work.
    fn set_boost_terms(&mut self, terms: &[BoostTerm]) -> Result<(), SpeechError> {
        if !self.is_ready() {
            return Err(SpeechError::NoModel);
        }
        let json = CString::new(boost_terms_json(terms))
            .map_err(|_| SpeechError::ModelLoad("boost terms contain a NUL byte".to_string()))?;
        // SAFETY: `json` is a valid NUL-terminated C string for the call.
        let code = unsafe { ttf_set_vocab(json.as_ptr()) };
        if code == 0 {
            return Ok(());
        }
        let message = last_error();
        Err(SpeechError::ModelLoad(if message.is_empty() {
            "FluidAudio vocabulary boost setup failed".to_string()
        } else {
            message
        }))
    }

    /// FluidAudio supports native vocabulary boosting (CTC rescoring) and
    /// per-word token timings.
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_boost: true,
            word_timings: true,
        }
    }
}

impl Drop for FluidParakeet {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// The bridge session is process-wide, and cargo runs tests in parallel
    /// threads, so every test that touches it takes this lock. Poisoning is
    /// irrelevant for the assertions here; recover the guard.
    fn ffi_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The FFI error paths that are safe to exercise without any model
    /// weights: a bogus version string, malformed vocabulary JSON, and an
    /// empty vocabulary list (which needs no models at all). None of these
    /// may touch the disk or the network.
    #[test]
    fn bridge_error_paths_are_clean() {
        let _guard = ffi_lock();

        // Unknown version: rejected by the Swift side before any download.
        let version = CString::new("banana").expect("static");
        let directory = CString::new("/nonexistent/teletype-fluid-test").expect("static");
        let mut err = vec![0u8; 256];
        // SAFETY: both string pointers are valid NUL-terminated C strings and
        // `err` is a valid writable region whose length is passed along.
        let code = unsafe {
            ttf_init(
                version.as_ptr(),
                directory.as_ptr(),
                err.as_mut_ptr().cast::<c_char>(),
                err.len(),
            )
        };
        assert_eq!(code, -1, "bogus version must fail");
        let message = decode_buffer(&err);
        assert!(
            message.contains("unknown FluidAudio model version"),
            "unexpected init error: {message}"
        );

        // Malformed vocabulary JSON: rejected before any model state changes.
        let bad = CString::new("{not json").expect("static");
        // SAFETY: `bad` is a valid NUL-terminated C string for the call.
        let code = unsafe { ttf_set_vocab(bad.as_ptr()) };
        assert_eq!(code, -1, "malformed JSON must fail");
        let message = last_error();
        assert!(
            message.contains("bad JSON"),
            "unexpected last error: {message}"
        );

        // Clearing the vocabulary list needs no models and must succeed.
        let empty = CString::new("[]").expect("static");
        // SAFETY: `empty` is a valid NUL-terminated C string for the call.
        let code = unsafe { ttf_set_vocab(empty.as_ptr()) };
        assert_eq!(code, 0, "clearing boost terms must succeed");

        // A provider whose `load` was never called refuses to transcribe.
        let mut provider = FluidParakeet::new(FluidVersion::Ultra);
        let error = provider
            .transcribe(&[0.0; 160], "en")
            .expect_err("unloaded provider must refuse");
        assert!(matches!(error, SpeechError::NoModel));
    }

    /// End-to-end test against real weights. Disabled unless
    /// `TELETYPE_FLUID_MODELS` points at a models base directory (an empty
    /// directory downloads into it — hundreds of MB), with
    /// `TELETYPE_FLUID_VERSION` optionally selecting `ultra` (default),
    /// `v3`, or `v2`. Never fails when the env var is unset.
    ///
    /// Speaks a sentence with macOS `say` (16 kHz mono WAV via `afconvert`)
    /// and asserts FluidAudio transcribes it with sane per-word timings;
    /// falls back to a silence smoke check when the TTS tools are missing.
    #[test]
    fn model_gated_end_to_end_transcription() {
        let Some(directory) = std::env::var_os("TELETYPE_FLUID_MODELS") else {
            return; // no weights requested on this machine
        };
        let version = match std::env::var("TELETYPE_FLUID_VERSION").as_deref() {
            Ok("v2") => FluidVersion::V2,
            Ok("v3") => FluidVersion::V3,
            _ => FluidVersion::Ultra,
        };

        let _guard = ffi_lock();
        let mut provider = FluidParakeet::new(version);
        provider
            .load(Path::new(&directory))
            .expect("loading real FluidAudio models must succeed");
        assert!(provider.is_loaded(), "provider reports loaded models");

        if let Some(samples) = synthesized_speech() {
            let detailed = provider
                .transcribe_detailed(&samples, "en")
                .expect("speech must transcribe without error");
            println!(
                "fluid e2e: text={:?} confidence={:.3} words={} first_words={:?}",
                detailed.text,
                detailed.confidence,
                detailed.words.len(),
                detailed
                    .words
                    .iter()
                    .take(6)
                    .map(|word| word.text.as_str())
                    .collect::<Vec<_>>()
            );
            let normalized = detailed.text.to_lowercase();
            assert!(
                normalized.contains("teletype") || normalized.contains("parakeet"),
                "expected a distinctive word from the phrase in {:?}",
                detailed.text
            );
            assert!(
                detailed.words.len() >= 4,
                "expected word timings for a full sentence, got {} in {:?}",
                detailed.words.len(),
                detailed.text
            );
            assert_word_timings_are_sane(&detailed);
        } else {
            println!("fluid e2e: `say`/`afconvert` unavailable; silence smoke check only");
            let silence = vec![0.0f32; 16_000];
            let detailed = provider
                .transcribe_detailed(&silence, "en")
                .expect("silence must transcribe without error");
            assert_word_timings_are_sane(&detailed);
        }

        provider.unload();
        assert!(!provider.is_loaded());
    }

    /// Confidence in range, word timings ordered and non-overlapping.
    fn assert_word_timings_are_sane(transcript: &Transcript) {
        assert!(
            (0.0..=1.0).contains(&transcript.confidence),
            "confidence out of range: {}",
            transcript.confidence
        );
        let mut previous_end = 0u32;
        for word in &transcript.words {
            assert!(
                word.start_ms <= word.end_ms,
                "word {:?} has inverted timing {}..{}",
                word.text,
                word.start_ms,
                word.end_ms
            );
            assert!(
                word.start_ms >= previous_end.saturating_sub(1),
                "word {:?} starts before the previous word ends",
                word.text
            );
            previous_end = word.end_ms;
        }
    }

    /// Speak a sentence with macOS TTS and decode it to 16 kHz mono f32.
    fn synthesized_speech() -> Option<Vec<f32>> {
        use std::process::Command;

        const PHRASE: &str = "Hello Teletype, the local parakeet engine is ready.";
        let workdir = std::env::temp_dir().join("teletype-fluid-e2e");
        std::fs::create_dir_all(&workdir).ok()?;
        let aiff = workdir.join("phrase.aiff");
        let wav = workdir.join("phrase.wav");

        let spoken = Command::new("say")
            .arg("-o")
            .arg(&aiff)
            .arg(PHRASE)
            .status()
            .ok()?;
        if !spoken.success() {
            return None;
        }
        let converted = Command::new("afconvert")
            .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
            .arg(&aiff)
            .arg(&wav)
            .status()
            .ok()?;
        if !converted.success() {
            return None;
        }
        read_wav_pcm16(&wav)
    }

    /// Minimal RIFF/WAVE reader for the 16 kHz mono PCM16 files `afconvert`
    /// writes (chunk-walking, so extra chunks like LIST are tolerated).
    fn read_wav_pcm16(path: &Path) -> Option<Vec<f32>> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return None;
        }
        let mut offset = 12;
        let mut saw_16k_mono = false;
        while offset + 8 <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
            let start = offset + 8;
            if id == b"fmt " && size >= 16 {
                let format = u16::from_le_bytes(bytes[start..start + 2].try_into().ok()?);
                let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().ok()?);
                let rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().ok()?);
                saw_16k_mono = format == 1 && channels == 1 && rate == 16_000;
            }
            if id == b"data" {
                if !saw_16k_mono {
                    return None;
                }
                let end = start.checked_add(size)?.min(bytes.len());
                return Some(
                    bytes[start..end]
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
                        .collect(),
                );
            }
            offset = start + size + (size & 1);
        }
        None
    }
}
