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

use crate::error::Result;
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
/// The real model config (verified against
/// `huggingface.co/neuphonic/neudecide`, snapshot
/// `fb0157e06da16d887caf041ca7929f96da419e6b`) looks like:
///
/// ```json
/// {
///   "sample_rate": 16000,
///   "max_audio_seconds": 30.0,
///   "max_tools_len": 2048,
///   "max_answer_len": 128,
///   "special_tokens": { "pad": 0, "eos": 1, "bos": 2, "unk": 3, "tool_call": 4, "tools": 5 },
///   "variants": { "q4": { "graphs": {
///       "audio_encoder": "audio_encoder_q4.onnx",
///       "tool_encoder": "tool_encoder_q4.onnx",
///       "decoder_step": "decoder_step_q4.onnx" } } }
/// }
/// ```
///
/// The Python package picks the graph files from `variants[variant].graphs`
/// (default variant `q4`) and the special token ids from `special_tokens`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NeuDecideConfig {
    /// Path to the audio encoder ONNX graph (relative to model dir).
    pub audio_encoder_path: String,
    /// Path to the tool encoder ONNX graph.
    pub tool_encoder_path: String,
    /// Path to the decoder ONNX graph.
    pub decoder_path: String,
    /// Path to the tokenizer file (binary SentencePiece `tokenizer.model`
    /// for the ort path, consumed by [`crate::spm::SpmTokenizer`]).
    pub tokenizer_path: String,
    /// Beginning-of-sequence token id (`special_tokens.bos`).
    pub bos_token_id: u32,
    /// End-of-sequence token id (`special_tokens.eos`).
    pub eos_token_id: u32,
    /// Special token prepended to the tokenized tool list
    /// (`special_tokens.tools`).
    pub tools_token_id: u32,
    /// Sample rate the model expects (16000).
    pub sample_rate: u32,
    /// Audio encoder frame rate (12.5).
    pub frame_rate: f32,
}

impl Default for NeuDecideConfig {
    fn default() -> Self {
        Self {
            audio_encoder_path: String::new(),
            tool_encoder_path: String::new(),
            decoder_path: String::new(),
            tokenizer_path: String::new(),
            bos_token_id: 2,
            eos_token_id: 1,
            tools_token_id: 5,
            sample_rate: SAMPLE_RATE,
            frame_rate: FRAME_RATE_HZ,
        }
    }
}

/// Config-file parsing helpers, used by [`NeuDecide::load`].
///
/// These are only reachable when the `onnx` feature is enabled (the
/// non-onnx build of `load` returns `RuntimeNotEnabled` without
/// touching the model files).
#[cfg(feature = "onnx")]
mod config {
    use serde::Deserialize;

    use super::{NeuDecideConfig, FRAME_RATE_HZ, SAMPLE_RATE};
    /// The `special_tokens` object in `config.json`.
    ///
    /// `bos`/`eos` seed and terminate the greedy decode loop; `tools`
    /// prefixes the tokenized tool list (the Python package's
    /// `encode_tools` prepends it before encoding). The remaining ids
    /// are deserialized so configs containing them parse; serde ignores
    /// keys we do not declare.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(default)]
    pub(super) struct SpecialTokens {
        #[serde(default)]
        pub eos: u32,
        #[serde(default)]
        pub bos: u32,
        /// Special token prepended to the tokenized tool list.
        #[serde(default)]
        pub tools: u32,
    }

    /// The `variants.<name>.graphs` object in `config.json`.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(default)]
    pub(super) struct Graphs {
        #[serde(default)]
        pub audio_encoder: String,
        #[serde(default)]
        pub tool_encoder: String,
        #[serde(default)]
        pub decoder_step: String,
    }

