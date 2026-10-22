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
    /// The §3.8 boosting axis: `on` feeds the case's vocab terms to
    /// `set_boost_terms` (native boosters) or to the post-ASR phonetic
    /// rescorer (engines without native boosting). `off` (default) runs the
    /// engine unboosted.
    pub boost: bool,
    /// Models root for catalog-engine measure runs (default: the app's
    /// models directory). The model file path is derived from the `--engine`
    /// catalog entry, mirroring the app's `resolve_speech_model`.
    pub models_dir: Option<PathBuf>,
}

/// What [`execute`] computed, before the binary decides the exit code.
pub struct RunOutput {
    pub report: EvalReport,
    /// Human-readable SKIP lines (a gate that found no data is skipped,
    /// never passed silently; with zero evaluated checks the gate refuses
    /// to print PASS).
    pub gate_notes: Vec<String>,
    /// Empty when the gate passed (or no thresholds file was given).
    pub breaches: Vec<String>,
    /// Sets requested but missing from the cache (skipped with a warning).
    pub skipped_sets: Vec<String>,
    /// Number of threshold checks that had data and were evaluated. `0`
    /// means every check was SKIPPED — the run has no evidence to pass on.
    pub gate_checks: usize,
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
                // Sets whose manifests already carry the `{set}-` prefix
                // (librispeech via subset.py, earnings22 via its builder)
                // must not get it twice; plain-id sets (human, ami, cv, …)
                // get it once when several sets are merged.
                let prefix = format!("{set}-");
                let vocab_path = dir.join("vocab.json");
                if let Ok(content) = std::fs::read_to_string(&vocab_path) {
                    match serde_json::from_str::<HashMap<String, Vec<String>>>(&content) {
                        Ok(map) => {
                            for (id, terms) in map {
                                let key = if multi && !id.starts_with(&prefix) {
                                    format!("{prefix}{id}")
                                } else {
                                    id
                                };
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
                    if multi && !case.id.starts_with(&prefix) {
                        case.id = format!("{prefix}{}", case.id);
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

/// The app's models directory, so a measure run without `--model` loads the
/// same file the app would (`~/.teletype/models` on macOS; the app data
/// dir `models/` elsewhere).
fn app_models_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TELETYPE_MODELS_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("com.teletype.app")
            .join("models")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".teletype").join("models")
    }
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
        // Replay measures this file against the corpus: every corpus case
        // needs a row. A missing row used to fall through to the measure
        // branch and panic on `provider.as_mut().unwrap()` (provider is
        // None in replay mode). A partial replay is not a measurement, so
        // fail closed with an explicit instrument verdict before scoring.
        let missing: Vec<&str> = corpus
            .cases
            .iter()
            .filter(|c| !replay.contains_key(&c.id))
            .map(|c| c.id.as_str())
            .collect();
        if !missing.is_empty() {
            let sample = missing
                .iter()
                .take(5)
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "INSTRUMENT_INVALID: hypotheses file {} is missing {} of {} corpus \
                 case(s) (first missing: {sample})",
                hyp_path.display(),
                missing.len(),
                corpus.cases.len()
            ));
        }
    }

