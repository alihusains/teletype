//! P2.7: command extraction via the local polish LLM.
//!
//! When Needle 3 is unavailable (its model files are not installed), an
//! armed or classified command take has no extractor: the pipeline returns
//! no [`CommandAction`] and the transcript falls back to plain dictation
//! injection. This module plugs that hole with the model that is already
//! resident — the polish LLM (Qwen3-1.7B, or whichever model the user
//! configured) — using the same tool definitions as the Needle path
//! ([`COMMAND_TOOLS_JSON`]) and grammar-constrained decoding via
//! llama-server's `json_schema` field.
//!
//! Safety posture (plan P2.7): the fallback can only ever *propose* a
//! `CommandAction`. The tool-name allowlist is enforced twice — once in the
//! decoding grammar, again when parsing the reply — and every argument is
//! re-validated by the executor (`command_executor.rs`) before any system
//! action runs. A malformed, out-of-allowlist, or unrunnable extraction
//! degrades to `None`, which the pipeline treats as "deliver the text",
//! never as a failure.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use teletype_core::llm::{
    CommandAction, DecisionProvider, GenerationParams, InferenceProvider, Intent,
};

use crate::needle::{COMMAND_SYSTEM_PROMPT, COMMAND_TOOLS_JSON};

/// Sentinel the model emits when nothing matches. Not a real tool; the
/// grammar needs a legal way out, and the parser maps it to `None`.
///
/// Without this sentinel a required-name grammar would *force* the model to
/// name a tool even for utterances that are not commands — a hallucinated
/// action is worse than no action.
const NO_TOOL: &str = "none";

/// Resolves the *currently loaded* polish provider.
///
/// A plain `Arc<dyn InferenceProvider>` snapshot would be stale: the app
/// builds this extractor once, then loads, swaps, and unloads models for
/// the rest of the session. The closure re-reads the live provider at
/// extraction time and returns `None` when no model is loaded.
pub type ProviderLookup = Arc<dyn Fn() -> Option<Arc<dyn InferenceProvider>> + Send + Sync>;

/// Command extraction over the local polish LLM (P2.7 fallback).
pub struct LlmCommandExtractor {
    lookup: ProviderLookup,
}

impl LlmCommandExtractor {
    pub fn new(lookup: ProviderLookup) -> Self {
        Self { lookup }
    }

    /// Runs one grammar-constrained generation and parses the tool call.
    /// `None` on any failure path — no provider loaded, generation error,
    /// truncated output, unparseable reply, or a tool outside the allowlist.
    pub fn extract(&self, text: &str) -> Option<CommandAction> {
        let provider = (self.lookup)()?;
        let params = GenerationParams {
            // A single tool call is ~40 tokens; 200 leaves room for a model
            // to open the object without ever approaching the window.
            // `temperature: 0.0` — this is a parser, not a writer.
            max_tokens: 200,
            temperature: 0.0,
            timeout: Duration::from_secs(15),
        };
        let raw = provider
            .generate_json(&build_system_prompt(), text, params, command_json_schema())
            .ok()?;
        parse_command_json(&raw)
    }
}

impl DecisionProvider for LlmCommandExtractor {
    fn model_id(&self) -> &str {
        "llm-fallback"
    }

    /// Always [`Intent::Dictation`]: classification stays TinyDecide's job
    /// (~5-12 ms). Asking a generation model would add hundreds of
    /// milliseconds to *every* utterance, and the fallback only ever runs
    /// once a command take is already known (armed per-take via `TakeKind`,
    /// or classified by TinyDecide).
    fn classify_intent(&self, _text: &str) -> Intent {
        Intent::Dictation
    }

    fn embed_text(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }

    fn extract_command(&self, text: &str) -> Option<CommandAction> {
        self.extract(text)
    }
}

impl crate::combined::CommandExtractor for LlmCommandExtractor {
    fn extract_command(&self, text: &str) -> Option<CommandAction> {
        self.extract(text)
    }

    fn model_id(&self) -> &str {
        DecisionProvider::model_id(self)
    }
}

/// Tool names + decoding schema, derived once from the shared Needle tool
/// list so the two extractors can never drift apart.
struct ToolPlan {
    names: Vec<String>,
    schema: Value,
}

