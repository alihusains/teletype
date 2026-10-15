//! `teletype-eval compare`: markdown diff table between two run directories.
//!
//! Each argument is either a run directory (containing `report.json`) or a
//! `report.json` path directly.

use std::path::Path;

use crate::latency::LatencySummary;
use crate::report::EvalReport;

fn load_report(arg: &str) -> Result<(String, EvalReport), String> {
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
    Ok((arg.to_string(), report))
}

fn fmt_opt(v: Option<f64>, digits: usize) -> String {
    match v {
        Some(x) => format!("{x:.digits$}"),
        None => "n/a".into(),
    }
}

fn fmt_lat(l: &Option<LatencySummary>, field: fn(&LatencySummary) -> f64) -> String {
    match l {
        Some(s) => format!("{:.0}", field(s)),
        None => "n/a".into(),
    }
}

fn p50(l: &LatencySummary) -> f64 {
    l.p50
}
fn p95(l: &LatencySummary) -> f64 {
    l.p95
}
fn p99(l: &LatencySummary) -> f64 {
    l.p99
}

fn delta(base: Option<f64>, head: Option<f64>, digits: usize, lower_is_better: bool) -> String {
    match (base, head) {
        (Some(b), Some(h)) => {
            let d = h - b;
            let arrow = if d.abs() < 0.5 * 10f64.powi(-(digits as i32)) {
                "="
            } else if (d < 0.0) == lower_is_better {
                "▼" // improved
            } else {
                "▲" // regressed
            };
            format!("{d:+.digits$} {arrow}")
        }
        _ => "n/a".into(),
    }
}

