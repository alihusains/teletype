//! The transform engine: model-agnostic orchestration of one transform.
//!
//! The engine takes the *protected* input (AutoText already placeholdered) and
//! returns either transformed text (still placeholdered) or a fallback to the
//! input. AutoText restoration happens in the pipeline, not here.

use super::prompt::{build_prompt, PromptContext};
use super::validator::{self, Failure, ValidatedOutput};
use super::TransformDefinition;

/// What a transform run costs/produced, for observability.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransformMetrics {
    pub latency_ms: u128,
    pub fell_back: bool,
    pub failure: Option<Failure>,
}

/// The result of running the pipeline's transform stage.
#[derive(Debug, Clone)]
pub struct TransformResult {
    /// Transformed text with placeholders intact, or the original input.
    pub text: String,
    /// True when the model output was used; false when we fell back.
    pub transformed: bool,
    pub metrics: TransformMetrics,
}

/// Runs one transform through a provider. Model-agnostic and synchronous in
/// its contract: the caller decides threading.
pub fn run_transform_blocking(
    provider: &dyn crate::llm::InferenceProvider,
    transform: &TransformDefinition,
    protected_input: &str,
    ctx: &PromptContext,
) -> TransformResult {
    let started = std::time::Instant::now();
    let prompt = build_prompt(transform, protected_input, ctx);
    let params = crate::llm::GenerationParams::default();

    let outcome = match provider.generate(&prompt, params) {
        Ok(raw) => match validator::validate(&raw, protected_input, transform, &prompt) {
            ValidatedOutput::Transformed(text) => TransformResult {
                text,
                transformed: true,
                metrics: TransformMetrics {
                    latency_ms: started.elapsed().as_millis(),
                    ..Default::default()
                },
            },
            ValidatedOutput::Fallback(orig, failure) => TransformResult {
                text: orig,
                transformed: false,
                metrics: TransformMetrics {
                    latency_ms: started.elapsed().as_millis(),
                    fell_back: true,
                    failure,
                },
            },
        },
        Err(e) => {
            tracing::warn!(error = %e, "transform inference failed; falling back to input");
            TransformResult {
                text: protected_input.to_string(),
                transformed: false,
                metrics: TransformMetrics {
                    latency_ms: started.elapsed().as_millis(),
                    fell_back: true,
                    failure: None,
                },
            }
        }
    };
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transforms::{TransformDefinition, TransformStore};

    fn polish() -> TransformDefinition {
        TransformStore::with_built_ins()
            .get("builtin-polish")
            .unwrap()
            .clone()
    }

    struct Scripted(Option<String>);
    impl crate::llm::InferenceProvider for Scripted {
        fn model_id(&self) -> &str {
            "mock"
        }
        fn model_name(&self) -> &str {
            "mock"
        }
        fn generate(
            &self,
            _prompt: &str,
            _params: crate::llm::GenerationParams,
        ) -> Result<String, String> {
            match &self.0 {
                Some(out) => Ok(out.clone()),
                None => Err("model crashed".into()),
            }
        }
    }

    fn scripted(out: Option<&str>) -> Scripted {
        Scripted(out.map(str::to_string))
    }

    #[test]
    fn clean_output_is_used() {
        let input = "hey john can you send the proposal";
        let result = run_transform_blocking(
            &scripted(Some("Hi John, can you send the proposal?")),
            &polish(),
            input,
            &PromptContext::default(),
        );
        assert!(result.transformed);
        assert_eq!(result.text, "Hi John, can you send the proposal?");
    }

    #[test]
    fn inference_error_falls_back_to_input() {
        let input = "my text here";
        let result =
            run_transform_blocking(&scripted(None), &polish(), input, &PromptContext::default());
        assert!(!result.transformed);
        assert_eq!(result.text, input);
        assert!(result.metrics.fell_back);
    }

    #[test]
    fn rejected_output_falls_back_to_input() {
        let input = "send the report to finance";
        let result = run_transform_blocking(
            &scripted(Some(
                "Here is the polished version: Send the report to finance.",
            )),
            &polish(),
            input,
            &PromptContext::default(),
        );
        assert!(!result.transformed);
        assert_eq!(result.text, input);
    }

    #[test]
    fn placeholder_survival_is_enforced() {
        let input = "send to {{AUTOTEXT_0}} ok";
        let result = run_transform_blocking(
            &scripted(Some("Send it to the address ok")),
            &polish(),
            input,
            &PromptContext::default(),
        );
        assert!(!result.transformed, "dropped placeholder must fall back");
        assert_eq!(result.text, input);
    }
}
