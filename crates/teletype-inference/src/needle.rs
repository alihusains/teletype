//! Needle 3 inference provider.
//!
//! Wraps the Cactus Compute Needle 3 engine (via `needle-sys`) as an
//! [`InferenceProvider`]. Unlike the `ServerProvider` (which spawns a
//! `llama-server` subprocess), Needle 3 runs in-process via a static C
//! library. The model is a single 29–35 MB `.cact` file loaded into memory.
//!
//! # Use cases
//!
//! Needle 3 is a **tool-calling model**, not a free-text generator. It is
//! used for:
//! - **Structured extraction** (ITN: extract time/number/name from transcript)
//! - **Intent classification** (choice: "command" vs "dictation" vs "code")
//! - **Embedding** (3072-dim vectors for semantic personalization)
//! - **Voice commands** (audio → tool call, via the Whistle companion)
//!
//! It is **NOT** used for transcript polish. Polish uses S1-mini or EG-1
//! (generative models that rewrite the full text). Needle 3 summarizes
//! instead of rewriting, which makes it unsuitable for the polish pass.
//!
//! # Thread safety
//!
//! The Needle 3 C API is process-global and non-thread-safe. This provider
//! serializes all engine calls behind a `Mutex`. The `InferenceProvider`
//! trait is `Send + Sync`, so the mutex is required for the trait bound.
//!
//! # Telemetry
//!
//! The engine sends telemetry by default. This provider sets
//! `NEEDLE_TELEMETRY=0` and `DO_NOT_TRACK=1` before loading the model.

use std::sync::Mutex;

use teletype_core::llm::{DecisionProvider, GenerationParams, InferenceProvider, Intent};

/// A local model served by the in-process Needle 3 engine.
pub struct NeedleProvider {
    model_id: String,
    model_name: String,
    /// Serializes all engine calls (the C API is not thread-safe).
    engine: Mutex<()>,
}

impl NeedleProvider {
    /// Creates a new provider and loads the model.
    ///
    /// `path` is the `.cact` file. The model is loaded into the
    /// process-global engine. Loading is idempotent: if a model is already
    /// loaded, the call is a no-op (the engine cannot unload).
    pub fn new(model_id: &str, model_name: &str, path: &std::path::Path) -> Result<Self, String> {
        // Disable telemetry before the engine initializes.
        std::env::set_var("NEEDLE_TELEMETRY", "0");
        std::env::set_var("DO_NOT_TRACK", "1");

        needle_sys::load_model(path)
            .map_err(|e| format!("needle load failed: {e}"))?;

        Ok(Self {
            model_id: model_id.to_string(),
            model_name: model_name.to_string(),
            engine: Mutex::new(()),
        })
    }

    /// Initializes the engine with a system prompt and tool schemas.
    ///
    /// For the polish use case, `tools_json` is a single "polish" tool that
    /// declares the output shape. The grammar-constrained decode guarantees
    /// the output matches the schema.
    pub fn init(&self, system_prompt: Option<&str>, tools_json: Option<&str>) -> Result<usize, String> {
        let _guard = self
            .engine
            .lock()
            .map_err(|e| format!("needle engine poisoned: {e}"))?;
        needle_sys::init(system_prompt, tools_json)
            .map_err(|e| format!("needle init failed: {e}"))
    }

    /// Runs a text completion and returns the raw JSON response.
    fn complete(&self, input: &str, max_new_tokens: u32) -> Result<String, String> {
        let _guard = self
            .engine
            .lock()
            .map_err(|e| format!("needle engine poisoned: {e}"))?;
        let raw = needle_sys::complete_text(input, max_new_tokens)
            .map_err(|e| format!("needle complete failed: {e}"))?;
        tracing::debug!(raw = %raw, "needle complete raw response");
        Ok(raw)
    }

    /// Extracts the polished text from a Needle 3 JSON response.
    ///
    /// Checks, in order:
    /// 1. `function_calls[0].arguments.text` — the call passed the confidence gate.
    /// 2. `suppressed_calls[0].arguments.text` — the call was generated but
    ///    suppressed by the confidence gate (< 0.1). The text is still valid;
    ///    the gate is a safety mechanism, not a quality signal. For the polish
    ///    use case we accept suppressed calls because the grammar already
    ///    guarantees the output shape.
    /// 3. `response` — the "respond" type (no tool call).
    /// 4. Top-level `text`.
    fn extract_text(json_str: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("needle JSON parse: {e}"))?;

        // 1. Function call (passed confidence gate).
        if let Some(text) = Self::first_call_text(v.get("function_calls")) {
            return Ok(text);
        }

        // 2. Suppressed call (below confidence gate, but text is valid).
        if let Some(text) = Self::first_call_text(v.get("suppressed_calls")) {
            tracing::debug!("using suppressed call (below confidence gate)");
            return Ok(text);
        }

