//! Combined decision provider: TinyDecide for classification, Needle 3
//! (or the P2.7 LLM fallback) for command extraction and embedding.
//!
//! The pipeline calls `classify_intent` (TinyDecide, ~5-12 ms) and, when
//! the intent is Command, `extract_command` (Needle 3, ~20-50 ms; the LLM
//! fallback, one local generation). `embed_text` also goes to the
//! extractor — Needle 3 (~1 ms) when installed, `None` otherwise.

use std::sync::Arc;

use teletype_core::llm::{CommandAction, DecisionProvider, Intent};

/// The extraction half of a [`CombinedDecisionProvider`]: Needle 3 when its
/// model is installed, or the local polish LLM as the P2.7 fallback.
///
/// Kept apart from [`DecisionProvider`] so the classifier stays the fast,
/// concrete TinyDecide model and the extractor can be swapped without
/// touching classification at all.
pub trait CommandExtractor: Send + Sync {
    /// One tool call for `text`, or `None` when nothing matched — including
    /// every failure path (model missing, generation error, unparseable
    /// reply, tool outside the allowlist).
    fn extract_command(&self, text: &str) -> Option<CommandAction>;

    /// Embedding is Needle's job; extractors that cannot embed return
    /// `None` and the pipeline simply skips similarity matching.
    fn embed_text(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }

    /// Stable id surfaced in status/logging, e.g. `"needle3"`.
    fn model_id(&self) -> &str;
}

/// Needle 3 as the extractor: the original combined behavior.
impl CommandExtractor for crate::needle::NeedleProvider {
    fn extract_command(&self, text: &str) -> Option<CommandAction> {
        DecisionProvider::extract_command(self, text)
    }

    fn embed_text(&self, text: &str) -> Option<Vec<f32>> {
        DecisionProvider::embed_text(self, text)
    }

    fn model_id(&self) -> &str {
        DecisionProvider::model_id(self)
    }
}

/// A decision provider that combines a classifier (TinyDecide) with a
/// command extractor (Needle 3, or the P2.7 LLM fallback).
pub struct CombinedDecisionProvider {
    classifier: Arc<dyn DecisionProvider>,
    extractor: Arc<dyn CommandExtractor>,
    model_id: String,
}

impl CombinedDecisionProvider {
    /// TinyDecide + Needle 3 — the fast path when both models are installed.
    pub fn new(
        classifier: crate::tinydecide::TinyDecideProvider,
        extractor: crate::needle::NeedleProvider,
    ) -> Self {
        Self::with_extractor(classifier, Arc::new(extractor))
    }

    /// TinyDecide + any extractor. P2.7 uses this to pair the classifier
    /// with the LLM fallback when Needle 3 is not installed, so command
    /// takes still extract instead of degrading to raw dictation.
    pub fn with_extractor(
        classifier: crate::tinydecide::TinyDecideProvider,
        extractor: Arc<dyn CommandExtractor>,
    ) -> Self {
        let model_id = format!("tinydecide+{}", extractor.model_id());
        Self::with_parts(Arc::new(classifier), extractor, model_id)
    }

    /// Any classifier + any extractor. Used by the tests, where neither
    /// concrete model can be loaded.
    pub fn with_parts(
        classifier: Arc<dyn DecisionProvider>,
        extractor: Arc<dyn CommandExtractor>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            classifier,
            extractor,
            model_id: model_id.into(),
        }
    }
}

impl DecisionProvider for CombinedDecisionProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn classify_intent(&self, text: &str) -> Intent {
        self.classifier.classify_intent(text)
    }

    fn embed_text(&self, text: &str) -> Option<Vec<f32>> {
        self.extractor.embed_text(text)
    }

    fn extract_command(&self, text: &str) -> Option<CommandAction> {
        self.extractor.extract_command(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockClassifier {
        intent: Intent,
    }

    impl DecisionProvider for MockClassifier {
        fn model_id(&self) -> &str {
            "mock-classifier"
        }

        fn classify_intent(&self, _text: &str) -> Intent {
            self.intent
        }

        fn embed_text(&self, _text: &str) -> Option<Vec<f32>> {
            None
        }
    }

    struct MockExtractor;

    impl CommandExtractor for MockExtractor {
        fn extract_command(&self, _text: &str) -> Option<CommandAction> {
            Some(CommandAction {
                tool: "open_app".into(),
                arguments: serde_json::json!({"app": "safari"}),
            })
        }

        fn embed_text(&self, _text: &str) -> Option<Vec<f32>> {
            Some(vec![1.0, 0.0])
        }

        fn model_id(&self) -> &str {
            "mock-extractor"
        }
    }

    #[test]
    fn delegates_each_half_to_its_owner() {
        let combined = CombinedDecisionProvider::with_parts(
            Arc::new(MockClassifier {
                intent: Intent::Command,
            }),
            Arc::new(MockExtractor),
            "mock-classifier+mock-extractor",
        );
        assert_eq!(combined.classify_intent("open safari"), Intent::Command);
        assert_eq!(combined.embed_text("x"), Some(vec![1.0, 0.0]));
        assert_eq!(
            combined.extract_command("open safari").unwrap().tool,
            "open_app"
        );
        assert_eq!(combined.model_id(), "mock-classifier+mock-extractor");
    }
}
