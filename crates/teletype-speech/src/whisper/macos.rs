use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::{SpeechError, SpeechProvider};

/// A whisper.cpp model wrapper.
pub struct WhisperProvider {
    context: Option<WhisperContext>,
    state: Option<WhisperState>,
    /// Last detected/used language code (e.g. "en", "ru").
    detected_language: Option<String>,
}

impl WhisperProvider {
    pub fn new() -> Self {
        Self {
            context: None,
            state: None,
            detected_language: None,
        }
    }

    /// The language detected on the last auto-detect transcription, if any.
    pub fn detected_language(&self) -> Option<String> {
        self.detected_language.clone()
    }
}

impl Default for WhisperProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechProvider for WhisperProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.is_loaded() {
            return Ok(());
        }
        let params = WhisperContextParameters::default();
        let context = WhisperContext::new_with_params(path, params)
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        let state = context
            .create_state()
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        self.context = Some(context);
        self.state = Some(state);
        Ok(())
    }

    fn unload(&mut self) {
        self.state = None;
        self.context = None;
        self.detected_language = None;
    }

    fn is_loaded(&self) -> bool {
        self.state.is_some()
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        let state = self.state.as_mut().ok_or(SpeechError::NoModel)?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if language != "auto" && !language.is_empty() {
            params.set_language(Some(language));
        }
        // P8: on Apple Silicon, efficiency cores slow whisper.cpp down, so cap
        // the worker threads at the performance-core count when we can see it.
        params.set_n_threads(whisper_thread_count());
        // Dictation utterances are short; skip timestamp bookkeeping.
        params.set_no_timestamps(true);

        state
            .full(params, samples)
            .map_err(|e| SpeechError::Transcribe(e.to_string()))?;

        // Remember what the engine detected so the UI can show a chip and
        // the user can lock the language. Only meaningful for auto-detect
        // runs; a locked language always yields the same code.
        self.detected_language = if language == "auto" || language.is_empty() {
            let id = state.full_lang_id_from_state();
            if id > 0 {
                whisper_rs::get_lang_str(id).map(str::to_string)
            } else {
                None
            }
        } else {
            Some(language.to_string())
        };

        let n_segments = state.full_n_segments();
        if n_segments == 0 {
            return Err(SpeechError::NoSpeech);
        }

        let mut text = String::new();
        for seg in state.as_iter() {
            if let Ok(s) = seg.to_str() {
                text.push_str(s);
            }
        }

        let text = text.trim().to_string();
        if text.is_empty() {
            Err(SpeechError::NoSpeech)
        } else {
            Ok(text)
        }
    }

    fn detected_language(&self) -> Option<String> {
        self.detected_language.clone()
    }
}

/// Worker-thread count for whisper.cpp. On Apple Silicon this is the
/// performance-core logical CPU count (`hw.perflevel0.logicalcpu`); efficiency
/// cores measurably slow the Metal backend down. Falls back to the total
/// available parallelism elsewhere.
fn whisper_thread_count() -> i32 {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_int;
        extern "C" {
            fn sysctl(
                name: *const i8,
                oldp: *mut c_int,
                oldlenp: *mut usize,
                newp: *mut c_int,
                newlen: usize,
            ) -> c_int;
        }
        let mut count: c_int = 0;
        let mut len = std::mem::size_of::<c_int>();
        // `hw.perflevel0.logicalcpu`
        let key = b"hw.perflevel0.logicalcpu\0";
        // sysctl takes a NUL-terminated name; the buffer must outlive the call.
        let r = unsafe {
            sysctl(
                key.as_ptr() as *const i8,
                &mut count,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r == 0 && count > 0 {
            return count.max(2) as i32;
        }
    }
    std::cmp::max(
        2,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    ) as i32
}
