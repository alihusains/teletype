//! Latency tracking for the ASR and full (ASR + polish) paths.
//!
//! Two series are tracked independently — `asr` and `total` — because the two
//! regress for different reasons: ASR latency moves with model size and
//! hardware, total latency additionally moves with the polish model and
//! prompt length.

/// One timing sample.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Sample {
    pub name: String,
    /// Wall-clock milliseconds.
    pub millis: f64,
}

/// One finished dictation trace, trimmed to the fields the eval harness
/// needs. This mirrors the P0.2 `DictationTrace` shape (`t_hotkey_up` =
/// release, `t_paste_sent` = paste): trace files are deserialized into the
/// real [`teletype_core::trace::DictationTrace`] (which landed with exactly
/// the plan §P0.2 field names) and then projected here, so extra pipeline
/// fields are ignored by construction. If the upstream shape ever renames
/// these two timestamps, the projection in [`load_trace_intervals`] fails to
/// compile — that is the intended tripwire.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TraceInterval {
    /// Monotonic µs of hotkey release (`t_hotkey_up`).
    #[serde(default)]
    pub t_hotkey_up: Option<u64>,
    /// Monotonic µs the paste was sent (`t_paste_sent`).
    #[serde(default)]
    pub t_paste_sent: Option<u64>,
    /// Session/engine identifiers, kept for debugging only.
    #[serde(default)]
    pub session: u64,
    #[serde(default)]
    pub engine: String,
}

impl TraceInterval {
    /// `release→paste` latency in milliseconds, or `None` when either stamp
    /// is missing or the pair is non-monotonic (a corrupt trace must not
    /// poison the percentiles).
    pub fn release_to_paste_ms(&self) -> Option<f64> {
        match (self.t_hotkey_up, self.t_paste_sent) {
            (Some(up), Some(paste)) if paste >= up => Some((paste - up) as f64 / 1000.0),
            _ => None,
        }
    }
}

/// Load `DictationTrace` JSONL (one trace per line) into [`TraceInterval`]s.
///
/// Lines are deserialized into the real P0.2
/// [`teletype_core::trace::DictationTrace`] first, so a renamed timestamp
/// field breaks the build here instead of silently zeroing the latency gate.
pub fn load_trace_intervals(path: &std::path::Path) -> Result<Vec<TraceInterval>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read traces {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let t: teletype_core::trace::DictationTrace =
            serde_json::from_str(line).map_err(|e| format!("traces line {}: {e}", idx + 1))?;
        out.push(TraceInterval {
            t_hotkey_up: t.t_hotkey_up,
            t_paste_sent: t.t_paste_sent,
            session: t.session,
            engine: t.engine,
        });
    }
    Ok(out)
}

/// Aggregated percentiles over one latency series.
///
/// Percentiles use the **nearest-rank** method on a sorted copy:
/// `p_k = sorted[ceil(k/100 * n)]`, 1-indexed. No external stats crate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatencySummary {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    /// p99, added for the plan §P0.4 live-latency gate. Defaults to 0.0 when
    /// loading reports written before the field existed.
    #[serde(default)]
    pub p99: f64,
}

impl LatencySummary {
    /// Summarise a series of samples. Panics on an empty series — a report
    /// with zero latency samples is a bug, not a valid measurement.
    pub fn new(samples: &[Sample]) -> Self {
        Self::try_new(samples).expect("LatencySummary::new on empty series")
    }

    /// Summarise a series, returning `None` instead of panicking on empty.
    pub fn try_new(samples: &[Sample]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<f64> = samples.iter().map(|s| s.millis).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let sum: f64 = sorted.iter().sum();
        Some(Self {
            count: n,
            min: sorted[0],
            max: sorted[n - 1],
            mean: sum / n as f64,
            p50: nearest_rank(&sorted, 50),
            p95: nearest_rank(&sorted, 95),
            p99: nearest_rank(&sorted, 99),
        })
    }

    /// Convenience: build a summary from raw millisecond values.
    pub fn from_millis(values: &[f64]) -> Self {
        let samples: Vec<Sample> = values
            .iter()
            .enumerate()
            .map(|(i, ms)| Sample {
                name: format!("sample-{i}"),
                millis: *ms,
            })
            .collect();
        Self::new(&samples)
    }
}

