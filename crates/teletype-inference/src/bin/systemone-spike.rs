//! Decision-model spike: compare llama.cpp `/v1/systemone` models against the
//! deterministic AutoText disambiguation rule (Decision D008).
//!
//! Usage:
//!   cargo run -p teletype-inference --bin systemone-spike --serve <model> [cases.json]
//!   cargo run -p teletype-inference --bin systemone-spike --base-url http://127.0.0.1:8080 [cases.json]
//!
//! `--serve <path|hf-ref>` spawns a child llama-server for the model and kills
//! it on exit (path exists → `-m`, otherwise `-hf`). `--base-url` targets a
//! server that is already running.
//!
//! The case file (default: the built-in set) is a JSON array of
//! `{"text": "...", "phrase": "comma", "start": 8}`. `start` is the byte index
//! of the phrase in `text`; when omitted it is found with `str::find`. The
//! rule verdict for each case is computed live via `entry_allowed`, so the
//! binary is the ground-truth harness.
//!
//! Output: per-case rule-vs-model table, agreement %, and p50/p95 latency.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The real cases from `crates/teletype-core/src/autotext/disambiguate.rs`
/// tests plus the D008 context examples.
const DEFAULT_CASES: &str = r##"[
  {"text": "the payment period ends in March", "phrase": "period"},
  {"text": "use a comma to separate the fields", "phrase": "comma"},
  {"text": "the star of the show", "phrase": "star"},
  {"text": "the quote in the article was wrong", "phrase": "quote"},
  {"text": "that is a plus for the team", "phrase": "plus"},
  {"text": "start a new line for the address", "phrase": "new line"},
  {"text": "period", "phrase": "period"},
  {"text": "a period", "phrase": "period"},
  {"text": "period please", "phrase": "period"},
  {"text": "insert a comma", "phrase": "comma"},
  {"text": "the symbol for a period", "phrase": "period"},
  {"text": "I need a comma here", "phrase": "comma"},
  {"text": "add a full stop here", "phrase": "full stop"},
  {"text": "the payment full stop ends in March", "phrase": "full stop"},
  {"text": "two plus sign two", "phrase": "plus"},
  {"text": "close paren", "phrase": "close paren"}
]"##;

struct Case {
    text: String,
    phrase: String,
    start: usize,
}

fn parse_cases(raw: &str) -> Vec<Case> {
    let arr: Vec<Value> =
        serde_json::from_str(raw).expect("case file must be a JSON array of {text, phrase}");
    arr.into_iter()
        .map(|v| {
            let text = v["text"]
                .as_str()
                .expect("case.text must be a string")
                .to_string();
            let phrase = v["phrase"]
                .as_str()
                .expect("case.phrase must be a string")
                .to_string();
            let start = v
                .get("start")
                .and_then(|s| s.as_u64())
                .map(|s| s as usize)
                .unwrap_or_else(|| {
                    text.find(&phrase)
                        .unwrap_or_else(|| panic!("phrase '{phrase}' not found in '{text}'"))
                });
            Case {
                text,
                phrase,
                start,
            }
        })
        .collect()
}

fn find_binary() -> PathBuf {
    if let Ok(p) = std::env::var("TELETYPE_LLAMA_SERVER") {
        return PathBuf::from(p);
    }
    let local = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/teletype-desktop/binaries/llama-server"
    ));
    if local.exists() {
        return local.to_path_buf();
    }
    panic!(
        "llama-server not found. Build it first (scripts/build-llama-server.sh) \
         or set TELETYPE_LLAMA_SERVER."
    );
}

fn spawn_server(model: &str) -> (Child, String, String) {
    let bin = find_binary();
    let port = 18_900u16 + (std::process::id() % 1000) as u16;
    let child = Command::new(&bin)
        .arg(if Path::new(model).exists() {
            "-m"
        } else {
            "-hf"
        })
        .arg(model)
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("127.0.0.1")
        .arg("-c")
        .arg("4096")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {:?}: {e}", bin));
    let base = format!("http://127.0.0.1:{port}");
    let api_key = format!("spike-{}", std::process::id());
    (child, base, api_key)
}

fn wait_ready(base: &str, api_key: &str) {
    let client = reqwest::blocking::Client::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        match client
            .get(format!("{base}/health"))
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
        {
            Ok(r) if r.status().is_success() => return,
            _ => std::thread::sleep(Duration::from_millis(500)),
        }
    }
    panic!("llama-server at {base} did not become ready in 180s");
}

