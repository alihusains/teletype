//! Per-stage dictation timing (Beat Wispr Flow plan §P0.2) and the in-app
//! per-stage latency percentiles (U2).
//!
//! Every dictation take fills one [`DictationTrace`] with monotonic marks at
//! each stage boundary (hotkey down, first audio, release, ASR start/end,
//! text-pipeline end, polish start/first-token/end, paste sent). At the
//! single completion attach point the controller derives per-stage durations
//! ([`DictationTrace::derive_stage_ms`]) from those marks, so a stage that
//! did not run is absent, never 0. [`latency_summary`] then computes p50/p95
//! per stage over a bounded recent window (nearest-rank, never
//! interpolated) for the Insights latency panel. Finished traces are appended
//! as one JSON line each to `<app_data>/traces/YYYY-MM-DD.jsonl`.
//!
//! The writer is deliberately boring: best-effort, never panics, never
//! blocks delivery. A failed write loses a measurement, never a dictation.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Process-start anchor for the single monotonic clock. Every mark is
/// microseconds since this instant, so marks are comparable across threads
/// and immune to wall-clock jumps (NTP, timezone changes).
static BOOT: OnceLock<Instant> = OnceLock::new();

/// Current time in microseconds on the single monotonic clock.
pub fn now_us() -> u64 {
    let boot = BOOT.get_or_init(Instant::now);
    boot.elapsed().as_micros() as u64
}

/// One take's per-stage timing. Field order is the pipeline order; every
/// `t_*` mark is monotonic microseconds from [`now_us`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DictationTrace {
    pub session: u64,
    /// Engine id, e.g. the selected speech-model id.
    pub engine: String,
    /// Measured speaking time (mic open to release).
    pub audio_ms: u32,
    pub t_hotkey_down: Option<u64>,
    pub t_first_audio: Option<u64>,
    pub t_hotkey_up: Option<u64>,
    pub t_asr_start: Option<u64>,
    pub t_asr_end: Option<u64>,
    pub t_text_pipeline_end: Option<u64>,
    pub t_polish_start: Option<u64>,
    pub t_polish_first_token: Option<u64>,
    pub t_polish_end: Option<u64>,
    pub t_paste_sent: Option<u64>,
    pub polish_skipped_reason: Option<String>,
    /// Segments the streaming committer committed mid-take (P4). 0 on
    /// whole-take fallback paths.
    pub committed_segments: u32,
    /// Audio length in ms of the uncommitted tail the release decodes (P4).
    /// 0 on whole-take fallback paths.
    pub tail_audio_ms: u32,
    /// U2: measured per-stage durations in milliseconds, derived from the
    /// `t_*` marks above at the one place the trace is finished. A stage
    /// that did not run is ABSENT from the map, never 0: zero latency is a
    /// claim, and "not measured" is the truth. Old trace lines (pre-U2)
    /// parse with an empty map.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub stage_ms: std::collections::BTreeMap<String, u32>,
    pub errors: Vec<String>,
    /// P2.9 command-take lifecycle, in order: `armed`, `started`,
    /// `classified=<intent>`, then `executed` or `refused(<reason>)`.
    /// Controller-side stages are pushed directly; worker-side stages
    /// arrive in [`TraceStageMarks::command_events`] and are appended by
    /// [`DictationTrace::apply_stage_marks`]. Empty (and omitted from the
    /// JSON line) for plain dictation takes with no command routing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_mode: Vec<String>,
    /// P5.7: per-take audio quality signals, computed by
    /// `teletype_core::audio::quality::take_quality`. `None` until the
    /// dictation-wiring unit attaches values; old traces stay valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_rms_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_rms_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bt_hfp: Option<bool>,
    /// A.3.3: which command decider decided this take — "neudecide",
    /// "needle" (the Needle 3 / LLM-fallback extraction path), or
    /// "needle-fallback" (the decider was unavailable or errored and the
    /// take fell back to the existing Needle path). `None` for takes that
    /// never ran the command route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decider: Option<String>,
    /// A.3.3: wall-clock milliseconds spent in the decider's `decide` call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decider_ms: Option<u32>,
    /// A.3.3: the tool the decider chose, e.g. "web_search" or "no_action".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// A.3.3: where the command arguments came from — "neudecide" (the
    /// model emitted them), "transcript" (filled from the ASR text by the
    /// resolver's rules), or "none" (no arguments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_source: Option<String>,
}

impl DictationTrace {
    pub fn new(session: u64, engine: impl Into<String>) -> Self {
        Self {
            session,
            engine: engine.into(),
            ..Default::default()
        }
    }

    /// Stamp the current monotonic time into whichever fields `f` sets.
    pub fn mark(&mut self, f: impl FnOnce(&mut Self, u64)) {
        f(self, now_us())
    }

    /// Merge the marks collected on the pipeline worker thread.
    pub fn apply_stage_marks(&mut self, m: TraceStageMarks) {
        self.t_polish_start = m.t_polish_start;
        self.t_polish_first_token = m.t_polish_first_token;
        self.t_polish_end = m.t_polish_end;
        self.t_text_pipeline_end = m.t_text_pipeline_end;
        self.polish_skipped_reason = m.polish_skipped_reason;
        self.committed_segments = m.committed_segments;
        self.tail_audio_ms = m.tail_audio_ms;
        // P2.9: worker-side command events append after the controller-side
        // ones (`armed`, `started`) that were pushed before the worker ran.
        self.command_mode.extend(m.command_events);
    }

    /// Release-to-paste latency in microseconds, the headline Gate 0 metric.
    pub fn release_to_paste_us(&self) -> Option<u64> {
        match (self.t_hotkey_up, self.t_paste_sent) {
            (Some(up), Some(paste)) => paste.checked_sub(up),
            _ => None,
        }
    }

