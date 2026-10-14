//! Shared eval-run driver: replay + measure flows, P0.4 metrics, gates.
//!
//! Both binaries (`eval` with the legacy flags, `teletype-eval run` with the
//! plan §P0.4 CLI) parse their arguments into [`RunConfig`] and call
//! [`execute`]. Behaviour for the legacy flags is unchanged; the new flags
//! add verbatim/clean/vocab/entity/semantic metrics, live-trace latency,
//! `--set` corpora, an output directory, and the plan-format thresholds gate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::corpus::{Case, Corpus};
use crate::latency::{LatencySummary, Sample};
use crate::metrics::{self, EntityScore, VocabScore};
use crate::polish::Judge;
use crate::polish::RuleJudge;
use crate::report::{CasePolish, CaseResult, EvalReport};
use crate::wer::{self, Wer};

/// Where fetched public sets live (`fetch_datasets.sh` layout).
pub const DEFAULT_CACHE_DIR_NAME: &str = ".cache/teletype-eval";

/// Resolve the dataset cache directory: `$TELETYPE_EVAL_CACHE`, else
/// `~/.cache/teletype-eval`.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TELETYPE_EVAL_CACHE") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(DEFAULT_CACHE_DIR_NAME)
}

/// One run's configuration, from either CLI.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// Explicit corpus directory (manifest.jsonl + audio). Mutually
    /// exclusive with `sets`.
    pub corpus: Option<PathBuf>,
    /// Named sets from the cache dir (e.g. `human`, `librispeech-clean`,
    /// `earnings22-vocab`, `ami`, `cv`, `noise-10`, `fleurs-de`).
    pub sets: Vec<String>,
    pub hypotheses: Option<PathBuf>,
    /// Engine id. `none` = replay (needs `--hypotheses`); `parakeet*` and
    /// `whisper*` map to the matching provider (so `parakeet-ultra` works as
    /// an alias of the local Parakeet provider).
    pub engine: String,
    pub model: Option<PathBuf>,
    /// Display label for the report (`--engine` value by default).
    pub model_label: Option<String>,
    /// Polish model id. Recorded only: the harness scores raw ASR output
    /// (invoking a polish model needs the inference sidecar, which is
    /// outside the harness). Anything but `none` prints a loud note.
    pub polish: String,
    /// Live mode: requires `--traces` (DictationTrace JSONL). The WER half
    /// still runs replay/measure; the latency half comes from the traces.
    pub live: bool,
    pub traces: Option<PathBuf>,
    /// Explicit vocab file (JSON `{case-id: [terms]}`); otherwise each
    /// set's `vocab.json` is auto-loaded when present.
    pub vocab: Option<PathBuf>,
    /// Baseline `report.json` for the verbatim-WER regression gate.
    pub baseline: Option<PathBuf>,
    /// Run the local semantic judge (needs `TELETYPE_EVAL_JUDGE_URL`).
    pub semantic: bool,
    /// Output directory: gets `report.json`, `report.md`, `meta.json`.
    pub out_dir: Option<PathBuf>,
    pub report_md: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
    pub thresholds: Option<PathBuf>,
}

/// What [`execute`] computed, before the binary decides the exit code.
pub struct RunOutput {
    pub report: EvalReport,
    /// Human-readable SKIP lines (a gate that found no data is skipped,
    /// never passed silently and never failed).
    pub gate_notes: Vec<String>,
    /// Empty when the gate passed (or no thresholds file was given).
    pub breaches: Vec<String>,
    /// Sets requested but missing from the cache (skipped with a warning).
    pub skipped_sets: Vec<String>,
}

/// One row of a hypotheses file (replay mode).
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

/// Merged `--set` corpus: cases, per-case vocab terms, skipped set names.
pub type LoadedSets = (Corpus, HashMap<String, Vec<String>>, Vec<String>);

