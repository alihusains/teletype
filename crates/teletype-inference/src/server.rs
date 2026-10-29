//! Subprocess `llama-server` inference provider.
//!
//! llama.cpp's ggml cannot be linked in-process alongside whisper.cpp's
//! ggml, so the model runs in a separate `llama-server` process (the standard
//! approach for in-process ggml conflicts). This provider:
//!
//! 1. Locates the binary (`TELETYPE_LLAMA_SERVER`, next to the app exe,
//!    bundle `Resources/`, then `PATH`).
//! 2. Spawns it on `127.0.0.1:<ephemeral port>` with a random API key.
//! 3. Polls `GET /health` until ready (or the child dies).
//! 4. Serves [`InferenceProvider::generate`] via `POST /v1/chat/completions`.
//! 5. Kills the child on `Drop`.

use std::{
    env,
    fs::File,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use teletype_core::llm::{GenerationParams, InferenceProvider};

/// Model load can take a while for larger GGUFs on first start. 120s is a
/// safe upper bound: large multi-shard models (several GB across several
/// shards) can exceed 60s to load on a cold disk cache, and a shorter
/// timeout kills the child mid-load with SIGKILL, surfacing as a spurious
/// download failure. Warm-cache loads finish in ~2s, so this only matters
/// for cold starts.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_INTERVAL: Duration = Duration::from_millis(250);
/// How long one `read` waits before the stream loop re-checks the overall
/// deadline. Long enough that a model generating one token is never mistaken
/// for a hung server, short enough that a genuinely wedged connection is
/// reported in seconds rather than at the full generation timeout.
const READ_SLICE: Duration = Duration::from_secs(5);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Chat-template kwargs for a model id, applied at spawn and on every request.
///
/// S1-mini is a Qwen3 fine-tune trained with thinking mode OFF. The embedded
/// chat template defaults to thinking ON, so without this the assistant turn
/// starts inside a live think block: the model emits an empty `think` block
/// and stops, returning empty content for every polish ("S1 generation
/// failed"). The model card mandates `enable_thinking: false`; it also warns
/// against `--reasoning-budget 0`, which suppresses the block a different way
/// and degrades output.
fn default_chat_template_kwargs(model_id: &str) -> &'static str {
    if model_id == "s1-mini" || model_id == "qwen3.5-2b" {
        r#"{"enable_thinking":false}"#
    } else {
        r#"{}"#
    }
}

/// Injects the parsed chat-template kwargs into a request body. llama.cpp's
/// `/v1/chat/completions` accepts `chat_template_kwargs` per request; sending
/// it on every call makes the template choice survive regardless of server
/// version quirks, and an empty object is a documented no-op.
fn apply_chat_template_kwargs(body: &mut Value, kwargs: &str) {
    if kwargs == "{}" {
        return;
    }
    if let Ok(v) = serde_json::from_str::<Value>(kwargs) {
        body["chat_template_kwargs"] = v;
    }
}

/// A local model served by a child `llama-server` process.
pub struct ServerProvider {
    model_id: String,
    model_name: String,
    path: PathBuf,
    /// Chat-template kwargs applied at spawn (server-level) and on every
    /// request (request-level). S1-mini is trained with Qwen3 thinking mode
    /// OFF; without `enable_thinking: false` the template defaults to
    /// thinking ON, the model emits an empty `think` block and stops, and
    /// every polish returns empty content ("S1 generation failed").
    /// Qwen3.5-2B defaults to non-thinking, and we pin it there so a template
    /// change cannot silently add seconds of reasoning to a polish.
    chat_template_kwargs: &'static str,
    runtime: Mutex<Option<Runtime>>,
}

struct Runtime {
    base_url: String,
    api_key: String,
    child: Child,
    log_path: PathBuf,
}

impl ServerProvider {
    /// Creates a provider that will spawn `llama-server` for `path` on
    /// [`ServerProvider::warm_up`].
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: &Path) -> Self {
        let model_id = id.into();
        let kwargs = default_chat_template_kwargs(&model_id);
        Self {
            model_id,
            model_name: name.into(),
            path: path.to_path_buf(),
            chat_template_kwargs: kwargs,
            runtime: Mutex::new(None),
        }
    }

    /// Where we record the pid of the child we spawn, so a crashed run's
    /// orphan can be reaped on the next launch. Lives beside the model file
    /// (inside the app's own data directory), not in a shared temp dir.
    fn pid_file(&self) -> PathBuf {
        self.path.with_extension("llama-server.pid")
    }

    /// Spawns `llama-server` and waits until `/health` returns 200.
    /// No-op if already running.
    pub fn warm_up(&self) -> Result<(), String> {
        let mut guard = self
            .runtime
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        if guard.is_some() {
            return Ok(());
        }
        if !self.path.exists() {
            return Err(format!("Model file not found: {}", self.path.display()));
        }

        // Defense in depth: a prior run that was force-killed (crash, Cmd-Q
        // while loading) can leave an orphaned llama-server holding the model
        // in RAM. Reap *our own* orphan before spawning, so we never stack a
        // second copy on top and OOM the new load. We only ever signal a pid we
        // recorded ourselves and whose command line still matches this model,
        // so a llama-server the user started is never touched.
        let pid_file = self.pid_file();
        kill_stale_servers(&pid_file);

        let bin = find_llama_server_bin()?;
        let port = pick_port()?;
        let api_key = uuid::Uuid::new_v4().to_string();
        let log_path = env::temp_dir().join(format!(
            "teletype-llama-server-{}-{port}.log",
            self.model_id
        ));
        let log = File::create(&log_path)
            .map_err(|e| format!("create server log {}: {e}", log_path.display()))?;

        let mut child = Command::new(&bin)
            .arg("-m")
            .arg(&self.path)
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .arg("--api-key")
            .arg(&api_key)
            .args([
                "-c",
                "4096",
                "-fa",
                "on",
                "-ctk",
                "q8_0",
                "-ctv",
                "q8_0",
                "--jinja",
                // P6.5: KV-cache reuse (256 tokens) speeds up repeated
                // prompts with the same static prefix (P1.7 cache warmth).
                // -np 1: single parallel sequence — polish is one-shot.
                "--cache-reuse",
                "256",
                "-np",
                "1",
            ])
            .args(["--chat-template-kwargs", self.chat_template_kwargs])
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

        let base_url = format!("http://127.0.0.1:{port}");
        if let Err(e) = wait_until_ready(&mut child, &base_url, &api_key, &log_path, &bin) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(self.pid_file());
            return Err(e);
        }
        // Record the pid so a crashed run's orphan can be reaped next time.
        let _ = std::fs::write(self.pid_file(), child.id().to_string());

        tracing::info!(model = %self.model_id, port, "llama-server ready");
        *guard = Some(Runtime {
            base_url,
            api_key,
            child,
            log_path,
        });
        Ok(())
    }

    /// Stops the child process if it is running.
    fn shutdown(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(rt) = guard.take() {
                stop_child(rt.child, &rt.log_path, &self.pid_file());
            }
        }
    }

    fn endpoint(&self) -> Result<(String, String), String> {
        let guard = self
            .runtime
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        let rt = guard
            .as_ref()
            .ok_or_else(|| "llama-server is not running (call warm_up)".to_string())?;
        Ok((rt.base_url.clone(), rt.api_key.clone()))
    }

    pub fn model_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    /// True when the subprocess is up and was ready at warm-up time.
    pub fn is_ready(&self) -> bool {
        self.runtime.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

impl Drop for ServerProvider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl InferenceProvider for ServerProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    /// P5.1: this *is* the local provider.
    ///
    /// The trait default is `false`, and until this override existed nothing
    /// in production implemented it — every `is_local` in the repo was a test
    /// mock. So `Pipeline`'s polish gate, which requires
    /// `self.inference.is_some_and(|p| p.is_local())`, could never take its
    /// skip branch, while `polish_gate_enabled` shipped defaulted to `true`
    /// and documented itself as a latency win. The setting was on, the feature
    /// was unreachable, and the two facts never met: the gate is a pure
    /// function of the utterance, so a test with a local mock passed and the
    /// real app took the other path every time.
    ///
    /// `ServerProvider` wraps a `llama-server` child process, which is the
    /// definition the trait documents.
    fn is_local(&self) -> bool {
        true
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat(messages, params)
    }

    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat(messages, params)
    }

    /// P2.7: the local server is the one backend that can grammar-constrain
    /// the decode, so the JSON schema is forwarded to llama-server instead of
    /// being ignored (the trait default).
    fn generate_json(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
        schema: &Value,
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat_inner(messages, params, Some(schema))
    }

    /// BUG-007: kill the child llama-server and wait for it to exit before
    /// the replacement provider is spawned, so at most one ~5 GB server is
    /// resident at any point during a model switch.
    fn stop(&self) {
        self.shutdown();
    }

    /// The local server is spawned with a 4096-token context, so the engine
    /// can preflight long transcripts against a known window.
    fn context_tokens(&self) -> Option<u32> {
        Some(4096)
    }

    fn generate_stream(
        &self,
        prompt: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat_stream(messages, params, on_token)
    }

    fn generate_with_system_stream(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat_stream(messages, params, on_token)
    }

    /// True when the child process is still alive. `is_ready` only records the
    /// warm-up result and never notices a server that crashes afterwards; a
    /// status chip that says "loaded" while the process is dead is the exact
    /// lie that made "S1 generation failed" look impossible.
    fn is_alive(&self) -> bool {
        let Ok(mut guard) = self.runtime.lock() else {
            return false;
        };
        let Some(rt) = guard.as_mut() else {
            return false;
        };
        rt.child.try_wait().map(|s| s.is_none()).unwrap_or(false)
    }
}

