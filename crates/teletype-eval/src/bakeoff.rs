//! Plan §3.8 engine bake-off: the harness side.
//!
//! The bake-off runs every candidate engine × eval set × boost on/off and
//! writes `docs/decisions/D-engine-defaults.md` with the winner per
//! platform. This module holds the pure, weight-free parts of that:
//!
//! - the engine-id → provider mapping ([`resolve_engine`]), with the legacy
//!   `--engine` aliases that the committed baseline was produced with;
//! - the boost-terms helper ([`boost_terms_for`]) that feeds the set's vocab
//!   terms to `set_boost_terms` (weight 2.0, so the rescoring conf gate
//!   opens) or to the post-ASR phonetic rescoring for engines without
//!   native boosting;
//! - the §3.8 per-cell metrics ([`bakeoff_metrics`] / [`BakeoffMetrics`])
//!   added to the run report, with `None` (JSON `null`) for anything not
//!   measurable (e.g. `wer_clean_human` when no human cases were scored);
//! - the decision-table writer ([`decision_markdown`]) that reads the
//!   per-engine `report.json` files and renders the winner table.
//!
//! Nothing here loads a model or touches the network; the measure path is
//! exercised by the driver with a real provider, and every function in this
//! file is unit-testable with synthetic inputs.

use std::path::{Path, PathBuf};

use teletype_speech::BoostTerm;

use crate::report::EvalReport;

/// The §3.8 boosting weight for set vocab terms.
///
/// `teletype_core::rescoring` waives its confidence gate at weight ≥ 2.0,
/// and the plan's boosting axis is "vocab_recall subject to false alarms" —
/// so a boosted term must be able to replace even a high-confidence wrong
/// word. Native-boosting engines (FluidAudio CTC) receive the same weight.
pub const BOOST_WEIGHT: f32 = 2.0;

/// Cap on boost terms per case: the rescoring budget (plan §3.7) is 5,000
/// terms for the whole transcript; a per-case cap keeps a pathological
/// vocab file from blowing it while still covering the Earnings-22-vocab
/// set (738 terms).
pub const MAX_BOOST_TERMS: usize = 5_000;

/// Resolve a `--engine` id to the catalog entry that loads it.
///
/// Accepts a catalog speech-model id (`parakeet-tdt-v3`,
/// `parakeet-ultra-coreml`, `sherpa-parakeet-v3-int8`, `large-v3-turbo-q5`,
/// …) or one of the legacy aliases the baseline was produced with:
/// `parakeet`/`parakeet-ultra` → the ggml Parakeet v3 file, `whisper` →
/// `large-v3-turbo`. `none` (replay mode) is not an engine and stays an
/// error here.
pub fn resolve_engine(
    engine_id: &str,
) -> Result<&'static teletype_speech::catalog::SpeechModel, String> {
    use teletype_speech::catalog::find;
    if let Some(m) = find(engine_id) {
        return Ok(m);
    }
    match engine_id {
        // Legacy aliases: the committed baseline (`evals/runs/baseline`) was
        // produced with `--engine parakeet-ultra` and must stay reproducible.
        "parakeet" | "parakeet-ultra" | "ggml-parakeet" => find("parakeet-tdt-v3"),
        "whisper" => find("large-v3-turbo"),
        _ => None,
    }
    .ok_or_else(|| {
        format!(
            "unknown --engine {engine_id:?}: expected a catalog speech-model id \
             (e.g. parakeet-tdt-v3, parakeet-ultra-coreml, sherpa-parakeet-v3-int8, \
              large-v3-turbo-q5) or a legacy alias (parakeet, parakeet-ultra, whisper)"
        )
    })
}

/// The `--engine` id the bake-off iterates (plan §3.8: macOS candidates are
/// Ultra Core ML vs ggml v3 vs v2; Windows candidates are sherpa v3 vs v2,
/// plus the whisper fallback for reference).
pub const BAKEOFF_ENGINES: &[&str] = &[
    "parakeet-ultra-coreml",
    "parakeet-tdt-v3",
    "parakeet-tdt-v2",
    "sherpa-parakeet-v3-int8",
    "sherpa-parakeet-v2-int8",
    "large-v3-turbo-q5",
];

/// The eval sets the bake-off covers (plan §3.8).
pub const BAKEOFF_SETS: &[&str] = &[
    "human",
    "librispeech-clean",
    "librispeech-other",
    "earnings22-vocab",
    "ami",
    "cv",
    "noise-10db",
    "noise-5db",
    "noise-0db",
    "fleurs-en",
];

/// One cell of the bake-off matrix: which engine, set, and boost to run,
/// where its output lands, and whether it can run at all right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakeoffPlanCell {
    pub engine: String,
    pub set: String,
    pub boost: String,
    /// `true` when the model/weights are on disk (the run can execute).
    pub model_present: bool,
    /// `true` when the output JSON already exists (resumable skip).
    pub output_exists: bool,
    /// The CLI command to run this cell.
    pub command: String,
    /// The output JSON path (resumability anchor).
    pub out_json: String,
}

impl BakeoffPlanCell {
    /// A cell is runnable when its model is present and its output does not
    /// already exist.
    pub fn should_run(&self) -> bool {
        self.model_present && !self.output_exists
    }
}

/// The full bake-off matrix: every engine × set × boost, with the command
/// to run each cell and whether it can run right now.
///
/// `models_dir` is the models root (the app's models directory); `out_root`
/// is where the per-cell run directories land (`evals/runs/bakeoff/…`).
pub fn planned_cells(models_dir: &Path, out_root: &Path) -> Vec<BakeoffPlanCell> {
    let mut out: Vec<BakeoffPlanCell> = Vec::new();
    for engine in BAKEOFF_ENGINES {
        let Ok(model) = resolve_engine(engine) else {
            continue;
        };
        let present = model_present(models_dir, model);
        for set in BAKEOFF_SETS {
            for boost in ["off", "on"] {
                let out_dir = out_root.join(engine).join(format!("{set}-{boost}"));
                let out_json = out_dir.join("report.json");
                let command = format!(
                    "teletype-eval run --engine {engine} --set {set} --boost {boost} \
                     --out {}",
                    out_dir.display()
                );
                out.push(BakeoffPlanCell {
                    engine: engine.to_string(),
                    set: set.to_string(),
                    boost: boost.to_string(),
                    model_present: present,
                    output_exists: out_json.is_file(),
                    command,
                    out_json: out_json.display().to_string(),
                });
            }
        }
    }
    out
}

