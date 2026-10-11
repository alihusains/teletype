//! Second sweep: phrasings aimed at the two failure patterns of P10
//! (over-expanding "plus", under-expanding cue-based comma/period).
use serde_json::Value;
use std::path::PathBuf;
use tinydecide::{Question, TinyDecide};

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
    let arr: Vec<Value> = serde_json::from_str(raw).unwrap();
    arr.into_iter()
        .map(|v| {
            let text = v["text"].as_str().unwrap().to_string();
            let phrase = v["phrase"].as_str().unwrap().to_string();
            Case {
                start: text.find(&phrase).unwrap(),
                text,
                phrase,
            }
        })
        .collect()
}
type Phrasing = fn(&str) -> Question;
fn q10(phrase: &str) -> Question {
    Question::noul(&format!("The speaker wants to insert the {phrase} symbol."))
}
// A: emphasize "only if they mean the symbol, not the word"
fn qa(phrase: &str) -> Question {
    Question::noul(&format!("The speaker is using the word '{phrase}' to mean the symbol, not the ordinary English word."))
}
// B: cue-aware — mention that a command word signals intent
fn qb(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker is giving a command to type the {phrase} symbol, not talking about the word."
    ))
}
// C: binary with explicit "ordinary word" as the default
fn qc(phrase: &str) -> Question {
    Question::choice(
        &format!("In this dictation, the word '{phrase}' is:"),
        &[
            "a command to type the symbol",
            "just the normal English word",
        ],
    )
}
// D: state the disambiguation explicitly
fn qd(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker meant the symbol {phrase}, not the word {phrase}."
    ))
}
// E: "type out" verb
fn qe(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker is asking to type out the {phrase} symbol."
    ))
}
struct PhrasingResult {
    name: String,
    agree: usize,
    avg_ms: f64,
    detail: Vec<(usize, bool, bool)>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_dir = args.first().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/tinydecide-model"
        ))
    });
    let model = TinyDecide::load(&model_dir).unwrap();
    let cases = parse_cases(DEFAULT_CASES);
    let rule: Vec<bool> = cases
        .iter()
        .map(|c| teletype_core::autotext::disambiguate::entry_allowed(&c.text, c.start, &c.phrase))
        .collect();
    let phrasings: Vec<(&str, Phrasing)> = vec![
        ("P10 (baseline best)", q10),
        ("A 'means the symbol, not the ordinary word'", qa),
        ("B 'giving a command to type the {p} symbol'", qb),
        ("C choice 'command to type symbol / normal word'", qc),
        ("D 'meant the symbol {p}, not the word {p}'", qd),
        ("E 'asking to type out the {p} symbol'", qe),
    ];
    println!("{:<52} {:>6} {:>8}", "phrasing", "agree", "avg_ms");
    println!("{}", "-".repeat(68));
    let mut results: Vec<PhrasingResult> = Vec::new();
    for (name, fn_) in &phrasings {
        let mut agree = 0usize;
        let mut total_ms = 0.0f64;
        let mut detail = Vec::new();
        for (i, c) in cases.iter().enumerate() {
            let q = fn_(&c.phrase);
            let r = model.answer(&c.text, &[q]).unwrap();
            total_ms += r.ms;
            let me = match r.answers[0].kind {
                tinydecide::QType::Noul => r.answers[0].p.unwrap() >= 0.5,
                tinydecide::QType::Choice => r.answers[0].pick == Some(0),
                _ => false,
            };
            if me == rule[i] {
                agree += 1;
            }
            detail.push((i, rule[i], me));
        }
        results.push(PhrasingResult {
            name: name.to_string(),
            agree,
            avg_ms: total_ms / cases.len() as f64,
            detail,
        });
    }
    results.sort_by(|a, b| b.agree.cmp(&a.agree));
    for r in &results {
        println!(
            "{:<52} {:>3}/{} {:>7.1}",
            r.name,
            r.agree,
            cases.len(),
            r.avg_ms
        );
    }
    // Show detail for the top phrasing
    let top = &results[0];
    println!("\n=== detail: {} ({}/16) ===", top.name, top.agree);
    for (i, rule, me) in &top.detail {
        let c = &cases[*i];
        let mark = if rule == me { "  " } else { "XX" };
        println!(
            "{mark} rule={:<6} model={:<6} | {} [{}]",
            if *rule { "expand" } else { "keep" },
            if *me { "expand" } else { "keep" },
            c.text,
            c.phrase
        );
    }
}