    /// U2: fill [`DictationTrace::stage_ms`] from the `t_*` marks. Called
    /// once, at the single attach point where a trace is finished (every
    /// completion path: success, refusal, cancel, supersede), so the
    /// derivation lives in exactly one place. A stage whose boundary marks
    /// are missing (or non-monotonic) is absent, never zero: the stage did
    /// not run, and 0 ms would be a false measurement.
    pub fn derive_stage_ms(&mut self) {
        let us = |a: Option<u64>, b: Option<u64>| -> Option<u32> {
            match (a, b) {
                (Some(a), Some(b)) if b >= a => Some(((b - a) / 1000).min(u32::MAX as u64) as u32),
                _ => None,
            }
        };
        let mut m: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        if let Some(v) = us(self.t_hotkey_down, self.t_hotkey_up) {
            m.insert(LatencyStage::Capture.id().to_string(), v);
        }
        if let Some(v) = us(self.t_hotkey_up, self.t_text_pipeline_end) {
            m.insert(LatencyStage::Asr.id().to_string(), v);
        }
        if let Some(v) = us(self.t_polish_start, self.t_polish_first_token) {
            m.insert(LatencyStage::PolishFirstToken.id().to_string(), v);
        }
        if let Some(v) = us(self.t_polish_start, self.t_polish_end) {
            m.insert(LatencyStage::Polish.id().to_string(), v);
        }
        if let Some(v) = us(self.t_text_pipeline_end, self.t_paste_sent) {
            m.insert(LatencyStage::Inject.id().to_string(), v);
        }
        if let Some(v) = us(self.t_hotkey_up, self.t_paste_sent) {
            m.insert(LatencyStage::Total.id().to_string(), v);
        }
        self.stage_ms = m;
    }
}

/// Marks collected on the pipeline worker thread, where the controller's
/// trace is unreachable. Travels to the controller inside the pipeline
/// payload and is merged with [`DictationTrace::apply_stage_marks`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TraceStageMarks {
    pub t_polish_start: Option<u64>,
    pub t_polish_first_token: Option<u64>,
    pub t_polish_end: Option<u64>,
    pub t_text_pipeline_end: Option<u64>,
    pub polish_skipped_reason: Option<String>,
    pub committed_segments: u32,
    pub tail_audio_ms: u32,
    /// P2.9 command-take lifecycle events decided on the pipeline worker
    /// (`classified=<intent>`, `executed`, `refused(<reason>)`); appended
    /// to [`DictationTrace::command_mode`] on merge, in order.
    pub command_events: Vec<String>,
}

/// Abstraction over "where finished traces go" so tests can observe the
/// write ordering without touching the filesystem.
pub trait TraceSink {
    fn write_trace(&self, trace: &DictationTrace);
}

/// Appends each finished trace as one JSON line to
/// `<app_data>/traces/YYYY-MM-DD.jsonl`. Best-effort throughout: every
/// failure is swallowed, because losing a measurement must never break
/// delivery of the user's text.
pub struct FileTraceSink {
    dir: PathBuf,
}

/// Keep 7 days of traces; prune older files on every write.
const RETENTION_DAYS: u64 = 7;
/// Cap the traces directory at 50 MB; delete oldest first when over.
const MAX_DIR_BYTES: u64 = 50 * 1024 * 1024;

impl FileTraceSink {
    pub fn new(app_data: &Path) -> Self {
        Self {
            dir: app_data.join("traces"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for_today(&self) -> PathBuf {
        let (y, m, d) = today_ymd();
        self.dir.join(format!("{y:04}-{m:02}-{d:02}.jsonl"))
    }

    fn prune(&self) {
        use std::time::Duration;
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file || path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            // Age out files older than the retention window. The file name
            // is the date, but mtime is the honest signal for "old".
            if SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO)
                > Duration::from_secs(RETENTION_DAYS * 86400)
            {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            files.push((path, size, modified));
        }
        // Cap total size: delete oldest first until under budget.
        let mut total: u64 = files.iter().map(|(_, s, _)| *s).sum();
        files.sort_by_key(|(_, _, m)| *m);
        for (path, size, _) in files {
            if total <= MAX_DIR_BYTES {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
}

impl TraceSink for FileTraceSink {
    fn write_trace(&self, trace: &DictationTrace) {
        // Serialize first: a trace that cannot serialize is dropped without
        // ever touching the directory, so a bad trace can't wedge rotation.
        let Ok(mut line) = serde_json::to_string(trace) else {
            return;
        };
        line.push('\n');
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path_for_today())
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            })
            .is_err()
        {
            return;
        }
        self.prune();
    }
}

/// U2: the per-stage latency stages of the dictation pipeline, in PIPELINE
/// order (not declaration order). Each stage carries a stable string id
/// (the key in [`DictationTrace::stage_ms`]), a display label, whether it is
/// user-facing (diagnostic stages can be recorded without being shown), and
/// its pipeline order.
///
/// `Total` is the stage the release budget of 700 ms p95 applies to
/// (`release_to_paste`), so the panel can show the promise next to the
/// measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyStage {
    /// Hotkey down to hotkey up: the mic was open and the user was speaking.
    Capture,
    /// Hotkey up to text-pipeline end: ASR decode plus the deterministic
    /// text pipeline (dictionary, ITN, AutoText) before polish.
    Asr,
    /// Polish start to first token: the LLM's time-to-first-token.
    PolishFirstToken,
    /// Polish start to polish end: the full LLM polish pass. Absent when
    /// polish was skipped by the gate (or no model was loaded).
    Polish,
    /// Text-pipeline end to paste sent: delivery verdict + injection.
    Inject,
    /// Hotkey up to paste sent: release-to-paste, the headline budget stage.
    Total,
}

impl LatencyStage {
    /// The stable string id, used as the key in [`DictationTrace::stage_ms`].
    pub fn id(self) -> &'static str {
        match self {
            LatencyStage::Capture => "capture",
            LatencyStage::Asr => "asr",
            LatencyStage::PolishFirstToken => "polish_first_token",
            LatencyStage::Polish => "polish",
            LatencyStage::Inject => "inject",
            LatencyStage::Total => "total",
        }
    }