/// Which provider family loads a catalog entry, and whether it can run on
/// this target at all (the FluidAudio bridge is macOS-Apple-Silicon only;
/// with `TELETYPE_FLUID_DISABLE_SWIFT=1` the crate compiles a stub that
/// refuses to load).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// ggml Parakeet via `teletype_speech::ParakeetProvider`.
    GgmlParakeet,
    /// ggml Whisper via `teletype_speech::whisper::WhisperProvider`.
    GgmlWhisper,
    /// NeMo Parakeet via `teletype_speech::sherpa::SherpaParakeetProvider`.
    Sherpa,
    /// Parakeet Core ML via `teletype_fluid::FluidParakeet` (Neural Engine).
    Fluid,
    /// ggml C++ Parakeet via `teletype_speech::ParakeetCppProvider` (native build).
    ParakeetCpp,
}

pub fn engine_kind(model: &teletype_speech::catalog::SpeechModel) -> EngineKind {
    use teletype_speech::catalog::Engine;
    match model.engine {
        Engine::Parakeet => EngineKind::GgmlParakeet,
        Engine::Whisper => EngineKind::GgmlWhisper,
        Engine::SherpaParakeet => EngineKind::Sherpa,
        Engine::ParakeetCoreml => EngineKind::Fluid,
        Engine::ParakeetCpp => EngineKind::ParakeetCpp,
    }
}

/// Whether the FluidAudio bridge is compiled into this build (macOS 14+
/// Apple Silicon without `TELETYPE_FLUID_DISABLE_SWIFT=1`).
pub fn fluid_bridge_available() -> bool {
    teletype_fluid::bridge_built()
}

/// The model path a measure run loads for `model`, under `models_dir`.
///
/// Mirrors the app's `resolve_speech_model` path semantics exactly:
/// Fluid → `<models_dir>/fluid` (the provider appends the repo folder and
/// downloads on first load), sherpa → `<models_dir>/sherpa/<file>` (the
/// extracted directory, which must contain the ONNX files), ggml →
/// `<models_dir>/<file>`.
pub fn model_path_for(models_dir: &Path, model: &teletype_speech::catalog::SpeechModel) -> PathBuf {
    match engine_kind(model) {
        EngineKind::Fluid => models_dir.join("fluid"),
        EngineKind::Sherpa => models_dir.join("sherpa").join(model.file),
        EngineKind::GgmlParakeet | EngineKind::GgmlWhisper | EngineKind::ParakeetCpp => {
            models_dir.join(model.file)
        }
    }
}

/// Whether the model files for `model` are present under `models_dir`.
///
/// ggml: the single `.bin` exists. sherpa: the extracted directory carries
/// all four ONNX/token files. Fluid: the bridge is compiled in (the
/// checkpoint itself is downloaded by the provider on first load, so
/// presence = the bridge can run at all).
pub fn model_present(models_dir: &Path, model: &teletype_speech::catalog::SpeechModel) -> bool {
    match engine_kind(model) {
        EngineKind::Fluid => fluid_bridge_available(),
        EngineKind::Sherpa => {
            let dir = model_path_for(models_dir, model);
            [
                "encoder.int8.onnx",
                "decoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt",
            ]
            .iter()
            .all(|f| dir.join(f).is_file())
        }
        EngineKind::GgmlParakeet | EngineKind::GgmlWhisper | EngineKind::ParakeetCpp => {
            model_path_for(models_dir, model).is_file()
        }
    }
}

/// The language string to pass to `transcribe` for this engine.
///
/// Parakeet (ggml, Core ML) and sherpa are fixed-language engines: "auto"
/// means English. Whisper can auto-detect, so it keeps "auto".
pub fn asr_language(model: &teletype_speech::catalog::SpeechModel) -> &'static str {
    if engine_kind(model) == EngineKind::GgmlWhisper {
        "auto"
    } else {
        "en"
    }
}

