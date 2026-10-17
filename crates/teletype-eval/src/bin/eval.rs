//! Teletype eval driver (legacy CLI).
//!
//! Kept for the existing replay/measure workflows and docs: the flags map
//! one-to-one onto [`teletype_eval::driver::RunConfig`] and the run itself
//! lives in [`teletype_eval::driver::execute`]. For the plan §P0.4 CLI
//! (`run`/`compare` subcommands, `--set`, `--live`, `--out`) use the
//! `teletype-eval` binary instead.

use std::path::PathBuf;
use std::process::exit;

use teletype_eval::driver::{execute, RunConfig};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        exit(1);
    }
}

fn parse_args() -> Result<RunConfig, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut corpus: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut json: Option<PathBuf> = None;
    let mut thresholds: Option<PathBuf> = None;
    let mut hypotheses: Option<PathBuf> = None;
    let mut asr = "none".to_string();
    let mut model: Option<PathBuf> = None;
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        let mut next = || {
            i += 1;
            argv.get(i).cloned().ok_or(format!("missing value for {a}"))
        };
        match a {
            "--corpus" => corpus = Some(PathBuf::from(next()?)),
            "--report" => report = Some(PathBuf::from(next()?)),
            "--json" => json = Some(PathBuf::from(next()?)),
            "--thresholds" => thresholds = Some(PathBuf::from(next()?)),
            "--hypotheses" => hypotheses = Some(PathBuf::from(next()?)),
            "--asr" => asr = next()?,
            "--model" => model = Some(PathBuf::from(next()?)),
            "--offline" => {}
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    if corpus.is_none() {
        return Err("--corpus <dir> is required".into());
    }
    Ok(RunConfig {
        corpus,
        sets: Vec::new(),
        hypotheses,
        engine: asr,
        model,
        model_label: None,
        polish: "none".into(),
        live: false,
        traces: None,
        vocab: None,
        baseline: None,
        semantic: false,
        out_dir: None,
        report_md: report,
        report_json: json,
        thresholds,
        boost: false,
        models_dir: None,
    })
}

fn run() -> Result<(), String> {
    let cfg = parse_args()?;
    let out = execute(&cfg)?;

    // Legacy stdout summary (kept byte-for-byte).
    println!(
        "cases={} micro_wer={:.4} macro_wer={:.4} polish_mean={:.1}",
        out.report.wer.cases,
        out.report.wer.micro_wer,
        out.report.wer.macro_wer,
        out.report.polish_mean_score
    );
    if let Some(l) = &out.report.latency_asr {
        println!("asr   p50={:.0}ms p95={:.0}ms", l.p50, l.p95);
    }
    if let Some(l) = &out.report.latency_total {
        println!("total p50={:.0}ms p95={:.0}ms", l.p50, l.p95);
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
    Ok(())
}