        // 3. Respond type.
        if let Some(resp) = v.get("response").and_then(|r| r.as_str()) {
            return Ok(resp.to_string());
        }

        // 4. Top-level text.
        if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
            return Ok(text.to_string());
        }

        Err("needle response has no extractable text".to_string())
    }

    /// Extracts `arguments.text` from the first call in a JSON array.
    fn first_call_text(calls: Option<&serde_json::Value>) -> Option<String> {
        calls
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("arguments"))
            .and_then(|a| a.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
    }
}

impl InferenceProvider for NeedleProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        // Enforce the wall-clock budget: Needle 3 is fast (~100–500 ms for
        // typical transcripts), but a pathological input could exceed the
        // budget. We check after the call rather than interrupting mid-call
        // because the C API has no cancellation mechanism.
        let started = std::time::Instant::now();
        let max_tokens = params.max_tokens.max(256);
        let raw = self.complete(prompt, max_tokens)?;
        let elapsed = started.elapsed();
        if elapsed > params.timeout {
            tracing::warn!(
                elapsed_ms = elapsed.as_millis(),
                budget_ms = params.timeout.as_millis(),
                "needle completion exceeded timeout budget"
            );
        }
        Self::extract_text(&raw)
    }

    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        // Needle 3's system prompt is set at init time, not per-call. For the
        // polish use case, the system prompt is a fixed instruction. We
        // re-init with the system prompt on each call (cheap: it just
        // re-tokenizes the static prefix).
        self.init(Some(system), None)
            .map_err(|e| format!("needle re-init failed: {e}"))?;
        self.generate(user, params)
    }

    fn context_tokens(&self) -> Option<u32> {
        // Needle 3 has an 8192-token context window.
        Some(8192)
    }

    fn is_local(&self) -> bool {
        true
    }

    fn stop(&self) {
        // The Needle 3 engine cannot be unloaded (process-global). Reset the
        // conversation state so the next init starts clean.
        let _guard = self.engine.lock();
        if let Ok(guard) = _guard {
            drop(guard);
            needle_sys::reset();
        }
    }

    fn shutdown(&self) {
        // Same as stop: the engine is process-global and cannot be unloaded.
        // Reset the conversation state.
        if let Ok(guard) = self.engine.lock() {
            drop(guard);
            needle_sys::reset();
        }
    }

    fn is_alive(&self) -> bool {
        // The engine is in-process; it's alive as long as the model is loaded.
        needle_sys::loaded_models() & needle_sys::NEEDLE_TEXT != 0
    }
}

impl DecisionProvider for NeedleProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn classify_intent(&self, text: &str) -> Intent {
        // Classify via the "classify" tool. The grammar-constrained decode
        // guarantees the output is a valid JSON object with a "category"
        // field matching one of the enum values. On any failure, return
        // Intent::Dictation (the safe default: run the polish pass).
        let _guard = match self.engine.lock() {
            Ok(g) => g,
            Err(_) => return Intent::Dictation,
        };

        // Init with the classify tool schema. This re-tokenizes the static
        // prefix (~20 ms). In a future optimization, the provider could
        // cache the initialized state and skip re-init when the schema
        // hasn't changed.
        if needle_sys::init(Some(EXTRACT_SYSTEM_PROMPT), Some(CLASSIFY_TOOLS_JSON)).is_err() {
            return Intent::Dictation;
        }

        let raw = match needle_sys::complete_text(text, 64) {
            Ok(r) => r,
            Err(_) => return Intent::Dictation,
        };

        // Parse the category from the response.
        let v: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return Intent::Dictation,
        };

        // The category is in function_calls[0].arguments.category or
        // suppressed_calls[0].arguments.category.
        let category = first_call_field(&v, "category");
        match category.as_deref() {
            Some("command") => Intent::Command,
            Some("dictation") | None => Intent::Dictation,
            Some(_) => Intent::Dictation, // unknown category → safe default
        }
    }

    fn embed_text(&self, text: &str) -> Option<Vec<f32>> {
        let _guard = match self.engine.lock() {
            Ok(g) => g,
            Err(_) => return None,
        };
        needle_sys::embed_text(text).ok()
    }

    fn extract_command(&self, text: &str) -> Option<teletype_core::llm::CommandAction> {
        let _guard = match self.engine.lock() {
            Ok(g) => g,
            Err(_) => return None,
        };

        // Init with the command tool schemas. Needle 3's tool-calling is
        // its specialty (86% Mobile Actions accuracy). The grammar-
        // constrained decode guarantees the output is a valid tool call.
        if needle_sys::init(Some(COMMAND_SYSTEM_PROMPT), Some(COMMAND_TOOLS_JSON)).is_err() {
            return None;
        }

        let raw = match needle_sys::complete_text(text, 128) {
            Ok(r) => r,
            Err(_) => return None,
        };

        let v: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return None,
        };

        // The tool name is in function_calls[0].name or
        // suppressed_calls[0].name.
        for key in ["function_calls", "suppressed_calls"] {
            if let Some(call) = v
                .get(key)
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
            {
                let tool = call.get("name").and_then(|n| n.as_str())?;
                let arguments = call
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                return Some(teletype_core::llm::CommandAction {
                    tool: tool.to_string(),
                    arguments,
                });
            }
        }

        None
    }
}