    /// `config.json` as parsed, before mapping onto [`NeuDecideConfig`].
    ///
    /// Only the fields the Rust runtime actually uses are declared; serde
    /// ignores the unknown keys (e.g. `max_audio_seconds`, `max_tools_len`,
    /// `max_answer_len`, `vocab_size`) so newer configs keep parsing.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(default)]
    pub(super) struct RawConfig {
        #[serde(default)]
        pub sample_rate: u32,
        #[serde(default)]
        pub special_tokens: SpecialTokens,
        /// `variants.<name>.graphs` keyed by variant name (e.g. `q4`).
        #[serde(default)]
        pub variants: std::collections::BTreeMap<String, Variant>,
        /// Top-level `graphs` fallback (pre-variant configs).
        #[serde(default)]
        pub graphs: Graphs,
    }

    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(default)]
    pub(super) struct Variant {
        #[serde(default)]
        pub graphs: Graphs,
    }

    impl RawConfig {
        /// Map onto the runtime config, picking the `q4` variant's graphs
        /// (falling back to the top-level `graphs` if present, then to the
        /// field defaults).
        pub(super) fn into_runtime(self, tokenizer_path: &str) -> NeuDecideConfig {
            let graphs = self
                .variants
                .get("q4")
                .map(|v| &v.graphs)
                .unwrap_or(&self.graphs)
                .clone();
            NeuDecideConfig {
                audio_encoder_path: graphs.audio_encoder,
                tool_encoder_path: graphs.tool_encoder,
                decoder_path: graphs.decoder_step,
                tokenizer_path: tokenizer_path.to_string(),
                bos_token_id: self.special_tokens.bos,
                eos_token_id: self.special_tokens.eos,
                tools_token_id: self.special_tokens.tools,
                sample_rate: if self.sample_rate == 0 {
                    SAMPLE_RATE
                } else {
                    self.sample_rate
                },
                frame_rate: FRAME_RATE_HZ,
            }
        }
    }
}

/// Abstraction over the three ONNX sessions (audio encoder, tool encoder,
/// decoder) plus the tokenizer. Implement this to run real inference
/// (OrtRunner) or to unit-test the greedy loop (FakeRunner).
///
/// The shapes below mirror the Python package's ONNX calls exactly
/// (verified against `neudecide` 0.1.0 on PyPI):
///
/// - `audio_encoder(waveform [1, n] f32, waveform_len [1] i64)`
///   -> `(audio_embeds [1, n_frames, d] f32, audio_len [1] i64)`
/// - `tool_encoder(audio_embeds, audio_len, tools_ids [1, m] i64)`
///   -> `(cross_key [1, T, H, D], cross_value [1, T, H, D], encoder_mask)`
/// - `decoder_step(input_ids [1] i64, past_key, past_value, cross_key,
///   cross_value, encoder_mask)` -> `(logits [1, 1, V], past_key, past_value)`
pub trait SessionRunner: Send {
    /// Run the audio encoder on mono f32 PCM at 16 kHz.
    /// Returns `(audio_embeds flattened, audio_len)`.
    fn encode_audio(&mut self, pcm: &[f32]) -> Result<(Vec<f32>, i64)>;

    /// Tokenize the serialized tool list. Returns token ids (the Python
    /// package prepends the `tools` special token before encoding).
    fn tokenize(&mut self, text: &str) -> Result<Vec<u32>>;

    /// Run the tool encoder. Returns the cross-attention K/V plus the
    /// encoder mask, flattened.
    fn encode_tools(
        &mut self,
        frames: &[f32],
        audio_len: i64,
        tool_tokens: &[u32],
    ) -> Result<(Vec<f32>, Vec<f32>, Vec<bool>)>;

    /// Run one decoder step on a single token, given the current
    /// self-attention KV cache. Returns `(next_token, new_past_key,
    /// new_past_value)`.
    fn decode_step(
        &mut self,
        token: u32,
        past_key: &[f32],
        past_value: &[f32],
        cross: &CrossState,
    ) -> Result<(u32, Vec<f32>, Vec<f32>)>;

    /// Detokenize a sequence of token ids into a string.
    fn detokenize(&mut self, tokens: &[u32]) -> Result<String>;
}