    struct Provider {
        inner: Box<dyn teletype_speech::SpeechProvider>,
    }
    let mut provider: Option<Provider> = None;
    let mut model_load_ms: Option<f64> = None;
    let mut boost_mode: Option<String> = None;
    if cfg.hypotheses.is_none() {
        // Resolve the engine: a catalog id (or legacy alias) picks the
        // provider family and model path; `none` is the replay-only error
        // handled above.
        let model = crate::bakeoff::resolve_engine(&cfg.engine)
            .map_err(|e| format!("unsupported --engine {:?}: {e}", cfg.engine))?;
        let model_path = match &cfg.model {
            Some(p) => p.clone(),
            None => {
                let models_dir = cfg.models_dir.clone().unwrap_or_else(app_models_dir);
                crate::bakeoff::model_path_for(&models_dir, model)
            }
        };
        // FluidAudio is macOS-Apple-Silicon only; with
        // TELETYPE_FLUID_DISABLE_SWIFT=1 the stub refuses to load.
        if crate::bakeoff::engine_kind(model) == crate::bakeoff::EngineKind::Fluid
            && !crate::bakeoff::fluid_bridge_available()
        {
            return Err(format!(
                "--engine {} needs the FluidAudio bridge (macOS 14+ Apple \
                 Silicon); this build has the stub \
                 (TELETYPE_FLUID_DISABLE_SWIFT=1 or non-macOS target)",
                model.id
            ));
        }
        let inner: Box<dyn teletype_speech::SpeechProvider> =
            match crate::bakeoff::engine_kind(model) {
                crate::bakeoff::EngineKind::GgmlParakeet => {
                    Box::new(teletype_speech::ParakeetProvider::new())
                }
                crate::bakeoff::EngineKind::GgmlWhisper => {
                    Box::new(teletype_speech::whisper::WhisperProvider::new())
                }
                crate::bakeoff::EngineKind::Sherpa => {
                    teletype_speech::SherpaParakeetProvider::new(model.id)
                        .map(Box::new)
                        .ok_or_else(|| {
                            format!("unsupported --engine {}: unknown sherpa model", model.id)
                        })?
                }

                crate::bakeoff::EngineKind::ParakeetCpp => {
                    return Err(format!(
                        "--engine {}: the ParakeetCpp (native C++) provider is not \
                         wired into the eval harness; use a bake-off engine \
                         (parakeet-ultra-coreml, parakeet-tdt-v3, parakeet-v2-coreml, \
                         sherpa-parakeet-v3-int8, sherpa-parakeet-v2-int8, \
                         large-v3-turbo-q5)",
                        model.id
                    ))
                }
                crate::bakeoff::EngineKind::Fluid => {
                    let version = match model.id {
                        "parakeet-v2-coreml" => teletype_fluid::FluidVersion::V2,
                        "parakeet-v3-coreml" => teletype_fluid::FluidVersion::V3,
                        _ => teletype_fluid::FluidVersion::Ultra,
                    };
                    Box::new(teletype_fluid::FluidParakeet::new(version))
                }
            };
        let mut provider_box = Provider { inner };
        let load_start = std::time::Instant::now();
        provider_box
            .inner
            .load(&model_path)
            .map_err(|e| format!("model load failed: {e}"))?;
        model_load_ms = Some(load_start.elapsed().as_secs_f64() * 1000.0);
        eprintln!("model loaded in {:.0} ms", model_load_ms.unwrap());
        // Boosting axis: per-case terms are pushed in the scoring loop
        // (native boosters apply them in-model; the rescorer path reads
        // the same list). Record which axis this run is.
        boost_mode = Some(if cfg.boost { "on" } else { "off" }.into());
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
    // Cases that could not be measured (WAV decode or ASR failure). One bad
    // clip must never abort a multi-hundred-case run; such cases are scored
    // as total errors (empty hypothesis) and listed in the report.
    let mut case_errors: Vec<String> = Vec::new();

    for case in &corpus.cases {
        let (raw, polished_text, asr_ms, total_ms) = if let Some(row) = replay.get(&case.id) {
            let polished = row
                .polished
                .clone()
                .unwrap_or_else(|| row.hypothesis.clone());
            (row.hypothesis.clone(), polished, row.asr_ms, row.total_ms)
        } else {
            // Unreachable after the replay coverage check above; kept as an
            // explicit error so a future refactor can never reintroduce a
            // panic on a missing hypothesis row.
            let prov = provider.as_mut().ok_or_else(|| {
                format!(
                    "INSTRUMENT_INVALID: no hypothesis row for case {:?} and no provider",
                    case.id
                )
            })?;
            // Boosting axis: push this case's terms in (native boosters
            // apply them in-model; the rescorer path reads them below) and
            // clear them again so the next case starts clean.
            let boost_terms = if cfg.boost {
                crate::bakeoff::boost_terms_for(&case.id, &vocab_map, &HashMap::new())
            } else {
                Vec::new()
            };
            if !boost_terms.is_empty() {
                prov.inner
                    .set_boost_terms(&boost_terms)
                    .map_err(|e| format!("set_boost_terms failed: {e}"))?;
            }
            let result = measure_case_detailed(prov.inner.as_mut(), &case.audio_path);
            if !boost_terms.is_empty() {
                let _ = prov.inner.set_boost_terms(&[]);
            }
            match result {
                Ok((text, words, ms)) => {
                    // Engines without native boosting get the post-ASR
                    // phonetic rescorer as their booster (plan §3.6/§3.7):
                    // same terms, weight 2.0 so the conf gate opens.
                    let raw = if cfg.boost
                        && !boost_terms.is_empty()
                        && !prov.inner.capabilities().native_boost
                    {
                        crate::bakeoff::rescore_text(&text, &words, &boost_terms)
                    } else {
                        text
                    };
                    // Measure mode has no polish model: polished == raw
                    // (conservative lower bound, same as the legacy driver).
                    let polished_text = raw.clone();
                    (raw, polished_text, Some(ms), Some(ms))
                }
                Err(e) => {
                    // One bad clip must never abort a multi-hundred-case
                    // run. Record it, score the case as a total error (the
                    // empty hypothesis tokenises to all deletions) and keep
                    // going.
                    let msg = format!("{}: {e}", case.id);
                    eprintln!("warning: case failed: {msg}");
                    case_errors.push(msg);
                    (String::new(), String::new(), None, None)
                }
            }
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
        boost: boost_mode.clone(),
        case_errors,
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
            "boost": boost_mode.as_deref().unwrap_or("off"),
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
    let mut gate_checks = 0usize;
    if let Some(tp) = &cfg.thresholds {
        let content = std::fs::read_to_string(tp)
            .map_err(|e| format!("cannot read thresholds file {}: {e}", tp.display()))?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("malformed thresholds file {}: {e}", tp.display()))?;
        if v.get("wer_verbatim_max_regression_pct").is_some() {
            gate_checks = gate_new_format(
                &report,
                &v,
                cfg.baseline.as_deref(),
                &mut gate_notes,
                &mut breaches,
            )?;
        } else if v.get("max_micro_wer").is_some() {
            gate_checks = gate_legacy_format(&report, &v, &mut breaches)?;
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
        gate_checks,
    })
}

/// Plan §P0.4 gate. A check with no data is SKIPPED (noted, never passed
/// silently); a check with data must hold. Returns how many checks had data
/// and were evaluated, so the caller can refuse a "PASS" with zero evidence.
fn gate_new_format(
    report: &EvalReport,
    v: &serde_json::Value,
    baseline: Option<&Path>,
    notes: &mut Vec<String>,
    breaches: &mut Vec<String>,
) -> Result<usize, String> {
    let num = |key: &str| -> Result<f64, String> {
        v[key]
            .as_f64()
            .ok_or_else(|| format!("thresholds: {key:?} must be a number"))
    };
    let max_regression = num("wer_verbatim_max_regression_pct")?;
    let vocab_min = num("vocab_recall_min")?;
    let r2p_p95_max = num("latency_release_to_paste_p95_ms_max")?;
    let clean_human_max = num("wer_clean_human_max")?;
    let mut checks = 0usize;

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
                    checks += 1;
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
        Some(r) => {
            checks += 1;
            if r < vocab_min {
                breaches.push(format!(
                    "vocab_recall {r:.4} < vocab_recall_min {vocab_min:.4}"
                ));
            }
        }
        None => notes.push("vocab_recall SKIPPED: no vocab terms measured".into()),
    }

    match &report.latency_release_to_paste {
        Some(l) => {
            checks += 1;
            if l.p95 > r2p_p95_max {
                breaches.push(format!(
                    "release→paste p95 {:.0} ms > max {r2p_p95_max:.0} ms",
                    l.p95
                ));
            }
        }
        None => notes.push("release→paste latency SKIPPED: no --traces given".into()),
    }

    match report.wer_clean_human_micro {
        Some(w) => {
            checks += 1;
            if w > clean_human_max {
                breaches.push(format!(
                    "human clean WER {w:.4} > wer_clean_human_max {clean_human_max:.4}"
                ));
            }
        }
        None => notes.push("human clean WER SKIPPED: no human cases with clean refs".into()),
    }
    Ok(checks)
}

/// Legacy gate (pre-P0.4 `thresholds.json` shape), kept so existing CI and
/// hand-written threshold files keep working.
fn gate_legacy_format(
    report: &EvalReport,
    v: &serde_json::Value,
    breaches: &mut Vec<String>,
) -> Result<usize, String> {
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
    // The legacy shape always compares all four metrics (none are optional).
    Ok(4)
}

/// Decode one case's WAV and transcribe it: the only per-case failure
/// points in a measure run. Errors are returned (never fatal to the run)
/// so the caller can record the case and move on; the returned millisecond
/// value is the ASR wall time on success.
pub fn measure_case(
    provider: &mut dyn teletype_speech::SpeechProvider,
    audio: &Path,
) -> Result<(String, f64), String> {
    let samples = decode_wav_16k(audio)?;
    let asr_start = std::time::Instant::now();
    let raw = provider
        .transcribe(&samples, "auto")
        .map_err(|e| format!("transcribe failed: {e}"))?;
    Ok((raw, asr_start.elapsed().as_secs_f64() * 1000.0))
}

/// Decode one case's WAV and transcribe it with word timings: the measure
/// path for the §3.8 bake-off, where boosting needs the `transcribe_detailed`
/// words (native boosters apply the terms in-model; the rescorer path reads
/// the same words). Engines without detailed support fall back to the plain
/// text with an empty word list through the trait default.
pub fn measure_case_detailed(
    provider: &mut dyn teletype_speech::SpeechProvider,
    audio: &Path,
) -> Result<(String, Vec<teletype_speech::Word>, f64), String> {
    let samples = decode_wav_16k(audio)?;
    let asr_start = std::time::Instant::now();
    let t = provider
        .transcribe_detailed(&samples, "auto")
        .map_err(|e| format!("transcribe failed: {e}"))?;
    Ok((t.text, t.words, asr_start.elapsed().as_secs_f64() * 1000.0))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Minimal fake provider: succeeds with a fixed text or fails with
    /// `NoSpeech`, mirroring what Parakeet does on undecodable clips.
    struct FakeProvider {
        text: Option<String>,
    }

    impl teletype_speech::SpeechProvider for FakeProvider {
        fn load(&mut self, _path: &Path) -> Result<(), teletype_speech::SpeechError> {
            Ok(())
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn transcribe(
            &mut self,
            _samples: &[f32],
            _language: &str,
        ) -> Result<String, teletype_speech::SpeechError> {
            match &self.text {
                Some(t) => Ok(t.clone()),
                None => Err(teletype_speech::SpeechError::NoSpeech),
            }
        }
    }

    /// 16 kHz mono 16-bit PCM wav with `n` silent frames.
    fn write_silent_wav(dir: &Path, name: &str, n: usize) -> PathBuf {
        let data_len = (n * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + n * 2);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&32_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + n * 2, 0);
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn measure_case_surfaces_provider_failure_as_error() {
        let dir = TempDir::new().unwrap();
        let wav = write_silent_wav(dir.path(), "a.wav", 1_600);
        let mut provider = FakeProvider { text: None };
        let err = measure_case(&mut provider, &wav).unwrap_err();
        assert!(
            err.contains("No speech detected"),
            "provider error must surface verbatim: {err}"
        );
    }

    #[test]
    fn measure_case_passes_through_text_and_millis() {
        let dir = TempDir::new().unwrap();
        let wav = write_silent_wav(dir.path(), "a.wav", 1_600);
        let mut provider = FakeProvider {
            text: Some("hello world".into()),
        };
        let (text, ms) = measure_case(&mut provider, &wav).unwrap();
        assert_eq!(text, "hello world");
        assert!(ms >= 0.0);
    }

    #[test]
    fn load_sets_does_not_double_prefix_prefixed_ids() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path();

        // Set whose ids/vocab keys already carry the set prefix (the shape
        // subset.py and the earnings22 builder write).
        let foo = cache.join("foo");
        std::fs::create_dir_all(foo.join("audio")).unwrap();
        std::fs::write(
            foo.join("manifest.jsonl"),
            "{\"id\":\"foo-0001\",\"audio\":\"audio/foo-0001.wav\",\
             \"reference\":\"alpha beta\",\"tags\":[],\"human\":false}\n",
        )
        .unwrap();
        std::fs::write(foo.join("audio/foo-0001.wav"), b"x").unwrap();
        std::fs::write(foo.join("vocab.json"), "{\"foo-0001\":[\"alpha\"]}").unwrap();

        // Plain-id set (the shape human/ami/cv manifests use).
        let bar = cache.join("bar");
        std::fs::create_dir_all(bar.join("audio")).unwrap();
        std::fs::write(
            bar.join("manifest.jsonl"),
            "{\"id\":\"0001\",\"audio\":\"audio/0001.wav\",\
             \"reference\":\"gamma\",\"tags\":[],\"human\":false}\n",
        )
        .unwrap();
        std::fs::write(bar.join("audio/0001.wav"), b"x").unwrap();

        let (corpus, vocab, skipped) = load_sets(cache, &["foo".into(), "bar".into()]).unwrap();
        assert!(skipped.is_empty(), "both sets must load");
        let ids: Vec<&str> = corpus.cases.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["bar-0001", "foo-0001"],
            "merged ids: plain ids get the prefix once, pre-prefixed ids are left alone"
        );
        assert_eq!(vocab["foo-0001"], vec!["alpha".to_string()]);
        assert!(!vocab.contains_key("foo-foo-0001"));

        // Single-set mode: ids and vocab keys stay exactly as the manifest
        // wrote them.
        let (corpus, vocab, _) = load_sets(cache, &["foo".into()]).unwrap();
        assert_eq!(corpus.cases[0].id, "foo-0001");
        assert_eq!(vocab["foo-0001"], vec!["alpha".to_string()]);
    }

