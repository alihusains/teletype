//! Qwen3-ASR speech provider: a child `llama-server` process served over its
//! OpenAI-compatible `POST /v1/audio/transcriptions` endpoint.
//!
//! The model is a TWO-FILE artifact: the main GGUF (the "language brain") and
//! a small `mmproj-*.gguf` (audio projector). Both must sit in one directory;
//! [`Qwen3AsrProvider::load`] takes that directory. Process management
//! (binary lookup, ephemeral port, health poll, pid-file orphan reaping)
//! mirrors [`crate::server::ServerProvider`].

use std::{
    env,
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use teletype_speech::{ModelCapabilities, SpeechError, SpeechProvider};

/// Audio is always 16 kHz mono f32, matching the dictation capture pipeline
/// and the WAV header we build for the transcription endpoint.
const SAMPLE_RATE: u32 = 16_000;

/// Model load can take a while for a 300 MB GGUF plus projector on first
/// start; 120s mirrors the LLM server provider's bound.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_INTERVAL: Duration = Duration::from_millis(250);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// One transcription request. The in-process engines need no bound because a
/// wedged decoder is impossible; over HTTP it is, and the dictation worker
/// that called us would otherwise sit there until the app quits. This matches
/// the load bound above, so a request can never take longer to give up than
/// the server took to start.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// A local Qwen3-ASR model served by a child `llama-server` process.
pub struct Qwen3AsrProvider {
    model_id: String,
    /// The model directory [`Qwen3AsrProvider::load`] was given; kept so
    /// `shutdown` can locate the pid file without a second lock.
    model_dir: PathBuf,
    runtime: Mutex<Option<Runtime>>,
}

struct Runtime {
    base_url: String,
    api_key: String,
    /// Shared for every transcription of this session. Building a blocking
    /// client per request spins up a runtime each time, which is pure overhead
    /// on the dictation hot path.
    client: reqwest::blocking::Client,
    child: Child,
    log_path: PathBuf,
}

impl Qwen3AsrProvider {
    /// Creates a provider that will spawn `llama-server` for the model
    /// directory on first use.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            model_id: id.into(),
            model_dir: PathBuf::new(),
            runtime: Mutex::new(None),
        }
    }

    /// Where we record the pid of the child we spawn, so a crashed run's
    /// orphan can be reaped on the next launch. Lives beside the model files
    /// (inside the app's own data directory), not in a shared temp dir.
    fn pid_file(&self) -> PathBuf {
        self.model_dir.join("qwen3-asr.llama-server.pid")
    }

    /// Shared spawn + health-wait body. No-op if a runtime is already up.
    fn ensure_running(&mut self, dir: &Path) -> Result<(), SpeechError> {
        let mut guard = self
            .runtime
            .lock()
            .map_err(|e| SpeechError::ModelLoad(format!("lock poisoned: {e}")))?;
        if guard.is_some() {
            return Ok(());
        }

        self.model_dir = dir.to_path_buf();
        let artifacts = teletype_speech::catalog::QWEN3_ASR_ARTIFACTS;
        let main = dir.join(artifacts[0].file);
        let mmproj = dir.join(artifacts[1].file);
        if !main.is_file() || !mmproj.is_file() {
            return Err(SpeechError::ModelLoad(format!(
                "Qwen3-ASR model files not found in {}: need {} and {}",
                dir.display(),
                artifacts[0].file,
                artifacts[1].file
            )));
        }

        // Defense in depth: a prior run that was force-killed (crash, Cmd-Q
        // while loading) can leave an orphaned llama-server holding the model
        // in RAM. Reap *our own* orphan before spawning, the same way the LLM
        // server provider does (only a pid we recorded ourselves and whose
        // command line still matches is ever touched).
        let pid_file = self.pid_file();
        reap_orphan(&pid_file);

        let bin = crate::server::find_llama_server_bin().map_err(SpeechError::ModelLoad)?;
        let port = pick_port()?;
        let api_key = uuid::Uuid::new_v4().to_string();
        let log_path = env::temp_dir().join(format!(
            "teletype-llama-server-{}-{port}.log",
            self.model_id
        ));
        let log = File::create(&log_path)
            .map_err(|e| SpeechError::ModelLoad(format!("create server log {log_path:?}: {e}")))?;

        let mut child = Command::new(&bin)
            .arg("-m")
            .arg(&main)
            .arg("--mmproj")
            .arg(&mmproj)
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .arg("--api-key")
            .arg(&api_key)
            // The Qwen3-ASR GGUF ships a chat template that defaults to
            // thinking mode ON; ASR requests must not pay for reasoning.
            // (`--chat-template-kwargs {"enable_thinking":false}` is the old
            // spelling and this build warns that it is deprecated.)
            .args(["--reasoning", "off"])
            // The model's trained context is far larger than any dictation
            // needs, and llama-server allocates the full KV cache at load:
            // the default costs 9.6 GB of RSS on this host, 2.8 GB with
            // 2048 tokens, for the same 120-word transcript and the same
            // decode time (measured 2026-10-29).
            .args(["--ctx-size", "2048"])
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| SpeechError::ModelLoad(format!("failed to spawn {bin:?}: {e}")))?;

        let base_url = format!("http://127.0.0.1:{port}");
        if let Err(e) = wait_until_ready(&mut child, &base_url, &api_key, &log_path, &bin) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&pid_file);
            return Err(SpeechError::ModelLoad(e));
        }
        let _ = std::fs::write(&pid_file, child.id().to_string());

        tracing::info!(model = %self.model_id, port, "qwen3-asr llama-server ready");
        let client = reqwest::blocking::Client::builder()
            .timeout(TRANSCRIBE_TIMEOUT)
            .build()
            .map_err(|e| SpeechError::ModelLoad(format!("http client: {e}")))?;
        *guard = Some(Runtime {
            base_url,
            api_key,
            client,
            child,
            log_path,
        });
        Ok(())
    }

    /// Stops the child process if it is running.
    fn shutdown(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(mut rt) = guard.take() {
                let _ = rt.child.kill();
                let _ = rt.child.wait();
                let _ = std::fs::remove_file(self.pid_file());
                tracing::info!(log = %rt.log_path.display(), "qwen3-asr llama-server stopped");
            }
        }
    }
}