fn tool_plan() -> &'static ToolPlan {
    static PLAN: OnceLock<ToolPlan> = OnceLock::new();
    PLAN.get_or_init(|| {
        let tools: Vec<Value> = serde_json::from_str(COMMAND_TOOLS_JSON).unwrap_or_default();
        let names: Vec<String> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        // One grammar branch per tool, carrying the tool's own parameter
        // schema from COMMAND_TOOLS_JSON — "the same JSON tool schema" the
        // plan requires. `oneOf` + `const` compile to a GBNF alternation on
        // llama-server, so a reply can only be one of these exact shapes.
        //
        // A loose `{"name": enum, "arguments": object}` schema was tried
        // first and failed live: s1-mini collapsed the argument object into
        // `{"name": "open_brave_browser"}` — the argument *keys* have to be
        // in the grammar, not just the prompt.
        let mut branches: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?;
                let params = t
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"}));
                Some(json!({
                    "type": "object",
                    "properties": {
                        "name": { "const": name },
                        "arguments": params
                    },
                    "required": ["name", "arguments"],
                    "additionalProperties": false
                }))
            })
            .collect();
        // The escape hatch: the model must have a legal way to decline.
        // Without it a required-name grammar *forces* a tool for any
        // utterance — a hallucinated action is worse than no action.
        branches.push(json!({
            "type": "object",
            "properties": {
                "name": { "const": NO_TOOL },
                "arguments": { "type": "object" }
            },
            "required": ["name", "arguments"],
            "additionalProperties": false
        }));
        let schema = json!({ "oneOf": branches });
        ToolPlan { names, schema }
    })
}

fn command_json_schema() -> &'static Value {
    &tool_plan().schema
}

