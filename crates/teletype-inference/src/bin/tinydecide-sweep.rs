//! TinyDecide prompt-phrasing sweep: test many question phrasings against the
//! 16 AutoText cases, all in one process (model loaded once), and report which
//! phrasing agrees with the deterministic rule the most.
//!
//! Usage: cargo run --release -p teletype-inference --bin tinydecide-sweep [model-dir]

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
            let start = text.find(&phrase).unwrap();
            Case {
                text,
                phrase,
                start,
            }
        })
        .collect()
}

type Phrasing = fn(&str) -> Question; // phrase is interpolated inside

fn p1_short_noul(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker is asking to type the symbol {phrase}."
    ))
}
fn p2_long_noul(phrase: &str) -> Question {
    Question::noul(&format!(
        "In this dictation, is the speaker asking to TYPE the character or symbol named '{phrase}' (a symbol request), rather than using '{phrase}' as an ordinary English word?"
    ))
}
fn p3_choice(phrase: &str) -> Question {
    Question::choice(
        &format!("In the utterance, does the word '{phrase}' refer to a request to type that symbol, or is it just the ordinary English word?"),
        &["symbol request", "ordinary word"],
    )
}
fn p4_noul_named(phrase: &str) -> Question {
    Question::noul(&format!(
        "The word '{phrase}' in the utterance is a request to insert that symbol."
    ))
}
fn p5_noul_type(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker wants the symbol {phrase} inserted into the text."
    ))
}
fn p6_noul_character(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker is dictating the character {phrase} to be typed."
    ))
}
fn p7_choice_short(phrase: &str) -> Question {
    Question::choice(
        &format!("Is '{phrase}' a symbol request or an ordinary word?"),
        &["symbol request", "ordinary word"],
    )
}
fn p8_noul_punct(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker is asking for the punctuation mark {phrase}."
    ))
}
fn p9_noul_say(phrase: &str) -> Question {
    Question::noul(&format!(
        "The speaker said the word {phrase} meaning they want that symbol typed."
    ))
}
fn p10_noul_insert(phrase: &str) -> Question {
    Question::noul(&format!("The speaker wants to insert the {phrase} symbol."))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_dir = args.first().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/tinydecide-model"
        ))
    });

    let model = TinyDecide::load(&model_dir)
        .unwrap_or_else(|e| panic!("failed to load from {:?}: {e}", model_dir));

    let cases = parse_cases(DEFAULT_CASES);
    // Precompute the rule verdict for each case.
    let rule: Vec<bool> = cases
        .iter()
        .map(|c| teletype_core::autotext::disambiguate::entry_allowed(&c.text, c.start, &c.phrase))
        .collect();

    let phrasings: Vec<(&str, Phrasing)> = vec![
        (
            "P1 short noul: 'asking to type the symbol {p}'",
            p1_short_noul,
        ),
        (
            "P2 long noul: 'TYPE the character... rather than ordinary word'",
            p2_long_noul,
        ),
        (
            "P3 choice: 'symbol request or ordinary word?' (long)",
            p3_choice,
        ),
        (
            "P4 noul named: 'is a request to insert that symbol'",
            p4_noul_named,
        ),
        (
            "P5 noul type: 'wants the symbol {p} inserted'",
            p5_noul_type,
        ),
        (
            "P6 noul char: 'dictating the character {p} to be typed'",
            p6_noul_character,
        ),
        (
            "P7 choice short: 'Is {p} a symbol request or ordinary word?'",
            p7_choice_short,
        ),
        (
            "P8 noul punct: 'asking for the punctuation mark {p}'",
            p8_noul_punct,
        ),
        (
            "P9 noul say: 'said {p} meaning they want that symbol typed'",
            p9_noul_say,
        ),
        (
            "P10 noul insert: 'wants to insert the {p} symbol'",
            p10_noul_insert,
        ),
    ];

    println!("{} cases, {} phrasings\n", cases.len(), phrasings.len());
    println!("{:<58} {:>6} {:>8}", "phrasing", "agree", "avg_ms");
    println!("{}", "-".repeat(72));

    let mut results: Vec<(String, usize, f64)> = Vec::new();

    for (name, fn_) in &phrasings {
        let mut agree = 0usize;
        let mut total_ms = 0.0f64;
        for (i, c) in cases.iter().enumerate() {
            let q = fn_(&c.phrase);
            let r = model.answer(&c.text, &[q]).unwrap();
            total_ms += r.ms;
            // noul: p >= 0.5 means "yes, symbol request" = expand
            // choice: pick == 0 means "symbol request" = expand
            let model_expand = match r.answers[0].kind {
                tinydecide::QType::Noul => r.answers[0].p.unwrap() >= 0.5,
                tinydecide::QType::Choice => r.answers[0].pick == Some(0),
                _ => false,
            };
            if model_expand == rule[i] {
                agree += 1;
            }
        }
        let avg_ms = total_ms / cases.len() as f64;
        results.push((name.to_string(), agree, avg_ms));
        println!("{:<58} {:>3}/{} {:>7.1}", name, agree, cases.len(), avg_ms);
    }

    // Rank by agreement, then by speed.
    results.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.partial_cmp(&a.2).unwrap()));
    println!("\n=== ranked by agreement ===");
    for (name, agree, ms) in &results {
        println!("{:<58} {:>3}/{}  {:>6.1} ms", name, agree, cases.len(), ms);
    }
}
