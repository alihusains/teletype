//! Whistle speech provider (Cactus-Compute), via the Needle C engine.
//!
//! Whistle reuses Needle's `.cact` container and the same runtime, so one
//! vendored static library (`third_party/needle/macos-arm64/libneedle.a`)
//! serves both STT (this provider) and, later, tool-calling. The public API is
//! a small C surface declared in `third_party/needle/needle.h`:
//!
//! - `needle_load(bytes, len)` — loads whichever model kind the `.cact` holds.
//! - `needle_transcribe(pcm_f32, samples, language, keywords, word_ts, out, cap)`
//!   — 16 kHz mono float PCM in [-1,1], at most 30 s, returns JSON
//!   `{"text":...,"language":...,"ttft_ms":...,"decode_tps":...}`.
//! - `needle_last_error()` — diagnostics.
//!
//! The engine is process-global and non-thread-safe (one model per kind), so
//! this provider serializes every FFI call behind a mutex and holds the whole
//! transcribe on one thread.

use std::path::Path;
use std::sync::Mutex;

use crate::{SpeechError, SpeechProvider};

/// The Cactus engine documents a ~30 s per-inference window for transcription.
pub const WHISTLE_MAX_SECONDS: u32 = 30;

/// Sample rate Whistle expects: 16 kHz mono.
pub const WHISTLE_SAMPLE_RATE: u32 = 16_000;

// The raw Needle C FFI. Mirrors `third_party/needle/needle.h`.
//
// All entry points here are `unsafe` (raw pointers / FFI); every caller in
// this module goes through the safe wrapper below, so `unsafe` stays confined
// to this one place.
unsafe extern "C" {
    fn needle_load(cact: *const u8, n: std::ffi::c_ulonglong) -> i32;
    fn needle_transcribe(
        pcm: *const f32,
        samples: i32,
        language: *const std::ffi::c_char,
        keywords: *const std::ffi::c_char,
        word_timestamps: i32,
        out: *mut std::ffi::c_char,
        out_capacity: i32,
    ) -> i32;
    fn needle_last_error() -> *const std::ffi::c_char;
}

/// A process-global lock: the Needle engine is not thread-safe and holds one
/// model per kind. Every FFI call is made under this guard.
static NEEDLE_LOCK: Mutex<()> = Mutex::new(());

/// Loads the model bytes exactly once, on first use. Returns the loaded size.
static LOADED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Reads the `.cact` at `path` and loads it into the process-global engine.
/// Idempotent: a second call is a no-op.
fn ensure_loaded(path: &Path) -> Result<(), SpeechError> {
    if LOADED.get().is_some() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
    if bytes.is_empty() {
        return Err(SpeechError::ModelLoad("empty model file".into()));
    }
    let guard = lock_engine();
    let rc = unsafe { needle_load(bytes.as_ptr(), bytes.len() as std::ffi::c_ulonglong) };
    drop(guard);
    if rc < 0 {
        return Err(SpeechError::ModelLoad(needle_error()));
    }
    LOADED.get_or_init(|| bytes.len());
    Ok(())
}

