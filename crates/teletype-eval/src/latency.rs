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
}

impl LatencySummary {
    /// Summarise a series of samples. Panics on an empty series — a report
    /// with zero latency samples is a bug, not a valid measurement.
    pub fn new(samples: &[Sample]) -> Self {
        assert!(!samples.is_empty(), "LatencySummary::new on empty series");
        let mut sorted: Vec<f64> = samples.iter().map(|s| s.millis).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let sum: f64 = sorted.iter().sum();
        Self {
            count: n,
            min: sorted[0],
            max: sorted[n - 1],
            mean: sum / n as f64,
            p50: nearest_rank(&sorted, 50),
            p95: nearest_rank(&sorted, 95),
        }
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
}
