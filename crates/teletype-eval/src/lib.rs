//! Teletype dictation evaluation harness.
//!
//! Purpose: turn "parity with Wispr Flow" from an assertion into a measurement.
//! Three metrics, each independently reportable:
//!
//! 1. **WER** (word error rate) for the speech layer: how often the ASR emits the
//!    wrong words, measured against a reference transcript per utterance.
//! 2. **Polish quality** for the text layer: a deterministic rubric over the
//!    polished output (filler removal, capitalisation, punctuation, no content
//!    loss, no hallucination) plus an optional LLM judge hook.
//! 3. **Latency percentiles**: p50 / p95 for the ASR call and for the full
//!    ASR + polish path, so regressions in perceived speed are caught.
//!
//! Everything pure lives in this crate so it is unit-testable without a
//! microphone, a model, or a network. The audio-facing driver is a separate
//! binary so `cargo test` never needs a 640 MB model on disk.

pub mod corpus;
pub mod latency;
pub mod polish;
pub mod report;
pub mod wer;

pub use corpus::{Case, Corpus, CorpusError};
pub use latency::{LatencySummary, Sample};
pub use polish::{score_polish, PolishVerdict};
pub use report::{EvalReport, ReportError};
pub use wer::{word_error_rate, WerSummary};