impl ServerProvider {
    fn chat(&self, messages: Value, params: GenerationParams) -> Result<String, String> {
        self.chat_inner(messages, params, None)
    }

    /// Shared chat body. `json_schema` (P2.7) is forwarded to llama-server's
    /// grammar-constrained decode when present; without it the server is
    /// free to emit anything.
    fn chat_inner(
        &self,
        messages: Value,
        params: GenerationParams,
        json_schema: Option<&Value>,
    ) -> Result<String, String> {
        let (base_url, api_key) = self.endpoint()?;

        let client = reqwest::blocking::Client::builder()
            .timeout(params.timeout)
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let mut body = json!({
            "model": "local",
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": false,
            // P6.5: stop sequences for the non-S1 polish path. The S1 path
            // uses `generate_with_system` (not `chat_inner`), so this does
            // not touch the S1 contract.
            "stop": ["\n\n\n", "</output>"],
        });
        apply_chat_template_kwargs(&mut body, self.chat_template_kwargs);
        if let Some(schema) = json_schema {
            // llama-server's `json_schema` field takes the schema as a JSON
            // *object*. The string-encoded form (`schema.to_string()`)
            // draws HTTP 400 "schema must be an object" on the bundled
            // server build — caught by the live test in `command_llm`.
            body["json_schema"] = schema.clone();
        }

        let deadline = Instant::now() + params.timeout;
        let mut last_err = String::from("request failed");
        // One retry on 5xx / network error, within the generation budget
        // (same pattern as the OpenAI-compatible provider).
        for attempt in 0..2 {
            if Instant::now() >= deadline {
                break;
            }
            match self.send_chat(&client, &base_url, &api_key, &body) {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let value: Value = resp
                            .json()
                            .map_err(|e| format!("llama-server response decode: {e}"))?;
                        // finish_reason == "length" is a partial rewrite; reject it.
                        if value["choices"][0]["finish_reason"].as_str() == Some("length") {
                            return Err(
                                "llama-server stopped at max_tokens (truncated output)".into()
                            );
                        }
                        return value["choices"][0]["message"]["content"]
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .ok_or_else(|| {
                                "llama-server response missing message content".to_string()
                            });
                    }
                    let retryable = status.is_server_error() || status.as_u16() == 429;
                    let text = resp.text().unwrap_or_default();
                    let text: String = text.chars().take(400).collect();
                    last_err = format!("llama-server HTTP {status}: {text}");
                    if !retryable || attempt == 1 {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = e;
                    if attempt == 1 {
                        return Err(last_err);
                    }
                }
            }
        }
        Err(last_err)
    }

    fn send_chat(
        &self,
        client: &reqwest::blocking::Client,
        base_url: &str,
        api_key: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, String> {
        client
            .post(format!("{base_url}/v1/chat/completions"))
            .bearer_auth(api_key)
            .json(body)
            .send()
            .map_err(|e| format!("llama-server request failed: {e}"))
    }

    /// Streaming variant of [`ServerProvider::chat`]: same request but with
    /// `"stream": true`, and the SSE body's deltas are delivered to `on_token`
    /// as they are parsed.
    ///
    /// True network-level streaming (T1.1b): `reqwest`'s blocking client has
    /// no incremental body reader (`.bytes()` blocks until the whole body
    /// lands), which would deliver every token in one burst at the end. Since
    /// llama-server is a plain HTTP/1.1 server on loopback with no
    /// compression, we open a raw TCP connection, write the request by hand,
    /// then read the body in chunks and parse each SSE frame as it arrives.
    /// `on_token` fires per token while the model is still generating, so the
    /// pill shows the words landing progressively.
    fn chat_stream(
        &self,
        messages: Value,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let (base_url, api_key) = self.endpoint()?;
        let (host, port) = parse_localhost_base_url(&base_url)?;

        let mut body = json!({
            "model": "local",
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": true,
            // P6.5: stop sequences for the non-S1 polish path (see chat_inner).
            "stop": ["\n\n\n", "</output>"],
        });
        apply_chat_template_kwargs(&mut body, self.chat_template_kwargs);
        let payload = body.to_string();

        let deadline = Instant::now() + params.timeout;
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Authorization: Bearer {api_key}\r\n\
             Content-Type: application/json\r\n\
             Accept: text/event-stream\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {payload}",
            payload.len()
        );

        let mut stream = TcpStream::connect((host, port))
            .map_err(|e| format!("llama-server stream connect: {e}"))?;
        // A real read timeout, not `None`.
        //
        // `params.timeout` is a wall-clock deadline, but it was only ever
        // checked *after* `read()` returned. With no read timeout a hung
        // llama-server blocked in `read()` forever and the deadline was never
        // reached, so the timeout that looks like it protects this call did
        // not. A model that is merely slow between tokens is fine: the loop
        // below treats a read timeout as "no data yet", re-arms, and carries on
        // until the real deadline passes. Only a server that stops sending
        // *and* passes the deadline fails.
        stream
            .set_read_timeout(Some(READ_SLICE))
            .map_err(|e| format!("stream read timeout: {e}"))?;
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("llama-server stream write: {e}"))?;

        let (status, _headers) =
            read_status_and_headers(&mut stream).map_err(|e| format!("stream headers: {e}"))?;
        if !status.is_success() {
            let detail = read_to_eof(&mut stream);
            let detail: String = detail.chars().take(400).collect();
            return Err(format!("llama-server stream HTTP {status}: {detail}"));
        }
        if Instant::now() >= deadline {
            return Err("llama-server stream timed out before first token".into());
        }

        let mut full = String::new();
        let mut sse = SseFrameParser::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stream.read(&mut buf) {
                Ok(n) => n,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // No data for READ_SLICE. A model generating a long token
                    // can legitimately be quiet that long, so only give up if
                    // the overall budget is actually spent.
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "llama-server stream stalled: no data for {}s and the \
                             {}s budget is spent (partial output: {} chars)",
                            READ_SLICE.as_secs(),
                            params.timeout.as_secs(),
                            full.chars().count()
                        ));
                    }
                    continue;
                }
                Err(e) => return Err(format!("llama-server stream body read: {e}")),
            };
            if n == 0 {
                // Connection closed (Connection: close). Only a stream that
                // saw `data: [DONE]` is complete; anything else is a server
                // that died mid-response and the partial text must not be
                // returned as a successful rewrite (BUG-014).
                return finish_stream(full, sse.done());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "llama-server stream timed out after {}s (partial output)",
                    params.timeout.as_secs()
                ));
            }
            for delta in sse.push(&buf[..n]) {
                on_token(&delta);
                full.push_str(&delta);
            }
            if sse.done() {
                break;
            }
        }
        Ok(full)
    }
}

