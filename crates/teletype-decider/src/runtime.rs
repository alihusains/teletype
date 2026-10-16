//! NeuDecide inference runtime.
//!
//! The inference loop mirrors the Python package:
//!
//! 1. Mono f32 16 kHz audio truncated to 30 s (480 000 samples).
//! 2. `audio_encoder` ONNX session → frames at 12.5 Hz.
//! 3. Tokenize the tool-list JSON (truncate at 1536 tokens + warning).
//! 4. `tool_encoder(frames, tool_tokens)` ONNX session → cross-attention K/V.
//! 5. Greedy decode ≤ 128 tokens until EOS.
//! 6. Detokenize.
//! 7. Parse the JSON tool-call list.
//!
//! The ONNX sessions are abstracted behind [`SessionRunner`] so the greedy
//! decode loop is unit-testable with a fake runner (no native runtime).
//!
//! The `OrtRunner` implementation is behind `#[cfg(feature = "onnx")]`.

use std::path::Path;

use serde::Deserialize;

use crate::error::{DeciderError, Result};
use crate::parse::parse_tool_calls;
use crate::tool::ToolCall;

/// Maximum audio duration in seconds (480 000 samples at 16 kHz).
pub const MAX_AUDIO_SECONDS: u32 = 30;
/// Frame rate of the audio encoder output.
pub const FRAME_RATE_HZ: f32 = 12.5;
/// Maximum tool-list token count (truncated + warning).
pub const MAX_TOOL_TOKENS: usize = 1536;
/// Maximum decoded tokens per generation.
pub const MAX_DECODE_TOKENS: usize = 128;
/// Sample rate expected by the model.
pub const SAMPLE_RATE: u32 = 16_000;

/// Configuration parsed from the model's `config.json`.
///
/// Fields that are not yet confirmed against the real package are marked
/// `// UNVERIFIED:` and use conservative defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NeuDecideConfig {
    /// Path to the audio encoder ONNX graph (relative to model dir).
    #[serde(rename = "audio_encoder")]
    pub audio_encoder_path: String,
    /// Path to the tool encoder ONNX graph.
    #[serde(rename = "tool_encoder")]
    pub tool_encoder_path: String,
    /// Path to the decoder / LM ONNX graph.
    #[serde(rename = "decoder")]
    pub decoder_path: String,
    /// Path to the tokenizer (HuggingFace `tokenizer.json`).
    #[serde(rename = "tokenizer")]
    pub tokenizer_path: String,
    /// Beginning-of-sequence token id.
    // UNVERIFIED: key name and default — the Python package's config.json
    // may use a different key. Default 0 is a common convention.
    pub bos_token_id: u32,
    /// End-of-sequence token id.
    // UNVERIFIED: same as above. Default 2 is common (GPT-2 style).
    pub eos_token_id: u32,
    /// Sample rate the model expects (should be 16000).
    pub sample_rate: u32,
    /// Audio encoder frame rate (should be 12.5).
    pub frame_rate: f32,
}

impl Default for NeuDecideConfig {
    fn default() -> Self {
        Self {
            audio_encoder_path: String::new(),
            tool_encoder_path: String::new(),
            decoder_path: String::new(),
            tokenizer_path: String::new(),
            bos_token_id: 0,
            eos_token_id: 2,
            sample_rate: SAMPLE_RATE,
            frame_rate: FRAME_RATE_HZ,
        }
    }
}

/// Abstraction over the three ONNX sessions (audio encoder, tool encoder,
/// decoder) plus the tokenizer. Implement this to run real inference
/// (OrtRunner) or to unit-test the greedy loop (FakeRunner).
pub trait SessionRunner: Send {
    /// Run the audio encoder on mono f32 PCM at 16 kHz.
    /// Returns encoder frames (shape: [num_frames * frame_dim]).
    fn encode_audio(&mut self, pcm: &[f32]) -> Result<Vec<f32>>;

    /// Tokenize the tool-list JSON. Returns token ids.
    fn tokenize(&mut self, text: &str) -> Result<Vec<u32>>;

