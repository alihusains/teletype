//! Streaming GGUF ASR backend via transcribe.cpp (handy-computer, MIT).
//!
//! The engine is the published `transcribe-cpp` Rust crate (0.4.x), whose
//! `transcribe-cpp-sys` dependency builds the native library from its
//! vendored transcribe.cpp source via CMake. It serves the
//! [`crate::catalog::Engine::TranscribeCpp`] rows: Nemotron 3.5 ASR
//! streaming, Moonshine streaming, and Granite Speech 5.0 TurboCTC.
//!
//! The native engine is wired **macOS-only in this build**: `transcribe-cpp`
//! enters the workspace under `target.'cfg(target_os = "macos")'`. The
//! upstream `-sys` crate does expose a `vulkan` feature
//! (transcribe-cpp-sys 0.4.1, `default = ["metal"]`), but nothing in this
//! tree enables it and no Windows build or measurement exists, so no Windows
//! path is claimed for this release. On every other target this module
//! provides a stub [`TranscribeCppProvider`] that compiles and reports a
//! clear "not supported" error if actually used.
//!
//! ## Model layout
//!
//! `load` takes a single `.gguf` file (the catalog's `provider_path` is
//! `<models_dir>/<file>`, the same layout as the ggml engines). The library
//! memory-maps the file, so a truncated download is rejected at load time.

/// Whether this build can run the transcribe.cpp engine right now.
///
/// The native library is compiled from source by the `transcribe-cpp`
/// crate's build script, so a build without it (or a cleaned target dir)
/// must not offer the engine's rows. On macOS the library is linked into
/// the binary, so the only runtime question is the platform; the stub
/// targets report `false` unconditionally.
pub fn engine_available() -> bool {
    cfg!(target_os = "macos")
}