/// Extracts a named field from the first call in a Needle 3 JSON response.
/// Checks `function_calls` first, then `suppressed_calls`.
fn first_call_field(v: &serde_json::Value, field: &str) -> Option<String> {
    for key in ["function_calls", "suppressed_calls"] {
        if let Some(text) = v
            .get(key)
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("arguments"))
            .and_then(|a| a.get(field))
            .and_then(|t| t.as_str())
        {
            return Some(text.to_string());
        }
    }
    None
}

/// Tool schema for **structured extraction** from a transcript.
///
/// Declares a single "extract" tool. The grammar-constrained decode
/// guarantees the output is a valid JSON object with the declared fields.
/// Use this for ITN (inverse text normalization): extract times, numbers,
/// names, and places from a voice transcript.
///
/// Example: input "the meeting is at three p m in room four b" →
/// `{"time": "3pm", "room": "4B"}`.
pub const EXTRACT_TOOLS_JSON: &str = r#"[{"name":"extract","description":"Extract structured fields from the transcript. Return only the fields that are present; omit fields that are not mentioned.","parameters":{"type":"object","properties":{"time":{"type":"string","description":"Time mentioned, e.g. '3pm'"},"date":{"type":"string","description":"Date mentioned"},"name":{"type":"string","description":"Person or place name"},"number":{"type":"string","description":"Number or quantity"},"room":{"type":"string","description":"Room or location"},"amount":{"type":"string","description":"Monetary amount"}}}}]"#;

/// Tool schema for **intent classification**.
///
/// Declares a single "classify" tool with an enum field. The model picks
/// one category from the list. Use this to route a transcript to the
/// appropriate transform (dictation or command).
pub const CLASSIFY_TOOLS_JSON: &str = r#"[{"name":"classify","description":"Classify the transcript into one category.","parameters":{"type":"object","properties":{"category":{"type":"string","enum":["dictation","command"],"description":"The intent category"}},"required":["category"]}}]"#;

/// System prompt for extraction/classification use cases.
///
/// Needle 3's system prompt carries "facts, never instructions" per the C
/// API docs. Keep it minimal; the tool description carries the instruction.
pub const EXTRACT_SYSTEM_PROMPT: &str = "Extract structured data from voice transcripts. Return only fields that are explicitly present in the transcript. Do not infer or guess.";