impl Drop for Qwen3AsrProvider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl SpeechProvider for Qwen3AsrProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        self.ensure_running(path)
    }

    fn unload(&mut self) {
        self.shutdown();
    }

    fn is_loaded(&self) -> bool {
        self.runtime.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        let (base_url, api_key, client) = {
            let guard = self
                .runtime
                .lock()
                .map_err(|e| SpeechError::Transcribe(format!("lock poisoned: {e}")))?;
            let rt = guard.as_ref().ok_or(SpeechError::NoModel)?.clone_endpoint();
            rt
        };

        let wav = f32_to_wav16(samples);

        let mut form = reqwest::blocking::multipart::Form::new().part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav).file_name("audio.wav"),
        );
        // `language` is a BCP code or "auto"; only send it when the caller
        // pinned one, so the model gets to auto-detect otherwise.
        if language != "auto" && !language.is_empty() {
            form = form.text("language", language.to_string());
        }

        let resp = client
            .post(format!("{base_url}/v1/audio/transcriptions"))
            .bearer_auth(&api_key)
            .multipart(form)
            .send()
            .map_err(|e| SpeechError::Transcribe(format!("transcription request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            let body: String = body.chars().take(400).collect();
            return Err(SpeechError::Transcribe(format!(
                "llama-server HTTP {status}: {body}"
            )));
        }
        let value: serde_json::Value = resp
            .json()
            .map_err(|e| SpeechError::Transcribe(format!("transcription response decode: {e}")))?;
        let text = value["text"]
            .as_str()
            .ok_or_else(|| SpeechError::Transcribe("transcription response missing `text`".into()))?
            .to_string();
        let cleaned = strip_language_prefix(&text);
        if cleaned.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        Ok(cleaned)
    }

    fn capabilities(&self) -> ModelCapabilities {
        // llama.cpp's audio path reports no per-word timings and has no
        // native vocabulary boost.
        ModelCapabilities {
            native_boost: false,
            word_timings: false,
        }
    }
}