/// Returns the current local date/time formatted as `YYYY-MM-DDTHH:MM` (24-hour),
/// or `None` on non-unix platforms or FFI failure.
///
/// Panic-free: any libc error returns `None` rather than panicking.
#[cfg(unix)]
pub fn current_local_datetime() -> Option<String> {
    use std::mem::MaybeUninit;

    let mut t: libc::time_t = -1;
    // SAFETY: `time` only writes the current epoch seconds through a valid
    // pointer to a local `time_t`, and reports failure with -1.
    if unsafe { libc::time(&mut t) } < 0 {
        return None;
    }
    // SAFETY: all-zero bytes are a valid `libc::tm`; fields are only read
    // after `localtime_r` succeeds and every field is plain integer data.
    let mut tm: libc::tm = unsafe { MaybeUninit::zeroed().assume_init() };
    // SAFETY: `t` is an initialized `time_t`, `tm` is a valid out-parameter,
    // and `localtime_r` has no aliasing requirements beyond those.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    format_local_datetime(tm.tm_year, tm.tm_mon, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

/// Returns the current local date/time formatted as `YYYY-MM-DDTHH:MM`,
/// or `None` on non-unix platforms.
#[cfg(not(unix))]
pub fn current_local_datetime() -> Option<String> {
    None
}

/// Pure formatter: takes already-extracted `tm` fields and produces the
/// `YYYY-MM-DDTHH:MM` string. Separated from the FFI read so it can be
/// unit-tested without touching libc.
///
/// `year` is years since 1900, `month` is 0-based (0 = January), `day` is
/// 1-based, `hour` is 0-23, `minute` is 0-59.
pub(crate) fn format_local_datetime(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
) -> Option<String> {
    if year < 100 || !(0..=11).contains(&month) || !(1..=31).contains(&day) || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    let y = year + 1900;
    let m = month + 1;
    Some(format!(
        "{y:04}-{m:02}-{day:02}T{hour:02}:{minute:02}"
    ))
}

/// `COMMAND_SYSTEM_PROMPT` plus the current local date/time line.
///
/// Rule (13) tells the model to resolve relative dates against that line, so a
/// prompt without it cannot schedule "tomorrow". Both command paths (this
/// crate's LLM providers and the Needle grammar decoder) must init with this,
/// not with the bare constant.
pub(crate) fn system_prompt_with_datetime() -> String {
    let date_line = match current_local_datetime() {
        Some(dt) => format!(
            "Current local date/time: {dt} (use this to resolve tomorrow, next Friday, at 9, and similar)."
        ),
        None => "Current local date/time: unknown".to_string(),
    };
    format!("{COMMAND_SYSTEM_PROMPT}\n\n{date_line}")
}

fn build_system_prompt() -> String {
    format!(
        "{}\n\nAvailable tools (JSON schema):\n{COMMAND_TOOLS_JSON}\n\n\
         Reply with exactly one JSON object and nothing else, e.g. \
         {{\"name\": \"open_app\", \"arguments\": {{\"app\": \"brave\"}}}} for \"open brave \
         browser\". Argument keys must match the chosen tool's schema exactly; `compose_email` and `create_calendar_event` open drafts and never send or save. Use only the tool names listed above, and inside \"arguments\" only the \
         keys that tool's schema declares. If the transcript is not a command, or no tool \
         fits, reply with {{\"name\": \"{NO_TOOL}\", \"arguments\": {{}}}}.",
        system_prompt_with_datetime()
    )
}

/// Parses a model reply into a command, enforcing the tool allowlist.
///
/// Accepts the three shapes a local LLM realistically produces for this
/// prompt: `{"name": ...}`, `{"tool": ...}`, and Needle's own
/// `{"function_calls": [{"name": ...}]}` wrapper (prompt-tuned models
/// sometimes echo the tool-calling convention they were trained with).
/// Markdown fences and prose around the object are tolerated. Anything
/// else — or a tool outside the shared allowlist — is `None`.
pub fn parse_command_json(raw: &str) -> Option<CommandAction> {
    let v = parse_json_object(raw)?;
    let (tool, arguments) = if let Some(call) = v
        .get("function_calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.first())
    {
        (
            call.get("name").and_then(Value::as_str),
            call.get("arguments").cloned(),
        )
    } else {
        (
            v.get("name")
                .or_else(|| v.get("tool"))
                .and_then(Value::as_str),
            v.get("arguments").cloned(),
        )
    };
    let tool = tool?;
    if tool.is_empty() || tool == NO_TOOL {
        return None;
    }
    if !tool_plan().names.iter().any(|n| n == tool) {
        // The grammar already restricts this; a provider that ignored the
        // schema can still get here. Refuse — unknown tools must never
        // reach the executor.
        return None;
    }
    let arguments = match arguments {
        None | Some(Value::Null) => json!({}),
        Some(v @ Value::Object(_)) => v,
        // Some models double-encode the arguments object as a string.
        Some(Value::String(s)) => {
            let parsed: Value = serde_json::from_str(&s).ok()?;
            parsed.as_object()?;
            parsed
        }
        Some(_) => return None,
    };
    Some(CommandAction {
        tool: tool.to_string(),
        arguments,
    })
}

/// Finds the JSON object in a possibly-noisy reply.
fn parse_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            return Some(v);
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }
    let v: Value = serde_json::from_str(&trimmed[start..=end]).ok()?;
    v.is_object().then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Seen {
        system: Option<String>,
        user: Option<String>,
        schema: Option<Value>,
    }

    /// A scripted `InferenceProvider` standing in for llama-server: returns
    /// a canned reply and records what it was asked for.
    struct FakeLlm {
        reply: Result<String, String>,
        seen: Mutex<Seen>,
    }

    impl FakeLlm {
        fn new(reply: Result<&str, &str>) -> Arc<Self> {
            Arc::new(Self {
                reply: reply.map(str::to_string).map_err(str::to_string),
                seen: Mutex::new(Seen::default()),
            })
        }

        fn extractor(self: &Arc<Self>) -> LlmCommandExtractor {
            let fake = Arc::clone(self);
            let lookup: ProviderLookup =
                Arc::new(move || Some(Arc::clone(&fake) as Arc<dyn InferenceProvider>));
            LlmCommandExtractor::new(lookup)
        }
    }

    impl InferenceProvider for FakeLlm {
        fn model_id(&self) -> &str {
            "fake-llm"
        }

        fn model_name(&self) -> &str {
            "Fake LLM"
        }

        fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
            Ok("not used".into())
        }

        fn generate_json(
            &self,
            system: &str,
            user: &str,
            _params: GenerationParams,
            schema: &Value,
        ) -> Result<String, String> {
            let mut seen = self.seen.lock().unwrap();
            seen.system = Some(system.to_string());
            seen.user = Some(user.to_string());
            seen.schema = Some(schema.clone());
            self.reply.clone()
        }
    }

    #[test]
    fn extracts_a_valid_tool_call() {
        let fake = FakeLlm::new(Ok(
            r#"{"name":"open_url","arguments":{"url":"github.com"}}"#,
        ));
        let cmd = fake
            .extractor()
            .extract("go to github.com")
            .expect("extracted");
        assert_eq!(cmd.tool, "open_url");
        assert_eq!(cmd.arguments["url"], "github.com");
    }

    #[test]
    fn sends_the_shared_tool_schema_and_transcript() {
        let fake = FakeLlm::new(Ok(r#"{"name":"mute_speaker","arguments":{"muted":true}}"#));
        let _ = fake.extractor().extract("mute the speaker");
        let seen = fake.seen.lock().unwrap();
        // The system prompt carries the same tool definitions the Needle
        // path serves …
        let system = seen.system.as_deref().unwrap();
        assert!(system.contains("open_app"));
        assert!(system.contains("mute_speaker"));
        // … and the transcript goes through as the user message, verbatim.
        assert_eq!(seen.user.as_deref(), Some("mute the speaker"));
        // The decoding grammar is a `oneOf` over the exact tool shapes from
        // the shared list (with the "none" escape hatch), so it constrains
        // argument *keys* too — not just the tool name.
        let branches = seen.schema.as_ref().unwrap()["oneOf"].as_array().unwrap();
        let names: Vec<&str> = branches
            .iter()
            .filter_map(|b| b["properties"]["name"]["const"].as_str())
            .collect();
        assert!(names.contains(&"open_app"));
        assert!(names.contains(&"send_message"));
        assert!(names.contains(&NO_TOOL));
        let open_app = branches
            .iter()
            .find(|b| b["properties"]["name"]["const"] == "open_app")
            .unwrap();
        assert!(open_app["properties"]["arguments"]["properties"]["app"].is_object());
        assert_eq!(
            open_app["properties"]["arguments"]["required"],
            json!(["app"])
        );
    }

    #[test]
    fn rejects_tool_outside_the_allowlist() {
        let fake = FakeLlm::new(Ok(r#"{"name":"delete_everything","arguments":{}}"#));
        assert!(fake.extractor().extract("delete everything").is_none());
    }

    #[test]
    fn none_sentinel_is_not_a_command() {
        let fake = FakeLlm::new(Ok(r#"{"name":"none","arguments":{}}"#));
        assert!(fake.extractor().extract("tell me a joke").is_none());
    }

    #[test]
    fn tolerates_fences_and_prose_around_the_object() {
        let fake = FakeLlm::new(Ok(
            "Sure, here you go:\n```json\n{\"name\":\"keyboard_shortcut\",\"arguments\":{\"action\":\"copy\"}}\n```",
        ));
        let cmd = fake.extractor().extract("copy that").expect("extracted");
        assert_eq!(cmd.tool, "keyboard_shortcut");
        assert_eq!(cmd.arguments["action"], "copy");
    }

    #[test]
    fn accepts_tool_key_and_needle_wrapper_shapes() {
        for reply in [
            r#"{"tool":"open_app","arguments":{"app":"safari"}}"#,
            r#"{"function_calls":[{"name":"open_app","arguments":{"app":"safari"}}]}"#,
        ] {
            let cmd = FakeLlm::new(Ok(reply))
                .extractor()
                .extract("open safari")
                .unwrap_or_else(|| panic!("extracted from {reply}"));
            assert_eq!(cmd.tool, "open_app");
            assert_eq!(cmd.arguments["app"], "safari");
        }
    }

    #[test]
    fn accepts_string_encoded_arguments() {
        let fake = FakeLlm::new(Ok(
            r#"{"name":"open_app","arguments":"{\"app\":\"slack\"}"}"#,
        ));
        let cmd = fake.extractor().extract("open slack").expect("extracted");
        assert_eq!(cmd.tool, "open_app");
        assert_eq!(cmd.arguments["app"], "slack");
    }

    #[test]
    fn missing_arguments_become_an_empty_object() {
        let fake = FakeLlm::new(Ok(r#"{"name":"mute_speaker"}"#));
        let cmd = fake.extractor().extract("mute").expect("extracted");
        assert_eq!(cmd.tool, "mute_speaker");
        assert_eq!(cmd.arguments, json!({}));
    }

    #[test]
    fn rejects_non_object_arguments() {
        let fake = FakeLlm::new(Ok(r#"{"name":"open_app","arguments":[1,2,3]}"#));
        assert!(fake.extractor().extract("open it").is_none());
    }

    #[test]
    fn prose_without_json_means_no_command() {
        let fake = FakeLlm::new(Ok("I am not sure what you mean."));
        assert!(fake.extractor().extract("hmm").is_none());
    }

    #[test]
    fn no_loaded_model_means_no_command() {
        // The lookup runs at extraction time: no model loaded → `None`,
        // and the pipeline's deliver-the-text fallback takes over.
        let lookup: ProviderLookup = Arc::new(|| None);
        let extractor = LlmCommandExtractor::new(lookup);
        assert!(extractor.extract("open safari").is_none());
    }

    #[test]
    fn generation_error_means_no_command() {
        let fake = FakeLlm::new(Err("connection refused"));
        assert!(fake.extractor().extract("open safari").is_none());
    }

    #[test]
    fn tool_plan_covers_all_shared_tools_without_duplicates() {
        let plan = tool_plan();
        assert_eq!(
            plan.names,
            vec![
                "open_app",
                "open_url",
                "set_brightness",
                "set_volume",
                "mute_speaker",
                "keyboard_shortcut",
                "play_media",
                "set_timer",
                "send_message",
                "compose_email",
                "create_calendar_event",
                "open_file",
                "type_text",
            ],
            "shared tool list drifted from the executor contract"
        );
        let mut seen = std::collections::HashSet::new();
        for n in &plan.names {
            assert!(seen.insert(n.clone()), "duplicate tool name: {n}");
        }
        // Every tool name has a grammar branch in the decoding schema.
        let branches = plan.schema["oneOf"].as_array().unwrap();
        let consts: Vec<&str> = branches
            .iter()
            .filter_map(|b| b["properties"]["name"]["const"].as_str())
            .collect();
        assert_eq!(consts.len(), plan.names.len() + 1, "plus the none escape hatch");
        for name in &plan.names {
            assert!(consts.contains(&name.as_str()), "missing branch for {name}");
        }
    }

    #[test]
    fn parses_a_valid_compose_email_call() {
        let fake = FakeLlm::new(Ok(
            r#"{"name":"compose_email","arguments":{"to":"bob@example.com","subject":"Re: invoice","body":"Hi Bob, attaching the invoice."}}"#,
        ));
        let cmd = fake.extractor().extract("email bob at example dot com that the invoice is attached").expect("extracted");
        assert_eq!(cmd.tool, "compose_email");
        assert_eq!(cmd.arguments["to"], "bob@example.com");
        assert_eq!(cmd.arguments["subject"], "Re: invoice");
        assert_eq!(cmd.arguments["body"], "Hi Bob, attaching the invoice.");
    }

    #[test]
    fn prompt_contains_new_tools_and_routing_clauses() {
        let system = build_system_prompt();
        for name in [
            "compose_email",
            "create_calendar_event",
            "open_file",
            "type_text",
        ] {
            assert!(system.contains(name), "prompt missing tool {name}");
        }
        for clause in [
            "NEVER invent or guess an email address",
            "a folder is never open_app, and an app name is never open_file",
            "never add Enter and never send",
            "default the end to one hour after the start",
            "never words like 'tomorrow'",
        ] {
            assert!(system.contains(clause), "prompt missing clause: {clause}");
        }
    }

    #[test]
    fn prompt_carries_the_current_local_date_line() {
        let system = build_system_prompt();
        assert!(
            system.starts_with(COMMAND_SYSTEM_PROMPT),
            "date line must be appended, not baked into the constant"
        );
        assert!(
            system.contains("Current local date/time: 20")
                || system.contains("Current local date/time: unknown"),
            "missing date line in: {system}"
        );
        assert!(system.contains("use this to resolve tomorrow, next Friday, at 9, and similar"));
    }

    #[test]
    fn format_local_datetime_builds_iso24h() {
        assert_eq!(
            format_local_datetime(126, 9, 26, 21, 14).as_deref(),
            Some("2026-10-26T21:14")
        );
        assert_eq!(
            format_local_datetime(126, 0, 1, 0, 5).as_deref(),
            Some("2026-01-01T00:05")
        );
    }

    #[test]
    fn format_local_datetime_rejects_out_of_range_fields() {
        assert!(format_local_datetime(126, 12, 1, 0, 0).is_none());
        assert!(format_local_datetime(126, 0, 0, 0, 0).is_none());
        assert!(format_local_datetime(126, 0, 1, 24, 0).is_none());
        assert!(format_local_datetime(126, 0, 1, 0, 60).is_none());
        assert!(format_local_datetime(99, 0, 1, 0, 0).is_none());
    }

    #[test]
    fn current_local_datetime_is_well_formed_or_none() {
        if let Some(dt) = current_local_datetime() {
            assert_eq!(dt.len(), 16, "expected YYYY-MM-DDTHH:MM, got {dt}");
            assert_eq!(&dt[4..5], "-");
            assert_eq!(&dt[7..8], "-");
            assert_eq!(&dt[10..11], "T");
            assert_eq!(&dt[13..14], ":");
        }
    }

    #[test]
    fn classifies_as_dictation_so_plain_takes_stay_plain() {
        // Contract: this provider never classifies a take as a command; it
        // only extracts once something else (TakeKind or TinyDecide)
        // already said "command".
        let fake = FakeLlm::new(Ok(r#"{"name":"open_app","arguments":{"app":"safari"}}"#));
        assert_eq!(
            fake.extractor().classify_intent("open safari"),
            Intent::Dictation
        );
    }

    /// Live end-to-end check against the real local LLM: spawns
    /// llama-server with the S1-mini GGUF, sends the exact request body the
    /// fallback builds, and parses the reply with the real parser.
    ///
    /// Ignored by default (it needs the model file and ~1 GB of RAM). Run:
    ///
    /// ```text
    /// cargo test -p teletype-inference --lib llm_fallback_live \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// `TELETYPE_LIVE_GGUF` overrides the model path. `TELETYPE_LLAMA_SERVER`
    /// points at the llama-server binary when it is not next to the test
    /// executable (`find_llama_server_bin`'s usual places).
    #[test]
    #[ignore = "spawns llama-server with a local GGUF; run explicitly"]
    fn llm_fallback_live() {
        let path = std::env::var("TELETYPE_LIVE_GGUF")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join("Library/Application Support/com.teletype.app/models/s1-mini.gguf")
            });
        if !path.exists() {
            eprintln!("skipping: no GGUF at {}", path.display());
            return;
        }
        let provider = Arc::new(crate::server::ServerProvider::new(
            "s1-mini", "S1 Mini", &path,
        ));
        provider.warm_up().expect("llama-server warm-up");
        let for_lookup = Arc::clone(&provider);
        let lookup: ProviderLookup =
            Arc::new(move || Some(Arc::clone(&for_lookup) as Arc<dyn InferenceProvider>));
        let extractor = LlmCommandExtractor::new(lookup);

        // 1. Plumbing: the server accepts our request body (including the
        //    `json_schema` string) and answers with schema-shaped JSON.
        let raw = provider
            .generate_json(
                &build_system_prompt(),
                "open brave browser",
                GenerationParams {
                    max_tokens: 200,
                    temperature: 0.0,
                    timeout: Duration::from_secs(60),
                },
                command_json_schema(),
            )
            .expect("generate_json against the live server");
        eprintln!("raw reply: {raw}");
        let v: Value = serde_json::from_str(raw.trim()).expect("reply is valid JSON");
        let name = v["name"].as_str().expect("name field is a string");
        assert!(
            name == NO_TOOL || tool_plan().names.iter().any(|n| n == name),
            "grammar allowed a foreign tool name: {name}"
        );

        // 2. The full extractor path on a clear command.
        let cmd = extractor.extract("open brave browser").expect("extracted");
        eprintln!("extracted: {} {}", cmd.tool, cmd.arguments);
        assert_eq!(cmd.tool, "open_app");
        let app = cmd.arguments["app"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        assert!(app.contains("brave"), "unexpected app argument: {app}");

        // 3. A non-command must not crash; if the model picks something, it
        //    must still be one of the allowlisted tools (never free-form).
        if let Some(cmd) = extractor.extract("I really enjoyed that movie last night") {
            assert!(
                tool_plan().names.contains(&cmd.tool),
                "non-command produced a foreign tool: {}",
                cmd.tool
            );
            eprintln!("note: non-command mapped to {} {}", cmd.tool, cmd.arguments);
        }
    }
}
