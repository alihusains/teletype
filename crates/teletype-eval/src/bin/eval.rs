//! Teletype eval driver.
//!
//! Two modes:
//!
//! - **Replay** (`--hypotheses <file>`): reads pre-recorded ASR output per
//!   case id from a JSONL file. WER, polish scoring, latency percentiles,
//!   the report, and the threshold gate all execute with no model, no
//!   network, and no download. This is the CI path.
//!
//! - **Measure** (`--asr parakeet --model <path>`): loads the real model
//!   through `teletype-speech`, decodes each case, and times it. This is
//!   what produces numbers you can quote.
//!
//! `--asr none` without `--hypotheses` exits non-zero with a clear message.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Instant;

use teletype_eval::corpus::Corpus;
use teletype_eval::latency::{LatencySummary, Sample};
use teletype_eval::polish::{Judge, RuleJudge};
use teletype_eval::report::{CasePolish, CaseResult, EvalReport};
use teletype_eval::wer::{self, word_error_rate};

#[derive(Debug)]
struct Args {
    corpus: PathBuf,
    report: Option<PathBuf>,
    json: Option<PathBuf>,
    thresholds: Option<PathBuf>,
    hypotheses: Option<PathBuf>,
    asr: String,
    model: Option<PathBuf>,
    offline: bool,
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = Args {
        corpus: PathBuf::new(),
        report: None,
        json: None,
        thresholds: None,
        hypotheses: None,
        asr: "none".into(),
        model: None,
        offline: false,
    };
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        let mut next = || {
            i += 1;
            argv.get(i).cloned().ok_or(format!("missing value for {a}"))
        };
        match a {
            "--corpus" => args.corpus = PathBuf::from(next()?),
            "--report" => args.report = Some(PathBuf::from(next()?)),
            "--json" => args.json = Some(PathBuf::from(next()?)),
            "--thresholds" => args.thresholds = Some(PathBuf::from(next()?)),
            "--hypotheses" => args.hypotheses = Some(PathBuf::from(next()?)),
            "--asr" => args.asr = next()?,
            "--model" => args.model = Some(PathBuf::from(next()?)),
            "--offline" => args.offline = true,
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    if args.corpus.as_os_str().is_empty() {
        return Err("--corpus <dir> is required".into());
    }
    Ok(args)
}

/// One row of the hypotheses file (replay mode).
#[derive(serde::Deserialize)]
struct HypothesisRow {
    id: String,
    hypothesis: String,
    #[serde(default)]
    polished: Option<String>,
    #[serde(default)]
    asr_ms: Option<f64>,
    #[serde(default)]
    total_ms: Option<f64>,
}

#[derive(serde::Deserialize)]
struct Thresholds {
    max_micro_wer: f64,
    max_p95_asr_ms: f64,
    max_p95_total_ms: f64,
    min_mean_polish_score: f64,
}

fn load_thresholds(path: &Path) -> Result<Thresholds, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read thresholds file {}: {e}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("malformed thresholds file {}: {e}", path.display()))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;

    if args.asr == "none" && args.hypotheses.is_none() {
        return Err(
            "no hypotheses file: --asr none requires --hypotheses <file> \
             (replay mode). Pass --asr parakeet --model <path> for a real run."
                .into(),
        );
    }

    let corpus = Corpus::load(&args.corpus).map_err(|e| format!("corpus: {e}"))?;
    eprintln!(
        "loaded {} cases from {}",
        corpus.cases.len(),
        args.corpus.display()
    );

    let git_sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "0".into());

    let mut case_results: Vec<CaseResult> = Vec::new();
    // (id, reference, hypothesis, wer) — hypothesis feeds the worst-cases
    // table in the report.
    let mut wer_cases: Vec<(String, String, String, wer::Wer)> = Vec::new();
    // (id, reference, tags, wer) for the per-tag WER grouping.
    let mut tag_cases: Vec<(String, String, Vec<String>, wer::Wer)> = Vec::new();
    let mut polish_results: Vec<CasePolish> = Vec::new();
    let mut asr_samples: Vec<Sample> = Vec::new();
    let mut total_samples: Vec<Sample> = Vec::new();
    let mut model_load_ms: Option<f64> = None;

    let judge = RuleJudge {};

    if let Some(hyp_path) = &args.hypotheses {
        // ── Replay mode ──────────────────────────────────────────────
        let content = std::fs::read_to_string(hyp_path)
            .map_err(|e| format!("cannot read hypotheses {}: {e}", hyp_path.display()))?;
        let mut hyps: HashMap<String, HypothesisRow> = HashMap::new();
        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let row: HypothesisRow = serde_json::from_str(line)
                .map_err(|e| format!("hypotheses line {}: {e}", idx + 1))?;
            hyps.insert(row.id.clone(), row);
        }

        for case in &corpus.cases {
            let hyp = hyps.get(&case.id).ok_or(format!(
                "no hypothesis for case {:?} in {}",
                case.id,
                hyp_path.display()
            ))?;

            let w = word_error_rate(&case.reference, &hyp.hypothesis);
            wer_cases.push((
                case.id.clone(),
                case.reference.clone(),
                hyp.hypothesis.clone(),
                w.clone(),
            ));
            tag_cases.push((
                case.id.clone(),
                case.reference.clone(),
                case.tags.clone(),
                w.clone(),
            ));

            let polished = hyp
                .polished
                .clone()
                .unwrap_or_else(|| hyp.hypothesis.clone());
            let verdict = judge.judge(case, &polished)?;
            polish_results.push(CasePolish {
                id: case.id.clone(),
                score: verdict.score,
                failed: verdict.failed.clone(),
            });

            let asr_ms = hyp.asr_ms;
            let total_ms = hyp.total_ms;
            if let Some(ms) = asr_ms {
                asr_samples.push(Sample {
                    name: case.id.clone(),
                    millis: ms,
                });
            }
            if let Some(ms) = total_ms {
                total_samples.push(Sample {
                    name: case.id.clone(),
                    millis: ms,
                });
            }

            case_results.push(CaseResult {
                id: case.id.clone(),
                reference: case.reference.clone(),
                hypothesis: hyp.hypothesis.clone(),
                polished: Some(polished),
                tags: case.tags.clone(),
                human: case.human,
                wer: w.wer,
                substitutions: w.substitutions,
                deletions: w.deletions,
                insertions: w.insertions,
                polish_score: Some(verdict.score),
                polish_failed: verdict.failed.clone(),
                asr_ms,
                total_ms,
            });
        }
    } else {
        // ── Measure mode ─────────────────────────────────────────────
        let model_path = args
            .model
            .as_ref()
            .ok_or("--model <path> is required with --asr parakeet")?;

        let mut provider: Box<dyn teletype_speech::SpeechProvider> = if args.asr == "parakeet" {
            Box::new(teletype_speech::ParakeetProvider::new())
        } else if args.asr == "whisper" {
            Box::new(teletype_speech::whisper::WhisperProvider::new())
        } else {
            return Err(format!("unsupported --asr value: {}", args.asr));
        };

        let load_start = Instant::now();
        provider
            .load(model_path)
            .map_err(|e| format!("model load failed: {e}"))?;
        model_load_ms = Some(load_start.elapsed().as_secs_f64() * 1000.0);
        eprintln!("model loaded in {:.0} ms", model_load_ms.unwrap());

        for case in &corpus.cases {
            let samples = decode_wav_16k(&case.audio_path)?;
            let asr_start = Instant::now();
            let raw = provider
                .transcribe(&samples, "auto")
                .map_err(|e| format!("transcribe {:?}: {e}", case.id))?;
            let asr_ms = asr_start.elapsed().as_secs_f64() * 1000.0;

            // Polish: run the deterministic rubric on the raw ASR output.
            // (A real polish model would be invoked here; the rubric scores
            // whatever string the ASR produced, which is the conservative
            // lower bound.)
            let polished = raw.clone();
            let verdict = judge.judge(case, &polished)?;
            let total_ms = asr_ms; // polish not timed separately in measure mode

            let w = word_error_rate(&case.reference, &raw);
            wer_cases.push((
                case.id.clone(),
                case.reference.clone(),
                raw.clone(),
                w.clone(),
            ));
            tag_cases.push((
                case.id.clone(),
                case.reference.clone(),
                case.tags.clone(),
                w.clone(),
            ));
            polish_results.push(CasePolish {
                id: case.id.clone(),
                score: verdict.score,
                failed: verdict.failed.clone(),
            });
            asr_samples.push(Sample {
                name: case.id.clone(),
                millis: asr_ms,
            });
            total_samples.push(Sample {
                name: case.id.clone(),
                millis: total_ms,
            });

            case_results.push(CaseResult {
                id: case.id.clone(),
                reference: case.reference.clone(),
                hypothesis: raw,
                polished: Some(polished),
                tags: case.tags.clone(),
                human: case.human,
                wer: w.wer,
                substitutions: w.substitutions,
                deletions: w.deletions,
                insertions: w.insertions,
                polish_score: Some(verdict.score),
                polish_failed: verdict.failed.clone(),
                asr_ms: Some(asr_ms),
                total_ms: Some(total_ms),
            });
        }
    }

    let wer_summary = wer::summarize(&wer_cases);
    let wer_by_tag = wer::by_tag(&tag_cases);
    let polish_mean = if polish_results.is_empty() {
        0.0
    } else {
        polish_results.iter().map(|p| p.score as f64).sum::<f64>() / polish_results.len() as f64
    };

    let human_count = corpus.cases.iter().filter(|c| c.human).count();
    let synth_count = corpus.cases.len() - human_count;

    let report = EvalReport {
        git_sha,
        timestamp,
        model: args
            .model
            .as_ref()
            .map(|m| m.display().to_string())
            .unwrap_or_else(|| args.asr.clone()),
        mode: if args.hypotheses.is_some() {
            "replay".into()
        } else {
            "measure".into()
        },
        has_synthesised_audio: synth_count > 0,
        human_case_count: human_count,
        synthesised_case_count: synth_count,
        wer: wer_summary,
        wer_by_tag,
        polish: polish_results,
        polish_mean_score: polish_mean,
        latency_asr: (!asr_samples.is_empty()).then(|| LatencySummary::new(&asr_samples)),
        latency_total: (!total_samples.is_empty()).then(|| LatencySummary::new(&total_samples)),
        model_load_ms,
        cases: case_results,
    };

    if let Some(p) = &args.report {
        report.write_markdown(p).map_err(|e| e.to_string())?;
        eprintln!("wrote markdown report to {}", p.display());
    }
    if let Some(p) = &args.json {
        report.write_json(p).map_err(|e| e.to_string())?;
        eprintln!("wrote JSON report to {}", p.display());
    }

    // Print a short summary to stdout.
    println!(
        "cases={} micro_wer={:.4} macro_wer={:.4} polish_mean={:.1}",
        report.wer.cases, report.wer.micro_wer, report.wer.macro_wer, report.polish_mean_score
    );
    if let Some(l) = &report.latency_asr {
        println!("asr   p50={:.0}ms p95={:.0}ms", l.p50, l.p95);
    }
    if let Some(l) = &report.latency_total {
        println!("total p50={:.0}ms p95={:.0}ms", l.p50, l.p95);
    }

    // Threshold gate.
    if let Some(tp) = &args.thresholds {
        let thresholds = load_thresholds(tp)?;
        let mut breaches: Vec<String> = Vec::new();

        if report.wer.micro_wer > thresholds.max_micro_wer {
            breaches.push(format!(
                "micro_wer {:.4} > max_micro_wer {:.4}",
                report.wer.micro_wer, thresholds.max_micro_wer
            ));
        }
        if let Some(l) = &report.latency_asr {
            if l.p95 > thresholds.max_p95_asr_ms {
                breaches.push(format!(
                    "p95_asr_ms {:.0} > max_p95_asr_ms {:.0}",
                    l.p95, thresholds.max_p95_asr_ms
                ));
            }
        }
        if let Some(l) = &report.latency_total {
            if l.p95 > thresholds.max_p95_total_ms {
                breaches.push(format!(
                    "p95_total_ms {:.0} > max_p95_total_ms {:.0}",
                    l.p95, thresholds.max_p95_total_ms
                ));
            }
        }
        if report.polish_mean_score < thresholds.min_mean_polish_score {
            breaches.push(format!(
                "mean_polish_score {:.1} < min_mean_polish_score {:.1}",
                report.polish_mean_score, thresholds.min_mean_polish_score
            ));
        }

        if breaches.is_empty() {
            println!("threshold gate: PASS");
        } else {
            for b in &breaches {
                eprintln!("THRESHOLD BREACH: {b}");
            }
            eprintln!("threshold gate: FAIL ({} breach(es))", breaches.len());
            exit(1);
        }
    }

    Ok(())
}