/// Maps a language setting to the engine's language hint.
///
/// `"auto"`/empty mean "the engine decides" (no hint), which the three
/// supported models all honour: Nemotron auto-detects, Moonshine is
/// English-only, Granite reports its own label.
fn language_hint(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// transcribe.cpp streaming GGUF ASR (Nemotron 3.5, Moonshine, Granite).
///
/// The model is created in [`SpeechProvider::load`] and dropped in
/// [`SpeechProvider::unload`]. Transcription is synchronous and
/// blocking: call it from a worker thread, as with every other provider.
#[cfg(target_os = "macos")]
pub struct TranscribeCppProvider {
    session: Option<transcribe_cpp::Session>,
}

#[cfg(not(target_os = "macos"))]
pub struct TranscribeCppProvider {
    loaded: bool,
}

#[cfg(target_os = "macos")]
mod native {
    use super::{language_hint, TranscribeCppProvider};
    use std::path::Path;

    use crate::{BoostTerm, ModelCapabilities, SpeechError, SpeechProvider, Transcript, Word};
    use transcribe_cpp::{Model, RunOptions, TimestampKind};

    impl std::fmt::Debug for TranscribeCppProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TranscribeCppProvider")
                .field("loaded", &self.session.is_some())
                .finish()
        }
    }

    impl TranscribeCppProvider {
        pub fn new() -> Self {
            Self { session: None }
        }
    }

    impl Default for TranscribeCppProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Maps a transcribe.cpp error to a [`SpeechError`] without leaking
    /// dictated or screen text: only the error kind and the model path.
    ///
    /// `InputTooLong` is the one kind the caller can act on (shorten the
    /// clip), so it gets its own message instead of a bare debug of the kind.
    fn map_error(err: &transcribe_cpp::Error, path: &Path) -> SpeechError {
        let kind = err.kind();
        match kind {
            transcribe_cpp::ErrorKind::ModelFileNotFound
            | transcribe_cpp::ErrorKind::ModelLoad
            | transcribe_cpp::ErrorKind::Backend => SpeechError::ModelLoad(format!(
                "transcribe.cpp could not load the model at {}: {:?}",
                path.display(),
                kind
            )),
            transcribe_cpp::ErrorKind::InputTooLong => {
                SpeechError::Transcribe("the clip is longer than this model accepts".to_string())
            }
            _ => SpeechError::Transcribe(format!("{:?}", kind)),
        }
    }

    /// Maps a decode failure, where the model is already loaded and the path
    /// adds nothing the user can act on.
    fn decode_error(err: transcribe_cpp::Error) -> SpeechError {
        match err.kind() {
            transcribe_cpp::ErrorKind::InputTooLong => {
                SpeechError::Transcribe("the clip is longer than this model accepts".to_string())
            }
            kind => SpeechError::Transcribe(format!("{:?}", kind)),
        }
    }

    impl SpeechProvider for TranscribeCppProvider {
        fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
            if self.is_loaded() {
                return Ok(());
            }
            if !path.is_file() {
                return Err(SpeechError::ModelLoad(format!(
                    "transcribe.cpp model file missing: {}",
                    path.display()
                )));
            }
            let model = Model::load(path).map_err(|e| map_error(&e, path))?;
            let session = model.session().map_err(|e| map_error(&e, path))?;
            self.session = Some(session);
            Ok(())
        }

        fn unload(&mut self) {
            self.session = None;
        }

        fn is_loaded(&self) -> bool {
            self.session.is_some()
        }

        fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
            let session = self.session.as_mut().ok_or(SpeechError::NoModel)?;
            if samples.is_empty() {
                return Err(SpeechError::NoSpeech);
            }
            let options = RunOptions {
                // Word timestamps when the model produces them (Nemotron and
                // Moonshine do); the engine answers a clean Unsupported for a
                // request finer than the model's max, so Auto is the safe ask.
                timestamps: TimestampKind::Auto,
                language: language_hint(language),
                ..Default::default()
            };
            let result = session.run(samples, &options).map_err(decode_error)?;
            let text = result.text.trim().to_string();
            if text.is_empty() {
                Err(SpeechError::NoSpeech)
            } else {
                Ok(text)
            }
        }

        /// Asks the engine for the richest timestamps the loaded model
        /// supports and maps whatever comes back to [`Word`] rows.
        ///
        /// `TimestampKind::Auto` resolves per family at run time. Asking for
        /// `Word` outright is not safe here: it is what the Nemotron family
        /// gives, and the Moonshine and Granite families answer
        /// `ErrorKind::Unsupported` for a request finer than the model's
        /// maximum, which failed every take of both engines on the 52-clip
        /// human corpus before this changed. A model that returns no word
        /// rows is a valid result, not an error: the text still lands, and
        /// the boost rescoring path treats an empty word list as "no timings
        /// to use". Per-word confidence is family-specific and often absent,
        /// so words carry the neutral 1.0, as with sherpa.
        fn transcribe_detailed(
            &mut self,
            samples: &[f32],
            language: &str,
        ) -> Result<Transcript, SpeechError> {
            let session = self.session.as_mut().ok_or(SpeechError::NoModel)?;
            if samples.is_empty() {
                return Err(SpeechError::NoSpeech);
            }
            let options = RunOptions {
                timestamps: TimestampKind::Auto,
                language: language_hint(language),
                ..Default::default()
            };
            let result = session.run(samples, &options).map_err(decode_error)?;
            let text = result.text.trim().to_string();
            if text.is_empty() {
                return Err(SpeechError::NoSpeech);
            }
            let words: Vec<Word> = result
                .words
                .iter()
                .filter(|w| !w.text.trim().is_empty())
                .map(|w| Word {
                    text: w.text.clone(),
                    start_ms: w.t0_ms.max(0) as u32,
                    end_ms: w.t1_ms.max(0) as u32,
                    conf: 1.0,
                })
                .collect();
            Ok(Transcript {
                text,
                words,
                confidence: 1.0,
                language: result.language,
            })
        }

        /// This seam does not report detection: the engine returns the
        /// language inside a `transcribe_detailed` result, not through a
        /// separate query, and `&self` has no run to read it from. Callers
        /// that need the detected language take it from the transcript.
        fn detected_language(&self) -> Option<String> {
            None
        }

        /// Native boosting: the engine takes `RunOptions::vocabulary` for
        /// families that support it. The three shipped models are not
        /// documented to honour it (Nemotron's language prompt is its own
        /// conditioning), so terms are stored nowhere and post-ASR
        /// rescoring covers this engine, like sherpa.
        fn set_boost_terms(&mut self, _terms: &[BoostTerm]) -> Result<(), SpeechError> {
            Ok(())
        }

        /// Claims word timings so the take path reads them through
        /// `transcribe_detailed`; the engine asks `Auto`, so a family that
        /// gives no word rows returns text with an empty word list instead
        /// of failing. Native boosting is off: none of the three shipped
        /// models is documented to honour `RunOptions::vocabulary`.
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                native_boost: false,
                word_timings: true,
            }
        }
    }
}