impl Runtime {
    fn clone_endpoint(&self) -> (String, String, reqwest::blocking::Client) {
        (
            self.base_url.clone(),
            self.api_key.clone(),
            self.client.clone(),
        )
    }
}

/// Reap a llama-server recorded in `pid_file` from a previous run, if it is
/// still alive. Bounded: SIGTERM + up to 5 s, then SIGKILL, then up to 2 s.
///
/// Only a pid we recorded ourselves and that is still a llama-server is ever
/// touched, so a llama-server the user started themselves is never killed.
fn reap_orphan(pid_file: &Path) -> bool {
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
    tracing::info!(pid, reaped, "qwen3-asr: reaped previous llama-server");
    let _ = std::fs::remove_file(pid_file);
    reaped
}

/// True when `ps -o state=` output names a process that is still live.
#[cfg(unix)]
fn is_live_state(ps_state: &str) -> bool {
    match ps_state.trim().chars().next() {
        None => false,
        Some(c) => !matches!(c, 'Z' | 'z' | 'X' | 'x'),
    }
}

/// Send SIGTERM to `pid` and wait (bounded) for it to exit, escalating to
/// SIGKILL if it does not go. Returns `true` if the process is confirmed gone.
#[cfg(unix)]
fn bounded_kill(pid: u32) -> bool {
    use std::time::{Duration, Instant};

    let signal = |signum: i32| -> bool {
        // `pid_t` is signed: a pid above `i32::MAX` would wrap negative, and
        // `kill(-n, ...)` signals a whole process group. Reject instead of
        // casting so a corrupt pid file can never fan out a signal.
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: `kill(2)` only reads the two integer arguments. `pid` is a
        // positive value converted above, so it names a single process, and
        // an invalid or already-exited pid is a harmless `-1`/ESRCH result.
        unsafe { libc::kill(pid, signum) == 0 }
    };

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

/// Bounded-kill stub for non-unix targets (the reaper is a no-op there,
/// matching the LLM server provider).
#[cfg(not(unix))]
fn bounded_kill(pid: u32) -> bool {
    let _ = pid;
    true
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
                // reason "Code Signature Invalid".
                let logged = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
                if logged == 0 {
                    return Err(format!(
                        "llama-server was killed before it started ({status}) and wrote \
                         nothing to its log. On macOS this is almost always a binary whose \
                         contents were overwritten while another llama-server was running \
                         from it: the file then cannot be executed (SIGKILL, Code Signature \
                         Invalid) even though `file` reports a valid Mach-O. Fix with \
                         `codesign --force --sign - {bin}` and check for a stale server \
                         with `pgrep -fl llama-server`.",
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

/// Pick an ephemeral loopback port for the child server.
fn pick_port() -> Result<u16, SpeechError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| SpeechError::ModelLoad(format!("bind ephemeral port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| SpeechError::ModelLoad(format!("local_addr: {e}")))?
        .port();
    // Drop the listener so llama-server can bind it (small race, acceptable
    // for a single local process).
    drop(listener);
    Ok(port)
}

/// Encodes 16 kHz mono f32 samples as a minimal 16-bit PCM WAV in memory.
///
/// 44-byte RIFF/WAVE header, then little-endian int16 frames clamped to
/// [-1.0, 1.0]. The transcription endpoint accepts WAV; this is the smallest
/// shape it does.
fn f32_to_wav16(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());

    for s in samples {
        let v = s.clamp(-1.0, 1.0);
        let i = (v * 32767.0) as i16;
        out.extend_from_slice(&i.to_le_bytes());
    }
    out
}

/// Strips the language header that llama.cpp's Qwen3-ASR path leaves on the
/// transcript (e.g. `"language English<asr_text>hello world"`, or a bare
/// `"english hello world"`).
///
/// Conservative by design: only the `<asr_text>` marker or a leading token
/// from a fixed allowlist is removed. There is no shape heuristic, so the
/// first word of a dictation ("no thanks", "the weather", "it works") is
/// never eaten.
fn strip_language_prefix(text: &str) -> String {
    let trimmed = text.trim();
    // The model's chat template closes the language header with this marker;
    // llama.cpp does not strip the header (llama.cpp #26749, #24765). Where
    // the marker survives, it is the authoritative split point.
    if let Some((_, after)) = trimmed.rsplit_once(ASR_TEXT_MARKER) {
        return after.trim().to_string();
    }
    match trimmed.split_once(char::is_whitespace) {
        Some((first, rest)) if is_language_tag(first) => rest.trim().to_string(),
        None if is_language_tag(trimmed) => String::new(),
        _ => trimmed.to_string(),
    }
}

/// The marker llama.cpp leaves between the language header and the text.
const ASR_TEXT_MARKER: &str = "<asr_text>";

/// The language *words* Qwen3-ASR may name in its header.
const LANGUAGE_WORDS: &[&str] = &[
    "english",
    "polski",
    "polish",
    "deutsch",
    "german",
    "francais",
    "french",
    "espanol",
    "spanish",
    "italiano",
    "italian",
    "portugues",
    "portuguese",
    "japans",
    "japanese",
    "chines",
    "chinese",
    "korean",
    "arabic",
    "hindi",
    "turkish",
    "russian",
    "ukrainian",
    "czech",
    "dutch",
    "swedish",
    "norwegian",
    "finnish",
    "danish",
    "hungarian",
    "romanian",
    "vietnamese",
    "thai",
    "indonesian",
];

/// The two-letter codes from this model's supported-language list that are
/// safe to strip: `"hi"`, `"it"` and `"no"` are excluded because they are
/// English words a dictation can legitimately start with, and the header is
/// only ever a fallback for the (usual) case where `<asr_text>` is absent.
const LANGUAGE_CODES: &[&str] = &[
    "ar", "bg", "cs", "da", "de", "el", "en", "es", "fi", "fr", "hr", "hu", "id", "ja", "ko", "ms",
    "nl", "pl", "pt", "ro", "ru", "sk", "sv", "tr", "uk", "vi", "zh",
];

fn is_language_tag(token: &str) -> bool {
    token
        .chars()
        .all(|c| c.is_ascii() && c.is_ascii_lowercase())
        && (LANGUAGE_WORDS.contains(&token) || LANGUAGE_CODES.contains(&token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_riff_pcm16_mono_16k() {
        let wav = f32_to_wav16(&[0.0; 100]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[16..20], &16u32.to_le_bytes()); // fmt chunk size
        assert_eq!(&wav[20..22], &1u16.to_le_bytes()); // PCM
        assert_eq!(&wav[22..24], &1u16.to_le_bytes()); // mono
        assert_eq!(&wav[24..28], &16_000u32.to_le_bytes()); // sample rate
        assert_eq!(&wav[28..32], &32_000u32.to_le_bytes()); // byte rate
        assert_eq!(&wav[32..34], &2u16.to_le_bytes()); // block align
        assert_eq!(&wav[34..36], &16u16.to_le_bytes()); // bits per sample
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[40..44], &200u32.to_le_bytes()); // data length
                                                         // 44 header + 100 frames * 2 bytes.
        assert_eq!(wav.len(), 44 + 200);
    }

    #[test]
    fn wav_encodes_and_clamps_samples() {
        let wav = f32_to_wav16(&[0.0, 0.5, -1.0, 3.0]);
        let frames = &wav[44..];
        let frame = |i: usize| i16::from_le_bytes([frames[i * 2], frames[i * 2 + 1]]);
        assert_eq!(frame(0), 0);
        // 0.5 * 32767 = 16383.5 -> 16383
        assert_eq!(frame(1), 16383);
        assert_eq!(frame(2), -32767);
        // 3.0 clamps to 1.0 -> 32767
        assert_eq!(frame(3), 32767);
    }

    #[test]
    fn wav_empty_input_is_header_only() {
        let wav = f32_to_wav16(&[]);
        assert_eq!(wav.len(), 44);
        assert_eq!(&wav[40..44], &0u32.to_le_bytes());
    }

    #[test]
    fn strips_language_word_prefix() {
        assert_eq!(strip_language_prefix("english hello world"), "hello world");
        assert_eq!(strip_language_prefix("polski dobry dzien"), "dobry dzien");
        assert_eq!(strip_language_prefix("deutsch hallo"), "hallo");
    }

    #[test]
    fn strips_language_code_prefix() {
        assert_eq!(strip_language_prefix("en hello"), "hello");
        assert_eq!(strip_language_prefix("pl dobry"), "dobry");
        assert_eq!(strip_language_prefix("de hallo"), "hallo");
    }

    #[test]
    fn leaves_real_first_words_alone() {
        // A single long word is content, not a tag.
        assert_eq!(strip_language_prefix("hello world"), "hello world");
        assert_eq!(strip_language_prefix("hello"), "hello");
        // A 5-letter word is too long to be a code.
        assert_eq!(strip_language_prefix("never mind"), "never mind");
        // Uppercase first word is content.
        assert_eq!(strip_language_prefix("Hello world"), "Hello world");
    }

    #[test]
    fn bare_language_tag_becomes_empty() {
        assert_eq!(strip_language_prefix("en"), "");
        assert_eq!(strip_language_prefix("english"), "");
        assert_eq!(strip_language_prefix("   "), "");
        assert_eq!(strip_language_prefix(""), "");
    }

    #[test]
    fn strips_the_asr_text_marker_header() {
        assert_eq!(
            strip_language_prefix("language English<asr_text>hello world"),
            "hello world"
        );
        // The header names the language in words; the marker is still the cut.
        assert_eq!(
            strip_language_prefix("language Polski<asr_text>dobry dzien"),
            "dobry dzien"
        );
        // Marker with no text after it means nothing was transcribed.
        assert_eq!(strip_language_prefix("language English<asr_text>"), "");
    }

    #[test]
    fn never_eats_a_word_that_is_also_a_language_code() {
        // "no", "it" and "hi" are codes for Norwegian, Italian and Hindi, but
        // they are also the first word of plenty of English dictations.
        assert_eq!(strip_language_prefix("no thanks"), "no thanks");
        assert_eq!(strip_language_prefix("it works fine"), "it works fine");
        assert_eq!(strip_language_prefix("hi there"), "hi there");
        // Ordinary short English function words are not tags either.
        assert_eq!(
            strip_language_prefix("the weather today"),
            "the weather today"
        );
        assert_eq!(strip_language_prefix("ok sounds good"), "ok sounds good");
        assert_eq!(strip_language_prefix("yes please"), "yes please");
    }

    #[test]
    fn capabilities_are_base_text_only() {
        let p = Qwen3AsrProvider::new("qwen3-asr");
        let caps = p.capabilities();
        assert!(!caps.native_boost);
        assert!(!caps.word_timings);
    }

    /// End-to-end proof against the real weights: the two-file directory
    /// layout, the spawn arguments (including `--mmproj`), the
    /// `/v1/audio/transcriptions` route, and the language-header strip. Every
    /// other test here mocks the server, so this is the only one that shows a
    /// pinned llama-server can actually load a pinned Qwen3-ASR pair. It needs
    /// the download and about 3 GB of RAM, so it is ignored by default. Run:
    ///
    /// ```text
    /// cargo test -p teletype-inference --lib qwen3_asr_live \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// `TELETYPE_QWEN3_ASR_DIR` overrides the model directory and
    /// `TELETYPE_QWEN3_ASR_WAV` the clip (mono 16 kHz PCM16).
    #[test]
    #[ignore = "spawns llama-server with the 2.4 GB Qwen3-ASR pair"]
    fn qwen3_asr_live_transcribes_a_real_clip() {
        let dir = repo_relative(&env::var("TELETYPE_QWEN3_ASR_DIR").unwrap_or_else(|_| {
            format!(
                "{}/Library/Application Support/com.teletype.app/models/qwen3-asr",
                env::var("HOME").unwrap_or_default()
            )
        }));
        let wav = repo_relative(
            &env::var("TELETYPE_QWEN3_ASR_WAV")
                .unwrap_or_else(|_| "evals/corpus/audio/plain-01.wav".into()),
        );
        let samples = match read_wav_pcm16_mono(&wav) {
            Some(s) => s,
            None => {
                eprintln!(
                    "skipping: {} is not a readable mono 16 kHz PCM16 clip",
                    wav.display()
                );
                return;
            }
        };
        let mut provider = Qwen3AsrProvider::new("qwen3-asr");
        if !dir.is_dir() {
            eprintln!("skipping: no model directory at {}", dir.display());
            return;
        }
        let started = Instant::now();
        provider
            .load(&dir)
            .unwrap_or_else(|e| panic!("load failed for {}: {e}", dir.display()));
        eprintln!(
            "loaded in {} ms ({} s of audio)",
            started.elapsed().as_millis(),
            samples.len() / SAMPLE_RATE as usize
        );
        let decode = Instant::now();
        let text = provider
            .transcribe(&samples, "en")
            .expect("transcription failed");
        eprintln!("transcript in {} ms: {text}", decode.elapsed().as_millis());
        assert!(
            text.split_whitespace().count() >= 3,
            "a real clip produced {text:?}"
        );
        assert!(!text.contains(ASR_TEXT_MARKER), "header survived: {text}");
        assert!(
            !text.to_lowercase().starts_with("language "),
            "header survived: {text}"
        );
        // Scored against the clip's own reference so any corpus clip works.
        let reference = manifest_reference(&wav);
        if let Some(reference) = reference {
            let expected = tokenize_words(&reference);
            let got = tokenize_words(&text);
            let errors = edit_distance(&expected, &got);
            let wer = errors as f64 / expected.len().max(1) as f64;
            assert!(
                wer <= 0.2,
                "word error rate {wer:.2} ({errors} of {}): expected {reference:?}, got {text:?}",
                expected.len()
            );
        }
    }

    /// Lowercase alphanumeric words, so punctuation and casing cannot fail a
    /// transcript comparison.
    fn tokenize_words(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_lowercase())
            .collect()
    }

    /// Levenshtein distance over word slices.
    fn edit_distance(a: &[String], b: &[String]) -> usize {
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        let mut cur = vec![0usize; b.len() + 1];
        for (i, x) in a.iter().enumerate() {
            cur[0] = i + 1;
            for (j, y) in b.iter().enumerate() {
                let cost = if x == y { 0 } else { 1 };
                cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        prev[b.len()]
    }

    /// The `evals/corpus/manifest.jsonl` reference for `wav`, matched by file
    /// name. `None` when the clip is not from the corpus.
    fn manifest_reference(wav: &Path) -> Option<String> {
        let name = wav.file_name()?.to_string_lossy().to_string();
        let manifest =
            std::fs::read_to_string(repo_relative("evals/corpus/manifest.jsonl")).ok()?;
        for line in manifest.lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if entry["audio"].as_str().is_some_and(|a| a.ends_with(&name)) {
                return entry["reference"].as_str().map(str::to_string);
            }
        }
        None
    }

    /// Resolves a path against the workspace root, because `cargo test` runs
    /// the binary from the crate directory, not from the repo root. Absolute
    /// paths pass through untouched.
    fn repo_relative(path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
    }

    /// Minimal RIFF/WAVE reader for 16 kHz mono PCM16 (chunk-walking), the
    /// shape the eval corpus and the capture pipeline both produce.
    fn read_wav_pcm16_mono(path: &Path) -> Option<Vec<f32>> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return None;
        }
        let mut offset = 12;
        let mut saw_16k_mono = false;
        while offset + 8 <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
            let start = offset + 8;
            if id == b"fmt " && size >= 16 {
                let format = u16::from_le_bytes(bytes[start..start + 2].try_into().ok()?);
                let channels = u16::from_le_bytes(bytes[start + 2..start + 4].try_into().ok()?);
                let rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().ok()?);
                saw_16k_mono = format == 1 && channels == 1 && rate == 16_000;
            }
            if id == b"data" {
                if !saw_16k_mono {
                    return None;
                }
                let end = start.checked_add(size)?.min(bytes.len());
                return Some(
                    bytes[start..end]
                        .chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
                        .collect(),
                );
            }
            offset = start + size + (size & 1);
        }
        None
    }
}
