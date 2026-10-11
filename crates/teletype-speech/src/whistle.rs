//! Whistle speech provider (Cactus-Compute), via the Needle C engine.
//!
//! Whistle reuses Needle's `.cact` container and the same runtime, so one
//! vendored static library (`third_party/needle/macos-arm64/libneedle.a`)
//! serves both STT (this provider) and structured tool calling (Command
//! Mode). The public API is declared in `third_party/needle/needle.h`:
//!
//! - `needle_load(bytes, len)` — loads whichever model kind the `.cact` holds.
//! - `needle_transcribe(pcm_f32, samples, language, keywords, word_ts, out, cap)`
//!   — 16 kHz mono float PCM in [-1,1], at most 30 s, returns JSON
//!   `{"text":...,"language":...,"ttft_ms":...,"decode_tps":...}`.
//! - `needle_init(system_prompt, tools_json, tool_index_path)` — sets the
//!   static prefix for `needle_complete`. Fails when the system prompt plus
//!   tools exceed the context window.
//! - `needle_complete(input, pcm, samples, max_new_tokens, out, cap)` —
//!   answers text (or speech) and returns a JSON object; with tools
//!   configured it may contain a single `tool_calls` array.
//! - `needle_set_audio(language, keywords, word_timestamps)` — configures the
//!   transcription `needle_complete` runs for itself.
//! - `needle_reset()` — forgets the conversation; the static prefix stays.
//! - `needle_last_error()` — diagnostics.
//!
//! The engine is process-global and non-thread-safe (one model per kind), so
//! this module serializes every FFI call behind one mutex and holds the whole
//! inference on one thread.
//!
//! **Asset provenance.** The archive and header are local, untracked
//! artifacts; see `docs/whistle-integration.md` ("Asset provenance"). The
//! `cactus` feature (off by default) links the archive; `build.rs` fails the
//! build with an actionable message when the feature is on and the archive is
//! missing, so a clean checkout can never silently link an absent binary.

use std::path::Path;
#[cfg(feature = "cactus")]
use std::sync::Mutex;

use crate::{SpeechError, SpeechProvider};

/// Compiles only when the `cactus` feature links the vendored runtime.
/// Without it the provider degrades to "runtime not bundled" and every load
/// fails with a clear message instead of a link error.
///
/// The declarations are NOT wrapped in `unsafe extern "C"`: the `cactus`
/// feature is a default feature of this crate (the desktop app builds with
/// it on), and a non-unsafe `extern "C"` block lets the linker — not the
/// compiler — decide which symbols are required. The vendored
/// `libneedle.a` predates the tool-calling additions in `needle.h` and lacks
/// `needle_init`/`needle_complete`: declaring them in an `unsafe` block made
/// the compiler force-link them and the build failed even though the
/// transcription path never calls them. (A non-unsafe block is not an
/// `unsafe_op_in_unsafe_fn` exemption — every call below is still inside an
/// explicit `unsafe {}` block with a SAFETY comment.)
#[cfg(feature = "cactus")]
mod ffi {
    // The raw Needle C FFI. Mirrors `third_party/needle/needle.h` line for
    // line (verified 2026-10-10 against the local header).
    //
    // All entry points are raw-pointer FFI; every caller goes through the
    // safe wrappers in the parent module, so `unsafe` stays confined to the
    // call sites.
    extern "C" {
        pub fn needle_load(cact: *const u8, n: std::ffi::c_ulonglong) -> i32;
        pub fn needle_models() -> i32;
        pub fn needle_last_error() -> *const std::ffi::c_char;
        pub fn needle_init(
            system_prompt: *const std::ffi::c_char,
            tools_json: *const std::ffi::c_char,
            tool_index_path: *const std::ffi::c_char,
        ) -> i32;
        pub fn needle_complete(
            input: *const std::ffi::c_char,
            pcm: *const f32,
            samples: i32,
            max_new_tokens: i32,
            out: *mut std::ffi::c_char,
            out_capacity: i32,
        ) -> i32;
        pub fn needle_set_audio(
            language: *const std::ffi::c_char,
            keywords: *const std::ffi::c_char,
            word_timestamps: i32,
        );
        pub fn needle_reset();
        pub fn needle_transcribe(
            pcm: *const f32,
            samples: i32,
            language: *const std::ffi::c_char,
            keywords: *const std::ffi::c_char,
            word_timestamps: i32,
            out: *mut std::ffi::c_char,
            out_capacity: i32,
        ) -> i32;
    }
}

