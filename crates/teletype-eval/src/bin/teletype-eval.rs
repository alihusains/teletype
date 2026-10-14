//! `teletype-eval`: the plan §P0.4 eval CLI.
//!
//! ```text
//! teletype-eval run --engine parakeet-ultra --polish s1-mini \
//!     --set human,librispeech,earnings22-vocab,ami,cv,noise --live \
//!     --out evals/runs/<git-sha>/
//! teletype-eval compare evals/runs/<base> evals/runs/<head>
//! ```
//!
//! `run` shares the scoring core with the legacy `eval` binary (see
//! `teletype_eval::driver`); only the argument surface is new.

use std::path::PathBuf;
use std::process::exit;

use teletype_eval::driver::{execute, RunConfig};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        exit(1);
    }
}

fn usage() -> &'static str {
    "usage:\n  \
     teletype-eval run [--engine <id>] [--polish <id>] [--set <a,b,c> | --corpus <dir>]\n                      \
     [--hypotheses <file>] [--model <path>] [--live --traces <jsonl>]\n                      \
     [--vocab <json>] [--baseline <report.json>] [--semantic]\n                      \
     [--out <dir>] [--thresholds <json>]\n  \
     teletype-eval compare <base-run-dir|report.json> <head-run-dir|report.json>"
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let sub = argv.first().map(String::as_str).unwrap_or("");
    match sub {
        "compare" => {
            if argv.len() != 3 {
                return Err(format!("compare wants exactly 2 runs\n{0}", usage()));
            }
            let md = teletype_eval::compare::compare(&argv[1], &argv[2])?;
            print!("{md}");
            Ok(())
        }
        "run" => {
            let mut cfg = RunConfig {
                corpus: None,
                sets: Vec::new(),
                hypotheses: None,
                engine: "none".into(),
                model: None,
                model_label: None,
                polish: "none".into(),
                live: false,
                traces: None,
                vocab: None,
                baseline: None,
                semantic: false,
                out_dir: None,
                report_md: None,
                report_json: None,
                thresholds: None,
            };
            let mut i = 1;
            while i < argv.len() {
                let a = argv[i].as_str();
                let mut next = || {
                    i += 1;
                    argv.get(i).cloned().ok_or(format!("missing value for {a}"))
                };
                match a {
                    "--engine" => {
                        cfg.model_label = Some(next()?);
                        cfg.engine = cfg.model_label.clone().unwrap();
                    }
                    "--polish" => cfg.polish = next()?,
                    "--set" => {
                        cfg.sets = next()?
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "--corpus" => cfg.corpus = Some(PathBuf::from(next()?)),
                    "--hypotheses" => cfg.hypotheses = Some(PathBuf::from(next()?)),
                    "--model" => cfg.model = Some(PathBuf::from(next()?)),
                    "--live" => cfg.live = true,
                    "--traces" => cfg.traces = Some(PathBuf::from(next()?)),
                    "--vocab" => cfg.vocab = Some(PathBuf::from(next()?)),
                    "--baseline" => cfg.baseline = Some(PathBuf::from(next()?)),
                    "--semantic" => cfg.semantic = true,
                    "--out" => cfg.out_dir = Some(PathBuf::from(next()?)),
                    "--thresholds" => cfg.thresholds = Some(PathBuf::from(next()?)),
                    "--report" => cfg.report_md = Some(PathBuf::from(next()?)),
                    "--json" => cfg.report_json = Some(PathBuf::from(next()?)),
                    other => return Err(format!("unknown argument: {other}\n{}", usage())),
                }
                i += 1;
            }
            // `--engine none` without hypotheses is a driver error with a
            // clear message; surface it unchanged.
            let out = execute(&cfg)?;

            println!(
                "cases={} verbatim_micro={} clean_micro={} vocab_recall={} entity={} polish_mean={:.1}",
                out.report.wer.cases,
                fmt4(out.report.wer_verbatim_micro),
                fmt4(out.report.wer_clean_micro),
                fmt4(out.report.vocab_recall),
                fmt4(out.report.entity_accuracy),
                out.report.polish_mean_score,
            );
            if let Some(l) = &out.report.latency_release_to_paste {
                println!(
                    "release→paste p50={:.0}ms p95={:.0}ms p99={:.0}ms",
                    l.p50, l.p95, l.p99
                );
            }
            for note in &out.gate_notes {
                println!("gate note: {note}");
            }
            if cfg.thresholds.is_some() {
                if out.breaches.is_empty() {
                    println!("threshold gate: PASS");
                } else {
                    for b in &out.breaches {
                        eprintln!("THRESHOLD BREACH: {b}");
                    }
                    eprintln!("threshold gate: FAIL ({} breach(es))", out.breaches.len());
                    exit(1);
                }
            }
            if !out.skipped_sets.is_empty() {
                eprintln!("skipped sets: {}", out.skipped_sets.join(", "));
            }
            Ok(())
        }
        _ => Err(usage().into()),
    }
}

fn fmt4(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.4}"),
        None => "n/a".into(),
    }
}
