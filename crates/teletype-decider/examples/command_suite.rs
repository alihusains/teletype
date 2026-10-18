// Command-suite accuracy harness (Addendum A.4, task 7).
//
// Runs the frozen TTS command corpus (`evals/commands/corpus/corpus.json`)
// through the three engines in the command-decision chain and writes a
// per-clip report:
//
// - **NeuDecide** — the decider crate itself. Runs in stub mode by default
//   (no `onnx` feature, no provisioned model): every clip records
//   `status: "unavailable"`. That is a measured column, not a bug.
// - **Needle** — `teletype-inference`'s Needle 3 provider. Runs only when
//   the `.cact` model file is present locally (probed at run time);
//   otherwise every clip records `status: "env-gated"`.
// - **LLM Fallback** — the OpenAI-compatible polish endpoint. Runs only
//   when `TELETYPE_EVAL_LLM_URL` / `TELETYPE_EVAL_LLM_API_KEY` /
//   `TELETYPE_EVAL_LLM_MODEL` are set (the same convention as
//   `teletype-eval`'s `LlmJudge`); otherwise `status: "env-gated"`.
//
// Skip-cells are JSON `null` (Phase 10 harness convention). The report
// lands at `evals/runs/command_suite_<date>/report.json`.
//
// Usage:
//   cargo run -p teletype-decider --example command_suite \
//       [--manifest path] [--out evals/runs/command_suite_<ts>/]
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

/// Minimal 16-bit mono WAV reader: returns f32 samples normalized to
/// [-1.0, 1.0] and the file's sample rate. ~40 lines, no deps.
fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(format!("{}: not a RIFF/WAVE file", path.display()));
    }
    let mut pos = 12usize;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 1;
    let mut bits: u16 = 0;
    let mut pcm: Option<&[u8]> = None;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            channels = u16::from_le_bytes(data[body + 2..body + 4].try_into().unwrap());
            sample_rate = u32::from_le_bytes(data[body + 4..body + 8].try_into().unwrap());
            bits = u16::from_le_bytes(data[body + 14..body + 16].try_into().unwrap());
        } else if id == b"data" {
            pcm = Some(&data[body..body + size.min(data.len() - body)]);
        }
        pos = body + size + (size & 1);
    }
    let pcm = pcm.ok_or_else(|| format!("{}: no data chunk", path.display()))?;
    if bits != 16 {
        return Err(format!(
            "{}: unsupported bit depth {bits} (need 16)",
            path.display()
        ));
    }
    // Mono: one i16 per 2 bytes. (Corpus wavs are mono by construction;
    // the generator downmixes at write time.)
    let mut samples = Vec::with_capacity(pcm.len() / 2);
    for i in 0..pcm.len() / 2 {
        let v = i16::from_le_bytes(pcm[i * 2..i * 2 + 2].try_into().unwrap());
        samples.push(v as f32 / 32768.0);
    }
    let _ = channels;
    Ok((samples, sample_rate))
}

/// Normalizes one engine's raw outcome to a comparable shape:
/// `Some((tool, args))` or `None` (= `continue_speaking` / no tool call).
///
/// Accepts the shapes the three engines realistically emit:
/// - corpus manifest: `{"type": "open_app", "tool": "open_app", "args": {...}}`
/// - Needle: `{"function_calls": [{"name": ..., "arguments": {...}}]}`
/// - LLM fallback: `{"name": ..., "arguments": {...}}` (or `"tool"` key),
///   with the `"none"` sentinel meaning no call.
pub fn normalize_decision(v: &Value) -> Option<(String, Value)> {
    // Decider `Vec<ToolCall>` shape: a JSON array of {"name", "arguments"} —
    // take the first call (an empty list means no tool call).
    if let Some(arr) = v.as_array() {
        return if arr.is_empty() {
            None
        } else {
            normalize_decision(&arr[0])
        };
    }
    // Corpus manifest expected shape.
    if let (Some(t), Some(tool)) = (v.get("type"), v.get("tool")) {
        if t.as_str() == Some("open_app") || tool.as_str().is_some() {
            let tool = tool.as_str()?.to_string();
            let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
            return Some((tool, args));
        }
    }
    // Needle shape.
    if let Some(calls) = v.get("function_calls").and_then(|c| c.as_array()) {
        return if calls.is_empty() {
            None
        } else {
            let first = &calls[0];
            Some((
                first["name"].as_str()?.to_string(),
                first["arguments"].clone(),
            ))
        };
    }
    if let Some(sup) = v.get("suppressed_calls").and_then(|c| c.as_array()) {
        return if sup.is_empty() {
            None
        } else {
            let first = &sup[0];
            Some((
                first["name"].as_str()?.to_string(),
                first["arguments"].clone(),
            ))
        };
    }
    // LLM fallback shape.
    let name = v
        .get("name")
        .or_else(|| v.get("tool"))
        .and_then(|n| n.as_str())?;
    if name == "none" {
        return None;
    }
    let args = v.get("arguments").cloned().unwrap_or_else(|| json!({}));
    Some((name.to_string(), args))
}

