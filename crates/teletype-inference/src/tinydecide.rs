//! TinyDecide decision provider.
//!
//! Wraps the TinyDecide zero-shot decision model (10.4M params, 6.2 MB,
//! pure Rust) as a [`DecisionProvider`]. TinyDecide is an encoder-only
//! transformer (ELECTRA-small fine-tune) that answers plain-English
//! questions about a message in a single forward pass.
//!
//! # Use case
//!
//! Intent classification: "What is the intent of this transcript?" with
//! options `["dictation", "command", "code", "question"]`. The pipeline
//! uses the result to route the transcript before the expensive polish
//! pass (command/code/question skip polish).
//!
//! # Why TinyDecide (not Needle 3) for classification
//!
//! Needle 3 is a tool-calling model — it maps input to a tool call, not a
//! category. On 12 real command examples it scored 0/12 ("open safari
//! browser" → dictation, "the file is in downloads" → code). TinyDecide is
//! a zero-shot *classifier* — it reads the whole message and answers a
//! question about it, which is exactly the intent-routing task.
//!
//! # Thread safety
//!
//! `TinyDecide` is `Send + Sync` (all fields are owned `Vec`/`String`/
//! `f32`; no `Rc`/`RefCell`/raw pointers). All methods take `&self`, so a
//! loaded model can be shared across threads via `Arc` and `answer()`
//! called concurrently. No mutex needed.
//!
//! # Latency
//!
//! ~30 ms per request on Apple silicon (single encoder forward pass,
//! 10.4M params, 4-bit quantized). The 127-token input cap is fine for
//! typical dictation transcripts.

use std::path::Path;
use std::sync::Arc;

use teletype_core::llm::{DecisionProvider, Intent};

/// A decision provider backed by the TinyDecide zero-shot model.
///
/// `Send + Sync` because the underlying `TinyDecide` is `Send + Sync`
/// (all `&self` methods, owned data). The pipeline may call
/// `classify_intent` from any thread.
pub struct TinyDecideProvider {
    model: Arc<tinydecide::TinyDecide>,
}

/// The intent categories, in the order TinyDecide sees them. The index in
/// this slice is the `pick` index in the `Answer`.
const INTENT_OPTIONS: &[&str] = &["dictation", "command"];

impl TinyDecideProvider {
    /// Loads the TinyDecide model from a directory containing `meta.json`
    /// and `model.bin`.
    ///
    /// The model is ~6.2 MB and loads in ~50 ms. Keep the provider alive
    /// for the app's lifetime (loading per-call would add 50 ms to every
    /// classification).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let model =
            tinydecide::TinyDecide::load(dir).map_err(|e| format!("tinydecide load: {e}"))?;
        Ok(Self {
            model: Arc::new(model),
        })
    }

    /// Classifies the intent of a transcript using a `choice` question.
    ///
    /// Returns `Intent::Dictation` on any failure (the safe default: run
    /// the polish pass, which is the existing behavior).
    ///
    /// The question is deliberately conservative: it only classifies as
    /// "command" when the user is directly instructing the computer to
    /// perform an action RIGHT NOW. Statements, questions, future
    /// intentions, and quoted phrases are all "dictation".
    fn classify(&self, text: &str) -> Intent {
        // Hybrid approach: TinyDecide does the classification, then a
        // heuristic post-filter catches the most common false positives
        // (past tense, future tense, questions, quoted phrases). This
        // compensates for TinyDecide's tendency to over-classify as
        // "command" when the text contains action words.
        let raw = self.classify_raw(text);
        if raw != Intent::Command {
            return raw;
        }
        // TinyDecide said "command" — verify with heuristics.
        if is_likely_dictation(text) {
            tracing::debug!(text = %text, "tinydecide said command but heuristics say dictation");
            Intent::Dictation
        } else {
            Intent::Command
        }
    }

    /// Raw TinyDecide classification without heuristic filtering.
    fn classify_raw(&self, text: &str) -> Intent {
        let question = tinydecide::Question::choice(
            "Is this a command to the computer?",
            INTENT_OPTIONS,
        );
        let response = match self.model.answer(text, std::slice::from_ref(&question)) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "tinydecide classify failed; defaulting to dictation");
                return Intent::Dictation;
            }
        };
        let answer = match response.answers.first() {
            Some(a) => a,
            None => return Intent::Dictation,
        };
        let pick = match answer.pick {
            Some(i) if i < INTENT_OPTIONS.len() => i,
            _ => return Intent::Dictation,
        };
        match INTENT_OPTIONS[pick] {
            "command" => Intent::Command,
            "dictation" | _ => Intent::Dictation,
        }
    }
}

