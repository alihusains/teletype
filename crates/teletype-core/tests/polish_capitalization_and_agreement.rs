// claim: polish-quality
//! Polish must keep Wispr Flow's formatting promise: the model edits words,
//! while sentence casing and unambiguous agreement errors are guaranteed in
//! code. This covers the card-locked cleanup path, which cannot be reprompted.

use teletype_core::llm::{GenerationParams, InferenceProvider};
use teletype_core::transforms::engine::run_transform_blocking;
use teletype_core::transforms::prompt::PromptContext;
use teletype_core::transforms::TransformStore;

struct LowCaseCleanup;

const ANSWER: &str = "hello world. she don't know. i has the answer";

impl InferenceProvider for LowCaseCleanup {
    fn model_id(&self) -> &str {
        "s1-mini"
    }
    fn model_name(&self) -> &str {
        "S1-mini"
    }
    fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
        Ok(ANSWER.to_string())
    }
    fn generate_stream(
        &self,
        _prompt: &str,
        _params: GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(ANSWER.to_string())
    }
    fn generate_with_system_stream(
        &self,
        _system: &str,
        _user: &str,
        _params: GenerationParams,
        _on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        Ok(ANSWER.to_string())
    }
}

#[test]
fn polish_normalizes_sentence_starts_and_agreement() {
    let transform = TransformStore::with_built_ins()
        .get("builtin-polish")
        .unwrap()
        .clone();
    let ctx = PromptContext {
        language: "en".into(),
        polish_max_change_percent: 100,
        ..Default::default()
    };
    let result = run_transform_blocking(&LowCaseCleanup, &transform, ANSWER, &ctx, &mut None);
    assert!(
        result.transformed,
        "cleanup answer must be accepted: {result:?}"
    );
    assert_eq!(
        result.text,
        "Hello world. She doesn't know. I have the answer"
    );
}