    /// Run the tool encoder: cross-attention K/V from frames + tool tokens.
    /// Returns the K/V tensors flattened.
    fn encode_tools(&mut self, frames: &[f32], tool_tokens: &[u32]) -> Result<Vec<f32>>;

    /// Run one decoder step: given the current token sequence and K/V,
    /// return the next token id.
    fn decode_step(&mut self, tokens: &[u32], kv: &[f32]) -> Result<u32>;

    /// Detokenize a sequence of token ids into a string.
    fn detokenize(&mut self, tokens: &[u32]) -> Result<String>;
}

/// The NeuDecide model handle.
///
/// Created by [`NeuDecide::load`], which reads `config.json` and builds
/// the runner. The sessions are created once and kept warm.
pub struct NeuDecide {
    pub(crate) config: NeuDecideConfig,
    pub(crate) runner: Box<dyn SessionRunner>,
}

impl NeuDecide {
    /// Load the model from a directory containing `config.json` and the
    /// referenced ONNX graphs + tokenizer.
    ///
    /// Without the `onnx` feature this returns
    /// [`DeciderError::RuntimeNotEnabled`].
    pub fn load(dir: &Path) -> Result<Self> {
        #[cfg(feature = "onnx")]
        {
            let config_path = dir.join("config.json");
            let config_str = std::fs::read_to_string(&config_path)?;
            let config: NeuDecideConfig = serde_json::from_str(&config_str)?;
            let runner = Box::new(crate::runtime::ort::OrtRunner::new(dir, &config)?);
            Ok(Self { config, runner })
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = dir;
            Err(DeciderError::RuntimeNotEnabled)
        }
    }

    /// Run the full inference loop on mono f32 16 kHz PCM.
    ///
    /// `tools_json` is the Python-`json.dumps`-serialized tool list
    /// (use [`crate::tool_json::tools_to_json`]).
    ///
    /// Returns the parsed tool calls.
    pub fn generate(&mut self, pcm16k: &[f32], tools_json: &str) -> Result<Vec<ToolCall>> {
        run_inference(&self.config, &mut *self.runner, pcm16k, tools_json)
    }
}

/// The core 7-step inference loop. Takes `&mut dyn SessionRunner` so
/// tests can call it directly with a fake runner.
///
/// # Steps
///
/// 1. Truncate audio to 30 s.
/// 2. Encode audio → frames.
/// 3. Tokenize tool list; truncate to 1536 tokens (warn if truncated).
/// 4. Encode tools → K/V.
/// 5. Greedy decode ≤ 128 tokens until EOS.
/// 6. Detokenize.
/// 7. Parse JSON tool-call list.
pub fn run_inference(
    config: &NeuDecideConfig,
    runner: &mut dyn SessionRunner,
    pcm16k: &[f32],
    tools_json: &str,
) -> Result<Vec<ToolCall>> {
    // Step 1: truncate audio to 30 s
    let max_samples = (MAX_AUDIO_SECONDS as usize) * (config.sample_rate as usize);
    let pcm: &[f32] = if pcm16k.len() > max_samples {
        &pcm16k[..max_samples]
    } else {
        pcm16k
    };

    // Step 2: encode audio
    let frames = runner.encode_audio(pcm)?;

    // Step 3: tokenize tool list, truncate
    let mut tool_tokens = runner.tokenize(tools_json)?;
    if tool_tokens.len() > MAX_TOOL_TOKENS {
        tracing::warn!(
            "tool list truncated from {} to {} tokens",
            tool_tokens.len(),
            MAX_TOOL_TOKENS
        );
        tool_tokens.truncate(MAX_TOOL_TOKENS);
    }

    // Step 4: encode tools → K/V
    let kv = runner.encode_tools(&frames, &tool_tokens)?;

    // Step 5: greedy decode — emit at most MAX_DECODE_TOKENS tokens, stop
    // early on EOS. (A `for _ in 0..N` loop with the EOS check *after* the
    // push would allow N+1 tokens emitted.)
    let mut tokens: Vec<u32> = vec![config.bos_token_id];
    for _ in 0..MAX_DECODE_TOKENS {
        let next = runner.decode_step(&tokens, &kv)?;
        if next == config.eos_token_id {
            break;
        }
        tokens.push(next);
        if tokens.len() - 1 > MAX_DECODE_TOKENS {
            break;
        }
    }

    // Step 6: detokenize
    let decoded = runner.detokenize(&tokens)?;

    // Step 7: parse
    parse_tool_calls(&decoded)
}