    /// The human label shown in the panel.
    pub fn label(self) -> &'static str {
        match self {
            LatencyStage::Capture => "Capture",
            LatencyStage::Asr => "Transcribe",
            LatencyStage::PolishFirstToken => "Polish (first token)",
            LatencyStage::Polish => "Polish (full)",
            LatencyStage::Inject => "Inject",
            LatencyStage::Total => "Release to paste",
        }
    }

    /// Whether the stage is shown in the panel. Diagnostic stages can be
    /// recorded without being shown; re-deciding that in the UI would put a
    /// second opinion next to the declaration.
    pub fn user_facing(self) -> bool {
        !matches!(self, LatencyStage::PolishFirstToken)
    }

    /// Pipeline order, used to sort the panel rows. Not declaration order.
    pub fn order(self) -> u8 {
        match self {
            LatencyStage::Capture => 0,
            LatencyStage::Asr => 1,
            LatencyStage::PolishFirstToken => 2,
            LatencyStage::Polish => 3,
            LatencyStage::Inject => 4,
            LatencyStage::Total => 5,
        }
    }

    /// Every stage, in pipeline order. The aggregation iterates this so the
    /// summary is stable regardless of which stages have samples.
    pub const ALL: [LatencyStage; 6] = [
        LatencyStage::Capture,
        LatencyStage::Asr,
        LatencyStage::PolishFirstToken,
        LatencyStage::Polish,
        LatencyStage::Inject,
        LatencyStage::Total,
    ];
}

/// U2: the release-to-paste budget in p95 ms, the promise the panel shows
/// next to the `Total` stage's measurement. Mirrors
/// `evals/thresholds.json` `latency_release_to_paste_p95_ms_max = 700`.
///
/// That threshold was set against the short, clean clips of the eval corpus, so
/// it is only a promise for short takes. [`SHORT_TAKE_AUDIO_MS`] is how the
/// panel keeps that straight instead of comparing an all-lengths p95 to a
/// short-clip number and reporting a violation that is really a length effect.
pub const RELEASE_TO_PASTE_P95_MS_MAX: u32 = 700;

/// A take short enough to be compared against [`RELEASE_TO_PASTE_P95_MS_MAX`].
/// Five seconds is the length the eval clips sit under, and the first real
/// latency split on this machine (window of 200, across all engines, release to
/// paste p95): under 5 s it is 1399 ms, from 5 to 15 s it is 2443 ms, and at
/// 15 s and up it is 6978 ms.
pub const SHORT_TAKE_AUDIO_MS: u32 = 5_000;

/// The fewest short takes needed before [`SHORT_TAKE_ENGINES`] is worth
/// consulting. A p95 over a handful of samples is noise, so below this the
/// figure stays engine-blind, which is honest because the panel quotes the
/// sample count next to it.
pub const SHORT_TAKE_ENGINE_MIN_TAKES: u32 = 20;

/// The engines with a known per-engine short-take split, so the panel can name
/// the one a scoped figure was measured on.
///
/// The second condition the eval corpus sat under, after length: one engine.
/// The first real per-engine split on this machine (window of 200, takes under
/// 5 s, release-to-paste p50/p95) was `parakeet-ultra-coreml` 407/892,
/// `fluid-speech-mini` 669/897, `parakeet-tdt-v3` 1056/1618. Quoting one number
/// across all of them reports a violation to someone who switched engines.
pub const SHORT_TAKE_ENGINES: [&str; 3] = [
    "parakeet-ultra-coreml",
    "fluid-speech-mini",
    "parakeet-tdt-v3",
];

/// U2: one stage's p50/p95 over a bounded recent window, with its sample
/// count. Crosses the IPC boundary, so it is camelCase. `sample_count` of 0
/// means the stage never ran in the window; the panel renders a hyphen,
/// never a zero (zero latency is a claim, "not measured yet" is the truth).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStageSummary {
    /// The stable stage id (see [`LatencyStage::id`]).
    pub stage: String,
    /// The display label (see [`LatencyStage::label`]).
    pub label: String,
    /// Pipeline order (see [`LatencyStage::order`]).
    pub order: u8,
    /// Whether the stage is shown in the panel.
    pub user_facing: bool,
    /// Nearest-rank p50 in ms, or `None` when the stage has no samples.
    pub p50_ms: Option<u32>,
    /// Nearest-rank p95 in ms, or `None` when the stage has no samples.
    pub p95_ms: Option<u32>,
    /// How many of the recent takes measured this stage.
    pub sample_count: u32,
}