/// Build the boost terms for one case: the case's vocab terms (the
/// Earnings-22-vocab set carries 738; other sets use the reference's proper
/// nouns, which is what their `vocab.json` holds) at weight [`BOOST_WEIGHT`],
/// capped at [`MAX_BOOST_TERMS`].
///
/// `aliases` maps a canonical term to its spoken aliases, passed through
/// unchanged (native boosting and the rescoring Aho-Corasick pass both
/// match on the canonical text *and* its aliases).
pub fn boost_terms_for(
    case_id: &str,
    vocab: &std::collections::HashMap<String, Vec<String>>,
    aliases: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<BoostTerm> {
    let Some(terms) = vocab.get(case_id) else {
        return Vec::new();
    };
    let mut out: Vec<BoostTerm> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for term in terms {
        if out.len() >= MAX_BOOST_TERMS {
            break;
        }
        let key = term.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let alias_list = aliases.get(term);
        let mut alias_vec: Vec<String> = alias_list
            .map(|a| a.iter().filter(|a| !a.is_empty()).cloned().collect())
            .unwrap_or_default();
        // A term without an explicit alias list still matches its own
        // lowercase spelling through the rescorer's pass 1.
        if alias_vec.is_empty() {
            alias_vec.push(term.to_lowercase());
        }
        out.push(BoostTerm {
            text: term.clone(),
            aliases: alias_vec,
            weight: BOOST_WEIGHT,
        });
    }
    out
}

/// Post-ASR boosting for engines without native boosting: run the phonetic
/// rescorer over the `transcribe_detailed` words with the case's boost
/// terms. Returns the boosted text (identical to the input when there are
/// no terms or no words).
///
/// `transcribe_detailed` words carry the engine's per-word confidences
/// (1.0 when the engine reports none); the weight-2.0 terms waive the conf
/// gate, so the phonetic pass can replace a wrong word even at full
/// confidence — that is the documented trade (plan §3.8: "vocab_recall
/// subject to false alarms").
pub fn rescore_text(
    transcript: &str,
    words: &[teletype_speech::Word],
    terms: &[BoostTerm],
) -> String {
    // Engines without word timings (e.g. ggml Parakeet) return the trait
    // default: the plain text with an empty `words` list. Rescoring an
    // empty word list yields an empty string, which the driver would
    // score as a total error (WER 1.0) and report as an all-empty
    // hypothesis — the §3.8 bake-off's earnings22-vocab/tdt-v3/boost-on
    // collapse. When no words are available, the transcript text is
    // returned unchanged: boosting is a no-op boost, never a
    // text-destroying one.
    if words.is_empty() {
        return transcript.to_string();
    }
    let mut texts: Vec<String> = Vec::with_capacity(words.len());
    let mut confs: Vec<f32> = Vec::with_capacity(words.len());
    for w in words {
        texts.push(w.text.clone());
        confs.push(w.conf);
    }
    let rterms: Vec<teletype_core::rescoring::RescoreTerm> = terms
        .iter()
        .map(|t| teletype_core::rescoring::RescoreTerm {
            text: t.text.clone(),
            aliases: t.aliases.clone(),
            weight: t.weight,
        })
        .collect();
    let (out, _replacements) = teletype_core::rescoring::rescore_words(&texts, &confs, &rterms);
    out.join(" ")
}

/// One §3.8 bake-off cell: engine × set × boost.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BakeoffMetrics {
    /// The `--engine` id (catalog id or legacy alias).
    pub engine: String,
    /// `on` or `off`.
    pub boost: String,
    /// Human-corpus clean WER (the decision metric for the platform
    /// default). `None` when the cell scored no human cases with clean
    /// references — the plan says mark it null, never fabricate it.
    #[serde(default)]
    pub wer_clean_human: Option<f64>,
    /// Harness ASR wall time, p50/p95/p95 in ms (the §3.8 latency budget is
    /// on `asr_ms` p95 for a 10 s clip).
    #[serde(default)]
    pub asr_ms_p50: Option<f64>,
    #[serde(default)]
    pub asr_ms_p95: Option<f64>,
    #[serde(default)]
    pub asr_ms_p99: Option<f64>,
    /// Vocab-term recall (the boosting decision metric).
    #[serde(default)]
    pub vocab_recall: Option<f64>,
    /// Vocab-term false-alarm rate (must stay ≤ 0.01 when boosting).
    #[serde(default)]
    pub vocab_false_alarm: Option<f64>,
    /// Verbatim WER (micro) and its delta vs the baseline row
    /// (`None` when this cell or the baseline has no verbatim WER).
    #[serde(default)]
    pub verbatim_wer: Option<f64>,
    #[serde(default)]
    pub verbatim_wer_delta: Option<f64>,
    /// The committed baseline run this cell is compared against.
    #[serde(default)]
    pub baseline: Option<String>,
}

impl BakeoffMetrics {
    /// Compute the §3.8 metrics for one run report.
    ///
    /// `baseline_vw` is the baseline row's `wer_verbatim_micro` (the
    /// committed Gate-0 baseline: 0.0789) and `baseline_name` its label
    /// for the report.
    pub fn from_report(
        report: &EvalReport,
        baseline_vw: Option<f64>,
        baseline_name: Option<String>,
    ) -> Self {
        let boost = report.boost.clone().unwrap_or_else(|| "off".into());
        let wer_clean_human = if report.human_case_count > 0 {
            report.wer_clean_human_micro
        } else {
            // No human cases in this cell: the metric is not measurable.
            // `None` (JSON null), never a number from synth audio.
            None
        };
        let (p50, p95, p99) = match &report.latency_asr {
            Some(l) => (Some(l.p50), Some(l.p95), Some(l.p99)),
            None => (None, None, None),
        };
        let verbatim = report.wer_verbatim_micro;
        let delta = match (verbatim, baseline_vw) {
            (Some(v), Some(b)) => Some(v - b),
            _ => None,
        };
        Self {
            engine: report.model.clone(),
            boost,
            wer_clean_human,
            asr_ms_p50: p50,
            asr_ms_p95: p95,
            asr_ms_p99: p99,
            vocab_recall: report.vocab_recall,
            vocab_false_alarm: report.vocab_false_alarm,
            verbatim_wer: verbatim,
            verbatim_wer_delta: delta,
            baseline: baseline_name,
        }
    }
}

/// One row of the §3.8 decision table (a run directory or report.json).
#[derive(Debug, Clone)]
pub struct BakeoffCell {
    /// `engine/set/boost` label for the table.
    pub label: String,
    pub engine: String,
    pub set: String,
    pub boost: String,
    /// True when the model/weights for this cell are on disk (the run can
    /// actually execute). A cell that cannot run is a skip, not a failure.
    pub model_present: bool,
    /// The output JSON path for this cell (for resumability: a cell whose
    /// output already exists is skipped on re-run).
    pub out_json: String,
    pub metrics: BakeoffMetrics,
}

/// The §3.8 decision bounds (mirrors the `bakeoff` block of
/// `evals/thresholds.json`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakeoffBounds {
    /// macOS: best `wer_clean_human` with `asr_ms` p95 ≤ this (10 s clip, M1).
    pub macos_asr_p95_max_ms: f64,
    /// Windows: best `wer_clean_human` with `asr_ms` p95 ≤ this (10 s clip,
    /// 4-core x64 laptop).
    pub windows_asr_p95_max_ms: f64,
    /// Boosting may be on only when the false-alarm rate stays ≤ this.
    pub boost_false_alarm_max: f64,
    /// …and the verbatim-WER regression (absolute) stays ≤ this.
    pub boost_verbatim_regression_max: f64,
}

impl Default for BakeoffBounds {
    fn default() -> Self {
        Self {
            macos_asr_p95_max_ms: 120.0,
            windows_asr_p95_max_ms: 600.0,
            boost_false_alarm_max: 0.01,
            boost_verbatim_regression_max: 0.003,
        }
    }
}