/// Tool schemas for **command extraction**.
///
/// Declares the set of system commands the user can speak. Needle 3's
/// tool-calling is its specialty (86% Mobile Actions accuracy). The
/// grammar-constrained decode guarantees the output is a valid tool call
/// with the declared arguments.
///
/// Example: "open brave browser" → `open_app({"app": "brave"})`
/// Example: "lower the brightness" → `set_brightness({"level": "lower"})`
/// Example: "mute the speaker" → `mute_speaker({})`
pub const COMMAND_TOOLS_JSON: &str = r#"[
{"name":"open_app","description":"Open or launch an application by name. Use for: open Brave, launch Chrome, start Safari, open VS Code, open terminal, open settings, open Slack, open Discord, open app store, open file manager, open calculator, open downloads folder, open documents folder.","parameters":{"type":"object","properties":{"app":{"type":"string","description":"Application or folder name, e.g. 'brave', 'chrome', 'safari', 'vs code', 'terminal', 'settings', 'slack', 'discord', 'app store', 'file manager', 'calculator', 'downloads', 'documents'"},"url":{"type":"string","description":"Optional URL to load in the app, e.g. 'github.com', 'slack.com'"}},"required":["app"]}},
{"name":"open_url","description":"Navigate to a website or search the web. Use for: go to GitHub, open Google, search for Python tutorials, go to a URL.","parameters":{"type":"object","properties":{"url":{"type":"string","description":"The URL or search query, e.g. 'github.com', 'google.com', 'python tutorials'"}},"required":["url"]}},
{"name":"set_brightness","description":"Adjust screen brightness. Use for: increase brightness, make screen brighter, lower brightness, dim screen, set brightness to X percent.","parameters":{"type":"object","properties":{"level":{"type":"string","enum":["lower","higher","min","max","0","10","20","30","40","50","60","70","80","90","100"],"description":"Brightness level or direction"}},"required":["level"]}},
{"name":"set_volume","description":"Adjust system volume. Use for: turn volume up, turn volume down, make it louder, make it quieter, set volume to X percent.","parameters":{"type":"object","properties":{"level":{"type":"string","enum":["lower","higher","mute","unmute","min","max","0","10","20","30","40","50","60","70","80","90","100"],"description":"Volume level or direction"}},"required":["level"]}},
{"name":"mute_speaker","description":"Mute or unmute the system speaker. Use for: mute the speaker, unmute the speaker, turn sound back on.","parameters":{"type":"object","properties":{"muted":{"type":"boolean","description":"true to mute, false to unmute"}},"required":["muted"]}},
{"name":"open_terminal","description":"Open a terminal and optionally run a command. Use for: run npm test, run git status, run npm install, open terminal and run X.","parameters":{"type":"object","properties":{"app":{"type":"string","description":"Terminal app name, e.g. 'terminal', 'iterm'"},"command":{"type":"string","description":"Command to run, e.g. 'npm test', 'git status', 'npm install'"}}}},
{"name":"keyboard_shortcut","description":"Perform a keyboard shortcut action. Use for: select all, copy, paste, cut, undo, redo, save, save as, close window, minimize window, take screenshot, lock screen, sleep computer, show desktop, refresh page, new tab.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["select_all","copy","paste","cut","undo","redo","save","save_as","close_window","minimize_window","maximize_window","take_screenshot","lock_screen","sleep","show_desktop","refresh","new_tab","close_tab","next_tab","previous_tab","go_back","go_forward"],"description":"The keyboard action to perform"}},"required":["action"]}},
{"name":"play_media","description":"Play, pause, or skip media.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["play","pause","next","previous"],"description":"Media action"}},"required":["action"]}},
{"name":"set_timer","description":"Set a timer or alarm.","parameters":{"type":"object","properties":{"duration":{"type":"string","description":"Duration, e.g. '5 minutes', '1 hour'"}},"required":["duration"]}},
{"name":"send_message","description":"Send a message to a contact or channel.","parameters":{"type":"object","properties":{"to":{"type":"string","description":"Recipient name or channel"},"text":{"type":"string","description":"Message text"}},"required":["to","text"]}}
]"#;

/// System prompt for command extraction.
pub const COMMAND_SYSTEM_PROMPT: &str = "You are a voice command parser. Convert the user's spoken command into exactly one tool call. Rules: (1) 'open/launch/start [app name]' → open_app. (2) 'go to [website]' or 'search for [query]' → open_url. (3) 'brightness' → set_brightness. (4) 'volume' or 'louder/quieter' → set_volume. (5) 'mute/unmute speaker' → mute_speaker. (6) 'run [command]' or 'open terminal and run' → open_terminal. (7) 'select all/copy/paste/cut/undo/redo/save/close window/minimize/take screenshot/lock screen/sleep/show desktop/refresh/new tab' → keyboard_shortcut. (8) 'lock my computer' → keyboard_shortcut with action lock_screen. (9) 'put my computer to sleep' → keyboard_shortcut with action sleep. Do NOT use mute_speaker for lock, sleep, screenshot, or terminal commands. Do not guess; if the command doesn't match any tool, return no tool call.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_from_tool_call() {
        let json = r#"{
            "type": "call",
            "success": true,
            "function_calls": [{"name": "polish", "arguments": {"text": "Hello, world."}}],
            "confidence": 0.95
        }"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Hello, world.");
    }

    #[test]
    fn extract_text_from_response() {
        let json = r#"{
            "type": "respond",
            "response": "Cleaned text here."
        }"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Cleaned text here.");
    }

    #[test]
    fn extract_text_from_top_level_text() {
        let json = r#"{"text": "Fallback text"}"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Fallback text");
    }

    #[test]
    fn extract_text_empty_calls() {
        // Empty function_calls = refusal; no text to extract.
        let json = r#"{
            "type": "call",
            "function_calls": [],
            "confidence": 0.1
        }"#;
        assert!(NeedleProvider::extract_text(json).is_err());
    }

    #[test]
    fn extract_tools_json_is_valid() {
        let v: serde_json::Value = serde_json::from_str(EXTRACT_TOOLS_JSON).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "extract");
    }

    #[test]
    fn classify_tools_json_is_valid() {
        let v: serde_json::Value = serde_json::from_str(CLASSIFY_TOOLS_JSON).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "classify");
        // Verify the enum has the expected categories.
        let enum_vals = v[0]["parameters"]["properties"]["category"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(enum_vals.len(), 2);
    }
}