/// U2: the per-stage latency summaries plus the window size, in one round
/// trip. Crosses the IPC boundary, so it is camelCase.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencySummary {
    /// One entry per stage in [`LatencyStage::ALL`], in pipeline order.
    pub stages: Vec<LatencyStageSummary>,
    /// How many recent takes the percentiles were computed over.
    pub window: u32,
    /// The release-to-paste p95 budget in ms (`Total` stage's promise).
    pub release_to_paste_p95_ms_max: u32,
    /// `Total` p95 over the SHORT takes in the window (audio under
    /// [`SHORT_TAKE_AUDIO_MS`]), which is the only condition
    /// [`RELEASE_TO_PASTE_P95_MS_MAX`] was ever measured under. `None` when no
    /// short take in the window produced a total.
    pub short_take_p95_ms: Option<u32>,
    /// How many takes produced [`Self::short_take_p95_ms`], under whatever
    /// engine scope the figure ended up using.
    pub short_take_count: u32,
    /// The length cutoff behind [`Self::short_take_p95_ms`], in ms. Carried
    /// across so the panel quotes the same number the aggregation used instead
    /// of hardcoding a second copy that can drift.
    pub short_take_audio_ms_max: u32,
    /// The engine [`Self::short_take_p95_ms`] was scoped to, or `None` when it
    /// spans every engine. See [`SHORT_TAKE_ENGINES`] for when scoping kicks
    /// in; the panel must name the engine it found, because "p95 892 ms" means
    /// different things on a 407 ms engine and a 1016 ms one.
    pub short_take_engine: Option<String>,
}

/// U2: nearest-rank percentile over a sorted slice of milliseconds. Returns
/// `None` on an empty slice. Nearest-rank (never interpolated): with a
/// handful of samples an interpolated p95 reports a number no take ever
/// took, which is the wrong thing to show beside a promise about real
/// latency. `fraction` is in [0, 1]. This is the single product-side
/// implementation; `teletype-eval` keeps its own (the dependency direction
/// does not allow sharing it).
pub fn percentile_nearest_rank(sorted_ms: &[u32], fraction: f64) -> Option<u32> {
    let n = sorted_ms.len();
    if n == 0 {
        return None;
    }
    let rank = (fraction * n as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(n - 1);
    Some(sorted_ms[index])
}

/// U2: compute p50 and p95 per stage over the most recent `window` takes.
/// Returns one [`LatencyStageSummary`] per stage in [`LatencyStage::ALL`],
/// in pipeline order. A stage absent from every take in the window gets
/// `sample_count: 0` and `None` percentiles. The window is in takes, not
/// days, so the number describes how the app behaves now rather than six
/// months ago.
pub fn latency_summary(traces: &[DictationTrace], window: u32) -> LatencySummary {
    let window = window.max(1);
    let window_usize = window as usize;
    // Group samples per stage id, preserving the most recent `window` takes.
    // `traces` is newest-first (see `read_recent_traces`), so taking the
    // first `window` is the bounded recent window.
    let mut per_stage: std::collections::BTreeMap<String, Vec<u32>> =
        std::collections::BTreeMap::new();
    for t in traces.iter().take(window_usize) {
        for (id, ms) in &t.stage_ms {
            per_stage.entry(id.clone()).or_default().push(*ms);
        }
    }
    // The short-take total: the only comparison the 700 ms promise supports.
    // A take with no recorded audio length is excluded, because that is an
    // aborted or file-transcribed row rather than a short take.
    let shorts: Vec<&DictationTrace> = traces
        .iter()
        .take(window_usize)
        .filter(|t| t.audio_ms > 0 && t.audio_ms < SHORT_TAKE_AUDIO_MS)
        .collect();
    // Then scoped to one engine, when enough short takes came off it to carry a
    // p95. Below that floor the figure stays engine-blind rather than inventing
    // a tail out of three takes. The *newest* short take picks the engine: a
    // fixed list order would scope the figure to an engine this machine
    // stopped using months ago.
    let engine_scope: Option<&str> = shorts
        .first()
        .map(|t| t.engine.as_str())
        .filter(|e| SHORT_TAKE_ENGINES.contains(e))
        .filter(|e| {
            shorts.iter().filter(|t| t.engine.as_str() == *e).count() as u32
                >= SHORT_TAKE_ENGINE_MIN_TAKES
        });
    let mut short_totals: Vec<u32> = shorts
        .iter()
        .filter(|t| engine_scope.is_none_or(|e| t.engine.as_str() == e))
        .filter_map(|t| t.stage_ms.get(LatencyStage::Total.id()).copied())
        .collect();
    short_totals.sort();
    let stages = LatencyStage::ALL
        .iter()
        .map(|stage| {
            let samples = per_stage
                .get(stage.id())
                .map(|v| {
                    let mut v = v.clone();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            let sample_count = samples.len() as u32;
            LatencyStageSummary {
                stage: stage.id().to_string(),
                label: stage.label().to_string(),
                order: stage.order(),
                user_facing: stage.user_facing(),
                p50_ms: percentile_nearest_rank(&samples, 0.50),
                p95_ms: percentile_nearest_rank(&samples, 0.95),
                sample_count,
            }
        })
        .collect();
    LatencySummary {
        stages,
        window,
        release_to_paste_p95_ms_max: RELEASE_TO_PASTE_P95_MS_MAX,
        short_take_p95_ms: percentile_nearest_rank(&short_totals, 0.95),
        short_take_count: short_totals.len() as u32,
        short_take_audio_ms_max: SHORT_TAKE_AUDIO_MS,
        short_take_engine: engine_scope.map(|e| e.to_string()),
    }
}

/// Current UTC date as (year, month, day), via the proleptic Gregorian
/// calendar (Howard Hinnant's civil-from-days). std-only: chrono would be
/// a new dependency for one filename.
fn today_ymd() -> (i32, u32, u32) {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86400) as i64)
        .unwrap_or(0);
    civil_from_days(days)
}

pub(crate) fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 99]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}

/// `traces-export-YYYY-MM-DD.jsonl` file name for the Developer export button.
pub fn export_file_name() -> String {
    let (y, m, d) = today_ymd();
    format!("traces-export-{y:04}-{m:02}-{d:02}.jsonl")
}