impl BakeoffBounds {
    /// Read the §3.8 bounds from the `bakeoff` block of a thresholds file;
    /// missing keys fall back to the plan defaults.
    pub fn from_json(v: &serde_json::Value) -> Self {
        let b = v.get("bakeoff");
        let num = |key: &str, default: f64| {
            b.and_then(|b| b.get(key))
                .and_then(|x| x.as_f64())
                .unwrap_or(default)
        };
        Self {
            macos_asr_p95_max_ms: num("macos_asr_p95_ms_max", 120.0),
            windows_asr_p95_max_ms: num("windows_asr_p95_ms_max", 600.0),
            boost_false_alarm_max: num("boost_false_alarm_max", 0.01),
            boost_verbatim_regression_max: num("boost_verbatim_regression_max", 0.003),
        }
    }
}

/// A platform winner, or the reason none of the candidates qualified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Winner {
    /// The winning engine id and the reason it won.
    Pick(String, String),
    /// No candidate met the latency budget (or no human-corpus data).
    None(String),
}

/// Pick the platform default: best `wer_clean_human` among candidates whose
/// `asr_ms` p95 fits the budget. Candidates without a measurable
/// `wer_clean_human` (no human cases) can never win — the plan's default
/// decision is on the human corpus.
pub fn pick_winner(cells: &[BakeoffCell], platform: &str, bounds: &BakeoffBounds) -> Winner {
    let budget = if platform == "macos" {
        bounds.macos_asr_p95_max_ms
    } else if platform == "windows" {
        bounds.windows_asr_p95_max_ms
    } else {
        return Winner::None(format!("unknown platform {platform:?}"));
    };

    // Among the candidates that fit the latency budget, the lowest
    // `wer_clean_human` wins (the plan: "best wer_clean_human within
    // latency budget").
    let mut best: Option<(&BakeoffCell, f64)> = None;
    for cell in cells {
        let Some(wch) = cell.metrics.wer_clean_human else {
            continue;
        };
        let Some(p95) = cell.metrics.asr_ms_p95 else {
            continue;
        };
        if p95 > budget {
            continue;
        }
        match best {
            Some((_, bw)) if wch >= bw => {}
            _ => best = Some((cell, wch)),
        }
    }
    match best {
        Some((cell, wch)) => Winner::Pick(
            cell.engine.clone(),
            format!(
                "lowest wer_clean_human {wch:.4} among candidates with asr p95 ≤ {budget:.0} ms"
            ),
        ),
        None => Winner::None(format!(
            "no candidate has a measurable wer_clean_human with asr p95 ≤ {budget:.0} ms \
             (human corpus empty or latency over budget)"
        )),
    }
}

/// Whether the `boost on` cell is admissible for an engine: false alarms
/// ≤ 1% and verbatim-WER regression ≤ 0.3 pt vs the `boost off` cell.
pub fn boost_admissible(off: &BakeoffMetrics, on: &BakeoffMetrics, bounds: &BakeoffBounds) -> bool {
    let Some(fa) = on.vocab_false_alarm else {
        return false;
    };
    if fa > bounds.boost_false_alarm_max {
        return false;
    }
    let Some(regression) = on.verbatim_wer_delta else {
        return false;
    };
    // The `off` cell anchors the comparison: a boost run that is not paired
    // with its own unboosted twin (different engine) is not admissible.
    if off.engine != on.engine {
        return false;
    }
    regression <= bounds.boost_verbatim_regression_max
}

/// The `D-engine-defaults.md` markdown for a finished bake-off.
///
/// `cells` are the per-engine × set × boost rows (any order; grouped here),
/// `baseline_name` labels the committed baseline the deltas reference.
pub fn decision_markdown(
    cells: &[BakeoffCell],
    bounds: &BakeoffBounds,
    baseline_name: &str,
) -> String {
    let macos = pick_winner(cells, "macos", bounds);
    let windows = pick_winner(cells, "windows", bounds);

    let mut out = String::new();
    out.push_str("# Decision: engine defaults (plan §3.8)\n\n");
    out.push_str(&format!(
        "Generated by `teletype-eval compare --bake-off` from the per-engine run reports. \
         Baseline: `{baseline_name}` (Gate-0 baseline, committed in `evals/runs/baseline/`).\n\n"
    ));
    out.push_str(&format!(
        "Decision bounds: macOS asr p95 ≤ {m:.0} ms, Windows asr p95 ≤ {w:.0} ms \
         (10 s clip); boosting admissible when vocab_false_alarm ≤ {fa:.2} and \
         verbatim-WER regression ≤ {reg:.3}.\n\n",
        m = bounds.macos_asr_p95_max_ms,
        w = bounds.windows_asr_p95_max_ms,
        fa = bounds.boost_false_alarm_max,
        reg = bounds.boost_verbatim_regression_max
    ));

    out.push_str("## Winners\n\n");
    out.push_str("| Platform | Default engine | Reasoning |\n");
    out.push_str("|----------|----------------|-----------|\n");
    push_winner_row(&mut out, "macOS", &macos);
    push_winner_row(&mut out, "Windows", &windows);
    out.push('\n');

    out.push_str("## Full table\n\n");
    out.push_str("| Engine | Set | Boost | wer_clean_human | asr p50/p95/p99 (ms) | vocab recall | vocab FA | verbatim WER (Δ vs baseline) |\n");
    out.push_str("|--------|-----|-------|-----------------|----------------------|--------------|----------|------------------------------|\n");
    for cell in cells {
        let m = &cell.metrics;
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            m.engine,
            cell.set,
            m.boost,
            fmt_opt4(m.wer_clean_human),
            fmt_asr(m.asr_ms_p50, m.asr_ms_p95, m.asr_ms_p99),
            fmt_opt4(m.vocab_recall),
            fmt_opt4(m.vocab_false_alarm),
            fmt_verbatim(m.verbatim_wer, m.verbatim_wer_delta),
        ));
    }
    out.push('\n');

    out.push_str("## Boosting verdict\n\n");
    let mut any = false;
    for (off, on) in boost_pairs(cells) {
        any = true;
        let ok = boost_admissible(&off.metrics, &on.metrics, bounds);
        out.push_str(&format!(
            "- `{}`: {} (false alarms {}, verbatim Δ {})\n",
            on.label,
            if ok { "admissible" } else { "not admissible" },
            fmt_opt4(on.metrics.vocab_false_alarm),
            fmt_delta(on.metrics.verbatim_wer_delta),
        ));
    }
    if !any {
        out.push_str("- (no boost on/off pair present in the run reports)\n");
    }
    out
}

