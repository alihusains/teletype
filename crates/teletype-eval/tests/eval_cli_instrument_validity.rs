// claim: eval-integrity
//! End-to-end instrument-validity checks for the `teletype-eval` CLI.
//!
//! These spawn the real binary (replay mode, no model needed) and pin the
//! exit-code contract the CI/ratchet tooling relies on:
//!
//!   0  a threshold gate with at least one evaluated check passed
//!   1  a replay hypotheses file does not cover the corpus (INSTRUMENT_INVALID)
//!   3  a threshold gate with zero evaluated checks must not print PASS
//!
//! Replay mode scores the hypotheses file against the corpus, so the fixture
//! needs no ASR engine - only a manifest and tiny WAV files the loader stats.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    corpus: PathBuf,
    hypotheses: PathBuf,
    out: PathBuf,
    thresholds: PathBuf,
    /// Held so the temp dir outlives the fixture's paths.
    _dir: tempfile::TempDir,
}

/// 16 kHz mono 16-bit PCM wav with `frames` silent frames.
fn write_silent_wav(dir: &Path, name: &str, frames: usize) -> PathBuf {
    let data_len = (frames * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + frames * 2);
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
    bytes.resize(44 + frames * 2, 0);
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Two-case corpus plus a hypotheses file containing exactly `hyp_rows`.
fn fixture(hyp_rows: &[(&str, &str)]) -> Fixture {
    let dir = tempfile::TempDir::new().unwrap();
    let corpus = dir.path().join("corpus");
    std::fs::create_dir_all(corpus.join("audio")).unwrap();

    let cases = [
        ("case-a", "the quick brown fox"),
        ("case-b", "pack my box with five dozen liquor jugs"),
    ];
    let mut manifest = String::new();
    for (id, reference) in cases {
        write_silent_wav(&corpus.join("audio"), &format!("{id}.wav"), 160);
        manifest.push_str(&format!(
            "{{\"id\":\"{id}\",\"audio\":\"audio/{id}.wav\",\
             \"reference\":\"{reference}\",\"tags\":[\"plain\"],\"human\":false}}\n"
        ));
    }
    std::fs::write(corpus.join("manifest.jsonl"), manifest).unwrap();

    let hypotheses = dir.path().join("hypotheses.jsonl");
    let mut hyp = String::new();
    for (id, text) in hyp_rows {
        hyp.push_str(&format!("{{\"id\":\"{id}\",\"hypothesis\":\"{text}\"}}\n"));
    }
    std::fs::write(&hypotheses, hyp).unwrap();

    let thresholds = dir.path().join("thresholds.json");
    std::fs::write(
        &thresholds,
        "{\n  \"wer_verbatim_max_regression_pct\": 5.0,\n  \"vocab_recall_min\": 0.9,\n  \
         \"latency_release_to_paste_p95_ms_max\": 800,\n  \"wer_clean_human_max\": 0.2\n}\n",
    )
    .unwrap();

    Fixture {
        corpus,
        hypotheses,
        out: dir.path().join("run"),
        thresholds,
        _dir: dir,
    }
}

fn run_eval(f: &Fixture, baseline: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_teletype-eval"));
    cmd.args(["run", "--corpus"])
        .arg(&f.corpus)
        .arg("--hypotheses")
        .arg(&f.hypotheses)
        .arg("--engine")
        .arg("none")
        .arg("--out")
        .arg(&f.out)
        .arg("--thresholds")
        .arg(&f.thresholds);
    if let Some(b) = baseline {
        cmd.arg("--baseline").arg(b);
    }
    cmd.output().expect("spawn teletype-eval")
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn all_skipped_gate_is_instrument_invalid() {
    let f = fixture(&[
        ("case-a", "the quick brown cat"),
        ("case-b", "pack my box with five dozen liquor mugs"),
    ]);
    let out = run_eval(&f, None);
    assert_eq!(
        out.status.code(),
        Some(3),
        "zero evaluated checks must exit 3, not PASS: {}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("INSTRUMENT_INVALID"),
        "the refusal must name the instrument verdict: {stderr}"
    );
    assert!(
        stderr.contains("0 checks had data"),
        "the refusal must say why: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("threshold gate: PASS"),
        "a zero-check gate must never print PASS: {stdout}"
    );
    // The evidence is still produced before the gate refuses, so a caller can
    // inspect what was (not) measured.
    assert!(
        f.out.join("report.json").is_file(),
        "report.json must exist"
    );
}

#[test]
fn missing_replay_row_is_a_clean_error_not_a_panic() {
    let f = fixture(&[("case-a", "the quick brown cat")]);
    let out = run_eval(&f, None);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a partial replay is a clean error: {}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("INSTRUMENT_INVALID"),
        "missing rows must surface as an instrument verdict: {stderr}"
    );
    assert!(
        stderr.contains("case-b"),
        "the error must name the uncovered case: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "a missing hypothesis row must never panic: {stderr}"
    );
}

#[test]
fn baseline_makes_the_gate_evaluate_and_pass() {
    let f = fixture(&[
        ("case-a", "the quick brown cat"),
        ("case-b", "pack my box with five dozen liquor mugs"),
    ]);
    // First run refuses to pass (zero checks) but writes report.json.
    let first = run_eval(&f, None);
    assert_eq!(first.status.code(), Some(3), "{}", stderr_of(&first));

    let baseline = f._dir.path().join("baseline.json");
    std::fs::copy(f.out.join("report.json"), &baseline).unwrap();

    // Second run: the regression check has data and the gate genuinely passes.
    let second = run_eval(&f, Some(&baseline));
    assert_eq!(
        second.status.code(),
        Some(0),
        "a gate with one evaluated check must pass: {}",
        stderr_of(&second)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("threshold gate: PASS"),
        "the pass must be printed: {stdout}"
    );
}
