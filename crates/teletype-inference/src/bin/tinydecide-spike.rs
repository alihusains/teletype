//! TinyDecide spike: run the in-process TinyDecide decision model (10.4M params,
//! 6.2 MB, Apache 2.0) against the same 16 AutoText disambiguation cases as
//! `systemone-spike`, and compare against the deterministic rule.
//!
//! Unlike `systemone-spike` (which talks to a llama-server subprocess over
//! HTTP), this calls `TinyDecide::answer()` directly in-process — no server,
//! no port, no ggml. That is the architectural point: TinyDecide has no ggml,
//! so it does not collide with whisper.cpp (D002) and needs no subprocess.
//!
//! Usage:
//!   cargo run -p teletype-inference --bin tinydecide-spike [model-dir] [cases.json]
//!
//! `model-dir` defaults to `vendor/tinydecide-model` (the downloaded
//! meta.json + model.bin). The case file defaults to the same 16 built-in
//! cases as `systemone-spike`, so the two spikes are directly comparable.

use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;
use tinydecide::{Question, TinyDecide};

/// Same 16 cases as `systemone-spike`, so the two are directly comparable.
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

fn default_model_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/tinydecide-model"
    ))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(default_model_dir);
    let cases_path = args
        .iter()
        .find(|a| a.ends_with(".json"))
        .map(|s| s.as_str());

    println!("model dir: {:?}\n", model_dir);
    let t0 = Instant::now();
    let model = TinyDecide::load(&model_dir)
        .unwrap_or_else(|e| panic!("failed to load TinyDecide from {:?}: {e}", model_dir));
    println!("loaded in {:?}\n", t0.elapsed());

    let cases_raw = match cases_path {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {p}: {e}")),
        None => DEFAULT_CASES.to_string(),
    };
    let cases = parse_cases(&cases_raw);
    println!("{} cases\n", cases.len());

    println!(
        "{:<48} {:<10} {:<10} {:<6}",
        "text", "rule", "model", "agree"
    );
    let mut agree = 0usize;
    let mut scored = 0usize;
    let mut total_ms: f64 = 0.0;
    let mut max_ms: f64 = 0.0;
    let mut min_ms: f64 = f64::MAX;

    for case in &cases {
        let rule = teletype_core::autotext::disambiguate::entry_allowed(
            &case.text,
            case.start,
            &case.phrase,
        );
        let shown: String = case.text.chars().take(44).collect();
        let question = Question::noul(&format!(
            "The speaker wants to insert the {} symbol.",
            case.phrase
        ));
        match model.answer(&case.text, &[question]) {
            Ok(resp) => {
                scored += 1;
                let ms = resp.ms;
                total_ms += ms;
                max_ms = max_ms.max(ms);
                min_ms = min_ms.min(ms);
                let p = resp.answers.first().and_then(|a| a.p).unwrap_or(0.0);
                let model_expand = p >= 0.5;
                let ok = rule == model_expand;
                if ok {
                    agree += 1;
                }
                println!(
                    "{shown:<48} {:<10} {:<10.3} {:<6}",
                    if rule { "expand" } else { "keep" },
                    if model_expand { "expand" } else { "keep" },
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
        println!(
            "\nagreement with rule: {agree}/{scored} ({:.0}%)",
            100.0 * agree as f64 / scored as f64
        );
        println!(
            "latency (model-reported ms): avg {:.2}  min {:.2}  max {:.2}",
            total_ms / scored as f64,
            min_ms,
            max_ms
        );
    } else {
        println!("\nno case produced a model answer; nothing to score.");
    }
}