/// Expand `--set` names against the cache dir.
///
/// Each set is `<cache>/<dir>/manifest.jsonl` (+ audio). With more than one
/// set, case ids are prefixed `{set}-{id}` so per-set reports stay
/// attributable; a single set keeps its raw ids (so `--vocab` maps keyed by
/// plain id keep working). Missing sets are skipped with a warning and
/// reported in `skipped` — the caller records them in `meta.json`.
/// `vocab.json` files found next to a set manifest are merged (explicit
/// `--vocab` wins per case id).
pub fn load_sets(cache_dir: &Path, sets: &[String]) -> Result<LoadedSets, String> {
    let mut cases: Vec<Case> = Vec::new();
    let mut vocab: HashMap<String, Vec<String>> = HashMap::new();
    let mut skipped: Vec<String> = Vec::new();
    let multi = sets.len() > 1;

    for set in sets {
        let dir = cache_dir.join(set);
        match Corpus::load(&dir) {
            Ok(mut corpus) => {
                let vocab_path = dir.join("vocab.json");
                if let Ok(content) = std::fs::read_to_string(&vocab_path) {
                    match serde_json::from_str::<HashMap<String, Vec<String>>>(&content) {
                        Ok(map) => {
                            for (id, terms) in map {
                                let key = if multi { format!("{set}-{id}") } else { id };
                                vocab.insert(key, terms);
                            }
                        }
                        Err(e) => {
                            return Err(format!(
                                "malformed vocab file {}: {e}",
                                vocab_path.display()
                            ));
                        }
                    }
                }
                for mut case in corpus.cases.drain(..) {
                    if multi {
                        case.id = format!("{}-{}", set, case.id);
                    }
                    cases.push(case);
                }
            }
            Err(e) => {
                eprintln!(
                    "warning: skipping set {set:?} ({dir:?} not usable: {e}); \
                     run scripts/eval/fetch_datasets.sh to download it"
                );
                skipped.push(set.clone());
            }
        }
    }

    if cases.is_empty() {
        return Err("no usable sets: every requested set was skipped".into());
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((
        Corpus {
            dir: cache_dir.to_path_buf(),
            cases,
        },
        vocab,
        skipped,
    ))
}

/// Load the explicit `--vocab` file (`{case-id: [terms]}`).
fn load_vocab_file(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read vocab file {}: {e}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("malformed vocab file {}: {e}", path.display()))
}

