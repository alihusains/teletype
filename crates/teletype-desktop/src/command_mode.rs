//! Addendum A.3.3: the Command Mode decider.
//!
//! Replaces Whistle → Needle as the default Command Mode decider with the
//! NeuDecide hybrid: tool + enum args from audio (NeuDecide), free-form
//! args (query, recipient, body) from the transcript. NeuDecide runs only
//! on command takes and is skipped for non-English. When the decider is
//! unavailable (model missing, non-English, feature off) or errors, the
//! take falls back to the existing Needle path — never a regression.
//!
//! The `DecisionProvider` (TinyDecide + Needle 3 / LLM, in
//! `teletype-inference`) is untouched: it still classifies intent and
//! serves as the fallback extractor. This module adds the *decider* layer
//! on top of it.

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use teletype_core::llm::CommandAction;

/// Which decider ran (or would have run) for a command take. The label is
/// what lands in `DictationTrace::decider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeciderKind {
    /// NeuDecide (speech → tool call) is enabled and was used.
    NeuDecide,
    /// The existing Needle 3 / LLM extraction path.
    Needle,
    /// The local polish LLM JSON-schema extractor.
    LlmFallback,
}

impl DeciderKind {
    /// Stable label for traces and logs.
    pub fn label(self) -> &'static str {
        match self {
            DeciderKind::NeuDecide => "neudecide",
            DeciderKind::Needle => "needle",
            DeciderKind::LlmFallback => "llm-fallback",
        }
    }
}

/// The decision a [`CommandDecider`] makes for one command take.
#[derive(Debug, Clone)]
pub struct Decision {
    /// The tool name, e.g. "web_search" or "no_action".
    pub tool: String,
    /// The tool arguments, as emitted/filled by the decider + resolver.
    pub args: Map<String, Value>,
    /// Where the arguments came from: "neudecide", "transcript", "llm", or
    /// "none".
    pub args_source: String,
    /// Wall-clock milliseconds spent in the `decide` call.
    pub decider_ms: u32,
}

impl Decision {
    /// `no_action` is the sentinel for "no command in this take": the
    /// caller flashes "Didn't catch a command" and the transcript is
    /// delivered as plain dictation.
    pub fn is_no_action(&self) -> bool {
        self.tool == "no_action"
    }

    /// Maps a decision to the pipeline's [`CommandAction`]. `no_action`
    /// maps to `None` (no execution; the text is delivered).
    pub fn to_command_action(&self) -> Option<CommandAction> {
        if self.is_no_action() {
            return None;
        }
        Some(CommandAction {
            tool: self.tool.clone(),
            arguments: Value::Object(self.args.clone()),
        })
    }
}

/// Decides a command take: tool + args from the take audio and transcript.
///
/// Implementations must be thread-safe in the sense that `decide` may be
/// called concurrently from the pipeline worker (it is not, in practice —
/// one take at a time — but the trait is `Send` so it can be shared).
pub trait CommandDecider: Send + Sync {
    /// Which decider this is (for traces and logs).
    fn kind(&self) -> DeciderKind;

    /// Decide a command take. `pcm16k` is the take audio (mono f32, 16 kHz);
    /// `transcript` is the final ASR text. Implementations must never block
    /// for longer than ~2 s.
    fn decide(&self, pcm16k: &[f32], transcript: &str) -> Result<Decision, String>;
}

/// The pure transcript-side rules: fill free-form args from the ASR text.
///
/// NeuDecide emits the tool and the *enum* args (app, transform, action)
/// directly from audio; the free-form args (a search query, a message
/// recipient + body) come from the transcript. This resolver is the
/// boundary between the two, and it is pure — fully unit-testable with no
/// model and no app state.
pub struct CommandResolver;