/// Post-loop decision for `chat_stream` (BUG-014): a stream that ended
/// without `data: [DONE]` is a server death mid-response, and its partial
/// text must surface as an error, not a successful but truncated rewrite.
fn finish_stream(full: String, done: bool) -> Result<String, String> {
    if done {
        Ok(full)
    } else {
        Err(format!(
            "llama-server stream ended before [DONE] (partial output: {} chars)",
            full.chars().count()
        ))
    }
}

/// Splits `http://127.0.0.1:PORT` (or `http://localhost:PORT`) into the host
/// string and port. The llama-server is always bound to loopback, so anything
/// else is a bug in our own spawn logic.
fn parse_localhost_base_url(base_url: &str) -> Result<(String, u16), String> {
    let without_scheme = base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
        .ok_or_else(|| format!("unsupported base_url scheme: {base_url}"))?;
    let (host, port) = without_scheme
        .rsplit_once(':')
        .ok_or_else(|| format!("base_url missing :port: {base_url}"))?;
    let port: u16 = port
        .parse()
        .map_err(|_| format!("base_url port not numeric: {base_url}"))?;
    let host = host.trim_end_matches('/');
    if host.is_empty() {
        return Err(format!("base_url missing host: {base_url}"));
    }
    Ok((host.to_string(), port))
}

/// Reads from `stream` until the blank line that ends the HTTP response
/// headers, returning the parsed status and the raw header block.
fn read_status_and_headers(
    stream: &mut TcpStream,
) -> Result<(reqwest::StatusCode, String), String> {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .map_err(|e| format!("stream header read: {e}"))?;
        if n == 0 {
            return Err("llama-server closed connection mid-headers".into());
        }
        raw.push(byte[0]);
        if raw.len() >= 4 && raw[raw.len() - 4..] == *b"\r\n\r\n" {
            break;
        }
        if raw.len() > 65536 {
            return Err("llama-server response headers too large".into());
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| "llama-server response missing status line".to_string())?;
    let code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("malformed status line: {status_line}"))?;
    let status = reqwest::StatusCode::from_u16(code.parse().map_err(|_| code.to_string())?)
        .map_err(|_| format!("bad status code: {code}"))?;
    let headers = lines.collect::<Vec<_>>().join("\n");
    Ok((status, headers))
}

/// Reads the remainder of the connection (used to surface an error body).
fn read_to_eof(stream: &mut TcpStream) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > 8192 {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Incremental SSE frame parser. Feed it raw bytes in any chunking; it buffers
/// partial frames and yields complete `data:` payload contents in order,
/// stopping after `data: [DONE]`. Unit-testable without a server.
///
/// Buffers raw **bytes**, not `String`: a multi-byte UTF-8 character split
/// across two socket reads would otherwise be replaced by U+FFFD per half
/// (accents, CJK, Arabic, emoji all corrupt). Frames are split on the byte
/// delimiter `b"\n\n"` and each frame is decoded only once it is complete.
pub struct SseFrameParser {
    pending: Vec<u8>,
    done: bool,
}

impl SseFrameParser {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            done: false,
        }
    }

    /// Appends `chunk` and returns the contents of every complete `data:`
    /// frame it now contains (in order). Empty for frames with no content.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        if self.done {
            return Vec::new();
        }
        self.pending.extend_from_slice(chunk);
        let mut deltas = Vec::new();
        // Frames are separated by a blank line; the last element after the
        // final split may be an incomplete trailing frame, so we keep it.
        let mut frames: Vec<Vec<u8>> = Vec::new();
        let mut start = 0;
        while let Some(rel) = self.pending[start..].windows(2).position(|w| w == b"\n\n") {
            let end = start + rel;
            frames.push(self.pending[start..end].to_vec());
            start = end + 2; // skip the `\n\n` separator
        }
        self.pending.drain(..start);
        for frame in frames {
            // A complete frame is complete UTF-8 (the model's JSON output is
            // valid UTF-8); decode once per frame instead of per read.
            let frame = String::from_utf8_lossy(&frame);
            for line in frame.lines() {
                let line = line.trim();
                let Some(payload) = line.strip_prefix("data: ") else {
                    continue;
                };
                if payload == "[DONE]" {
                    self.done = true;
                    return deltas;
                }
                if let Ok(value) = serde_json::from_str::<Value>(payload) {
                    if let Some(content) = value["choices"][0]["delta"]["content"].as_str() {
                        if !content.is_empty() {
                            deltas.push(content.to_string());
                        }
                    }
                }
            }
        }
        deltas
    }

    fn done(&self) -> bool {
        self.done
    }
}

impl Default for SseFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

/// One-shot convenience over [`SseFrameParser`]: parses a complete SSE body.
///
/// Test-only, and marked as such so it does not read as production code that
/// happens to be unused. The streaming path uses `SseFrameParser::push`
/// directly; nothing else wants a whole-body parse.
#[cfg(test)]
fn parse_sse_deltas(body: &str) -> Vec<String> {
    let mut p = SseFrameParser::new();
    let mut out = p.push(body.as_bytes());
    // A well-formed body ends with a blank line, so one push captures
    // everything; drain any trailing residual frame for robustness.
    out.extend(p.push(b"\n\n"));
    out
}

/// Fuzz harness surface for `cargo fuzz`.
///
/// `cargo fuzz` builds with `--cfg fuzzing`, not `#[cfg(test)]`. This module
/// exposes the minimal byte-level API the fuzz targets need without making
/// `SseFrameParser` itself public.
#[cfg(any(test, fuzzing))]
pub mod fuzz_surface {
    use super::SseFrameParser;

    pub fn parse_sse_once(data: &[u8]) -> Vec<String> {
        let mut p = SseFrameParser::new();
        let mut out = p.push(data);
        out.extend(p.push(b"\n\n"));
        out
    }

    pub fn parse_sse_split(data: &[u8], split_at: usize) -> Vec<String> {
        let split_at = split_at.min(data.len());
        let mut p = SseFrameParser::new();
        let mut out = p.push(&data[..split_at]);
        out.extend(p.push(&data[split_at..]));
        out.extend(p.push(b"\n\n"));
        out
    }
}

fn stop_child(mut child: Child, log_path: &Path, pid_file: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    // The child is gone, so the recorded pid must go with it. Leaving it would
    // make the next launch `ps` a pid the OS may already have recycled.
    let _ = std::fs::remove_file(pid_file);
    tracing::info!(log = %log_path.display(), "llama-server stopped");
}