/// Scores one clip for one engine. Returns a JSON row with `correct`,
/// `got`, `expected`. Callers wrap skip-cells as `null` separately.
pub fn score_clip(expected: Option<&Value>, got_raw: Option<&Value>) -> Value {
    let expected_call = expected.and_then(normalize_decision);
    let got_call = got_raw.and_then(normalize_decision);
    let correct = match (expected_call.as_ref(), got_call.as_ref()) {
        (Some((e_tool, e_args)), Some((g_tool, g_args))) => {
            e_tool.trim() == g_tool.trim() && norm_args(e_args) == norm_args(g_args)
        }
        (None, None) => true,
        _ => false,
    };
    json!({
        "correct": correct,
        "got": got_raw,
        "expected": expected,
    })
}

/// Normalizes argument objects for comparison: trim string values,
/// drop nulls.
pub fn norm_args(args: &Value) -> Value {
    match args {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if v.is_null() {
                    continue;
                }
                let v = match v {
                    Value::String(s) => Value::String(s.trim().to_string()),
                    Value::Object(_) => norm_args(v),
                    other => other.clone(),
                };
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        Value::String(s) => Value::String(s.trim().to_string()),
        other => other.clone(),
    }
}

pub fn sha256_12(bytes: &[u8]) -> String {
    // First 12 hex chars of SHA-256 (no new deps for an example).
    let h = sha256(bytes);
    let mut s = String::with_capacity(12);
    for byte in &h[..6] {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The Needle / LLM tool set (mirrors `COMMAND_TOOLS_JSON` in
/// `teletype-inference/src/needle.rs`). Inlined here so the example does
/// not pull in `teletype-inference` as a dev-dependency (it drags in
/// `needle-sys`, `reqwest`, and the whole inference stack). The seam for
/// a real in-process Needle column is documented in `run_needle_column`.
const COMMAND_TOOLS_JSON: &str = r#"[
{"name":"open_app","description":"Open or launch ANY application by the name the user said, exactly as spoken.","parameters":{"type":"object","properties":{"app":{"type":"string","description":"The application name exactly as the user said it"}}}},
{"name":"open_url","description":"Navigate to a website or search the web.","parameters":{"type":"object","properties":{"url":{"type":"string","description":"The URL or search query"}},"required":["url"]}},
{"name":"set_brightness","description":"Adjust screen brightness.","parameters":{"type":"object","properties":{"level":{"type":"string","enum":["lower","higher","min","max"]}},"required":["level"]}},
{"name":"set_volume","description":"Adjust system volume.","parameters":{"type":"object","properties":{"level":{"type":"string","enum":["lower","higher","mute","unmute","min","max"]}},"required":["level"]}},
{"name":"mute_speaker","description":"Mute or unmute the system speaker.","parameters":{"type":"object","properties":{"muted":{"type":"boolean"}},"required":["muted"]}},
{"name":"keyboard_shortcut","description":"Perform a keyboard shortcut action.","parameters":{"type":"object","properties":{"action":{"type":"string"}},"required":["action"]}},
{"name":"play_media","description":"Play, pause, or skip media.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["play","pause","next","previous"]}},"required":["action"]}},
{"name":"set_timer","description":"Set a timer or alarm.","parameters":{"type":"object","properties":{"duration":{"type":"string"}},"required":["duration"]}},
{"name":"send_message","description":"Send a message to a contact or channel.","parameters":{"type":"object","properties":{"to":{"type":"string"},"text":{"type":"string"}},"required":["to","text"]}},
{"name":"none","description":"Return this when the utterance is not a command.","parameters":{"type":"object","properties":{}}}
]"#;

const COMMAND_SYSTEM_PROMPT: &str = "You are a voice command parser. Convert the user's spoken command into exactly one tool call. If the utterance is not a command, return the 'none' tool. Do not guess.";

/// Runs the Needle column for one clip.
///
/// SEAM NOTE (documented per spec): `NeedleProvider` lives in
/// `teletype-inference`. Adding it as a dev-dependency of
/// `teletype-decider` is a clean one-liner with no feature entanglement
/// and no dep cycle (`teletype-inference` does not depend on
/// `teletype-decider`), but it pulls the whole inference stack
/// (`needle-sys`, `reqwest`, `teletype-speech`, `teletype-core`) into
/// the full dev build, so the column
/// stays behind `TELETYPE_EVAL_NEEDLE_CACT` (same path convention as
/// `teletype-inference`'s e2e tests, `models/needle3.cact`): unset or
/// (2026-10-17): production path — fresh engine per clip (load
/// ~11 ms), then `DecisionProvider::extract_command`, which self-inits
/// with the command catalog and decodes under grammar constraint. A
/// `None` result is a legitimate "no tool call" decision (scored),
/// not an engine failure.
fn run_needle_column(_samples: &[f32], text: &str) -> Option<Option<Value>> {
    use teletype_core::llm::DecisionProvider;
    let cact = env::var("TELETYPE_EVAL_NEEDLE_CACT").ok()?;
    let cact = PathBuf::from(&cact);
    if !cact.is_file() {
        return None;
    }
    // Needle 3 is a TEXT model: the pipeline feeds it the ASR
    // transcript, so this column scores on the corpus text.
    let provider = teletype_inference::NeedleProvider::new("needle3", "Needle 3", &cact).ok()?;
    let call = provider.extract_command(text);
    // The needle shape `normalize_decision` handles: an empty
    // `function_calls` array = no-call decision.
    let v = match call {
        Some(a) => json!({"function_calls": [{"name": a.tool, "arguments": a.arguments}]}),
        None => json!({"function_calls": []}),
    };
    Some(Some(v))
}

/// Runs the LLM Fallback column via a plain HTTP POST to the
/// OpenAI-compatible endpoint (the `TELETYPE_EVAL_LLM_*` convention from
/// `teletype-eval`'s `LlmJudge`). No HTTP dep: a minimal std-only client
/// for `http://` URLs (llama-server is the documented local target);
/// `https://` URLs are env-gated (no TLS in std).
///
/// NOTE: without an ASR step this column decides from clip metadata only,
/// so its accuracy is not comparable to the audio-fed columns until a
/// transcript source is wired in. It is kept env-gated-by-default and
/// reported as measured plumbing.
fn run_llm_column(samples_len: usize, _rate: u32) -> Option<Option<Value>> {
    let url = env::var("TELETYPE_EVAL_LLM_URL").ok()?;
    let api_key = env::var("TELETYPE_EVAL_LLM_API_KEY").unwrap_or_default();
    let model = env::var("TELETYPE_EVAL_LLM_MODEL").ok()?;
    if url.starts_with("https://") {
        return None;
    }
    if samples_len == 0 {
        return None;
    }
    let body = json!({
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": format!("{COMMAND_SYSTEM_PROMPT}\nCommand tools: {COMMAND_TOOLS_JSON}")},
            {"role": "user", "content": format!("Audio clip: {samples_len} samples at 16 kHz mono. No transcript is available in this harness; return the 'none' tool unless the metadata alone identifies a command.")},
        ],
    });
    let raw = http_post(&url, &api_key, &body.to_string()).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let content = v["choices"][0]["message"]["content"].as_str()?;
    let content = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: Value = serde_json::from_str(content).ok()?;
    Some(Some(parsed))
}

/// Minimal std-only HTTP POST (http:// only). Returns the response body.
fn http_post(url: &str, api_key: &str, body: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "only http:// supported".to_string())?;
    let (host, path) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/".to_string()));
    let addr = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
        api_key
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut resp = Vec::new();
    stream
        .read_to_end(&mut resp)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&resp);
    let body_start = text.find("\r\n\r\n").ok_or("no header/body split")? + 4;
    Ok(text[body_start..].to_string())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut manifest_path = PathBuf::from("evals/commands/corpus/corpus.json");
    let mut out_dir = PathBuf::from(format!("evals/runs/command_suite_{}", date_stamp()));
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--manifest" => {
                i += 1;
                manifest_path = PathBuf::from(&args[i]);
            }
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(&args[i]);
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let manifest_bytes = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let manifest: Value =
        serde_json::from_str(&manifest_bytes).unwrap_or_else(|e| panic!("bad manifest JSON: {e}"));
    let clips = manifest
        .get("clips")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let manifest_hash = sha256_12(manifest_bytes.as_bytes());
    println!(
        "command_suite: {} clips, manifest sha256-12 {manifest_hash}",
        clips.len()
    );

    let corpus_dir = manifest_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    // A.5 corpus: `open_app`'s `app` arg is an enum built from the runtime's
    // top-N app list; an empty list makes the tool undecodable (the 0/20
    // artifact found 2026-10-17). Inject the apps the manifest expects.
    let mut apps: Vec<String> = clips
        .iter()
        .filter_map(|c| c["expected"]["args"]["app"].as_str())
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .collect();
    apps.sort();
    apps.dedup();
    let tools_json = teletype_decider::tools_to_json(&teletype_decider::v1_tools(&apps, &[], &[]));
    // Probe engine availability once.
    let neudecide_available = probe_neudecide();
    let needle_cact = env::var("TELETYPE_EVAL_NEEDLE_CACT")
        .ok()
        .filter(|p| Path::new(p).is_file());
    let llm_available =
        env::var("TELETYPE_EVAL_LLM_URL").is_ok() && env::var("TELETYPE_EVAL_LLM_MODEL").is_ok();

    println!(
        "engines: neudecide={} needle={} llm={}",
        if neudecide_available {
            "available"
        } else {
            "unavailable (stub mode)"
        },
        match &needle_cact {
            Some(p) => format!("model at {p} (in-process needle, TELETYPE_EVAL_NEEDLE_CACT)"),
            None => "env-gated (no TELETYPE_EVAL_NEEDLE_CACT)".to_string(),
        },
        if llm_available {
            "available"
        } else {
            "env-gated (no TELETYPE_EVAL_LLM_URL)"
        }
    );

    let mut rows: Vec<Value> = Vec::with_capacity(clips.len());
    let mut agg: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // engine -> (correct, scored)
    let t0 = Instant::now();

    for clip in &clips {
        let path = clip["path"].as_str().unwrap_or_default();
        let text = clip["text"].as_str().unwrap_or_default();
        let category = clip["category"].as_str().unwrap_or_default();
        let expected = clip.get("expected");

        // Manifest `path` values are relative to `evals/`
        // (e.g. `commands/corpus/open_app_000.wav`); the corpus dir is
        // `<evals>/commands/corpus`, so join against the repo root.
        let wav_path = corpus_dir
            .join("..")
            .join("..")
            .join(path)
            .canonicalize()
            .unwrap_or_else(|_| corpus_dir.join(path));
        let (samples, rate) = match read_wav(&wav_path) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("WARN: {e}");
                rows.push(json!({
                    "path": path,
                    "category": category,
                    "text": text,
                    "expected": expected,
                    "neudecide": Value::Null,
                    "needle": Value::Null,
                    "llm": Value::Null,
                    "error": e,
                }));
                continue;
            }
        };

        // NeuDecide column.
        let neudecide_cell = if neudecide_available {
            run_neudecide(&samples, expected, &tools_json)
        } else {
            json!({"status": "unavailable", "reason": "model not provisioned (stub mode)"})
        };

        // Needle column.
        let needle_cell = match run_needle_column(&samples, text) {
            Some(raw) => {
                let mut v = score_clip(expected, raw.as_ref());
                v["status"] = json!("ok");
                v
            }
            None => {
                json!({"status": "env-gated", "reason": "TELETYPE_EVAL_NEEDLE_CACT not set or in-process seam not wired"})
            }
        };

        // LLM column.
        let llm_cell = if llm_available {
            match run_llm_column(samples.len(), rate) {
                Some(raw) => {
                    let mut v = score_clip(expected, raw.as_ref());
                    v["status"] = json!("ok");
                    v
                }
                None => json!({"status": "env-gated", "reason": "endpoint unreachable or https"}),
            }
        } else {
            json!({"status": "env-gated", "reason": "TELETYPE_EVAL_LLM_URL/TELETYPE_EVAL_LLM_MODEL not set"})
        };

        // Aggregate per engine (only scored cells count).
        for (engine, cell) in [
            ("neudecide", &neudecide_cell),
            ("needle", &needle_cell),
            ("llm", &llm_cell),
        ] {
            if let Some(correct) = cell.get("correct").and_then(|c| c.as_bool()) {
                let (c, s) = agg.entry(engine.to_string()).or_insert((0, 0));
                *s += 1;
                if correct {
                    *c += 1;
                }
            }
        }

        rows.push(json!({
            "path": path,
            "category": category,
            "text": text,
            "expected": expected,
            "neudecide": neudecide_cell,
            "needle": needle_cell,
            "llm": llm_cell,
        }));
    }

    // Per-category accuracy per engine (scored cells only).
    let mut cat_agg: BTreeMap<String, BTreeMap<String, (u64, u64)>> = BTreeMap::new();
    for row in &rows {
        let cat = row["category"].as_str().unwrap_or_default().to_string();
        for engine in ["neudecide", "needle", "llm"] {
            if let Some(correct) = row[engine].get("correct").and_then(|c| c.as_bool()) {
                let (c, s) = cat_agg
                    .entry(cat.clone())
                    .or_default()
                    .entry(engine.to_string())
                    .or_insert((0, 0));
                *s += 1;
                if correct {
                    *c += 1;
                }
            }
        }
    }

    let categories: Vec<&str> = vec![
        "open_app",
        "edit_verb",
        "focus_control",
        "pointer",
        "itn_sensitive",
        "negative",
    ];

    let report = json!({
        "schema": "teletype-command-suite-report-v1",
        "date": date_stamp(),
        "manifest": manifest_path.display().to_string(),
        "manifest_sha256_12": manifest_hash,
        "clips": clips.len(),
        "elapsed_ms": t0.elapsed().as_millis() as u64,
        "rows": rows,
        "aggregates": {
            "overall": overall_agg(&agg),
            "per_category": per_category_agg(&cat_agg, &categories),
        },
    });

    fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", out_dir.display()));
    let report_path = out_dir.join("report.json");
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap_or_else(|e| panic!("cannot write {}: {e}", report_path.display()));

    // Print the RESULTS.md table section (the lead pastes it).
    print_results_section(&report);
    println!("\nreport: {}", report_path.display());
}