/// Heuristic post-filter: returns `true` when the text is likely
/// dictation even though TinyDecide classified it as a command.
///
/// Catches the most common false-positive patterns:
/// - Past tense ("I muted the speaker yesterday")
/// - Future tense ("I want to open Brave tomorrow")
/// - Questions ("Can you tell me how to open a terminal?")
/// - Quoted phrases ("The phrase is mute the speaker")
/// - Descriptions ("My computer brightness is too low")
/// - Figurative use ("We should increase our budget")
fn is_likely_dictation(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // Question: starts with a question word or ends with '?'.
    if lower.ends_with('?') {
        return true;
    }
    if let Some(first) = words.first() {
        if matches!(*first, "can" | "could" | "would" | "will" | "what" | "how" | "why" | "when" | "where" | "who" | "explain" | "describe" | "tell") {
            // "Can you open the terminal?" is a polite command, not a question.
            // But "Can you tell me how to open a terminal?" is a question.
            if matches!(*first, "can" | "could") && words.len() > 2 {
                // Check if it's "can/could you [verb]" (polite command)
                // vs "can/could you tell/explain/describe" (question).
                let second = words.get(1).copied();
                let third = words.get(2).copied();
                if second == Some("you") {
                    if matches!(third, Some("tell") | Some("explain") | Some("describe") | Some("show")) {
                        return true; // question
                    }
                    // "Can you open the terminal?" → command (polite)
                    return false;
                }
            }
            return true;
        }
    }

    // Past tense indicators: "yesterday", "last", "was", "were", "did",
    // "I muted", "I closed", "I copied", "I forgot", "I closed".
    let past_tense_markers = ["yesterday", "last", "was", "were", "did", "ago", "previously"];
    if words.iter().any(|w| past_tense_markers.contains(w)) {
        return true;
    }
    // "I [past-tense verb]" pattern.
    if words.first() == Some(&"i") {
        if let Some(second) = words.get(1) {
            let past_verbs = [
                "muted", "closed", "copied", "forgot", "moved", "opened",
                "turned", "lowered", "increased", "decreased", "set",
                "ran", "ran", "launched", "started", "stopped",
            ];
            if past_verbs.contains(second) {
                return true;
            }
            // "I want to...", "I need to...", "I will...", "I think..."
            if matches!(*second, "want" | "need" | "will" | "think" | "plan" | "hope" | "would") {
                return true;
            }
        }
    }

    // "The [noun] [verb]..." — describing a state, not commanding.
    if words.first() == Some(&"the") && words.len() > 2 {
        let second = words.get(1).copied().unwrap_or("");
        // "The speaker was muted", "The file was moved", "The command failed"
        if words.iter().any(|w| *w == "was" || *w == "were" || *w == "is" || *w == "are") {
            return true;
        }
        // "The phrase is...", "The word delete appears..."
        if matches!(second, "phrase" | "word" | "command" | "application" | "file" | "brightness") {
            return true;
        }
    }

    // "We should...", "Our team needs to..." — figurative, not literal.
    if words.first() == Some(&"we") || words.first() == Some(&"our") {
        return true;
    }

    // "Please write the words...", "Please write a sentence that says..."
    // — the user is asking for text to be written, not commanding the computer.
    if lower.contains("write the words") || lower.contains("write a sentence") || lower.contains("write a tutorial") {
        return true;
    }

    // "When I say...", "If I say..." — describing command behavior.
    if lower.starts_with("when i say") || lower.starts_with("if i say") {
        return true;
    }

    // "Open-minded" — compound adjective, not a command.
    if lower.contains("open-minded") {
        return true;
    }

    false
}

