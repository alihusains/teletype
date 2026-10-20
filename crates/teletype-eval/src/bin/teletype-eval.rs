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
     teletype-eval run [--engine <catalog-id>] [--polish <id>] [--set <a,b,c> | --corpus <dir>]\n                      \
     [--hypotheses <file>] [--model <path>] [--models-dir <dir>] [--boost on|off]\n                      \
     [--live --traces <jsonl>] [--vocab <json>] [--baseline <report.json>] [--semantic]\n                      \
     [--out <dir>] [--thresholds <json>]\n  \
     teletype-eval compare <base-run-dir|report.json> <head-run-dir|report.json>\n  \
     teletype-eval compare --bake-off <run-dir|report.json>... [--baseline <report.json>] [--thresholds <json>] [--out <file.md>]\n\
     --engine: a catalog speech-model id (parakeet-tdt-v3, parakeet-ultra-coreml,\n          \
     sherpa-parakeet-v3-int8, large-v3-turbo-q5, ...) or a legacy alias\n          \
     (parakeet, parakeet-ultra, whisper). --boost on feeds the set's vocab\n          \
     terms to the engine (native boost or post-ASR rescoring); default off.\n          \
     --bake-off: write docs/decisions/D-engine-defaults.md from the given runs."
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let sub = argv.first().map(String::as_str).unwrap_or("");
    if sub == "--help" || sub == "-h" || sub == "help" {
        println!("{}", usage());
        return Ok(());
    }
    match sub {
        "compare" => {
            if let Some(pos) = argv.iter().position(|a| a == "--bake-off") {
                return run_bakeoff(&argv[pos + 1..]);
            }
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
                boost: false,
                models_dir: None,
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
                    "--models-dir" => cfg.models_dir = Some(PathBuf::from(next()?)),
                    "--boost" => {
                        let v = next()?;
                        cfg.boost = match v.as_str() {
                            "on" => true,
                            "off" => false,
                            other => return Err(format!("--boost wants on|off, got {other:?}")),
                        };
                    }
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
                if !out.breaches.is_empty() {
                    for b in &out.breaches {
                        eprintln!("THRESHOLD BREACH: {b}");
                    }
                    eprintln!("threshold gate: FAIL ({} breach(es))", out.breaches.len());
                    exit(1);
                } else if out.gate_checks == 0 {
                    // Every check was SKIPPED (no baseline, no vocab terms,
                    // no traces, no clean human refs). "PASS" with zero
                    // realized checks is a claim the run cannot support.
                    eprintln!(
                        "threshold gate: INSTRUMENT_INVALID \
                         (0 checks had data; see gate notes above)"
                    );
                    exit(3);
                } else {
                    println!("threshold gate: PASS");
                }
            }
            if !out.skipped_sets.is_empty() {
                eprintln!("skipped sets: {}", out.skipped_sets.join(", "));
            }
            if !out.report.case_errors.is_empty() {
                eprintln!(
                    "{} case(s) failed to measure; see case_errors in report.json",
                    out.report.case_errors.len()
                );
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

/// `teletype-eval compare --bake-off`: read the per-engine run reports and
/// write the §3.8 decision file `docs/decisions/D-engine-defaults.md`.
fn run_bakeoff(args: &[String]) -> Result<(), String> {
    let mut cells: Vec<String> = Vec::new();
    let mut baseline: Option<String> = None;
    let mut out: Option<String> = None;
    let mut thresholds: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut next = || {
            i += 1;
            args.get(i).cloned().ok_or(format!("missing value for {a}"))
        };
        match a {
            "--baseline" => baseline = Some(next()?),
            "--out" => out = Some(next()?),
            "--thresholds" => thresholds = Some(next()?),
            other if other.starts_with('-') => {
                return Err(format!("unknown --bake-off argument: {other}\n{}", usage()))
            }
            other => cells.push(other.to_string()),
        }
        i += 1;
    }
    if cells.is_empty() {
        return Err(format!(
            "compare --bake-off wants at least one run dir or report.json\n{}",
            usage()
        ));
    }

    // Baseline: explicit --baseline wins; else the committed Gate-0
    // baseline; else the report with the lowest verbatim WER among the
    // cells (so a bake-off run on a new baseline still gets deltas).
    let (baseline_vw, baseline_name) = if let Some(b) = &baseline {
        let r = load_report_value(b)?;
        (
            r.get("wer_verbatim_micro").and_then(|v| v.as_f64()),
            Some(b.clone()),
        )
    } else {
        let committed = "evals/runs/baseline/report.json";
        if std::path::Path::new(committed).is_file() {
            if let Ok(r) = load_report_value(committed) {
                (
                    r.get("wer_verbatim_micro").and_then(|v| v.as_f64()),
                    Some(committed.into()),
                )
            } else {
                (None, Some(committed.into()))
            }
        } else {
            (None, None)
        }
    };
    let cells: Vec<teletype_eval::bakeoff::BakeoffCell> = cells
        .iter()
        .map(|c| teletype_eval::bakeoff::load_cell(c, baseline_vw, baseline_name.clone()))
        .collect::<Result<_, _>>()?;

    let bounds = match &thresholds {
        Some(t) => {
            let content = std::fs::read_to_string(t)
                .map_err(|e| format!("cannot read thresholds {}: {e}", t))?;
            let v: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("malformed thresholds {}: {e}", t))?;
            teletype_eval::bakeoff::BakeoffBounds::from_json(&v)
        }
        None => teletype_eval::bakeoff::BakeoffBounds::default(),
    };
    let md = teletype_eval::bakeoff::decision_markdown(
        &cells,
        &bounds,
        baseline_name.as_deref().unwrap_or("n/a"),
    );
    match out {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }
            }
            std::fs::write(&path, &md).map_err(|e| format!("cannot write {path}: {e}"))?;
            eprintln!("wrote decision file to {path}");
            print!("{md}");
        }
        None => print!("{md}"),
    }
    Ok(())
}

fn load_report_value(path: &str) -> Result<serde_json::Value, String> {
    let p = std::path::Path::new(path);
    let json = if p.is_dir() {
        p.join("report.json")
    } else {
        p.to_path_buf()
    };
    let content = std::fs::read_to_string(&json)
        .map_err(|e| format!("cannot read {}: {e}", json.display()))?;
    serde_json::from_str(&content).map_err(|e| format!("malformed {}: {e}", json.display()))
}
