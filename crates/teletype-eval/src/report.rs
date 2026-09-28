//! Evaluation report: Markdown and JSON serialisation of one eval run.
//!
//! The report is the artefact CI diffs between runs. The JSON form has stable
//! key names; the Markdown form is for humans.

use crate::latency::LatencySummary;
use crate::wer::{TagWer, WerSummary};
use std::path::Path;

/// Errors writing or reading a report.
#[derive(Debug)]
pub enum ReportError {
    Io(std::io::Error),
    Malformed(String),
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "report IO error: {e}"),
            Self::Malformed(msg) => write!(f, "malformed report: {msg}"),
        }
    }
}

impl std::error::Error for ReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ReportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A per-case result row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaseResult {
    pub id: String,
    pub reference: String,
    pub hypothesis: String,
    pub polished: Option<String>,
    pub tags: Vec<String>,
    pub human: bool,
    pub wer: f64,
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
    pub polish_score: Option<u32>,
    pub polish_failed: Vec<String>,
    pub asr_ms: Option<f64>,
    pub total_ms: Option<f64>,
}

/// The full report for one eval run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalReport {
    pub git_sha: String,
    pub timestamp: String,
    pub model: String,
    pub mode: String,
    /// True if any case in the corpus had `human: false`.
    pub has_synthesised_audio: bool,
    pub human_case_count: usize,
    pub synthesised_case_count: usize,
    pub wer: WerSummary,
    pub wer_by_tag: Vec<(String, TagWer)>,
    pub polish: Vec<CasePolish>,
    pub polish_mean_score: f64,
    pub latency_asr: Option<LatencySummary>,
    pub latency_total: Option<LatencySummary>,
    pub model_load_ms: Option<f64>,
    pub cases: Vec<CaseResult>,
}

/// Polish verdict for one case in the report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CasePolish {
    pub id: String,
    pub score: u32,
    pub failed: Vec<String>,
}

impl EvalReport {
    /// Render as Markdown.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Teletype Eval Report\n\n");
        out.push_str(&format!("- **Git SHA:** {}\n", self.git_sha));
        out.push_str(&format!("- **Timestamp:** {}\n", self.timestamp));
        out.push_str(&format!("- **Model:** {}\n", self.model));
        out.push_str(&format!("- **Mode:** {}\n\n", self.mode));

        // Prominent synthesised-audio warning.
        if self.has_synthesised_audio {
            out.push_str(
                "> **WARNING:** This corpus contains synthesised audio\n\
                 > (`human: false`). WER numbers computed over synthesised\n\
                 > speech validate the harness plumbing and are **not** a\n\
                 > statement about Teletype's accuracy on human speech.\n\n",
            );
        }

        // Metrics table.
        out.push_str("## Metrics\n\n");
        out.push_str("| Metric | Value |\n|---|---|\n");
        out.push_str(&format!("| Cases | {} |\n", self.wer.cases));
        out.push_str(&format!("| Micro WER | {:.4} |\n", self.wer.micro_wer));
        out.push_str(&format!("| Macro WER | {:.4} |\n", self.wer.macro_wer));
        out.push_str(&format!(
            "| Mean polish score | {:.1} |\n",
            self.polish_mean_score
        ));
        if let Some(l) = &self.latency_asr {
            out.push_str(&format!(
                "| ASR p50 / p95 (ms) | {:.0} / {:.0} |\n",
                l.p50, l.p95
            ));
        }
        if let Some(l) = &self.latency_total {
            out.push_str(&format!(
                "| Total p50 / p95 (ms) | {:.0} / {:.0} |\n",
                l.p50, l.p95
            ));
        }
        if let Some(ms) = self.model_load_ms {
            out.push_str(&format!("| Model load (ms) | {:.0} |\n", ms));
        }
        out.push('\n');

