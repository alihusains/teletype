// claim: polish-quality
//! Adversarial, corpus-driven coverage for the deterministic polish rubric.
//!
//! Loads `evals/polish-adversarial/cases.jsonl` (or `$POLISH_ADVERSARIAL_CASES`)
//! and scores each case with `teletype_eval::polish::score_polish`, the exact
//! CI judge. The API shape pinned here:
//!
//!   fn score_polish(reference: &str, polished: &str) -> PolishVerdict
//!   pub struct PolishVerdict {
//!       pub checks: Vec<Check { name: String, passed: bool }>,
//!       pub score: u32,        // 0-100 weighted sum of passing checks
//!       pub failed: Vec<String>, // names of the failing checks
//!   }
//!
//! JSONL schema, one object per line:
//!   {"id": "...", "reference": "...", "polished": "...",
//!    "expect": {"score": <u32>, "failed": ["check", ...]},
//!    "xfail": "reason (optional)"}
//!
//! `expect.score` is optional (omit to pin only the failed list). `expect.failed`
//! is the expected exact set of failing check names (order-insensitive).
//!
//! Every case pins the *observed* rubric behavior (including case 08, which
//! pins a known bug). xfail is an annotation on such a pin: while the
//! observation still matches the pin, the case reports XFAIL (expected,
//! counted, not a failure). If the observation changes — most importantly
//! when the annotated bug is fixed — the case reports XPASS and FAILS the
//! suite, so the pin gets updated (set the new expected values, drop the
//! xfail). Non-xfail cases must always match their pin.
//!
//! The corpus must contain at least 22 cases; the guard below is part of the
//! test, so a silently-truncated corpus fails CI.

use std::env;
use std::path::PathBuf;
use std::process::exit;

use serde_json::Value;
use teletype_eval::polish::score_polish;

fn cases_path() -> PathBuf {
    match env::var_os("POLISH_ADVERSARIAL_CASES") {
        Some(p) => {
            let pb = PathBuf::from(p);
            if pb.is_absolute() {
                pb
            } else {
                // Relative overrides resolve against the repo root, not the
                // crate dir (integration tests run with CARGO_MANIFEST_DIR).
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(pb)
            }
        }
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("evals/polish-adversarial/cases.jsonl"),
    }
}

#[test]
fn polish_adversarial_corpus() {
    let path = cases_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read cases file {}: {e}", path.display()));

    let mut cases: Vec<Value> = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("{}: line {}: bad JSON: {e}", path.display(), idx + 1));
        for field in ["id", "reference", "polished", "expect"] {
            assert!(v.get(field).is_some(), "line {}: missing {field}", idx + 1);
        }
        cases.push(v);
    }

    assert!(
        cases.len() >= 22,
        "cases len guard: corpus has {} cases, need >= 22 (truncated or corrupted?)",
        cases.len()
    );

    let mut pass = 0;
    let mut xfail = 0;
    let mut bad: Vec<String> = Vec::new();

    for c in &cases {
        let id = c["id"].as_str().unwrap().to_string();
        let reference = c["reference"].as_str().unwrap().to_string();
        let polished = c["polished"].as_str().unwrap().to_string();
        let xfail_reason = c.get("xfail").and_then(|x| x.as_str());
        let verdict = score_polish(&reference, &polished);

        let expected_failed: Vec<String> = c["expect"]["failed"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();
        let expected_score = c["expect"].get("score").and_then(|s| s.as_u64());
        // `failed` compares as a set: the check order is an implementation
        // detail, the set of failing checks is the claim (documented above).
        let mut got_failed = verdict.failed.clone();
        let mut want_failed = expected_failed.clone();
        got_failed.sort();
        want_failed.sort();
        let case_ok = got_failed == want_failed
            && expected_score.is_none_or(|es| verdict.score == es as u32);

        // xfail pins a known-bug behavior: matching the pin is the expected
        // (green) outcome; a changed observation is the alert.
        let status = if xfail_reason.is_some() {
            if case_ok {
                xfail += 1;
                "XFAIL"
            } else {
                "XPASS" // pinned known-bug behavior changed: update the pin
            }
        } else if case_ok {
            pass += 1;
            "PASS"
        } else {
            "FAIL"
        };

        println!("{status:<6} {id}  score={} failed={:?}", verdict.score, verdict.failed);

        if status == "FAIL" || status == "XPASS" {
            bad.push(format!(
                "{status} {id}: observed score={} failed={:?}; expected score={} failed={:?}{}",
                verdict.score,
                verdict.failed,
                c["expect"].get("score").map(|v| v.to_string()).unwrap_or_default(),
                expected_failed,
                xfail_reason
                    .map(|r| format!(" [xfail: {r}]"))
                    .unwrap_or_default()
            ));
        }
    }

    println!(
        "polish-adversarial summary: {} cases, {} pass, {} xfail, {} fail/xpass",
        cases.len(),
        pass,
        xfail,
        bad.len()
    );

    if !bad.is_empty() {
        for b in &bad {
            eprintln!("{b}");
        }
        exit(1);
    }
}