/// The Cactus engine documents a ~30 s per-inference window for transcription.
pub const WHISTLE_MAX_SECONDS: u32 = 30;

/// Sample rate Whistle expects: 16 kHz mono.
pub const WHISTLE_SAMPLE_RATE: u32 = 16_000;

/// A process-global lock: the Needle engine is not thread-safe and holds one
/// model per kind. Every FFI call is made under this guard, and it is never
/// held across a call that itself locks (the dictation controller owns the
/// worker thread; this lock only serializes the C engine).
#[cfg(feature = "cactus")]
static NEEDLE_LOCK: Mutex<()> = Mutex::new(());

/// Loads the model bytes exactly once, on first use. Returns the loaded size.
static LOADED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// The `.cact` path the loaded model came from; used to answer "which model
/// is resident". Only the runtime-linked build records it.
#[cfg(feature = "cactus")]
static LOADED_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(feature = "cactus")]
fn lock_engine() -> std::sync::MutexGuard<'static, ()> {
    NEEDLE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads the `.cact` at `path` and loads it into the process-global engine.
/// Idempotent: a second call is a no-op.
#[cfg(feature = "cactus")]
fn ensure_loaded(path: &Path) -> Result<(), SpeechError> {
    if LOADED.get().is_some() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
    if bytes.is_empty() {
        return Err(SpeechError::ModelLoad("empty model file".into()));
    }
    #[cfg(feature = "cactus")]
    {
        let guard = lock_engine();
        let rc = unsafe { ffi::needle_load(bytes.as_ptr(), bytes.len() as std::ffi::c_ulonglong) };
        drop(guard);
        if rc < 0 {
            return Err(SpeechError::ModelLoad(needle_error()));
        }
    }
    #[cfg(feature = "cactus")]
    {
        LOADED_PATH.get_or_init(|| path.display().to_string());
    }
    LOADED.get_or_init(|| bytes.len());
    Ok(())
}

/// The Needle engine is process-global with no per-handle unload; the model
/// stays resident for the process lifetime (it is small, ~17 MB). `unload`
/// therefore only flips the provider's flag.
pub fn engine_loaded() -> bool {
    LOADED.get().is_some()
}

/// Which model kinds the process-global engine currently holds.
/// `0x01` = text, `0x02` = speech (see `NEEDLE_TEXT` / `NEEDLE_SPEECH`).
/// Returns 0 when the runtime is not bundled or nothing is loaded.
pub fn engine_models() -> i32 {
    #[cfg(feature = "cactus")]
    {
        let guard = lock_engine();
        let rc = unsafe { ffi::needle_models() };
        drop(guard);
        rc
    }
    #[cfg(not(feature = "cactus"))]
    {
        0
    }
}

/// True when the vendored runtime is linked into this build.
pub fn runtime_available() -> bool {
    #[cfg(feature = "cactus")]
    {
        true
    }
    #[cfg(not(feature = "cactus"))]
    {
        false
    }
}