fn git_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Run the eval: load corpus, score every case, aggregate, gate.
pub fn execute(cfg: &RunConfig) -> Result<RunOutput, String> {
    if cfg.live && cfg.traces.is_none() {
        return Err("live mode needs --traces <DictationTrace JSONL>: the \
            harness cannot synthesize hotkey up/down events by itself; wire \
            the P0.2 pipeline hooks first (plan §P0.4 calls this out: replay \
            mode doesn't count)"
            .into());
    }
    if cfg.engine == "none" && cfg.hypotheses.is_none() {
        return Err(
            "no hypotheses file: --engine none requires --hypotheses <file> \
             (replay mode). Pass --engine parakeet --model <path> for a real run."
                .into(),
        );
    }
    if cfg.corpus.is_none() && cfg.sets.is_empty() {
        return Err("no corpus: pass --corpus <dir> or --set <name,...>".into());
    }
    if cfg.polish != "none" {
        eprintln!(
            "note: --polish {} is recorded in the report, but the harness \
             scores raw ASR output: invoking a polish model needs the \
             inference sidecar, which is outside the harness",
            cfg.polish
        );
    }

    // ── corpus ──────────────────────────────────────────────────────────
    let (corpus, mut vocab_map, skipped_sets) = if !cfg.sets.is_empty() {
        let cache = default_cache_dir();
        load_sets(&cache, &cfg.sets)?
    } else {
        let dir = cfg.corpus.as_ref().unwrap();
        (
            Corpus::load(dir).map_err(|e| format!("corpus: {e}"))?,
            HashMap::new(),
            Vec::new(),
        )
    };
    if let Some(path) = &cfg.vocab {
        for (id, terms) in load_vocab_file(path)? {
            vocab_map.insert(id, terms);
        }
    }
    eprintln!(
        "loaded {} cases from {}",
        corpus.cases.len(),
        corpus.dir.display()
    );

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());

    // ── hypotheses (replay) or provider (measure) ───────────────────────
    let mut replay: HashMap<String, HypothesisRow> = HashMap::new();
    if let Some(hyp_path) = &cfg.hypotheses {
        let content = std::fs::read_to_string(hyp_path)
            .map_err(|e| format!("cannot read hypotheses {}: {e}", hyp_path.display()))?;
        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let row: HypothesisRow = serde_json::from_str(line)
                .map_err(|e| format!("hypotheses line {}: {e}", idx + 1))?;
            replay.insert(row.id.clone(), row);
        }
    }

    struct Provider {
        inner: Box<dyn teletype_speech::SpeechProvider>,
    }
    let mut provider: Option<Provider> = None;
    let mut model_load_ms: Option<f64> = None;
    if cfg.hypotheses.is_none() {
        let model_path = cfg
            .model
            .as_ref()
            .ok_or("--model <path> is required for a measure run")?;
        let inner: Box<dyn teletype_speech::SpeechProvider> = if cfg.engine.starts_with("parakeet")
        {
            Box::new(teletype_speech::ParakeetProvider::new())
        } else if cfg.engine.starts_with("whisper") {
            Box::new(teletype_speech::whisper::WhisperProvider::new())
        } else {
            return Err(format!(
                "unsupported --engine {:?} (want none|parakeet*|whisper*)",
                cfg.engine
            ));
        };
        let mut provider_box = Provider { inner };
        let load_start = std::time::Instant::now();
        provider_box
            .inner
            .load(model_path)
            .map_err(|e| format!("model load failed: {e}"))?;
        model_load_ms = Some(load_start.elapsed().as_secs_f64() * 1000.0);
        eprintln!("model loaded in {:.0} ms", model_load_ms.unwrap());
        provider = Some(provider_box);
    }

    // ── per-case scoring ────────────────────────────────────────────────
    let judge = RuleJudge {};
    let mut case_results: Vec<CaseResult> = Vec::new();
    let mut wer_cases: Vec<(String, String, String, Wer)> = Vec::new();
    let mut tag_cases: Vec<(String, String, Vec<String>, Wer)> = Vec::new();
    let mut verbatim_wers: Vec<f64> = Vec::new();
    let mut verbatim_errors: usize = 0;
    let mut verbatim_words: usize = 0;
    let mut clean_errors: usize = 0;
    let mut clean_words: usize = 0;
    let mut clean_human_errors: usize = 0;
    let mut clean_human_words: usize = 0;
    let mut vocab_hits: usize = 0;
    let mut vocab_ref_terms: usize = 0;
    let mut vocab_false_alarms: usize = 0;
    let mut vocab_produced: usize = 0;
    let mut vocab_cases: usize = 0;
    let mut entity_matched: usize = 0;
    let mut entity_ref: usize = 0;
    let mut semantic_ok: usize = 0;
    let mut semantic_judged: usize = 0;
    let mut semantic_ran = false;
    let mut polish_results: Vec<CasePolish> = Vec::new();
    let mut asr_samples: Vec<Sample> = Vec::new();
    let mut total_samples: Vec<Sample> = Vec::new();

    for case in &corpus.cases {
        let (raw, polished_text, asr_ms, total_ms) = if let Some(row) = replay.get(&case.id) {
            let polished = row
                .polished
                .clone()
                .unwrap_or_else(|| row.hypothesis.clone());
            (row.hypothesis.clone(), polished, row.asr_ms, row.total_ms)
        } else {
            let prov = provider.as_mut().unwrap();
            let samples = decode_wav_16k(&case.audio_path)?;
            let asr_start = std::time::Instant::now();
            let raw = prov
                .inner
                .transcribe(&samples, "auto")
                .map_err(|e| format!("transcribe {:?}: {e}", case.id))?;
            let ms = asr_start.elapsed().as_secs_f64() * 1000.0;
            // Measure mode has no polish model: polished == raw (conservative
            // lower bound, same as the legacy driver).
            let polished_text = raw.clone();
            (raw, polished_text, Some(ms), Some(ms))
        };

        // Legacy WER (unchanged semantics: raw tokenise, no normalizer).
        let w = wer::word_error_rate(&case.reference, &raw);
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

        // Verbatim WER (normalized, fillers dropped on both sides).
        let wv = metrics::wer_verbatim(&raw, &case.reference);
        verbatim_errors += wv.substitutions + wv.deletions + wv.insertions;
        verbatim_words += wv.reference_words;
        verbatim_wers.push(wv.wer);

        // Clean WER (only cases carrying a clean reference): final pasted
        // text against clean.
        if let Some(clean) = case.clean.as_deref() {
            let wc = metrics::wer_clean(&polished_text, clean);
            clean_errors += wc.substitutions + wc.deletions + wc.insertions;
            clean_words += wc.reference_words;
            if case.human {
                clean_human_errors += wc.substitutions + wc.deletions + wc.insertions;
                clean_human_words += wc.reference_words;
            }
        }

        // Polish rubric scores what would be pasted (the polished string in
        // replay mode; raw ASR in measure mode — the conservative lower
        // bound, same as the legacy driver).
        let verdict = judge.judge(case, &polished_text)?;
        polish_results.push(CasePolish {
            id: case.id.clone(),
            score: verdict.score,
            failed: verdict.failed.clone(),
        });

        // Vocab (only cases present in the vocab map).
        let vocab_terms = vocab_map.get(&case.id);
        let v: Option<VocabScore> =
            vocab_terms.map(|terms| metrics::vocab_score(&case.reference, &raw, terms));
        if let Some(v) = v {
            if v.terms_in_ref > 0 {
                vocab_cases += 1;
            }
            vocab_hits += v.hits;
            vocab_ref_terms += v.terms_in_ref;
            vocab_false_alarms += v.false_alarms;
            vocab_produced += v.produced;
        }

        // Entities (always computable).
        let e: EntityScore = metrics::entity_accuracy(&case.reference, &raw);
        entity_matched += e.matched;
        entity_ref += e.ref_entities;

        // Semantic judge (opt-in, report-only): candidate is the pasted text.
        if cfg.semantic {
            match metrics::judge_semantic(&case.reference, &polished_text)? {
                Some(r) => {
                    semantic_ran = true;
                    semantic_judged += 1;
                    if r.ok {
                        semantic_ok += 1;
                    }
                }
                None => {
                    if !semantic_ran {
                        eprintln!(
                            "note: --semantic given but {0} is unset; \
                             semantic_ok not measured",
                            metrics::JUDGE_URL_ENV
                        );
                    }
                }
            }
        }

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
            hypothesis: raw,
            polished: Some(polished_text),
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

    // ── aggregation ─────────────────────────────────────────────────────
    let wer_summary = wer::summarize(&wer_cases);
    let wer_by_tag = wer::by_tag(&tag_cases);
    let polish_mean = if polish_results.is_empty() {
        0.0
    } else {
        polish_results.iter().map(|p| p.score as f64).sum::<f64>() / polish_results.len() as f64
    };
    let human_count = corpus.cases.iter().filter(|c| c.human).count();
    let synth_count = corpus.cases.len() - human_count;

    let wer_verbatim_micro =
        (verbatim_words > 0).then(|| verbatim_errors as f64 / verbatim_words as f64);
    let wer_verbatim_macro = (!verbatim_wers.is_empty())
        .then(|| verbatim_wers.iter().sum::<f64>() / verbatim_wers.len() as f64);
    let wer_clean_micro = (clean_words > 0).then(|| clean_errors as f64 / clean_words as f64);
    let wer_clean_human_micro =
        (clean_human_words > 0).then(|| clean_human_errors as f64 / clean_human_words as f64);
    let (vocab_recall, vocab_false_alarm) = if vocab_ref_terms > 0 {
        (
            Some(vocab_hits as f64 / vocab_ref_terms as f64),
            Some(if vocab_produced == 0 {
                0.0
            } else {
                vocab_false_alarms as f64 / vocab_produced as f64
            }),
        )
    } else {
        (None, None)
    };
    let entity_accuracy = (entity_ref > 0).then(|| entity_matched as f64 / entity_ref as f64);
    let semantic_ok_rate =
        (semantic_judged > 0).then(|| semantic_ok as f64 / semantic_judged as f64);

    // Live-mode release→paste latency from DictationTrace JSONL.
    let mut latency_release_to_paste: Option<LatencySummary> = None;
    if let Some(path) = &cfg.traces {
        let traces = crate::latency::load_trace_intervals(path)?;
        let samples: Vec<Sample> = traces
            .iter()
            .filter_map(|t| {
                t.release_to_paste_ms().map(|ms| Sample {
                    name: format!("session-{}", t.session),
                    millis: ms,
                })
            })
            .collect();
        eprintln!(
            "loaded {} traces from {}, {} with a valid release→paste pair",
            traces.len(),
            path.display(),
            samples.len()
        );
        latency_release_to_paste = LatencySummary::try_new(&samples);
    }

    let mode = if cfg.live {
        "live"
    } else if cfg.hypotheses.is_some() {
        "replay"
    } else {
        "measure"
    };
    let report = EvalReport {
        git_sha: git_sha(),
        timestamp,
        model: cfg
            .model_label
            .clone()
            .or_else(|| cfg.model.as_ref().map(|m| m.display().to_string()))
            .unwrap_or_else(|| cfg.engine.clone()),
        mode: mode.into(),
        has_synthesised_audio: synth_count > 0,
        human_case_count: human_count,
        synthesised_case_count: synth_count,
        wer: wer_summary,
        wer_by_tag,
        polish: polish_results,
        polish_mean_score: polish_mean,
        latency_asr: LatencySummary::try_new(&asr_samples),
        latency_total: LatencySummary::try_new(&total_samples),
        model_load_ms,
        cases: case_results,
        wer_verbatim_micro,
        wer_verbatim_macro,
        wer_clean_micro,
        wer_clean_human_micro,
        vocab_recall,
        vocab_false_alarm,
        entity_accuracy,
        semantic_ok_rate,
        latency_release_to_paste,
    };

    // ── output files ────────────────────────────────────────────────────
    let mut report_md = cfg.report_md.clone();
    let mut report_json = cfg.report_json.clone();
    if let Some(dir) = &cfg.out_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create out dir {}: {e}", dir.display()))?;
        report_md = Some(dir.join("report.md"));
        report_json = Some(dir.join("report.json"));
        let meta = serde_json::json!({
            "engine": cfg.engine,
            "polish": cfg.polish,
            "live": cfg.live,
            "sets": cfg.sets,
            "corpus": cfg.corpus,
            "vocab_cases": vocab_cases,
            "entity_ref_count": entity_ref,
            "semantic_judged": semantic_judged,
            "skipped_sets": skipped_sets,
            "git_sha": report.git_sha,
            "timestamp": report.timestamp,
        });
        std::fs::write(
            dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .map_err(|e| format!("cannot write meta.json: {e}"))?;
    }
    if let Some(p) = &report_md {
        report.write_markdown(p).map_err(|e| e.to_string())?;
        eprintln!("wrote markdown report to {}", p.display());
    }
    if let Some(p) = &report_json {
        report.write_json(p).map_err(|e| e.to_string())?;
        eprintln!("wrote JSON report to {}", p.display());
    }

    // ── threshold gate ──────────────────────────────────────────────────
    let mut gate_notes: Vec<String> = Vec::new();
    let mut breaches: Vec<String> = Vec::new();
    if let Some(tp) = &cfg.thresholds {
        let content = std::fs::read_to_string(tp)
            .map_err(|e| format!("cannot read thresholds file {}: {e}", tp.display()))?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("malformed thresholds file {}: {e}", tp.display()))?;
        if v.get("wer_verbatim_max_regression_pct").is_some() {
            gate_new_format(
                &report,
                &v,
                cfg.baseline.as_deref(),
                &mut gate_notes,
                &mut breaches,
            )?;
        } else if v.get("max_micro_wer").is_some() {
            gate_legacy_format(&report, &v, &mut breaches)?;
        } else {
            return Err(format!(
                "unrecognized thresholds format in {}: want plan §P0.4 keys \
                 (wer_verbatim_max_regression_pct, …) or legacy keys (max_micro_wer, …)",
                tp.display()
            ));
        }
    }

    Ok(RunOutput {
        report,
        gate_notes,
        breaches,
        skipped_sets,
    })
}