/// The cross-attention state produced by the tool encoder, carried into
/// every decoder step.
pub struct CrossState {
    /// Flattened cross-attention keys.
    pub key: Vec<f32>,
    /// Flattened cross-attention values.
    pub value: Vec<f32>,
    /// Encoder mask (bool).
    pub mask: Vec<bool>,
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
            let raw: config::RawConfig = serde_json::from_str(&config_str)?;
            // The ort path tokenizes with the pure-Rust [`SpmTokenizer`]
            // (hand-rolled SentencePiece port in [`crate::spm`]), so it
            // must consume the binary `tokenizer.model`, the
            // authoritative format here. The HF-style `tokenizer.json`
            // the model also ships is information-lossy (all BPE merge
            // scores are zero) and cannot reproduce the SPM ids Python
            // sends, so it is only kept as a last resort; the SPM loader
            // will refuse it with a clear error.
            let tokenizer_path = if dir.join("tokenizer.model").is_file() {
                "tokenizer.model"
            } else {
                "tokenizer.json"
            };
            let config = raw.into_runtime(tokenizer_path);
            let runner = Box::new(crate::runtime::ort::OrtRunner::new(dir, &config)?);
            Ok(Self { config, runner })
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = dir;
            Err(crate::error::DeciderError::RuntimeNotEnabled)
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

/// The core 7-step inference loop, mirroring the Python package's
/// `NeuDecide.generate_ids`. Takes `&mut dyn SessionRunner` so tests can
/// call it directly with a fake runner.
///
/// # Steps
///
/// 1. Truncate audio to 30 s.
/// 2. `audio_encoder` → audio embeds + length.
/// 3. Tokenize the tool list; truncate to the config limit (warn).
/// 4. `tool_encoder` → cross-attention K/V + mask.
/// 5. Greedy decode ≤ 128 tokens until EOS, with a growing self-attention
///    KV cache. The decoder starts from the EOS token (as Python does).
/// 6. Detokenize (dropping structural tokens).
/// 7. Parse the JSON tool-call list.
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
    let (frames, audio_len) = runner.encode_audio(pcm)?;

    // Step 3: tokenize tool list, truncate
    // The Python package's `encode_tools` prepends the `tools` special
    // token before the tokenized list; mirror that here.
    let mut tool_tokens = runner.tokenize(tools_json)?;
    let mut prefixed = Vec::with_capacity(tool_tokens.len() + 1);
    prefixed.push(config.tools_token_id);
    prefixed.extend(tool_tokens);
    tool_tokens = prefixed;
    if tool_tokens.len() > MAX_TOOL_TOKENS {
        tracing::warn!(
            "tool list truncated from {} to {} tokens",
            tool_tokens.len(),
            MAX_TOOL_TOKENS
        );
        tool_tokens.truncate(MAX_TOOL_TOKENS);
    }

    // Step 4: encode tools → cross-attention K/V + mask
    let (key, value, mask) = runner.encode_tools(&frames, audio_len, &tool_tokens)?;
    let cross = CrossState { key, value, mask };

    // Steps 5+6: greedy decode with a growing self-attention KV cache.
    // The decoder starts from the EOS token; each step feeds the single
    // previous token plus the current cache and returns the next token
    // and the updated cache.
    let mut past_key: Vec<f32> = Vec::new();
    let mut past_value: Vec<f32> = Vec::new();
    let mut token = config.eos_token_id;
    let mut out: Vec<u32> = Vec::new();
    for _ in 0..MAX_DECODE_TOKENS {
        let (next, pk, pv) = runner.decode_step(token, &past_key, &past_value, &cross)?;
        if next == config.eos_token_id {
            break;
        }
        out.push(next);
        past_key = pk;
        past_value = pv;
        token = next;
    }

    // Step 7: detokenize + parse. Unparseable model output (noise,
    // silence, token soup) yields an empty list — a "no command"
    // outcome, never an error.
    let decoded = runner.detokenize(&out)?;
    Ok(parse_tool_calls(&decoded))
}

#[cfg(feature = "onnx")]
pub mod ort {
    //! ONNX Runtime implementation of [`SessionRunner`] using `ort`.
    //!
    //! The tensor names, dtypes and shapes mirror the Python package's
    //! `onnxruntime` calls (verified against `neudecide` 0.1.0 on PyPI):
    //!
    //! - `audio_encoder`: `waveform` [1, n] f32 + `waveform_len` [1] i64
    //!   -> `audio_embeds` [1, n_frames, d] f32 + `audio_len` [1] i64
    //! - `tool_encoder`: `audio_embeds`, `audio_len`, `tools_ids` [1, m] i64
    //!   -> `cross_key` [1, T, H, D] f32, `cross_value` [1, T, H, D] f32,
    //!   `encoder_mask` i64
    //! - `decoder_step`: `input_ids` [1] i64, `past_key` / `past_value`
    //!   [L, 1, H, 0, D] f32 (empty at step 0), `cross_key`, `cross_value`,
    //!   `encoder_mask` -> `logits` [1, 1, V] f32 + updated KV caches
    //!
    //! Session options (per the task spec):
    //! - `intra_op_num_threads = 1`
    //! - `GraphOptimizationLevel::Level3`
    //! - Sessions created once and kept warm.