/// Compare two runs and render a markdown diff table.
///
/// `lower_is_better` marks regressions with ▲ and improvements with ▼.
/// Rates where higher is better (recall, accuracy, polish) pass `false`.
pub fn compare(base_arg: &str, head_arg: &str) -> Result<String, String> {
    let (base_name, base) = load_report(base_arg)?;
    let (head_name, head) = load_report(head_arg)?;

    let mut out = String::new();
    out.push_str(&format!(
        "# Eval comparison: `{base_name}` → `{head_name}`\n\n\
         base: {} ({}, {}) · head: {} ({}, {})\n\n",
        base.git_sha, base.model, base.mode, head.git_sha, head.model, head.mode
    ));
    out.push_str("| Metric | Base | Head | Δ (head − base) |\n");
    out.push_str("|---|---|---|---|\n");

    let mut row = |name: &str, b: String, h: String, d: String| {
        out.push_str(&format!("| {name} | {b} | {h} | {d} |\n"));
    };

    row(
        "Cases",
        base.wer.cases.to_string(),
        head.wer.cases.to_string(),
        if base.wer.cases == head.wer.cases {
            "=".into()
        } else {
            format!(
                "{:+} ▲/▼ n/a",
                head.wer.cases as i64 - base.wer.cases as i64
            )
        },
    );
    row(
        "Micro WER (legacy)",
        format!("{:.4}", base.wer.micro_wer),
        format!("{:.4}", head.wer.micro_wer),
        delta(Some(base.wer.micro_wer), Some(head.wer.micro_wer), 4, true),
    );
    row(
        "Macro WER (legacy)",
        format!("{:.4}", base.wer.macro_wer),
        format!("{:.4}", head.wer.macro_wer),
        delta(Some(base.wer.macro_wer), Some(head.wer.macro_wer), 4, true),
    );
    row(
        "Verbatim WER (micro)",
        fmt_opt(base.wer_verbatim_micro, 4),
        fmt_opt(head.wer_verbatim_micro, 4),
        delta(base.wer_verbatim_micro, head.wer_verbatim_micro, 4, true),
    );
    row(
        "Verbatim WER (macro)",
        fmt_opt(base.wer_verbatim_macro, 4),
        fmt_opt(head.wer_verbatim_macro, 4),
        delta(base.wer_verbatim_macro, head.wer_verbatim_macro, 4, true),
    );
    row(
        "Clean WER (micro)",
        fmt_opt(base.wer_clean_micro, 4),
        fmt_opt(head.wer_clean_micro, 4),
        delta(base.wer_clean_micro, head.wer_clean_micro, 4, true),
    );
    row(
        "Clean WER human (micro)",
        fmt_opt(base.wer_clean_human_micro, 4),
        fmt_opt(head.wer_clean_human_micro, 4),
        delta(
            base.wer_clean_human_micro,
            head.wer_clean_human_micro,
            4,
            true,
        ),
    );
    row(
        "Vocab recall",
        fmt_opt(base.vocab_recall, 4),
        fmt_opt(head.vocab_recall, 4),
        delta(base.vocab_recall, head.vocab_recall, 4, false),
    );
    row(
        "Vocab false-alarm",
        fmt_opt(base.vocab_false_alarm, 4),
        fmt_opt(head.vocab_false_alarm, 4),
        delta(base.vocab_false_alarm, head.vocab_false_alarm, 4, true),
    );
    row(
        "Entity accuracy",
        fmt_opt(base.entity_accuracy, 4),
        fmt_opt(head.entity_accuracy, 4),
        delta(base.entity_accuracy, head.entity_accuracy, 4, false),
    );
    row(
        "Semantic ok (report-only)",
        fmt_opt(base.semantic_ok_rate, 4),
        fmt_opt(head.semantic_ok_rate, 4),
        delta(base.semantic_ok_rate, head.semantic_ok_rate, 4, false),
    );
    row(
        "Mean polish score",
        format!("{:.1}", base.polish_mean_score),
        format!("{:.1}", head.polish_mean_score),
        delta(
            Some(base.polish_mean_score),
            Some(head.polish_mean_score),
            1,
            false,
        ),
    );
    for (label, field) in [
        ("ASR p50", p50 as fn(&LatencySummary) -> f64),
        ("ASR p95", p95 as fn(&LatencySummary) -> f64),
        ("ASR p99", p99 as fn(&LatencySummary) -> f64),
    ] {
        let b = base.latency_asr.as_ref().map(field);
        let h = head.latency_asr.as_ref().map(field);
        row(
            &format!("{label} (ms)"),
            fmt_lat(&base.latency_asr, field),
            fmt_lat(&head.latency_asr, field),
            delta(b, h, 0, true),
        );
    }
    for (label, field) in [
        ("Total p50", p50 as fn(&LatencySummary) -> f64),
        ("Total p95", p95 as fn(&LatencySummary) -> f64),
        ("Total p99", p99 as fn(&LatencySummary) -> f64),
    ] {
        let b = base.latency_total.as_ref().map(field);
        let h = head.latency_total.as_ref().map(field);
        row(
            &format!("{label} (ms)"),
            fmt_lat(&base.latency_total, field),
            fmt_lat(&head.latency_total, field),
            delta(b, h, 0, true),
        );
    }
    for (label, field) in [
        ("Release→paste p50", p50 as fn(&LatencySummary) -> f64),
        ("Release→paste p95", p95 as fn(&LatencySummary) -> f64),
        ("Release→paste p99", p99 as fn(&LatencySummary) -> f64),
    ] {
        let b = base.latency_release_to_paste.as_ref().map(field);
        let h = head.latency_release_to_paste.as_ref().map(field);
        row(
            &format!("{label} (ms)"),
            fmt_lat(&base.latency_release_to_paste, field),
            fmt_lat(&head.latency_release_to_paste, field),
            delta(b, h, 0, true),
        );
    }

    out.push('\n');
    out.push_str("▼ = head improved · ▲ = head regressed · = = unchanged\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::EvalReport;
    use crate::wer::WerSummary;

    fn report(sha: &str, verbatim: Option<f64>) -> EvalReport {
        EvalReport {
            git_sha: sha.into(),
            timestamp: "0".into(),
            model: "m".into(),
            mode: "replay".into(),
            has_synthesised_audio: false,
            human_case_count: 0,
            synthesised_case_count: 0,
            wer: WerSummary {
                cases: 2,
                total_reference_words: 10,
                total_errors: 1,
                micro_wer: 0.1,
                macro_wer: 0.1,
                worst: vec![],
            },
            wer_by_tag: vec![],
            polish: vec![],
            polish_mean_score: 90.0,
            latency_asr: None,
            latency_total: None,
            model_load_ms: None,
            cases: vec![],
            wer_verbatim_micro: verbatim,
            wer_verbatim_macro: verbatim,
            wer_clean_micro: None,
            wer_clean_human_micro: None,
            vocab_recall: None,
            vocab_false_alarm: None,
            entity_accuracy: None,
            semantic_ok_rate: None,
            latency_release_to_paste: None,
            case_errors: vec![],
        }
    }

    #[test]
    fn compare_marks_regression_and_improvement() {
        let dir = std::env::temp_dir().join(format!("eval-compare-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("base")).unwrap();
        std::fs::create_dir_all(dir.join("head")).unwrap();
        std::fs::write(
            dir.join("base").join("report.json"),
            serde_json::to_string(&report("aaa", Some(0.10))).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("head").join("report.json"),
            serde_json::to_string(&report("bbb", Some(0.08))).unwrap(),
        )
        .unwrap();
        let md = compare(
            dir.join("base").to_str().unwrap(),
            dir.join("head").to_str().unwrap(),
        )
        .unwrap();
        // Verbatim WER improved 0.10 → 0.08: delta row carries ▼.
        assert!(md.contains("Verbatim WER (micro)"));
        assert!(md.contains("▼"), "{md}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compare_missing_metrics_are_na() {
        let dir = std::env::temp_dir().join(format!("eval-compare-na-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("base")).unwrap();
        std::fs::create_dir_all(dir.join("head")).unwrap();
        std::fs::write(
            dir.join("base").join("report.json"),
            serde_json::to_string(&report("aaa", None)).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("head").join("report.json"),
            serde_json::to_string(&report("bbb", None)).unwrap(),
        )
        .unwrap();
        let md = compare(
            dir.join("base").to_str().unwrap(),
            dir.join("head").to_str().unwrap(),
        )
        .unwrap();
        assert!(md.contains("n/a"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