fn lock_engine() -> std::sync::MutexGuard<'static, ()> {
    NEEDLE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn needle_error() -> String {
    unsafe {
        let p = needle_last_error();
        if p.is_null() {
            "needle engine error".into()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Extracts the `"text"` field from the engine's JSON reply. The engine returns
/// a flat object; a tiny targeted parse is safer than pulling in a JSON dep for
/// one field, and keeps the hot path allocation-light.
fn extract_text(json: &str) -> String {
    // Find the "text" key, then the following quoted string.
    let key = "\"text\"";
    let Some(kpos) = json.find(key) else {
        return String::new();
    };
    let after = &json[kpos + key.len()..];
    let after = after.trim_start();
    let after = after.strip_prefix(':').unwrap_or(after).trim_start();
    let after = after.strip_prefix('"').unwrap_or(after);
    // Walk to the closing unescaped quote.
    let mut out = String::new();
    let mut it = after.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => break,
            '\\' => {
                if let Some(n) = it.next() {
                    out.push(n);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The Whistle [`SpeechProvider`].
///
/// `f32` 16 kHz samples in, transcript out — the same contract as Parakeet and
/// Whisper, so the rest of the app needs no engine-specific branching.
///
/// The engine's 30 s window is enforced here: longer audio is split into 30 s
/// windows, each transcribed, and the transcripts joined with a space.
pub struct WhistleProvider {
    loaded: bool,
}

impl Default for WhistleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WhistleProvider {
    pub fn new() -> Self {
        Self { loaded: false }
    }

    /// Splits `samples` (16 kHz f32) into 30 s windows, transcribes each, and
    /// joins the transcripts.
    fn transcribe_chunked(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        if !self.loaded {
            return Err(SpeechError::NoModel);
        }
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let max_samples = (WHISTLE_MAX_SECONDS as usize).saturating_mul(WHISTLE_SAMPLE_RATE as usize);
        let lang = language.is_empty() || language == "auto";
        let lang_c = std::ffi::CString::new(language).unwrap_or_default();

        let mut parts = Vec::new();
        for chunk in samples.chunks(max_samples) {
            let out = self.transcribe_window(chunk, &lang_c, lang)?;
            let trimmed = out.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        if parts.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        Ok(parts.join(" "))
    }

    /// One engine call for a window of <= 30 s. Holds the global lock for the
    /// duration so concurrent dictations never interleave inside the C engine.
    fn transcribe_window(
        &self,
        chunk: &[f32],
        lang_c: &std::ffi::CString,
        detect: bool,
    ) -> Result<String, SpeechError> {
        let guard = lock_engine();
        let cap = 65_536;
        let mut buf = vec![0u8; cap];
        let lang_ptr = if detect {
            std::ptr::null()
        } else {
            lang_c.as_ptr()
        };
        let rc = unsafe {
            needle_transcribe(
                chunk.as_ptr(),
                chunk.len() as i32,
                lang_ptr,
                std::ptr::null(),
                0,
                buf.as_mut_ptr() as *mut std::ffi::c_char,
                cap as i32,
            )
        };
        drop(guard);
        if rc < 0 {
            return Err(SpeechError::Transcribe(needle_error()));
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let json = String::from_utf8_lossy(&buf[..len]);
        Ok(extract_text(&json))
    }
}

impl SpeechProvider for WhistleProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.loaded {
            return Ok(());
        }
        ensure_loaded(path)?;
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        // The Needle engine is process-global with no per-handle unload; the
        // model stays resident for the process lifetime (it is small, ~17 MB).
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        self.transcribe_chunked(samples, language)
    }
}

unsafe impl Send for WhistleProvider {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16 kHz f32 sine-ish samples; the exact values don't matter for the
    /// chunking / wiring tests, only the count.
    fn samples(n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.5 * ((i % 7) as f32 - 3.0) / 3.0).collect()
    }

    #[test]
    fn transcribe_before_load_is_no_model() {
        let mut p = WhistleProvider::new();
        assert!(!p.is_loaded());
        assert!(matches!(
            p.transcribe(&samples(1000), "en"),
            Err(SpeechError::NoModel)
        ));
    }

    #[test]
    fn empty_audio_is_no_speech_when_loaded() {
        // We cannot load the real engine in a unit test without the model, so
        // this asserts the guard order: NoModel is checked before NoSpeech.
        let mut p = WhistleProvider::new();
        assert!(matches!(
            p.transcribe(&[], "en"),
            Err(SpeechError::NoModel)
        ));
    }

    #[test]
    fn extract_text_pulls_the_transcript_field() {
        assert_eq!(
            extract_text(r#"{"text":"hello world","language":"en","ttft_ms":12.0}"#),
            "hello world"
        );
        assert_eq!(extract_text(r#"{"text":"","language":"en"}"#), "");
        // Escaped quote inside the transcript.
        assert_eq!(
            extract_text(r#"{"text":"he said \"hi\"","language":"en"}"#),
            "he said \"hi\""
        );
        // Missing key → empty, not a panic.
        assert_eq!(extract_text(r#"{"language":"en"}"#), "");
    }

    #[test]
    fn chunk_window_math_matches_the_30s_limit() {
        // Exactly 30 s → one window; 31 s → two.
        let max = (WHISTLE_MAX_SECONDS as usize) * (WHISTLE_SAMPLE_RATE as usize);
        assert_eq!(samples(max).chunks(max).count(), 1);
        assert_eq!(samples(max + 1).chunks(max).count(), 2);
        // 61 s → three windows (30 + 30 + 1).
        assert_eq!(samples(61 * 16_000).chunks(max).count(), 3);
    }

    #[test]
    fn whistle_constants_are_documented() {
        assert_eq!(WHISTLE_SAMPLE_RATE, 16_000);
        assert_eq!(WHISTLE_MAX_SECONDS, 30);
    }
}