fn overall_agg(agg: &BTreeMap<String, (u64, u64)>) -> Value {
    let mut obj = serde_json::Map::new();
    for engine in ["neudecide", "needle", "llm"] {
        obj.insert(
            engine.to_string(),
            match agg.get(engine) {
                Some(&(c, s)) if s > 0 => {
                    json!({ "correct": c, "scored": s, "accuracy": c as f64 / s as f64 })
                }
                _ => Value::Null,
            },
        );
    }
    Value::Object(obj)
}

fn per_category_agg(
    cat_agg: &BTreeMap<String, BTreeMap<String, (u64, u64)>>,
    categories: &[&str],
) -> Value {
    let mut obj = serde_json::Map::new();
    for cat in categories {
        let mut engines = serde_json::Map::new();
        for engine in ["neudecide", "needle", "llm"] {
            engines.insert(
                engine.to_string(),
                cat_agg
                    .get(*cat)
                    .and_then(|m| m.get(engine))
                    .map(|&(c, s)| {
                        if s > 0 {
                            json!({ "correct": c, "scored": s, "accuracy": c as f64 / s as f64 })
                        } else {
                            Value::Null
                        }
                    })
                    .unwrap_or(Value::Null),
            );
        }
        obj.insert(cat.to_string(), Value::Object(engines));
    }
    Value::Object(obj)
}

