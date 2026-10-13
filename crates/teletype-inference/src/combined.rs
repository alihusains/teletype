//! Combined decision provider: TinyDecide for classification, Needle 3
//! for command extraction and embedding.
//!
//! The pipeline calls `classify_intent` (TinyDecide, ~5-12 ms) and, when
//! the intent is Command, `extract_command` (Needle 3, ~20-50 ms).
//! `embed_text` also goes to Needle 3 (~1 ms).

use std::sync::Arc;

use teletype_core::llm::{CommandAction, DecisionProvider, Intent};

/// A decision provider that combines TinyDecide (classifier) with
/// Needle 3 (command extractor + embedder).
pub struct CombinedDecisionProvider {
    classifier: Arc<crate::tinydecide::TinyDecideProvider>,
    extractor: Arc<crate::needle::NeedleProvider>,
}

impl CombinedDecisionProvider {
    pub fn new(
        classifier: crate::tinydecide::TinyDecideProvider,
        extractor: crate::needle::NeedleProvider,
    ) -> Self {
        Self {
            classifier: Arc::new(classifier),
            extractor: Arc::new(extractor),
        }
    }
}

impl DecisionProvider for CombinedDecisionProvider {
    fn model_id(&self) -> &str {
        "tinydecide+needle3"
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
