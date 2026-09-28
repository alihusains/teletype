//! Evaluation corpus: a JSONL manifest of utterances with reference
//! transcripts.
//!
//! Manifest format (`manifest.jsonl` inside the corpus directory), one JSON
//! object per line:
//!
//! ```json
//! {"id":"plain-01","audio":"audio/plain-01.wav","reference":"the deploy went out this morning and it is all good","tags":["plain"],"human":false}
//! ```
//!
//! - `id`: unique, non-empty, `[a-z0-9-]` only.
//! - `audio`: path relative to the manifest's directory.
//! - `reference`: the exact words a human would say, as spoken (lowercase,
//!   no trailing period). This is the WER ground truth and the polish input.
//! - `tags`: category strings used to group the report.
//! - `human`: `true` for real human recordings, `false` for synthesised
//!   speech. A WER over synthesised speech is a plumbing check, not a
//!   product claim, so the flag must be surfaced in the report.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One utterance in the corpus.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Case {
    pub id: String,
    /// Path to the audio file, relative to the corpus directory.
    #[serde(skip)]
    pub audio: PathBuf,
    /// Absolute path to the audio file.
    #[serde(skip)]
    pub audio_path: PathBuf,
    /// The reference transcript (WER ground truth, polish input).
    pub reference: String,
    pub tags: Vec<String>,
    /// True when the audio is a real human recording.
    pub human: bool,
}

#[derive(Deserialize)]
struct RawCase {
    id: String,
    audio: String,
    reference: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    human: bool,
}

/// Errors loading a corpus manifest.
#[derive(Debug)]
pub enum CorpusError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    BadJson {
        line: usize,
        message: String,
    },
    BadField {
        line: usize,
        message: String,
    },
    AudioMissing {
        line: usize,
        path: PathBuf,
    },
}

impl std::fmt::Display for CorpusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read manifest {path:?}: {source}"),
            Self::BadJson { line, message } => {
                write!(f, "manifest line {line}: invalid JSON: {message}")
            }
            Self::BadField { line, message } => write!(f, "manifest line {line}: {message}"),
            Self::AudioMissing { line, path } => {
                write!(
                    f,
                    "manifest line {line}: audio file missing or unreadable: {path:?}"
                )
            }
        }
    }
}

impl std::error::Error for CorpusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A loaded corpus: cases sorted by id.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    /// The directory containing `manifest.jsonl`.
    pub dir: PathBuf,
    pub cases: Vec<Case>,
}