fn print_results_section(report: &Value) {
    let date = report["date"].as_str().unwrap_or_default();
    let hash = report["manifest_sha256_12"].as_str().unwrap_or_default();
    let categories = [
        "open_app",
        "edit_verb",
        "focus_control",
        "pointer",
        "itn_sensitive",
        "negative",
    ];
    println!("\n## Command suite accuracy");
    println!();
    println!("Date: {date} · manifest sha256-12 `{hash}`");
    println!();
    println!("| engine | {} | overall |", categories.join(" | "));
    let sep = vec!["---"; categories.len() + 2].join("|");
    println!("|{sep}|");
    for engine in ["neudecide", "needle", "llm"] {
        let mut cells = vec![engine.to_string()];
        for cat in &categories {
            let cell = &report["aggregates"]["per_category"][cat][engine];
            cells.push(match cell {
                Value::Null => "n/a".to_string(),
                _ => format_acc(cell),
            });
        }
        let overall = &report["aggregates"]["overall"][engine];
        cells.push(match overall {
            Value::Null => "n/a (model not provisioned)".to_string(),
            _ => format_acc(overall),
        });
        println!("| {} |", cells.join(" | "));
    }
    println!();
    println!(
        "Skip-cells are `null` in report.json; `n/a` in the table. Rule: a new dated row per run, never edit past rows."
    );
}