/// Reap a llama-server we started in a previous run and lost track of.
///
/// The old implementation ran `pgrep -f llama-server` and killed every pid that
/// was not our own process id. `pgrep -f` matches the full argv of *any*
/// process, so a user running their own llama-server, or LM Studio, or
/// text-generation-webui, lost that server the moment they clicked Select on a
/// model here. It also killed a second Teletype instance's server, leaving
/// that instance reporting `is_ready() == true` while every transform silently
/// returned connection-refused.
///
/// Now we only ever signal a pid from `pid_file`, and only after confirming the
/// live process is still a llama-server running *this* model. A recycled pid
/// belonging to something else is left alone.
fn kill_stale_servers(pid_file: &Path) {
    let Ok(raw) = std::fs::read_to_string(pid_file) else {
        return; // Nothing recorded: nothing of ours to reap.
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        let _ = std::fs::remove_file(pid_file);
        return;
    };
    if pid == std::process::id() {
        let _ = std::fs::remove_file(pid_file);
        return;
    }
    // Confirm the pid is still a llama-server before signalling it. Without
    // this, a stale pid file (or a symlinked one) could point at an unrelated
    // process.
    if let Ok(out) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    {
        let cmd = String::from_utf8_lossy(&out.stdout);
        if !(cmd.contains("llama-server") && cmd.contains("-m")) {
            tracing::info!(
                pid,
                "recorded pid is no longer a llama-server; leaving it alone"
            );
            let _ = std::fs::remove_file(pid_file);
            return;
        }
    } else {
        return;
    }
    let _ = bounded_kill(pid);
    tracing::info!(pid, "reaped our previous llama-server");
    let _ = std::fs::remove_file(pid_file);
}

/// True when `ps -o state=` output names a process that is still live.
///
/// The state field is a letter followed by zero or more flags (`Z+` for a
/// zombie in a foreground process group under a pty, `Ss` for a session
/// leader, ...), so only the leading letter is meaningful. Zombie (`Z`) and
/// dead (`X`, BSD only) states count as gone.
#[cfg(unix)]
fn is_live_state(ps_state: &str) -> bool {
    match ps_state.trim().chars().next() {
        None => false,
        Some(c) => !matches!(c, 'Z' | 'z' | 'X' | 'x'),
    }
}

/// Send SIGTERM to `pid` and wait (bounded) for it to exit, escalating to
/// SIGKILL if it does not go. Returns `true` if the process is confirmed gone.
///
/// This is the shared bounded-kill primitive for both the in-process
/// `stop_child` path and the startup reaper. The bounded wait guarantees the
/// caller never blocks indefinitely on a stuck child, and the SIGKILL
/// escalation guarantees a process that ignores SIGTERM is still cleaned up.
///
/// "Gone" means the pid no longer resolves to a *running* process. A zombie
/// (terminated but not yet `wait()`ed) counts as gone — `kill -0` succeeds on
/// a zombie, so we check the process state via `ps` instead of a bare
/// existence probe.
#[cfg(unix)]
fn bounded_kill(pid: u32) -> bool {
    use std::time::{Duration, Instant};

    let signal = |signum: i32| -> bool {
        // Deliver the signal in-process, not by shelling out to `/bin/kill`.
        // A shell-out forks, and a fork that fails under process-table pressure
        // on a loaded runner silently skips delivery (the caller ignores this
        // return value while polling), so a still-running child survived both
        // SIGTERM and SIGKILL and `bounded_kill` burned its whole budget. The
        // startup reaper must not depend on being able to spawn a helper.
        //
        // `pid_t` is signed: a pid above `i32::MAX` would wrap negative, and
        // `kill(-n, ...)` signals a whole process group. Reject instead of
        // casting so a corrupt pid file can never fan out a signal.
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: `kill(2)` only reads the two integer arguments. `pid` is a
        // positive value converted above, so it names a single process, and an
        // invalid or already-exited pid is a harmless `-1`/ESRCH result.
        unsafe { libc::kill(pid, signum) == 0 }
    };

    // "Running" means the pid resolves to a process that is not a zombie and
    // not absent; `ps -o state=` prints nothing for a missing pid. The state
    // parsing lives in `is_live_state`, which handles the `Z+` form that a CI
    // runner under a pty prints for a foreground zombie.
    let running = || -> bool {
        let Ok(out) = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "state="])
            .output()
        else {
            return false;
        };
        let state = String::from_utf8_lossy(&out.stdout);
        is_live_state(&state)
    };

    if !running() {
        return true; // already gone (or never existed)
    }
    let _ = signal(15); // SIGTERM
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Still running after the bounded wait: escalate.
    let _ = signal(9); // SIGKILL
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !running()
}

/// Bounded-kill stub for non-unix targets: signal delivery via /bin/kill is a
/// unix facility, so the reaper is a no-op there (matching the rest of this
/// module's unix-only process handling).
#[cfg(not(unix))]
fn bounded_kill(pid: u32) -> bool {
    let _ = pid;
    true
}

/// Reap a llama-server recorded in `pid_file` from a previous run, if it is
/// still alive. Bounded: SIGTERM + up to 5 s, then SIGKILL, then up to 2 s.
///
/// This is the *startup* hook: unlike `kill_stale_servers` (which only runs
/// inside `warm_up` for the model being loaded), this runs at app launch for
/// the model the settings point at, so an orphan left behind by a SIGTERM
/// that pre-dated the handler, or by a hard kill, is cleaned up even if the
/// user selects a different model this time. Only a pid we recorded ourselves
/// and that is still a llama-server is ever touched.
pub fn reap_orphan(pid_file: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(pid_file) else {
        return false; // nothing recorded
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        let _ = std::fs::remove_file(pid_file);
        return false;
    };
    if pid == std::process::id() {
        let _ = std::fs::remove_file(pid_file);
        return false;
    }
    // Confirm the pid is still a llama-server before signalling it (a recycled
    // pid must never be killed).
    if let Ok(out) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    {
        let cmd = String::from_utf8_lossy(&out.stdout);
        if !(cmd.contains("llama-server") && cmd.contains("-m")) {
            let _ = std::fs::remove_file(pid_file);
            return false;
        }
    } else {
        return false;
    }
    let reaped = bounded_kill(pid);
    tracing::info!(pid, reaped, "startup reaper: reaped previous llama-server");
    let _ = std::fs::remove_file(pid_file);
    reaped
}

fn wait_until_ready(
    child: &mut Child,
    base_url: &str,
    api_key: &str,
    log_path: &Path,
    bin: &Path,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(HEALTH_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A 0-byte log means the process died before it wrote anything,
                // which on macOS is a specific and very confusing failure: a
                // file whose contents were replaced while a process was
                // executing it is killed by the kernel with SIGKILL and the
                // reason "Code Signature Invalid". The binary looks perfect to
                // `file`, passes an executable-bit check, and is unrunnable.
                // That happened when the build script `cp`'d over a
                // llama-server that a dev loop had left running, and the user
                // saw only "Polish skipped: no model loaded", which names
                // neither the cause nor the fix.
                let logged = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
                if logged == 0 {
                    return Err(format!(
                        "llama-server was killed before it started ({status}) and wrote \
                         nothing to its log. On macOS this is almost always a binary whose \
                         contents were overwritten while another llama-server was running \
                         from it: the file then cannot be executed (SIGKILL, Code Signature \
                         Invalid) even though `file` reports a valid Mach-O. Fix with \
                         `codesign --force --sign - {bin}` and check for a stale server with \
                         `pgrep -fl llama-server`.",
                        status = status,
                        bin = bin.display(),
                    ));
                }
                return Err(format!(
                    "llama-server exited early with {status} (log: {})",
                    log_path.display()
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("wait on llama-server: {e}")),
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "llama-server not ready after {}s (log: {})",
                READY_TIMEOUT.as_secs(),
                log_path.display()
            ));
        }

        let ready = client
            .get(format!("{base_url}/health"))
            .bearer_auth(api_key)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ready {
            return Ok(());
        }
        thread::sleep(HEALTH_INTERVAL);
    }
}