/// Plan §P0.4 gate. A check with no data is SKIPPED (noted, never passed
/// silently); a check with data must hold.
fn gate_new_format(
    report: &EvalReport,
    v: &serde_json::Value,
    baseline: Option<&Path>,
    notes: &mut Vec<String>,
    breaches: &mut Vec<String>,
) -> Result<(), String> {
    let num = |key: &str| -> Result<f64, String> {
        v[key]
            .as_f64()
            .ok_or_else(|| format!("thresholds: {key:?} must be a number"))
    };
    let max_regression = num("wer_verbatim_max_regression_pct")?;
    let vocab_min = num("vocab_recall_min")?;
    let r2p_p95_max = num("latency_release_to_paste_p95_ms_max")?;
    let clean_human_max = num("wer_clean_human_max")?;

    // Verbatim-WER regression vs the baseline report.
    match (report.wer_verbatim_micro, baseline) {
        (Some(cur), Some(base_path)) => {
            let content = std::fs::read_to_string(base_path)
                .map_err(|e| format!("cannot read baseline {}: {e}", base_path.display()))?;
            let base: EvalReport = serde_json::from_str(&content)
                .map_err(|e| format!("malformed baseline {}: {e}", base_path.display()))?;
            match base.wer_verbatim_micro {
                Some(0.0) | None => {
                    notes.push("regression check SKIPPED: baseline has no verbatim WER".into())
                }
                Some(b) => {
                    let pct = (cur - b) / b * 100.0;
                    if pct > max_regression {
                        breaches.push(format!(
                            "verbatim WER regression {pct:.2}% > {max_regression:.2}% \
                             (baseline {b:.4}, current {cur:.4})"
                        ));
                    }
                }
            }
        }
        (_, None) => notes.push("regression check SKIPPED: no --baseline given".into()),
        (None, _) => notes.push("regression check SKIPPED: no verbatim WER measured".into()),
    }

    match report.vocab_recall {
        Some(r) if r < vocab_min => breaches.push(format!(
            "vocab_recall {r:.4} < vocab_recall_min {vocab_min:.4}"
        )),
        Some(_) => {}
        None => notes.push("vocab_recall SKIPPED: no vocab terms measured".into()),
    }

    match &report.latency_release_to_paste {
        Some(l) if l.p95 > r2p_p95_max => breaches.push(format!(
            "release→paste p95 {:.0} ms > max {r2p_p95_max:.0} ms",
            l.p95
        )),
        Some(_) => {}
        None => notes.push("release→paste latency SKIPPED: no --traces given".into()),
    }

    match report.wer_clean_human_micro {
        Some(w) if w > clean_human_max => breaches.push(format!(
            "human clean WER {w:.4} > wer_clean_human_max {clean_human_max:.4}"
        )),
        Some(_) => {}
        None => notes.push("human clean WER SKIPPED: no human cases with clean refs".into()),
    }
    Ok(())
}