/// Decode a wav file to 16 kHz mono f32.
///
/// The generator (`evals/generate-corpus.sh`) emits 16 kHz mono LEI16 wavs
/// via `say --data-format=LEI16@16000`, so the common path is a plain RIFF
/// parser. If the file is not 16 kHz mono we say so and resample with the
/// same resampler the live dictation path uses
/// (`teletype_core::audio::resample_to_target`).
fn decode_wav_16k(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!(
            "{} is not a RIFF/WAVE file (symphonia is a dev-dependency and \
             unavailable to this binary; generate 16 kHz mono wavs with \
             evals/generate-corpus.sh)",
            path.display()
        ));
    }

    // Walk WAVE chunks to find fmt and data.
    let mut pos = 12usize;
    let mut sample_rate: u32 = 16_000;
    let mut channels: u16 = 1;
    let mut bits: u16 = 16;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let cid = std::str::from_utf8(&bytes[pos..pos + 4]).unwrap_or("????");
        let csize = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if body + csize > bytes.len() {
            break;
        }
        match cid {
            "fmt " => {
                if csize >= 16 {
                    channels = u16::from_le_bytes(bytes[body + 2..body + 4].try_into().unwrap());
                    sample_rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
                    bits = u16::from_le_bytes(bytes[body + 14..body + 16].try_into().unwrap());
                }
            }
            "data" => data = Some(&bytes[body..body + csize]),
            _ => {}
        }
        pos = body + csize + if csize % 2 == 1 { 1 } else { 0 };
    }

    let data = data.ok_or_else(|| format!("no data chunk in wav file {}", path.display()))?;
    if bits != 16 {
        return Err(format!(
            "{} is {}-bit PCM; only 16-bit is supported",
            path.display(),
            bits
        ));
    }
    if sample_rate != 16_000 {
        eprintln!(
            "warning: {} is {} Hz, not 16 kHz — resampling",
            path.display(),
            sample_rate
        );
    }
    if channels > 1 {
        eprintln!(
            "warning: {} has {} channels, mixing to mono",
            path.display(),
            channels
        );
    }

    let nch = channels as usize;
    let n_frames = data.len() / (2 * nch);
    let mut pcm = vec![0f32; n_frames];
    for (i, frame) in pcm.iter_mut().enumerate() {
        let mut sum = 0f32;
        for c in 0..nch {
            let off = (i * nch + c) * 2;
            let v = i16::from_le_bytes(data[off..off + 2].try_into().unwrap());
            sum += v as f32 / 32_768.0;
        }
        *frame = sum / channels as f32;
    }
    if pcm.is_empty() {
        return Err(format!("no audio data in {}", path.display()));
    }
    if sample_rate != 16_000 {
        pcm = teletype_core::audio::resample_to_target(&pcm, sample_rate);
    }
    Ok(pcm)
}
