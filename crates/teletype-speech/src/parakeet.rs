//! NVIDIA Parakeet TDT backend via whisper.cpp's `parakeet` engine (C API).
//!
//! Parakeet TDT 0.6B v3 is the state-of-the-art streaming dictation model:
//! 25× faster than real time with punctuation, and more accurate than
//! Whisper for English dictation. The engine is compiled from whisper.cpp
//! v1.9.4 by the `parakeet-sys` crate.

use std::path::Path;

use crate::{SpeechError, SpeechProvider};

// ---- FFI (subset of whisper.cpp include/parakeet.h, v1.9.4) ----

#[allow(non_camel_case_types)]
#[repr(C)]
struct parakeet_context_params {
    use_gpu: bool,
    gpu_device: i32,
}

#[repr(C)]
struct parakeet_full_params {
    strategy: i32,
    n_threads: i32,
    offset_ms: i32,
    duration_ms: i32,
    no_context: bool,
    audio_ctx: i32,
    new_segment_callback: *mut std::ffi::c_void,
    new_segment_callback_user_data: *mut std::ffi::c_void,
    new_token_callback: *mut std::ffi::c_void,
    new_token_callback_user_data: *mut std::ffi::c_void,
    progress_callback: *mut std::ffi::c_void,
    progress_callback_user_data: *mut std::ffi::c_void,
    encoder_begin_callback: *mut std::ffi::c_void,
    encoder_begin_callback_user_data: *mut std::ffi::c_void,
    abort_callback: *mut std::ffi::c_void,
    abort_callback_user_data: *mut std::ffi::c_void,
}

const PARAKEET_SAMPLING_GREEDY: i32 = 0;

extern "C" {
    fn parakeet_init_from_file_with_params(
        path_model: *const std::ffi::c_char,
        params: parakeet_context_params,
    ) -> *mut std::ffi::c_void;
    fn parakeet_free(ctx: *mut std::ffi::c_void);
    fn parakeet_full_default_params(strategy: i32) -> parakeet_full_params;
    fn parakeet_full(
        ctx: *mut std::ffi::c_void,
        params: parakeet_full_params,
        samples: *const f32,
        n_samples: i32,
    ) -> i32;
    fn parakeet_full_n_segments(ctx: *mut std::ffi::c_void) -> i32;
    fn parakeet_full_get_segment_text(
        ctx: *mut std::ffi::c_void,
        i_segment: i32,
    ) -> *const std::ffi::c_char;
    fn parakeet_free_params(params: *mut parakeet_full_params);
}

/// A Parakeet TDT model wrapper.
pub struct ParakeetProvider {
    ctx: *mut std::ffi::c_void,
}

// SAFETY: The opaque C context is owned exclusively by this handle and only
// ever accessed through it; freed exactly once in Drop.
unsafe impl Send for ParakeetProvider {}

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
        if !self.ctx.is_null() {
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
            use_gpu: cfg!(target_os = "macos"),
            gpu_device: 0,
        };
        // SAFETY: c_path is a valid NUL-terminated C string; params is a valid
        // context-params struct. Returns null on failure, which we check.
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

        // parakeet_full_default_params allocates; free it afterwards.
        // SAFETY: returns a valid default params struct for the given strategy.
        let mut params = unsafe { parakeet_full_default_params(PARAKEET_SAMPLING_GREEDY) };
        params.n_threads = std::cmp::max(
            2,
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
        ) as i32;

        let params_ptr = &params as *const parakeet_full_params as *mut parakeet_full_params;
        // SAFETY: ctx is non-null and valid; params is a valid, fully
        // initialized struct; samples points to a valid f32 buffer of the
        // given length.
        let ret = unsafe { parakeet_full(ctx, params, samples.as_ptr(), samples.len() as i32) };
        if ret != 0 {
            // SAFETY: params_ptr is the valid pointer we passed to parakeet_full.
            unsafe {
                parakeet_free_params(params_ptr);
            }
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
                // SAFETY: c is a non-null C string pointer returned by parakeet.
                let s = unsafe { std::ffi::CStr::from_ptr(c) }
                    .to_string_lossy()
                    .into_owned();
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(s.trim());
            }
        }
        // SAFETY: params_ptr is the valid pointer we passed to parakeet_full.
        unsafe {
            parakeet_free_params(params_ptr);
        }

        let text = text.trim().to_string();
        if text.is_empty() {
            Err(SpeechError::NoSpeech)
        } else {
            Ok(text)
        }
    }
}