impl Corpus {
    /// Load `manifest.jsonl` from `dir`.
    ///
    /// Rejects: duplicate ids, missing/unreadable audio files, malformed
    /// JSON (with the line number), and ids outside `[a-z0-9-]`. Returns
    /// cases sorted by id so the report is deterministic.
    pub fn load(dir: &Path) -> Result<Self, CorpusError> {
        let manifest = dir.join("manifest.jsonl");
        let content = std::fs::read_to_string(&manifest).map_err(|e| CorpusError::Io {
            path: manifest.clone(),
            source: e,
        })?;

        let mut seen: HashSet<String> = HashSet::new();
        let mut cases: Vec<Case> = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line_no = idx + 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let raw: RawCase = serde_json::from_str(line).map_err(|e| CorpusError::BadJson {
                line: line_no,
                message: e.to_string(),
            })?;

            validate_id(&raw.id).map_err(|msg| CorpusError::BadField {
                line: line_no,
                message: msg,
            })?;
            if !seen.insert(raw.id.clone()) {
                return Err(CorpusError::BadField {
                    line: line_no,
                    message: format!("duplicate id {:?}", raw.id),
                });
            }
            if raw.reference.trim().is_empty() {
                return Err(CorpusError::BadField {
                    line: line_no,
                    message: "reference must be non-empty".into(),
                });
            }

            let audio_rel = Path::new(&raw.audio);
            let audio_abs = dir.join(audio_rel);
            if !audio_abs.is_file() {
                return Err(CorpusError::AudioMissing {
                    line: line_no,
                    path: audio_abs,
                });
            }

            cases.push(Case {
                id: raw.id,
                audio: audio_rel.to_path_buf(),
                audio_path: audio_abs,
                reference: raw.reference,
                tags: raw.tags,
                human: raw.human,
            });
        }

        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self {
            dir: dir.to_path_buf(),
            cases,
        })
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("id must be non-empty".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!("id {:?} must match [a-z0-9-] only", id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_corpus(manifest: &str, audio_files: &[&str]) -> std::path::PathBuf {
        // The pid plus a wall-clock reading is not unique: these tests run on
        // parallel threads, and `SystemTime` can hand two of them the same
        // value. Two tests then shared one directory and raced on
        // `manifest.jsonl`, which made `rejects_missing_audio` fail
        // intermittently and for no reason related to what it tests. A process
        // -wide counter cannot collide.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "teletype-eval-test-{}-{seq}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for f in audio_files {
            let p = dir.join(f);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, b"fake").unwrap();
        }
        let mut m = std::fs::File::create(dir.join("manifest.jsonl")).unwrap();
        m.write_all(manifest.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn loads_valid_manifest_sorted_by_id() {
        let dir = temp_corpus(
            r#"{"id":"b-case","audio":"a/b.wav","reference":"hello world","tags":["plain"],"human":false}
{"id":"a-case","audio":"a/a.wav","reference":"good morning","tags":["plain"],"human":true}
"#,
            &["a/a.wav", "a/b.wav"],
        );
        let corpus = Corpus::load(&dir).unwrap();
        assert_eq!(corpus.cases.len(), 2);
        assert_eq!(corpus.cases[0].id, "a-case");
        assert_eq!(corpus.cases[1].id, "b-case");
        assert!(corpus.cases[0].human);
        assert!(!corpus.cases[1].human);
        assert!(corpus.cases[0].audio_path.is_file());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let dir = temp_corpus(
            r#"{"id":"x","audio":"x.wav","reference":"hi","tags":[],"human":false}
{"id":"x","audio":"x.wav","reference":"hi","tags":[],"human":false}
"#,
            &["x.wav"],
        );
        let err = Corpus::load(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::BadField { line: 2, .. }));
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_missing_audio() {
        let dir = temp_corpus(
            r#"{"id":"x","audio":"missing.wav","reference":"hi","tags":[],"human":false}"#,
            &[],
        );
        let err = Corpus::load(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::AudioMissing { line: 1, .. }));
    }

    #[test]
    fn rejects_malformed_json_with_line_number() {
        let dir = temp_corpus(
            r#"{"id":"x","audio":"x.wav","reference":"hi","tags":[],"human":false}
not json at all
"#,
            &["x.wav"],
        );
        let err = Corpus::load(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::BadJson { line: 2, .. }));
        assert!(err.to_string().contains("line 2"));
    }

    #[test]
    fn rejects_bad_id_characters() {
        let dir = temp_corpus(
            r#"{"id":"Bad_ID","audio":"x.wav","reference":"hi","tags":[],"human":false}"#,
            &["x.wav"],
        );
        let err = Corpus::load(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::BadField { line: 1, .. }));
        assert!(err.to_string().contains("[a-z0-9-]"));
    }

    #[test]
    fn rejects_empty_reference() {
        let dir = temp_corpus(
            r#"{"id":"x","audio":"x.wav","reference":"   ","tags":[],"human":false}"#,
            &["x.wav"],
        );
        let err = Corpus::load(&dir).unwrap_err();
        assert!(matches!(err, CorpusError::BadField { line: 1, .. }));
    }

    #[test]
    fn blank_lines_are_skipped() {
        let dir = temp_corpus(
            r#"{"id":"x","audio":"x.wav","reference":"hi","tags":[],"human":false}

"#,
            &["x.wav"],
        );
        let corpus = Corpus::load(&dir).unwrap();
        assert_eq!(corpus.cases.len(), 1);
    }
}