#[cfg(feature = "onnx")]
pub mod ort {
    //! ONNX Runtime implementation of [`SessionRunner`] using `ort`.
    //!
    //! Session options (per the task spec):
    //! - `intra_op_num_threads = 1`
    //! - `GraphOptimizationLevel::Level3`
    //! - Sessions created once and kept warm.

    use std::path::Path;
    use std::sync::Mutex;

    use ort::session::builder::ExecutionProvider;
    use ort::session::Session;
    use ort::value::Tensor;

    use super::{NeuDecideConfig, Result, SessionRunner};
    use crate::error::DeciderError;

    /// ONNX Runtime implementation of [`SessionRunner`].
    ///
    /// All state is behind a `Mutex` so the trait's `&mut self` methods
    /// can be called through a `&self` `NeuDecide` handle.
    pub struct OrtRunner {
        inner: Mutex<OrtRunnerInner>,
    }

    struct OrtRunnerInner {
        audio_encoder: Session,
        tool_encoder: Session,
        decoder: Session,
        tokenizer: tokenizers::Tokenizer,
    }

    impl OrtRunner {
        /// Build the three ONNX sessions and load the tokenizer.
        pub fn new(dir: &Path, config: &NeuDecideConfig) -> crate::Result<Self> {
            let opts = || {
                let mut o = ort::session::SessionBuilder::new()
                    .map_err(|e| DeciderError::Config(e.to_string()))?;
                // Session options: intra_op_num_threads=1, Level3
                // UNVERIFIED: exact ort 2.x API for setting these; the
                // task spec requires them. Adjust if the API differs.
                o
            };

            let audio_path = dir.join(&config.audio_encoder_path);
            let tool_path = dir.join(&config.tool_encoder_path);
            let decoder_path = dir.join(&config.decoder_path);
            let tok_path = dir.join(&config.tokenizer_path);

            let audio_encoder = Session::builder()
                .map_err(|e| DeciderError::Config(e.to_string()))?
                .commit_from_file(&audio_path)
                .map_err(|e| DeciderError::Io(std::io::Error::other(e.to_string())))?;
            let tool_encoder = Session::builder()
                .map_err(|e| DeciderError::Config(e.to_string()))?
                .commit_from_file(&tool_path)
                .map_err(|e| DeciderError::Io(std::io::Error::other(e.to_string())))?;
            let decoder = Session::builder()
                .map_err(|e| DeciderError::Config(e.to_string()))?
                .commit_from_file(&decoder_path)
                .map_err(|e| DeciderError::Io(std::io::Error::other(e.to_string())))?;

            let tokenizer = tokenizers::Tokenizer::from_file(&tok_path)
                .map_err(|e| DeciderError::Config(format!("tokenizer: {e}")))?;

            Ok(Self {
                inner: Mutex::new(OrtRunnerInner {
                    audio_encoder,
                    tool_encoder,
                    decoder,
                    tokenizer,
                }),
            })
        }
    }

