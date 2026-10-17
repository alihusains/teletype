//! Conformance against the reference engine (tinydecide.js), fixture in ../conformance.
//!   cargo test --release -- --nocapture
//! Model directories: env TINYDECIDE_S768 / TINYDECIDE_FULL; by default .. and ../full inside the
//! Hugging Face repo (rust/ next to model.bin), else ../../web_S768 and ../../web_fc.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tinydecide::corrections::{make_protos, Example};
use tinydecide::{Answer, Protos, QType, Question, TinyDecide};

const TOL: f64 = 1e-3;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// env var, else `hf` when the crate sits inside the Hugging Face repo, else `dev` (this repo's exports)
fn model_dir(var: &str, hf: &str, dev: &str) -> PathBuf {
    if let Ok(d) = std::env::var(var) {
        return PathBuf::from(d);
    }
    if root().join("../model.bin").exists() {
        root().join(hf)
    } else {
        root().join(dev)
    }
}

fn read_json(p: &Path) -> Value {
    let s = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

fn fv(v: &Value) -> Vec<f64> {
    v.as_array()
        .map(|a| a.iter().map(f).collect())
        .unwrap_or_default()
}

#[derive(Default)]
struct Tally {
    tok_bad: usize,
    req: usize,
    id_bad: usize,
    trunc_bad: usize,
    err_bad: usize,
    answers: usize,
    pick_bad: usize,
    span_bad: usize,
    max_dp: f64,
    max_dq: f64,
    max_dz: f64,
    protos_bad: usize,
    lam_bad: usize,
    max_dproto: f64,
}

impl Tally {
    fn upd(m: &mut f64, a: f64, b: f64) {
        let d = (a - b).abs();
        if d.is_nan() || d > *m {
            *m = if d.is_nan() { f64::INFINITY } else { d };
        }
    }

    fn answer(&mut self, name: &str, e: &Value, a: &Answer) {
        self.answers += 1;
        let t = e["type"].as_str().unwrap();
        assert_eq!(t, a.kind.as_str(), "{name}: answer type");
        match a.kind {
            QType::Choice | QType::Score => {
                let ep = fv(&e["probs"]);
                if ep.len() != a.probs.len() {
                    self.pick_bad += 1;
                    return;
                }
                for (x, y) in ep.iter().zip(&a.probs) {
                    Self::upd(&mut self.max_dp, *x, *y);
                }
                Self::upd(&mut self.max_dp, f(&e["confidence"]), a.confidence.unwrap());
                if a.kind == QType::Score {
                    Self::upd(&mut self.max_dp, f(&e["score"]), a.score.unwrap());
                }
                if e["pick"].as_u64().map(|p| p as usize) != a.pick {
                    self.pick_bad += 1;
                    println!(
                        "  pick differs in {name}: js {} rust {:?}",
                        e["pick"], a.pick
                    );
                }
            }
            QType::Noul => Self::upd(&mut self.max_dp, f(&e["p"]), a.p.unwrap()),
            QType::Span => {
                Self::upd(&mut self.max_dp, f(&e["p_present"]), a.p_present.unwrap());
                Self::upd(&mut self.max_dp, f(&e["p_span"]), a.p_span.unwrap());
                let tok = (
                    e["tok"][0].as_u64().unwrap() as usize,
                    e["tok"][1].as_u64().unwrap() as usize,
                );
                let text = e["text"].as_str().unwrap_or("");
                if Some(tok) != a.tok || Some(text) != a.text.as_deref() {
                    self.span_bad += 1;
                    println!(
                        "  span differs in {name}: js {tok:?} {text:?} rust {:?} {:?}",
                        a.tok, a.text
                    );
                }
            }
        }
        if a.kind != QType::Span {
            for (x, y) in fv(&e["qvec"]).iter().zip(&a.qvec) {
                Self::upd(&mut self.max_dq, *x, *y as f64);
            }
            for (x, y) in fv(&e["z0"]).iter().zip(&a.z0) {
                Self::upd(&mut self.max_dz, *x, *y);
            }
        }
    }

    fn passed(&self) -> bool {
        self.tok_bad == 0
            && self.id_bad == 0
            && self.trunc_bad == 0
            && self.err_bad == 0
            && self.pick_bad == 0
            && self.span_bad == 0
            && self.protos_bad == 0
            && self.lam_bad == 0
            && self.max_dp <= TOL
            && self.max_dq <= TOL
            && self.max_dz <= TOL
            && self.max_dproto <= 1e-5
    }
}

fn ids_of(v: &Value) -> Vec<u32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u32)
        .collect()
}