/// Legacy gate (pre-P0.4 `thresholds.json` shape), kept so existing CI and
/// hand-written threshold files keep working.
fn gate_legacy_format(
    report: &EvalReport,
    v: &serde_json::Value,
    breaches: &mut Vec<String>,
) -> Result<(), String> {
    let num = |key: &str| -> Result<f64, String> {
        v[key]
            .as_f64()
            .ok_or_else(|| format!("thresholds: {key:?} must be a number"))
    };
    let max_micro_wer = num("max_micro_wer")?;
    let max_p95_asr_ms = num("max_p95_asr_ms")?;
    let max_p95_total_ms = num("max_p95_total_ms")?;
    let min_mean_polish_score = num("min_mean_polish_score")?;

    if report.wer.micro_wer > max_micro_wer {
        breaches.push(format!(
            "micro_wer {:.4} > max_micro_wer {:.4}",
            report.wer.micro_wer, max_micro_wer
        ));
    }
    if let Some(l) = &report.latency_asr {
        if l.p95 > max_p95_asr_ms {
            breaches.push(format!(
                "p95_asr_ms {:.0} > max_p95_asr_ms {:.0}",
                l.p95, max_p95_asr_ms
            ));
        }
    }
    if let Some(l) = &report.latency_total {
        if l.p95 > max_p95_total_ms {
            breaches.push(format!(
                "p95_total_ms {:.0} > max_p95_total_ms {:.0}",
                l.p95, max_p95_total_ms
            ));
        }
    }
    if report.polish_mean_score < min_mean_polish_score {
        breaches.push(format!(
            "mean_polish_score {:.1} < min_mean_polish_score {:.1}",
            report.polish_mean_score, min_mean_polish_score
        ));
    }
    Ok(())
}

/// Decode a wav file to 16 kHz mono f32.
///
/// The generator (`evals/generate-corpus.sh`) emits 16 kHz mono LEI16 wavs,
/// so the common path is a plain RIFF parser. Anything else is warned about
/// and resampled with the live dictation path's resampler.
pub fn decode_wav_16k(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!(
            "{} is not a RIFF/WAVE file (generate 16 kHz mono wavs with \
             evals/generate-corpus.sh or scripts/eval/fetch_datasets.sh)",
            path.display()
        ));
    }

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