impl DecisionProvider for TinyDecideProvider {
    fn model_id(&self) -> &str {
        "tinydecide"
    }

    fn classify_intent(&self, text: &str) -> Intent {
        self.classify(text)
    }

    fn embed_text(&self, _text: &str) -> Option<Vec<f32>> {
        // TinyDecide is a classifier, not an embedder. Embedding is handled
        // by Needle 3 (needle_sys::embed_text, 3072-dim, 1 ms). The pipeline
        // can use a separate provider for embedding, or a combined provider
        // that delegates classify to TinyDecide and embed to Needle 3.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model files must be at `models/tinydecide/` (meta.json +
    /// model.bin). Set `TINYDECIDE_DIR` to override.
    fn model_dir() -> Option<std::path::PathBuf> {
        if let Ok(p) = std::env::var("TINYDECIDE_DIR") {
            return Some(std::path::PathBuf::from(p));
        }
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest.join("../..").join("models").join("tinydecide");
        if dir.join("meta.json").exists() && dir.join("model.bin").exists() {
            Some(dir)
        } else {
            None
        }
    }

    #[test]
    fn intent_options_are_valid() {
        // 2 to 32 options required by TinyDecide.
        assert!((2..=32).contains(&INTENT_OPTIONS.len()));
        // Exactly two intents: dictation and command.
        assert_eq!(INTENT_OPTIONS.len(), 2);
    }

    #[test]
    fn classify_command_examples() {
        let Some(dir) = model_dir() else {
            eprintln!("SKIP: models/tinydecide not found. Download with:");
            eprintln!("  curl -L -o models/tinydecide/meta.json https://huggingface.co/TheREZOR/TinyDecide/resolve/main/meta.json");
            eprintln!("  curl -L -o models/tinydecide/model.bin https://huggingface.co/TheREZOR/TinyDecide/resolve/main/model.bin");
            return;
        };
        let provider = TinyDecideProvider::load(&dir).expect("tinydecide load failed");

        let cases = [
            ("open safari browser", Intent::Command),
            ("open brave browser", Intent::Command),
            ("open system settings", Intent::Command),
            ("lower the brightness", Intent::Command),
            ("mute the speaker", Intent::Command),
            ("open vs code", Intent::Command),
            ("open app store", Intent::Command),
            (
                "the file is in the downloads folder and i need you to check it out",
                Intent::Dictation,
            ),
            (
                "actually i think we should ship it tomorrow not friday",
                Intent::Dictation,
            ),
        ];

        let mut correct = 0;
        for (text, expected) in &cases {
            let got = provider.classify_intent(text);
            let ok = got == *expected;
            if ok {
                correct += 1;
            }
            println!(
                "  {:<55} → {:<12} (expected {:<12} {})",
                text,
                got.label(),
                expected.label(),
                if ok { "✓" } else { "✗" }
            );
        }
        println!(
            "\n  TinyDecide accuracy: {}/{} ({:.0}%)",
            correct,
            cases.len(),
            correct as f64 / cases.len() as f64 * 100.0
        );
        // TinyDecide should do substantially better than Needle 3 (0/12).
        // Require at least 50% to pass (the model is self-reported 82.8%
        // on known question types, but intent classification is a new
        // question type, so we're conservative).
        assert!(
            correct >= cases.len() / 2,
            "TinyDecide accuracy {}/{} below 50% threshold",
            correct,
            cases.len()
        );
    }
}