fn push_winner_row(out: &mut String, platform: &str, winner: &Winner) {
    match winner {
        Winner::Pick(engine, reason) => {
            out.push_str(&format!("| {platform} | **{engine}** | {reason} |\n"));
        }
        Winner::None(reason) => {
            out.push_str(&format!("| {platform} | _none yet_ | {reason} |\n"));
        }
    }
}

/// The (boost off, boost on) pairs per engine and set, in first-seen order.
fn boost_pairs(cells: &[BakeoffCell]) -> Vec<(&BakeoffCell, &BakeoffCell)> {
    let mut out: Vec<(&BakeoffCell, &BakeoffCell)> = Vec::new();
    for i in 0..cells.len() {
        if cells[i].metrics.boost != "off" {
            continue;
        }
        for j in 0..cells.len() {
            if cells[j].metrics.boost != "on"
                || cells[j].engine != cells[i].engine
                || cells[j].set != cells[i].set
            {
                continue;
            }
            out.push((&cells[i], &cells[j]));
        }
    }
    out
}

fn fmt_opt4(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.4}"),
        None => "n/a".into(),
    }
}

fn fmt_asr(p50: Option<f64>, p95: Option<f64>, p99: Option<f64>) -> String {
    match (p50, p95, p99) {
        (Some(a), Some(b), Some(c)) => format!("{a:.0}/{b:.0}/{c:.0}"),
        _ => "n/a".into(),
    }
}

fn fmt_verbatim(v: Option<f64>, delta: Option<f64>) -> String {
    match (v, delta) {
        (Some(x), Some(d)) => format!("{x:.4} ({d:+.4})"),
        (Some(x), None) => format!("{x:.4}"),
        (None, _) => "n/a".into(),
    }
}

fn fmt_delta(d: Option<f64>) -> String {
    match d {
        Some(x) => format!("{x:+.4}"),
        None => "n/a".into(),
    }
}