fn run_build(build: &str, dir: &Path, cases: &Value) -> bool {
    let exp = read_json(&root().join(format!("../conformance/expected.{build}.json")));
    let model = TinyDecide::load(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut t = Tally::default();

    // tokenizer
    let strings = cases["tokenizer"].as_array().unwrap();
    assert_eq!(
        strings.len(),
        exp["tokenizer"].as_array().unwrap().len(),
        "tokenizer case count"
    );
    assert_eq!(
        cases["requests"].as_array().unwrap().len(),
        exp["requests"].as_array().unwrap().len(),
        "request count"
    );
    assert_eq!(
        cases["protos"].as_array().unwrap().len(),
        exp["protos"].as_array().unwrap().len(),
        "protos case count"
    );
    for (s, e) in strings.iter().zip(exp["tokenizer"].as_array().unwrap()) {
        let s = s.as_str().unwrap();
        let got = model.tokenizer().encode(s).ids;
        if got != ids_of(e) {
            t.tok_bad += 1;
            if t.tok_bad <= 5 {
                println!(
                    "  tokenizer differs: {s:?}\n    js   {:?}\n    rust {got:?}",
                    ids_of(e)
                );
            }
        }
    }

    // requests
    let mut ms = 0.0;
    let mut timed = 0;
    for (c, e) in cases["requests"]
        .as_array()
        .unwrap()
        .iter()
        .zip(exp["requests"].as_array().unwrap())
    {
        t.req += 1;
        let name = c["name"].as_str().unwrap();
        assert_eq!(Some(name), e["name"].as_str());
        let qs: Vec<Question> = serde_json::from_value(c["questions"].clone()).unwrap();
        let got = model.answer(c["state"].as_str().unwrap(), &qs);
        if let Some(msg) = e.get("error").and_then(|m| m.as_str()) {
            match got {
                Err(err) if err.to_string() == msg => {}
                other => {
                    t.err_bad += 1;
                    println!(
                        "  {name}: expected error {msg:?}, got {:?}",
                        other.map(|r| r.tokens)
                    );
                }
            }
            continue;
        }
        let r = match got {
            Ok(r) => r,
            Err(err) => {
                t.err_bad += 1;
                println!("  {name}: unexpected error {err}");
                continue;
            }
        };
        ms += r.ms;
        timed += 1;
        if r.ids != ids_of(&e["ids"]) {
            t.id_bad += 1;
            println!("  ids differ in {name}");
            continue;
        }
        if Some(r.truncated) != e["truncated"].as_bool() {
            t.trunc_bad += 1;
        }
        assert_eq!(
            r.answers.len(),
            e["answers"].as_array().unwrap().len(),
            "{name}: answer count"
        );
        for (ea, a) in e["answers"].as_array().unwrap().iter().zip(&r.answers) {
            t.answer(name, ea, a);
        }
    }

    // corrections
    let beta = model.beta().to_vec();
    for (c, e) in cases["protos"]
        .as_array()
        .unwrap()
        .iter()
        .zip(exp["protos"].as_array().unwrap())
    {
        let name = c["name"].as_str().unwrap();
        let q: Question = serde_json::from_value(c["question"].clone()).unwrap();
        let lists: Vec<Vec<Example>> = serde_json::from_value(e["examples"].clone()).unwrap();
        let mut p: Protos = make_protos(q.kind, &lists, &beta, None).expect("protos");
        if c["no_center"].as_bool() == Some(true) {
            p.center = None;
        }
        let ep = &e["protos"];
        let cnt: Vec<i32> = ep["cnt"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_i64().unwrap() as i32)
            .collect();
        if cnt != p.cnt || fv(&ep["vec"]).len() != p.vec.len() {
            t.protos_bad += 1;
        }
        for (x, y) in fv(&ep["vec"]).iter().zip(&p.vec) {
            Tally::upd(&mut t.max_dproto, *x, *y as f64);
        }
        if let Some(c) = &p.center {
            for (x, y) in fv(&ep["center"]).iter().zip(c) {
                Tally::upd(&mut t.max_dproto, *x, *y as f64);
            }
        } else if !ep["center"].is_null() {
            t.protos_bad += 1;
        }
        if ep["lam"].as_f64() != p.lam {
            t.lam_bad += 1;
            println!("  lam differs in {name}: js {} rust {:?}", ep["lam"], p.lam);
        }
        let r = model
            .answer_with(
                c["state"].as_str().unwrap(),
                std::slice::from_ref(&q),
                &[Some(p)],
            )
            .unwrap();
        if r.ids != ids_of(&e["ids"]) {
            t.id_bad += 1;
        }
        assert_eq!(
            r.answers.len(),
            e["answers"].as_array().unwrap().len(),
            "{name}: answer count"
        );
        for (ea, a) in e["answers"].as_array().unwrap().iter().zip(&r.answers) {
            t.answer(name, ea, a);
        }
    }

    let ok = t.passed();
    println!(
        "{build}: {} {} tokenizer strings ({} mismatches), {} requests ({} id, {} truncated, {} error mismatches), \
         {} answers ({} pick, {} span mismatches), max|dp| {:.2e}, max|dqvec| {:.2e}, max|dz0| {:.2e}, \
         {} protos cases ({} protos, {} lam mismatches, max|dvec| {:.2e}), avg {:.0} ms/request",
        if ok { "PASS" } else { "FAIL" },
        strings.len(),
        t.tok_bad,
        t.req,
        t.id_bad,
        t.trunc_bad,
        t.err_bad,
        t.answers,
        t.pick_bad,
        t.span_bad,
        t.max_dp,
        t.max_dq,
        t.max_dz,
        cases["protos"].as_array().unwrap().len(),
        t.protos_bad,
        t.lam_bad,
        t.max_dproto,
        if timed > 0 { ms / timed as f64 } else { 0.0 },
    );
    ok
}
/// These tests compare against the reference engine's outputs; the fixture
/// files and model builds ship with the upstream TinyDecide repo, not with
/// this vendored copy (see README). Return true (and print SKIP) when any
/// required artifact is absent so fresh checkouts stay green.
fn skip_if_artifacts_missing() -> bool {
    let conf = root().join("../conformance");
    let s768 = model_dir("TINYDECIDE_S768", "..", "../../web_S768");
    let full = model_dir("TINYDECIDE_FULL", "../full", "../../web_fc");
    let has = |d: &Path| d.join("meta.json").exists() && d.join("model.bin").exists();
    if !conf.join("cases.json").exists()
        || !conf.join("expected.S768.json").exists()
        || !conf.join("expected.full.json").exists()
        || !has(&s768)
        || !has(&full)
    {
        eprintln!("SKIP: conformance fixtures or model builds absent (upstream-only artifacts); see crates/tinydecide/README.md");
        return true;
    }
    false
}

#[test]
fn conformance() {
    if skip_if_artifacts_missing() {
        return;
    }
    let cases = read_json(&root().join("../conformance/cases.json"));
    let s768 = model_dir("TINYDECIDE_S768", "..", "../../web_S768");
    let full = model_dir("TINYDECIDE_FULL", "../full", "../../web_fc");
    let a = run_build("S768", &s768, &cases);
    let b = run_build("full", &full, &cases);
    assert!(a && b, "conformance failed (see the lines above)");
}

#[test]
fn bad_input_is_an_error() {
    if skip_if_artifacts_missing() {
        return;
    }
    let dir = model_dir("TINYDECIDE_S768", "..", "../../web_S768");
    let model = TinyDecide::load(dir).unwrap();
    assert!(model
        .answer("hi", &[Question::choice("Pick one.", &[] as &[&str])])
        .is_err());
    let p = Protos {
        vec: vec![0.0; 3],
        cnt: vec![1, 1],
        center: None,
        lam: None,
    };
    assert!(model
        .answer_with("hi", &[Question::noul("It is a test.")], &[Some(p)])
        .is_err());
    assert!(TinyDecide::from_bytes("{}", &[]).is_err());
    let dir = model_dir("TINYDECIDE_S768", "..", "../../web_S768");
    let meta = std::fs::read_to_string(dir.join("meta.json")).unwrap();
    let bin = std::fs::read(dir.join("model.bin")).unwrap();
    assert!(TinyDecide::from_bytes(&meta, &bin[..bin.len() / 2]).is_err());
    assert!(TinyDecide::load("/nonexistent").is_err());
}

#[test]
fn encode_until_is_a_prefix() {
    if skip_if_artifacts_missing() {
        return;
    }
    let cases = read_json(&root().join("../conformance/cases.json"));
    let dir = model_dir("TINYDECIDE_S768", "..", "../../web_S768");
    let tok = TinyDecide::load(dir).unwrap().tokenizer().clone();
    let mut texts: Vec<String> = cases["tokenizer"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    texts.extend(
        cases["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["state"].as_str().unwrap().to_string()),
    );
    texts.push("word ".repeat(400));
    texts.push("x".repeat(5000) + " tail");
    texts.push("東京 ".repeat(100));
    for t in &texts {
        let full = tok.encode(t);
        for limit in [0, 1, 2, 5, 127, usize::MAX] {
            let got = tok.encode_until(t, limit);
            let n = got.ids.len();
            assert_eq!(
                &got.ids[..],
                &full.ids[..n],
                "{t:?} limit {limit}: not a prefix"
            );
            assert_eq!(
                &got.offsets[..],
                &full.offsets[..n],
                "{t:?} limit {limit}: offsets"
            );
            assert_eq!(
                n > limit,
                full.ids.len() > limit,
                "{t:?} limit {limit}: truncation"
            );
            if n <= limit {
                assert_eq!(n, full.ids.len(), "{t:?} limit {limit}: stopped early");
            }
        }
    }
}

#[test]
fn make_protos_rejects_mismatched_lengths() {
    let beta = [0.5, 0.6, 0.7, 0.8, 0.9];
    let ex = |n: usize| Example {
        v: vec![0.5; n],
        z: vec![0.1, 0.2],
    };
    let lists = vec![vec![ex(4), ex(4)], vec![ex(4)]];
    assert!(make_protos(QType::Choice, &lists, &beta, None).is_some());
    assert!(make_protos(QType::Choice, &lists, &beta, Some(&[0.0; 2])).is_none());
    assert!(make_protos(
        QType::Choice,
        &[vec![ex(4), ex(6)], vec![ex(4)]],
        &beta,
        None
    )
    .is_none());
}