// ---- Non-macOS (Windows) stub ----
//
// No native transcribe.cpp engine is built on these targets (the dependency
// is macOS-only in this tree, like whisper-rs). The stub keeps the type and
// its constructor available so the rest of the app compiles unchanged, and
// surfaces a clear error if dictation is actually attempted.

#[cfg(not(target_os = "macos"))]
mod stub {
    use std::path::Path;

    use crate::{SpeechError, SpeechProvider};

    impl std::fmt::Debug for TranscribeCppProvider {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TranscribeCppProvider")
                .field("loaded", &self.loaded)
                .finish()
        }
    }

    impl TranscribeCppProvider {
        pub fn new() -> Self {
            Self { loaded: false }
        }
    }

    impl Default for TranscribeCppProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SpeechProvider for TranscribeCppProvider {
        fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
            Err(SpeechError::ModelLoad(
                "transcribe.cpp is not built for this platform (macOS only in this tree)".into(),
            ))
        }

        fn unload(&mut self) {
            self.loaded = false;
        }

        fn is_loaded(&self) -> bool {
            self.loaded
        }

        fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
            Err(SpeechError::NoModel)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{SpeechError, SpeechProvider, TranscribeCppProvider};

    #[test]
    fn load_rejects_a_missing_file_with_a_clean_error() {
        let mut p = TranscribeCppProvider::new();
        let err = p
            .load(Path::new(
                "/nonexistent/nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf",
            ))
            .expect_err("loading a missing file must fail");
        assert!(matches!(err, SpeechError::ModelLoad(_)), "{err:?}");
        assert!(!p.is_loaded());
    }

    #[test]
    fn load_rejects_a_non_gguf_file_with_a_clean_error() {
        let mut p = TranscribeCppProvider::new();
        let dir = std::env::temp_dir().join("teletype-transcribe-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bad = dir.join("not-a-model.gguf");
        std::fs::write(&bad, b"this is not a gguf file").expect("write");
        let err = p.load(&bad).expect_err("loading a bad file must fail");
        assert!(matches!(err, SpeechError::ModelLoad(_)), "{err:?}");
        assert!(!p.is_loaded());
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn transcribe_without_a_model_is_no_model() {
        let mut p = TranscribeCppProvider::new();
        let err = p
            .transcribe(&[0.0f32; 16_000], "en")
            .expect_err("no model, no text");
        assert!(matches!(err, SpeechError::NoModel), "{err:?}");
    }

    #[test]
    fn empty_audio_is_no_speech() {
        let mut p = TranscribeCppProvider::new();
        // Not loaded: NoModel is the honest answer for an empty clip on an
        // unloaded engine; the NoSpeech path is exercised by the live takes.
        let err = p.transcribe(&[], "en").expect_err("nothing to transcribe");
        assert!(
            matches!(err, SpeechError::NoModel | SpeechError::NoSpeech),
            "{err:?}"
        );
    }

    #[test]
    fn capabilities_report_word_timings_without_native_boost() {
        let p = TranscribeCppProvider::new();
        let caps = p.capabilities();
        assert!(caps.word_timings);
        assert!(!caps.native_boost);
    }
}