    /// Two-case replay corpus on disk; returns the corpus directory.
    fn write_replay_corpus(dir: &Path, ids: &[(&str, &str)]) -> PathBuf {
        let corpus = dir.join("corpus");
        std::fs::create_dir_all(corpus.join("audio")).unwrap();
        let mut manifest = String::new();
        for (id, reference) in ids {
            write_silent_wav(&corpus.join("audio"), &format!("{id}.wav"), 160);
            manifest.push_str(&format!(
                "{{\"id\":\"{id}\",\"audio\":\"audio/{id}.wav\",\
                 \"reference\":\"{reference}\",\"tags\":[],\"human\":false}}\n"
            ));
        }
        std::fs::write(corpus.join("manifest.jsonl"), manifest).unwrap();
        corpus
    }

    fn replay_config(corpus: PathBuf, hypotheses: PathBuf) -> RunConfig {
        RunConfig {
            corpus: Some(corpus),
            sets: Vec::new(),
            hypotheses: Some(hypotheses),
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
        }
    }

    #[test]
    fn replay_missing_row_is_instrument_invalid_not_a_panic() {
        let dir = TempDir::new().unwrap();
        let corpus = write_replay_corpus(
            dir.path(),
            &[("case-a", "the quick brown fox"), ("case-b", "pack my box")],
        );
        let hyp = dir.path().join("hypotheses.jsonl");
        std::fs::write(
            &hyp,
            "{\"id\":\"case-a\",\"hypothesis\":\"the quick brown cat\"}\n",
        )
        .unwrap();
        let err = match execute(&replay_config(corpus, hyp)) {
            Ok(_) => panic!("a replay that misses a corpus case must fail"),
            Err(e) => e,
        };
        assert!(
            err.contains("INSTRUMENT_INVALID"),
            "missing replay rows must be an instrument verdict: {err}"
        );
        assert!(
            err.contains("case-b"),
            "the error must name the missing case: {err}"
        );
        assert!(
            err.contains("is missing"),
            "the coverage check must report the gap, not a per-case fallback: {err}"
        );
    }