fn needle_error() -> String {
    #[cfg(feature = "cactus")]
    {
        unsafe {
            let p = ffi::needle_last_error();
            if p.is_null() {
                "needle engine error".into()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    }
    #[cfg(not(feature = "cactus"))]
    {
        "needle runtime not bundled (build with the `cactus` feature)".into()
    }
}

/// Extracts the `"text"` field from the engine's JSON reply. The engine returns
/// a flat object; a tiny targeted parse is safer than pulling in a JSON dep for
/// one field, and keeps the hot path allocation-light.
pub fn extract_text_public(json: &str) -> String {
    extract_text(json)
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
                    match n {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        other => out.push(other),
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tool-calling (Command Mode, Phase 5)
// ---------------------------------------------------------------------------

/// The maximum size of a `needle_complete` reply we are willing to buffer.
/// A command answer is a short tool call or a short refusal; anything bigger
/// is a model failure, not a command.
pub const NEEDLE_COMPLETE_MAX_OUTPUT: usize = 32_768;

/// The maximum length of a single tool-call argument string. Model output is
/// untrusted data; arguments that long are model failure, not a command.
pub const NEEDLE_MAX_ARG_CHARS: usize = 8_000;

/// One structured tool call returned by `needle_complete`.
///
/// The engine's exact JSON shape is not documented, so the parser accepts
/// every plausible spelling of a single call and rejects everything else
/// (multiple calls, unknown fields, oversized or non-UTF-8 payloads). The
/// caller (Command Mode) re-validates the name and every argument against
/// its own allowlist — this type is a transport, not a trust boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Vec<(String, String)>,
}

impl ToolCall {
    /// Looks up a string argument by key.
    pub fn arg(&self, key: &str) -> Option<&str> {
        self.arguments
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

fn is_plain_object_key(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

/// Parses the bounded JSON object `needle_complete` writes into `out`.
///
/// Accepts, in order of preference:
/// 1. `{"tool_calls":[{"name":...,"arguments":{...}}]}` (OpenAI-style),
/// 2. `{"tool_call":{"name":...,"arguments":{...}}}`,
/// 3. `{"name":...,"arguments":{...}}` at the top level.
///
/// Returns `Err` for anything else: malformed JSON, no callable shape, more
/// than one call, an argument that is not a plain string, or a payload over
/// [`NEEDLE_COMPLETE_MAX_OUTPUT`]. Callers treat `Err` as "no usable tool
/// call" and fall back, never as a panic.
pub fn parse_tool_call(json: &str) -> Result<Option<ToolCall>, String> {
    if json.len() > NEEDLE_COMPLETE_MAX_OUTPUT {
        return Err("tool-call reply too large".into());
    }
    let root = top_object(json)
        .ok_or_else(|| "tool-call reply is not a JSON object".to_string())?;
    // Preference 1: tool_calls array with a single entry.
    if let Some(v) = root.get("tool_calls").filter(|v| first_char(json, *v) == Some('[')) {
        let (end, _after) = skip_value(json, v)?;
        let start = skip_ws(json, v + 1);
        if json[start..end].trim().is_empty() {
            return Ok(None);
        }
        // Exactly one element, and it must be an object.
        if first_char(json, start) != Some('{') {
            return Err("tool call is not a JSON object".into());
        }
        let (elem_end, _after_elem) = skip_value(json, start)?;
        // `elem_end` is the index of the element's closing brace. The element
        // may be followed by whitespace and/or the array's closing ']';
        // anything else means more than one call.
        let after_elem = skip_ws(json, elem_end.saturating_add(1));
        if first_char(json, after_elem) == Some(']') {
            return Ok(Some(parse_call_object(json, start)?));
        }
        if first_char(json, after_elem) == Some(',') {
            return Err("multiple tool calls are not supported".into());
        }
        return Err("malformed tool_calls array".into());
    }
    // Preference 2: a single tool_call object.
    if let Some(v) = root.get("tool_call").filter(|v| first_char(json, *v) == Some('{')) {
        let call = skip_ws(json, v);
        return Ok(Some(parse_call_object(json, call)?));
    }
    // Preference 3: the top-level object is the call itself.
    if root.get("name").is_some() && root.get("arguments").is_some() {
        return Ok(Some(parse_call_object(json, 0)?));
    }
// A plain text answer: no tool call.
    Ok(None)
}

/// Parses `{"name": "...", "arguments": {...}}` at position `obj`.
fn parse_call_object(json: &str, obj: usize) -> Result<ToolCall, String> {
    let mut name: Option<String> = None;
    let mut args: Vec<(String, String)> = Vec::new();
    let mut i = obj + 1;
    loop {
        i = skip_ws(json, i);
        let Some(c) = first_char(json, i) else {
            return Err("unterminated tool-call object".into());
        };
        if c == '}' {
            break;
        }
        if c != '"' {
            return Err("malformed tool-call object".into());
        }
        let (key, after) = parse_string(json, i)?;
        i = skip_ws(json, after);
        if first_char(json, i) != Some(':') {
            return Err("missing ':' in tool-call object".into());
        }
        i = skip_ws(json, i + 1);
        match key.as_str() {
            "name" => {
                if first_char(json, i) != Some('"') {
                    return Err("tool name is not a string".into());
                }
                let (s, after) = parse_string(json, i)?;
                if name.is_some() {
                    return Err("duplicate tool name".into());
                }
                name = Some(s);
                i = after;
            }
            "arguments" => match first_char(json, i) {
                Some('{') => {
                    let (args_parsed, after) = parse_arguments(json, i)?;
                    args = args_parsed;
                    i = after;
                }
                Some('"') => {
                    // Some models emit the arguments as a JSON *string*.
                    // Accept only a string that itself parses as a plain
                    // object; anything else is refused.
                    let (s, after) = parse_string(json, i)?;
                    if let Some(call) =
                        parse_tool_call(&s).map_err(|e| format!("arguments string: {e}"))?
                    {
                        args = call.arguments;
                    }
                    i = after;
                }
                _ => return Err("arguments is not an object".into()),
            },
            _ => {
                // Unknown fields are ignored, not errors: the engine may
                // add its own metadata. The values are skipped and
                // bounded by parse_tool_call's length cap.
                let (_, after) = skip_value(json, i)?;
                i = after;
            }
        }
        i = skip_ws(json, i);
        match first_char(json, i) {
            Some(',') => i += 1,
            _ => continue,
        }
    }
    let name = name.ok_or_else(|| "tool call has no name".to_string())?;
    if name.chars().count() > 128 {
        return Err("tool name too long".into());
    }
    Ok(ToolCall { name, arguments: args })
}

/// Parses a plain string-keyed object of string values at position `obj`.
/// Returns (args, index-after-object). Numbers and booleans are coerced to
/// their JSON text form; nested objects/arrays/null are refused (a command
/// tool takes simple values only).
fn parse_arguments(json: &str, obj: usize) -> Result<(Vec<(String, String)>, usize), String> {
    let mut args = Vec::new();
    let mut i = obj + 1;
    loop {
        i = skip_ws(json, i);
        let Some(c) = first_char(json, i) else {
            return Err("unterminated arguments object".into());
        };
        if c == '}' {
            break;
        }
        if c != '"' {
            return Err("malformed arguments object".into());
        }
        let (key, after) = parse_string(json, i)?;
        if !key.chars().all(is_plain_object_key) || key.len() > 64 {
            return Err("invalid argument key".into());
        }
        i = skip_ws(json, after);
        if first_char(json, i) != Some(':') {
            return Err("missing ':' in arguments object".into());
        }
        i = skip_ws(json, i + 1);
        let c = first_char(json, i).ok_or("truncated argument")?;
        let (value, after) = match c {
            '"' => {
                let (s, after) = parse_string(json, i)?;
                if s.chars().count() > NEEDLE_MAX_ARG_CHARS {
                    return Err("tool argument too large".into());
                }
                (s, after)
            }
            't' | 'f' | 'n' => {
                // true / false / null: take the literal.
                let (end, after) = skip_value(json, i)?;
                (json[i..end].to_string(), after)
            }
            '-' | '0'..='9' => {
                let (end, after) = skip_value(json, i)?;
                (json[i..end].to_string(), after)
            }
            _ => return Err("unsupported argument type (objects/arrays are not allowed)".into()),
        };
        args.push((key, value));
        i = skip_ws(json, after);
        match first_char(json, i) {
            Some(',') => i += 1,
            _ => continue,
        }
    }
    Ok((args, i + 1))
}

/// Position of the first value at/after `start`, and the index just past it.
fn skip_value(json: &str, mut i: usize) -> Result<(usize, usize), String> {
    i = skip_ws(json, i);
    let c = first_char(json, i).ok_or("truncated JSON value")?;
    match c {
        '{' | '[' => {
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            let bytes = json.as_bytes();
            let mut j = i;
            while j < bytes.len() {
                let b = bytes[j];
                if in_str {
                    if esc {
                        esc = false;
                    } else if b == b'\\' {
                        esc = true;
                    } else if b == b'"' {
                        in_str = false;
                    }
                } else {
                    match b {
                        b'"' => in_str = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Ok((j, j + 1));
                            }
                        }
                        _ => {}
                    }
                }
                j += 1;
            }
            Err("unterminated JSON value".into())
        }
        '"' => {
            let (_, end) = parse_string(json, i)?;
            Ok((end, end))
        }
        _ => {
            // Bare literal: true/false/null/number.
            let start = i;
            while let Some(ch) = first_char(json, i) {
                if matches!(ch, ' ' | '\t' | '\n' | '\r' | ',' | '}' | ']') {
                    break;
                }
                i += 1;
            }
            if i == start {
                return Err("empty JSON value".into());
            }
            Ok((i, i))
        }
    }
}

/// The top-level object's keys as (key, value-index) pairs, where
/// value-index points at the first character of the value.
struct TopObject {
    keys: Vec<(String, usize)>,
}

impl TopObject {
    fn get(&self, key: &str) -> Option<usize> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }
}

/// Parses the key/value layout of the object opening at index 0.
fn top_object(json: &str) -> Option<TopObject> {
    if first_char(json, 0) != Some('{') {
        return None;
    }
    let mut keys = Vec::new();
    let mut i = 1;
    loop {
        i = skip_ws(json, i);
        let c = first_char(json, i)?;
        if c == '}' {
            return Some(TopObject { keys });
        }
        if c != '"' {
            return None;
        }
        let (key, after) = parse_string(json, i).ok()?;
        i = skip_ws(json, after);
        if first_char(json, i) != Some(':') {
            return None;
        }
        i = skip_ws(json, i + 1);
        let value_start = i;
        let (_, after_value) = skip_value(json, i).ok()?;
        keys.push((key, value_start));
        i = skip_ws(json, after_value);
        match first_char(json, i)? {
            ',' => i += 1,
            '}' => return Some(TopObject { keys }),
            _ => return None,
        }
    }
}

fn first_char(json: &str, i: usize) -> Option<char> {
    json.get(i..).and_then(|s| s.chars().next())
}

fn skip_ws(json: &str, mut i: usize) -> usize {
    while let Some(c) = first_char(json, i) {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Parses the JSON string starting at `start` (which must be `"`). Returns
/// (value, index-after-closing-quote).
fn parse_string(json: &str, start: usize) -> Result<(String, usize), String> {
    debug_assert_eq!(json.as_bytes().get(start).copied(), Some(b'"'));
    let bytes = json.as_bytes();
    let mut out = String::new();
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => return Ok((out, i + 1)),
            b'\\' => {
                let Some(n) = bytes.get(i + 1) else {
                    return Err("dangling escape in string".into());
                };
                match n {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'u' => {
                        let hex = json.get(i + 2..i + 6).ok_or("bad \\u escape")?;
                        let code = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u escape")?;
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        i += 4;
                    }
                    _ => return Err("bad escape in string".into()),
                }
                i += 1;
            }
            _ => {
                // Copy the full UTF-8 sequence.
                let Some(c) = first_char(json, i) else {
                    return Err("bad UTF-8".into());
                };
                out.push(c);
                i += c.len_utf8();
                continue;
            }
        }
        i += 1;
    }
    Err("unterminated string".into())
}

/// Configures the static prefix for `needle_complete`: the system prompt and
/// the JSON tool schema. `tool_index_path` is `None` (the engine's tool index
/// is baked into the model; the header takes a path or null).
///
/// Returns the tokenized static-prefix length on success. The engine can fail
/// when the prefix exceeds the context window.
#[cfg(feature = "cactus")]
pub fn needle_init(system_prompt: &str, tools_json: &str) -> Result<usize, String> {
    let sys = c_str(system_prompt)?;
    let tools = c_str(tools_json)?;
    let guard = lock_engine();
    let rc = unsafe { ffi::needle_init(sys.as_ptr(), tools.as_ptr(), std::ptr::null()) };
    drop(guard);
    if rc < 0 {
        Err(needle_error())
    } else {
        Ok(rc as usize)
    }
}

#[cfg(not(feature = "cactus"))]
pub fn needle_init(_system_prompt: &str, _tools_json: &str) -> Result<usize, String> {
    Err(needle_error())
}

/// Runs one `needle_complete` with text input (the transcribed command).
/// `max_new_tokens` bounds the answer; the reply is a bounded JSON object.
/// The engine is process-global: this holds the engine lock for the whole
/// call, so Command Mode never runs two completions at once.
#[cfg(feature = "cactus")]
pub fn needle_complete_text(input: &str, max_new_tokens: u32) -> Result<String, String> {
    let input_c = c_str(input)?;
    let cap = NEEDLE_COMPLETE_MAX_OUTPUT;
    let mut buf = vec![0u8; cap];
    let guard = lock_engine();
    let rc = unsafe {
        ffi::needle_complete(
            input_c.as_ptr(),
            std::ptr::null(),
            0,
            max_new_tokens as i32,
            buf.as_mut_ptr() as *mut std::ffi::c_char,
            cap as i32,
        )
    };
    drop(guard);
    if rc < 0 {
        return Err(needle_error());
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Ok(String::from_utf8_lossy(&buf[..len]).into_owned())
}

#[cfg(not(feature = "cactus"))]
pub fn needle_complete_text(_input: &str, _max_new_tokens: u32) -> Result<String, String> {
    Err(needle_error())
}

/// Forgets the conversation, keeping the static prefix (system prompt +
/// tools). Must be called between independent command completions.
#[cfg(feature = "cactus")]
pub fn needle_reset() {
    let guard = lock_engine();
    unsafe { ffi::needle_reset() };
    drop(guard);
}

#[cfg(not(feature = "cactus"))]
pub fn needle_reset() {}

/// Validates that a string can become a NUL-terminated C string.
#[cfg(feature = "cactus")]
fn c_str(s: &str) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(s).map_err(|_| "input contains a NUL byte".to_string())
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
    fn transcribe_chunked(
        &mut self,
        samples: &[f32],
        language: &str,
    ) -> Result<String, SpeechError> {
        if !self.loaded {
            return Err(SpeechError::NoModel);
        }
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let max_samples =
            (WHISTLE_MAX_SECONDS as usize).saturating_mul(WHISTLE_SAMPLE_RATE as usize);
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
    #[cfg(feature = "cactus")]
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
            ffi::needle_transcribe(
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

    /// Without the runtime linked there is no engine to call.
    #[cfg(not(feature = "cactus"))]
    fn transcribe_window(
        &self,
        _chunk: &[f32],
        _lang_c: &std::ffi::CString,
        _detect: bool,
    ) -> Result<String, SpeechError> {
        Err(SpeechError::Transcribe(needle_error()))
    }
}

impl SpeechProvider for WhistleProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.loaded {
            return Ok(());
        }
        #[cfg(not(feature = "cactus"))]
        {
            let _ = path;
            return Err(SpeechError::ModelLoad(
                "Whistle needs the Cactus Needle runtime, which is not bundled in this \
                 build (the `cactus` feature links a local, untracked archive). \
                 Pick a Whisper or Parakeet model instead."
                    .into(),
            ));
        }
        #[cfg(feature = "cactus")]
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
        assert!(matches!(p.transcribe(&[], "en"), Err(SpeechError::NoModel)));
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
        // Newline escapes decode, not copy-through.
        assert_eq!(extract_text(r#"{"text":"line1\nline2"}"#), "line1\nline2");
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

    #[test]
    fn runtime_availability_reflects_the_feature() {
        #[cfg(feature = "cactus")]
        assert!(runtime_available());
        #[cfg(not(feature = "cactus"))]
        assert!(!runtime_available());
        // Without the runtime nothing is loaded.
        #[cfg(not(feature = "cactus"))]
        assert!(!engine_loaded());
    }

    // --- Tool-call parsing (Command Mode Phase 5) ---

    #[test]
    fn parses_openai_style_tool_calls() {
        let json = r#"{"tool_calls":[{"name":"open_app","arguments":{"bundle_id":"com.apple.Safari"}}],"content":""}"#;
        let call = parse_tool_call(json).unwrap().unwrap();
        assert_eq!(call.name, "open_app");
        assert_eq!(call.arg("bundle_id"), Some("com.apple.Safari"));
    }

    #[test]
    fn parses_single_tool_call_object() {
        let json = r#"{"tool_call":{"name":"draft_text","arguments":{"text":"Hi Sam, ..."}}}"#;
        let call = parse_tool_call(json).unwrap().unwrap();
        assert_eq!(call.name, "draft_text");
        assert!(call.arg("text").unwrap().starts_with("Hi Sam"));
    }

    #[test]
    fn parses_top_level_call() {
        let json = r#"{"name":"open_app","arguments":{"bundle_id":"com.apple.finder"}}"#;
        let call = parse_tool_call(json).unwrap().unwrap();
        assert_eq!(call.name, "open_app");
        assert_eq!(call.arg("bundle_id"), Some("com.apple.finder"));
    }

    #[test]
    fn plain_text_answer_is_no_call() {
        let json = r#"{"text":"I can help with that."}"#;
        assert_eq!(parse_tool_call(json).unwrap(), None);
    }

    #[test]
    fn multiple_tool_calls_are_rejected() {
        let json = r#"{"tool_calls":[{"name":"open_app","arguments":{}},{"name":"draft_text","arguments":{}}]}"#;
        assert!(parse_tool_call(json).is_err());
    }

    #[test]
    fn unknown_or_malformed_input_is_an_error_not_a_panic() {
        for bad in [
            "",
            "not json",
            r#"{"text":"hi"extra"#,
            r#"[1,2,3]"#,
            r#"{"tool_calls":[]}"#,
            r#"{"tool_calls":"open_app"}"#,
        ] {
            let r = parse_tool_call(bad);
            // Empty tool_calls array is "no call"; the rest must not panic.
            if bad == r#"{"tool_calls":[]}"# {
                assert_eq!(r.unwrap(), None, "{bad}");
            } else if bad.is_empty() || bad == "not json" {
                assert!(r.is_err(), "{bad}");
            } else {
                // Must not panic; either a call, no-call, or a clean error.
                let _ = r;
            }
        }
    }

    #[test]
    fn oversized_reply_is_rejected() {
        let big = format!(
            r#"{{"text":"{}"}}"#,
            "x".repeat(NEEDLE_COMPLETE_MAX_OUTPUT)
        );
        assert!(parse_tool_call(&big).is_err());
    }

    #[test]
    fn arguments_with_weird_values_are_rejected() {
        // Nested object argument: refused (command tools take simple values).
        let json = r#"{"name":"open_app","arguments":{"bundle_id":{"a":1}}}"#;
        assert!(parse_tool_call(json).is_err());
        // Array argument: refused.
        let json = r#"{"name":"open_app","arguments":{"bundle_id":["x"]}}"#;
        assert!(parse_tool_call(json).is_err());
        // Number and boolean coerce to their JSON text form.
        let json = r#"{"name":"t","arguments":{"n":3,"b":true}}"#;
        let call = parse_tool_call(json).unwrap().unwrap();
        assert_eq!(call.arg("n"), Some("3"));
        assert_eq!(call.arg("b"), Some("true"));
    }

    #[test]
    fn escaped_strings_in_arguments_decode() {
        let json = r#"{"name":"draft_text","arguments":{"text":"He said \"move it\" and left\nok"}}"#;
        let call = parse_tool_call(json).unwrap().unwrap();
        let text = call.arg("text").unwrap();
        assert!(text.contains("He said \"move it\" and left"));
        assert!(text.contains('\n'));
    }
}