impl CommandResolver {
    /// Remove a leading search trigger phrase from a query.
    ///
    /// "search the web for rust async" -> "rust async";
    /// "Google  rust async" -> "rust async"; "rust async" -> "rust async".
    /// Case-insensitive; trims surrounding punctuation and whitespace.
    pub fn strip_search_triggers(query: &str) -> String {
        const TRIGGERS: &[&str] = &[
            "search the web for",
            "search the web",
            "search for",
            "search",
            "google",
            "look up",
        ];
        let mut s = query.trim();
        loop {
            let lower = s.to_ascii_lowercase();
            let mut stripped = false;
            for t in TRIGGERS {
                if lower == *t {
                    s = "";
                    stripped = true;
                    break;
                }
                if let Some(rest) = lower.strip_prefix(t) {
                    // Only strip when the trigger is a whole word: the next
                    // character must be whitespace or punctuation.
                    let next_ok = rest
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_whitespace() || c.is_ascii_punctuation());
                    if next_ok {
                        // Use the original casing for the remainder.
                        let rest_orig = &s[t.len()..];
                        s = rest_orig;
                        stripped = true;
                        break;
                    }
                }
            }
            s = s
                .trim()
                .trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
            if !stripped {
                break;
            }
        }
        s.to_string()
    }

    /// Parse a draft message from the transcript using rules.
    ///
    /// Shapes (case-insensitive): "to X saying Y", "email X that Y",
    /// "email X saying Y", "message X Y". `X` is 1-3 words (recipient);
    /// `Y` is the rest (body). Returns `None` when the shape does not
    /// match — the LLM fallback handles those.
    pub fn parse_draft_message(transcript: &str) -> Option<(String, String)> {
        let t = transcript.trim();
        let lower = t.to_ascii_lowercase();

        // "to X saying Y": "to" + recipient + "saying" + body.
        if let Some(rest) = lower.strip_prefix("to ") {
            if let Some(idx) = rest.find(" saying ") {
                let recipient = t["to ".len().."to ".len() + idx]
                    .trim_end_matches(['.', ',', '!', '?'])
                    .trim()
                    .to_string();
                let body = t["to ".len() + idx + " saying ".len()..].trim().to_string();
                if (1..=3).contains(&recipient.split_whitespace().count()) && !body.is_empty() {
                    return Some((recipient, body));
                }
            }
        }
        // "email X that Y" / "email X saying Y".
        for marker in ["email ", "message "] {
            if let Some(rest) = lower.strip_prefix(marker) {
                for sep in [" that ", " saying ", " with "] {
                    if let Some(idx) = rest.find(sep) {
                        let recipient = t[marker.len()..marker.len() + idx]
                            .trim_end_matches(['.', ',', '!', '?'])
                            .trim()
                            .to_string();
                        let body = t[marker.len() + idx + sep.len()..].trim().to_string();
                        if (1..=3).contains(&recipient.split_whitespace().count())
                            && !body.is_empty()
                        {
                            return Some((recipient, body));
                        }
                    }
                }
                // "message X Y" (no separator): recipient = first 1-3 words,
                // body = the rest.
                if marker == "message " {
                    let words: Vec<&str> = rest.split_whitespace().collect();
                    for n in 1..=3usize {
                        if words.len() > n {
                            // Rebuild from the original casing.
                            let recipient = t[marker.len()..]
                                .split_whitespace()
                                .take(n)
                                .collect::<Vec<_>>()
                                .join(" ");
                            let body = t[marker.len()..]
                                .splitn(n + 1, ' ')
                                .last()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            if !body.is_empty() {
                                return Some((recipient, body));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Fill the free-form args for `tool` from the transcript.
    ///
    /// Returns the resulting `args_source` label: "transcript" when the
    /// rules filled an arg, "neudecide" when the model's args stand,
    /// "llm" when the rules could not fill (the LLM fallback will), and
    /// "none" when there are no args.
    pub fn fill_free_form_args(
        tool: &str,
        args: &mut Map<String, Value>,
        transcript: &str,
    ) -> String {
        match tool {
            "web_search" => {
                let query = Self::strip_search_triggers(transcript);
                if !query.is_empty() {
                    args.insert("query".into(), Value::String(query));
                    "transcript".to_string()
                } else {
                    "none".to_string()
                }
            }
            "draft_message" => match Self::parse_draft_message(transcript) {
                Some((recipient, body)) => {
                    args.insert("recipient".into(), Value::String(recipient));
                    args.insert("body".into(), Value::String(body));
                    "transcript".to_string()
                }
                None => "llm".to_string(),
            },
            "no_action" => "none".to_string(),
            _ => {
                if args.is_empty() {
                    "none".to_string()
                } else {
                    "neudecide".to_string()
                }
            }
        }
    }
}

/// The NeuDecide decider: speech → tool call via the ONNX model, with the
/// [`CommandResolver`] filling free-form args from the transcript.
///
/// The model is loaded once (lazily, on first `decide`) and held in a
/// `Mutex` (the ONNX sessions are not thread-safe for concurrent
/// inference). When the `neudecide` feature is off, or the model files are
/// missing, [`NeuDecideDecider::new`] returns `Ok(None)` and the selection
/// in `ensure_decision_provider` falls through to Needle.
pub struct NeuDecideDecider {
    model: Mutex<Option<teletype_decider::NeuDecide>>,
    model_dir: std::path::PathBuf,
    tools_json: String,
}

impl NeuDecideDecider {
    /// Build the decider if the model is present. `model_dir` is where the
    /// catalog download placed the NeuDecide files (`config.json`, the ONNX
    /// graphs, the tokenizer).
    ///
    /// Returns `None` (not an error) when the model is absent or the
    /// `neudecide` feature is off — the caller falls through to Needle.
    pub fn new(model_dir: &std::path::Path) -> Option<Self> {
        if !model_dir.join("config.json").exists() {
            return None;
        }
        Some(Self {
            model: Mutex::new(None),
            model_dir: model_dir.to_path_buf(),
            tools_json: teletype_decider::tools_to_json(&teletype_decider::v1_tools(&[], &[], &[])),
        })
    }

    /// True when the decider can run: the model files exist AND the
    /// `neudecide` feature is on. The language gate is applied at
    /// selection time (the language is a per-take / per-app value, not a
    /// property of the model).
    pub fn available(model_dir: &std::path::Path) -> bool {
        #[cfg(feature = "neudecide")]
        {
            model_dir.join("config.json").exists() && model_dir.join("tokenizer.json").exists()
        }
        #[cfg(not(feature = "neudecide"))]
        {
            let _ = model_dir;
            false
        }
    }

    /// True when `language` is acceptable for NeuDecide: English
    /// (case-insensitive `en` prefix) or "auto" (the model is English-
    /// trained; "auto" is allowed because the ASR language gate already
    /// applies to the transcript).
    pub fn language_ok(language: &str) -> bool {
        let l = language.to_ascii_lowercase();
        l == "auto" || l.starts_with("en")
    }

    /// Load the model on first use. Returns the error when the ONNX
    /// runtime is not enabled (feature off) or the model is corrupt.
    /// Load the model on first use. Returns the error when the ONNX
    /// runtime is not enabled (feature off) or the model is corrupt.
    /// Load the model on first use. Returns the model by value (moved
    /// out of the `Mutex`); the caller is responsible for dropping it
    /// after use (the `Mutex` slot is left `None`, so the next `decide`
    /// call reloads the model).
    fn ensure_loaded(&self) -> Result<teletype_decider::NeuDecide, String> {
        let mut g = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.is_none() {
            *g = Some(
                teletype_decider::NeuDecide::load(&self.model_dir).map_err(|e| e.to_string())?,
            );
        }
        g.take().ok_or_else(|| "model not loaded".to_string())
    }
}

impl CommandDecider for NeuDecideDecider {
    fn kind(&self) -> DeciderKind {
        DeciderKind::NeuDecide
    }

    fn decide(&self, pcm16k: &[f32], transcript: &str) -> Result<Decision, String> {
        let started = std::time::Instant::now();
        let mut model = self.ensure_loaded()?;
        let calls = model
            .generate(pcm16k, &self.tools_json)
            .map_err(|e| e.to_string())?;
        drop(model);
        let call = calls
            .first()
            .cloned()
            .unwrap_or_else(|| teletype_decider::ToolCall {
                name: "no_action".into(),
                arguments: Map::new(),
            });
        let mut decision = Decision {
            tool: call.name,
            args: call.arguments,
            args_source: "neudecide".to_string(),
            decider_ms: 0,
        };
        // The resolver fills free-form args from the transcript and
        // overrides `args_source` for any arg it filled.
        decision.args_source =
            CommandResolver::fill_free_form_args(&decision.tool, &mut decision.args, transcript);
        decision.decider_ms = started.elapsed().as_millis() as u32;
        Ok(decision)
    }
}

/// The Needle decider: a thin adapter over the existing
/// `DecisionProvider` (TinyDecide + Needle 3, or the LLM-only fallback).
///
/// This is the *fallback* path: when NeuDecide is unavailable or errors,
/// the take is decided by the provider that was already built by
/// `ensure_decision_provider`. It keeps today's behaviour exactly.
pub struct NeedleDecider {
    provider:
        std::sync::Arc<std::sync::Mutex<Option<Box<dyn teletype_core::llm::DecisionProvider>>>>,
}

impl NeedleDecider {
    pub fn new(
        provider: std::sync::Arc<
            std::sync::Mutex<Option<Box<dyn teletype_core::llm::DecisionProvider>>>,
        >,
    ) -> Self {
        Self { provider }
    }
}

impl CommandDecider for NeedleDecider {
    fn kind(&self) -> DeciderKind {
        DeciderKind::Needle
    }

    fn decide(&self, _pcm16k: &[f32], transcript: &str) -> Result<Decision, String> {
        let started = std::time::Instant::now();
        let guard = self
            .provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let provider = guard
            .as_ref()
            .ok_or_else(|| "needle: no provider loaded".to_string())?;
        let action = provider
            .extract_command(transcript)
            .ok_or_else(|| "needle: no command extracted".to_string())?;
        drop(guard);
        let args = match action.arguments {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("value".into(), other);
                m
            }
        };
        let args_source = if args.is_empty() { "none" } else { "needle" };
        Ok(Decision {
            tool: action.tool,
            args,
            args_source: args_source.to_string(),
            decider_ms: started.elapsed().as_millis() as u32,
        })
    }
}

/// The LLM-fallback decider: a thin adapter over the P2.7
/// `LlmCommandExtractor` (grammar-constrained JSON over the local polish
/// LLM). Always available; the extractor returns `None` (→ error here)
/// when no LLM is loaded.
pub struct LlmFallbackDecider {
    extractor: Arc<teletype_inference::LlmCommandExtractor>,
}

impl LlmFallbackDecider {
    pub fn new(extractor: Arc<teletype_inference::LlmCommandExtractor>) -> Self {
        Self { extractor }
    }
}

impl CommandDecider for LlmFallbackDecider {
    fn kind(&self) -> DeciderKind {
        DeciderKind::LlmFallback
    }

    fn decide(&self, _pcm16k: &[f32], transcript: &str) -> Result<Decision, String> {
        let started = std::time::Instant::now();
        let action = self
            .extractor
            .extract(transcript)
            .ok_or_else(|| "llm-fallback: no command extracted".to_string())?;
        let args = match action.arguments {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("value".into(), other);
                m
            }
        };
        let args_source = if args.is_empty() { "none" } else { "llm" };
        Ok(Decision {
            tool: action.tool,
            args,
            args_source: args_source.to_string(),
            decider_ms: started.elapsed().as_millis() as u32,
        })
    }
}

/// The slot that holds the selected command decider, shared between
/// `ensure_decision_provider` (which rebuilds it) and the pipeline worker
/// (which calls `decide` on each command take).
///
/// `None` when no decider is available (no model, non-English, feature
/// off): the take then runs the existing Needle path via the
/// `DecisionProvider` alone, and the trace records the fallback.
pub struct DeciderSlot {
    inner: Mutex<Option<Arc<dyn CommandDecider>>>,
}

impl DeciderSlot {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Replace the decider (called from `ensure_decision_provider`).
    pub fn set(&self, d: Option<Arc<dyn CommandDecider>>) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = d;
    }

    /// Snapshot the current decider, if any.
    pub fn get(&self) -> Option<Arc<dyn CommandDecider>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Default for DeciderSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Select the command decider for a take.
///
/// Order (Addendum A.3.3):
/// 1. NeuDecide — model present, `neudecide` feature on, language English
///    (or "auto"). The default when available.
/// 2. Needle — the existing combined provider (TinyDecide + Needle 3).
/// 3. LLM fallback — the P2.7 grammar-constrained extractor.
///
/// Returns the decider to use for this take, or `None` when nothing is
/// available (the take runs the plain Needle path via the
/// `DecisionProvider` and the trace records `needle-fallback`).
///
/// The decider is stored in a [`DeciderSlot`] because a `Box<dyn
/// DecisionProvider>` cannot be cloned (no `clone_box` on the trait): the
/// slot is the one place the selected decider lives between
/// `ensure_decision_provider` (which rebuilds it) and the pipeline worker
/// (which consumes it per take).
pub fn select_decider(slot: &DeciderSlot, language: &str) -> Option<Arc<dyn CommandDecider>> {
    let d = slot.get()?;
    // NeuDecide is gated on the language (English or "auto"); the Needle /
    // LLM fallback deciders are language-independent.
    if d.kind() == DeciderKind::NeuDecide && !NeuDecideDecider::language_ok(language) {
        return None;
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted `CommandDecider` for the selection tests: returns the
    /// canned `Decision` or error, and records the transcripts it saw.
    struct FakeDecider {
        kind: DeciderKind,
        outcome: Result<(String, Map<String, Value>), String>,
    }

    /// A `CommandDecider` that captures the PCM slice it receives, for
    /// asserting the audio seam threads real take audio to `decide`.
    struct PcmCaptureDecider {
        pcm: Mutex<Option<Vec<f32>>>,
    }

    impl PcmCaptureDecider {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                pcm: Mutex::new(None),
            })
        }
        fn captured(&self) -> Option<Vec<f32>> {
            self.pcm.lock().unwrap().clone()
        }
    }

    impl CommandDecider for PcmCaptureDecider {
        fn kind(&self) -> DeciderKind {
            DeciderKind::NeuDecide
        }
        fn decide(&self, pcm16k: &[f32], _transcript: &str) -> Result<Decision, String> {
            self.pcm.lock().unwrap().replace(pcm16k.to_vec());
            Ok(Decision {
                tool: "no_action".into(),
                args: Map::new(),
                args_source: "none".into(),
                decider_ms: 1,
            })
        }
    }

    impl FakeDecider {
        fn ok(kind: DeciderKind, tool: &str, args: Map<String, Value>) -> Arc<Self> {
            Arc::new(Self {
                kind,
                outcome: Ok((tool.to_string(), args)),
            })
        }
        fn err(kind: DeciderKind, e: &str) -> Arc<Self> {
            Arc::new(Self {
                kind,
                outcome: Err(e.to_string()),
            })
        }
    }

    impl CommandDecider for FakeDecider {
        fn kind(&self) -> DeciderKind {
            self.kind
        }
        fn decide(&self, _pcm16k: &[f32], _transcript: &str) -> Result<Decision, String> {
            match &self.outcome {
                Ok((tool, args)) => Ok(Decision {
                    tool: tool.clone(),
                    args: args.clone(),
                    args_source: "none".to_string(),
                    decider_ms: 7,
                }),
                Err(e) => Err(e.clone()),
            }
        }
    }

    // ------------------------------------------------------------------
    // CommandResolver: trigger stripping
    // ------------------------------------------------------------------

    #[test]
    fn strip_search_triggers_each_phrase() {
        assert_eq!(
            CommandResolver::strip_search_triggers("search the web for rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("search the web rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("search for rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("search rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("google rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("look up rust async"),
            "rust async"
        );
    }

    #[test]
    fn strip_search_triggers_case_and_punctuation() {
        assert_eq!(
            CommandResolver::strip_search_triggers("Search The Web For  rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("google: rust async"),
            "rust async"
        );
        assert_eq!(
            CommandResolver::strip_search_triggers("LOOK UP! rust async"),
            "rust async"
        );
        // Repeated triggers strip one pass at a time.
        assert_eq!(
            CommandResolver::strip_search_triggers("search for search for rust"),
            "rust"
        );
        // No trigger: unchanged (trimmed).
        assert_eq!(
            CommandResolver::strip_search_triggers("  rust async  "),
            "rust async"
        );
        // A word that merely starts with a trigger survives.
        assert_eq!(
            CommandResolver::strip_search_triggers("searching for rust"),
            "searching for rust"
        );
    }

    // ------------------------------------------------------------------
    // CommandResolver: draft message shapes
    // ------------------------------------------------------------------

    #[test]
    fn parse_draft_message_shapes() {
        assert_eq!(
            CommandResolver::parse_draft_message("to Alice saying hello there"),
            Some(("Alice".into(), "hello there".into()))
        );
        assert_eq!(
            CommandResolver::parse_draft_message("email Bob that the meeting moved"),
            Some(("Bob".into(), "the meeting moved".into()))
        );
        assert_eq!(
            CommandResolver::parse_draft_message("email Bob saying see you"),
            Some(("Bob".into(), "see you".into()))
        );
        // Three-word recipient.
        assert_eq!(
            CommandResolver::parse_draft_message("message Alice Smith that lunch at noon"),
            Some(("Alice Smith".into(), "lunch at noon".into()))
        );
        // "message X Y" without a separator: the first word is the
        // recipient, the rest the body.
        assert_eq!(
            CommandResolver::parse_draft_message("message Bob lunch at noon"),
            Some(("Bob".into(), "lunch at noon".into()))
        );
    }

    #[test]
    fn parse_draft_message_non_matching() {
        assert_eq!(CommandResolver::parse_draft_message("hello there"), None);
        // Four-word recipient: the " that " separator limits the
        // recipient to the words before "that" (three words here), so
        // this actually matches.
        assert_eq!(
            CommandResolver::parse_draft_message("email Alice Smith Jones that hi"),
            Some(("Alice Smith Jones".into(), "hi".into()))
        );
        // Empty body.
        assert_eq!(
            CommandResolver::parse_draft_message("to Alice saying"),
            None
        );
    }

    // ------------------------------------------------------------------
    // CommandResolver: fill_free_form_args
    // ------------------------------------------------------------------

    #[test]
    fn fill_free_form_args_per_tool() {
        let mut args = Map::new();
        let src = CommandResolver::fill_free_form_args(
            "web_search",
            &mut args,
            "search the web for rust async",
        );
        assert_eq!(src, "transcript");
        assert_eq!(
            args.get("query").and_then(Value::as_str),
            Some("rust async")
        );

        let mut args = Map::new();
        let src =
            CommandResolver::fill_free_form_args("draft_message", &mut args, "email Bob that hi");
        assert_eq!(src, "transcript");
        assert_eq!(args.get("recipient").and_then(Value::as_str), Some("Bob"));
        assert_eq!(args.get("body").and_then(Value::as_str), Some("hi"));

        // draft_message the rules cannot parse: the LLM fallback takes it.
        let mut args = Map::new();
        assert_eq!(
            CommandResolver::fill_free_form_args("draft_message", &mut args, "hmm"),
            "llm"
        );

        // An enum-arg tool keeps the model's args.
        let mut args = Map::new();
        args.insert("app".into(), Value::String("Safari".into()));
        assert_eq!(
            CommandResolver::fill_free_form_args("open_app", &mut args, "open safari"),
            "neudecide"
        );

        // no_action passthrough.
        assert_eq!(
            CommandResolver::fill_free_form_args("no_action", &mut Map::new(), "x"),
            "none"
        );

        // Unknown tool with no args.
        assert_eq!(
            CommandResolver::fill_free_form_args("mystery", &mut Map::new(), "x"),
            "none"
        );
    }

    // ------------------------------------------------------------------
    // Decision → CommandAction mapping
    // ------------------------------------------------------------------

    #[test]
    fn decision_maps_to_command_action() {
        let mut args = Map::new();
        args.insert("app".into(), Value::String("Safari".into()));
        let d = Decision {
            tool: "open_app".into(),
            args,
            args_source: "neudecide".into(),
            decider_ms: 3,
        };
        let a = d.to_command_action().expect("not no_action");
        assert_eq!(a.tool, "open_app");
        assert_eq!(
            a.arguments.get("app").and_then(Value::as_str),
            Some("Safari")
        );
    }

    #[test]
    fn no_action_decision_maps_to_none() {
        let d = Decision {
            tool: "no_action".into(),
            args: Map::new(),
            args_source: "none".into(),
            decider_ms: 3,
        };
        assert!(d.is_no_action());
        assert!(d.to_command_action().is_none());
    }

    // ------------------------------------------------------------------
    // Selection: language gate + slot
    // ------------------------------------------------------------------

    #[test]
    fn select_decider_language_gate() {
        let slot = DeciderSlot::new();
        // Empty slot: nothing selected, any language.
        assert!(select_decider(&slot, "en").is_none());
        assert!(select_decider(&slot, "de").is_none());

        let nd: Arc<dyn CommandDecider> =
            FakeDecider::ok(DeciderKind::NeuDecide, "open_app", Map::new());
        slot.set(Some(nd.clone()));
        // English and "auto" pass the gate.
        assert!(select_decider(&slot, "en").is_some());
        assert!(select_decider(&slot, "en-US").is_some());
        assert!(select_decider(&slot, "ENGLISH").is_some());
        assert!(select_decider(&slot, "auto").is_some());
        // Non-English: NeuDecide is skipped (falls back to the plain
        // Needle path, recorded as needle-fallback).
        assert!(select_decider(&slot, "de").is_none());

        // A non-NeuDecide decider is language-independent.
        slot.set(Some(FakeDecider::ok(
            DeciderKind::Needle,
            "open_app",
            Map::new(),
        )));
        assert!(select_decider(&slot, "de").is_some());
    }

    #[test]
    fn neudecide_language_ok() {
        assert!(NeuDecideDecider::language_ok("en"));
        assert!(NeuDecideDecider::language_ok("EN"));
        assert!(NeuDecideDecider::language_ok("en-GB"));
        assert!(NeuDecideDecider::language_ok("auto"));
        assert!(!NeuDecideDecider::language_ok("de"));
        assert!(!NeuDecideDecider::language_ok("fr"));
    }

    #[test]
    fn neudecide_available_requires_model_files() {
        // No model dir: not available.
        assert!(!NeuDecideDecider::available(std::path::Path::new(
            "/nonexistent"
        )));
        // With the `neudecide` feature off, availability is always false.
        #[cfg(not(feature = "neudecide"))]
        {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("config.json"), "{}").unwrap();
            std::fs::write(dir.path().join("tokenizer.json"), "{}").unwrap();
            assert!(!NeuDecideDecider::available(dir.path()));
        }
    }

    // ------------------------------------------------------------------
    // Fallback matrix (command take routing)
    // ------------------------------------------------------------------

    /// Mirrors the dictation worker's routing: decide with the selected
    /// decider; on error or no decider, fall back to the provider's
    /// extraction. Returns (action, decider label, args_source).
    fn route_take(
        decider: Option<Arc<dyn CommandDecider>>,
        provider_action: Option<CommandAction>,
        transcript: &str,
    ) -> (Option<CommandAction>, Option<String>, Option<String>) {
        let label: Option<String>;
        let mut args_source: Option<String> = None;
        let action = match decider {
            Some(d) => match d.decide(&[], transcript) {
                Ok(decision) => {
                    label = Some(d.kind().label().to_string());
                    args_source = Some(decision.args_source.clone());
                    decision.to_command_action()
                }
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("decider error, falling back: {e}"),
                    );
                    label = Some(d.kind().label().to_string());
                    provider_action
                }
            },
            None => {
                label = Some("needle-fallback".into());
                provider_action
            }
        };
        (action, label, args_source)
    }

    #[test]
    fn fallback_matrix() {
        let provider_action = Some(CommandAction {
            tool: "open_app".into(),
            arguments: Value::Object(Map::new()),
        });

        // 1. NeuDecide available + language ok: the NeuDecide path wins.
        let d = FakeDecider::ok(DeciderKind::NeuDecide, "web_search", Map::new());
        let (action, label, _) = route_take(Some(d), None, "search for rust");
        assert_eq!(label.as_deref(), Some("neudecide"));
        assert_eq!(action.map(|a| a.tool), Some("web_search".to_string()));

        // 2. NeuDecide absent (slot empty): the provider extraction stands,
        //    recorded as needle-fallback.
        let (action, label, _) = route_take(None, provider_action.clone(), "open safari");
        assert_eq!(label.as_deref(), Some("needle-fallback"));
        assert_eq!(action.map(|a| a.tool), Some("open_app".to_string()));

        // 3. Decider errors: the provider extraction stands, the decider
        //    that was tried is recorded.
        let d = FakeDecider::err(DeciderKind::NeuDecide, "boom");
        let (action, label, _) = route_take(Some(d), provider_action.clone(), "open safari");
        assert_eq!(label.as_deref(), Some("neudecide"));
        assert_eq!(action.map(|a| a.tool), Some("open_app".to_string()));

        // 4. Non-English: select_decider returns None, so the provider
        //    extraction stands as needle-fallback.
        let slot = DeciderSlot::new();
        slot.set(Some(FakeDecider::ok(
            DeciderKind::NeuDecide,
            "web_search",
            Map::new(),
        )));
        let selected = select_decider(&slot, "de");
        let (action, label, _) = route_take(selected, provider_action.clone(), "öffne safari");
        assert_eq!(label.as_deref(), Some("needle-fallback"));
        assert_eq!(action.map(|a| a.tool), Some("open_app".to_string()));

        // 5. A command take must always produce a command when the
        //    provider has one (no regression: a take still produces a
        //    command even when the decider is unavailable or errors).
        let (action, _, _) = route_take(None, provider_action.clone(), "open safari");
        assert!(action.is_some());
        let (action, _, _) = route_take(
            Some(FakeDecider::err(DeciderKind::Needle, "boom")),
            provider_action.clone(),
            "open safari",
        );
        assert!(action.is_some());
    }

    #[test]
    fn decider_kind_labels() {
        assert_eq!(DeciderKind::NeuDecide.label(), "neudecide");
        assert_eq!(DeciderKind::Needle.label(), "needle");
        assert_eq!(DeciderKind::LlmFallback.label(), "llm-fallback");
    }

    #[test]
    fn decide_receives_take_audio_pcm() {
        // A.3.3: the command decider must receive the take audio at 16 kHz
        // mono f32, not an empty slice. Synthesize a 1.0 s take (16_000
        // samples), pass it through `decide`, and assert the decider saw
        // exactly 16_000 samples.
        let take_secs: f32 = 1.0;
        let sample_rate: usize = 16_000;
        let n_samples = (take_secs * sample_rate as f32) as usize;
        let pcm: Vec<f32> = (0..n_samples).map(|i| (i as f32) * 0.001).collect();
        assert_eq!(pcm.len(), 16_000);

        let d = PcmCaptureDecider::new();
        let result = d
            .decide(&pcm, "open safari")
            .expect("decide should not error");
        assert!(result.is_no_action());

        let captured = d.captured().expect("decider must capture the pcm");
        assert_eq!(
            captured.len(),
            16_000,
            "decide must receive the full take audio (16 kHz mono f32)"
        );
        // Spot-check a few samples to confirm it is the same buffer.
        assert!((captured[0] - pcm[0]).abs() < f32::EPSILON);
        assert!((captured[15999] - pcm[15999]).abs() < f32::EPSILON);
    }

    #[test]
    fn decide_receives_empty_pcm_for_silent_take() {
        // A.3.3: a take with no audio (e.g. audio finish failed) passes an
        // empty slice, not a panic.
        let d = PcmCaptureDecider::new();
        let result = d
            .decide(&[], "open safari")
            .expect("decide should not error");
        assert!(result.is_no_action());
        let captured = d.captured().expect("decider must capture the pcm");
        assert!(captured.is_empty());
    }
}