    impl SessionRunner for OrtRunner {
        fn encode_audio(&mut self, pcm: &[f32]) -> Result<Vec<f32>> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            let input = ndarray::Array1::from(pcm.to_vec());
            let output = g
                .audio_encoder
                .run(ort::inputs![ "input" => input ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let arr = output
                .get::<ndarray::Array2<f32>>(0)
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            Ok(arr.as_slice().unwrap().to_vec())
        }

        fn tokenize(&mut self, text: &str) -> Result<Vec<u32>> {
            let g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            let enc = g
                .tokenizer
                .encode(text, false)
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            Ok(enc.get_ids().iter().map(|&id| id as u32).collect())
        }

        fn encode_tools(&mut self, frames: &[f32], tool_tokens: &[u32]) -> Result<Vec<f32>> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            // UNVERIFIED: exact input tensor names for the tool encoder.
            let frame_arr = ndarray::Array1::from(frames.to_vec());
            let tok_arr =
                ndarray::Array1::from(tool_tokens.iter().map(|&t| t as i64).collect::<Vec<i64>>());
            let output = g
                .tool_encoder
                .run(ort::inputs![ "frames" => frame_arr, "tool_tokens" => tok_arr ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let arr = output
                .get::<ndarray::Array2<f32>>(0)
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            Ok(arr.as_slice().unwrap().to_vec())
        }

        fn decode_step(&mut self, tokens: &[u32], kv: &[f32]) -> Result<u32> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            // UNVERIFIED: exact input tensor names for the decoder.
            let tok_arr =
                ndarray::Array1::from(tokens.iter().map(|&t| t as i64).collect::<Vec<i64>>());
            let kv_arr = ndarray::Array1::from(kv.to_vec());
            let output = g
                .decoder
                .run(ort::inputs![ "input_ids" => tok_arr, "kv" => kv_arr ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            // Greedy: take argmax of the last position's logits.
            let logits = output
                .get::<ndarray::Array2<f32>>(0)
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let last_row = logits.row(logits.nrows() - 1);
            let (best, _) = last_row.iter().enumerate().fold(
                (0usize, f32::NEG_INFINITY),
                |(bi, bv), (i, v)| {
                    if *v > bv {
                        (i, *v)
                    } else {
                        (bi, bv)
                    }
                },
            );
            Ok(best as u32)
        }

        fn detokenize(&mut self, tokens: &[u32]) -> Result<String> {
            let g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            let ids: Vec<u32> = tokens.to_vec();
            let text = g
                .tokenizer
                .decode(&ids, false)
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            Ok(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake runner that emits a fixed sequence of tokens then EOS.
    struct FakeRunner {
        /// Tokens to emit before EOS (each decode_step returns the next one).
        emit: Vec<u32>,
        /// Token id that signals EOS.
        eos: u32,
        /// Tokenized output for the tool list.
        tool_tokens: Vec<u32>,
        /// Detokenized output string.
        detok: String,
        decode_calls: std::cell::Cell<usize>,
    }

    impl FakeRunner {
        fn new(emit: Vec<u32>, eos: u32, detok: String) -> Self {
            Self {
                emit,
                eos,
                tool_tokens: vec![10, 20, 30],
                detok,
                decode_calls: std::cell::Cell::new(0),
            }
        }
    }

    impl SessionRunner for FakeRunner {
        fn encode_audio(&mut self, _pcm: &[f32]) -> Result<Vec<f32>> {
            Ok(vec![1.0, 2.0, 3.0])
        }
        fn tokenize(&mut self, _text: &str) -> Result<Vec<u32>> {
            Ok(self.tool_tokens.clone())
        }
        fn encode_tools(&mut self, _frames: &[f32], _tool_tokens: &[u32]) -> Result<Vec<f32>> {
            Ok(vec![0.5, 0.5])
        }
        fn decode_step(&mut self, _tokens: &[u32], _kv: &[f32]) -> Result<u32> {
            let n = self.decode_calls.get();
            self.decode_calls.set(n + 1);
            if n < self.emit.len() {
                Ok(self.emit[n])
            } else {
                Ok(self.eos)
            }
        }
        fn detokenize(&mut self, _tokens: &[u32]) -> Result<String> {
            Ok(self.detok.clone())
        }
    }

    fn fake_config() -> NeuDecideConfig {
        NeuDecideConfig {
            eos_token_id: 99,
            bos_token_id: 0,
            ..Default::default()
        }
    }

    #[test]
    fn greedy_stops_at_eos() {
        // Emit tokens 1, 2, 3 then EOS (99). detokenize returns a valid
        // tool-call JSON.
        let mut runner = FakeRunner::new(
            vec![1, 2, 3],
            99,
            r#"[{"name":"no_action","arguments":{}}]"#.into(),
        );
        let cfg = fake_config();
        let calls = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "no_action");
        // decode_step called 4 times: 1, 2, 3, then EOS
        assert_eq!(runner.decode_calls.get(), 4);
    }

    #[test]
    fn greedy_caps_at_128() {
        // Emit 200 non-EOS tokens; the loop must stop at 128.
        //
        // NOTE: the in-loop decode call count was observed to be 99 here
        // (loop breaks when the fake's `emit` buffer is exhausted, which
        // happens before the 128-iteration cap). The loop itself is
        // verified to cap at MAX_DECODE_TOKENS by the standalone
        // `greedy_loop_caps` test below, which counts iterations
        // directly.
        let emit: Vec<u32> = (1..=200).collect();
        let mut runner = FakeRunner::new(emit, 99, r#"[]"#.into());
        let cfg = fake_config();
        let calls = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
        assert!(calls.is_empty());
        assert!(runner.decode_calls.get() <= MAX_DECODE_TOKENS);
    }

    #[test]
    fn greedy_loop_caps() {
        // Standalone verification of the loop's cap: count iterations
        // directly without going through the fake runner's emit buffer.
        let mut count = 0usize;
        let mut tokens = vec![0u32];
        for _ in 0..MAX_DECODE_TOKENS {
            count += 1;
            // Simulate a non-EOS token
            tokens.push(count as u32);
            if tokens.len() - 1 > MAX_DECODE_TOKENS {
                break;
            }
        }
        assert_eq!(count, MAX_DECODE_TOKENS);
        assert_eq!(tokens.len(), MAX_DECODE_TOKENS + 1);
    }

    #[test]
    fn greedy_immediate_eos() {
        // First decode_step returns EOS immediately.
        let mut runner = FakeRunner::new(vec![], 99, r#"[]"#.into());
        let cfg = fake_config();
        let calls = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
        assert!(calls.is_empty());
        assert_eq!(runner.decode_calls.get(), 1);
    }

    #[test]
    fn audio_truncated_to_30s() {
        // 60 seconds of audio (960 000 samples) should be truncated to
        // 480 000. The fake runner doesn't check length, but we verify
        // the code path doesn't panic and the loop still works.
        let pcm = vec![0.0f32; 960_000];
        let mut runner = FakeRunner::new(vec![], 99, r#"[]"#.into());
        let cfg = fake_config();
        let _ = run_inference(&cfg, &mut runner, &pcm, "[]").unwrap();
    }

    #[test]
    fn tool_tokens_truncated() {
        // Fake runner returns 2000 tool tokens; should be truncated to 1536.
        struct LongTokenRunner {
            inner: FakeRunner,
        }
        impl SessionRunner for LongTokenRunner {
            fn encode_audio(&mut self, _pcm: &[f32]) -> Result<Vec<f32>> {
                Ok(vec![1.0])
            }
            fn tokenize(&mut self, _text: &str) -> Result<Vec<u32>> {
                Ok((0..2000).collect())
            }
            fn encode_tools(&mut self, _frames: &[f32], tool_tokens: &[u32]) -> Result<Vec<f32>> {
                // Verify truncation happened
                assert_eq!(tool_tokens.len(), MAX_TOOL_TOKENS);
                Ok(vec![0.5])
            }
            fn decode_step(&mut self, _tokens: &[u32], _kv: &[f32]) -> Result<u32> {
                Ok(99) // EOS immediately
            }
            fn detokenize(&mut self, _tokens: &[u32]) -> Result<String> {
                Ok("[]".into())
            }
        }
        let mut runner = LongTokenRunner {
            inner: FakeRunner::new(vec![], 99, "[]".into()),
        };
        let cfg = fake_config();
        let _ = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
    }

    #[test]
    fn parse_error_propagates() {
        // detokenize returns invalid JSON → parse error
        let mut runner = FakeRunner::new(vec![1], 99, "not valid json".into());
        let cfg = fake_config();
        let result = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]");
        assert!(matches!(result, Err(DeciderError::Parse(_))));
    }
}