        // WER by tag.
        if !self.wer_by_tag.is_empty() {
            out.push_str("## WER by Tag\n\n");
            out.push_str("| Tag | Cases | Ref words | Errors | WER |\n");
            out.push_str("|---|---|---|---|---|\n");
            for (tag, tw) in &self.wer_by_tag {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {:.4} |\n",
                    tag, tw.cases, tw.reference_words, tw.errors, tw.wer
                ));
            }
            out.push('\n');
        }

        // Worst 5 cases.
        if !self.wer.worst.is_empty() {
            out.push_str("## Worst 5 Cases\n\n");
            out.push_str("| ID | WER | S | D | I | Reference | Hypothesis |\n");
            out.push_str("|---|---|---|---|---|---|---|\n");
            for w in &self.wer.worst {
                out.push_str(&format!(
                    "| {} | {:.4} | {} | {} | {} | {} | {} |\n",
                    w.id,
                    w.wer,
                    w.substitutions,
                    w.deletions,
                    w.insertions,
                    truncate(&w.reference, 60),
                    truncate(&w.hypothesis, 60)
                ));
            }
            out.push('\n');
        }

        // Polish verdicts.
        let failed_counts: std::collections::HashMap<&str, usize> = self
            .polish
            .iter()
            .flat_map(|c| c.failed.iter())
            .fold(std::collections::HashMap::new(), |mut m, f| {
                *m.entry(f.as_str()).or_insert(0) += 1;
                m
            });
        if !failed_counts.is_empty() {
            out.push_str("## Polish Failed Checks\n\n");
            out.push_str("| Check | Failures |\n|---|---|\n");
            let mut entries: Vec<(&&str, &usize)> = failed_counts.iter().collect();
            entries.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            for (check, count) in entries {
                out.push_str(&format!("| {} | {} |\n", check, count));
            }
            out.push('\n');
        }

        // Latency percentiles.
        if self.latency_asr.is_some() || self.latency_total.is_some() {
            out.push_str("## Latency\n\n");
            out.push_str("| Series | Count | Min | Max | Mean | p50 | p95 |\n");
            out.push_str("|---|---|---|---|---|---|---|\n");
            for (name, l) in [("asr", &self.latency_asr), ("total", &self.latency_total)] {
                if let Some(l) = l {
                    out.push_str(&format!(
                        "| {} | {} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |\n",
                        name, l.count, l.min, l.max, l.mean, l.p50, l.p95
                    ));
                }
            }
            out.push('\n');
        }

        out
    }

    /// Render as pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("EvalReport serialisation cannot fail")
    }

    /// Write Markdown to `path`.
    pub fn write_markdown(&self, path: &Path) -> Result<(), ReportError> {
        std::fs::write(path, self.to_markdown())?;
        Ok(())
    }

    /// Write JSON to `path`.
    pub fn write_json(&self, path: &Path) -> Result<(), ReportError> {
        std::fs::write(path, self.to_json())?;
        Ok(())
    }

    /// Load a prior report from JSON (for CI diffing).
    pub fn load_json(path: &Path) -> Result<Self, ReportError> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(|e| ReportError::Malformed(e.to_string()))
    }
}

/// Truncate a string to `max` chars, appending `…` if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wer::{Wer, WerSummary};

    fn empty_report() -> EvalReport {
        EvalReport {
            git_sha: "abc1234".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            model: "test-model".into(),
            mode: "replay".into(),
            has_synthesised_audio: true,
            human_case_count: 0,
            synthesised_case_count: 1,
            wer: WerSummary {
                cases: 1,
                total_reference_words: 5,
                total_errors: 1,
                micro_wer: 0.2,
                macro_wer: 0.2,
                worst: vec![],
            },
            wer_by_tag: vec![],
            polish: vec![],
            polish_mean_score: 90.0,
            latency_asr: None,
            latency_total: None,
            model_load_ms: None,
            cases: vec![],
        }
    }

    #[test]
    fn markdown_contains_sha_and_warning() {
        let r = empty_report();
        let md = r.to_markdown();
        assert!(md.contains("abc1234"));
        assert!(md.contains("synthesised"));
        assert!(md.contains("not** a"));
    }

    #[test]
    fn json_roundtrip() {
        let r = empty_report();
        let json = r.to_json();
        let loaded: EvalReport = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.git_sha, "abc1234");
        assert_eq!(loaded.wer.micro_wer, 0.2);
    }

    #[test]
    fn markdown_worst_cases_table() {
        let mut r = empty_report();
        r.wer.worst = vec![crate::wer::WorstCase {
            id: "test-01".into(),
            reference: "the quick brown fox".into(),
            hypothesis: "the quick red fox".into(),
            wer: 0.25,
            substitutions: 1,
            deletions: 0,
            insertions: 0,
        }];
        let md = r.to_markdown();
        assert!(md.contains("Worst 5 Cases"));
        assert!(md.contains("test-01"));
    }

    #[test]
    fn markdown_polish_failed_checks() {
        let mut r = empty_report();
        r.polish = vec![
            CasePolish {
                id: "a".into(),
                score: 80,
                failed: vec!["no_fillers".into(), "sentence_cased".into()],
            },
            CasePolish {
                id: "b".into(),
                score: 90,
                failed: vec!["no_fillers".into()],
            },
        ];
        let md = r.to_markdown();
        assert!(md.contains("Polish Failed Checks"));
        assert!(md.contains("no_fillers"));
        // no_fillers should have count 2, sentence_cased count 1.
        let filler_line = md.lines().find(|l| l.starts_with("| no_fillers")).unwrap();
        assert!(filler_line.contains("| 2 |"));
    }

    #[test]
    fn load_json_malformed() {
        let dir = std::env::temp_dir().join(format!("eval-report-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.json");
        std::fs::write(&p, "not json").unwrap();
        let err = EvalReport::load_json(&p).unwrap_err();
        assert!(matches!(err, ReportError::Malformed(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unused_wer_suppresses() {
        let _ = Wer {
            substitutions: 0,
            deletions: 0,
            insertions: 0,
            reference_words: 0,
            wer: 0.0,
            edits: vec![],
        };
    }
}