/// One decision request. Returns the P(yes) for the noul question.
fn ask(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: &str,
    model_id: &str,
    text: &str,
    phrase: &str,
) -> Result<(f32, Duration), String> {
    let mut body = json!({
        "state": text,
        "questions": {
            "symbol_request": {
                "type": "noul",
                "instructions": format!(
                    "In this dictation, is the speaker asking to TYPE the character or symbol named '{phrase}' (a symbol request), rather than using '{phrase}' as an ordinary English word?"
                )
            }
        }
    });
    if !model_id.is_empty() {
        body["model"] = json!(model_id);
    }
    let start = Instant::now();
    let resp = client
        .post(format!("{base}/v1/systemone"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let value: Value = resp.json().map_err(|e| format!("bad JSON: {e}"))?;
    let elapsed = start.elapsed();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {value}"));
    }
    let p = value["answers"]["symbol_request"]["noul"]
        .as_f64()
        .ok_or_else(|| format!("no noul probability in response: {value}"))?;
    Ok((p as f32, elapsed))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut serve: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut cases_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--serve" => {
                i += 1;
                serve = Some(
                    args.get(i)
                        .cloned()
                        .unwrap_or_else(|| panic!("--serve needs a model path or hf ref")),
                );
            }
            "--base-url" => {
                i += 1;
                base_url = Some(
                    args.get(i)
                        .cloned()
                        .unwrap_or_else(|| panic!("--base-url needs a URL")),
                );
            }
            other if !other.starts_with("--") => cases_path = Some(other.to_string()),
            other => eprintln!("unknown arg {other}, ignoring"),
        }
        i += 1;
    }
    if serve.is_none() && base_url.is_none() {
        eprintln!("Usage: systemone-spike --serve <model> | --base-url <url> [cases.json]");
        std::process::exit(1);
    }

    let cases_raw = match &cases_path {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {p}: {e}")),
        None => DEFAULT_CASES.to_string(),
    };
    let cases = parse_cases(&cases_raw);
    println!("{} cases\n", cases.len());

    let (child, base, api_key) = match &serve {
        Some(model) => {
            println!("spawning llama-server for {model} ...\n");
            let (c, b, k) = spawn_server(model);
            wait_ready(&b, &k);
            (Some(c), b, k)
        }
        None => {
            let b = base_url.unwrap();
            (None, b.clone(), "unused".to_string())
        }
    };
    let client = reqwest::blocking::Client::new();

    // Probe whether this server implements /v1/systemone at all.
    let probe = client
        .post(format!("{base}/v1/systemone"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&json!({
            "state": "ping",
            "questions": {"q": {"type": "noul", "instructions": "Is this a ping?"}}
        }))
        .send();
    if let Ok(r) = &probe {
        if r.status() == 404 {
            eprintln!(
                "server at {base} does not implement /v1/systemone (HTTP 404). \
                 It must be built from llama.cpp b11488 or newer (pin a657f7e9)."
            );
            std::process::exit(1);
        }
    }

    println!(
        "{:<48} {:<10} {:<10} {:<6}",
        "text", "rule", "model", "agree"
    );
    let mut agree = 0usize;
    let mut scored = 0usize;
    let mut latencies: Vec<Duration> = Vec::new();

    for case in &cases {
        let rule = teletype_core::autotext::disambiguate::entry_allowed(
            &case.text,
            case.start,
            &case.phrase,
        );
        let shown: String = case.text.chars().take(44).collect();
        match ask(&client, &base, &api_key, "", &case.text, &case.phrase) {
            Ok((p, elapsed)) => {
                scored += 1;
                latencies.push(elapsed);
                let model = p >= 0.5;
                let ok = rule == model;
                if ok {
                    agree += 1;
                }
                println!(
                    "{shown:<48} {:<10} {:<10.3} {:<6}",
                    if rule { "expand" } else { "keep" },
                    if model { "expand" } else { "keep" },
                    if ok { "yes" } else { "NO" }
                );
            }
            Err(e) => println!(
                "{shown:<48} {:<10} {:<10}",
                if rule { "expand" } else { "keep" },
                format!("ERR {e}")
            ),
        }
    }

    if scored > 0 {
        latencies.sort();
        let p50 = latencies[latencies.len() / 2];
        let p95 = latencies[std::cmp::min(
            (latencies.len() as f64 * 0.95) as usize,
            latencies.len() - 1,
        )];
        println!(
            "\nagreement with rule: {agree}/{scored} ({:.0}%)",
            100.0 * agree as f64 / scored as f64
        );
        println!("latency p50: {p50:?}  p95: {p95:?}");
    } else {
        println!("\nno case produced a model answer; nothing to score.");
    }

    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
}