/// Load one bake-off cell from a run directory (or report.json path),
/// reading `meta.json` for the engine/set/boost labels when present.
pub fn load_cell(
    arg: &str,
    baseline_vw: Option<f64>,
    baseline_name: Option<String>,
) -> Result<BakeoffCell, String> {
    let p = Path::new(arg);
    let json = if p.is_dir() {
        p.join("report.json")
    } else {
        p.to_path_buf()
    };
    let content = std::fs::read_to_string(&json)
        .map_err(|e| format!("cannot read {}: {e}", json.display()))?;
    let report: EvalReport =
        serde_json::from_str(&content).map_err(|e| format!("malformed {}: {e}", json.display()))?;
    let meta_path = if p.is_dir() {
        Some(p.join("meta.json"))
    } else {
        json.parent().map(|d| d.join("meta.json"))
    };
    let meta: serde_json::Value = meta_path
        .and_then(|mp| std::fs::read_to_string(mp).ok())
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or(serde_json::Value::Null);
    let engine = meta
        .get("engine")
        .and_then(|v| v.as_str())
        .unwrap_or(&report.model)
        .to_string();
    let set = meta
        .get("sets")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| {
            meta.get("set")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "-".into())
        });
    let boost = report.boost.clone().unwrap_or_else(|| "off".into());
    let metrics = BakeoffMetrics::from_report(&report, baseline_vw, baseline_name);
    let label = format!("{engine}/{set}/{boost}");
    Ok(BakeoffCell {
        label,
        engine,
        set,
        boost,
        model_present: true,
        out_json: json.display().to_string(),
        metrics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn report(base: &str) -> EvalReport {
        serde_json::from_str(base).unwrap()
    }

    const REPORT: &str = r#"{
        "git_sha": "test",
        "timestamp": "1",
        "model": "parakeet-ultra-coreml",
        "mode": "measure",
        "has_synthesised_audio": false,
        "human_case_count": 2,
        "synthesised_case_count": 0,
        "wer": {"cases": 2, "total_reference_words": 100, "total_errors": 5, "micro_wer": 0.05, "macro_wer": 0.06, "worst": []},
        "wer_by_tag": [],
        "polish": [],
        "polish_mean_score": 0.0,
        "latency_asr": {"count": 2, "min": 50.0, "max": 100.0, "mean": 75.0, "p50": 75.0, "p95": 95.0, "p99": 99.0},
        "latency_total": null,
        "model_load_ms": null,
        "cases": [],
        "wer_verbatim_micro": 0.08,
        "wer_verbatim_macro": 0.08,
        "wer_clean_micro": 0.03,
        "wer_clean_human_micro": 0.02,
        "vocab_recall": 0.92,
        "vocab_false_alarm": 0.005,
        "entity_accuracy": 0.9,
        "semantic_ok_rate": null,
        "latency_release_to_paste": null,
        "case_errors": []
    }"#;

    #[test]
    fn resolve_engine_maps_catalog_ids_and_legacy_aliases() {
        let m = resolve_engine("parakeet-tdt-v3").unwrap();
        assert_eq!(m.id, "parakeet-tdt-v3");
        let m = resolve_engine("parakeet-ultra-coreml").unwrap();
        assert_eq!(m.id, "parakeet-ultra-coreml");
        let m = resolve_engine("sherpa-parakeet-v3-int8").unwrap();
        assert_eq!(m.id, "sherpa-parakeet-v3-int8");
        let m = resolve_engine("large-v3-turbo-q5").unwrap();
        assert_eq!(m.id, "large-v3-turbo-q5");
        // Legacy aliases (the baseline was produced with these).
        let m = resolve_engine("parakeet-ultra").unwrap();
        assert_eq!(m.id, "parakeet-tdt-v3");
        let m = resolve_engine("parakeet").unwrap();
        assert_eq!(m.id, "parakeet-tdt-v3");
        let m = resolve_engine("whisper").unwrap();
        assert_eq!(m.id, "large-v3-turbo");
    }

    #[test]
    fn resolve_engine_rejects_unknown_ids_with_a_clean_error() {
        let err = resolve_engine("no-such-engine").unwrap_err();
        assert!(err.contains("unknown --engine"), "{err}");
        assert!(err.contains("no-such-engine"), "{err}");
        let err = resolve_engine("none").unwrap_err();
        assert!(err.contains("unknown --engine"), "{err}");
    }

    #[test]
    fn engine_kind_and_language_follow_the_catalog() {
        let m = resolve_engine("parakeet-tdt-v3").unwrap();
        assert_eq!(engine_kind(m), EngineKind::GgmlParakeet);
        assert_eq!(asr_language(m), "en");
        let m = resolve_engine("parakeet-ultra-coreml").unwrap();
        assert_eq!(engine_kind(m), EngineKind::Fluid);
        assert_eq!(asr_language(m), "en");
        let m = resolve_engine("sherpa-parakeet-v3-int8").unwrap();
        assert_eq!(engine_kind(m), EngineKind::Sherpa);
        let m = resolve_engine("large-v3-turbo-q5").unwrap();
        assert_eq!(engine_kind(m), EngineKind::GgmlWhisper);
        assert_eq!(asr_language(m), "auto");
    }

    #[test]
    fn model_path_and_presence_follow_app_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let m = resolve_engine("parakeet-tdt-v3").unwrap();
        assert_eq!(
            model_path_for(dir.path(), m),
            dir.path().join("ggml-parakeet-tdt-0.6b-v3-q8_0.bin")
        );
        assert!(!model_present(dir.path(), m));
        std::fs::write(model_path_for(dir.path(), m), b"x").unwrap();
        assert!(model_present(dir.path(), m));

        let s = resolve_engine("sherpa-parakeet-v3-int8").unwrap();
        assert_eq!(
            model_path_for(dir.path(), s),
            dir.path().join("sherpa").join(s.file)
        );
        assert!(!model_present(dir.path(), s));

        let f = resolve_engine("parakeet-ultra-coreml").unwrap();
        assert_eq!(model_path_for(dir.path(), f), dir.path().join("fluid"));
    }

    #[test]
    fn boost_terms_are_weighted_capped_and_alias_passthrough() {
        let mut vocab = HashMap::new();
        vocab.insert(
            "c1".to_string(),
            vec![
                "Alpha Corp".to_string(),
                "beta".to_string(),
                "beta".to_string(),
                "Gamma".to_string(),
            ],
        );
        let mut aliases = HashMap::new();
        aliases.insert(
            "Alpha Corp".to_string(),
            vec!["alpha corp".to_string(), "al phab corp".to_string()],
        );
        let terms = boost_terms_for("c1", &vocab, &aliases);
        assert_eq!(terms.len(), 3, "duplicates collapse");
        assert_eq!(terms[0].text, "Alpha Corp");
        assert_eq!(terms[0].aliases, vec!["alpha corp", "al phab corp"]);
        assert_eq!(terms[0].weight, BOOST_WEIGHT);
        assert_eq!(terms[1].text, "beta");
        assert_eq!(
            terms[1].aliases,
            vec!["beta"],
            "no alias entry → own lowercase"
        );
        assert_eq!(terms[1].weight, 2.0);
        assert!(boost_terms_for("missing", &vocab, &aliases).is_empty());
    }

    #[test]
    fn boost_terms_are_capped() {
        let mut vocab = HashMap::new();
        vocab.insert(
            "c1".to_string(),
            (0..6_000).map(|i| format!("term-{i}")).collect(),
        );
        let terms = boost_terms_for("c1", &vocab, &HashMap::new());
        assert_eq!(terms.len(), MAX_BOOST_TERMS);
    }

    #[test]
    fn rescore_text_replaces_low_confidence_spelling() {
        let words = vec![teletype_speech::Word {
            text: "kubernetez".into(),
            start_ms: 0,
            end_ms: 100,
            conf: 0.5,
        }];
        let terms = vec![BoostTerm {
            text: "Kubernetes".into(),
            aliases: vec!["kubernetez".into()],
            weight: 2.0,
        }];
        assert_eq!(rescore_text("kubernetez", &words, &terms), "Kubernetes");
        // No terms: unchanged.
        assert_eq!(rescore_text("kubernetez", &words, &[]), "kubernetez");
        // No words but a transcript (the ggml Parakeet shape: the trait
        // default returns text with an empty word list): the transcript
        // must come back unchanged, never as an empty string. The
        // earnings22-vocab/tdt-v3/boost-on bake-off cell collapsed to all
        // empty hypotheses before this guard.
        assert_eq!(
            rescore_text("good morning and welcome", &[], &terms),
            "good morning and welcome"
        );
        // No words and no transcript: empty (nothing to boost).
        assert_eq!(rescore_text("", &[], &terms), "");
    }

    #[test]
    fn bakeoff_metrics_report_shape_and_nulls() {
        let r = report(REPORT);
        let m = BakeoffMetrics::from_report(&r, Some(0.0789), Some("baseline".into()));
        assert_eq!(m.engine, "parakeet-ultra-coreml");
        assert_eq!(m.boost, "off");
        assert_eq!(m.wer_clean_human, Some(0.02));
        assert_eq!(m.asr_ms_p50, Some(75.0));
        assert_eq!(m.asr_ms_p95, Some(95.0));
        assert_eq!(m.vocab_recall, Some(0.92));
        let d = m.verbatim_wer_delta.unwrap();
        assert!((d - (0.08 - 0.0789)).abs() < 1e-9, "delta {d}");
        // No baseline → no delta.
        let m2 = BakeoffMetrics::from_report(&r, None, None);
        assert_eq!(m2.verbatim_wer_delta, None);
        // No human cases → wer_clean_human is null, not fabricated.
        let mut r = r.clone();
        r.human_case_count = 0;
        let m3 = BakeoffMetrics::from_report(&r, Some(0.0789), None);
        assert_eq!(m3.wer_clean_human, None);
        // Serializes with the new keys present and null when absent.
        let v = serde_json::to_value(&m3).unwrap();
        assert!(v.get("wer_clean_human").is_some());
        assert!(v["wer_clean_human"].is_null());
        assert!(v.get("asr_ms_p95").is_some());
        assert!(v.get("verbatim_wer_delta").is_some());
    }

    #[allow(clippy::too_many_arguments)] // test cell constructor: 8 fixed codec columns
    fn cell(
        engine: &str,
        set: &str,
        boost: &str,
        wch: Option<f64>,
        p95: Option<f64>,
        fa: Option<f64>,
        vw: Option<f64>,
        vd: Option<f64>,
    ) -> BakeoffCell {
        BakeoffCell {
            label: format!("{engine}/{set}/{boost}"),
            engine: engine.into(),
            set: set.into(),
            boost: boost.into(),
            model_present: true,
            out_json: format!("{engine}/{set}/{boost}/report.json"),
            metrics: BakeoffMetrics {
                engine: engine.into(),
                boost: boost.into(),
                wer_clean_human: wch,
                asr_ms_p50: p95.map(|x| x / 2.0),
                asr_ms_p95: p95,
                asr_ms_p99: p95.map(|x| x * 1.2),
                vocab_recall: Some(0.9),
                vocab_false_alarm: fa,
                verbatim_wer: vw,
                verbatim_wer_delta: vd,
                baseline: Some("baseline".into()),
            },
        }
    }

    #[test]
    fn winner_honors_the_p95_budget_and_human_metric() {
        let bounds = BakeoffBounds::default();
        let cells = vec![
            // Over the macOS budget.
            cell(
                "parakeet-tdt-v3",
                "human",
                "off",
                Some(0.02),
                Some(430.0),
                Some(0.0),
                Some(0.0789),
                None,
            ),
            // Under budget, worse WER.
            cell(
                "sherpa-parakeet-v3-int8",
                "human",
                "off",
                Some(0.05),
                Some(90.0),
                Some(0.0),
                Some(0.08),
                None,
            ),
            // Under budget, best WER.
            cell(
                "parakeet-ultra-coreml",
                "human",
                "off",
                Some(0.03),
                Some(110.0),
                Some(0.0),
                Some(0.079),
                None,
            ),
            // No human data: can never win.
            cell(
                "large-v3-turbo-q5",
                "human",
                "off",
                None,
                Some(50.0),
                Some(0.0),
                Some(0.07),
                None,
            ),
        ];
        match pick_winner(&cells, "macos", &bounds) {
            Winner::Pick(e, reason) => {
                assert_eq!(e, "parakeet-ultra-coreml");
                assert!(reason.contains("120"), "{reason}");
            }
            other => panic!("expected a pick, got {other:?}"),
        }
        // Windows budget admits the ggml v3 cell too; it has the lowest
        // wer_clean_human, so it wins on Windows.
        match pick_winner(&cells, "windows", &bounds) {
            Winner::Pick(e, _) => assert_eq!(e, "parakeet-tdt-v3"),
            other => panic!("expected a pick, got {other:?}"),
        }
        // Nobody under budget → None.
        let cells2 = vec![cell(
            "parakeet-tdt-v3",
            "human",
            "off",
            Some(0.02),
            Some(430.0),
            None,
            None,
            None,
        )];
        match pick_winner(&cells2, "macos", &bounds) {
            Winner::None(reason) => assert!(reason.contains("120"), "{reason}"),
            other => panic!("expected None, got {other:?}"),
        }
    }

    #[test]
    fn boost_admissibility_applies_the_bounds() {
        let bounds = BakeoffBounds::default();
        let off = cell(
            "e",
            "s",
            "off",
            Some(0.03),
            Some(100.0),
            None,
            Some(0.08),
            None,
        )
        .metrics;
        let on = cell(
            "e",
            "s",
            "on",
            Some(0.029),
            Some(101.0),
            Some(0.005),
            Some(0.082),
            Some(0.002),
        )
        .metrics;
        assert!(boost_admissible(&off, &on, &bounds));
        // False alarms over 1%.
        let on = cell(
            "e",
            "s",
            "on",
            Some(0.029),
            Some(101.0),
            Some(0.02),
            Some(0.082),
            Some(0.002),
        )
        .metrics;
        assert!(!boost_admissible(&off, &on, &bounds));
        // Verbatim regression over 0.3 pt.
        let on = cell(
            "e",
            "s",
            "on",
            Some(0.029),
            Some(101.0),
            Some(0.005),
            Some(0.084),
            Some(0.004),
        )
        .metrics;
        assert!(!boost_admissible(&off, &on, &bounds));
        // Missing data is not admissible.
        let on = cell(
            "e",
            "s",
            "on",
            Some(0.029),
            Some(101.0),
            None,
            Some(0.082),
            Some(0.002),
        )
        .metrics;
        assert!(!boost_admissible(&off, &on, &bounds));
        let on = cell(
            "e",
            "s",
            "on",
            Some(0.029),
            Some(101.0),
            Some(0.005),
            Some(0.082),
            None,
        )
        .metrics;
        assert!(!boost_admissible(&off, &on, &bounds));
    }

    #[test]
    fn decision_markdown_renders_winners_and_table() {
        let bounds = BakeoffBounds::default();
        let cells = vec![
            cell(
                "parakeet-ultra-coreml",
                "human",
                "off",
                Some(0.03),
                Some(110.0),
                Some(0.0),
                Some(0.079),
                Some(0.0001),
            ),
            cell(
                "parakeet-ultra-coreml",
                "human",
                "on",
                Some(0.029),
                Some(112.0),
                Some(0.005),
                Some(0.08),
                Some(0.0011),
            ),
            cell(
                "parakeet-tdt-v3",
                "human",
                "off",
                Some(0.02),
                Some(430.0),
                Some(0.0),
                Some(0.0789),
                Some(0.0),
            ),
        ];
        let md = decision_markdown(&cells, &bounds, "baseline");
        assert!(
            md.contains("# Decision: engine defaults (plan §3.8)"),
            "\n{md}"
        );
        assert!(
            md.contains("| macOS | **parakeet-ultra-coreml** |"),
            "\n{md}"
        );
        assert!(
            md.contains("parakeet-tdt-v3 | human | off | 0.0200 | 215/430/516 |"),
            "\n{md}"
        );
        assert!(md.contains("(+0.0001)"), "\n{md}");
        assert!(
            md.contains("`parakeet-ultra-coreml/human/on`: admissible"),
            "\n{md}"
        );
        // The over-budget ggml engine is not the macOS winner.
        let macos_line = md.lines().find(|l| l.starts_with("| macOS |")).unwrap();
        assert!(!macos_line.contains("parakeet-tdt-v3"), "{macos_line}");
    }

    #[test]
    fn decision_markdown_reports_no_winner_when_data_missing() {
        let bounds = BakeoffBounds::default();
        let cells = vec![cell(
            "parakeet-tdt-v3",
            "human",
            "off",
            None,
            Some(430.0),
            None,
            Some(0.0789),
            None,
        )];
        let md = decision_markdown(&cells, &bounds, "baseline");
        assert!(md.contains("| macOS | _none yet_ |"), "\n{md}");
        assert!(
            md.contains("no candidate has a measurable wer_clean_human"),
            "\n{md}"
        );
    }

    #[test]
    fn load_cell_reads_meta_and_report() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("report.json"), REPORT).unwrap();
        std::fs::write(
            dir.path().join("meta.json"),
            r#"{"engine":"parakeet-ultra-coreml","sets":["human"],"live":false}"#,
        )
        .unwrap();
        let c = load_cell(
            dir.path().to_str().unwrap(),
            Some(0.0789),
            Some("baseline".into()),
        )
        .unwrap();
        assert_eq!(c.engine, "parakeet-ultra-coreml");
        assert_eq!(c.set, "human");
        assert_eq!(c.boost, "off");
        assert_eq!(c.label, "parakeet-ultra-coreml/human/off");
        assert_eq!(c.metrics.wer_clean_human, Some(0.02));
    }

    #[test]
    fn load_cell_falls_back_to_report_model_without_meta() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("report.json"), REPORT).unwrap();
        let c = load_cell(dir.path().to_str().unwrap(), None, None).unwrap();
        assert_eq!(c.engine, "parakeet-ultra-coreml");
        assert_eq!(c.set, "-");
        assert_eq!(c.metrics.verbatim_wer_delta, None);
    }

    #[test]
    fn planned_cells_covers_engine_set_boost_matrix() {
        let dir = tempfile::tempdir().unwrap();
        let out_root = dir.path().join("runs");
        let cells = planned_cells(dir.path(), &out_root);
        // 6 engines × 10 sets × 2 boosts = 120 cells.
        assert_eq!(cells.len(), 120, "full matrix");
        for c in &cells {
            assert!(!c.command.is_empty());
            assert!(c.command.contains("--boost"));
            assert!(c.command.contains("--set"));
            assert!(c.out_json.ends_with("report.json"));
        }
        // Fluid cells are "present" only when the bridge is compiled in
        // (macOS Apple Silicon without TELETYPE_FLUID_DISABLE_SWIFT=1);
        // every ggml/sherpa cell is absent on a bare temp dir.
        let fluid_ok = fluid_bridge_available();
        for c in &cells {
            let is_fluid = c.engine == "parakeet-ultra-coreml";
            if is_fluid && fluid_ok {
                assert!(c.model_present, "fluid cell present with bridge");
            } else {
                assert!(!c.model_present, "cell {} should be absent", c.engine);
                assert!(!c.should_run(), "cell {} should not run", c.engine);
            }
        }
    }

    #[test]
    fn planned_cells_skip_existing_output_and_missing_models() {
        let dir = tempfile::tempdir().unwrap();
        let out_root = dir.path().join("runs");
        // Put one model on disk (the ggml parakeet v3 file).
        let m = resolve_engine("parakeet-tdt-v3").unwrap();
        std::fs::write(model_path_for(dir.path(), m), b"x").unwrap();
        // Pre-create the output for one cell (parakeet-tdt-v3/human/off).
        let done = out_root.join("parakeet-tdt-v3").join("human-off");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(done.join("report.json"), "{}").unwrap();

        let cells = planned_cells(dir.path(), &out_root);
        let v3: Vec<&BakeoffPlanCell> = cells
            .iter()
            .filter(|c| c.engine == "parakeet-tdt-v3")
            .collect();
        // 10 sets × 2 boosts = 20 cells for v3; all model-present.
        assert_eq!(v3.len(), 20);
        assert!(v3.iter().all(|c| c.model_present));
        // The pre-done cell is skipped; the other 19 should run.
        let done_cell = v3
            .iter()
            .find(|c| c.set == "human" && c.boost == "off")
            .unwrap();
        assert!(done_cell.output_exists);
        assert!(!done_cell.should_run());
        let running = v3.iter().filter(|c| c.should_run()).count();
        assert_eq!(running, 19, "one cell skipped, 19 to run");
        // Engines without models on disk never run (fluid excepted when
        // the bridge is compiled in — its checkpoint downloads on load).
        let sherpa: Vec<&BakeoffPlanCell> = cells
            .iter()
            .filter(|c| c.engine == "sherpa-parakeet-v3-int8")
            .collect();
        assert!(!sherpa.is_empty());
        assert!(sherpa.iter().all(|c| !c.model_present));
        assert!(sherpa.iter().all(|c| !c.should_run()));
    }

    #[test]
    fn bounds_from_json_reads_bakeoff_block() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"bakeoff":{"macos_asr_p95_ms_max":150,"windows_asr_p95_ms_max":700,
               "boost_false_alarm_max":0.02,"boost_verbatim_regression_max":0.005}}"#,
        )
        .unwrap();
        let b = BakeoffBounds::from_json(&v);
        assert_eq!(b.macos_asr_p95_max_ms, 150.0);
        assert_eq!(b.windows_asr_p95_max_ms, 700.0);
        assert_eq!(b.boost_false_alarm_max, 0.02);
        assert_eq!(b.boost_verbatim_regression_max, 0.005);
        // No bakeoff block → defaults.
        let b2 = BakeoffBounds::from_json(&serde_json::json!({}));
        assert_eq!(b2, BakeoffBounds::default());
    }
}
