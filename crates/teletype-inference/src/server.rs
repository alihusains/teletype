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
/// safe upper bound: large multi-shard models like EG-1 (8 shards, ~2.7 GB)
/// can exceed 60s to load on a cold disk cache, and a shorter timeout kills
/// the child mid-load with SIGKILL, surfacing as a spurious download failure.
/// Warm-cache loads finish in ~2s, so this only matters for cold starts.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_INTERVAL: Duration = Duration::from_millis(250);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// A local model served by a child `llama-server` process.
pub struct ServerProvider {
    model_id: String,
    model_name: String,
    path: PathBuf,
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
        Self {
            model_id: id.into(),
            model_name: name.into(),
            path: path.to_path_buf(),
            runtime: Mutex::new(None),
        }
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

        let bin = find_llama_server()?;
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
                "-c", "4096", "-fa", "on", "-ctk", "q8_0", "-ctv", "q8_0", "--jinja",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

        let base_url = format!("http://127.0.0.1:{port}");
        if let Err(e) = wait_until_ready(&mut child, &base_url, &api_key, &log_path) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

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
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(rt) = guard.take() {
                stop_child(rt.child, &rt.log_path);
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
}

impl ServerProvider {
    fn chat(&self, messages: Value, params: GenerationParams) -> Result<String, String> {
        let (base_url, api_key) = self.endpoint()?;

        let client = reqwest::blocking::Client::builder()
            .timeout(params.timeout)
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let body = json!({
            "model": "local",
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": false,
        });

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

        let body = json!({
            "model": "local",
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": true,
        });
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
        // No client-level read timeout: `params.timeout` is enforced as a
        // wall-clock deadline checked between tokens instead.
        stream
            .set_read_timeout(None)
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
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("llama-server stream body read: {e}"))?;
            if n == 0 {
                break; // connection closed (Connection: close)
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
struct SseFrameParser {
    pending: String,
    done: bool,
}

impl SseFrameParser {
    fn new() -> Self {
        Self {
            pending: String::new(),
            done: false,
        }
    }

    /// Appends `chunk` and returns the contents of every complete `data:`
    /// frame it now contains (in order). Empty for frames with no content.
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        if self.done {
            return Vec::new();
        }
        self.pending.push_str(&String::from_utf8_lossy(chunk));
        let mut deltas = Vec::new();
        // Frames are separated by a blank line; the last element after the
        // final split may be an incomplete trailing frame, so we keep it.
        let parts: Vec<String> =
            self.pending.split("\n\n").map(|s| s.to_string()).collect();
        let trailing = parts.last().cloned().unwrap_or_default();
        let complete = parts.len().saturating_sub(1);
        self.pending = trailing;
        for frame in &parts[..complete] {
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

/// One-shot convenience over [`SseFrameParser`]: parses a complete SSE body.
/// Kept for the existing tests and as a reference for the frame format.
fn parse_sse_deltas(body: &str) -> Vec<String> {
    let mut p = SseFrameParser::new();
    let mut out = p.push(body.as_bytes());
    // A well-formed body ends with a blank line, so one push captures
    // everything; drain any trailing residual frame for robustness.
    out.extend(p.push(b"\n\n"));
    out
}

fn stop_child(mut child: Child, log_path: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    tracing::info!(log = %log_path.display(), "llama-server stopped");
}

fn wait_until_ready(
    child: &mut Child,
    base_url: &str,
    api_key: &str,
    log_path: &Path,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(HEALTH_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
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
fn find_llama_server() -> Result<PathBuf, String> {
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

    #[test]
    fn find_llama_server_error_mentions_env_override() {
        // When the binary is absent from PATH/exe dir, the error must tell
        // the user how to point at one. We only assert the message shape when
        // lookup fails; a successful lookup (dev machine with the binary) is
        // also fine.
        match find_llama_server() {
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
        assert!(p.push(&frame2_full[..mid]).is_empty(), "partial frame must not fire");
        assert_eq!(p.push(&frame2_full[mid..]), vec!["lo"]);
        assert!(!p.done(), "not done until [DONE]");
        assert_eq!(p.push(done), Vec::<String>::new(), "[DONE] frame has no content");
        assert!(p.done(), "done after [DONE]");
        assert_eq!(p.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n"), Vec::<String>::new(), "nothing after [DONE]");
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
}