fn format_acc(cell: &Value) -> String {
    let c = cell["correct"].as_u64().unwrap_or(0);
    let s = cell["scored"].as_u64().unwrap_or(0);
    if s == 0 {
        "n/a".to_string()
    } else {
        format!("{c}/{s} ({:.1}%)", 100.0 * c as f64 / s as f64)
    }
}

/// Probe for a provisioned NeuDecide model. In this build the `onnx`
/// feature is off, so `NeuDecide::load` returns `RuntimeNotEnabled`
/// regardless of the model path; the column is then `unavailable`.
fn neudecide_dir() -> PathBuf {
    env::var(concat!("TELETYPE_EVAL_NEUDECIDE", "_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("models/neudecide"))
}

/// Probe for a provisioned NeuDecide model. In this build the `onnx`
/// feature is off, so `NeuDecide::load` returns `RuntimeNotEnabled`
/// regardless of the model path; the column is then `unavailable`.
fn probe_neudecide() -> bool {
    neudecide_dir().join("config.json").is_file()
}

fn run_neudecide(samples: &[f32], expected: Option<&Value>, tools_json: &str) -> Value {
    // Real model path. `NeuDecide::load` only works with the `onnx`
    // feature; without it this returns `unavailable`. With it, we call
    // `generate` and score the tool calls. Kept as a function so the
    // column is real the moment the model is provisioned + the feature
    // is enabled. The tool schema is built in `main` from the apps the
    // manifest expects (an empty injected app list makes `open_app`'s
    // enum undecodable — the 0/20 artifact).
    match teletype_decider::NeuDecide::load(&neudecide_dir()) {
        Ok(mut model) => match model.generate(samples, tools_json) {
            Ok(calls) => {
                let v: Value = serde_json::to_value(&calls).unwrap_or(Value::Null);
                let mut row = score_clip(expected, Some(&v));
                row["status"] = json!("ok");
                row
            }
            Err(e) => json!({"status": "error", "error": e.to_string()}),
        },
        Err(e) => json!({"status": "unavailable", "error": e.to_string()}),
    }
}

fn date_stamp() -> String {
    // YYYY-MM-DD from the system clock (no `chrono` dep for an example).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = (secs / 86400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