/// Resolve the `llama-server` binary.
///
/// Order: `TELETYPE_LLAMA_SERVER` → next to the current exe →
/// macOS bundle `../Resources/llama-server` → `PATH`.
pub fn find_llama_server_bin() -> Result<PathBuf, String> {
    if let Ok(p) = env::var("TELETYPE_LLAMA_SERVER") {
        let p = PathBuf::from(p);
        if is_runnable_binary(&p) {
            return Ok(p);
        }
        return Err(format!(
            "TELETYPE_LLAMA_SERVER points at a missing, empty, or \
             non-executable file: {}",
            p.display()
        ));
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("llama-server"),
                dir.join("llama-server.exe"),
                dir.join("..").join("Resources").join("llama-server"),
                dir.join("..").join("Resources").join("llama-server.exe"),
            ];
            for c in candidates {
                if is_runnable_binary(&c) {
                    return Ok(c);
                }
            }
        }
    }

    if let Some(p) = find_in_path("llama-server").or_else(|| find_in_path("llama-server.exe")) {
        return Ok(p);
    }

    Err(
        "llama-server binary not found. Set TELETYPE_LLAMA_SERVER or run \
         scripts/build-llama-server.sh"
            .to_string(),
    )
}

/// A candidate only counts if it could actually be executed.
///
/// The bundle declares `resources.binaries/llama-server`, and `cargo tauri
/// build` requires that path to exist, so a 0-byte placeholder often sits
/// next to the dev exe. Spawning it fails with `EACCES` (os error 13), which
/// surfaces to the user as a baffling "Permission denied" instead of the
/// actionable "run scripts/build-llama-server.sh". Skipping such a stub here
/// lets lookup fall through to PATH and the clear not-found error.
fn is_runnable_binary(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(md) => md.is_file() && md.len() > 0 && has_exec_bit(&md),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn has_exec_bit(md: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn has_exec_bit(_md: &std::fs::Metadata) -> bool {
    true
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|c| is_runnable_binary(c))
}

fn pick_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    // Drop the listener so llama-server can bind it (small race, acceptable
    // for a single local process).
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P5.1 regression guard.
    ///
    /// `polish_gate_enabled` defaults to `true` and `Pipeline` only consults
    /// the gate when `inference.is_local()`. When this method was absent, the
    /// trait default `false` applied, the gate was unreachable in the shipped
    /// app, and no test failed: the pipeline tests all use a local mock, so
    /// they exercised the branch the real provider never took.
    ///
    /// Asserted on the trait method rather than through the pipeline, so a
    /// failure names the actual cause instead of "the gate did not skip".
    #[test]
    fn the_local_server_provider_reports_itself_local() {
        let p = ServerProvider::new(
            "qwen3-0.6b",
            "Qwen3 0.6B",
            Path::new("/nonexistent/model.gguf"),
        );
        assert!(
            InferenceProvider::is_local(&p),
            "ServerProvider wraps a llama-server child process; if it does not \
             report is_local(), the polish gate can never fire in the real app"
        );
    }

    /// The counterpart. A remote provider must NOT claim to be local, or the
    /// gate would start skipping polish on network calls where the latency
    /// argument does not hold.
    #[test]
    fn the_openai_compatible_provider_does_not_claim_to_be_local() {
        let p = crate::openai_compat::OpenAiCompatProvider::new(
            "openai",
            "OpenAI",
            crate::openai_compat::OpenAiCompatConfig::new(
                "https://api.openai.com/v1",
                Some("test".to_string()),
                "gpt-4o-mini",
            ),
        );
        assert!(
            !InferenceProvider::is_local(&p),
            "a remote API provider is not local; claiming otherwise makes the \
             gate skip polish based on a latency argument that does not apply"
        );
    }

    /// Regression: S1-mini is a Qwen3 fine-tune trained with thinking mode
    /// OFF. Without `enable_thinking: false` the embedded template defaults to
    /// thinking ON, the model emits an empty think block and stops, and every
    /// polish fails with empty content ("S1 generation failed"). The kwargs
    /// must be applied at spawn and on every request.
    #[test]
    fn s1_mini_gets_enable_thinking_false_other_models_get_none() {
        assert_eq!(
            default_chat_template_kwargs("s1-mini"),
            r#"{"enable_thinking":false}"#
        );
        for other in ["qwen2.5-3b", "fast", "quality"] {
            assert_eq!(
                default_chat_template_kwargs(other),
                r#"{}"#,
                "{other} must not carry S1's thinking kwargs"
            );
        }

        let mut body = json!({ "messages": [] });
        apply_chat_template_kwargs(&mut body, default_chat_template_kwargs("s1-mini"));
        assert_eq!(
            body["chat_template_kwargs"]["enable_thinking"],
            Value::Bool(false)
        );

        let mut body = json!({ "messages": [] });
        apply_chat_template_kwargs(&mut body, default_chat_template_kwargs("fast"));
        assert!(
            body.get("chat_template_kwargs").is_none(),
            "an empty kwargs object is a no-op and must not be sent"
        );
    }

    #[test]
    fn find_llama_server_error_mentions_env_override() {
        // When the binary is absent from PATH/exe dir, the error must tell
        // the user how to point at one. We only assert the message shape when
        // lookup fails; a successful lookup (dev machine with the binary) is
        // also fine.
        match find_llama_server_bin() {
            Ok(p) => assert!(!p.as_os_str().is_empty()),
            Err(e) => assert!(e.contains("TELETYPE_LLAMA_SERVER")),
        }
    }

    #[test]
    fn pick_port_is_localhost_ephemeral() {
        let port = pick_port().unwrap();
        assert!(port > 0);
        // Port must be bindable again after pick_port drops the listener.
        // Retry a few times to absorb TIME_WAIT from concurrent test threads.
        let mut ok = false;
        for _ in 0..5 {
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(ok, "port {port} not bindable after 5 retries");
    }

    #[test]
    fn placeholder_stubs_are_not_runnable() {
        let dir = std::env::temp_dir().join(format!("teletype-llama-stub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty-llama-server");
        std::fs::write(&empty, b"").unwrap();
        assert!(
            !is_runnable_binary(&empty),
            "0-byte placeholder must be skipped"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let non_exec = dir.join("non-exec-llama-server");
            std::fs::write(&non_exec, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&non_exec, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(
                !is_runnable_binary(&non_exec),
                "non-executable file must be skipped"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_kills_the_spawned_child() {
        // Prove the process-lifecycle fix directly: a ServerProvider that has
        // spawned a child must kill it on shutdown(), so a quit never orphans
        // a model process. Uses a tiny compiled C "llama-server" stub that
        // answers /health with 200 and idles, so warm_up succeeds without a
        // real model.
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("teletype-llama-shut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Compile a minimal HTTP stub: read the port from the --port arg,
        // answer every request with 200 "ok", and block forever.
        let stub_src = dir.join("stub.c");
        std::fs::write(
            &stub_src,
            r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
int main(int argc, char **argv) {
    int port = 0;
    for (int i = 1; i < argc - 1; i++)
        if (!strcmp(argv[i], "--port")) { port = atoi(argv[i+1]); break; }
    int s = socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in a; memset(&a, 0, sizeof a);
    a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    a.sin_port = htons(port);
    if (bind(s, (struct sockaddr*)&a, sizeof a) < 0) return 1;
    listen(s, 16);
    for (;;) {
        int c = accept(s, 0, 0);
        if (c < 0) continue;
        char buf[1024]; (void)!read(c, buf, sizeof buf);
        const char *r = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        (void)!write(c, r, strlen(r));
        close(c);
    }
}
"#,
        )
        .unwrap();
        let stub = dir.join("llama-server");
        let compiled = Command::new("cc")
            .arg("-o")
            .arg(&stub)
            .arg(&stub_src)
            .output();
        assert!(
            compiled.is_ok() && compiled.as_ref().unwrap().status.success(),
            "cc stub failed: {:?}",
            compiled
                .as_ref()
                .map(|c| String::from_utf8_lossy(&c.stderr).into_owned())
        );
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Model file just needs to exist.
        let model = dir.join("model.gguf");
        std::fs::write(&model, b"stub").unwrap();

        let provider = ServerProvider::new("stub", "Stub", &model);
        std::env::set_var("TELETYPE_LLAMA_SERVER", &stub);
        let result = provider.warm_up();
        std::env::remove_var("TELETYPE_LLAMA_SERVER");

        assert!(result.is_ok(), "stub warm_up failed: {result:?}");
        assert!(provider.is_ready());

        // Grab the child pid before shutdown so we can confirm it died.
        let pid = {
            let g = provider.runtime.lock().unwrap();
            g.as_ref().unwrap().child.id()
        };

        provider.shutdown();

        // After shutdown the child must be gone and the provider not ready.
        assert!(!provider.is_ready());
        let still_alive = Command::new("kill").arg("-0").arg(pid.to_string()).status();
        // kill -0 fails (non-zero) when the process no longer exists.
        assert!(
            still_alive.is_err() || !still_alive.unwrap().success(),
            "child {pid} still alive after shutdown"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_sse_deltas_extracts_content_in_order_and_stops_at_done() {
        let body = [
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\",\"}}]}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}],\"finish_reason\":null}",
            "",
            "data: [DONE]",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"never delivered\"}}]}",
        ]
        .join("\n");
        assert_eq!(parse_sse_deltas(&body), vec!["Hello", ",", " world"]);
    }

    #[test]
    fn parse_sse_deltas_ignores_non_data_lines_and_bad_json() {
        let body = [
            ": keep-alive comment",
            "event: message",
            "data: not-json",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}",
        ]
        .join("\n");
        assert_eq!(parse_sse_deltas(&body), vec!["ok"]);
    }

    #[test]
    fn parse_sse_deltas_empty_body() {
        assert!(parse_sse_deltas("").is_empty());
    }

    /// Builds a well-formed SSE body whose `data:` frames carry multilingual
    /// content (accented Latin, CJK, Arabic, emoji). Returned as bytes.
    fn multilingual_sse_body() -> Vec<u8> {
        let contents = [
            "caf\u{e9} na\u{ef}ve",             // accented Latin (2-byte codepoints)
            "\u{4f60}\u{597d}\u{4e16}\u{5b89}", // CJK (3-byte)
            "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627}", // Arabic (2-byte)
            "\u{1f30d}\u{1f680}\u{2764}\u{fe0f}", // emoji incl. 4-byte + variation selector
        ];
        // Content is pure text with no JSON-special characters, so no escaping
        // is needed in the `data:` payload.
        let body = contents
            .iter()
            .map(|c| format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{c}\"}}}}]}}\n\n"))
            .collect::<Vec<_>>()
            .concat();
        body.into_bytes()
    }

    #[test]
    fn sse_frame_parser_survives_mid_codepoint_chunk_splits() {
        // A 3-byte CJK character split across two reads must not become
        // two U+FFFD. Regression test for the old per-read
        // `String::from_utf8_lossy` behavior.
        let body = multilingual_sse_body();
        // Find the byte offset of the 3-byte CJK char \u{4f60} (0xE4 0xBD 0xA0).
        let cjk = "\u{4f60}".as_bytes();
        let off = body
            .windows(3)
            .position(|w| w == cjk)
            .expect("CJK byte sequence present in body");
        // Split inside the codepoint: first chunk ends mid-character.
        let mut p = SseFrameParser::new();
        let mut got = p.push(&body[..off + 1]);
        got.extend(p.push(&body[off + 1..]));
        let reference: Vec<String> = [
            "caf\u{e9} na\u{ef}ve",
            "\u{4f60}\u{597d}\u{4e16}\u{5b89}",
            "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627}",
            "\u{1f30d}\u{1f680}\u{2764}\u{fe0f}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(got, reference, "mid-codepoint split corrupted the stream");
    }

    // Chunk sizes 1..=64: 1 forces every boundary to fall mid-codepoint
    // (a 4-byte emoji spans four chunks), small sizes hit the common
    // 8 KB-read alignment, large sizes stay well below one frame.
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn sse_frame_parser_chunked_matches_unfragmented(chunk_size in 1usize..=64) {
            let body = multilingual_sse_body();
            let mut p = SseFrameParser::new();
            let mut got = Vec::new();
            let mut i = 0;
            while i < body.len() {
                let end = (i + chunk_size).min(body.len());
                got.extend(p.push(&body[i..end]));
                i = end;
            }
            let reference: Vec<String> = [
                "caf\u{e9} na\u{ef}ve",
                "\u{4f60}\u{597d}\u{4e16}\u{5b89}",
                "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627}",
                "\u{1f30d}\u{1f680}\u{2764}\u{fe0f}",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            prop_assert_eq!(got, reference);
        }
    }

    #[test]
    fn sse_frame_parser_streams_across_chunk_boundaries() {
        // Simulate the server sending one frame per network chunk, with a
        // frame split across two chunks.
        let mut p = SseFrameParser::new();
        let frame1 = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n";
        let frame2_full = b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n";
        let done = b"data: [DONE]\n\n";

        assert_eq!(p.push(frame1), vec!["Hel"]);
        // Split frame2 across two chunks.
        let mid = frame2_full.len() / 2;
        assert!(
            p.push(&frame2_full[..mid]).is_empty(),
            "partial frame must not fire"
        );
        assert_eq!(p.push(&frame2_full[mid..]), vec!["lo"]);
        assert!(!p.done(), "not done until [DONE]");
        assert_eq!(
            p.push(done),
            Vec::<String>::new(),
            "[DONE] frame has no content"
        );
        assert!(p.done(), "done after [DONE]");
        assert_eq!(
            p.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n"),
            Vec::<String>::new(),
            "nothing after [DONE]"
        );
    }

    #[test]
    fn sse_frame_parser_survives_utf8_split_across_reads() {
        // B1 regression: a multi-byte character (here the em dash, 3 bytes)
        // split across two socket reads must round-trip intact, not become
        // U+FFFD. The old implementation decoded each 8 KB chunk lossily.
        let frame =
            "data: {\"choices\":[{\"delta\":{\"content\":\"caf\u{2014}\u{65e5}\u{672c}\"}}]}
\n";
        let bytes = frame.as_bytes();
        // Split in the middle of the first multi-byte character.
        let split = bytes.iter().position(|b| *b == 0xE2).unwrap() + 1;
        let mut p = SseFrameParser::new();
        assert!(
            p.push(&bytes[..split]).is_empty(),
            "partial frame must not fire"
        );
        let out = p.push(&bytes[split..]);
        assert_eq!(out, vec!["caf\u{2014}\u{65e5}\u{672c}"]);
    }

    #[test]
    fn sse_frame_parser_survives_arbitrary_chunking() {
        // Every byte-by-byte feed of the same body must give the same output
        // as one big feed.
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"H\u{65e5}\"}}]}
\ndata: {\"choices\":[{\"delta\":{\"content\":\"ello\"}}]}
\ndata: [DONE]
\n";
        let mut p = SseFrameParser::new();
        assert_eq!(p.push(body.as_bytes()), vec!["H\u{65e5}", "ello"]);
        assert!(p.done());

        let mut p = SseFrameParser::new();
        let mut out: Vec<String> = Vec::new();
        for b in body.as_bytes() {
            out.extend(p.push(&[*b]));
        }
        assert_eq!(out, vec!["H\u{65e5}", "ello"]);
        assert!(p.done());
    }

    #[test]
    fn finish_stream_rejects_eof_before_done() {
        let err = finish_stream("Hel".into(), false).unwrap_err();
        assert!(
            err.contains("[DONE]"),
            "the error must name the missing [DONE] marker, got: {err}"
        );
        assert!(
            err.contains("3 chars"),
            "the error must report the partial length, got: {err}"
        );
    }

    #[test]
    fn finish_stream_accepts_done() {
        assert_eq!(finish_stream("Hello".into(), true).unwrap(), "Hello");
    }

    /// BUG-014: a server that sends one complete `data:` frame and then closes
    /// the socket without `data: [DONE]` must produce an `Err` naming the
    /// truncation, not an `Ok` carrying the partial text. Drives the real
    /// `chat_stream` against a real `TcpListener`.
    #[test]
    fn chat_stream_eof_before_done_is_an_error() {
        use std::io::Write as _;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                // Drain the request (the client connects, writes it, then
                // reads) so the server-side close is a clean FIN, not a RST
                // from an unread request hitting the socket buffer.
                let mut req = [0u8; 8192];
                let _ = sock.read(&mut req);
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\n");
                let _ = sock.write_all(b"Content-Type: text/event-stream\r\n");
                let _ = sock.write_all(b"Connection: close\r\n\r\n");
                let _ = sock.write_all(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
                );
                // Close WITHOUT `data: [DONE]`.
            }
        });

        let provider = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        provider.runtime.lock().unwrap().replace(Runtime {
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: "test".into(),
            child: Command::new("/usr/bin/true").spawn().unwrap(),
            log_path: PathBuf::from("/dev/null"),
        });

        let mut seen = String::new();
        let result = provider.chat_stream(
            json!([{ "role": "user", "content": "hi" }]),
            GenerationParams::default(),
            &mut |tok| seen.push_str(tok),
        );
        let err = result
            .err()
            .unwrap_or_else(|| panic!("expected Err, got Ok({seen:?})"));
        assert!(
            err.contains("[DONE]"),
            "the error must name the missing [DONE] marker, got: {err}"
        );
        assert!(
            err.contains("3 chars"),
            "the error must report the partial length, got: {err}"
        );
        assert_eq!(seen, "Hel");
    }

    /// BUG-014 counterpart: the same frames plus `data: [DONE]` must be
    /// delivered as `Ok` with the concatenated text.
    #[test]
    fn chat_stream_done_before_eof_is_ok() {
        use std::io::Write as _;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                // Drain the request first so the close is a clean FIN (see the
                // sibling EOF test for the RST rationale).
                let mut req = [0u8; 8192];
                let _ = sock.read(&mut req);
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\n");
                let _ = sock.write_all(b"Content-Type: text/event-stream\r\n");
                let _ = sock.write_all(b"Connection: close\r\n\r\n");
                let _ = sock.write_all(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
                );
                let _ = sock.write_all(b"data: [DONE]\n\n");
            }
        });

        let provider = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        provider.runtime.lock().unwrap().replace(Runtime {
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: "test".into(),
            child: Command::new("/usr/bin/true").spawn().unwrap(),
            log_path: PathBuf::from("/dev/null"),
        });

        let mut seen = String::new();
        let full = provider
            .chat_stream(
                json!([{ "role": "user", "content": "hi" }]),
                GenerationParams::default(),
                &mut |tok| seen.push_str(tok),
            )
            .unwrap();
        assert_eq!(full, "Hel");
        assert_eq!(seen, "Hel");
    }

    #[test]
    fn generate_without_warm_up_errors() {
        let p = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        let err = p.generate("hi", GenerationParams::default()).unwrap_err();
        assert!(err.contains("not running"), "got: {err}");
    }

    #[test]
    fn warm_up_missing_model_errors() {
        let p = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        let err = p.warm_up().unwrap_err();
        assert!(
            err.contains("not found") || err.contains("llama-server"),
            "got: {err}"
        );
    }

    /// A child that exits without writing to its log has to produce the
    /// actionable message, not the bare exit status.
    ///
    /// This is the failure that made the polish button look broken: the build
    /// script overwrote a `llama-server` that a dev loop had left running, macOS
    /// killed every subsequent exec of that file with SIGKILL, the server log
    /// stayed 0 bytes, and the user was told "Polish skipped: no model loaded"
    /// with nothing pointing at the real cause.
    /// A server that accepts the connection and then never sends anything must
    /// not hang forever.
    ///
    /// This is the case the old `set_read_timeout(None)` could not handle. The
    /// `params.timeout` deadline looked like it protected this call, but it was
    /// only ever checked *after* `read()` returned, and with no read timeout a
    /// silent server meant `read()` never returned. The symptom is the worst one
    /// available: dictation shows "Transforming…" forever, with no error and no
    /// way out short of quitting the app.
    #[test]
    fn a_silent_server_fails_instead_of_hanging() {
        use std::io::Write as _;
        use std::net::TcpListener;

        // Accept one connection and then never write a byte.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                // Hold the socket open without responding, so the client sees a
                // live connection that never delivers.
                std::thread::sleep(std::time::Duration::from_secs(30));
                let _ = sock.write_all(b"");
            }
        });

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        // The shape of the read loop, with a short budget so the test is quick.
        let deadline = Instant::now() + Duration::from_millis(700);
        let mut buf = [0u8; 1024];
        let mut polls = 0;
        let mut errored = None;
        loop {
            polls += 1;
            match stream.read(&mut buf) {
                Ok(0) => break, // closed
                Ok(_) => continue,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    if Instant::now() >= deadline {
                        errored = Some(format!("stalled after {} polls with {} bytes", polls, 0));
                        break;
                    }
                    continue;
                }
                Err(e) => {
                    errored = Some(e.to_string());
                    break;
                }
            }
        }
        assert!(
            errored.is_some(),
            "a silent server must produce an error, not an infinite loop"
        );
        assert!(
            polls > 1,
            "the loop must re-arm the read rather than giving up on the first \
             timeout, or a slow model would look like a dead one (polls={polls})"
        );
    }

    #[test]
    fn a_server_killed_before_it_starts_explains_itself() {
        // A command that exits non-zero immediately without writing anything.
        let mut child = Command::new("/usr/bin/false").spawn().unwrap();
        let dir = std::env::temp_dir().join(format!("teletype-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("empty.log");
        std::fs::write(&log, b"").unwrap();

        let err = wait_until_ready(
            &mut child,
            "http://127.0.0.1:1",
            "k",
            &log,
            Path::new("/usr/bin/false"),
        )
        .unwrap_err();

        assert!(
            err.contains("Code Signature Invalid"),
            "must name the macOS cause, got: {err}"
        );
        assert!(
            err.contains("codesign --force --sign -"),
            "must give the fix, got: {err}"
        );
        assert!(
            err.contains("pgrep -fl llama-server"),
            "must say how to find a stale server, got: {err}"
        );
        assert!(
            !err.contains("exited early with"),
            "the bare status is what misled everyone before, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same failure with a real log is still a normal "exited early".
    ///
    /// The diagnostic must only fire on a 0-byte log, or every genuine
    /// crash-then-report would be dressed up as a signature problem.
    #[test]
    fn a_server_that_logged_something_keeps_the_ordinary_message() {
        let mut child = Command::new("/usr/bin/false").spawn().unwrap();
        let dir = std::env::temp_dir().join(format!("teletype-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("full.log");
        std::fs::write(&log, b"error: failed to load model\n").unwrap();

        let err = wait_until_ready(
            &mut child,
            "http://127.0.0.1:1",
            "k",
            &log,
            Path::new("/usr/bin/false"),
        )
        .unwrap_err();

        assert!(err.contains("exited early with"), "got: {err}");
        assert!(!err.contains("Code Signature Invalid"), "got: {err}");
        assert!(
            err.contains("full.log"),
            "must point at the log, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    mod proptest_properties {
        use super::fuzz_surface::{parse_sse_once, parse_sse_split};
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(256))]
            #[test]
            fn sse_arbitrary_chunk_split_identical(bytes in prop::collection::vec(any::<u8>(), 0..1024), split in 0usize..1024) {
                let once = parse_sse_once(&bytes);
                let split_out = parse_sse_split(&bytes, split);
                prop_assert_eq!(once, split_out, "SSE must give identical output regardless of chunk boundary");
            }

            #[test]
            fn sse_never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
                let _ = parse_sse_once(&bytes);
                let _ = parse_sse_split(&bytes, bytes.len() / 2);
            }

            #[test]
            fn sse_valid_frames_preserve_payload(payload in "[a-zA-Z0-9 ]{0,40}") {
                let body = format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{payload}\"}}}}]}}\n\n");
                let out = parse_sse_once(body.as_bytes());
                if payload.is_empty() {
                    prop_assert!(out.is_empty());
                } else {
                    prop_assert_eq!(out, vec![payload]);
                }
            }
        }
    }

    // --- t7-sigterm regression: bounded_kill + reap_orphan ---

    /// t7-sigterm regression: `ps -o state=` appends flags to the state letter
    /// (`Z+` under a pty, `Ss` for a session leader). Only the leading letter
    /// decides liveness, and zombie/dead states must read as gone — the old
    /// exact `== "Z"` check treated `Z+` as alive and burned the kill budget.
    #[cfg(unix)]
    #[test]
    fn ps_state_parsing_counts_zombies_and_dead_as_gone() {
        for gone in ["", " ", "Z", "Z+", "z", "X", "X+"] {
            assert!(!is_live_state(gone), "{gone:?} must count as gone");
        }
        for live in ["R", "R+", "S", "S+", "Ss", "D", "I"] {
            assert!(is_live_state(live), "{live:?} must count as live");
        }
    }

    /// t7-sigterm regression: `bounded_kill` must SIGTERM a live process and
    /// confirm it is gone within the bounded wait (no indefinite block, no
    /// leak). We spawn a `sleep 30` as a stand-in for a stuck llama-server
    /// (SIGTERM kills it cleanly because `sleep` does not trap the signal).
    #[cfg(unix)]
    #[test]
    fn bounded_kill_terminates_a_live_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let reaped = bounded_kill(pid);
        assert!(reaped, "bounded_kill should confirm the process is gone");
        let _ = child.wait(); // reap the zombie (already dead)
    }

    /// t7-sigterm regression: `bounded_kill` must return `true` immediately
    /// when the pid is already gone (no signal sent, no wait loop).
    #[cfg(unix)]
    #[test]
    fn bounded_kill_returns_true_for_dead_pid() {
        // Spawn and kill a process, wait on it, then confirm bounded_kill sees
        // it as already gone.
        let mut child = std::process::Command::new("sleep")
            .arg("1")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let _ = Command::new("kill").arg(pid.to_string()).status();
        let _ = child.wait(); // reap
                              // Give the kernel a moment to fully clean up the pid.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let gone = bounded_kill(pid);
        assert!(
            gone,
            "bounded_kill on a dead pid should return true immediately"
        );
    }

    /// t7-sigterm regression: `reap_orphan` must kill a live llama-server
    /// recorded in a pid file and remove the file. We cannot spawn a real
    /// `llama-server` in a unit test (no model), so we stand in with a process
    /// whose command line contains both `llama-server` and `-m` — the exact
    /// markers `reap_orphan` checks. A `sleep 30` with a matching arg does the
    /// job: we wrap it so `ps -o command=` shows the markers.
    #[cfg(unix)]
    #[test]
    fn reap_orphan_kills_a_matching_orphan_and_removes_pid_file() {
        let tmp = std::env::temp_dir().join(format!("teletype-reap-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let pid_file = tmp.join("orphan.llama-server.pid");

        // Spawn a process whose command line contains both "llama-server" and
        // "-m" so the `ps` check in `reap_orphan` matches. A `sh -c` command
        // whose body mentions both markers: `ps -o command=` on the shell shows
        // the full `sh -c '...'` string, which contains both substrings. The
        // shell sleeps so the process stays alive long enough to be reaped.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "llama-server -m dummy; sleep 30"])
            .spawn()
            .expect("spawn fake llama-server");
        let pid = child.id();
        std::fs::write(&pid_file, pid.to_string()).expect("write pid file");

        let reaped = reap_orphan(&pid_file);
        assert!(reaped, "reap_orphan should reap the live orphan");
        assert!(
            !pid_file.exists(),
            "pid file should be removed after reaping"
        );
        let _ = child.wait(); // already dead
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// t7-sigterm regression: `reap_orphan` must NOT kill a pid that is not
    /// a llama-server (a recycled pid). It should remove the stale pid file
    /// and return `false`.
    #[cfg(unix)]
    #[test]
    fn reap_orphan_leaves_a_non_llama_server_pid_alone() {
        let tmp =
            std::env::temp_dir().join(format!("teletype-reap-test-nollama-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let pid_file = tmp.join("stale.llama-server.pid");

        // Spawn a `sleep` whose command line does NOT contain "llama-server"
        // or "-m".
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        std::fs::write(&pid_file, pid.to_string()).expect("write pid file");

        let reaped = reap_orphan(&pid_file);
        assert!(!reaped, "reap_orphan must not reap a non-llama-server pid");
        assert!(!pid_file.exists(), "stale pid file should be removed");
        // The process must still be alive.
        let still_alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(still_alive, "the process must still be running");
        let _ = Command::new("kill").arg(pid.to_string()).status();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// t7-sigterm regression: `reap_orphan` on a missing pid file is a no-op
    /// returning `false` (nothing recorded, nothing to reap).
    #[test]
    fn reap_orphan_missing_pid_file_is_noop() {
        let tmp =
            std::env::temp_dir().join(format!("teletype-reap-test-nofile-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let pid_file = tmp.join("absent.llama-server.pid");
        assert!(!pid_file.exists());
        let reaped = reap_orphan(&pid_file);
        assert!(!reaped, "no pid file => false");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// t7-sigterm regression: `reap_orphan` on a pid file containing our own
    /// pid must remove the file and return `false` (never kill ourselves).
    #[test]
    fn reap_orphan_own_pid_removes_file_and_returns_false() {
        let tmp =
            std::env::temp_dir().join(format!("teletype-reap-test-own-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let pid_file = tmp.join("own.llama-server.pid");
        std::fs::write(&pid_file, std::process::id().to_string()).expect("write own pid");
        let reaped = reap_orphan(&pid_file);
        assert!(!reaped, "own pid => false");
        assert!(!pid_file.exists(), "pid file should be removed");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