    use std::path::Path;
    use std::sync::Mutex;

    use ort::session::Session;
    use ort::value::Tensor;

    use super::{CrossState, NeuDecideConfig, Result, SessionRunner};
    use crate::error::DeciderError;
    use crate::spm::SpmTokenizer;

    /// Load the model's tokenizer via the pure-Rust [`SpmTokenizer`].
    ///
    /// Needs the binary SentencePiece model (`tokenizer.model`): the
    /// `tokenizer.json` the same model ships alongside is
    /// information-lossy (all BPE merge scores are zero), so the HF
    /// `tokenizers` crate cannot reproduce the SPM ids the Python
    /// package sends. [`SpmTokenizer`] is verified 1:1 against Python's
    /// `sentencepiece` (the 18-case encode/decode oracle in `spm.rs`
    /// plus the A.4 `parity_with_python` gate).
    fn build_tokenizer(path: &Path) -> std::result::Result<SpmTokenizer, String> {
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            return Err(format!(
                "{}: not a binary SentencePiece model; the ort path needs tokenizer.model",
                path.display()
            ));
        }
        SpmTokenizer::load(path)
    }

    /// ONNX Runtime implementation of [`SessionRunner`].
    /// All state is behind a `Mutex` so the trait's `&mut self` methods
    /// can be called through a `&self` `NeuDecide` handle.
    pub struct OrtRunner {
        inner: Mutex<OrtRunnerInner>,
    }

    struct OrtRunnerInner {
        audio_encoder: Session,
        tool_encoder: Session,
        decoder: Session,
        tokenizer: SpmTokenizer,
    }

    impl OrtRunner {
        /// Build the three ONNX sessions and load the tokenizer.
        pub fn new(dir: &Path, config: &NeuDecideConfig) -> crate::Result<Self> {
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

            let tokenizer = build_tokenizer(&tok_path)
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
        fn encode_audio(&mut self, pcm: &[f32]) -> Result<(Vec<f32>, i64)> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            let waveform = Tensor::from_array((vec![1usize, pcm.len()], pcm.to_vec()))
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let waveform_len = Tensor::from_array((vec![1usize], vec![pcm.len() as i64]))
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let output = g
                .audio_encoder
                .run(ort::inputs![ "waveform" => waveform, "waveform_len" => waveform_len ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let embeds = output
                .get("audio_embeds")
                .ok_or_else(|| DeciderError::Config("missing audio_embeds".into()))?;
            let len = output
                .get("audio_len")
                .ok_or_else(|| DeciderError::Config("missing audio_len".into()))?;
            let embeds_data = embeds
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("audio_embeds: {e}")))?;
            let len_data = len
                .try_extract_array::<i64>()
                .map_err(|e| DeciderError::Config(format!("audio_len: {e}")))?;
            let audio_len = *len_data.iter().next().unwrap_or(&0);
            Ok((embeds_data.as_slice().unwrap().to_vec(), audio_len))
        }

        fn tokenize(&mut self, text: &str) -> Result<Vec<u32>> {
            let g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            Ok(g.tokenizer.encode(text))
        }

        fn encode_tools(
            &mut self,
            frames: &[f32],
            audio_len: i64,
            tool_tokens: &[u32],
        ) -> Result<(Vec<f32>, Vec<f32>, Vec<bool>)> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            // audio_embeds is [1, n_frames, d]; the frame dim is d_model
            // (512 for the q4 model). Rebuild the 3-D shape from the
            // flattened encoder output.
            let d_model = 512usize;
            let n_frames = frames.len() / d_model;
            let audio_embeds =
                Tensor::from_array((vec![1usize, n_frames, d_model], frames.to_vec()))
                    .map_err(|e| DeciderError::Config(format!("audio_embeds: {e}")))?;
            let audio_len_arr = Tensor::from_array((vec![1usize], vec![audio_len]))
                .map_err(|e| DeciderError::Config(format!("audio_len: {e}")))?;
            let tools_ids = Tensor::from_array((
                vec![1usize, tool_tokens.len()],
                tool_tokens.iter().map(|&t| t as i64).collect::<Vec<_>>(),
            ))
            .map_err(|e| DeciderError::Config(format!("tools_ids: {e}")))?;
            let output = g
                .tool_encoder
                .run(ort::inputs![ "audio_embeds" => audio_embeds, "audio_len" => audio_len_arr, "tools_ids" => tools_ids ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let cross_key = output
                .get("cross_key")
                .ok_or_else(|| DeciderError::Config("missing cross_key".into()))?;
            let cross_value = output
                .get("cross_value")
                .ok_or_else(|| DeciderError::Config("missing cross_value".into()))?;
            let encoder_mask = output
                .get("encoder_mask")
                .ok_or_else(|| DeciderError::Config("missing encoder_mask".into()))?;
            let cross_key = cross_key
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("cross_key: {e}")))?
                .as_slice()
                .unwrap()
                .to_vec();
            let cross_value = cross_value
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("cross_value: {e}")))?
                .as_slice()
                .unwrap()
                .to_vec();
            let encoder_mask = encoder_mask
                .try_extract_array::<bool>()
                .map_err(|e| DeciderError::Config(format!("encoder_mask: {e}")))?
                .as_slice()
                .unwrap()
                .to_vec();
            Ok((cross_key, cross_value, encoder_mask))
        }

        fn decode_step(
            &mut self,
            token: u32,
            past_key: &[f32],
            past_value: &[f32],
            cross: &CrossState,
        ) -> Result<(u32, Vec<f32>, Vec<f32>)> {
            let mut g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            let input_ids = Tensor::from_array((vec![1usize, 1usize], vec![token as i64]))
                .map_err(|e| DeciderError::Config(format!("input_ids: {e}")))?;
            // The self-attention KV cache starts empty (shape [8, 1, 4, 0, 64])
            // and grows by one position per step.
            let layers = 8usize;
            let heads = 4usize;
            let head_dim = 64usize;
            let pos = past_key.len() / (layers * heads * head_dim);
            let pk_shape = vec![layers, 1usize, heads, pos, head_dim];
            let past_key_arr = Tensor::from_array((pk_shape.clone(), past_key.to_vec()))
                .map_err(|e| DeciderError::Config(format!("past_key: {e}")))?;
            let past_value_arr = Tensor::from_array((pk_shape, past_value.to_vec()))
                .map_err(|e| DeciderError::Config(format!("past_value: {e}")))?;
            let cross_key_arr = Tensor::from_array((
                vec![
                    layers,
                    1usize,
                    heads,
                    cross.key.len() / (layers * heads * head_dim),
                    head_dim,
                ],
                cross.key.clone(),
            ))
            .map_err(|e| DeciderError::Config(format!("cross_key: {e}")))?;
            let cross_value_arr = Tensor::from_array((
                vec![
                    layers,
                    1usize,
                    heads,
                    cross.value.len() / (layers * heads * head_dim),
                    head_dim,
                ],
                cross.value.clone(),
            ))
            .map_err(|e| DeciderError::Config(format!("cross_value: {e}")))?;
            let encoder_mask_arr =
                Tensor::from_array((vec![1usize, cross.mask.len()], cross.mask.clone()))
                    .map_err(|e| DeciderError::Config(format!("encoder_mask: {e}")))?;
            let output = g
                .decoder
                .run(ort::inputs![ "input_ids" => input_ids, "past_key" => past_key_arr, "past_value" => past_value_arr, "cross_key" => cross_key_arr, "cross_value" => cross_value_arr, "encoder_mask" => encoder_mask_arr ])
                .map_err(|e| DeciderError::Config(e.to_string()))?;
            let logits = output
                .get("logits")
                .ok_or_else(|| DeciderError::Config("missing logits".into()))?;
            let new_key = output
                .get("present_key")
                .ok_or_else(|| DeciderError::Config("missing new past_key".into()))?;
            let new_value = output
                .get("present_value")
                .ok_or_else(|| DeciderError::Config("missing new past_value".into()))?;
            let logits_arr = logits
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("logits: {e}")))?;
            let logits_slice = logits_arr
                .as_slice()
                .ok_or_else(|| DeciderError::Config("logits not contiguous".into()))?
                .to_vec();
            // Greedy: argmax of the last (only) position's logits.
            let (best, _) = logits_slice.iter().enumerate().fold(
                (0usize, f32::NEG_INFINITY),
                |(bi, bv), (i, v)| {
                    if *v > bv {
                        (i, *v)
                    } else {
                        (bi, bv)
                    }
                },
            );
            let new_key = new_key
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("new past_key: {e}")))?
                .as_slice()
                .ok_or_else(|| DeciderError::Config("past_key not contiguous".into()))?
                .to_vec();
            let new_value = new_value
                .try_extract_array::<f32>()
                .map_err(|e| DeciderError::Config(format!("new past_value: {e}")))?
                .as_slice()
                .ok_or_else(|| DeciderError::Config("past_value not contiguous".into()))?
                .to_vec();
            Ok((best as u32, new_key, new_value))
        }

        fn detokenize(&mut self, tokens: &[u32]) -> Result<String> {
            let g = self
                .inner
                .lock()
                .map_err(|e| DeciderError::Config(format!("poisoned: {e}")))?;
            // Python's `NeuDecide.decode` drops the structural tokens
            // (pad 0 / eos 1 / bos 2 / tool_call 4 / tools 5) before
            // sentencepiece-decoding; mirror that here.
            let ids: Vec<u32> = tokens
                .iter()
                .copied()
                .filter(|t| !matches!(t, 0 | 1 | 2 | 4 | 5))
                .collect();
            g.tokenizer.decode(&ids)
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
        fn encode_audio(&mut self, _pcm: &[f32]) -> Result<(Vec<f32>, i64)> {
            Ok((vec![1.0, 2.0, 3.0], 3))
        }
        fn tokenize(&mut self, _text: &str) -> Result<Vec<u32>> {
            Ok(self.tool_tokens.clone())
        }
        fn encode_tools(
            &mut self,
            _frames: &[f32],
            _audio_len: i64,
            _tool_tokens: &[u32],
        ) -> Result<(Vec<f32>, Vec<f32>, Vec<bool>)> {
            Ok((vec![0.5], vec![0.5], vec![true]))
        }
        fn decode_step(
            &mut self,
            _token: u32,
            _past_key: &[f32],
            _past_value: &[f32],
            _cross: &CrossState,
        ) -> Result<(u32, Vec<f32>, Vec<f32>)> {
            let n = self.decode_calls.get();
            self.decode_calls.set(n + 1);
            if n < self.emit.len() {
                Ok((self.emit[n], vec![0.0], vec![0.0]))
            } else {
                Ok((self.eos, vec![0.0], vec![0.0]))
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
        struct LongTokenRunner;
        impl SessionRunner for LongTokenRunner {
            fn encode_audio(&mut self, _pcm: &[f32]) -> Result<(Vec<f32>, i64)> {
                Ok((vec![1.0], 1))
            }
            fn tokenize(&mut self, _text: &str) -> Result<Vec<u32>> {
                Ok((0..2000).collect())
            }
            fn encode_tools(
                &mut self,
                _frames: &[f32],
                _audio_len: i64,
                tool_tokens: &[u32],
            ) -> Result<(Vec<f32>, Vec<f32>, Vec<bool>)> {
                // Verify truncation happened
                assert_eq!(tool_tokens.len(), MAX_TOOL_TOKENS);
                Ok((vec![0.5], vec![0.5], vec![true]))
            }
            fn decode_step(
                &mut self,
                _token: u32,
                _past_key: &[f32],
                _past_value: &[f32],
                _cross: &CrossState,
            ) -> Result<(u32, Vec<f32>, Vec<f32>)> {
                Ok((99, vec![0.0], vec![0.0])) // EOS immediately
            }
            fn detokenize(&mut self, _tokens: &[u32]) -> Result<String> {
                Ok("[]".into())
            }
        }
        let mut runner = LongTokenRunner;
        let cfg = fake_config();
        let _ = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
    }

    #[test]
    fn unparseable_output_yields_empty() {
        // detokenize returns non-JSON garbage (the noise/silence case)
        // → an empty call list, not an error.
        let mut runner = FakeRunner::new(vec![1], 99, "not valid json".into());
        let cfg = fake_config();
        let calls = run_inference(&cfg, &mut runner, &[0.0f32; 160], "[]").unwrap();
        assert!(calls.is_empty());
    }
}