    #[test]
    fn gate_counts_only_checks_with_data() {
        let dir = TempDir::new().unwrap();
        let corpus = write_replay_corpus(
            dir.path(),
            &[("case-a", "the quick brown fox"), ("case-b", "pack my box")],
        );
        let hyp = dir.path().join("hypotheses.jsonl");
        std::fs::write(
            &hyp,
            "{\"id\":\"case-a\",\"hypothesis\":\"the quick brown cat\"}\n\
             {\"id\":\"case-b\",\"hypothesis\":\"pack my box\"}\n",
        )
        .unwrap();
        let out = execute(&replay_config(corpus, hyp)).unwrap();
        let v = serde_json::json!({
            "wer_verbatim_max_regression_pct": 5.0,
            "vocab_recall_min": 0.9,
            "latency_release_to_paste_p95_ms_max": 800.0,
            "wer_clean_human_max": 0.2,
        });
        let mut notes = Vec::new();
        let mut breaches = Vec::new();
        let checks = gate_new_format(&out.report, &v, None, &mut notes, &mut breaches).unwrap();
        assert_eq!(
            checks, 0,
            "no baseline/vocab/traces/clean-human data: zero checks"
        );
        assert_eq!(notes.len(), 4, "each skipped check must leave a note");
        assert!(breaches.is_empty());

        // With the run's own report as baseline the regression check has
        // data (a non-zero verbatim WER), so exactly one check evaluates.
        let baseline = dir.path().join("baseline.json");
        out.report.write_json(&baseline).unwrap();
        let mut notes = Vec::new();
        let mut breaches = Vec::new();
        let checks =
            gate_new_format(&out.report, &v, Some(&baseline), &mut notes, &mut breaches).unwrap();
        assert_eq!(
            checks, 1,
            "the regression check must evaluate against a baseline"
        );
        assert!(breaches.is_empty());

        // Every check with data counts, including the ones that pass.
        let mut full = out.report.clone();
        full.vocab_recall = Some(0.95);
        full.wer_clean_human_micro = Some(0.1);
        full.latency_release_to_paste = Some(crate::latency::LatencySummary {
            count: 10,
            min: 100.0,
            max: 500.0,
            mean: 250.0,
            p50: 240.0,
            p95: 480.0,
            p99: 500.0,
        });
        let mut notes = Vec::new();
        let mut breaches = Vec::new();
        let checks =
            gate_new_format(&full, &v, Some(&baseline), &mut notes, &mut breaches).unwrap();
        assert_eq!(
            checks, 4,
            "regression + vocab + latency + clean human must all evaluate"
        );
        assert!(notes.is_empty(), "no notes when every check has data");
        assert!(breaches.is_empty());
    }
}