/// Reads the most recent traces across the rotated JSONL files, newest
/// first, up to `limit`. Corrupt lines are skipped; a missing directory
/// yields an empty vec. Backend for the Developer panel.
pub fn read_recent_traces(dir: &Path, limit: usize) -> Vec<DictationTrace> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // File names are dates, so lexical order is chronological.
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    files.reverse();
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Appends go last, so iterate bottom-up for newest-first.
        for line in text.lines().rev() {
            if out.len() >= limit {
                return out;
            }
            if let Ok(trace) = serde_json::from_str::<DictationTrace>(line) {
                out.push(trace);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory sink: records every trace it receives so tests can assert
    /// on write ordering without touching the filesystem.
    struct RecordingSink {
        lines: Mutex<Vec<String>>,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                lines: Mutex::new(Vec::new()),
            }
        }

        fn count(&self) -> usize {
            self.lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }

        fn last(&self) -> Option<serde_json::Value> {
            self.lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .and_then(|l| serde_json::from_str(l).ok())
        }
    }

    impl TraceSink for RecordingSink {
        fn write_trace(&self, trace: &DictationTrace) {
            if let Ok(line) = serde_json::to_string(trace) {
                self.lines
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(line);
            }
        }
    }

    #[test]
    fn trace_marks_are_monotonic() {
        let mut t = DictationTrace::new(1, "test-engine");
        t.mark(|s, now| s.t_hotkey_down = Some(now));
        t.mark(|s, now| s.t_first_audio = Some(now));
        t.mark(|s, now| s.t_hotkey_up = Some(now));
        t.mark(|s, now| s.t_asr_start = Some(now));
        t.mark(|s, now| s.t_asr_end = Some(now));
        t.mark(|s, now| s.t_text_pipeline_end = Some(now));
        t.mark(|s, now| s.t_polish_start = Some(now));
        t.mark(|s, now| s.t_polish_first_token = Some(now));
        t.mark(|s, now| s.t_polish_end = Some(now));
        t.mark(|s, now| s.t_paste_sent = Some(now));
        let marks = [
            t.t_hotkey_down,
            t.t_first_audio,
            t.t_hotkey_up,
            t.t_asr_start,
            t.t_asr_end,
            t.t_text_pipeline_end,
            t.t_polish_start,
            t.t_polish_first_token,
            t.t_polish_end,
            t.t_paste_sent,
        ];
        // Every stage is stamped, and no later stage predates an earlier
        // one (equality is fine: two marks can land in the same microsecond).
        let mut prev = 0u64;
        for m in marks {
            let v = m.expect("every stage must be stamped");
            assert!(v >= prev, "marks went backwards: {v} < {prev}");
            prev = v;
        }
    }

    #[test]
    fn trace_written_after_paste_not_before() {
        let sink = RecordingSink::new();
        let mut t = DictationTrace::new(7, "test-engine");
        t.audio_ms = 1200;
        t.mark(|s, now| s.t_hotkey_down = Some(now));
        t.mark(|s, now| s.t_first_audio = Some(now));
        t.mark(|s, now| s.t_hotkey_up = Some(now));
        t.mark(|s, now| s.t_asr_start = Some(now));
        t.mark(|s, now| s.t_asr_end = Some(now));
        t.apply_stage_marks(TraceStageMarks {
            t_text_pipeline_end: Some(now_us()),
            ..Default::default()
        });
        // The whole pipeline ran, but no paste yet: nothing may be written.
        assert_eq!(
            sink.count(),
            0,
            "trace must not be written before the paste is sent"
        );
        assert!(t.release_to_paste_us().is_none());
        // Paste sent: now the trace is complete and exactly one line lands.
        t.mark(|s, now| s.t_paste_sent = Some(now));
        sink.write_trace(&t);
        assert_eq!(sink.count(), 1);
        let v = sink.last().expect("one line must have been written");
        assert_eq!(v["session"], 7);
        assert!(v["t_paste_sent"].is_number(), "{v}");
        let latency = t.release_to_paste_us().expect("both marks set");
        assert!(
            v["t_paste_sent"].as_u64().unwrap() - v["t_hotkey_up"].as_u64().unwrap() == latency,
            "{v}"
        );
    }

    #[test]
    fn trace_write_failure_never_panics() {
        // Plan §0.1 rule 4: the trace writer is instrumentation, and its
        // fallback is silence. Point it at a path that cannot be a
        // directory (an existing file) and demand no panic, no delivery
        // impact — the write is simply dropped.
        let dir = std::env::temp_dir().join(format!("teletype-trace-test-{}", std::process::id()));
        let _ = std::fs::write(&dir, b"not a directory");
        let sink = FileTraceSink::new(&dir.join("unreachable-child"));
        let mut t = DictationTrace::new(1, "test-engine");
        t.mark(|s, now| s.t_paste_sent = Some(now));
        sink.write_trace(&t);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        // 1970-01-01 is day 0; spot-check the conversion the trace
        // filenames depend on.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19654), (2023, 10, 24));
    }

    #[test]
    fn command_mode_lifecycle_merges_in_order() {
        // P2.9: a mock command take. The controller-side stages (`armed`,
        // `started`) are pushed straight onto the trace at take start; the
        // worker-side stages (`classified=`, `executed`/`refused`) travel
        // in `TraceStageMarks` and are appended on merge. The full ordered
        // sequence is what Gate 2 debugging reads back.
        let mut t = DictationTrace::new(1, "test-engine");
        t.command_mode.push("armed".into());
        t.command_mode.push("started".into());
        t.apply_stage_marks(TraceStageMarks {
            command_events: vec!["classified=command".into(), "executed".into()],
            ..Default::default()
        });
        assert_eq!(
            t.command_mode,
            vec!["armed", "started", "classified=command", "executed"]
        );
    }

    #[test]
    fn command_mode_omitted_when_empty_and_refusal_round_trips() {
        // Plain dictation takes must not grow a `command_mode` key ...
        let t = DictationTrace::new(1, "test-engine");
        let v = serde_json::to_value(&t).expect("serialize");
        assert!(v.get("command_mode").is_none(), "{v}");
        // ... and old trace lines written before the field existed still
        // parse (the container-level `serde(default)` covers it).
        let old = r#"{"session":3,"engine":"e","errors":[]}"#;
        let parsed: DictationTrace = serde_json::from_str(old).expect("old line parses");
        assert!(parsed.command_mode.is_empty());

        // A refused command take keeps the full reason through a round trip.
        let mut t2 = DictationTrace::new(2, "test-engine");
        t2.command_mode.push("armed".into());
        t2.apply_stage_marks(TraceStageMarks {
            command_events: vec!["refused(open_app: app not found)".into()],
            ..Default::default()
        });
        let line = serde_json::to_string(&t2).expect("serialize");
        let back: DictationTrace = serde_json::from_str(&line).expect("round trip");
        assert_eq!(
            back.command_mode,
            vec!["armed", "refused(open_app: app not found)"]
        );
    }

    #[test]
    fn read_recent_traces_is_newest_first_and_skips_garbage() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dir = tmp.path().join("traces");
        std::fs::create_dir_all(&dir).unwrap();
        let write_day = |name: &str, sessions: &[u64], garbage: bool| {
            let mut text = String::new();
            for s in sessions {
                let t = DictationTrace::new(*s, "test-engine");
                text.push_str(&serde_json::to_string(&t).unwrap());
                text.push('\n');
            }
            if garbage {
                text.push_str("{not json}\n");
            }
            std::fs::write(dir.join(name), text).unwrap();
        };
        write_day("2026-10-12.jsonl", &[1, 2], false);
        write_day("2026-10-13.jsonl", &[3], true);
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();

        // Missing dir and zero limit are empty, not errors.
        assert!(read_recent_traces(&dir.join("nope"), 50).is_empty());
        assert!(read_recent_traces(&dir, 0).is_empty());

        let all = read_recent_traces(&dir, 50);
        let ids: Vec<u64> = all.iter().map(|t| t.session).collect();
        assert_eq!(ids, vec![3, 2, 1], "newest file first, newest line first");

        let two = read_recent_traces(&dir, 2);
        assert_eq!(
            two.iter().map(|t| t.session).collect::<Vec<_>>(),
            vec![3, 2]
        );

        assert!(export_file_name().starts_with("traces-export-"));
        assert!(export_file_name().ends_with(".jsonl"));
    }

    /// A.3.3: the four decider fields are additive and `#[serde(default)]`
    /// — a trace line written before the fields existed still parses, and
    /// the new fields round-trip when set.
    #[test]
    fn a3_3_decider_fields_serialize_and_default() {
        // Old trace JSON line (no decider fields) still parses.
        let old_json = r#"{"session":7,"engine":"parakeet","audio_ms":1200,"t_hotkey_down":1,"t_paste_sent":9}"#;
        let t: DictationTrace = serde_json::from_str(old_json).unwrap();
        assert_eq!(t.session, 7);
        assert!(t.decider.is_none());
        assert!(t.decider_ms.is_none());
        assert!(t.tool.is_none());
        assert!(t.args_source.is_none());

        // Set the fields: they serialize.
        let mut t = DictationTrace::new(8, "parakeet");
        t.decider = Some("neudecide".into());
        t.decider_ms = Some(42);
        t.tool = Some("web_search".into());
        t.args_source = Some("transcript".into());
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["decider"], "neudecide");
        assert_eq!(v["decider_ms"], 42);
        assert_eq!(v["tool"], "web_search");
        assert_eq!(v["args_source"], "transcript");

        // Unset fields are omitted from the JSON line.
        let t = DictationTrace::new(9, "parakeet");
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("decider").is_none());
        assert!(v.get("decider_ms").is_none());
        assert!(v.get("tool").is_none());
        assert!(v.get("args_source").is_none());
    }

    // ── U2: stage taxonomy, nearest-rank percentiles, aggregation ─────────

    #[test]
    fn percentile_nearest_rank_odd_count() {
        // 5 values 10..=50. p50: ceil(0.5*5)=3rd=30. p95: ceil(0.95*5)=5th=50.
        let s = [10u32, 20, 30, 40, 50];
        assert_eq!(percentile_nearest_rank(&s, 0.50), Some(30));
        assert_eq!(percentile_nearest_rank(&s, 0.95), Some(50));
        // Never interpolates into a value nothing measured.
        assert!(s.contains(&percentile_nearest_rank(&s, 0.80).unwrap()));
    }

    #[test]
    fn percentile_nearest_rank_even_count() {
        // 4 values 10..=40. p50: ceil(0.5*4)=2nd=20. p95: ceil(0.95*4)=4th=40.
        let s = [10u32, 20, 30, 40];
        assert_eq!(percentile_nearest_rank(&s, 0.50), Some(20));
        assert_eq!(percentile_nearest_rank(&s, 0.95), Some(40));
    }

    #[test]
    fn percentile_nearest_rank_single_sample() {
        let s = [42u32];
        assert_eq!(percentile_nearest_rank(&s, 0.50), Some(42));
        assert_eq!(percentile_nearest_rank(&s, 0.95), Some(42));
    }

    #[test]
    fn percentile_nearest_rank_empty_is_none() {
        assert_eq!(percentile_nearest_rank(&[], 0.50), None);
        assert_eq!(percentile_nearest_rank(&[], 0.95), None);
    }

    #[test]
    fn stage_ids_round_trip_and_are_distinct() {
        let mut ids: Vec<&str> = Vec::new();
        for stage in LatencyStage::ALL {
            ids.push(stage.id());
        }
        // Six stages, all distinct string ids.
        assert_eq!(ids.len(), 6);
        let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), 6, "stage ids must be distinct: {ids:?}");
        // The total stage is the budget stage.
        assert_eq!(LatencyStage::Total.id(), "total");
        assert!(!LatencyStage::PolishFirstToken.user_facing(), "first-token is diagnostic");
        assert!(LatencyStage::Total.user_facing());
        // Pipeline order is strictly increasing in ALL.
        let orders: Vec<u8> = LatencyStage::ALL.iter().map(|s| s.order()).collect();
        assert!(orders.windows(2).all(|w| w[0] < w[1]), "pipeline order must be increasing: {orders:?}");
    }

    /// A full take with every mark set derives every stage; a stage that did
    /// not run (polish skipped) is absent, never zero.
    #[test]
    fn derive_stage_ms_full_take_and_absent_polish() {
        // Full take: every boundary present and monotonic.
        let mut t = DictationTrace::new(1, "test");
        t.t_hotkey_down = Some(0);
        t.t_hotkey_up = Some(1_000_000); // 1000 ms capture
        t.t_text_pipeline_end = Some(3_000_000); // 2000 ms transcribe (up->text_pipeline_end)
        t.t_polish_start = Some(2_000_000);
        t.t_polish_first_token = Some(2_200_000); // 200 ms first token
        t.t_polish_end = Some(2_800_000); // 800 ms polish
        t.t_paste_sent = Some(3_500_000); // 500 ms inject, 2500 ms total
        t.derive_stage_ms();
        assert_eq!(t.stage_ms.get(LatencyStage::Capture.id()), Some(&1000));
        assert_eq!(t.stage_ms.get(LatencyStage::Asr.id()), Some(&2000));
        assert_eq!(t.stage_ms.get(LatencyStage::PolishFirstToken.id()), Some(&200));
        assert_eq!(t.stage_ms.get(LatencyStage::Polish.id()), Some(&800));
        assert_eq!(t.stage_ms.get(LatencyStage::Inject.id()), Some(&500));
        assert_eq!(t.stage_ms.get(LatencyStage::Total.id()), Some(&2500));

        // Polish skipped: no polish marks. The stage is ABSENT, not 0.
        let mut t2 = DictationTrace::new(2, "test");
        t2.t_hotkey_down = Some(0);
        t2.t_hotkey_up = Some(1_000_000);
        t2.t_text_pipeline_end = Some(2_000_000);
        t2.t_paste_sent = Some(2_500_000);
        t2.derive_stage_ms();
        assert!(!t2.stage_ms.contains_key(LatencyStage::Polish.id()), "skipped polish must be absent, not zero");
        assert!(!t2.stage_ms.contains_key(LatencyStage::PolishFirstToken.id()));
        assert_eq!(t2.stage_ms.get(LatencyStage::Total.id()), Some(&1500));
    }

    /// A non-monotonic pair (corrupt trace) must not produce a stage.
    #[test]
    fn derive_stage_ms_rejects_non_monotonic() {
        let mut t = DictationTrace::new(3, "test");
        t.t_hotkey_down = Some(1_000_000);
        t.t_hotkey_up = Some(500_000); // went backwards
        t.derive_stage_ms();
        assert!(!t.stage_ms.contains_key(LatencyStage::Capture.id()));
    }

    /// The aggregation: known samples produce exact p50/p95; an absent stage
    /// reports sample_count 0 and None percentiles (it must not render zero).
    #[test]
    fn latency_summary_exact_percentiles_and_absent_stage() {
        let mk = |down: u64, up: u64, tpe: u64, paste: u64| -> DictationTrace {
            let mut t = DictationTrace::new(0, "test");
            t.t_hotkey_down = Some(down);
            t.t_hotkey_up = Some(up);
            t.t_text_pipeline_end = Some(tpe);
            t.t_paste_sent = Some(paste);
            t.derive_stage_ms();
            t
        };
        // 4 takes. Injection (tpe->paste) = 100, 200, 300, 400 ms.
        let traces = vec![
            mk(0, 1_000_000, 1_100_000, 1_200_000),
            mk(0, 1_000_000, 1_100_000, 1_300_000),
            mk(0, 1_000_000, 1_100_000, 1_400_000),
            mk(0, 1_000_000, 1_100_000, 1_500_000),
        ];
        let summary = latency_summary(&traces, 200);
        assert_eq!(summary.window, 200);
        assert_eq!(summary.release_to_paste_p95_ms_max, 700);
        let inject = summary
            .stages
            .iter()
            .find(|s| s.stage == LatencyStage::Inject.id())
            .unwrap();
        assert_eq!(inject.sample_count, 4);
        // p50 of [100,200,300,400] = 2nd = 200; p95 = 4th = 400.
        assert_eq!(inject.p50_ms, Some(200));
        assert_eq!(inject.p95_ms, Some(400));
        // Total (up->paste) = 200,300,400,500 -> p50=300, p95=500.
        let total = summary
            .stages
            .iter()
            .find(|s| s.stage == LatencyStage::Total.id())
            .unwrap();
        assert_eq!(total.p50_ms, Some(300));
        assert_eq!(total.p95_ms, Some(500));
        // Polish never ran: absent, sample_count 0, None percentiles.
        let polish = summary
            .stages
            .iter()
            .find(|s| s.stage == LatencyStage::Polish.id())
            .unwrap();
        assert_eq!(polish.sample_count, 0);
        assert_eq!(polish.p50_ms, None, "absent stage must not render zero");
        assert_eq!(polish.p95_ms, None);
        // Stages are in pipeline order.
        let orders: Vec<u8> = summary.stages.iter().map(|s| s.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3, 4, 5]);
    }

    /// The window bounds the aggregation: only the most recent `window`
    /// takes count.
    #[test]
    fn latency_summary_respects_window() {
        let mk = |paste: u64| -> DictationTrace {
            let mut t = DictationTrace::new(0, "test");
            t.t_hotkey_up = Some(1_000_000);
            t.t_text_pipeline_end = Some(1_100_000);
            t.t_paste_sent = Some(paste);
            t.derive_stage_ms();
            t
        };
        // 4 takes, newest first. Inject = 100,200,300,400.
        let traces = vec![mk(1_200_000), mk(1_300_000), mk(1_400_000), mk(1_500_000)];
        let summary = latency_summary(&traces, 2);
        assert_eq!(summary.window, 2);
        let inject = summary
            .stages
            .iter()
            .find(|s| s.stage == LatencyStage::Inject.id())
            .unwrap();
        // Only the 2 most recent takes (100, 200) count.
        assert_eq!(inject.sample_count, 2);
        assert_eq!(inject.p50_ms, Some(100));
        assert_eq!(inject.p95_ms, Some(200));
    }

    /// `stage_ms` is additive and `#[serde(default)]`: an old trace line
    /// without the field still parses, and a new one round-trips.
    #[test]
    fn stage_ms_additive_and_round_trips() {
        let old = r#"{"session":3,"engine":"e","errors":[]}"#;
        let t: DictationTrace = serde_json::from_str(old).unwrap();
        assert!(t.stage_ms.is_empty(), "old line parses with no stage_ms");

        let mut t2 = DictationTrace::new(4, "test");
        t2.t_hotkey_up = Some(1_000_000);
        t2.t_paste_sent = Some(1_500_000);
        t2.derive_stage_ms();
        let line = serde_json::to_string(&t2).unwrap();
        let back: DictationTrace = serde_json::from_str(&line).unwrap();
        assert_eq!(back.stage_ms.get(LatencyStage::Total.id()), Some(&500));
    }

    /// The 700 ms promise only covers short takes, so the short-take p95 must
    /// exclude long takes and takes with no recorded audio length, while the
    /// all-lengths total keeps reporting the real number.
    #[test]
    fn short_take_p95_excludes_long_takes_and_unmeasured_audio() {
        let mk = |audio_ms: u32, total_ms: u64| -> DictationTrace {
            let mut t = DictationTrace::new(1, "test");
            t.audio_ms = audio_ms;
            t.t_hotkey_up = Some(0);
            t.t_text_pipeline_end = Some(total_ms * 1000 - 100_000);
            t.t_paste_sent = Some(total_ms * 1000);
            t.derive_stage_ms();
            t
        };
        // Three short takes, one 60 s take, one with no audio length recorded.
        let traces = vec![
            mk(1_000, 300),
            mk(2_000, 400),
            mk(3_000, 500),
            mk(60_000, 9_000),
            mk(0, 700),
        ];
        let s = latency_summary(&traces, 200);
        assert_eq!(s.short_take_count, 3);
        assert_eq!(s.short_take_p95_ms, Some(500));
        assert_eq!(
            s.short_take_engine, None,
            "an engine with no known split must not claim a scope"
        );
        // The all-lengths p95 is untouched and still carries the long take.
        let total = s
            .stages
            .iter()
            .find(|x| x.stage == LatencyStage::Total.id())
            .unwrap();
        assert_eq!(total.p95_ms, Some(9_000));
    }

    /// The same promise sits under a second condition the eval corpus had: one
    /// engine. Enough short takes off one engine scope the figure to it; not
    /// enough leaves it engine-blind rather than reading a tail off five takes.
    #[test]
    fn short_take_p95_scopes_to_the_newest_engine_when_enough_takes_back_it() {
        let mk = |engine: &str, total_ms: u64| -> DictationTrace {
            let mut t = DictationTrace::new(1, engine);
            t.audio_ms = 1_500;
            t.t_hotkey_up = Some(0);
            t.t_text_pipeline_end = Some(total_ms * 1000 - 100_000);
            t.t_paste_sent = Some(total_ms * 1000);
            t.derive_stage_ms();
            t
        };
        let mut traces: Vec<DictationTrace> = (0..20)
            .map(|_| mk("parakeet-ultra-coreml", 500))
            .collect();
        traces.extend((0..5).map(|_| mk("parakeet-tdt-v3", 5_000)));

        let s = latency_summary(&traces, 200);
        assert_eq!(
            s.short_take_engine.as_deref(),
            Some("parakeet-ultra-coreml")
        );
        assert_eq!(s.short_take_count, 20);
        assert_eq!(s.short_take_p95_ms, Some(500));

        // Same data, oldest-first, so the newest short take is the engine with
        // only five takes behind it: the floor refuses to scope, and the slow
        // takes dominate the figure again. That difference is the whole point.
        let mut flipped = traces;
        flipped.reverse();
        let f = latency_summary(&flipped, 200);
        assert_eq!(f.short_take_engine, None);
        assert_eq!(f.short_take_count, 25);
        assert_eq!(f.short_take_p95_ms, Some(5_000));
    }
}