/// Nearest-rank percentile on a **sorted** slice: `ceil(p/100 * n)`, 1-indexed.
fn nearest_rank(sorted: &[f64], p: u32) -> f64 {
    let n = sorted.len();
    let rank = ((p as f64 / 100.0) * n as f64).ceil() as usize;
    let idx = rank.clamp(1, n).saturating_sub(1);
    sorted[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_single_value() {
        let s = LatencySummary::from_millis(&[42.0]);
        assert_eq!(s.count, 1);
        assert_eq!(s.min, 42.0);
        assert_eq!(s.max, 42.0);
        assert_eq!(s.mean, 42.0);
        assert_eq!(s.p50, 42.0);
        assert_eq!(s.p95, 42.0);
    }

    #[test]
    fn nearest_rank_hand_computed() {
        // 20 values 1..=20.
        // p50: ceil(0.50 * 20) = 10th value = 10.
        // p95: ceil(0.95 * 20) = 19th value = 19.
        let vals: Vec<f64> = (1..=20).map(|i| i as f64).collect();
        let s = LatencySummary::from_millis(&vals);
        assert_eq!(s.count, 20);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 20.0);
        assert!((s.mean - 10.5).abs() < 1e-12);
        assert_eq!(
            s.p50, 10.0,
            "p50 of 1..20 by nearest-rank is the 10th value"
        );
        assert_eq!(
            s.p95, 19.0,
            "p95 of 1..20 by nearest-rank is the 19th value"
        );
    }

    #[test]
    fn nearest_rank_small_sample() {
        // 4 values. p95: ceil(0.95 * 4) = ceil(3.8) = 4th value.
        let s = LatencySummary::from_millis(&[10.0, 20.0, 30.0, 40.0]);
        assert_eq!(s.p50, 20.0, "p50 of 4 values: ceil(2)=2nd = 20");
        assert_eq!(s.p95, 40.0, "p95 of 4 values: ceil(3.8)=4th = 40");
    }

    #[test]
    fn nearest_rank_unsorted_input() {
        // Input is not pre-sorted; the summary must sort internally.
        let s = LatencySummary::from_millis(&[50.0, 10.0, 30.0, 20.0, 40.0]);
        assert_eq!(s.min, 10.0);
        assert_eq!(s.max, 50.0);
        // sorted: [10, 20, 30, 40, 50]; p50: ceil(2.5)=3rd=30; p95: ceil(4.75)=5th=50
        assert_eq!(s.p50, 30.0);
        assert_eq!(s.p95, 50.0);
    }

    #[test]
    #[should_panic(expected = "empty series")]
    fn empty_series_panics() {
        LatencySummary::from_millis(&[]);
    }

    #[test]
    fn sample_serializes() {
        let s = Sample {
            name: "asr-01".into(),
            millis: 123.45,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["name"], "asr-01");
        assert_eq!(v["millis"], 123.45);
    }

    #[test]
    fn p99_hand_computed() {
        // 100 values 1..=100: p99 = ceil(0.99*100) = 99th value = 99.
        let vals: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let s = LatencySummary::from_millis(&vals);
        assert_eq!(s.p99, 99.0);
    }

    #[test]
    fn try_new_empty_is_none() {
        assert!(LatencySummary::try_new(&[]).is_none());
    }

    #[test]
    fn trace_interval_release_to_paste() {
        let t = TraceInterval {
            t_hotkey_up: Some(1_000_000),
            t_paste_sent: Some(1_350_000),
            session: 7,
            engine: "parakeet".into(),
        };
        assert!((t.release_to_paste_ms().unwrap() - 350.0).abs() < 1e-9);
        // Missing stamps and time-travel both yield None, never poison data.
        let missing = TraceInterval::default();
        assert!(missing.release_to_paste_ms().is_none());
        let backwards = TraceInterval {
            t_hotkey_up: Some(2_000_000),
            t_paste_sent: Some(1_000_000),
            ..Default::default()
        };
        assert!(backwards.release_to_paste_ms().is_none());
    }

    #[test]
    fn trace_jsonl_ignores_extra_fields() {
        // Extra P0.2 DictationTrace fields must not break the load.
        let dir =
            std::env::temp_dir().join(format!("teletype-eval-trace-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("traces.jsonl");
        std::fs::write(
            &p,
            "{\"session\":1,\"engine\":\"x\",\"t_hotkey_up\":1000000,\"t_paste_sent\":1600000,\"t_asr_end\":1500000,\"errors\":[]}\n",
        )
        .unwrap();
        let traces = load_trace_intervals(&p).unwrap();
        assert_eq!(traces.len(), 1);
        assert!((traces[0].release_to_paste_ms().unwrap() - 600.0).abs() < 1e-9);
        std::fs::remove_dir_all(&dir).ok();
    }
}
